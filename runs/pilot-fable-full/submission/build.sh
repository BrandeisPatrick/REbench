#!/bin/bash
set -e
cd "$(dirname "$0")"
export GOCACHE=/tmp/gocache GOPATH=/tmp/gopath GOFLAGS=-mod=mod GO111MODULE=on GOTOOLCHAIN=local GOPROXY=off
go build -o calc .
