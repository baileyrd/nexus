#!/bin/bash
source ~/.bashrc 2>/dev/null || true
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:$PATH"
cd /mnt/c/Users/baile/dev/Nexus
cargo test -p nexus-bootstrap --test theme_ipc 2>&1 | tail -40
