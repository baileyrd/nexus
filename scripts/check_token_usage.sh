#!/usr/bin/env bash
# SH-012: Audit shell/src for legacy CSS token aliases.
#
# The canonical token set is the Obsidian palette defined in shell/index.html:
#   --background-*, --text-*, --interactive-*, --font-*, --radius-*, etc.
#
# Legacy alias families that should NOT appear in new code:
#   Forge short names:   --bg, --fg, --line, --accent, --r, --f-ui, --f-body, --f-mono
#   VSCode-style:        --shell-*, --activitybar-*, --editor-bg, --panel-bg, etc.
#   color-* aliases:     --color-bg, --color-accent, --color-error*
#   Bases aliases:       --bg-primary, --fg-primary, --bg-input, --bg-selection,
#                        --border-subtle, --fg-on-accent, --bg-raised-dim
#
# Usage:
#   bash scripts/check_token_usage.sh            # exits 1 if any violations found
#   bash scripts/check_token_usage.sh --fix       # NOT implemented; run migration manually

set -euo pipefail

ROOT="$(dirname "$0")/.."
SRC="$ROOT/shell/src"

# Patterns that flag a violation (match against CSS custom property references)
PATTERNS=(
  # Forge short names
  'var(--bg[^-]'
  'var(--bg)'
  'var(--fg[^-]'
  'var(--fg)'
  'var(--line[^-]'
  'var(--line)'
  'var(--accent[^-]'
  'var(--accent)'
  'var(--r[^a-z-]'
  'var(--r)'
  'var(--f-ui'
  'var(--f-body'
  'var(--f-mono'
  # VSCode-style
  'var(--shell-'
  'var(--activitybar-'
  'var(--sidebar-bg'
  'var(--editor-bg'
  'var(--panel-bg'
  'var(--statusbar-'
  'var(--list-hover'
  'var(--list-active'
  'var(--input-bg'
  'var(--font-ui)'
  'var(--font-ui,'
  'var(--font-mono)'
  'var(--font-mono,'
  'var(--titlebar-bg'
  # color-* aliases
  'var(--color-bg'
  'var(--color-accent'
  'var(--color-error'
  'var(--color-border'
  # Bases-only aliases
  'var(--bg-primary'
  'var(--fg-primary'
  'var(--bg-input'
  'var(--bg-selection'
  'var(--border-subtle'
  'var(--fg-on-accent'
  'var(--bg-raised-dim'
)

violations=0

for pattern in "${PATTERNS[@]}"; do
  matches=$(grep -rn --include="*.tsx" --include="*.ts" --include="*.css" \
    --exclude="*.test.ts" --exclude="*.test.tsx" \
    "$pattern" "$SRC" 2>/dev/null || true)
  if [[ -n "$matches" ]]; then
    echo "VIOLATION: $pattern"
    echo "$matches" | head -5
    violations=$((violations + 1))
  fi
done

if [[ $violations -gt 0 ]]; then
  echo ""
  echo "Found $violations legacy CSS token pattern(s). Use canonical Obsidian tokens:"
  echo "  --background-primary, --background-secondary, --text-normal, --interactive-accent, etc."
  exit 1
else
  echo "✓ No legacy CSS token violations found."
fi
