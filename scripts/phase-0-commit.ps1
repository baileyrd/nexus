<#
.SYNOPSIS
    Phase 0 commit + tag for the shell-migration decision (ADR 0011).

.DESCRIPTION
    Stages ONLY the intentional Phase 0 edits (banners, ADR accept,
    CONTRIBUTING, integration review artifacts, parity checklist). Skips
    the 884 CRLF-drift files the audit environment had in its working
    tree so they don't pollute the commit.

    Then creates an annotated tag `v0.1.0-legacy-shell` at that commit
    so the pre-freeze state of the legacy shell is recoverable.

.NOTES
    Run from the repo root in PowerShell (Windows PowerShell 5.1 or
    PowerShell 7+). If a stale .git\index.lock is present, the script
    removes it after warning you.

    Requires:
      - git on PATH
      - git config user.name and user.email set (globally or for this repo)
      - clean commit authority (no IDE / Git GUI holding the index)

.EXAMPLE
    PS C:\path\to\nexus> .\scripts\phase-0-commit.ps1

.EXAMPLE
    # Dry run (stage but don't commit or tag):
    PS C:\path\to\nexus> .\scripts\phase-0-commit.ps1 -DryRun
#>

[CmdletBinding()]
param(
    [switch]$DryRun,
    [switch]$SkipTag,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Move to repo root (parent of scripts/)
# ---------------------------------------------------------------------------
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $repoRoot
Write-Host "Repo root:  $repoRoot" -ForegroundColor Cyan

# ---------------------------------------------------------------------------
# Sanity: git available, we're in a repo, committer configured
# ---------------------------------------------------------------------------
function Assert-Command($name) {
    if (-not (Get-Command $name -ErrorAction SilentlyContinue)) {
        throw "Required command '$name' not on PATH."
    }
}
Assert-Command git

if (-not (Test-Path '.git')) {
    throw "No .git directory at $repoRoot. Run this from the repo root."
}

$committerName = (git config user.name)
$committerEmail = (git config user.email)
if ([string]::IsNullOrWhiteSpace($committerName) -or [string]::IsNullOrWhiteSpace($committerEmail)) {
    throw "git user.name and user.email must be set. Run: git config user.name 'Your Name'; git config user.email 'you@example.com'"
}
Write-Host "Committer:  $committerName <$committerEmail>" -ForegroundColor Cyan

# ---------------------------------------------------------------------------
# Clear stale index.lock if present
# ---------------------------------------------------------------------------
$indexLock = Join-Path $repoRoot '.git\index.lock'
if (Test-Path $indexLock) {
    Write-Host "Found stale .git\index.lock — removing." -ForegroundColor Yellow
    try {
        Remove-Item $indexLock -Force
        Write-Host "  Removed." -ForegroundColor Green
    } catch {
        Write-Host "  Could NOT remove: $($_.Exception.Message)" -ForegroundColor Red
        Write-Host "  Close any IDE / Git GUI holding the index, then retry." -ForegroundColor Red
        throw
    }
}

# Also clean up any stray test-write file left behind by the audit sandbox
$strayTestWrite = Join-Path $repoRoot '.git\test-write'
if (Test-Path $strayTestWrite) {
    Write-Host "Found stray .git\test-write (audit artifact) — removing." -ForegroundColor Yellow
    Remove-Item $strayTestWrite -Force -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------------------
# Sanity: branch + HEAD
# ---------------------------------------------------------------------------
$branch = (git rev-parse --abbrev-ref HEAD).Trim()
$headShort = (git log -1 --format="%h %s").Trim()
Write-Host "Branch:     $branch"
Write-Host "HEAD:       $headShort"
Write-Host ""

# ---------------------------------------------------------------------------
# Tag collision check
# ---------------------------------------------------------------------------
if (-not $SkipTag) {
    $existingTag = (git tag -l 'v0.1.0-legacy-shell').Trim()
    if ($existingTag -eq 'v0.1.0-legacy-shell' -and -not $Force) {
        throw "Tag v0.1.0-legacy-shell already exists. Re-run with -Force to overwrite, or -SkipTag to just commit."
    }
}

# ---------------------------------------------------------------------------
# Files to stage (Phase 0 intentional edits only)
# ---------------------------------------------------------------------------
$files = @(
    # Phase 0 edits
    'README.md',
    'CONTRIBUTING.md',
    'app/README.md',
    'crates/nexus-app/src/lib.rs',
    # Decision record
    'docs/adr/0011-adopt-plugin-first-shell.md',
    # Companion docs + data
    'docs/INTEGRATION-REVIEW.md',
    'docs/INTEGRATION-ARCHITECTURE.html',
    'docs/Nexus-Integration-Architecture.docx',
    'docs/SHELL-COMPARISON.md',
    'docs/Shell-Capability-Comparison.xlsx',
    'docs/PARITY-CHECKLIST.md',
    'docs/Parity-Checklist.xlsx',
    # Helper scripts
    'scripts/phase-0-commit.sh',
    'scripts/phase-0-commit.ps1'
)

# Verify files exist
Write-Host "== Verifying files ==" -ForegroundColor Cyan
$missing = @()
foreach ($f in $files) {
    if (-not (Test-Path $f)) {
        $missing += $f
    }
}
if ($missing.Count -gt 0) {
    Write-Host "Missing files:" -ForegroundColor Red
    $missing | ForEach-Object { Write-Host "  - $_" }
    throw "Cannot proceed — $($missing.Count) file(s) missing."
}
Write-Host "All $($files.Count) files present." -ForegroundColor Green
Write-Host ""

# ---------------------------------------------------------------------------
# Stage
# ---------------------------------------------------------------------------
Write-Host "== Staging ==" -ForegroundColor Cyan
foreach ($f in $files) {
    git add -- $f
    if ($LASTEXITCODE -ne 0) {
        throw "git add failed for $f"
    }
}

# Show what's staged (path-scoped so CRLF drift stays invisible)
Write-Host "Staged (scoped to Phase 0 files):" -ForegroundColor Cyan
git status --short -- $files
Write-Host ""

if ($DryRun) {
    Write-Host "-DryRun specified — stopping before commit." -ForegroundColor Yellow
    Write-Host "To unstage: git reset HEAD -- $($files -join ' ')" -ForegroundColor Yellow
    exit 0
}

# ---------------------------------------------------------------------------
# Commit
# ---------------------------------------------------------------------------
Write-Host "== Committing ==" -ForegroundColor Cyan
$commitMsg = @"
Phase 0: freeze legacy shell, adopt plugin-first shell (ADR 0011)

Accept ADR 0011 (plugin-first shell as the single desktop target).
Add DEPRECATED banners to the legacy shell (app/ + crates/nexus-app).
Add CONTRIBUTING.md with the freeze policy. Land the integration
review, ADR, architecture diagram, Word companion, per-command
comparison matrix, and Phase 2 parity checklist (23 work items).

Tag v0.1.0-legacy-shell at this commit preserves the pre-freeze state.

Per CONTRIBUTING.md, new desktop capabilities land as service-crate
IPC handlers + a plugin in shell/src/plugins/nexus/, not as new
#[tauri::command] handlers in crates/nexus-app.

See docs/INTEGRATION-REVIEW.md and docs/PARITY-CHECKLIST.md.
"@

# Write commit message to a temp file so PowerShell quoting doesn't mangle it
$msgFile = Join-Path ([System.IO.Path]::GetTempPath()) ("nexus-commit-msg-" + [System.Guid]::NewGuid().ToString('N') + '.txt')
Set-Content -Path $msgFile -Value $commitMsg -NoNewline -Encoding UTF8
try {
    git commit -F $msgFile
    if ($LASTEXITCODE -ne 0) {
        throw "git commit failed. If a pre-commit hook fired, check its output above."
    }
} finally {
    Remove-Item $msgFile -Force -ErrorAction SilentlyContinue
}
Write-Host ""

# ---------------------------------------------------------------------------
# Tag
# ---------------------------------------------------------------------------
if (-not $SkipTag) {
    Write-Host "== Tagging v0.1.0-legacy-shell ==" -ForegroundColor Cyan
    $tagMsg = @"
Pre-freeze snapshot of the legacy Tauri desktop shell (app/ + crates/nexus-app).

Per ADR 0011 (docs/adr/0011-adopt-plugin-first-shell.md, Accepted
2026-04-23), the plugin-first shell at shell/ + shell/src-tauri
(crate nexus-shell) is the single desktop target going forward. This
tag preserves the legacy tree at its last unfrozen state.

Recovery: git checkout v0.1.0-legacy-shell -- app crates/nexus-app
to retrieve specific legacy files if needed during Phase 2 migration.
"@
    $tagMsgFile = Join-Path ([System.IO.Path]::GetTempPath()) ("nexus-tag-msg-" + [System.Guid]::NewGuid().ToString('N') + '.txt')
    Set-Content -Path $tagMsgFile -Value $tagMsg -NoNewline -Encoding UTF8
    try {
        $tagArgs = @('tag', '-a', 'v0.1.0-legacy-shell', '-F', $tagMsgFile)
        if ($Force) { $tagArgs += '-f' }
        & git @tagArgs
        if ($LASTEXITCODE -ne 0) {
            throw "git tag failed."
        }
    } finally {
        Remove-Item $tagMsgFile -Force -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "== Done ==" -ForegroundColor Green
$newHead = (git log -1 --format='%h %s').Trim()
Write-Host "Commit: $newHead"
if (-not $SkipTag) {
    $tagDesc = (git describe --tags --exact-match HEAD 2>$null)
    if ($tagDesc) {
        Write-Host "Tag:    $($tagDesc.Trim())"
    }
}
Write-Host ""
Write-Host "To push:" -ForegroundColor Cyan
Write-Host "  git push origin $branch"
if (-not $SkipTag) {
    Write-Host "  git push origin v0.1.0-legacy-shell"
}
