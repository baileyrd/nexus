#!/bin/bash
cd /mnt/c/Users/baile/dev/Nexus
/home/baileyrd/.cargo/bin/cargo check -p nexus-plugins 2>&1 | tail -60
