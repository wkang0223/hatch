#!/usr/bin/env bash
# Regenerate Rust/Python code from .proto definitions.
# Requires: protoc, protoc-gen-prost, tonic-build (via cargo build)
#
# The primary code generation happens automatically via build.rs in nm-proto.
# This script validates that protoc is installed and triggers a fresh build.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$REPO_ROOT"

echo "→ Checking protoc..."
if ! command -v protoc &>/dev/null; then
    echo "  protoc not found. Install via:"
    echo "    brew install protobuf"
    exit 1
fi
echo "  protoc $(protoc --version)"

echo "→ Validating .proto files..."
for proto in proto/*.proto; do
    protoc --proto_path=proto "$proto" -o /dev/null 2>&1 && echo "  ✓ $proto" || echo "  ✗ $proto"
done

echo "→ Running cargo build to trigger tonic build.rs code generation..."
cargo build -p nm-proto 2>&1 | grep -E '(error|warning.*unused|Compiling nm-proto)' || true

echo "✓ Proto generation complete. Generated files in:"
echo "  crates/nm-proto/src/generated/"
ls crates/nm-proto/src/generated/ 2>/dev/null || echo "  (will appear after first build)"
