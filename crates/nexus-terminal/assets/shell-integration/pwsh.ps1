# Nexus terminal shell integration for PowerShell — emits OSC 133 semantic-prompt
# marks so the server-side VT grid can capture command / exit-code boundaries
# (RFC 0003). Sourced into a session's shell when it opts in. Adapted from
# baileyrd/rusty_term's extra/shell-integration/pwsh.ps1; only OSC 133 is kept.
#
# Marks: A=prompt start, B=prompt end, D;<exit>=command finished. (PowerShell
# has no portable pre-execution hook, so C is omitted.)

if ($Global:__NexusShellIntegration) { return }
$Global:__NexusShellIntegration = $true

# Preserve the user's existing prompt so we only wrap it.
$Global:__NexusOriginalPrompt = $function:prompt

function global:prompt {
    $exit = $LASTEXITCODE
    if ($null -eq $exit) { $exit = 0 }
    $esc = [char]27
    $bel = [char]7
    # D (previous command's exit status) then A (prompt start).
    [Console]::Write("$esc]133;D;$exit$bel$esc]133;A$bel")
    $rendered = & $Global:__NexusOriginalPrompt
    # B (prompt end) after the user's prompt text.
    [Console]::Write("$esc]133;B$bel")
    $rendered
}
