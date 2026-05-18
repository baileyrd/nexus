# Desktop Strategy — Nexus v1.0 PRD

**Status:** 🟢 Shipped (desktop)
**Version:** 1.0 (reframed 2026-05-12 per [DG-38](../roadmap/DOC-GAPS.md#dg-38))
**Owner:** Architecture
**Date:** April 2026 · reframed May 2026
**Target Platforms:** Desktop (Tauri 2.x) on Linux / macOS / Windows

> **Scoping note (2026-05-12, DG-38).** This PRD is reframed as a **desktop strategy**. The original document scoped Web (WASM) and Mobile (iOS / Android via UniFFI) as committed targets alongside desktop; that commitment is withdrawn. The web / mobile sections (§3, §5, §6, plus per-platform rows in the §7 build pipeline, §10 feature matrix, §11 performance targets, §12 testing matrix, §13 parity roadmap, §14 native-behavior columns, §15 web onboarding, §16 mobile UX) are preserved below for historical context and design rationale — they are **not** committed work items. If multi-platform is pursued later, each platform must be promoted to its own BL entry and re-validated against current architecture (ADR 0011 single-shell desktop, ADR 0016 microkernel native-vs-WASM split). The active scope is Desktop / Tauri 2.x; everything else is exploratory.

---

## Executive Summary

Nexus is a Rust-based, AI-native developer knowledge environment shipping on the desktop via Tauri 2.x. The architecture keeps the kernel, storage, and plugin layers platform-agnostic in Rust so a future multi-platform pivot remains reachable, but the committed product surface today is the Tauri desktop shell on Linux, macOS, and Windows.

---

## 1. Architecture Overview

### Shared Rust Core
- **nexus-kernel**: Tokenizer, forge indexing, AI inference hooks, CRDT sync engine
- **nexus-storage**: Traits for FileSystem, OPFS/IndexedDB, and mobile file backends
- **nexus-plugin**: Plugin protocol, WASM sandboxing, IPC to kernel
- **nexus-ai**: LLM bindings, embedding cache, token budgeting

All crates target `wasm32-unknown-unknown`, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-linux-gnu`, `x86_64-pc-windows-msvc`.

### Platform-Specific Layers
- **Desktop**: Tauri 2.x window manager, React + Zustand UI, ts-rs TypeScript bindings, portable-pty PTY, wgpu canvas
- **Web**: WASM kernel, IndexedDB + OPFS storage, service worker, xterm.js terminal, Web Workers for background processing
- **Mobile**: UniFFI Kotlin/Swift bindings, native navigation shell, WebView for editor, CRDT sync daemon

---

## 2. Tauri 2.x Desktop Architecture

### Window Management
- **Multi-window support**: Main editor window + detachable panels (AI assistant, forge explorer, settings)
- **State persistence**: Window bounds, focus, tab state serialized via `serde_json` to `%APPDATA%/nexus/state.json`
- **Deep linking**: `nexus://forge/my-forge/notes/abc123` routes to specific note; tray icon supports deep links
- **Tray integration**: Minimize to tray, quick-access forge switcher, `:` global hotkey triggers command palette

### WebView Configuration
```toml
[tauri.windows.editor]
url = { spa = "dist/index.html" }
transparent = false
resizable = true
fullscreen = false
minWidth = 800
minHeight = 600
```
- JavaScript context pre-injects `window.__NEXUS__` with platform info: `{ platform: "desktop", version: "1.0.0" }`
- IPC channels: `invoke_kernel()`, `listen_kernel()`, `emit_to_renderer()` for bidirectional kernel ↔ UI communication
- DevTools enabled in development, disabled in production

### Plugin Model
- Tauri plugins (Rust) vs in-process WASM plugins
- Plugins extend menu bar, contribute custom Tauri commands, hook into file open/save dialogs
- Example: Git plugin registers custom file icons, suggests version history on forge right-click

### Tauri Updater
- **Strategy**: Self-hosted updater with signature verification
- **Frequency**: Check on startup + daily background check
- **Payload**: Delta updates (only changed files) to minimize bandwidth
- **Rollback**: Previous version preserved in `~/.tauri` cache; user can revert from Help menu
- **Version file hosted at**: `https://releases.nexus.dev/latest.json`

### Build Pipeline
```bash
# Development
cargo build --release
npm run build  # React + Vite
tauri dev

# Production build + signing
tauri build --target x86_64-pc-windows-msvc
tauri build --target x86_64-apple-darwin
tauri build --target x86_64-unknown-linux-gnu
codesign -s "Developer ID Application" src-tauri/target/release/bundle/macos/Nexus.app
```

---

## 3. WASM Compilation Strategy

> **Deferred (DG-38, 2026-05-12).** Not a committed work item. Preserved for design rationale.

### Crate Compatibility Matrix
| Crate | WASM Target | Notes |
|-------|-------------|-------|
| nexus-kernel | ✓ | All tokenization, indexing, CRDT code pure Rust |
| nexus-storage | ✓ (IndexedDB backend) | Trait impl for Web; FileSystem trait unavailable |
| portable-pty | ✗ | Desktop only; web uses xterm.js terminal emulator |
| wgpu | ✓ (WebGPU subset) | Canvas rendering limited to WebGL2 fallback on older browsers |
| rusqlite | ✗ | Replaced by browser-native IndexedDB for persistence |
| serde | ✓ | JSON serialization for all platforms |
| tokio | ✓ (wasm-bindgen-futures) | Async runtime mapped to JS Promises |

### wasm-pack Configuration
```toml
[package.metadata.wasm]
wasm-opt = ["-O4", "--enable-simd", "--enable-mutable-globals"]
```
- Target: `wasm32-unknown-unknown`
- Bindings: `wasm-bindgen` generates JS/TS interfaces from Rust code
- Bundle includes: `nexus_kernel_bg.wasm` (~2.8 MB gzip), `.d.ts` type definitions

### Bundle Size Targets
- **Kernel WASM**: 2.8 MB (gzip) — includes tokenizer, CRDT engine, embeddings cache
- **UI assets**: 1.2 MB (React, Zustand, editor, themes)
- **Total first load**: < 5 MB (uncompressed), < 1.5 MB (gzip)
- **Lazy load plugins**: Separate chunks loaded on-demand

### Performance Differential (Native vs WASM)
| Operation | Desktop (Native) | Web (WASM) | Ratio |
|-----------|-----------------|-----------|-------|
| Tokenize 50k tokens | 45 ms | 120 ms | 2.7x slower |
| Index 10k notes | 380 ms | 950 ms | 2.5x slower |
| Full-text search (1k results) | 22 ms | 68 ms | 3.1x slower |
| Embeddings lookup (1k) | 35 ms | 85 ms | 2.4x slower |

Web performance acceptable for typical usage; heavy indexing deferred to background workers.

---

## 4. Platform Abstraction Layer

> **Partially deferred (DG-38, 2026-05-12).** The desktop column of the implementation matrix is shipped (file I/O, Tauri APIs, keyring, hotkeys). The `nexus-platform` crate itself was never created — desktop currently calls Tauri / keyring / portable-pty directly. Web / iOS / Android columns are deferred design notes.

### Core Traits (nexus-platform)

```rust
// FileSystem trait — implemented per platform
pub trait FileSystem: Send + Sync {
    async fn read(&self, path: &Path) -> Result<Vec<u8>>;
    async fn write(&self, path: &Path, data: &[u8]) -> Result<()>;
    async fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;
    async fn watch(&self, path: &Path) -> Result<WatchHandle>;
}

// WindowManager trait — for native features (tray, notifications, dialogs)
pub trait WindowManager: Send + Sync {
    async fn show_save_dialog(&self, title: &str, filter: &str) -> Result<Option<PathBuf>>;
    async fn show_notification(&self, title: &str, body: &str) -> Result<()>;
    async fn set_clipboard(&self, text: &str) -> Result<()>;
    async fn get_clipboard(&self) -> Result<String>;
}

// SystemInfo trait
pub trait SystemInfo: Send + Sync {
    fn platform(&self) -> Platform; // Desktop, Web, iOS, Android
    fn app_version(&self) -> String;
    fn is_offline(&self) -> bool;
}

// Keychain trait — for API keys, auth tokens
pub trait Keychain: Send + Sync {
    async fn store(&self, service: &str, account: &str, value: &str) -> Result<()>;
    async fn retrieve(&self, service: &str, account: &str) -> Result<Option<String>>;
}

// GlobalHotkeys trait — desktop only, stub on web/mobile
pub trait GlobalHotkeys: Send + Sync {
    async fn register(&self, key: &str, handler: Box<dyn Fn() + Send>) -> Result<()>;
}
```

### Implementation Matrix
| Platform | FileSystem | WindowManager | Keychain | GlobalHotkeys |
|----------|-----------|---------------|----------|---------------|
| Desktop (Tauri) | Real file I/O | tauri::api | keyring crate | tauri::hotkey |
| Web | IndexedDB + OPFS | Browser dialogs | LocalStorage (unsecured) | N/A (stub) |
| iOS (UniFFI) | FileManager API | UIKit/SwiftUI | Keychain Services | N/A (stub) |
| Android (UniFFI) | Context.filesDir | Android APIs | Keystore | N/A (stub) |

---

## 5. Web Platform Implementation

> **Deferred (DG-38, 2026-05-12).** Not a committed work item. Preserved for design rationale.

### File System Backend: OPFS + IndexedDB
- **OPFS (File System Access API)**: User grants directory picker once; persistent access to `/nexus-forges/`
- **IndexedDB**: Metadata, indexed search cache, CRDT state, user preferences
- **Fallback**: If OPFS unavailable (Safari < 17.2), degrade to IndexedDB-only (slower traversal)

```javascript
const root = await navigator.storage.getDirectory();
const forgeDir = await root.getDirectoryHandle('nexus-forges', { create: true });
const noteFile = await forgeDir.getFileHandle('notes/abc123.md', { create: true });
const writable = await noteFile.createWritable();
await writable.write(content);
await writable.close();
```

### Service Worker Offline Support
- **Strategy**: Cache kernel WASM, UI assets, and frequently-accessed notes during online periods
- **Offline behavior**: Read cached notes, queue edits in IndexedDB, disable AI features (requires server)
- **Sync on reconnect**: CRDT engine detects online event, merges local changes with server state

### PWA Manifest
```json
{
  "name": "Nexus",
  "short_name": "Nexus",
  "start_url": "/?pwa=1",
  "display": "standalone",
  "scope": "/",
  "icons": [{ "src": "/icon-192.png", "sizes": "192x192" }],
  "screenshots": [
    { "src": "/screenshot-540.png", "sizes": "540x720", "form_factor": "narrow" }
  ],
  "categories": ["productivity"],
  "shortcuts": [
    { "name": "New Note", "short_name": "+", "url": "/?action=new-note" }
  ]
}
```

### Terminal Emulator: xterm.js
- **Why**: No native PTY in browser; xterm.js provides ANSI terminal emulation + selection
- **Limitation**: No shell process execution (security sandbox); displays pre-recorded terminal sessions or static logs
- **Alternative**: Plan future WebRTC tunneling to desktop for true shell access

### Background Processing: Web Workers
- **Worker threads**: Async indexing, embeddings lookup, CRDT operations run in Workers to avoid blocking UI
- **Message passing**: Kernel spawns workers via `worker_threads.rs` (Rust ↔ JS), handles results asynchronously

### WebRTC Sync
- **Desktop ↔ Web**: Optional real-time CRDT sync via signaling server
- **Negotiation**: User scans QR code on web client; desktop relays CRDT ops via WebRTC data channel
- **Fallback**: Cloud sync via API if WebRTC unavailable

---

## 6. Mobile Platform Implementation (iOS/Android)

> **Deferred (DG-38, 2026-05-12).** Not a committed work item. Preserved for design rationale.

### UniFFI Bindings
```rust
// In Cargo.toml
[dependencies]
uniffi = { version = "0.27", features = ["cli"] }

// In lib.rs
pub fn create_forge(name: String) -> Result<String, String> { /* ... */ }
pub fn list_forges() -> Vec<ForgeMetadata> { /* ... */ }
pub fn search(query: String) -> Vec<SearchResult> { /* ... */ }

// Generate bindings
uniffi-bindgen-cli --language kotlin src/nexus.udl
uniffi-bindgen-cli --language swift src/nexus.udl
```
- **Output**: `nexus.kt` (Kotlin), `Nexus.swift` (Swift) with type-safe interfaces
- **Regeneration**: CI runs bindings generation on every Rust change; fails if Swift/Kotlin signatures would break

### Native UI Shells

**iOS (SwiftUI):**
- Tab bar navigation: Forges, Search, Capture, Settings
- Forge explorer: Hierarchical outline view with swipe-to-delete
- Read-mode: Optimized for portrait, large text, dark mode
- Editor WebView: Fallback for complex edits; native markdown preview when possible

**Android (Jetpack Compose):**
- Bottom navigation + drawer for forge switcher
- Material You theming + dynamic colors from wallpaper
- Floating action button triggers quick-capture dialog
- WebView for editor with system keyboard integration

### Background Sync Daemon
- **Schedule**: Daily sync + on-demand (pull-to-refresh)
- **Mechanism**: UniFFI async function spawns background task; OS grants ~30 seconds to sync CRDT state
- **Conflict resolution**: Mobile edits always win; cloud changes merged on next sync

### Platform-Native Features
| Feature | iOS | Android |
|---------|-----|---------|
| Quick capture widget | Lockscreen widget (iOS 17+) | Home screen widget |
| Share extension | Custom share target for text/URLs | Share sheet integration |
| Spotlight search | App Search indexing | Launcher search |
| Haptic feedback | Taptic Engine (UIFeedbackGenerator) | Vibration API |
| Deep linking | Universal Links (nexus://...) | App Links + Custom Scheme |

---

## 7. Build & CI Pipeline

### Local Development Workflow
```bash
# Desktop
cargo build --release -p nexus-tauri
npm run dev  # Vite dev server
tauri dev

# Web
cargo build --release --target wasm32-unknown-unknown -p nexus-kernel
wasm-pack build --target web --release
npm run build
npm run serve

# Mobile (requires Xcode/Android Studio)
cargo build --release --target aarch64-apple-ios -p nexus-kernel
uniffi-bindgen-cli --language swift src/nexus.udl
# (then build in Xcode)
```

### CI Matrix (GitHub Actions)
```yaml
jobs:
  build-matrix:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            features: desktop
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            features: desktop
          - os: macos-latest
            target: x86_64-apple-darwin
            features: desktop
          - os: macos-latest
            target: aarch64-apple-darwin
            features: desktop
          - os: ubuntu-latest
            target: wasm32-unknown-unknown
            features: web
    steps:
      - uses: actions-rs/toolchain@v1
        with:
          target: ${{ matrix.target }}
      - run: cargo build --release --target ${{ matrix.target }}
```

### Feature Flags
```toml
[features]
default = ["desktop"]
desktop = ["tauri", "portable-pty", "wgpu"]
web = []
mobile = ["uniffi"]
```

### Code Signing & Release
- **macOS**: Sign with Apple Developer ID; notarize via Apple's notarization service
- **Windows**: Sign with EV certificate; SmartScreen whitelisting via Tauri
- **Linux**: No signature required; distribute via AppImage + repo
- **iOS**: Sign with distribution certificate; TestFlight beta, then App Store
- **Android**: Sign with keystore; upload to Play Store

### Auto-Update Process
- **Desktop**: Tauri updater checks `releases.nexus.dev` on startup
- **iOS/Android**: App Store and Play Store handle updates automatically
- **Web**: Service Worker cache-busting; new build auto-deployed to CDN

---

## 8. TypeScript Binding Generation

### ts-rs Pipeline
```rust
use ts_rs::TS;

#[derive(TS, Serialize)]
#[ts(export)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub created_at: String, // ISO 8601
}

// Generates: Note.ts
// export interface Note {
//   id: string;
//   title: string;
//   tags: string[];
//   created_at: string;
// }
```

### Sync & Validation
- **Generation**: `cargo test --features ts-exports` runs ts-rs codegen in CI
- **Export dir**: `src/bindings/generated/`
- **CI check**: `npm run type-check` ensures TS compiles against generated types
- **Failure mode**: CI fails if Rust type changes without TS counterpart

### Runtime Validation (zod)
```typescript
import { NoteSchema } from './bindings/generated/Note';
import { z } from 'zod';

const validated = NoteSchema.parse(kernelResponse);
```
- ts-rs generates Zod schemas alongside interfaces
- All IPC results validated at boundary before passing to React state

---

## 9. Shared State Management

### Zustand Store (Desktop/Web)
```typescript
interface NexusStore {
  currentForge: Forge | null;
  notes: Record<string, Note>;
  selectedNoteId: string | null;
  
  setCurrentForge: (forge: Forge) => void;
  updateNote: (note: Note) => void;
}

export const useNexus = create<NexusStore>((set) => ({
  currentForge: null,
  notes: {},
  selectedNoteId: null,
  
  setCurrentForge: (forge) => set({ currentForge: forge }),
  updateNote: (note) => set((state) => ({
    notes: { ...state.notes, [note.id]: note }
  })),
}));
```

### Kernel ↔ UI Sync
- **Kernel state**: Persistent CRDT state in storage, in-memory index
- **UI state**: Zustand for viewport, selections, open panels
- **IPC flow**: `invoke_kernel("update_note", { note })` → Rust processes → `emit_to_renderer("notes:updated")` → Zustand updates
- **Batch updates**: High-frequency edits (keystroke-level) debounced; emitted as single CRDT operation every 500 ms

### Event Coalescing
- **Keystroke example**: 10 edit events/second coalesced into 1 CRDT op/500ms
- **Search example**: Filter results cached in Zustand; only re-query kernel if filter input unchanged for 200 ms
- **Benefit**: Reduces IPC overhead, kernel lock contention

---

## 10. Platform-Specific Features Matrix

| Feature | Desktop | Web | iOS | Android |
|---------|---------|-----|-----|---------|
| **Editor** | Full | Full | Read + lightweight edit | Read + lightweight edit |
| **PTY/Shell** | ✓ (portable-pty) | Web emulator (read-only) | – | – |
| **Forges (local)** | ✓ Unlimited | OPFS (quota: 50 GB) | ✓ (app sandbox) | ✓ (app sandbox) |
| **Offline mode** | ✓ Full | ✓ (IndexedDB cache) | ✓ (full) | ✓ (full) |
| **AI assistant** | ✓ (local + API) | ✓ (API) | ✓ (API) | ✓ (API) |
| **Plugins (WASM)** | ✓ | ✓ | – | – |
| **Global hotkeys** | ✓ | – | – | – |
| **Clipboard** | ✓ (read/write) | ✓ (read/write) | ✓ (read) | ✓ (read) |
| **File drag-drop** | ✓ | OPFS picker | – | – |
| **GPU canvas** | ✓ (wgpu) | ✓ (WebGL2) | – | – |
| **Quick capture** | – | – | ✓ (widget) | ✓ (widget) |
| **Sync (CRDT)** | ✓ Cloud + P2P | ✓ Cloud + optional P2P | ✓ Cloud | ✓ Cloud |

---

## 11. Performance Targets

### Desktop
- **Startup time**: < 1.5s (cold), < 500 ms (warm)
- **Memory usage**: < 350 MB (idle), < 600 MB (with 10k notes loaded)
- **Bundle size**: 85 MB (DMG), 65 MB (AppImage)
- **Battery impact**: Negligible when idle; sync every 30 min in background

### Web
- **First load**: < 3s (1.5 MB gzip on 4G)
- **Subsequent**: < 500 ms (service worker cache)
- **Memory**: < 200 MB (UI + cache)
- **Bundle size**: 4.2 MB total (kernel WASM + UI)

### Mobile
- **Startup**: < 2s (cold), < 600 ms (warm)
- **Memory**: < 250 MB (iOS), < 300 MB (Android)
- **App size**: 35 MB (iOS), 42 MB (Android, including ART)
- **Battery**: < 3% drain/hour idle; sync every 1 hour

---

## 12. Testing Strategy

### Unit Tests
- **Kernel**: `cargo test -p nexus-kernel` runs on all platforms (native + WASM via wasm-bindgen-test)
- **CRDT**: Conflict resolution, concurrent edits, peer sync
- **Bindings**: ts-rs type export validation

### Integration Tests
- **Desktop**: Tauri test harness controls window, invokes IPC, verifies state
- **Web**: Puppeteer/Playwright tests IndexedDB, offline mode, service worker
- **Mobile**: XCTest (iOS) and Espresso (Android) for native UI flows

### Cross-Platform Matrix
```
Tests/Platforms:  Desktop (Linux)  Desktop (macOS)  Desktop (Windows)  Web (Chrome)  Web (Safari)  iOS  Android
Unit tests        ✓ CI            ✓ CI            ✓ CI              ✓ CI          ✓ CI         –    –
Integration       ✓ CI            ✓ nightly       ✓ nightly         ✓ CI          ✓ nightly    ✓*   ✓*
Feature tests     ✓ Nightly       –               –                 ✓ Nightly     –            –    –
(*manual on physical devices)
```

### CI Infrastructure
- **GitHub Actions**: Linux (unit + web), macOS (desktop), Windows (desktop)
- **Cron jobs**: Nightly full cross-platform on real devices (AWS Device Farm)
- **Failures**: Slack notification to #engineering; block merge if critical

---

## 13. Feature Parity Roadmap

### MVP (Launch)
- **All platforms**: Read notes, search, CRDT sync, AI chat
- **Desktop**: Full editing, terminal, plugins
- **Web**: Full editing, no plugins
- **Mobile**: Read-only, quick capture, search

### v1.1 (Q2 2026)
- **Mobile**: Lightweight editing (markdown only)
- **Web**: Plugin support (sandboxed subset)
- **Desktop**: Multi-window plugins

### v1.2 (Q3 2026)
- **All platforms**: Collaborative editing (real-time CRDT)
- **Mobile**: Full plugin support
- **Desktop**: GPU-accelerated graph visualization

### v1.3+ (Q4 2026+)
- **Web**: Terminal emulator (xterm.js) with desktop tunneling
- **All platforms**: AI training on private forges

---

## 14. Platform-Native Behavior

### Keyboard Shortcuts
- **macOS**: Cmd+N (new), Cmd+S (save → implicit), Cmd+F (find), Cmd+/  (toggle sidebar)
- **Windows/Linux**: Ctrl+N, Ctrl+S, Ctrl+F, Ctrl+\
- **Web**: Same as Windows/Linux (no platform variance in browser)
- **Mobile**: Hardware buttons (back to list, home to forges)

### Scrolling & Selection
- **Desktop**: Native momentum scrolling (Tauri WebView inherits OS behavior)
- **Web**: Browser default (smooth scroll with CSS `scroll-behavior`)
- **Mobile**: Native iOS/Android scroll physics; long-press for selection

### Context Menus
- **Desktop**: Right-click → native context menu (edit, delete, export)
- **Web**: Right-click → web context menu (+ browser's native options)
- **Mobile**: Long-press → iOS-style menu (iOS) or Material 3 menu (Android)

### Drag & Drop
- **Desktop**: Drag files into editor to embed; drag notes to reorganize
- **Web**: OPFS file picker; notes reorganization via dnd-kit
- **Mobile**: Tap-and-drag (iOS) or Material drag-handle (Android)

### Native File Dialogs
- **Desktop**: Tauri's `dialog::open_file` → native file picker
- **Web**: File System Access API picker, fallback to `<input type="file">`
- **Mobile**: Document picker (iOS) or file manager (Android)

---

## 15. Web Onboarding Flow

> **Deferred (DG-38, 2026-05-12).** Not a committed work item. Preserved for design rationale.

1. **Land on nexus.dev** → PWA install prompt (Chrome/Edge only)
2. **Create account** → Google/GitHub OAuth or email+password
3. **Create first forge** → Single-click with name + GitHub repo link (optional)
4. **Guided tour** → 4-step tour (sidebar, editor, AI panel, settings)
5. **Import from GitHub** → Scan `nexus.yaml` from repo; auto-populate notes
6. **Try the web app** → Full functionality in browser; optional desktop download
7. **Switch to desktop** → User clicks "Download" button → platform detection → Tauri installer → import cloud forges on first launch

---

## 16. Mobile UX

> **Deferred (DG-38, 2026-05-12).** Not a committed work item. Preserved for design rationale.

### Quick Capture Widget (iOS 17+ / Android 12+)
- **iOS Lockscreen widget**: Tap to open capture dialog; adds note without opening app
- **Android home widget**: 2x2 grid; tap to quick-capture
- **Sync**: Captured note syncs in background; visible in app on next open

### Read-Mode Navigation
- **Tab bar**: ← (back to forge list) | 📖 (reading view) | 🔍 (search) | ⚙️ (settings)
- **Gesture navigation**: Swipe left/right to navigate notes within folder
- **Typography**: Large serif font for reading; dark mode respects system setting

### Search-First Interface
- **On open**: Show search bar prominent at top
- **Instant results**: Type 3+ chars → results appear (live from IndexedDB)
- **Filters**: Tap tag to filter by tag; combine filters with AND logic

### Share Extension
- **iOS**: Share → Nexus → Append to "Inbox" note with URL + timestamp
- **Android**: Share → Nexus → Quick-capture modal; auto-tag as "shared"
- **Result**: Citation preserved in note for later review

---

## Acceptance Criteria

### Implementation Complete
- [ ] Tauri 2.x desktop builds for Windows, macOS, Linux (x64 + ARM)
- [ ] WASM kernel compiles, bundles to < 3 MB (gzip); performance benchmarked
- [ ] Platform abstraction traits implemented for Desktop, Web, iOS, Android
- [ ] Web OPFS + IndexedDB storage fully functional; offline sync tested
- [ ] Mobile UniFFI bindings generate and type-check without errors
- [ ] CI/CD pipeline builds all platforms on every commit
- [ ] ts-rs bindings auto-generate and validate in tests
- [ ] Zustand stores coalesce events; IPC latency < 50 ms avg
- [ ] Feature matrix verified: all items marked ✓ tested, all degraded features documented

### Testing Complete
- [ ] Unit tests pass on all platforms (native + WASM)
- [ ] Integration tests pass for desktop, web; mobile manual on real devices
- [ ] Performance targets met: startup times, memory, bundle sizes

### Documentation Complete
- [ ] This PRD finalized and approved
- [ ] Architecture decision records for each major platform choice
- [ ] Developer onboarding guide for setting up each platform's build chain
- [ ] Troubleshooting guide for CI failures, simulator issues

### Launch Criteria
- [ ] All features marked MVP tested on all three platforms
- [ ] Desktop installer code-signed and notarized
- [ ] Web PWA manifest valid; offline mode works
- [ ] Mobile apps submitted to App Store and Play Store
- [ ] Auto-update system functional (desktop)
- [ ] Analytics/error reporting working without user tracking

---

## Dependencies & Blockers

### External
- **Apple certificate renewal**: Required by March 2027 for iOS builds
- **Play Store API access**: Required for automated Android releases
- **WebRTC signaling server**: Optional; blocks P2P sync feature in v1.2

### Internal
- **nexus-kernel stable API**: Must freeze IPC interface before web/mobile bindings
- **CRDT algorithm finalized**: Changes break existing sync state
- **Plugin protocol locked**: Changes require recompile across all platforms

---

## Success Metrics

- **Desktop adoption**: 5k DAU by Q3 2026
- **Web adoption**: 10k DAU (browsers without desktop)
- **Mobile adoption**: 2k DAU (iOS + Android combined)
- **Feature parity**: 95%+ of core features available on all platforms by v1.2
- **Performance**: All platforms meet startup time targets
- **Reliability**: < 0.1% crash rate; sync conflict resolution < 1% user-visible issues

---

## Version History

| Version | Date | Status |
|---------|------|--------|
| 1.0 | April 2026 | Final |

