//! `hatch provider` subcommands.

use crate::client::ClientContext;
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::process::Command;

#[derive(Subcommand)]
pub enum ProviderCmd {
    /// Install ML runtimes and configure this Mac as a provider
    Install {
        /// Runtimes to install (comma-separated: mlx,torch-mps,onnx-coreml,llama-cpp)
        #[arg(long, default_value = "mlx,torch-mps,onnx-coreml")]
        runtimes: String,
    },
    /// Start the neuralmesh-agent daemon
    Start,
    /// Stop the neuralmesh-agent daemon
    Stop,
    /// Show provider status (GPU state, active jobs, earnings)
    Status,
    /// Configure provider settings
    Config {
        #[arg(long)] idle_threshold: Option<f32>,
        #[arg(long)] idle_minutes: Option<u32>,
        #[arg(long)] floor_price: Option<f64>,
        #[arg(long)] max_job_ram: Option<u32>,
    },
}

pub async fn run(cmd: ProviderCmd, ctx: &ClientContext) -> Result<()> {
    match cmd {
        ProviderCmd::Install { runtimes } => install(runtimes).await,
        ProviderCmd::Start  => start_daemon().await,
        ProviderCmd::Stop   => stop_daemon().await,
        ProviderCmd::Status => show_status(ctx).await,
        ProviderCmd::Config { idle_threshold, idle_minutes, floor_price, max_job_ram } => {
            configure(idle_threshold, idle_minutes, floor_price, max_job_ram).await
        }
    }
}

async fn install(runtimes: String) -> Result<()> {
    println!("{}", "Hatch Provider Setup".bold().cyan());
    println!("Installing ML runtimes: {}", runtimes.yellow());
    println!();

    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner()
        .template("{spinner:.green} {msg}")
        .unwrap());

    // Detect chip
    pb.set_message("Detecting Apple Silicon chip...");
    let chip_out = Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]).output()?;
    let chip = String::from_utf8_lossy(&chip_out.stdout).trim().to_string();
    pb.finish_with_message(format!("Detected: {}", chip.green()));

    // Create neuralmesh_worker user
    println!("\n{}", "Creating isolated worker user...".bold());
    let user_exists = Command::new("id").arg("neuralmesh_worker").status()?.success();
    if !user_exists {
        let status = Command::new("sudo")
            .args(["dscl", ".", "-create", "/Users/neuralmesh_worker"])
            .status()?;
        if status.success() {
            Command::new("sudo")
                .args(["dscl", ".", "-create", "/Users/neuralmesh_worker", "UserShell", "/usr/bin/false"])
                .status()?;
            println!("  {} Created neuralmesh_worker user", "✓".green());
        }
    } else {
        println!("  {} neuralmesh_worker user already exists", "✓".green());
    }

    // Install each runtime
    for runtime in runtimes.split(',') {
        let runtime = runtime.trim();
        println!("\n{} {}...", "Installing".bold(), runtime.yellow());

        match runtime {
            "mlx" => {
                install_pip_package("mlx")?;
                install_pip_package("mlx-lm")?;
                println!("  {} MLX installed", "✓".green());
            }
            "torch-mps" => {
                install_pip_package("torch torchvision torchaudio")?;
                println!("  {} PyTorch (MPS) installed", "✓".green());
            }
            "onnx-coreml" => {
                install_pip_package("onnxruntime")?;
                println!("  {} ONNX Runtime (CoreML EP) installed", "✓".green());
            }
            "llama-cpp" => {
                println!("  Installing llama-cpp-python with Metal support...");
                let status = Command::new("pip3")
                    .args(["install", "llama-cpp-python"])
                    .env("CMAKE_ARGS", "-DGGML_METAL=on")
                    .env("FORCE_CMAKE", "1")
                    .status()?;
                if status.success() {
                    println!("  {} llama-cpp-python (Metal) installed", "✓".green());
                } else {
                    println!("  {} llama-cpp-python install failed — skipping", "⚠".yellow());
                }
            }
            _ => println!("  {} Unknown runtime: {} — skipping", "⚠".yellow(), runtime),
        }
    }

    // Create /tmp/neuralmesh directory
    std::fs::create_dir_all("/tmp/neuralmesh")?;
    println!("\n{} Working directory created: /tmp/neuralmesh", "✓".green());

    println!("\n{}", "Setup complete!".bold().green());
    println!("Run {} to start offering your GPU to the network.", "`nm provider start`".cyan());

    Ok(())
}

