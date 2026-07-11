#!/usr/bin/env bash
# SH-010: Download self-hosted font woff2 files into shell/public/fonts/.
#
# Fetches Inter, IBM Plex Serif, and JetBrains Mono from GitHub releases.
# Run once before building or when updating font versions.
#
# Usage:
#   bash scripts/download_fonts.sh
#
# Requirements: curl, a working internet connection.

set -euo pipefail

FONTS_DIR="$(dirname "$0")/../shell/public/fonts"
mkdir -p "$FONTS_DIR"

echo "Downloading fonts into $FONTS_DIR ..."

# ── Inter ────────────────────────────────────────────────────────────────────
# As of v4.x, rsms/inter no longer publishes per-weight woff2 assets —
# the release ships a single `Inter-<ver>.zip` whose `web/` folder holds
# the static woff2 instances. Download + extract only the weights we use.
INTER_ZIP_URL="https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip"
if [[ ! -f "$FONTS_DIR/Inter-Regular.woff2" ]]; then
  echo "  Inter (zip)"
  INTER_TMP_ZIP="$(mktemp -t inter.XXXXXX).zip"
  INTER_TMP_DIR="$(mktemp -d)"
  curl -fsSL "$INTER_ZIP_URL" -o "$INTER_TMP_ZIP"
  unzip -q "$INTER_TMP_ZIP" -d "$INTER_TMP_DIR"
  for weight in Regular Medium SemiBold Bold; do
    src="$(find "$INTER_TMP_DIR" -name "Inter-${weight}.woff2" | head -1)"
    if [[ -n "$src" ]]; then
      cp "$src" "$FONTS_DIR/Inter-${weight}.woff2"
    else
      echo "    WARNING: Inter-${weight}.woff2 not found in zip" >&2
    fi
  done
  rm -f "$INTER_TMP_ZIP"
  rm -rf "$INTER_TMP_DIR"
fi

# ── IBM Plex Serif ───────────────────────────────────────────────────────────
PLEX_BASE="https://github.com/IBM/plex/releases/download/%40ibm%2Fplex-serif%401.1.0/ibm-plex-serif.zip"
# IBM Plex is distributed as a zip; download and extract only what we need.
TMP_ZIP="$(mktemp -t plex.XXXXXX).zip"
TMP_DIR="$(mktemp -d)"
if [[ ! -f "$FONTS_DIR/IBMPlexSerif-Regular.woff2" ]]; then
  echo "  IBMPlexSerif (zip)"
  curl -fsSL "$PLEX_BASE" -o "$TMP_ZIP"
  unzip -q "$TMP_ZIP" -d "$TMP_DIR"
  for weight in Regular Italic Medium SemiBold; do
    src="$(find "$TMP_DIR" -name "IBMPlexSerif-${weight}.woff2" | head -1)"
    if [[ -n "$src" ]]; then
      cp "$src" "$FONTS_DIR/IBMPlexSerif-${weight}.woff2"
    fi
  done
  rm -f "$TMP_ZIP"
  rm -rf "$TMP_DIR"
fi

# ── JetBrains Mono ───────────────────────────────────────────────────────────
# Like Inter, recent JetBrains Mono releases ship a single zip
# (`JetBrainsMono-<ver>.zip`) with the woff2 instances under
# `fonts/webfonts/` rather than per-weight release assets.
JB_ZIP_URL="https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip"
if [[ ! -f "$FONTS_DIR/JetBrainsMono-Regular.woff2" ]]; then
  echo "  JetBrainsMono (zip)"
  JB_TMP_ZIP="$(mktemp -t jbmono.XXXXXX).zip"
  JB_TMP_DIR="$(mktemp -d)"
  curl -fsSL "$JB_ZIP_URL" -o "$JB_TMP_ZIP"
  unzip -q "$JB_TMP_ZIP" -d "$JB_TMP_DIR"
  for weight in Regular Medium; do
    src="$(find "$JB_TMP_DIR" -name "JetBrainsMono-${weight}.woff2" | head -1)"
    if [[ -n "$src" ]]; then
      cp "$src" "$FONTS_DIR/JetBrainsMono-${weight}.woff2"
    else
      echo "    WARNING: JetBrainsMono-${weight}.woff2 not found in zip" >&2
    fi
  done
  rm -f "$JB_TMP_ZIP"
  rm -rf "$JB_TMP_DIR"
fi

echo "Done. Font files:"
ls -lh "$FONTS_DIR"/*.woff2 2>/dev/null || echo "  (no .woff2 files found — manual download may be needed)"
