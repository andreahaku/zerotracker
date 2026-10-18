#!/usr/bin/env bash
#
# Cross-build for the CardputerZero inside a Debian trixie container
# (docker/cp0-build/Dockerfile), so the binaries match the device's glibc and
# libstdc++ regardless of the host toolchain. Adapted from ZeroRadio.
#
#   scripts/cp0-docker-build.sh             # configure + build (Release)
#   scripts/cp0-docker-build.sh --package   # also produce the .deb (CPack)
#
# The repo is mounted at the same absolute path and the build runs as the
# calling user, so build/cp0-trixie and target/ stay host-owned.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="zerotracker-cp0-build:trixie"

docker build -q -t "$IMAGE" "$REPO/docker/cp0-build" >/dev/null

package=0
args=()
for a in "$@"; do
    if [ "$a" = "--package" ]; then package=1; else args+=("$a"); fi
done

inner="cmake --preset cp0-trixie && cmake --build --preset cp0-trixie-rel ${args[*]:-}"
if [ "$package" = 1 ]; then
    inner="$inner && cd build/cp0-trixie && cpack -C Release -G DEB"
fi

docker run --rm \
    --user "$(id -u):$(id -g)" \
    -e HOME=/tmp -e CARGO_HOME="$REPO/.cache/cargo-home" \
    -v "$REPO:$REPO" -w "$REPO" \
    "$IMAGE" bash -c "$inner"
