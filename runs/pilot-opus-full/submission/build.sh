#!/bin/bash
set -e
cd "$(dirname "$0")"
g++ -O2 -std=c++17 -o calc src/main.cpp -lzip -lpugixml -lpthread