fn install_pip_package(packages: &str) -> Result<()> {
    let args: Vec<&str> = std::iter::once("install")
        .chain(packages.split_whitespace())
        .collect();
    let status = Command::new("pip3").args(&args).status()?;
    if !status.success() {
        anyhow::bail!("pip3 install {} failed", packages);
    }
    Ok(())
}

async fn start_daemon() -> Result<()> {
    // Find neuralmesh-agent binary
    let agent_bin = find_agent_binary()?;
    let config_path = dirs::config_dir()
        .unwrap_or_default()
        .join("neuralmesh/agent.toml");

    println!("Starting neuralmesh-agent...");
    let status = Command::new("sudo")
        .args([
            &agent_bin,
            "service", "install",
            "--binary", &agent_bin,
            "--config", config_path.to_str().unwrap_or(""),
        ])
        .status()?;

    if status.success() {
        println!("{} neuralmesh-agent started", "✓".green());
        println!("Your Mac will start offering idle GPU time to the network.");
    } else {
        anyhow::bail!("Failed to start agent daemon");
    }
    Ok(())
}

async fn stop_daemon() -> Result<()> {
    Command::new("sudo")
        .args(["launchctl", "unload", "-w", "/Library/LaunchDaemons/io.neuralmesh.agent.plist"])
        .status()?;
    println!("{} neuralmesh-agent stopped", "✓".green());
    Ok(())
}

async fn show_status(ctx: &ClientContext) -> Result<()> {
    // Check launchd service status
    let running = Command::new("launchctl")
        .args(["list", "io.neuralmesh.agent"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    println!("{}", "Provider Status".bold().cyan());
    println!("─────────────────────────────────────────");

    let status_str = if running { "● Running".green().to_string() } else { "○ Stopped".red().to_string() };
    println!("  Agent:        {}", status_str);

    // Try to get provider info from coordinator
    if running {
        match ctx.http().get(ctx.coordinator_url("/api/v1/stats")).send().await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    println!("  Network:      {} available providers", json["available_providers"].as_i64().unwrap_or(0));
                    println!("  Network RAM:  {} GB available across network", json["total_available_ram_gb"].as_i64().unwrap_or(0));
                }
            }
            Err(_) => println!("  Network:      (coordinator unreachable)"),
        }
    }

    // Detect local chip
    if let Ok(chip) = Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]).output() {
        let chip_str = String::from_utf8_lossy(&chip.stdout).trim().to_string();
        println!("  Chip:         {}", chip_str.yellow());
    }

    Ok(())
}

async fn configure(
    idle_threshold: Option<f32>,
    idle_minutes: Option<u32>,
    floor_price: Option<f64>,
    max_job_ram: Option<u32>,
) -> Result<()> {
    let cfg_path = dirs::config_dir()
        .unwrap_or_default()
        .join("neuralmesh/agent.toml");

    let content = if cfg_path.exists() {
        std::fs::read_to_string(&cfg_path)?
    } else {
        String::new()
    };

    let mut cfg: nm_common::config::AgentConfig = if content.is_empty() {
        nm_common::config::AgentConfig::default()
    } else {
        toml::from_str(&content).unwrap_or_default()
    };

    if let Some(t) = idle_threshold { cfg.idle_threshold_pct = t; }
    if let Some(m) = idle_minutes   { cfg.idle_duration_minutes = m; }
    if let Some(p) = floor_price    { cfg.floor_price_nmc_per_hour = p; }
    if let Some(r) = max_job_ram    { cfg.max_job_ram_gb = Some(r); }

    std::fs::create_dir_all(cfg_path.parent().unwrap())?;
    std::fs::write(&cfg_path, toml::to_string_pretty(&cfg)?)?;
    println!("{} Provider config updated", "✓".green());

    Ok(())
}

fn find_agent_binary() -> Result<String> {
    // Look for neuralmesh-agent in PATH or next to the nm binary
    if let Ok(out) = Command::new("which").arg("neuralmesh-agent").output() {
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }
    }
    // Try same directory as current binary
    let cur = std::env::current_exe()?;
    let sibling = cur.parent().unwrap().join("neuralmesh-agent");
    if sibling.exists() {
        return Ok(sibling.to_string_lossy().to_string());
    }
    anyhow::bail!("hatch-agent binary not found. Reinstall Hatch.")
}
