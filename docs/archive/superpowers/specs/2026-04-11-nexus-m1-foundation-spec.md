# Nexus M1 (Foundation) — Implementation Spec

**Version:** 0.1
**Date:** 2026-04-11
**Status:** Approved (brainstorming session output)
**Scope:** Implementation contract for Milestone 1 of the Nexus v0.1 roadmap. Captures decisions from the M1 brainstorming session that the underlying PRDs left ambiguous. This doc + the PRDs together are the input to the writing-plans skill that will produce M1's task-level implementation plan.

**Parent doc:** [`2026-04-11-nexus-roadmap-design.md`](./2026-04-11-nexus-roadmap-design.md)

---

## 1. Frame

### What M1 covers

PRDs 01–05 + 04a, in the slimmed form defined by Section 3 of the roadmap:

- **01-kernel-event-system.md** — microkernel, event bus, plugin lifecycle, capability system
- **02-security-model.md** *(slimmed)* — capability enforcement, WASM sandbox, slimmed audit, keyring; cuts: sync/replication threats, full audit subsystem, plugin code review workflows, credential key rotation
- **03-storage-engine.md** *(CRDT sync deferred)* — file-as-truth, SQLite index, file watcher, Tantivy search
- **04-plugin-system.md** *(slimmed)* — WASM sandbox, capability gating, manifest, settings UI; cuts: marketplace, community registry, plugin discovery UI, ratings, runtime vendor justification
- **04a-plugin-templates.md** *(both templates kept)* — `cargo-generate` templates for core + community plugins
- **05-cli.md** — `nexus` binary, output formatters, completions, headless, watch mode

### What M1 does NOT cover

- Anything from M2–M5 (editor, themes, terminal, database, git, AI, agents, workflows)
- Cross-platform support (M6, deferred indefinitely)
- Performance optimization beyond PRD acceptance criteria
- The cut sections from PRDs 02 and 04 — these stay on disk in the PRDs as deferred reference
- Plugin signing verification (deferred to v0.2)

### Operating constraints (from roadmap)

- Solo developer + AI agents
- Personal tool framing (single user, single machine)
- Strict PRD order *across* phases; within-phase parallelism allowed where the dependency graph permits
- Inter-PRD contracts (roadmap §5.1) treated as load-bearing — every public type/trait/function listed in this doc is the contract

---

## 2. Project Structure

### Cargo workspace layout

```
nexus/
├── Cargo.toml                  # workspace root
├── rust-toolchain.toml         # pin MSRV
├── crates/
│   ├── nexus-types/            # shared structs used across crates and plugins
│   ├── nexus-kernel/           # PRD 01: event bus, plugin lifecycle, context, KV, capabilities
│   ├── nexus-security/         # PRD 02: keyring, audit log, capability risk metadata
│   ├── nexus-storage/          # PRD 03: file watcher, SQLite index, Tantivy search, comrak
│   ├── nexus-plugins/          # PRD 04 + 04a: manifest parser, WASM sandbox, loader, hot-reload, settings UI
│   └── nexus-cli/              # PRD 05: nexus binary, formatters, subcommand dispatch
├── tests/
│   ├── acceptance/             # per-PRD acceptance tests
│   │   ├── prd-01-kernel/
│   │   ├── prd-02-security/
│   │   ├── prd-03-storage/
│   │   ├── prd-04-plugins/
│   │   ├── prd-04a-templates/
│   │   └── prd-05-cli/
│   ├── integration/m1/         # cross-PRD integration tests (7 categories I1–I7)
│   └── fixtures/
│       └── m1-acceptance-plugin/   # test plugin built from PRD 04a community template
├── docs/
│   ├── adr/                    # ADRs (cross-cutting concern §5.2 of roadmap)
│   └── superpowers/specs/      # design docs (this file)
└── PRDs/                       # the source PRDs
```

### Crate naming and conventions

- **Crate names:** kebab-case (`nexus-kernel`, `nexus-security`, etc.)
- **Module paths in code:** snake_case (`use nexus_kernel::EventBus;`)
- **Workspace dependencies in root `Cargo.toml`:** all third-party deps listed in `[workspace.dependencies]` with pinned versions; per-crate `Cargo.toml` files reference `workspace = true`
- **MSRV:** latest stable Rust at M1 start (verify at start of M1 work, currently 1.78 as of 2026-04-11). Pin in `rust-toolchain.toml`

### Dependency graph (DAG, no cycles)

```
nexus-cli ──> nexus-plugins ──> nexus-kernel ──> nexus-types
        ╲             ╲              ↑
         ╲             ╲             │
          ╲             └──> nexus-security
           ╲                       ↑
            └────> nexus-storage ──┘
```

