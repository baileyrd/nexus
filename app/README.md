# Nexus frontend

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
