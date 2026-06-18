# Nexus terminal shell integration for fish — emits OSC 133 semantic-prompt
# marks so the server-side VT grid can capture command / exit-code boundaries
# (RFC 0003). Sourced into a session's shell when it opts in. Adapted from
# baileyrd/rusty_term's extra/shell-integration/fish.fish; only OSC 133 is kept.
#
# Marks: A=prompt start, B=prompt end, C=command output start,
# D;<exit>=command finished.

if status is-interactive
    and not set -q __NEXUS_SHELL_INTEGRATION
        set -g __NEXUS_SHELL_INTEGRATION 1

        # C — a command line was submitted and is about to run.
        function __nexus_preexec --on-event fish_preexec
            printf '\e]133;C\a'
        end

        # D — the command finished; report its exit status.
        function __nexus_postexec --on-event fish_postexec
            printf '\e]133;D;%s\a' $status
        end

        # Wrap the existing fish_prompt with A (start) and B (end). Copy the
        # current definition once, then redefine fish_prompt to bracket it.
        if not functions -q __nexus_orig_fish_prompt
            functions -c fish_prompt __nexus_orig_fish_prompt
        end
        function fish_prompt
            printf '\e]133;A\a'
            __nexus_orig_fish_prompt
            printf '\e]133;B\a'
        end
end