- `nexus-types` is the single leaf — depended on by everyone, depends on nothing in the workspace.
- `nexus-kernel` depends only on `nexus-types`.
- `nexus-security` depends on `nexus-kernel` (for `Capability` enum) + `nexus-types`. **Does NOT depend on `nexus-plugins`.** Capability enforcement is mediated by the kernel.
- `nexus-storage`, `nexus-plugins` both depend on `nexus-kernel`, `nexus-security`, `nexus-types`.
- `nexus-cli` depends on every other Nexus crate (it's the orchestration layer).

This is a strict DAG. Any agent edit that introduces a cycle must fail in code review.

---

## 3. Tech Stack (locked dependencies)

All versions pinned in workspace root `Cargo.toml`. Bumps require an ADR.

| Concern | Crate | Version | Notes |
|---|---|---|---|
| Async runtime | `tokio` | 1.35+ | Full features. No abstraction over runtimes. |
| Logging | `tracing` + `tracing-subscriber` | 0.1 / 0.3 | Use everywhere. `tracing-log` shim bridges deps that use `log`. |
| Log file output | `tracing-appender` | 0.2 | Daily rolling file at `<forge>/.nexus/logs/nexus-YYYY-MM-DD.log` |
| TOML read | `toml` | 0.8 | Manifest parsing |
| TOML write | `toml_edit` | 0.22 | Used by `nexus forge config set` to preserve user comments |
| CLI parser | `clap` | 4.4+ (pin major) | `derive` feature; no `cargo` feature (we don't ship to crates.io as a CLI library) |
| SQLite | `rusqlite` | 0.30+ | `bundled` feature — ships SQLite with binary, no system library |
| SQLite pool | `r2d2` + `r2d2_sqlite` | 0.12 / 0.23 | Default pool size 4 |
| WASM runtime | `wasmtime` | 18.0 | `cranelift` + `async` features; no JIT cache for v0.1 |
| File watcher | `notify` + `notify-debouncer-mini` | 6.1+ / 0.4+ | Debounce 300ms |
| Markdown parser | `comrak` | 0.21+ | GFM extensions enabled per PRD 03 §6 |
| Full-text search | `tantivy` | 0.21+ | English tokenizer + stemming filter |
| CRDT | *(deferred — sync cut from M1)* | — | If revived: `automerge` 0.5+ |
| JSON Schema validator | `jsonschema` | 0.17+ | For plugin settings UI schemas |
| Cryptography (AEAD) | `ring` | 0.17 | For credential encryption-at-rest fallback (currently unused since keyring is hard-fail) |
| Cryptography (signatures) | *(deferred — signing cut from M1)* | — | If revived: `ed25519-dalek` 2.0 |
| TLS | `rustls` + `rustls-native-certs` | 0.22 / 0.7 | Pure-Rust, avoids OpenSSL system dep |
| Keyring | `keyring` (`keyring-rs`) | 2.3 | NOT `keytar` (which PRD 02 named, but is unmaintained Node-era). Hard-fail if unavailable (see §10.3) |
| Table formatting | `comfy-table` | 7.1 | CLI table output |
| Pager | `pager` | 0.16 | Shell out to `$PAGER` or `less` |
| Progress indicators | `indicatif` | 0.17 | Spinners + progress bars |
| Plugin scaffolding | `cargo-generate` | 0.20+ | PRD 04a templates |
| Test runner | `nextest` | 0.9+ | Replaces `cargo test` for the workspace |
| Async test macro | `tokio::test` | (in `tokio`) | For async tests |
| Benchmarking | *(deferred — YAGNI for v0.1)* | — | If revived: `criterion` 0.5 |
| Error handling (libs) | `thiserror` | 1.0 | Used in every crate except `nexus-cli` |
| Error handling (binary) | `anyhow` | 1.0 | Used only in `nexus-cli` for the binary entry point |
| Async traits | `async-trait` | 0.1 | Until native async trait tooling stabilizes |
| Serialization | `serde` + `serde_json` | 1.0 | Universal |
| Parallelism | `rayon` | 1.8 | Used in storage indexing (parallel file hashing) |

### MSRV policy
Personal tool — pin to latest stable, bump as needed without ceremony. Document each bump in an ADR (~5 lines).

---

## 4. Capability Model

### Format: hierarchical dot-namespaced strings

Capabilities are referenced in plugin manifests as dot-namespaced strings. Each string maps bidirectionally to a `Capability` enum variant in `nexus-kernel`. The enum is the single source of truth; manifests are validated against it at parse time.

### M1 capability set

```rust
// nexus-kernel/src/capabilities.rs
pub enum Capability {
    // File system
    FsRead,                  // "fs.read"            — read files within forge_root
    FsWrite,                 // "fs.write"           — write files within forge_root
    FsReadExternal,          // "fs.read.external"   — read files outside forge_root (HIGH risk)
    FsWriteExternal,         // "fs.write.external"  — write files outside forge_root (HIGH risk)

    // Network
    NetHttp,                 // "net.http"           — outbound HTTP
    NetHttpLocalhost,        // "net.http.localhost" — localhost-only HTTP

    // Process
    ProcessSpawn,            // "process.spawn"      — spawn child processes (HIGH risk)

    // KV store
    KvRead,                  // "kv.read"            — read plugin's KV store
    KvWrite,                 // "kv.write"           — write plugin's KV store

    // IPC
    IpcCall,                 // "ipc.call"           — call other plugins via IPC

    // Database (used by plugins that want SQLite access — gated for M3+ but defined now)
    DbQuery,                 // "db.query"           — query SQLite tables registered by the plugin
    DbWrite,                 // "db.write"           — write to plugin-registered SQLite tables
}
```

### Rules

1. **No wildcards in manifests.** Every capability listed explicitly. `"fs.*"` is not a valid manifest entry.
2. **Capability strings must exactly match `Capability::as_str()`.** Typos (`"fs_read"` instead of `"fs.read"`) fail at manifest parse time with a clear error pointing to the line.
3. **Capability checks are enforced inside the kernel's `PluginContext` impl** (see §6.3). Plugins cannot bypass the check because the only handle they hold is the trait.
4. **Risk levels are metadata in `nexus-security`**, not in the enum itself:
   ```rust
   // nexus-security/src/capability_risk.rs
   pub fn risk_level(cap: Capability) -> RiskLevel {
       match cap {
           Capability::FsRead | Capability::KvRead | Capability::KvWrite => RiskLevel::Low,
           Capability::FsWrite | Capability::NetHttpLocalhost | Capability::DbQuery
                                                              | Capability::DbWrite => RiskLevel::Medium,
           Capability::FsReadExternal | Capability::FsWriteExternal
                                      | Capability::NetHttp
                                      | Capability::ProcessSpawn
                                      | Capability::IpcCall => RiskLevel::High,
       }
   }
   ```
5. **Plugin trust levels** (`core` vs `community`, from PRD 04a) gate which capabilities a plugin can declare:
   - **Core plugins:** any capability allowed
   - **Community plugins:** any LOW or MEDIUM capability allowed; HIGH capabilities require explicit user approval at install time (`nexus plugin install` prompts on each HIGH capability)
   - In M1, the prompt is a CLI yes/no (`nexus plugin install <path>` shows the capability list and asks for confirmation). M2 will add a GUI variant.
6. **No cryptographic signing in M1.** Trust level is declared by the plugin author in the manifest, not verified. Signing is deferred to v0.2.

---

## 5. Event Taxonomy

### Closed enum + single `Custom` variant for plugin events

```rust
// nexus-kernel/src/event.rs
pub enum NexusEvent {
    // M1: kernel/storage events
    FileCreated   { path: PathBuf, content_hash: String },
    FileModified  { path: PathBuf, content_hash: String },
    FileDeleted   { path: PathBuf },
    FileRenamed   { from: PathBuf, to: PathBuf, content_hash: String },

    // M1: plugin lifecycle
    PluginLoaded  { plugin_id: String, version: String },
    PluginStarted { plugin_id: String },
    PluginStopped { plugin_id: String, reason: StopReason },
    PluginCrashed { plugin_id: String, error: String },

    // M1: capability lifecycle
    CapabilityGranted { plugin_id: String, capability: Capability },
    CapabilityDenied  { plugin_id: String, capability: Capability },

    // M1: indexing lifecycle
    IndexingStarted   { total_files: usize },
    IndexingProgress  { files_processed: usize, total_files: usize },
    IndexingCompleted { duration_ms: u64 },

    // Plugin-emitted custom events (any phase)
    Custom {
        type_id: String,        // namespaced: must start with emitting plugin's id
        emitting_plugin: String, // set by the kernel, not the plugin (anti-spoofing)
        payload: serde_json::Value,
    },

    // Future-phase placeholder — variants added per-phase by their owning crates
    // M2 adds: EditorBufferOpened, EditorBufferSaved, ThemeChanged, ...
    // M3 adds: TerminalSpawned, GitCommitCreated, DatabaseQueryExecuted, ...
    // M4 adds: AiQueryStarted, AiResponseChunk, SkillActivated, McpToolInvoked, ...
    // M5 adds: AgentTaskStarted, WorkflowTriggered, WorkflowStepCompleted, ...
}

pub enum StopReason {
    UserRequested,
    HotReload,
    Shutdown,
    CrashRecovery,
}
```

### Rules

1. **Each phase adds new variants by editing `nexus-kernel`.** Pattern matches stay exhaustive; a new event type forces compile errors in any subscriber that hasn't been updated.
2. **Plugins cannot emit kernel events.** Plugins can only emit `NexusEvent::Custom`. The `emitting_plugin` field is set by the kernel from the calling plugin's identity — plugins cannot spoof other plugins.
3. **The `type_id` in custom events must start with the emitting plugin's id**, by reverse-DNS convention. Example: a plugin with `id = "com.example.weather"` can emit `"com.example.weather.forecast.updated"` but not `"system.shutdown"`. Enforced at emit time.
4. **Subscriptions filter by either kernel-event variant OR custom-event type_id prefix:**
   ```rust
   ctx.subscribe(EventFilter::Variant("FileModified"));
   ctx.subscribe(EventFilter::CustomPrefix("com.example.weather."));
   ctx.subscribe(EventFilter::CustomExact("com.example.weather.forecast.updated"));
   ctx.subscribe(EventFilter::All);  // for debugging/tracing only
   ```
5. **Bounded broadcast channel: 2048 events.** Slow subscribers receive a `Lagged` notification (tokio broadcast semantics) and lose intermediate events rather than blocking the bus. Configurable later in `nexus.toml` if needed.
6. **No event history.** The brainstorm fuel R3 worry about unbounded growth is answered by the bounded channel: if a subscriber is too slow, they lose events. There is no replay buffer in M1.

---

## 6. Plugin Contract Surface

### 6.1 Manifest format

Per PRD 04 §1.1, with M1-specific clarifications:

```toml
# manifest.toml
[plugin]
id = "com.example.weather"          # reverse-DNS, validated by regex from PRD 04 §1.2
name = "Weather"
version = "0.1.0"
trust_level = "community"           # "core" or "community"
api_version = "1"                   # nexus plugin contract version

[capabilities]
required = ["fs.read", "kv.read", "kv.write", "net.http"]
# Strict-only — no wildcards. Every cap explicit.

[wasm]
module = "weather.wasm"
memory_mb = 16
fuel = 10000000                     # wasmtime fuel for execution limits

[settings]
schema = "settings.json"            # JSON Schema v7 file in plugin dir
# kept per user override — drives the Plugin Settings UI

[[registrations.cli_subcommand]]
id = "weather.forecast"
handler_id = 1
description = "Get weather forecast for a location"
usage = "weather forecast <LOCATION>"

[[registrations.ipc_command]]
id = "weather.get-current"
handler_id = 100

[[registrations.event_subscriber]]
id = "weather.on-file-created"
filter = { Variant = "FileCreated" }
handler_id = 200

[lifecycle]
on_init = true
on_start = true
on_stop = true
```

**Manifest validation rules** (enforced by `nexus-plugins::manifest::validate`):
- Plugin ID matches regex `^[a-z0-9]+([-._][a-z0-9]+)*\.[a-z0-9]+([-._][a-z0-9]+)*$` (PRD 04 §1.2)
- All capability strings must exist in the `Capability` enum
- All `handler_id` values must be unique within the manifest
- `wasm.memory_mb` must be in `[1, 256]`
- `wasm.fuel` must be > 0 (or omitted to mean unlimited — only allowed for `trust_level = "core"`)
- `settings.schema` must point to a file that exists and is a valid JSON Schema v7

### 6.2 Lifecycle hooks

```rust
// nexus-kernel/src/plugin.rs
#[async_trait]
pub trait PluginLifecycle: Send + Sync {
    async fn on_init(&mut self, ctx: &PluginContext)  -> Result<()>;
    async fn on_start(&mut self, ctx: &PluginContext) -> Result<()>;
    async fn on_stop(&mut self, ctx: &PluginContext)  -> Result<()>;
}
```

State machine: `Loaded → Initialized → Running → Stopped → (Reload? → Loaded) | Unloaded`. PRD 01 §4.1 has the full state diagram.

### 6.3 PluginContext API

```rust
// nexus-kernel/src/context.rs
#[async_trait]
pub trait PluginContext: Send + Sync {
    // Identity
    fn plugin_id(&self) -> &str;

    // Capability check (called internally by every other method)
    fn has_capability(&self, cap: Capability) -> bool;

    // File system (gated by fs.* capabilities)
    async fn read_file(&self, path: &Path)  -> Result<Vec<u8>>;
    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()>;
    async fn delete_file(&self, path: &Path) -> Result<()>;
    async fn list_files(&self, dir: &Path)   -> Result<Vec<PathBuf>>;

    // KV (gated by kv.read/write)
    async fn kv_get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    async fn kv_set(&self, key: &str, value: &[u8]) -> Result<()>;
    async fn kv_delete(&self, key: &str) -> Result<()>;

    // Events
    fn publish(&self, event: NexusEvent) -> Result<()>;     // Custom events only; plugins can't publish kernel events
    fn subscribe(&self, filter: EventFilter) -> EventSubscription;

    // IPC (gated by ipc.call)
    async fn ipc_call(&self, target_plugin_id: &str, command_id: &str, args: serde_json::Value)
                     -> Result<serde_json::Value>;

    // Logging (always allowed; plumbed through tracing)
    fn log(&self, level: LogLevel, message: &str);
}
```

**Capability enforcement is inside the impl, not at the call site.** Example (sketch):

```rust
// nexus-kernel/src/context_impl.rs
impl PluginContext for KernelPluginContext {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        if !self.capabilities.contains(Capability::FsRead) {
            self.security.audit_capability_denied(&self.plugin_id, Capability::FsRead);
            self.bus.publish(NexusEvent::CapabilityDenied { /* ... */ })?;
            return Err(Error::CapabilityDenied(Capability::FsRead));
        }
        self.security.audit_capability_used(&self.plugin_id, Capability::FsRead);
        self.storage.read_file(path).await
    }
    // ...
}
```

The plugin holds only `&dyn PluginContext`; there is no other code path that reads files on the plugin's behalf.

### 6.4 Calling convention: single dispatch + handler IDs

A WASM plugin exports exactly one function:

```rust
// generated by the plugin SDK macro in PRD 04a templates
#[no_mangle]
pub extern "C" fn nexus_dispatch(handler_id: u32, args_ptr: u32, args_len: u32) -> u64 {
    // 1. Read JSON args from linear memory at args_ptr/args_len
    // 2. Look up handler_id in the plugin's local dispatch table
    // 3. Call the handler with deserialized typed args
    // 4. Serialize result to JSON
    // 5. Write result back to linear memory and return packed (ptr<<32 | len)
}
```

- **Wire format:** JSON, via `serde_json`. Shared types live in `nexus-types` so the host and plugins use the same structs for events, file metadata, capability values, etc.
- **Handler IDs are namespaced:**
  - `0x01_xx_xx_xx` — CLI subcommand handlers
  - `0x02_xx_xx_xx` — IPC handlers
  - `0x03_xx_xx_xx` — event subscriber handlers
  - `0x04_xx_xx_xx` — settings UI handlers (validate, render hints)
- **The PRD 04a SDK macro** generates the dispatch function from `#[handler(id = N)]` attributes on the plugin's Rust functions, hiding the boilerplate.
- **Errors** are JSON-encoded with a known error envelope: `{"error": {"kind": "..." , "message": "..."}}`. The host translates them back to typed errors.

### 6.5 State persistence across hot-reload

KV-backed, plugin-managed, opt-in. Plugins that want state survival store to `ctx.kv_set("state", ...)` in `on_stop` and restore from `ctx.kv_get("state")` in `on_init`. No special kernel mechanism beyond the existing KV API.

PRD 04a templates include a commented-out example of the pattern in the community template; the core template stays minimal.

### 6.6 Hot-reload mechanism

- The plugin loader watches the plugin's WASM file via `notify` (separate `notify` instance from the storage watcher — the plugin loader's watch path is `<plugin-dir>/*.wasm`).
- On change: call `on_stop` on old instance with `StopReason::HotReload` → drop old wasmtime instance → load new WASM bytes → call `on_init` on new instance → call `on_start`.
- During the swap, IPC calls and subscribed event delivery to the plugin are queued (bounded queue, 256 messages); if the queue fills, new calls return `Error::PluginReloading`.
- Hot-reload is enabled by default in M1; can be disabled in `nexus.toml` for production-style "load once" behavior.

---

## 7. Storage Architecture

### 7.1 File watcher → event bus integration

Storage owns the watcher (per Q3, Option A). Flow:

```
notify (raw OS events)
   ↓
notify-debouncer-mini (300ms window)
   ↓
nexus-storage::Watcher
   ├─ rename detection via content hash
   ├─ ignore patterns (.git/, .nexus/, *~, .DS_Store, .swp)
   ↓
nexus-kernel::EventBus.publish(FileCreated | FileModified | FileDeleted | FileRenamed)
```

Rename detection: when a `Deleted` event is followed within the debounce window by a `Created` event whose content hash matches, emit a single `FileRenamed { from, to, content_hash }` event instead of `FileDeleted` + `FileCreated`.

### 7.2 Indexing

- SQLite index per PRD 03 §4–5
- `r2d2` connection pool, default size 4 (configurable in `nexus.toml`)
- WAL mode enabled (`PRAGMA journal_mode = WAL`)
- Concurrent readers OK; writers serialized behind a `Mutex<Connection>` write-side wrapper for now (the brainstorm fuel R2 concern about concurrent writes is acknowledged as a v0.2 optimization — not blocking M1)
- Initial index build emits `IndexingStarted`, `IndexingProgress` (every 500 files OR every 5s, whichever comes first), `IndexingCompleted` events to the kernel bus
- Parallel file hashing via `rayon` per PRD 03 §3
- Markdown parsed by `comrak` 0.21+ with GFM extensions enabled per PRD 03 §6

### 7.3 Search

- Tantivy 0.21+ index alongside the SQLite index
- Tokenizer: `default` (English text + simple lowercase) + English stemming filter
- Search query syntax: lucene-like, per Tantivy's `QueryParser`
- Schema: doc fields are `path` (stored), `content` (indexed), `block_id` (stored), `block_type` (stored, faceted), `mtime` (indexed)
- Index rebuild is full at M1 (incremental delta updates are a v0.2 optimization)

### 7.4 Storage public traits (the contract)

`StorageProvider`, `IndexProvider`, `SearchProvider` per PRD 03 §11. `SyncProvider` is defined but not instantiated in M1 (CRDT sync deferred).

---

## 8. CLI Architecture

### 8.1 Binary entry + subcommand structure

The `nexus` binary, parsed via `clap` 4.4+ derive macros. Subcommand groups per PRD 05 §1.2 (with the `forge`→`nexus` rename applied):

```
nexus forge <SUBCOMMAND>       # forge management (init, open, config, status)
nexus content <SUBCOMMAND>     # content CRUD (create, read, edit, delete, search)
nexus db <SUBCOMMAND>          # *(M3+, not in M1)*
nexus plugin <SUBCOMMAND>      # plugin install/list/call/uninstall/scaffold
nexus ai <SUBCOMMAND>          # *(M4+, not in M1)*
nexus proc <SUBCOMMAND>        # *(M3+, not in M1)*
nexus term <SUBCOMMAND>        # *(M3+, not in M1)*
nexus mcp <SUBCOMMAND>         # *(M4+, not in M1)*
nexus sync <SUBCOMMAND>        # *(deferred from v0.1)*
nexus git <SUBCOMMAND>         # *(M3+, not in M1)*
nexus run <SCRIPT>             # *(M5+, not in M1)*
nexus watch <GLOB>             # M1 — watch mode automation
nexus logs <SUBCOMMAND>        # M1 — tail and show audit/debug logs
```

**M1 implements only the subcommand groups marked M1.** Other groups are scaffolded as `clap` subcommand stubs that print "Not yet implemented (planned for M3)" and exit with code 1, so the surface is visible but the work isn't done.

### 8.2 Plugin extension mechanism

Plugins register CLI subcommands via the manifest's `[[registrations.cli_subcommand]]`. At startup, `nexus-cli`:

1. Asks `nexus-plugins` for the list of all loaded plugins and their CLI registrations
2. Builds dynamic clap subcommands at runtime (clap supports this via `Command::subcommand`)
3. When a plugin-registered subcommand is invoked, dispatches via `nexus-plugins` to the plugin's `nexus_dispatch` function with the corresponding `handler_id`

If two plugins register the same subcommand id, the kernel rejects the second plugin with `Error::DuplicateCliSubcommand` (the brainstorm fuel R1 concern about naming collisions is answered by a hard-fail policy).

### 8.3 Output formatters

Four formats per PRD 05 §2: `text` (default, ANSI color), `json`, `jsonl`, `table`. Selected via global `--format` flag or per-command override. Implementations:

- `text` — hand-rolled per command
- `json` — `serde_json` of a typed result struct
- `jsonl` — same as `json` but newline-delimited per record
- `table` — `comfy-table` rendering of typed result structs

A `Formatter` trait in `nexus-cli::output` lets each command implement its own formatter for each output type. Plugin-extended commands can supply their own formatters via the dispatch return value.

### 8.4 Pager + progress

- Pager auto-engaged on text output longer than terminal height when stdout is a TTY. `--no-pager` disables.
- Progress indicators via `indicatif` for any operation with a known total (indexing, plugin install). Suppressed in non-TTY mode.

---

## 9. Security Model (slimmed)

### 9.1 What survives the cut

- Threat model documentation (PRD 02 §2, single-user variant)
- WASM sandbox with capability gating (PRD 02 §3)
- Capability risk-level metadata (PRD 02 §4, in `nexus-security`)
- Keyring-based credential storage (PRD 02 §6, with hard-fail policy from §10.3)
- Slimmed audit logging (§9.2 below)
- Plugin trust levels in manifests (PRD 04a §3.1–3.2)

### 9.2 Audit logging — what survives the cut

- All capability grants and denials logged via `tracing::info!` with structured fields: `plugin_id`, `capability`, `result`, `timestamp`, `correlation_id`
- All plugin lifecycle transitions logged at `info` level
- File system access logged at `debug` level (off by default; enable via `RUST_LOG=nexus_security::audit=debug`)
- Output to a single rolling log file at `<forge>/.nexus/logs/nexus-YYYY-MM-DD.log`, written by `tracing-appender`. New file per day.
- **No** rotation policy beyond daily files. **No** compression. **No** JSONL exporter. **No** merkle tamper-detection.
- CLI access: `nexus logs tail [--level info]` and `nexus logs show <date>` and `nexus logs path`. That's it. No querying language.

### 9.3 Keyring fallback policy: hard-fail

If `keyring-rs` cannot access the OS keychain (no D-Bus on Linux, locked Keychain on macOS, etc.), Nexus refuses to start. The error message points the user at platform-specific setup docs:

```
Error: cannot access OS keychain.
  Reason: <keyring-rs error>
  On Linux, ensure D-Bus and a Secret Service provider (e.g., gnome-keyring or KWallet) are running.
  On macOS, ensure Keychain Access is unlocked.
  On Windows, ensure Credential Manager is accessible.

To bypass keyring entirely (NOT RECOMMENDED — credentials will be unavailable):
  NEXUS_NO_KEYRING=1 nexus <command>
```

`NEXUS_NO_KEYRING=1` disables credential operations entirely (any plugin requesting credentials gets an error). It exists as an escape hatch, not a fallback.

### 9.4 Plugin trust levels

- `trust_level = "core"`: any capability allowed without prompt
- `trust_level = "community"`: LOW/MEDIUM capabilities allowed without prompt; HIGH capabilities require user confirmation at install time via CLI yes/no prompt:

```
$ nexus plugin install ./weather-plugin
Plugin: com.example.weather v0.1.0
Trust level: community
Capabilities requested:
  ✓ fs.read           (LOW)    — read files within forge
  ✓ kv.read, kv.write (LOW)    — read/write plugin's KV store
  ! net.http          (HIGH)   — outbound HTTP

The HIGH-risk capability `net.http` requires your approval.
Approve? [y/N]
```

- No cryptographic signature verification. Trust level is declared, not verified. Signing is deferred to v0.2.

---

## 10. Cross-Cutting Conventions

### 10.1 Logging
`tracing` everywhere. Structured fields: prefer `tracing::info!(plugin_id = %id, capability = ?cap, "granted")` over string interpolation. Spans for multi-step operations (`#[instrument]` on async functions). Audit-level events at `info`, debug at `debug`, errors at `error`. No `println!` outside the CLI's user-facing output paths.

### 10.2 Error handling
- Library crates (`nexus-kernel`, `nexus-security`, `nexus-storage`, `nexus-plugins`, `nexus-types`): typed errors via `thiserror`. Each crate has an `Error` enum.
- Binary crate (`nexus-cli`): `anyhow::Result` for the binary entry point, with `.context()` annotations as errors propagate.
- No `unwrap()` or `expect()` in production code paths. Tests can use them freely.

### 10.3 Async convention
- All public async functions return `Result<T, ThiscrateError>`.
- Use `#[async_trait]` for trait definitions until native async traits are well-supported.
- Avoid `block_on` outside the binary entry point.

### 10.4 ADRs
Per roadmap §5.2, every non-trivial decision in M1 gets an ADR file in `docs/adr/NNNN-short-title.md`. Initial ADRs to write at the start of M1:

- `0001-cargo-workspace-with-prd-crates.md` (Q1)
- `0002-hierarchical-capability-strings.md` (Q2)
- `0003-storage-owns-file-watcher.md` (Q3)
- `0004-crate-boundaries-and-ownership.md` (Q4)
- `0005-single-dispatch-handler-ids.md` (Q5)
- `0006-kv-backed-plugin-state.md` (Q6)
- `0007-closed-event-enum-with-custom-variant.md` (Q7)
- `0008-tech-stack-defaults.md` (Q8 Part A)
- `0009-keyring-hard-fail-policy.md` (Q8 Part B2)
- `0010-no-plugin-signing-in-m1.md` (Q8 Part B3)

Each ADR is ~10 lines: context, decision, alternatives considered, consequences.

---

## 11. Testing Strategy

### 11.1 Per-PRD acceptance tests (Layer 1)

Each M1 PRD's acceptance criteria are translated into Rust integration tests in `tests/acceptance/<prd-number>/`. The PRDs already define the criteria; this layer just runs them. **No new content from the brainstorm needed for this layer.**

### 11.2 Cross-PRD integration tests (Layer 2)

Seven test categories in `tests/integration/m1/`. Each is a self-contained test suite verifying one cross-PRD seam.

| # | Suite | What it verifies |
|---|---|---|
| **I1** | `kernel_event_bus` | Publish/subscribe round-trip; broadcast `Lagged` notification; subscription filtering by variant + custom prefix |
| **I2** | `storage_to_event_bus` | Created/Modified/Deleted/Renamed events emitted correctly; debounce 300ms ±50ms; rename hash detection |
| **I3** | `plugin_lifecycle` | Manifest parse → load → on_init → on_start → on_stop → state persisted to KV → reload → on_init restores state |
| **I4** | `capability_enforcement` | Plugin without `fs.read` is denied; `CapabilityDenied` event published; tracing log entry written; plugin WITH cap succeeds |
| **I5** | `cli_to_kernel` | All M1 subcommands run, exit codes correct, `--format json` parseable |
| **I6** | `plugin_extends_cli` | Test plugin registers a CLI subcommand; `nexus <subcommand>` invokes the plugin's dispatch with serialized args; result returned |
| **I7** | `m1_walking_skeleton` | The end-to-end smoke test (§11.3) |

### 11.3 The walking-skeleton smoke test (the M1 gate)

If this passes, M1 is shippable. Every M1 architectural decision is exercised.

```bash
$ nexus forge init my-test-forge
✓ Forge initialized: my-test-forge
$ cd my-test-forge

$ nexus plugin install ../tests/fixtures/m1-acceptance-plugin
✓ Plugin loaded: com.nexus.m1-acceptance v0.1.0
  Capabilities granted: fs.read, kv.read, kv.write

$ nexus content create welcome.md "# Welcome"
✓ Created welcome.md (12 bytes)

$ nexus plugin call com.nexus.m1-acceptance get-events
[ { "type": "FileCreated", "path": "welcome.md", "content_hash": "..." } ]

$ nexus content search "welcome"
welcome.md:1: # Welcome  (score: 1.0)

$ nexus plugin call com.nexus.m1-acceptance try-network
Error: capability 'net.http' not granted to plugin 'com.nexus.m1-acceptance'

$ nexus logs tail --level info | grep CapabilityDenied
2026-04-11T14:33:21Z INFO  capability=net.http plugin=com.nexus.m1-acceptance result=denied

$ nexus forge status
Forge: my-test-forge
Plugins: 1 loaded (com.nexus.m1-acceptance)
Files: 1 indexed (welcome.md)
Storage: 12 bytes

# Hot-reload test
$ touch ../tests/fixtures/m1-acceptance-plugin/target/wasm32-wasi/release/plugin.wasm
$ sleep 1
$ nexus logs tail --level info | grep -E "PluginStopped|PluginStarted" | tail -2
2026-04-11T14:33:25Z INFO  plugin=com.nexus.m1-acceptance event=PluginStopped reason=hot_reload
2026-04-11T14:33:25Z INFO  plugin=com.nexus.m1-acceptance event=PluginStarted

# State preservation across reload
$ nexus plugin call com.nexus.m1-acceptance get-events
[ { "type": "FileCreated", "path": "welcome.md", "content_hash": "..." } ]
```

The `m1-acceptance-plugin` is itself an M1 deliverable, built from the PRD 04a community template. It serves as test fixture *and* first dogfood example of plugin authorship.

### 11.4 What's NOT tested at the M1 gate

- ❌ Performance benchmarks (deferred to v0.2)
- ❌ GUI / desktop app (M2)
- ❌ Editor functionality, themes, Markdown rendering UI (M2)
- ❌ Database engine, terminal, git (M3)
- ❌ AI integration (M4)
- ❌ Workflows, agents (M5)
- ❌ CRDT sync (cut from v0.1)
- ❌ Plugin signing verification (cut from M1)
- ❌ Audit log rotation/export/merkle (cut from M1; only daily file write tested)
- ❌ Cross-platform (cut from v0.1)

### 11.5 Test runner

`cargo nextest run --workspace` runs all unit + acceptance + integration tests in parallel. CI is not set up in M1 (personal tool — local runs are sufficient until a need surfaces).

---

## 12. M1 Done Definition

M1 is "done" when **all four** of the following are true:

1. **All Layer 1 (per-PRD acceptance) tests pass** for PRDs 01, 02 (slimmed), 03, 04 (slimmed), 04a, 05.
2. **All Layer 2 (integration) tests I1–I7 pass.** I7 (the walking-skeleton smoke test) is the load-bearing one.
3. **All ADRs from §10.4 are written** and capture the rationale for each non-trivial M1 decision.
4. **The M1 dogfood week succeeds.** After M1 passes its gate, the user spends ~1 week using the `nexus` CLI for non-Nexus workspace tasks (managing notes, scripting daily work, exercising the plugin scaffolder). Anything painful becomes a fix-list before M2 starts. Anything fundamentally broken triggers an M1 amendment cycle.

If all four are true, M1 ships and M2 brainstorming begins.

---

## 13. Open Follow-ups (deferred from M1)

These are real items the M1 work will touch but resolve in their own follow-up cycles, not block M1 shipping:

- **Templates rename pass** (`PRDs/templates/`): the `forge`→`nexus` PRD rename pass deferred the templates directory. Handle when M1 PRD 04a work begins.
- **05-cli.md §9.1 vs §3.1.3 short/long config form**: agent normalized to `nexus forge config`; revisit if a top-level `nexus config` alias is wanted.
- **13-skills.md singular/plural CLI naming** (`nexus skill` vs `nexus skills`): unify in a follow-up pass if it bothers you in practice.
- **PRD 02 cleanup**: the cut sections of PRD 02 (sync threats, full audit subsystem, plugin code review workflows, credential key rotation) need to be marked as deferred *in the PRD itself* so future readers know they're cut. A 30-minute documentation pass at the end of M1.
- **PRD 04 cleanup**: same — mark cut sections (marketplace, community registry, plugin discovery UI, ratings) as deferred in the PRD.
- **PRD 03 cleanup**: mark §8 (CRDT Sync Design) as deferred-from-v0.1.
- **JIT compilation cache for wasmtime**: deferred — adds startup speed on plugin reload, but a v0.2 optimization. Monitor wasmtime startup time during the M1 dogfood week; if it's painful, add the cache.
- **SQLite concurrent writer optimization**: brainstorm fuel PRD 03 R2 — current design serializes writes behind a mutex. Acceptable for M1; revisit in v0.2 if real-world concurrent edits cause friction.

---

## 14. Next Step

After this spec is approved by the user:

1. **Invoke the `superpowers:writing-plans` skill** with this spec as input. The skill produces an executable, task-level implementation plan for M1 — one that can be handed to AI agents one PRD at a time per the agent delegation pattern in roadmap §5.4.
2. **Initial ADRs written first** (the 10 from §10.4) before any code, so the rationale is captured while it's fresh.
3. **First PRD: 01-kernel-event-system.** Sequential execution per the roadmap — kernel grounds first, then security, then storage, then plugins (with 04a in parallel late), then CLI.
4. **Per-PRD interface specs** drafted by the human (not the agent) at the start of each PRD per cross-cutting concern §5.1 of the roadmap. These are the contracts agents implement against.

---

**End of M1 spec. Approval gate: user reviews and signs off, then we proceed to writing-plans.**
