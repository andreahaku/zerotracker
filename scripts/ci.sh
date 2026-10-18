#!/usr/bin/env bash
#
# Local CI: Rust checks and tests, desktop build, optional arm64 build.
#
#   scripts/ci.sh           # Rust + desktop
#   scripts/ci.sh --arm64   # also the device build in the trixie container
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

echo "== cargo fmt";    cargo fmt --all --check
echo "== cargo clippy"; cargo clippy --workspace --all-targets -- -D warnings
echo "== cargo test";   cargo test --workspace --quiet
echo "== desktop build"
cmake --preset linux-x86-64 >/dev/null
cmake --build --preset linux-x86-64-rel

if [ "${1:-}" = "--arm64" ]; then
    echo "== arm64 build (trixie container)"
    scripts/cp0-docker-build.sh
    file build/cp0-trixie/Release/zerotracker
fi
echo "== CI OK"
