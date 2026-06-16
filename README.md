# Hatch

> **Decentralized GPU marketplace.** Idle Apple Silicon Macs, NVIDIA RTX rigs, and AMD Ryzen AI systems lease their compute to AI workloads — earners get HTC credits, buyers get GPU time at a fraction of cloud cost.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org/)

---

## Why Hatch?

| Platform | Apple Silicon | NVIDIA RTX | AMD ROCm |
|---|:---:|:---:|:---:|
| **Hatch** | ✅ | ✅ | ✅ |
| io.net | ❌ | ✅ | ❌ |
| Vast.ai | ❌ | ✅ | partial |
| Akash | ❌ | ✅ | ❌ |
| Salad | ❌ | ✅ | ❌ |

**No other GPU marketplace supports Apple Silicon.** A Mac Studio M4 Ultra has 192 GB unified memory and 800 GB/s bandwidth — it runs 70B LLMs natively via MLX at 30+ tok/s, with no VRAM bottleneck. Hatch is the only way to monetize that.

---

## Supported Hardware

### Apple Silicon

| Device | Unified Memory | GPU Cores | Best For |
|---|---|---|---|
| Mac Mini M4 | 16–24 GB | 10 | Small model inference |
| Mac Mini M4 Pro | 24–64 GB | 20 | 30B LLM inference |
| Mac Studio M4 Max | 64–128 GB | 40 | 70B LLM inference |
| Mac Studio M4 Ultra | 128–192 GB | 80 | 405B inference shards |

### NVIDIA (RTX 3xxx → 5xxx + Datacenter)

| Generation | Examples | Runtime |
|---|---|---|
| Blackwell (RTX 50xx) | RTX 5090, 5080, 5070 Ti | `torch-cuda`, `vllm-cuda`, `tensorrt` |
| Ada Lovelace (RTX 40xx) | RTX 4090, 4080, 4070 Ti | `torch-cuda`, `vllm-cuda`, `tensorrt` |
| Ampere (RTX 30xx) | RTX 3090, 3080, 3070 | `torch-cuda`, `onnx-cuda` |
| Datacenter | B200, B100, H200, H100, A100 | `torch-cuda`, `vllm-cuda` |

### AMD

| Device | Memory | Runtime |
|---|---|---|
| Ryzen AI Max+ 395 (Strix Halo) | 128 GB shared | `torch-rocm`, `onnx-rocm`, `llama-cpp-hip` |

---

## How It Works

```
Provider (idle Mac / Linux rig)        Coordinator (Rust)            Consumer (any machine)
┌────────────────────────────┐        ┌──────────────────┐          ┌──────────────────┐
│  hatch-agent               │        │  REST + gRPC API │          │  hatch CLI       │
│  • GPU detection           │──────▶ │  Reverse auction │ ◀──────  │  hatch job sub.. │
│  • Runtime registration    │  gRPC  │  Runtime matcher │  REST    │  hatch wallet    │
│  • Job execution           │        │  Escrow engine   │          └──────────────────┘
│  • Heartbeat / checkpoint  │        │  NATS pub/sub    │
│  • WireGuard tunnel        │        │  PostgreSQL       │
└────────────────────────────┘        └──────────────────┘
  macOS: sandbox-exec                         │
  Linux: cgroups v2 + bubblewrap      ┌──────────────────┐
                                      │  hatch-ledger    │
                                      │  Off-chain HTC   │
                                      │  accounting      │
                                      └──────────────────┘
```

1. **Provider** installs `hatch-agent`. The agent detects GPU specs and registers available runtimes (`mlx`, `torch-cuda`, `torch-rocm`, etc.) with the coordinator.
2. **Consumer** submits a job: bundle URL, runtime, RAM requirement, max price in HTC/hr.
3. **Coordinator** runs a reverse-auction every 30 seconds — matches the job to the best available provider (by price + trust score + region), locks HTC in escrow.
4. **Job** runs sandboxed. WireGuard encrypts all traffic. Provider sends heartbeats every 30s.
5. **On completion**, escrow settles: provider earns HTC, 8% platform fee retained.

---

## Monorepo Structure

```
hatch/
├── agent/                  # Provider daemon (macOS launchd / Linux systemd)
├── coordinator/            # Job matching, REST + gRPC API, auction engine
├── ledger/                 # Off-chain HTC credit accounting
├── cli/                    # hatch CLI (providers + consumers)
├── dashboard/              # Next.js 15 web dashboard
├── crates/
│   ├── hatch-common/       # Shared types and error handling
│   ├── hatch-crypto/       # Ed25519 keypairs, post-quantum (Dilithium/Kyber)
│   ├── hatch-gpu/          # GPU detection: Apple Metal, NVIDIA CUDA, AMD ROCm
│   ├── hatch-macos/        # IOKit, CGSession, Metal, sandbox-exec
│   ├── hatch-linux/        # cgroups v2, bubblewrap, ROCm / CUDA detection
│   ├── hatch-proto/        # Generated gRPC stubs (prost)
│   └── hatch-wireguard/    # boringtun WireGuard wrapper
├── proto/                  # agent.proto, job.proto, ledger.proto
└── infrastructure/         # Docker Compose, render.yaml, Vercel config
```

