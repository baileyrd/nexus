#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(chat): session picker UI (PRD-12 §8)

Chat panel gains a <select> + 'New' / 'Delete' controls wired to
the com.nexus.ai::session_list / session_delete handlers added in
the previous commit. Active session id persists to localStorage so
the previous conversation re-opens on launch. First load falls back
to the legacy single-session file when multi-session is empty, then
migrates into chat/sessions/default.json on first save — existing
users don't lose their transcript during the rollout.

Titles are auto-derived from the first user turn (truncated to 48
chars) so the picker surfaces something useful without asking users
to name each session.

Microkernel invariant held: all state lives in com.nexus.ai over
ipc_call; ChatPanel only touches plugin APIs + localStorage for the
currently-active id.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
