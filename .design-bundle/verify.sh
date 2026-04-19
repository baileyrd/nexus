#!/bin/bash
source ~/.bashrc 2>/dev/null || true
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:$PATH"
cd /mnt/c/Users/baile/dev/Nexus
echo "=== nexus-theme ==="
cargo test -p nexus-theme --lib 2>&1 | tail -4
echo ""
echo "=== nexus-storage ==="
cargo test -p nexus-storage --lib 2>&1 | tail -4
echo ""
echo "=== theme_ipc ==="
cargo test -p nexus-bootstrap --test theme_ipc 2>&1 | tail -4
