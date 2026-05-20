# com.nexus.skills

- **Path:** `crates/nexus-skills/`
- **Tier:** Core Rust
- **Bootstrap order:** 11

## Architecture

- Entry point: `crates/nexus-skills/src/core_plugin.rs` — `SkillsCorePlugin::open(skills_dir)`. Bootstrap registration: `crates/nexus-bootstrap/src/plugins/skills.rs`. Lifecycle: `LifecycleFlags::NONE`. Bootstrap calls `nexus_skills::seed_builtins(&skills_dir)` before registration to materialise the built-in `.skill.md` files into `<forge>/.forge/skills/` (idempotent — skips files that already exist).
- Key modules:
  - `parse.rs` — `parse_skill_file` / `parse_skill_text`: splits YAML frontmatter (PRD-13 §2.3 schema) from the markdown body.
  - `registry.rs` — `SkillRegistry`: directory walk over `<forge>/.forge/skills/` (recurses sub-dirs), in-memory index by id.
  - `registry_index.rs` — emits a JSON sidecar (`REGISTRY.json`) on every load / reload so the shell can hydrate without re-parsing every `.skill.md`.
  - `compose.rs` — resolves `depends_on:` chains into a `ComposedSkill` (PRD-13 §5).
  - `substitute.rs` — `render` performs parameter substitution into the body.
  - `builtins.rs` — embedded built-in skill content + `seed_builtins` seeder.
- Persistence:
  - `<forge>/.forge/skills/<sub>/<name>.skill.md` — user + seeded built-in skill files (file-as-truth).
  - `<forge>/.forge/skills/REGISTRY.json` — derived index sidecar, rebuilt on every `reload`.
- Settings owned: none. No TOML config file; the skills directory itself is the input.
- External dependencies: `serde_yml` for frontmatter parsing. Read-mostly — no network, no SQLite, no subprocess.

## Surface

8 IPC handlers (full table at `crates/nexus-skills/src/core_plugin.rs:187`):

`list`, `get`, `list_by_context`, `triggered_by`, `reload`, `render`, `compose`, `invoke` (BL-054 Phase 3 — async; dispatches the skill body as an AI prompt).

## Necessity

- **Verdict:** Optional
- **Required for basic capabilities?** No — the `.skill.md` registry is a reusable-instructions feature. Browsing, editing, searching, and committing markdown all complete without ever touching it.
- **Depended on by:** `com.nexus.agent` (consults skills for context-applicable instructions), `com.nexus.ai` (Chat / inline AI panels offer skill-based prompts), shell-nexus `skills` panel.
- **Depends on:** `nexus-kernel`, `nexus-plugins` only. The plugin is library-only; `invoke` is the one async handler and it dispatches into `com.nexus.ai`.
- **What breaks if removed:** the skills panel goes dark; agents and AI surfaces lose the auto-activated-by-context prompts and trigger-keyword expansions. The basic-capability workflow is unaffected.

## Notes

- The bootstrap seeder logs `seeded built-in skills` with the created file list at info; failures fall through to a warn and continue with whatever is already on disk.
- `compose` resolves `depends_on:` cycles by reporting `ComposeConflict` rather than panicking.
- `restrictions` (modify_files / delete_content / execute_code / allowed_tools) on a skill are advisory metadata the registry surfaces — actual capability enforcement remains on the kernel side at IPC dispatch time.
- The `extra` field on `SkillMeta` round-trips unknown frontmatter keys so future schema additions stay readable by older Nexus builds.
