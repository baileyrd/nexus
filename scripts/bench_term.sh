#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1
# Build benches in release mode; run each bench with --quick to keep
# the measurement tractable in CI while still surfacing regressions.
cargo bench -p nexus-terminal --bench buffers -- --quick 2>&1 | tail -40
echo ""
echo "=== lines ==="
cargo bench -p nexus-terminal --bench lines -- --quick 2>&1 | tail -40
