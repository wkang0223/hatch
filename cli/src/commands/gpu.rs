//! `nm gpu` subcommands.

use crate::client::ClientContext;
use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use serde::Deserialize;

#[derive(Subcommand)]
pub enum GpuCmd {
    /// Browse available GPUs on the network
    List {
        /// Filter by minimum unified memory (GB)
        #[arg(long)]
        min_ram: Option<u32>,

        /// Filter by required runtime (mlx, torch-mps, onnx-coreml, llama-cpp)
        #[arg(long)]
        runtime: Option<String>,

        /// Filter by maximum price (NMC/hr)
        #[arg(long)]
        max_price: Option<f64>,

        /// Sort by: price, ram, trust, latency (default: price)
        #[arg(long, default_value = "price")]
        sort: String,

        /// Number of results to show
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Benchmark the local machine's ML runtimes
    Benchmark {
        /// Runtime to benchmark (mlx, torch-mps, onnx-coreml, all)
        #[arg(long, default_value = "all")]
        runtime: String,
    },
}

// ─── API response types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ProviderInfo {
    id: String,
    chip_model: String,
    unified_memory_gb: u32,
    gpu_cores: u32,
    installed_runtimes: Vec<String>,
    floor_price_nmc_per_hour: f64,
    trust_score: f64,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    bandwidth_mbps: Option<u32>,
    #[serde(default)]
    max_job_ram_gb: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ProviderListResponse {
    providers: Vec<ProviderInfo>,
    total: u32,
}

// ─── Command handlers ────────────────────────────────────────────────────────

pub async fn run(cmd: GpuCmd, ctx: &ClientContext) -> Result<()> {
    match cmd {
        GpuCmd::List { min_ram, runtime, max_price, sort, limit } => {
            list_gpus(ctx, min_ram, runtime, max_price, &sort, limit).await
        }
        GpuCmd::Benchmark { runtime } => benchmark_local(&runtime).await,
    }
}

async fn list_gpus(
    ctx: &ClientContext,
    min_ram: Option<u32>,
    runtime: Option<String>,
    max_price: Option<f64>,
    sort: &str,
    limit: u32,
) -> Result<()> {
    let mut params = vec![format!("limit={}", limit), format!("sort={}", sort)];
    if let Some(r) = &min_ram    { params.push(format!("min_ram_gb={}", r)); }
    if let Some(rt) = &runtime   { params.push(format!("runtime={}", rt)); }
    if let Some(p) = &max_price  { params.push(format!("max_price={}", p)); }

    let url = ctx.coordinator_url(&format!("/api/v1/providers?{}", params.join("&")));

    let resp = ctx
        .http()
        .get(&url)
        .send()
        .await
        .context("Failed to fetch provider list")?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to list GPUs: {}", err);
    }

    let result: ProviderListResponse = resp.json().await.context("Invalid provider list response")?;

    if result.providers.is_empty() {
        println!("No available providers match your criteria.");
        println!("Try relaxing filters or check back later.");
        return Ok(());
    }

    if ctx.output_json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "providers": result.providers.iter().map(|p| serde_json::json!({
                "id": p.id,
                "chip": p.chip_model,
                "memory_gb": p.unified_memory_gb,
                "gpu_cores": p.gpu_cores,
                "runtimes": p.installed_runtimes,
                "price_nmc_hr": p.floor_price_nmc_per_hour,
                "trust": p.trust_score,
                "region": p.region,
            })).collect::<Vec<_>>(),
            "total": result.total,
        }))?);
        return Ok(());
    }

    println!("{}", "Available Mac Providers".bold().cyan());
    if let Some(r) = min_ram    { print!("  RAM ≥ {}GB", r); }
    if let Some(rt) = &runtime  { print!("  Runtime: {}", rt.yellow()); }
    if let Some(p) = max_price  { print!("  Price ≤ {:.3} NMC/hr", p); }
    println!();
    println!("─────────────────────────────────────────────────────────────────────────────────────");
    println!("{:<38} {:<18} {:>7} {:>6} {:>10} {:>6} {}",
        "PROVIDER ID", "CHIP", "RAM GB", "GPU", "NMC/HR", "TRUST", "RUNTIMES");
    println!("─────────────────────────────────────────────────────────────────────────────────────");

    for p in &result.providers {
        let trust_stars = trust_stars(p.trust_score);
        let runtimes_short: Vec<&str> = p.installed_runtimes.iter()
            .map(|r| match r.as_str() {
                "mlx"          => "mlx",
                "torch-mps"    => "mps",
                "onnx-coreml"  => "onnx",
                "llama-cpp"    => "llama",
                other          => other,
            })
            .collect();

        println!("{:<38} {:<18} {:>7} {:>6} {:>10} {:>6} {}",
            p.id[..p.id.len().min(36)].cyan(),
            p.chip_model.chars().take(16).collect::<String>().yellow(),
            p.unified_memory_gb,
            p.gpu_cores,
            format!("{:.4}", p.floor_price_nmc_per_hour).green(),
            trust_stars,
            runtimes_short.join("+"),
        );
    }

    println!("─────────────────────────────────────────────────────────────────────────────────────");
    println!("  Showing {} of {} available providers. Sorted by {}.",
        result.providers.len(), result.total, sort);
    println!("\n  Submit a job:  {}", "nm job submit --runtime mlx --ram 48 ./inference.py".cyan());

    Ok(())
}

