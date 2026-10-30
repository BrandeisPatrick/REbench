#!/bin/bash
cd "$(dirname "$0")"
cargo build --release --offline 2>&1 | tail -30
cp target/release/calc ./calc
