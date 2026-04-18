#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1
ls fixtures/
ls fixtures/forge/ 2>/dev/null || echo "fixtures/forge missing"
ls fixtures/bases/ 2>/dev/null || echo "fixtures/bases missing"
echo "---untracked 1---"
ls -la "1" 2>/dev/null || echo "no file named 1"
