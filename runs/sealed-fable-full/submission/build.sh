#!/bin/bash
set -e
cd "$(dirname "$0")"
export GOFLAGS=-mod=mod GOPATH=/tmp/gopath GOCACHE=/tmp/gocache CGO_ENABLED=0 GOTOOLCHAIN=local GOPROXY=off
export HOME="${HOME:-/tmp}"
go build -o calc .