async fn benchmark_local(runtime: &str) -> Result<()> {
    println!("{}", "NeuralMesh Local Benchmark".bold().cyan());
    println!("Detecting hardware...\n");

    // Detect chip via sysctl
    let chip = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "Unknown".to_string());

    println!("  Chip:    {}", chip.yellow());

    // Total RAM via sysctl hw.memsize
    let ram_bytes = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
        .unwrap_or(0);
    println!("  Memory:  {} GB unified", ram_bytes / 1_073_741_824);

    println!();

    let runtimes_to_test: Vec<&str> = if runtime == "all" {
        vec!["mlx", "torch-mps", "onnx-coreml"]
    } else {
        vec![runtime]
    };

    for rt in &runtimes_to_test {
        benchmark_runtime(rt)?;
    }

    println!("\n{}", "Benchmark complete.".bold().green());
    println!("Run {} to start offering your GPU to the network.", "`nm provider start`".cyan());

    Ok(())
}

fn benchmark_runtime(runtime: &str) -> Result<()> {
    println!("  {} {}...", "Benchmarking".bold(), runtime.yellow());

    let (check_cmd, check_args) = match runtime {
        "mlx"         => ("python3", vec!["-c", "import mlx.core as mx; print(mx.default_device())"]),
        "torch-mps"   => ("python3", vec!["-c", "import torch; print(torch.backends.mps.is_available())"]),
        "onnx-coreml" => ("python3", vec!["-c", "import onnxruntime as ort; print(ort.get_available_providers())"]),
        _             => {
            println!("    {} Unknown runtime: {}", "⚠".yellow(), runtime);
            return Ok(());
        }
    };

    let start = std::time::Instant::now();
    let result = std::process::Command::new(check_cmd)
        .args(&check_args)
        .output();

    match result {
        Ok(out) if out.status.success() => {
            let elapsed = start.elapsed();
            let output = String::from_utf8_lossy(&out.stdout).trim().to_string();
            println!("    {} Available ({:.1}ms import) — {}", "✓".green(), elapsed.as_millis(), output);

            // Run a quick compute benchmark
            let bench_script = match runtime {
                "mlx" => Some(MLX_BENCH),
                "torch-mps" => Some(TORCH_MPS_BENCH),
                _ => None,
            };

            if let Some(script) = bench_script {
                let bench = std::process::Command::new("python3")
                    .args(["-c", script])
                    .output();
                if let Ok(b) = bench {
                    if b.status.success() {
                        let bres = String::from_utf8_lossy(&b.stdout).trim().to_string();
                        println!("    {} {}", "→".blue(), bres);
                    }
                }
            }
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            println!("    {} Not available: {}", "✗".red(), if err.is_empty() { "runtime not installed" } else { &err });
        }
        Err(e) => {
            println!("    {} Failed to run check: {}", "✗".red(), e);
        }
    }

    Ok(())
}

fn trust_stars(score: f64) -> String {
    let full = (score.round() as usize).min(5);
    let empty = 5 - full;
    format!("{}{}", "★".repeat(full), "☆".repeat(empty))
}

// ─── Inline benchmark scripts ─────────────────────────────────────────────────

const MLX_BENCH: &str = r#"
import mlx.core as mx
import time
N = 2048
a = mx.random.uniform(shape=(N, N))
b = mx.random.uniform(shape=(N, N))
mx.eval(a)
mx.eval(b)
t0 = time.perf_counter()
c = mx.matmul(a, b)
mx.eval(c)
dt = time.perf_counter() - t0
flops = 2 * N * N * N / dt / 1e12
print(f"MLX matmul {N}x{N}: {dt*1000:.1f}ms  ({flops:.2f} TFLOPS)")
"#;

const TORCH_MPS_BENCH: &str = r#"
import torch
import time
if not torch.backends.mps.is_available():
    print("MPS not available")
else:
    N = 2048
    a = torch.rand(N, N, device='mps')
    b = torch.rand(N, N, device='mps')
    torch.mps.synchronize()
    t0 = time.perf_counter()
    c = torch.matmul(a, b)
    torch.mps.synchronize()
    dt = time.perf_counter() - t0
    flops = 2 * N * N * N / dt / 1e12
    print(f"PyTorch MPS matmul {N}x{N}: {dt*1000:.1f}ms  ({flops:.2f} TFLOPS)")
"#;
