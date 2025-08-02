#!/bin/bash

# Stop on errors
set -e

echo "Building SneakTime for testing..."
make clean
make build

echo "Creating dist directory if needed..."
mkdir -p dist

echo "Copying binary to dist for tests..."
cp time-whisperer dist/timewhisperer-macos-amd64

echo "Running integration tests..."
go test -v ./tests -run "^Test.*$" -timeout 5m

echo "Integration tests completed!" 