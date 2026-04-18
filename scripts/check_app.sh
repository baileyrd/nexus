#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus/app || exit 1
npx --yes tsc --noEmit 2>&1 | tail -60
