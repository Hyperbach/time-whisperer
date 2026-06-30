#!/bin/bash
# Build the release binary, then run integration tests serially.

set -e

echo "Building SneakTime for testing..."
cargo build --release --bin time-whisperer

echo "Copying release binary to project root..."
cp target/release/time-whisperer ./time-whisperer

echo "Running integration tests..."
# end_to_end tests spawn the debug-build binary placed in target/debug; they
# call cargo build themselves if needed. Use --test-threads=1 to avoid port
# contention.
cargo test --test end_to_end --test log_rotation -- --test-threads=1

echo "Integration tests completed!"
