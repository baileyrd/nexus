# Nexus terminal shell integration for bash — emits OSC 133 semantic-prompt
# marks so the server-side VT grid can capture reliable command / exit-code
# boundaries (RFC 0003). Sourced into a session's shell when the session opts in
# (SessionConfig.shell_integration). Adapted from baileyrd/rusty_term's
# extra/shell-integration/bash.sh; only the OSC 133 marks are kept.
#
# Marks emitted: A=prompt start, B=prompt end / command start,
# C=command output start, D;<exit>=command finished.

# Only interactive bash, and only once.
if [ -z "$BASH_VERSION" ] || [[ $- != *i* ]] || [ -n "$__NEXUS_SHELL_INTEGRATION" ]; then
  return 0 2>/dev/null || true
fi
__NEXUS_SHELL_INTEGRATION=1

# D (just-finished command's exit status) — runs first in PROMPT_COMMAND, so $?
# is still the command's status. Also arms the next C.
__nexus_precmd() {
  local st=$?
  printf '\033]133;D;%s\007' "$st"
  __nexus_preexec_armed=1
}
case ";$PROMPT_COMMAND;" in
  *";__nexus_precmd;"*) ;;
  ";;") PROMPT_COMMAND="__nexus_precmd" ;;
  *) PROMPT_COMMAND="__nexus_precmd;$PROMPT_COMMAND" ;;
esac

# Wrap PS1 with A (prompt start) … B (prompt end). The \[ \] markers keep the
# escapes zero-width for bash's line-length accounting.
case "$PS1" in
  *'133;A'*) ;;
  *) PS1='\[\033]133;A\007\]'"$PS1"'\[\033]133;B\007\]' ;;
esac

# C (command output start) — the DEBUG trap fires before each command; emit only
# for the first real command after a prompt (not for PROMPT_COMMAND or completion).
__nexus_preexec_armed=0
__nexus_preexec() {
  [ -n "$COMP_LINE" ] && return
  [ "$BASH_COMMAND" = "$PROMPT_COMMAND" ] && return
  [ "$__nexus_preexec_armed" = 1 ] || return
  __nexus_preexec_armed=0
  printf '\033]133;C\007'
}
trap '__nexus_preexec' DEBUG
