#!/bin/bash
source ~/.bashrc 2>/dev/null || true
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:$PATH"
cd /mnt/c/Users/baile/dev/Nexus
cargo check -p nexus-storage 2>&1 | tail -30
echo "---test---"
cargo test -p nexus-storage --lib 2>&1 | tail -30
