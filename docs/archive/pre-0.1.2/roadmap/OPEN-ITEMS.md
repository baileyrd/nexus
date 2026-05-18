# Open Items — Post-Migration Carryover Gaps

> One open item remains from the 2026-04-24 Phase 4 (WI-37) leaf-migration
> sweep. The other 21 OIs shipped between 2026-04-24 and 2026-05-01 and
> live verbatim as the resolution audit trail at
> [`../archive/OPEN-ITEMS-resolved-2026-04-26.md`](../archive/OPEN-ITEMS-resolved-2026-04-26.md).
> Cross-listed from [`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md) "Post-migration
> carryover gaps."

---

## OI-05 — Rust dependency duplication

**Severity:** Build debt (compile time + binary size)
**Surfaced by:** audit 2026-04-24
**Status:** Partially drained 2026-05-17. Five duplicate-version pairs
collapsed. Remaining residue is upstream-bound on three crates we cannot
displace today.

### What changed on 2026-05-17

Cargo workspace bumps that consolidated dep graph as a side effect:

| Move                                                                         | Pairs collapsed                                                                              |
|-------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------|
| `wasmtime 42 → 44` (workspace pin)                                            | `foldhash 0.1/0.2` → `0.2`; dropped `hashbrown 0.15` (still split between `0.16/0.17`)        |
| `fs4 0.9 → 1` (nexus-storage direct dep, plus `fs_std::FileExt` → `FileExt` and `WouldBlock` → `TryLockError::WouldBlock` API adapter in `crates/nexus-storage/src/forge.rs`) | `rustix 0.38/1.1` → `1.1`; `linux-raw-sys 0.4/0.12` → `0.12`        |
| `rustyline 14 → 18` (workspace pin)                                           | `unicode-width 0.1/0.2` → `0.2`                                                              |
| `criterion 0.7 → 0.8` (workspace pin; also moved `nexus-terminal` off a local 0.7 pin onto the workspace dep) | no dedup win — criterion 0.8 still uses `itertools 0.13`, but the workspace is now consistent |

Net: from **34** duplicate-version pairs at the 2026-04-24 audit to **~25**
today.

### Remaining upstream blockers

Reverse-tree walk via `cargo tree -i`, grouped by the upstream that pins
the older half:

- **`wasmtime` 44.0.1** (workspace pin) still pulls `toml 0.9` →
  `winnow 0.7` + `toml_datetime 0.7`, `hashbrown 0.16` (via
  `cranelift-codegen`), `wasm-encoder 0.246` (via `wasmtime-environ`;
  paired with `wasm-encoder 0.248` from the `wat` dev-dep). Resolves
  when wasmtime ships a release that picks up `toml 1`, `hashbrown 0.17`,
  and `wasm-encoder 0.248`.
- **`portable-pty` 0.9** (via `nexus-terminal`) still pulls
  `filedescriptor 0.8 → thiserror 1.0`, `nix 0.28 → cfg_aliases 0.1`,
  and `downcast-rs 1`. `portable-pty` has not released since 0.9.0;
  switching PTY crates is a feature-level decision, not a drop-in bump.
- **`ed25519-dalek` 2.2** (via `nexus-plugins`) pulls the RustCrypto v0.10
  family — `sha2 0.10`, `digest 0.10`, `block-buffer 0.10`,
  `crypto-common 0.1`, `rand_core 0.6`, `getrandom 0.2`,
  `cpufeatures 0.2`. The v0.11 RustCrypto family is what `sha2 = "0.11"`
  in our workspace pulls. `ed25519-dalek 3.0.0-pre.7` exists on crates.io
  but is a pre-release; revisit when 3.0 ships stable.
- **`chacha20poly1305` 0.10.1** (via `nexus-plugins`) pulls `chacha20 0.9`
  (paired with `chacha20 0.10` from `rand 0.10` via `uuid 1.23`). The
  `0.11.0-rc.3` release is a pre-release-candidate; revisit when 0.11
  ships stable.
- **`tantivy` 0.26.1** (via `nexus-storage`) pulls `nom 7`,
  `fs4 0.13`, and `downcast-rs 2`. `nom 7` pairs with `nom 8` from
  `lettre`'s parser; `fs4 0.13` pairs with our `fs4 1`. Resolves when
  tantivy updates either.
- **`jsonschema` 0.46.2** (via `nexus-plugins` manifest validation)
  pulls `reqwest 0.13` (paired with our pinned `reqwest 0.12`) and
  `ahash → getrandom 0.3`. The reqwest pair would collapse if we move
  the workspace to `reqwest = "0.13"`; deferred because the AI provider
  HTTP clients (`nexus-ai`) have not been ported and the 0.12 → 0.13
  bump touches every TLS-config callsite. Filed below as a follow-up.
- **`itertools 0.13`** comes via `criterion 0.8`; paired with
  `itertools 0.14` from `ratatui-core`. Collapses when criterion or
  ratatui shifts.
- **`rand 0.9` / `rand_core 0.9`** (workspace pin from BL-101 nonces)
  paired with `rand 0.10` / `rand_core 0.10` (from `uuid 1.23` →
  `chacha20 0.10`). A workspace bump to `rand = "0.10"` would
  consolidate, but the chacha20poly1305 0.10 dependency still pulls
  `rand_core 0.6` separately, so the win is partial. Revisit
  alongside the chacha20poly1305 0.11 bump.

### Workspace-side follow-up (filed 2026-05-17)

- [ ] **`reqwest 0.12 → 0.13` workspace bump.** Touches every TLS-config
  callsite in `nexus-ai` (provider HTTP clients) and the rustls graph
  pinned alongside it. Effort medium; deferred until the AI providers
  surface a real need for a 0.13-only feature. Pairs with `jsonschema`'s
  `reqwest 0.13` and would collapse one pair.

### When to revisit

- Next wasmtime major release — re-run `cargo tree --duplicates` and
  sweep anything that unified as a side effect.
- When `ed25519-dalek 3.0` and `chacha20poly1305 0.11` ship stable, the
  RustCrypto v0.10 family will retire from this workspace's graph and
  the `digest` / `sha2` / `block-buffer` / `crypto-common` / `rand_core 0.6`
  / `getrandom 0.2` / `cpufeatures 0.2` family collapses with it.
- If the editor / terminal stack picks up a new PTY crate that doesn't
  depend on `filedescriptor`, `thiserror 1.0` goes away (taking
  `nix 0.28` + `cfg_aliases 0.1` + `downcast-rs 1` with it).
- Any direct dependency we add that pulls the older version of one of
  these families should be resisted — keep the forge on the newer half
  so the cleanup lands automatically when upstream moves.