---

## Quick Start

### Prerequisites

- Rust 1.78+
- PostgreSQL 15+
- Node.js 20+ (dashboard only)

### Run coordinator locally

```bash
# Create DB
createuser -s hatch && createdb -O hatch hatch

# Start coordinator (runs migrations automatically)
DATABASE_URL=postgresql://hatch:hatch@localhost/hatch \
  cargo run -p hatch-coordinator
```

### Register your Mac as a provider

```bash
# Install MLX (one-time, macOS only)
pip3 install mlx mlx-lm

# Start agent
HATCH_COORDINATOR=http://localhost:8080 \
  cargo run -p hatch-agent -- --foreground
```

The agent auto-detects your chip generation, available memory, and supported runtimes, then registers. It offers your GPU when the screen is locked and GPU utilisation is below 5%.

### Submit a job

```bash
# Submit an inference job (MLX runtime, needs 24GB RAM, max 0.50 HTC/hr)
curl -X POST http://localhost:8080/api/v1/jobs \
  -H 'Content-Type: application/json' \
  -d '{
    "account_id": "acc_your_id",
    "bundle_url": "https://your-model-bundle.tar.gz",
    "runtime": "mlx",
    "min_ram_gb": 24,
    "max_price_per_hour": 0.50
  }'
```

---

## Technology Stack

| Layer | Technology |
|---|---|
| Agent | Rust, IOKit FFI, CGSession, Metal |
| Coordinator | Rust, Axum (REST), Tonic (gRPC), SQLx |
| Job isolation | `sandbox-exec` (macOS), cgroups v2 + bubblewrap (Linux) |
| Job queue | NATS JetStream (optional, falls back to DB polling) |
| Database | PostgreSQL with SQLx migrations |
| P2P | libp2p Kademlia DHT, WireGuard (boringtun) |
| Payments | HTC credits, on-chain escrow via Arbitrum L2 |
| Crypto | Ed25519 + post-quantum NIST FIPS 203/204 (Dilithium, Kyber) |
| Dashboard | Next.js 15, Tailwind CSS |

---

## Security Model

- **Sandboxed execution**: jobs run as a dedicated `hatch_worker` OS user
  - macOS: `sandbox-exec` — file access limited to `/tmp/hatch/job-{id}/`, no internet
  - Linux: cgroups v2 + bubblewrap (user namespaces, no root required)
- **Encrypted tunnels**: per-job ephemeral WireGuard tunnels (ChaCha20-Poly1305)
- **Hardware attestation**: `IOPlatformSerialNumber` + `IOPlatformUUID` for Sybil resistance
- **Provider keys**: Ed25519 keypairs stored in macOS Keychain (Secure Enclave backed)
- **Post-quantum ready**: Dilithium (signatures) + Kyber (key exchange) — NIST FIPS 203/204
- **Trust system**: providers earn a trust score (0–5) that decays on disconnect/failure

---

## Runtimes

| Integer | Key | Platform |
|---|---|---|
| 0 | `mlx` | Apple Silicon |
| 1 | `torch-mps` | Apple Silicon |
| 2 | `onnx-coreml` | Apple Silicon |
| 3 | `llama-cpp` | Apple Silicon |
| 4 | `shell` | Any |
| 10 | `torch-cuda` | NVIDIA |
| 11 | `onnx-cuda` | NVIDIA |
| 12 | `tensorrt` | NVIDIA |
| 13 | `llama-cpp-cuda` | NVIDIA |
| 14 | `vllm-cuda` | NVIDIA |
| 20 | `torch-rocm` | AMD |
| 21 | `onnx-rocm` | AMD |
| 22 | `llama-cpp-hip` | AMD |

---

## Roadmap

| Phase | Scope | Status |
|---|---|---|
| 1 | Apple Silicon: agent, coordinator, REST/gRPC API, off-chain HTC credits | ✅ Done |
| 2 | Linux AMD (ROCm) + NVIDIA (CUDA) providers, runtime-aware matching | ✅ Done |
| 3 | Web dashboard, provider onboarding flow, public waitlist | 🔨 In progress |
| 4 | On-chain HTC escrow (Arbitrum L2), DMTCP job checkpointing | Planned |
| 5 | ZK compute proofs, DAO governance, Python SDK, HuggingFace integration | Planned |

---

## Deployment

| Service | Platform | Config |
|---|---|---|
| Coordinator | [Render](https://render.com) | `render.yaml` |
| PostgreSQL | Render managed DB | `render.yaml` |
| Dashboard | [Vercel](https://vercel.com) | `dashboard/vercel.json` |
| NATS (optional) | Self-hosted | `infrastructure/docker-compose.yml` |

---

## Contributing

Issues and PRs welcome. For significant changes, open an issue first to discuss the approach.

## License

MIT — see [LICENSE](LICENSE)
