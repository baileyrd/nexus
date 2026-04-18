#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1
# Restore deleted fixture files
git restore fixtures/
# Remove spurious redirect artefact
rm -f 1
echo "Done"
git status --short
