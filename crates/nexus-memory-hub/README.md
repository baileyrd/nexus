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

The matching Nexus-side sync engine (outbox + push/pull loop) and deployment
units (Containerfile / systemd) land in follow-up changes.

[`remind_me`]: https://github.com/baileyrd/remind_me
