"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import {
  Cpu, ChevronRight, ChevronLeft, CheckCircle, Terminal,
  Copy, Zap, DollarSign, ShieldCheck, Globe, AlertCircle,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { toast } from "sonner";

// ── Types ─────────────────────────────────────────────────────────────────────

type OS = "macos" | "linux-cuda" | "linux-rocm" | null;

// ── Steps ─────────────────────────────────────────────────────────────────────

const STEPS = [
  { id: "platform", label: "Platform" },
  { id: "install",  label: "Install agent" },
  { id: "configure", label: "Configure" },
  { id: "register", label: "Register" },
  { id: "done",     label: "Done" },
];

// ── Code block ────────────────────────────────────────────────────────────────

function CodeBlock({ code, label }: { code: string; label?: string }) {
  function copy() {
    navigator.clipboard.writeText(code);
    toast.success("Copied to clipboard");
  }
  return (
    <div className="group relative">
      {label && <p className="text-xs text-slate-500 mb-1.5">{label}</p>}
      <div className="bg-black rounded-lg p-4 font-mono text-xs text-green-400 leading-relaxed whitespace-pre overflow-x-auto border border-slate-800">
        {code}
      </div>
      <button
        onClick={copy}
        className="absolute top-2 right-2 p-1.5 rounded-md bg-slate-800/80 text-slate-500 hover:text-white opacity-0 group-hover:opacity-100 transition-opacity"
        title="Copy"
      >
        <Copy className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

// ── Step: Platform ────────────────────────────────────────────────────────────

function StepPlatform({ os, setOs }: { os: OS; setOs: (o: OS) => void }) {
  const options: { id: OS; icon: React.ElementType; label: string; sub: string; color: string }[] = [
    {
      id: "macos",
      icon: Cpu,
      label: "Apple Silicon Mac",
      sub: "Mac Mini M1–M4, Mac Studio, MacBook Pro",
      color: "text-brand-400",
    },
    {
      id: "linux-cuda",
      icon: Zap,
      label: "Linux · NVIDIA GPU",
      sub: "RTX 3xxx–5xxx, A100, H100 · CUDA 12.8+",
      color: "text-green-400",
    },
    {
      id: "linux-rocm",
      icon: Globe,
      label: "Linux · AMD GPU",
      sub: "Ryzen AI Max+ 395, RX 7xxx · ROCm 6.2+",
      color: "text-red-400",
    },
  ];

  return (
    <div className="space-y-3">
      <p className="text-slate-400 text-sm mb-4">
        What hardware will you be leasing?
      </p>
      {options.map((o) => (
        <button
          key={o.id}
          onClick={() => setOs(o.id)}
          className={cn(
            "w-full flex items-center gap-4 p-4 rounded-xl border text-left transition-all",
            os === o.id
              ? "border-brand-400/50 bg-brand-400/5"
              : "border-slate-700 bg-slate-900/50 hover:border-slate-600"
          )}
        >
          <div className={cn("h-10 w-10 rounded-lg bg-slate-800 flex items-center justify-center flex-shrink-0", o.color)}>
            <o.icon className="h-5 w-5" />
          </div>
          <div className="flex-1">
            <div className={cn("font-medium text-sm", os === o.id ? "text-white" : "text-slate-300")}>
              {o.label}
            </div>
            <div className="text-xs text-slate-500 mt-0.5">{o.sub}</div>
          </div>
          {os === o.id && <CheckCircle className="h-5 w-5 text-brand-400 flex-shrink-0" />}
        </button>
      ))}
    </div>
  );
}

// ── Step: Install ─────────────────────────────────────────────────────────────

function StepInstall({ os }: { os: OS }) {
  const macosSteps = [
    {
      label: "One-line installer (recommended)",
      code: `curl -fsSL https://raw.githubusercontent.com/wkang0223/hatch/master/scripts/install-agent-macos.sh | bash`,
    },
    {
      label: "Or build from source (Rust 1.78+)",
      code: `git clone https://github.com/wkang0223/hatch
cd hatch
cargo build --release -p hatch-agent
sudo cp target/release/hatch-agent /usr/local/bin/`,
    },
    {
      label: "Install MLX runtime (required for LLM inference)",
      code: `pip3 install mlx mlx-lm torch`,
    },
  ];

  const cudaSteps = [
    {
      label: "Install CUDA 12.8+ (skip if already installed)",
      code: `# Ubuntu/Debian
wget https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2204/x86_64/cuda-keyring_1.1-1_all.deb
sudo dpkg -i cuda-keyring_1.1-1_all.deb
sudo apt-get update && sudo apt-get install -y cuda-toolkit-12-8`,
    },
    {
      label: "Build and install hatch-agent",
      code: `git clone https://github.com/wkang0223/hatch
cd hatch
cargo build --release -p hatch-agent
sudo cp target/release/hatch-agent /usr/local/bin/`,
    },
    {
      label: "Install torch-cuda runtime",
      code: `pip3 install torch torchvision --index-url https://download.pytorch.org/whl/cu128
pip3 install vllm`,
    },
  ];

  const rocmSteps = [
    {
      label: "Install ROCm 6.2 (Ubuntu 22.04)",
      code: `sudo apt-get install -y rocm-hip-sdk
echo 'export PATH=$PATH:/opt/rocm/bin' >> ~/.bashrc
source ~/.bashrc`,
    },
    {
      label: "Build and install hatch-agent",
      code: `git clone https://github.com/wkang0223/hatch
cd hatch
cargo build --release -p hatch-agent
sudo cp target/release/hatch-agent /usr/local/bin/`,
    },
    {
      label: "Install torch-rocm runtime",
      code: `pip3 install torch torchvision --index-url https://download.pytorch.org/whl/rocm6.2`,
    },
  ];

  const steps = os === "macos" ? macosSteps : os === "linux-cuda" ? cudaSteps : rocmSteps;

  return (
    <div className="space-y-5">
      <p className="text-slate-400 text-sm">
        {os === "macos"
          ? "Install the hatch agent on your Mac. It will auto-detect your chip and start when your screen is locked."
          : "Install hatch-agent on your Linux machine. It runs as a systemd service."}
      </p>
      {steps.map((s, i) => (
        <div key={i}>
          <p className="text-xs text-slate-500 mb-1.5">
            <span className="text-brand-400 font-mono mr-2">{i + 1}.</span>
            {s.label}
          </p>
          <CodeBlock code={s.code} />
        </div>
      ))}
      <div className="flex items-start gap-2 p-3 rounded-lg bg-blue-500/5 border border-blue-500/20 text-xs text-blue-300">
        <AlertCircle className="h-4 w-4 flex-shrink-0 mt-0.5" />
        <span>
          {os === "macos"
            ? "The installer creates a launchd plist. Your Mac will only accept jobs when screen is locked and GPU utilisation is below 5%."
            : "The agent runs as user hatch_worker in a systemd unit. Jobs are isolated with cgroups v2 + bubblewrap."}
        </span>
      </div>
    </div>
  );
}

// ── Step: Configure ───────────────────────────────────────────────────────────

function StepConfigure({ os }: { os: OS }) {
  const [price, setPrice] = useState("0.05");
  const [region, setRegion] = useState("ap-southeast");
  const [idleMinutes, setIdleMinutes] = useState("10");

  const configCmd = `hatch provider config \\
  --floor-price ${price} \\
  --region ${region} \\
  --idle-minutes ${idleMinutes}`;

  const envCmd = `# Add to ~/.bashrc or ~/.zshrc
export HATCH_COORDINATOR=https://coordinator.hatch.dev
export HATCH_REGION=${region}`;

  return (
    <div className="space-y-5">
      <p className="text-slate-400 text-sm">
        Set your pricing and availability preferences.
      </p>

      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div>
          <label className="text-xs text-slate-500 block mb-1.5">Floor price (HTC/hr)</label>
          <input
            type="number" min="0.01" step="0.01"
            value={price}
            onChange={(e) => setPrice(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-200 font-mono focus:outline-none focus:border-brand-400/50"
          />
          <p className="text-xs text-slate-600 mt-1">Min you&apos;ll accept per hour</p>
        </div>
        <div>
          <label className="text-xs text-slate-500 block mb-1.5">Region</label>
          <select
            value={region}
            onChange={(e) => setRegion(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-200 focus:outline-none focus:border-brand-400/50"
          >
            <option value="ap-southeast">Asia Pacific (SE)</option>
            <option value="ap-northeast">Asia Pacific (NE)</option>
            <option value="us-east">US East</option>
            <option value="us-west">US West</option>
            <option value="eu-west">Europe West</option>
          </select>
        </div>
        <div>
          <label className="text-xs text-slate-500 block mb-1.5">Idle threshold (min)</label>
          <input
            type="number" min="1" max="60"
            value={idleMinutes}
            onChange={(e) => setIdleMinutes(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-200 font-mono focus:outline-none focus:border-brand-400/50"
          />
          <p className="text-xs text-slate-600 mt-1">Minutes idle before accepting jobs</p>
        </div>
      </div>

      <div>
        <p className="text-xs text-slate-500 mb-1.5">Generated config command</p>
        <CodeBlock code={configCmd} />
      </div>

      <div>
        <p className="text-xs text-slate-500 mb-1.5">Environment (add to shell profile)</p>
        <CodeBlock code={envCmd} />
      </div>
    </div>
  );
}

// ── Step: Register ────────────────────────────────────────────────────────────

function StepRegister({ os }: { os: OS }) {
  const startCmd = os === "macos"
    ? `# Start agent in foreground (first run — check output for errors)
hatch-agent --foreground

# Or load launchd plist (runs on screen lock, restarts on crash)
launchctl load ~/Library/LaunchAgents/dev.hatch.agent.plist`
    : `# Start agent (foreground, first run)
hatch-agent --foreground

# Or enable systemd service
sudo systemctl enable --now hatch-agent`;

  const verifyCmd = `# Check registration status
hatch provider status

# Expected output:
# Provider ID : abc123...
# State       : available
# Runtimes    : mlx, torch-mps  (or torch-cuda / torch-rocm)
# Last seen   : just now`;

  return (
    <div className="space-y-5">
      <p className="text-slate-400 text-sm">
        Start the agent. On first run it will register with the coordinator, generate an Ed25519 keypair,
        and begin advertising your GPU.
      </p>

      <CodeBlock code={startCmd} label="Start the agent" />
      <CodeBlock code={verifyCmd} label="Verify registration" />

      <div className="glass rounded-xl p-4 space-y-3">
        <p className="text-sm font-medium text-white">What happens on first run</p>
        {[
          "Detects GPU specs (chip model, memory, GPU cores, runtimes)",
          "Generates Ed25519 keypair (stored in macOS Keychain / Linux keyring)",
          "Registers with coordinator — you get a provider ID",
          "Begins advertising availability via libp2p DHT",
          "Jobs are only assigned when idle threshold is met",
        ].map((s) => (
          <div key={s} className="flex items-start gap-2 text-sm text-slate-400">
            <CheckCircle className="h-4 w-4 text-brand-400 flex-shrink-0 mt-0.5" />
            {s}
          </div>
        ))}
      </div>
    </div>
  );
}

// ── Step: Done ────────────────────────────────────────────────────────────────

function StepDone() {
  return (
    <div className="text-center space-y-8">
      <div className="h-16 w-16 rounded-full bg-brand-400/10 border border-brand-400/30 flex items-center justify-center mx-auto">
        <CheckCircle className="h-8 w-8 text-brand-400" />
      </div>
      <div>
        <h3 className="text-xl font-bold text-white mb-2">You&apos;re live as a provider</h3>
        <p className="text-slate-400 text-sm max-w-md mx-auto">
          Your device is now registered. The next time your screen locks and GPU is idle,
          Hatch will match you with a job automatically.
        </p>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 text-left">
        {[
          {
            icon: Terminal,
            label: "Monitor earnings",
            desc: "hatch provider status",
            href: "/provider",
          },
          {
            icon: DollarSign,
            label: "View your wallet",
            desc: "Check HTC balance",
            href: "/wallet",
          },
          {
            icon: ShieldCheck,
            label: "Account settings",
            desc: "KYC & device binding",
            href: "/account",
          },
        ].map((item) => (
          <a
            key={item.label}
            href={item.href}
            className="glass rounded-xl p-4 hover:border-slate-600 transition-colors group"
          >
            <item.icon className="h-5 w-5 text-brand-400 mb-3" />
            <div className="text-sm font-medium text-white mb-1">{item.label}</div>
            <div className="text-xs text-slate-500 font-mono">{item.desc}</div>
          </a>
        ))}
      </div>
    </div>
  );
}

// ── Main ──────────────────────────────────────────────────────────────────────

export default function OnboardPage() {
  const [step, setStep] = useState(0);
  const [os, setOs]     = useState<OS>(null);
  const router = useRouter();

  const canAdvance = step === 0 ? os !== null : true;

  function advance() {
    if (step < STEPS.length - 1) setStep(step + 1);
    else router.push("/provider");
  }

  function back() {
    if (step > 0) setStep(step - 1);
  }

  return (
    <div className="min-h-screen pt-14">
      <div className="max-w-2xl mx-auto px-4 sm:px-6 py-12">

        {/* Page header */}
        <div className="text-center mb-10">
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-brand-400/30 bg-brand-400/5 text-brand-300 text-xs font-medium mb-4">
            <Cpu className="h-3 w-3" />
            Provider Setup
          </div>
          <h1 className="text-3xl font-bold text-white">Become a Hatch provider</h1>
          <p className="text-slate-400 text-sm mt-2">
            Earn HTC credits while your GPU is idle — setup takes under 5 minutes.
          </p>
        </div>

        {/* Step indicators */}
        <div className="flex items-center gap-1 mb-10 overflow-x-auto pb-1">
          {STEPS.map((s, i) => (
            <div key={s.id} className="flex items-center gap-1 flex-shrink-0">
              <div className={cn(
                "h-7 px-3 rounded-full text-xs font-medium flex items-center gap-1.5 transition-colors",
                i === step
                  ? "bg-brand-400/15 border border-brand-400/40 text-brand-300"
                  : i < step
                    ? "bg-green-500/10 border border-green-500/20 text-green-400"
                    : "bg-slate-800/50 border border-slate-700/50 text-slate-500"
              )}>
                {i < step ? <CheckCircle className="h-3 w-3" /> : <span className="font-mono">{i + 1}</span>}
                {s.label}
              </div>
              {i < STEPS.length - 1 && (
                <div className={cn("h-px w-4 flex-shrink-0", i < step ? "bg-green-500/30" : "bg-slate-800")} />
              )}
            </div>
          ))}
        </div>

        {/* Step content */}
        <div className="glass rounded-2xl p-6 sm:p-8 mb-6 min-h-[340px]">
          {step === 0 && <StepPlatform os={os} setOs={setOs} />}
          {step === 1 && <StepInstall os={os} />}
          {step === 2 && <StepConfigure os={os} />}
          {step === 3 && <StepRegister os={os} />}
          {step === 4 && <StepDone />}
        </div>

        {/* Navigation */}
        <div className="flex items-center justify-between">
          <button
            onClick={back}
            disabled={step === 0}
            className="flex items-center gap-2 px-4 py-2.5 rounded-lg border border-slate-700 text-slate-400 hover:text-white hover:border-slate-600 text-sm font-medium transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          >
            <ChevronLeft className="h-4 w-4" />
            Back
          </button>

          <span className="text-xs text-slate-600">
            Step {step + 1} of {STEPS.length}
          </span>

          <button
            onClick={advance}
            disabled={!canAdvance}
            className="flex items-center gap-2 px-5 py-2.5 rounded-lg bg-brand-400 text-slate-950 text-sm font-semibold hover:bg-brand-300 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          >
            {step === STEPS.length - 1 ? "Go to dashboard" : "Continue"}
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  );
}
