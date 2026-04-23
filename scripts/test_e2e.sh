#!/bin/bash
# End-to-end test suite for the Nexus shell Tauri app.
#
# Windows-only: tauri-driver wraps msedgedriver to drive WebView2,
# which is a Windows-specific runtime. WSL cannot spawn the Windows
# WebView2 host, so this script refuses to run anywhere that isn't
# either a native Windows shell (MSYS/Git Bash) or WSL's mount of
# C:\. See shell/e2e/README.md for prereqs (tauri-driver +
# msedgedriver).
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/c/Users/baile/.cargo/bin

if [ -d /mnt/c/Users/baile/dev/Nexus ]; then
    cd /mnt/c/Users/baile/dev/Nexus/shell || exit 1
elif [ -d /c/Users/baile/dev/Nexus ]; then
    cd /c/Users/baile/dev/Nexus/shell || exit 1
else
    echo "no nexus shell root"; exit 1
fi

# 1. Kill any stale processes holding WebView2 / the scratch vault.
#    A crashed prior run locks the fixture vault via the kernel's
#    exclusive file handle (see shell/e2e/README.md "Known issues").
taskkill.exe /F /IM nexus-shell.exe /IM tauri-driver.exe /IM msedgedriver.exe 2>/dev/null

# 2. Build the app in the e2e shape. This sets VITE_E2E=true at
#    Vite-build time (which exposes window.__nexusShellApi) and
#    passes `--features custom-protocol` to cargo so Tauri serves
#    the embedded bundle via the asset protocol.
pnpm e2e:build 2>&1 | tail -40 || exit 1

# 3. Run the WDIO suite. Golden-path + tier1/* specs; wdio.conf.ts
#    globs specs/**/*.spec.ts.
pnpm e2e 2>&1 | tail -200
