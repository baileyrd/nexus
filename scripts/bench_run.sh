#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1
# --quick caps each bench at ~3 samples; still statistically useful for
# order-of-magnitude validation against PRD-09 §17 targets.
CARGO_TERM_COLOR=never cargo bench -p nexus-terminal --bench buffers -- --quick 2>&1
echo ""
echo "=== lines ==="
CARGO_TERM_COLOR=never cargo bench -p nexus-terminal --bench lines -- --quick 2>&1
