# Nexus terminal shell integration for zsh — emits OSC 133 semantic-prompt marks
# so the server-side VT grid can capture command / exit-code boundaries (RFC
# 0003). Sourced into a session's shell when it opts in. Adapted from
# baileyrd/rusty_term's extra/shell-integration/zsh.sh; only OSC 133 is kept.
#
# Marks: A=prompt start, B=prompt end, C=command output start,
# D;<exit>=command finished.

# Interactive only, and only once.
[[ -o interactive ]] || return 0
[[ -n "$__NEXUS_SHELL_INTEGRATION" ]] && return 0
__NEXUS_SHELL_INTEGRATION=1

autoload -Uz add-zsh-hook

# precmd runs just before the prompt is drawn: report the previous command's
# exit status (D), then open the new prompt (A).
__nexus_precmd() {
  local st=$?
  print -n "\e]133;D;${st}\a"
  print -n "\e]133;A\a"
}

# preexec runs after the user submits a line, before it executes (C).
__nexus_preexec() {
  print -n "\e]133;C\a"
}

add-zsh-hook precmd __nexus_precmd
add-zsh-hook preexec __nexus_preexec

# Close the prompt (B) at the very end of PROMPT. %{ %} keep it zero-width.
if [[ "$PROMPT" != *'133;B'* ]]; then
  PROMPT="${PROMPT}%{"$'\e]133;B\a'"%}"
fi
