#!/usr/bin/env bash
# Cross-compile NeuralMesh binaries for all target platforms.
#
# Targets:
#   darwin-arm64    macOS Apple Silicon (primary — agent + nm CLI)
#   linux-x86_64    Coordinator, ledger, nm CLI (Linux server)
#   linux-aarch64   Raspberry Pi / ARM servers
#
# Requires:
#   cargo + cross (cargo install cross --git https://github.com/cross-rs/cross)
#   Docker (for cross compilation)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
OUT_DIR="$REPO_ROOT/dist"

cd "$REPO_ROOT"

mkdir -p "$OUT_DIR"

echo "→ Building NeuralMesh release binaries"
echo "  Output: $OUT_DIR"
echo ""

# ── darwin-arm64 (native build on Apple Silicon) ──────────────────────────────

if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]; then
    echo "→ darwin-arm64 (native)"

    cargo build --release \
        -p neuralmesh-agent \
        -p neuralmesh-cli

    cp target/release/neuralmesh-agent "$OUT_DIR/neuralmesh-agent-darwin-arm64"
    cp target/release/nm               "$OUT_DIR/nm-darwin-arm64"

    echo "  ✓ neuralmesh-agent-darwin-arm64"
    echo "  ✓ nm-darwin-arm64"
    echo ""
fi

# ── linux-x86_64 ──────────────────────────────────────────────────────────────

if command -v cross &>/dev/null; then
    echo "→ linux-x86_64 (cross)"
    cross build --release \
        --target x86_64-unknown-linux-musl \
        -p neuralmesh-coordinator \
        -p neuralmesh-ledger \
        -p neuralmesh-cli

    cp target/x86_64-unknown-linux-musl/release/neuralmesh-coordinator \
        "$OUT_DIR/neuralmesh-coordinator-linux-x86_64"
    cp target/x86_64-unknown-linux-musl/release/neuralmesh-ledger \
        "$OUT_DIR/neuralmesh-ledger-linux-x86_64"
    cp target/x86_64-unknown-linux-musl/release/nm \
        "$OUT_DIR/nm-linux-x86_64"

    echo "  ✓ neuralmesh-coordinator-linux-x86_64"
    echo "  ✓ neuralmesh-ledger-linux-x86_64"
    echo "  ✓ nm-linux-x86_64"
    echo ""

    echo "→ linux-aarch64 (cross)"
    cross build --release \
        --target aarch64-unknown-linux-musl \
        -p neuralmesh-cli

    cp target/aarch64-unknown-linux-musl/release/nm \
        "$OUT_DIR/nm-linux-aarch64"
    echo "  ✓ nm-linux-aarch64"
    echo ""
else
    echo "  ⚠ 'cross' not installed — skipping Linux cross-compilation"
    echo "    Install: cargo install cross --git https://github.com/cross-rs/cross"
fi

# ── SHA256 checksums ──────────────────────────────────────────────────────────

echo "→ Computing checksums..."
(cd "$OUT_DIR" && shasum -a 256 * > SHA256SUMS && echo "  ✓ SHA256SUMS")

echo ""
echo "✓ Build complete. Artifacts in: $OUT_DIR"
ls -lh "$OUT_DIR"
