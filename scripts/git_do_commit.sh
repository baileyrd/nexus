#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(ai): multi-session chat storage (PRD-12 §8)

com.nexus.ai grows two append-only handlers:
  - session_list (10): enumerate .forge/chat/sessions/*.json
  - session_delete (11): remove one by id

session_load / session_save gain an optional 'id' arg. When supplied
the plugin routes to chat/sessions/<id>.json, otherwise the legacy
single-session path. Session ids are validated against
[A-Za-z0-9_-]{1,64} before touching the filesystem to block path
traversal through user input.

Tauri bridge exposes ai_session_list / ai_session_delete; TS helpers
aiSessionList / aiSessionDelete / aiSessionLoad(id?) consume them.
Chat panel UI still uses the legacy single-session path — a
session-picker follow-up exploits the new tree.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
