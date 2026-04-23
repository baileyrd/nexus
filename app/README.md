# Nexus frontend (LEGACY — DEPRECATED)

> **⚠️ DEPRECATED as of 2026-04-23.** This directory and its companion
> Rust host crate `crates/nexus-app` are the **legacy desktop shell**.
> Per [ADR 0011](../docs/adr/0011-adopt-plugin-first-shell.md), all new
> desktop work lands in the plugin-first shell at
> [`shell/`](../shell/) + [`shell/src-tauri/`](../shell/src-tauri/) (crate
> `nexus-shell`).
>
> **Freeze policy:** Do not add new Tauri commands, new frontend views,
> or new capabilities to this tree. Bug fixes and security patches only,
> until feature parity is reached in the new shell and this tree is
> deleted. New capability work goes into:
> - A service-crate IPC handler (reachable from CLI, TUI, MCP, and the
>   new shell through `context.ipc_call(...)`), and
> - A plugin in `shell/src/plugins/nexus/<feature>/`.
>
> See [`docs/INTEGRATION-REVIEW.md`](../docs/INTEGRATION-REVIEW.md) and
> [`docs/SHELL-COMPARISON.md`](../docs/SHELL-COMPARISON.md) for the
> migration plan. A snapshot tag `v0.1.0-legacy-shell` preserves the
> pre-freeze state.

---

Vite + React + TypeScript shell for the Tauri desktop app. Pairs with
`crates/nexus-app` (the Rust shell that hosts this UI in a Tauri window).

## Run in dev

```bash
cd app
npm install
npm run tauri:dev
```

`npm run tauri:dev` launches the Tauri Rust binary, which in turn starts
Vite (see `beforeDevCommand` in `crates/nexus-app/tauri.conf.json`) and
loads `http://localhost:5173` in the webview.

If you only want the frontend without Tauri (e.g. to iterate on styling
in a plain browser), run:

```bash
npm run dev
```

Note that IPC calls will fail in that mode — everything that hits
`invoke()` needs the Tauri runtime.

## Build

```bash
npm run tauri:build
```

Produces a bundled desktop app under `target/`.

## Layout

```
app/
├── src/
│   ├── App.tsx                    # Root component
│   ├── main.tsx                   # React entry
│   ├── styles.css                 # Base cascade + picker styling
│   ├── components/ThemePicker.tsx # One component so far
│   ├── ipc/theme.ts               # Typed invoke() wrappers
│   └── stores/theme.ts            # Zustand theme store
├── index.html
├── package.json
├── tsconfig.json
└── vite.config.ts
```
