#!/bin/bash
source ~/.bashrc 2>/dev/null || true
export PATH="$HOME/.cargo/bin:/usr/local/bin:$PATH"
cd /mnt/c/Users/baile/dev/Nexus
which cargo
cargo test -p nexus-theme --lib 2>&1 | tail -80
