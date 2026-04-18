#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/c/Users/baile/.cargo/bin
if [ -d /mnt/c/Users/baile/dev/Nexus ]; then
    cd /mnt/c/Users/baile/dev/Nexus
elif [ -d /c/Users/baile/dev/Nexus ]; then
    cd /c/Users/baile/dev/Nexus
else
    echo "no nexus root"; exit 1
fi
cargo test -p nexus-terminal --lib 2>&1 | tail -100
