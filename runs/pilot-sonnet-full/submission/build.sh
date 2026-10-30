#!/bin/bash
set -e
cd "$(dirname "$0")"
export GOFLAGS=-mod=mod GOCACHE=${GOCACHE:-/tmp/gocache} GOPATH=${GOPATH:-/tmp/gopath} CGO_ENABLED=0
go build -o calc .
