#!/bin/sh
set -e
cd "$(dirname "$0")"
CH="${CARGO_HOME:-$HOME/.cargo}"
if [ ! -w "$CH" ] || [ ! -w "$CH/.package-cache" -a -e "$CH/.package-cache" ]; then
  NEWCH="$(mktemp -d)"
  if [ -f "$CH/config.toml" ]; then cp "$CH/config.toml" "$NEWCH/"; fi
  export CARGO_HOME="$NEWCH"
fi
if [ ! -w . ]; then
  export CARGO_TARGET_DIR="$(mktemp -d)"
fi
cargo build --release --offline
cp "${CARGO_TARGET_DIR:-target}/release/calc" ./calc
