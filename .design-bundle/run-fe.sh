#!/bin/bash
source ~/.bashrc 2>/dev/null || true
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:$PATH"
cd /mnt/c/Users/baile/dev/Nexus/app
which node
which npm
npm run -s test 2>&1 | tail -60
echo "---build---"
npm run -s build 2>&1 | tail -40
