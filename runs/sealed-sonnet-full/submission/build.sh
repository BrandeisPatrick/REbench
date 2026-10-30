#!/bin/bash
set -e -o pipefail
cd "$(dirname "$0")"
cargo build --release --offline 2>&1 | tail -5
cp target/release/calc ./calc
