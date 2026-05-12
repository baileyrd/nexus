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
**Status:** Blocked on upstream. Every duplicate identified on 2026-04-24
traces back to a transitive dependency we don't own.

### Upstream blockers
Reverse-tree walk via `cargo tree -i`:
- **`wasmtime` 42.0.2** (pulled via `nexus-plugins`) pins `toml 0.9`,
  `sha2 0.10`, `digest 0.10`, `rand_core 0.6`, `reqwest 0.13`,
  `rustix 0.38`, `nix 0.28`, `hashbrown 0.15/0.16/0.17`, plus
  wasmtime-internal crates (`pulley-interpreter`, `wasmtime-internal-core`,
  `cranelift-bitset`) and `wasm-encoder` / `wasmparser` 0.244. Resolving
  any of these requires a wasmtime point release that itself upgrades them.
- **`portable-pty`** (via `nexus-terminal`) pulls `filedescriptor` which
  pins `thiserror 1.0`. Upgrading portable-pty or switching PTY crates
  is a feature-level decision, not a drop-in bump.
- The "identical version twice" rows (`bitflags 2.11.1`, `semver 1.0.28`,
  `libc 0.2.185`, etc.) are feature-flag splits inside wasmtime / Tauri —
  same version, two feature configurations.

### When to revisit
- Next wasmtime major release — re-run `cargo tree --duplicates` and
  sweep anything that unified as a side effect.
- If the editor / terminal stack picks up a new PTY crate that doesn't
  depend on `filedescriptor`, `thiserror` 1.0 goes away.
- Any direct dependency we add that pulls the older version of one of
  these families should be resisted — keep the forge on the newer half
  so the cleanup lands automatically when upstream moves.
