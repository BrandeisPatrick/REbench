#!/bin/bash
set -e
cd "$(dirname "$0")"
cargo build --release --offline 2>&1 | tail -n 5
cp target/release/calc ./calc
