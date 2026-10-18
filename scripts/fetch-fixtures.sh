#!/usr/bin/env bash
#
# Copies the upstream m8-file-parser 0.7.0 test files into m8fmt/examples/
# (git-ignored: M8 files never go in the repo). The crate's unit tests read them.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/m8-file-parser-0.7.0 2>/dev/null | head -1 || true)"
if [ -z "$SRC" ]; then
    tmp="$(mktemp -d)"
    (cd "$tmp" && cargo init -q --name fetch && cargo add -q m8-file-parser@=0.7.0 && cargo fetch -q)
    SRC="$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/m8-file-parser-0.7.0 | head -1)"
fi
mkdir -p "$REPO/m8fmt/examples"
cp -r "$SRC/examples/songs" "$SRC/examples/instruments" "$REPO/m8fmt/examples/"
echo "fixtures in m8fmt/examples from $SRC"
