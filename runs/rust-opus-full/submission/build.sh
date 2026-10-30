#!/bin/bash
set -e
cd "$(dirname "$0")"
cargo build --release --offline 2>&1
cp target/release/calc ./calc
