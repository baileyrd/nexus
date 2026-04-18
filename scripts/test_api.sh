#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1
cargo test -p nexus-plugin-api -p nexus-kernel -p nexus-plugins 2>&1
