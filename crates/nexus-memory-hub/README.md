# nexus-memory-hub

The central **sync hub** for Nexus memory: a standalone HTTP server that many
Nexus instances push their memories to and pull each other's from, so a single
memory store is shared across machines / implementations.

It mirrors the proven [`remind_me`] hub wire protocol but is backed by **SQLite**
(one file, no external database) — deploy is a single static binary plus a data
file. The wire protocol is storage-agnostic, so a Postgres backing could be
swapped in later without changing clients.

## Run

```sh
SYNC_SECRET=$(openssl rand -hex 32) \
  nexus-memory-hub --bind 0.0.0.0:8765 --db /var/lib/nexus-hub/hub.sqlite3
```

| Flag / env | Default | Meaning |
|------------|---------|---------|
| `--bind` | `127.0.0.1:8765` | `ip:port` to listen on |
| `--db` | `hub.sqlite3` | SQLite database path (created if absent) |
| `--secret` / `SYNC_SECRET` | — (required) | shared bearer token every client presents |

## HTTP surface

| Method & path | Auth | Purpose |
|---------------|------|---------|
| `GET /health` | none | liveness probe → `{ status, role, records }` |
| `POST /sync/push` | `Bearer <SYNC_SECRET>` | batch upsert (last-write-wins on `updated_at`) → `{ accepted, processed_ids, failed }` |
| `GET /sync/pull?since&since_id&exclude_node&limit` | `Bearer <SYNC_SECRET>` | keyset page newer than the `(since, since_id)` cursor → `{ records, count }` |

## Model

Records are stored opaquely by `id` with their `updated_at` (the LWW key + the
`(updated_at, id)` keyset cursor), the authoring `node_id`, the pushing
`origin_node` (hub-only; powers `exclude_node`, never returned), and the whole
record as a JSON `payload`. The hub understands none of the memory fields, so
new ones need no hub change. There is no node registry — any `node_id` with the
shared secret is accepted. Conflict resolution is last-write-wins on the
canonical ISO-8601-UTC `updated_at` string.

## Client (a Nexus instance)

Each Nexus instance syncs through its memory plugin's `sync` command
(`com.nexus.memory::sync`, also surfaced as **Memory: Sync Now** in the shell
and the `nexus_memory_sync` MCP tool). Config resolves from the call's args, or
these environment variables on the Nexus process:

| Env var | Meaning |
|---------|---------|
| `NEXUS_MEMORY_HUB_URL` | hub base URL, e.g. `http://127.0.0.1:8765` |
| `NEXUS_MEMORY_SYNC_SECRET` | the shared `SYNC_SECRET` |
| `NEXUS_MEMORY_NODE_ID` | this instance's stable node id |

Each sync runs one push (local memories newer than a keyset cursor, authored
here) + one pull (everyone else's, last-write-wins), resuming from cursors kept
in the forge's `memory.db`.

Deployment units (Containerfile / systemd) land in a follow-up change.

[`remind_me`]: https://github.com/baileyrd/remind_me
