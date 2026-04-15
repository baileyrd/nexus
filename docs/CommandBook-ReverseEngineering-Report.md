# Command Book 1.0.46 — Reverse-Engineering Report

> **Scope & intent.** Static teardown of the shipping macOS app `CommandBook-1.0.46.dmg` for personal learning. No binary patching, no network replay, no live runtime analysis. Everything below is evidenced by artifacts inside the bundle (Info.plist, Mach-O load commands, embedded entitlements, Mach-O cstring/CStrings sections, Assets.car asset list, GRDB migration identifiers, and Sparkle framework layout).
>
> **Method summary.** The DMG is a standard Apple UDIF image (`koly` trailer at offset `0x00AEAD52`). The `Apple_APFS` partition was reassembled from its zlib-compressed UDIF `mish` chunks, then parsed with `dissect.apfs`. The `Command Book.app` bundle was extracted intact and its Mach-O binaries were parsed with `macholib` and a custom code-signature walker to pull embedded entitlements.

---

## 1. At a Glance

| Field | Value |
|---|---|
| Product name | Command Book |
| Marketing version | `1.0.46` (build `46`) |
| Bundle identifier | `com.talkpython.CommandBook` |
| Team / vendor | Talk Python / PDX Web Properties, LLC — "Created by Michael Kennedy" |
| Copyright | `Copyright 2026 PDX Web Properties, LLC` |
| Product website | `https://commandbookapp.com` |
| Buy page | `https://commandbookapp.com/buy` |
| Docs | `https://commandbookapp.com/docs` |
| Pricing message | `One-time purchase. Unlimited commands, history, and output buffer. Free updates for life.` |
| Category | `public.app-category.developer-tools` |
| Min OS | macOS `15.6` (Sequoia) |
| Architectures | Universal 2: `x86_64` + `arm64` |
| Build toolchain | Xcode `2630` (Build `17C529`), macOS SDK `26.2` |
| UI framework | SwiftUI (+ AppKit bridging via `NS*` wrappers) |
| Persistence | SQLite via **GRDB.swift** |
| Auto-update | **Sparkle 2.x** (appcast: `https://commandbookapp.com/updates/appcast.xml`) |
| Licensing | Custom HTTP licensing server at `commandbookapp.com` (Gumroad-sourced keys) |
| Sandbox | **Not sandboxed** (only `com.apple.security.files.user-selected.read-only` entitlement) |
| Code signature | Apple Developer ID; app-group prefix `AR5CLMFJGN.com.talkpython.CommandBook.cli` |
| CLI companion | `commandbook-cli` binary embedded, symlinked to `~/.local/bin/commandbook` on install |

---

## 2. What Command Book Is (Capabilities)

Command Book is a Mac-native **long-running process manager / development dashboard**. Instead of juggling many terminal tabs, a developer registers frequently-used shell commands (e.g., `./venv/bin/python app/talkpython.py --reload`, `docker-compose up`, `npm run dev`) and then starts, stops, restarts, monitors, and searches their output inside a single sidebar-driven window. A command palette gives keyboard-first access to everything.

### 2.1 Primary user-facing capabilities (from strings + view names)

**Command library**
- Save named commands with: `command`, `working_directory`, `environment_variables`, `pre_command`, `auto_restart`, `restart_delay_seconds`, `custom_icon_type`, slug, display name, timestamps.
- "Duplicate Command…", "Create New Command", "Save as Command…" (from ad-hoc history).
- Import / Export commands (`Export Commands...`, `Restore Commands...`).
- Auto-generated icons per command, 35+ built-in tech icons (see §3.4), plus generic SF Symbol fallback `wrench.and.screwdriver`.

**Ad-hoc commands & history**
- Type a one-off command in the palette, it runs like a saved one and is recorded in `adhoc_history` (searchable, promotable into a saved command).
- "Clear Command History?" maintenance action; Personal license is capped at 25,000 lines of ad-hoc history.

**Process lifecycle**
- Start / Stop / Restart running processes, with Ctrl+C (SIGINT), SIGTERM (graceful), and SIGKILL (force) paths; also "Send EOF (Ctrl+D)".
- Auto-restart on crash with configurable delay (field `restart_delay_seconds`). Restart counter per process; restart can be cancelled.
- Pre-commands: an optional preliminary command must succeed before the main one starts (`Runs before main command. Must succeed for main command to start.`).
- Status strings include: `Process exited cleanly (exit code 0)`, `Process exited with code`, `Killed by signal`, `Terminated (SIGTERM)`, `Killed (SIGKILL)`, `Terminated by Ctrl+C (SIGINT)`, `Command not found`, `Command not executable`, `Misuse of shell command`.

**Live output viewer**
- Streaming terminal output pane (`SelectableOutputTextView` / `ProcessOutputTextView`) with ANSI escape parsing (regex `\[((?:\d+;)*\d*)m` for SGR codes).
- Auto-scroll that can be paused; persistent scroll position per process (`OutputTextCache.scrollPositions`).
- Ring-buffer backing store (`RingBuffer` class) with configurable `outputBufferSize` (Personal = 25K lines, Full = 500K).
- **Find in output** (`OutputSearchBarView`): plain, case-sensitive, and regex modes with match count, Previous / Next navigation (Shift+Enter / Enter).
- **URL auto-detection** (`DetectedURLsView`): scans output for `http[s]://…`, `localhost:port`, `IP:port`, shows a floating "N links detected" menu; single link → direct open, multiple → "MultipleURLsMenu" popover. Regex in binary: `(?<!:)//(?:localhost|\d{1,3}(?:\.\d{1,3}){3}|[a-zA-Z0-9][\w.-]*\.[a-zA-Z]{2,})(?::\d+)?(?:/[^\s<>\"'`\]\)}\|,;]*)?`.
- Live stats per process (`ProcessStatsView`, `StatItem`): uptime, memory usage, restart count, exit code — tracked in `_accumulatedUptime`, `_memoryUsageBytes`, `_restartCount`.

**Command palette**
- ⌘K-style palette overlay (`CommandPaletteView`, `PaletteNSTextField`, `PaletteTextField`): type to filter saved commands or history, Enter to run, arrows to navigate, Escape to dismiss, custom Delete keybinding on history rows, inline "Save as Command" action. Trigger text shown elsewhere: `"Press ⌘K or click + to start"` and `"Show command palette (⌘K)"`.

**Sidebar**
- Left sidebar (`SidebarView`) with drag-reorderable rows, **user-editable separators** (category headers with name + SF Symbol icon), group actions "Start all in group" / "Stop all in group", context-menu actions: Copy Working Directory, Reveal in Finder, Duplicate, Delete Separator, Add Category Separator Above.
- Sidebar persists ordering and types in the `sidebar_items` table.

**External terminal integration (`TerminalAppManager`)**
- "Open in terminal" handoff to the user's preferred terminal. Supported bundle IDs:
  - `com.apple.Terminal` (Terminal.app)
  - `com.googlecode.iterm2` (iTerm2)
  - `dev.warp.Warp-Stable` (Warp, with YAML launch-config generation under `.warp/launch_configurations`)
  - `com.mitchellh.ghostty` (Ghostty)
  - `net.kovidgoyal.kitty` (Kitty)
  - `org.alacritty` (Alacritty)
- Writes a temporary shell run-script (`commandbook-run-*`) that prints a styled banner using ANSI colors, then execs the configured command.
- Requires the `NSAppleEventsUsageDescription` TCC prompt: *"Command Book needs to send commands to terminal apps to run your commands externally."*

**CLI companion (`commandbook-cli`)**
- Installed by `CLIInstallManager` as a symlink `~/.local/bin/commandbook` → `<App>/Contents/MacOS/commandbook-cli`.
- Detects user's shell and produces PATH-setup snippets for bash/zsh (`export PATH="$HOME/.local/bin:$PATH"`) and fish (`set -Ux fish_user_paths $HOME/.local/bin $fish_user_paths`, writes into `~/.config/fish/config.fish`).
- Subcommands (from embedded argparse help strings): `list`, `run <slug> [--working-directory=...]`, `new`, `edit <slug>`, `open`, `--help`. Reads the same `commandbook.sqlite` DB the app uses; fails gracefully if the DB hasn't been initialized.

**Database management**
- User-settable storage location (Default / iCloud / Custom) via `SettingsStore.StorageLocation`; default path `~/Documents/CommandBook/commandbook.sqlite`.
- Backup on migration: files named `commandbook-backup-<timestamp>`.
- Export DB via file picker (`commandbook-export-*`); Restore DB with validation ("The selected file is not a valid Command Book database").

**Licensing**
- Personal license with per-machine activation (uses IOKit `IOPlatformExpertDevice` for machine ID).
- Endpoints on `commandbookapp.com`: `/licenses/verify`, `/licenses/activate`, `/licenses/deactivate`.
- Storage: **Keychain** via service identifier (`keychainService`) holding a `SignedEnvelope` with integrity checks ("License data integrity check failed").
- Transfer flow: "Transfer License to This Mac" — enter key from another mac, app deactivates there then activates locally.
- Machine mismatch detection → UI warning state.
- Error strings: `Invalid license key`, `Activation limit reached. Deactivate another device first.`, `This license has been chargebacked`, `This license has been refunded`.
- Free tier limits: **5 command slots**, **25,000-line output buffer / ad-hoc history**. Upgrade pitch (`TimeToUpgradeView`) lists Unlimited Commands, Unlimited History, 500K Line Buffer, Free updates within major version.

**Auto-update (Sparkle)**
- `Sparkle.framework` at `Versions/B`, including `Autoupdate` helper, `Updater.app`, and two XPC services (`Downloader.xpc`, `Installer.xpc`).
- Appcast: `https://commandbookapp.com/updates/appcast.xml`.
- `SUPublicEDKey` = `THisbdE/WoYjxH9KNTPiqtdYlbYSLc3f/JM1EC8YgeY=` (EdDSA signing key for appcast entries).
- Checks daily (`SUScheduledCheckInterval=86400`), prompt-to-update (not silent): `SUEnableAutomaticChecks=true`, `SUAutomaticallyUpdate=false`.

**Settings / preferences (tabs)**
- **General** (`GeneralSettingsView`) — warn-on-exit-if-running, output font family, window frame persistence.
- **Storage** (`StorageSettingsView`) — database location, export/restore, clear ad-hoc history.
- **CLI Tools** (`CLIToolsSettingsView`) — install/uninstall, PATH status badges (`~/.local/bin is in PATH` / `~/.local/bin is not in PATH`).
- **License** (`LicenseSettingsView`) — activate / deactivate / transfer / status badge.
- **About** (`AboutSettingsView`, `AboutView`, `AboutWindowController`).
- Additional preference keys: `preferredTerminalBundleId`, `outputFontFamily`, `warnOnExitIfRunning`, `outputBufferSize`, `windowFrameWidth/Height`.

**App-level features**
- Menu items: *About Command Book*, *Check for Updates…*, *Command Book Help* (opens `/docs`), *Show Command Palette*, *Find in Output…*, *Export Commands…*, *Restore Commands…*, *Enter Full Screen*, quit-with-confirmation if running commands.
- Quit confirmation lists names of still-running processes.
- Error alerts funneled through a single `alertError` state on `AppState`.

---

## 3. Technology Stack & Dependencies

### 3.1 Core frameworks (from Mach-O `LC_LOAD_DYLIB` / `LC_LOAD_WEAK_DYLIB`)

| Framework / dylib | Role |
|---|---|
| `SwiftUI.framework` | Primary view layer (`ContentView`, all `*View` files) |
| `AppKit.framework` | Bridged `NSTextField`/`NSTextView` for palette + output |
| `Combine.framework` | `_cancellables` on `UpdateManager`, reactive glue |
| `Observation` (`libswiftObservation.dylib`) | Swift 5.9+ `@Observable` macro state (`_$observationRegistrar`) |
| `CoreTransferable.framework` | Drag-and-drop payloads (sidebar reorder, .env drop zones) |
| `CryptoKit.framework` | License envelope signing / verification |
| `IOKit.framework` | `IOPlatformExpertDevice` machine identifier for license binding |
| `libsqlite3.dylib` | SQLite backend |
| `libswiftConcurrency.dylib` | Swift structured concurrency (`_restartTasks`, `searchDebounceTask`, async license calls) |
| `libswift_StringProcessing.dylib` | Regex literals (`#/ copy( \d+)?$/#`) |
| `UniformTypeIdentifiers` | File type predicates for .env / DB import |
| `Sparkle.framework` (`@rpath`, version B) | Auto-updater |
| `libswiftXPC.dylib` (weak) | Sparkle XPC services |

### 3.2 Third-party Swift packages

- **GRDB.swift** — shipped as `GRDB_GRDB.bundle` (Swift Package Manager resources bundle). Full GRDB surface compiled in: Pool, Queue, Snapshot, FTS3/4/5, Migrations, Observation, Combine publishers, DebugDumpFormat.
- **Sparkle 2** — standard framework with `Autoupdate`, `Updater.app`, `Downloader.xpc`, `Installer.xpc` XPC split.

### 3.3 No third-party runtimes

No Electron, no embedded Node, no WebView is present. No `.framework` other than Sparkle, no embedded Python/Ruby/JS runtime. It is a pure Swift application.

### 3.4 Asset catalog (`Resources/Assets.car`)

Built-in icon library (all stored at 1x + @2x PNG):

```
aws, bun, deno, digitalocean, django, docker, dotnet, ffmpeg, flask,
git, github, go, hugo, java, kubectl, lua, mongodb, mysql, nextjs,
nginx, node, npm, perl, php, pnpm, podman, postgres, python, ruby,
rust, shell, valkey, vite, webpack, xcode, yarn
```

The app also references a `CommandIconResolver` Swift class and an `IconCategory` enum with the categories: **Languages & Runtimes, Package Managers, Web & Frameworks, Databases, DevOps & Cloud, Tools & Other**.

Icon matching is heuristic — binary contains regexes like `^python[23]?(\.\d+)?\s`, `^python[23]?(\.\d+)?$`, `^docker-compose\s`, and a token list of recognized executables: `python, dotnet, kotlin, gradle, composer, bundler, poetry, docker, podman, kubectl, terraform, ansible, digitalocean, gcloud, jekyll, gatsby, webpack, uvicorn, gunicorn, hypercorn, granian, django, fastapi, postgres, valkey, mongodb, sqlite, ffmpeg, pytest, generic`. SF Symbols are used for non-tech categories (`wrench.and.screwdriver`, `checkmark.circle`, `clock.arrow.circlepath`, `square.stack.3d.up`, `arrow.triangle.branch`, etc.).

---

## 4. Bundle Layout

```
Command Book.app/
├─ Contents/
│  ├─ Info.plist                       # bundle metadata, Sparkle keys, AE prompt
│  ├─ PkgInfo                          # 'APPL????'
│  ├─ CodeResources                    # top-level signature
│  ├─ _CodeSignature/CodeResources     # nested signature manifest
│  ├─ MacOS/
│  │   ├─ CommandBook                  # 8.24 MB universal Mach-O (main app)
│  │   └─ commandbook-cli              # 6.66 MB universal Mach-O (CLI companion)
│  ├─ Resources/
│  │   ├─ AppIcon.icns
│  │   ├─ Assets.car                   # ~3.08 MB — tech icons + AppIcon variants
│  │   └─ GRDB_GRDB.bundle/            # GRDB SPM resources (PrivacyInfo.xcprivacy)
│  └─ Frameworks/
│     └─ Sparkle.framework/
│        ├─ Sparkle                    # framework dylib
│        ├─ Autoupdate                 # helper
│        ├─ Updater.app/               # GUI updater
│        ├─ XPCServices/
│        │   ├─ Downloader.xpc/        # sandboxed download XPC
│        │   └─ Installer.xpc/         # privileged install XPC
│        └─ Resources/                 # 41 locales (.lproj/Sparkle.strings)
```

### 4.1 Info.plist keys of interest

```
CFBundleIdentifier           com.talkpython.CommandBook
CFBundleShortVersionString   1.0.46
CFBundleVersion              46
LSMinimumSystemVersion       15.6
LSApplicationCategoryType    public.app-category.developer-tools
NSAppleEventsUsageDescription
  "Command Book needs to send commands to terminal apps to run your
   commands externally."
SUFeedURL                    https://commandbookapp.com/updates/appcast.xml
SUPublicEDKey                THisbdE/WoYjxH9KNTPiqtdYlbYSLc3f/JM1EC8YgeY=
SUEnableAutomaticChecks      true
SUAutomaticallyUpdate        false
SUScheduledCheckInterval     86400
```

Oddity: the plist also contains a few UIKit keys (`UIApplicationSupportsIndirectInputEvents`, `UIStatusBarStyle`, `UISupportedInterfaceOrientations~ipad/iphone`). These are **not active** — `CFBundleSupportedPlatforms=[MacOSX]` and `DTPlatformName=macosx` — but their presence suggests the Xcode project was bootstrapped from a multi-platform template (or planned future iPad support). No iOS slices or Catalyst markers are present in the Mach-O.

### 4.2 Code signature & entitlements

Embedded entitlements (extracted from `LC_CODE_SIGNATURE` → blob `0xfade7171`):

```xml
<plist version="1.0">
<dict>
  <key>com.apple.security.files.user-selected.read-only</key>
  <true/>
</dict>
</plist>
```

This is **the minimum surface for a Developer-ID app that spawns arbitrary subprocesses** — no App Sandbox, no Hardened Runtime exception declarations beyond what Xcode adds by default, just the "user picks a file via NSOpenPanel and we can read it" entitlement needed for the DB-restore / .env-file drop flows. Reading arbitrary files, running arbitrary binaries, talking to Terminal via AppleEvents, writing `~/.local/bin`, and editing `~/.config/fish/config.fish` all work because the app is **not sandboxed** — it relies on standard Unix permissions + macOS TCC prompts.

CLI binary advertises app-group prefix `AR5CLMFJGN.com.talkpython.CommandBook.cli`, tying it to the main app's team identifier.

---

## 5. Internal Architecture

All file names below are exact — extracted from the Swift source-location strings baked into the binary.

### 5.1 High-level object model

```
CommandBookApp (SwiftUI App)
  └─ AppDelegate (NSApplicationDelegate)
      ├─ AppState            (Observable, root of truth)
      │   ├─ savedCommands: [CommandConfig]
      │   ├─ runningProcesses: [RunningProcess]
      │   ├─ sidebarSeparators, sidebarItemOrder
      │   ├─ adHocHistory
      │   ├─ isCommandPaletteVisible / isCommandEditorVisible / isSettingsVisible
      │   ├─ showQuitConfirmation, showCommandLimitAlert
      │   ├─ searchQuery, shouldOpenOutputSearch, shouldFindNext/Previous
      │   ├─ selectedProcessId, editingSeparatorId, commandBeingEdited
      │   ├─ pendingAdHocProcessId, alertError
      │   ├─ outputTextCache: OutputTextCache
      │   ├─ databaseManager: DatabaseManager
      │   ├─ settingsStore: SettingsStore
      │   ├─ processManager: ProcessManager
      │   └─ licenseManager: LicenseManager
      ├─ splitViewObserver
      └─ escapeMonitor (AboutWindowController)
```

Swift `@Observable` is in use (`_$observationRegistrar` symbols in class metadata), which is the Swift 5.9+ macro-based replacement for `ObservableObject`. Underscored storage (`_savedCommands`, `_runningProcesses`, etc.) is consistent with the Observation macro's generated backing storage.

### 5.2 Domain models (Swift + GRDB records)

```
CommandConfig          (TableRecord, PersistableRecord)
  id, name/slug, command, working_directory, pre_command,
  environment_variables, auto_restart, restart_delay_seconds,
  custom_icon_type, sort_order, created_at, updated_at, last_run_at

RunningProcess         (Observable)
  commandConfigId?, originalName, displayName, commandText,
  workingDirectory, autoRestart, preCommand, isAdHoc, customIconType,
  status: ProcessStatus, outputBuffer: RingBuffer,
  startedAt, endedAt, memoryUsageBytes,
  detectedURLs, accumulatedUptime, restartCount,
  hasStartedThisSession, searchState: OutputSearchState,
  process: Process, inputPipe/outputPipe/errorPipe

SidebarItem            (polymorphic: 'process' | 'separator')
SidebarProcess         (legacy table kept for v2 migration)
SidebarSeparator       (label + SF Symbol icon)
AdHocHistoryEntry      (command, working_directory, last_run)

OutputLine             (terminal line with ANSI parse state)
OutputSearchState      (text, isCaseSensitive, isRegexEnabled,
                        matches, currentMatchIndex, regexError)
OutputTextCache        (cachedText, cachedLineCount, scrollPositions)
ProcessStatus          (enum, lifecycle states)
ChainStep              (pre-command → main-command orchestration)
```

### 5.3 GRDB schema (from migration identifiers)

The SQLite file is `commandbook.sqlite`. Migrations (in order) and what they set up:

1. `v1_create_command_configs` — primary `command_configs` table.
2. `v2_create_sidebar_processes` — original sidebar model (one row per pinned process).
3. `v3_add_pre_command` — adds `pre_command` column.
4. `v4_create_adhoc_history` — `adhoc_history` + indexes `idx_adhoc_history_last_run`, `idx_adhoc_history_command_dir`.
5. `v5_sidebar_adhoc_support` — lets sidebar hold ad-hoc entries (schema widened).
6. `v6_create_sidebar_items` — introduces polymorphic `sidebar_items` (process / separator / ad-hoc) with unified `sort_order`. An explicit SQL block copies rows from the old `sidebar_processes` table and then the old table is dropped.
7. `v7_add_custom_icon_type` — adds `custom_icon_type` column to `command_configs`.

Plus indexes: `idx_sidebar_items_sort_order`, `idx_sidebar_processes_sort_order`, `idx_command_configs_name`.

GRDB is configured in typical high-performance mode — strings present: `PRAGMA journal_mode = WAL`, `PRAGMA synchronous = NORMAL`, `PRAGMA foreign_keys = ON`. Integrity: `PRAGMA foreign_key_check` runs after migrations.

### 5.4 Process manager (`ProcessManager.swift`)

- Spawns commands via `Foundation.Process` (not `posix_spawn` directly in the GUI app; the CLI binary does use `posix_spawn`).
- Uses three pipes per process (`inputPipe`, `outputPipe`, `errorPipe`) and appends into a Swift `RingBuffer` + `OutputLine` stream.
- Concurrency-driven auto-restart tasks keyed by process UUID (`_restartTasks`, `_suppressAutoRestartFor`, `_stopRequestedFor`).
- A dedicated periodic task (`_statsUpdateTask`) polls memory usage.
- Human-readable lifecycle log lines are inlined into the process output (visible banners such as ` Restarting... (restart #N)`, ` Sending SIGTERM (graceful stop)...`, ` Auto-restart in Xs`, ` Execution time: ...`).
- URL detection runs continuously over the output buffer; hits are deduped into `_detectedURLs`.

### 5.5 Licensing flow (`LicenseManager.swift`)

Detected sequence:

1. On first activate: POST `/licenses/activate` with `license_key`, platform info, machine ID (from `IOPlatformExpertDevice`), `CFBundleShortVersionString`.
2. Server responds with JSON: `success`, `license_key`, `status`, `activations_used`, `activations_limit`, `is_test`, `message`, plus an opaque `SignedEnvelope` the client stores in Keychain.
3. On app launch: synchronous "verification cache" check (Keychain-backed) then background refresh via POST `/licenses/verify`.
4. Deactivation: POST `/licenses/deactivate` frees an activation slot.
5. Transfer: uses deactivate+activate under a single UI flow ("Transfer License to This Mac").
6. Client-side integrity: `SignedEnvelope` has a signature (CryptoKit-verified) so a tampered Keychain entry produces *"License data integrity check failed"*.

There is one suggestive literal in the binary: `CB-2024-xK9mP4vLqR7nW2jF`, sitting adjacent to license strings. Its exact role is not provable from strings alone — plausibly a seed for a signing key or a default/demo license identifier. Treating it as a secret, I'm not using or testing it; calling it out so the reader doesn't miss it.

### 5.6 Terminal handoff (`TerminalAppManager.swift`)

Strategy per terminal:

- **Warp**: writes a YAML launch config to `~/.warp/launch_configurations/<uuid>.yaml` with `windows: → tabs: → layout: cwd/commands:` and then opens it — Warp's public launch-config mechanism.
- **Generic (Terminal, iTerm2, Ghostty, Kitty, Alacritty)**: writes a small shell script `commandbook-run-*` into a temp dir (the script prints a banner using ANSI grey/cyan/white, announces the command, working dir, pre-command, then execs the command) and launches the user's preferred terminal with `--working-directory=` style flags.
- For Terminal.app / iTerm2 specifically, AppleEvents are used — which is why the TCC prompt in `NSAppleEventsUsageDescription` exists.

### 5.7 CLI companion (`commandbook-cli`)

Same Swift runtime, same GRDB, same SQLite file. Distinct module namespace `commandbook_cli` with its own `CLIDatabaseManager`. Shares the database with the GUI; both processes can read & write because GRDB is in WAL mode. Uses Apple's **Swift Argument Parser** (evident from the huge embedded bash completion template referencing `COMP_LINE`, `COMP_WORDS`, `non_repeating_flags`, etc.). `open` subcommand launches the GUI app via `NSWorkspace`/`open -a`. Interactive picker for `run` with no slug: `"Select a command to run:" → "Enter number (1-N)"`.

### 5.8 Update manager (`UpdateManager.swift`)

Thin wrapper over `SPUStandardUpdaterController`: exposes `canCheckForUpdates` (published via Combine) so the *Check for Updates…* menu item enables/disables correctly. All signing / downloading / installing is delegated to Sparkle's framework + XPC services.

---

## 6. UI Architecture

### 6.1 Scene & window structure

- Single-scene SwiftUI `App` (`CommandBookApp.swift`) using a `WindowGroup`-style main window + a separate About window (`AboutWindowController` subclasses `NSWindowController` and installs an `escapeMonitor` on `NSEvent`).
- Main window uses `NSSplitView` (observed via `splitViewObserver` on `AppDelegate`) with a sidebar + detail pane — classic macOS source-list pattern.
- Window frame is persisted via `SettingsStore` (`windowFrameWidth`, `windowFrameHeight`).
- A Settings scene is opened with the standard ⌘, command and driven by `SettingsView` → a `SettingsTab` enum switching between `GeneralSettingsView`, `StorageSettingsView`, `CLIToolsSettingsView`, `LicenseSettingsView`, `AboutSettingsView`.

### 6.2 Component tree (from Swift type names + file names)

```
ContentView
├─ SidebarView
│  ├─ SidebarProcessRowView
│  │   └─ CommandIcon  + StatusBadge + StatusIndicator
│  ├─ SeparatorRowView
│  └─ AdHocSeparatorView
├─ ProcessDetailView
│  ├─ ProcessDetailHeader
│  │   └─ ProcessStatsView  → StatItem …
│  ├─ ProcessOutputView
│  │   ├─ OutputSearchBarView
│  │   │   ├─ SearchTextField → SearchNSTextField(+ Coordinator)
│  │   │   └─ SearchNavButtonStyle / SearchToggleStyle
│  │   ├─ ProcessOutputTextView      (NSViewRepresentable)
│  │   │   └─ SelectableOutputTextView (NSTextView subclass,
│  │   │         with highlightedRanges + autoScroll toggle)
│  │   └─ ScrollViewConfigurator     (private helper NSView that
│  │                                    tweaks the enclosing NSScrollView)
│  ├─ DetectedURLsView
│  │   ├─ SingleURLLink
│  │   └─ MultipleURLsMenu
│  └─ ProcessDetailFooter
├─ CommandPaletteView  (overlay)
│  ├─ PaletteSearchBar
│  │   └─ PaletteTextField → PaletteNSTextField(+ Coordinator)
│  │       callbacks: onArrowUp, onArrowDown, onReturn,
│  │                  onEscape, onEdit, onDelete
│  ├─ PaletteSection
│  ├─ CommandPaletteRow
│  │   ├─ RunCommandRow
│  │   └─ AdHocHistoryRow
│  ├─ PointerCursorView / PointerCursor
│  └─ EmptyStateView  ("No commands found" / onboarding CTA)
├─ CommandEditorView  (sheet / modal)
│  ├─ Working-directory picker (file coordination)
│  ├─ EnvironmentVariablesEditor
│  │   └─ .env file drop target ("Browse for .env file or drop here")
│  └─ validation inline errors (Working/command/name required)
├─ TimeToUpgradeView  (conditional, feature-gated)
│  ├─ UpgradePrimaryButtonStyle
│  └─ UpgradeSecondaryButtonStyle
└─ (menu bar commands + quit-confirmation alert)
```

### 6.3 Design system (`DesignSystem.swift`)

Centralized reusable visual primitives defined as Swift types:

- **Button styles**: `PrimaryButtonStyle`, `DangerButtonStyle`, `SecondaryButtonStyle`, `SearchNavButtonStyle`, `UpgradePrimaryButtonStyle`, `UpgradeSecondaryButtonStyle`.
- **Toggle style**: `SearchToggleStyle`.
- **Atoms**: `StatusBadge`, `StatusIndicator`, `HighlightedText`, `CLIStatusBadge`, `LicenseStatusBadge`, `CommandIcon`.

### 6.4 AppKit bridging

SwiftUI is the default, but the app drops to AppKit via `NSViewRepresentable` anywhere native text handling matters:

- `PaletteNSTextField` / `PaletteTextField` — custom `NSTextField` subclass that forwards arrow keys, Return, Escape, and Delete as callbacks; required because SwiftUI's `TextField` doesn't give clean access to these key events.
- `SearchNSTextField` — same pattern for the output-search bar, adds `onShiftReturn`.
- `SelectableOutputTextView` / `ProcessOutputTextView` — custom `NSTextView` with attributed-string runs (ANSI-colored), maintained `highlightedRanges` for match highlighting, and `autoScroll` / `lastRenderedCount` / `lastFontDescription` state to make streaming cheap.
- `ScrollViewConfigurator` — hidden `NSView` that inspects its enclosing `NSScrollView` on layout to adjust ruler/insets.

### 6.5 Key interaction affordances

- **Global shortcuts** (from menu strings): Show Command Palette (⌘K), Find in Output… (⌘F by convention), Check for Updates…, Enter Full Screen, Help (`⌘?` opens `/docs`).
- **Drag & drop**: sidebar item reorder; `.env` files dropped onto the editor are parsed into environment variables rows (via `CoreTransferable`).
- **Context menus**: sidebar rows (Copy Working Directory, Reveal in Finder, Duplicate Command…, Delete Separator, Add Category Separator Above, Start all in group, Stop all in group).
- **Alerts / sheets**: Quit confirmation, Command-limit (free tier) alert, License activate/deactivate prompts, CLI install success/failure prompts.
- **Accessibility**: at least some explicit accessibility labels present, e.g. `"Terminal output from the running command"` for the output text view.

### 6.6 State flow

- Single root `AppState` is injected (via `@Environment`-style property wrappers, evidenced by the warning string `"Accessing Environment's value outside of being installed on a View. This will always read the default value and will not update."`).
- Views observe `AppState` through the Observation macro (not `ObservableObject`).
- Persistence happens in `AppState` on mutation (`"Failed to persist sidebar state:"`), so the database is treated as a write-through store rather than a live query source.
- Fields like `shouldOpenOutputSearch`, `shouldFindNext`, `shouldFindPrevious` act as one-shot command flags written by the main menu and consumed by views — a pattern used to bridge `NSMenuItem` actions into SwiftUI state.

---

## 7. Security / Privacy Surface

- **Minimal entitlements.** Only `com.apple.security.files.user-selected.read-only`. Not sandboxed.
- **Network egress.** Only `https://commandbookapp.com` (licensing + appcast). No analytics/telemetry endpoints are visible in strings.
- **Sensitive storage.** License credential lives in Keychain under a dedicated service with a signed envelope verified by CryptoKit.
- **Shell execution.** The app runs arbitrary shell commands in user-specified working directories with user-supplied env vars. That is the product; there's no privilege elevation.
- **AppleEvents.** Purpose string is present; used to drive Terminal / iTerm2. User receives the standard macOS "wants to control Terminal.app" TCC prompt.
- **Filesystem writes outside ~/Library.** Writes the CLI symlink to `~/.local/bin`, may edit `~/.config/fish/config.fish`, and writes a Warp launch config into `~/.warp/launch_configurations/`. All user-owned paths, no root required.
- **Update signing.** Sparkle appcast entries are EdDSA-signed against `SUPublicEDKey`; only updates signed by that key will install.
- **GRDB bundle** ships `PrivacyInfo.xcprivacy` (the empty SPM default from GRDB); no additional tracking/PII disclosures detected.

---

## 8. Notable Reverse-Engineering Evidence (so you can re-verify)

| Claim | Where it came from |
|---|---|
| DMG is UDIF zlib | `koly` trailer at file offset `0x00AEAD52`; XML plist at `0x00AE65B3` with `blkx` chunks using compression type `0x80000005` (zlib). |
| Main bundle & version | `Contents/Info.plist` → `CFBundleShortVersionString=1.0.46`, `CFBundleVersion=46`. |
| Swift + SwiftUI native | `LC_LOAD_DYLIB … SwiftUI.framework`, `libswiftObservation.dylib`; Swift mangled class names like `_TtC11CommandBook…`. |
| GRDB | `/Resources/GRDB_GRDB.bundle/`, `GRDB/*.swift` path strings baked into binary, `libsqlite3.dylib` load. |
| Sparkle | `Frameworks/Sparkle.framework/Versions/B/` with `Autoupdate`, `Updater.app`, `XPCServices/{Downloader,Installer}.xpc`; Info.plist `SUFeedURL` + `SUPublicEDKey`. |
| Licensing endpoints | cstring literals `/licenses/activate`, `/licenses/verify`, `/licenses/deactivate`, host literal `https://commandbookapp.com`. |
| Terminal bundle IDs | literals: `com.apple.Terminal`, `com.googlecode.iterm2`, `dev.warp.Warp-Stable`, `com.mitchellh.ghostty`, `net.kovidgoyal.kitty`, `org.alacritty`. |
| DB schema & migrations | migration-identifier strings `v1_create_command_configs` … `v7_add_custom_icon_type` and embedded SQL (`INSERT INTO sidebar_items …`). |
| CLI subcommands | help strings `List all saved commands`, `Run a saved command`, `Create a new command`, `Open the GUI app`, usage lines `commandbook list / run <slug> / edit <slug> / open / --help`. |
| Entitlements | `LC_CODE_SIGNATURE` blob `0xfade7171` in the main binary, contents shown in §4.2. |
| Free tier 5 commands / 25K lines | alert strings `"You're using all 5 command slots in your Personal license."`, `"Your Personal license includes 25,000 lines."`. |
| URL auto-detect regex | literal present in cstring section (`(?<!:)//(?:localhost|\d{1,3}…`). |

---

## 9. If You Wanted to Build Something Like It

Everything visible in Command Book is achievable with the stack as-used:

- **Shell & language**: SwiftUI + AppKit bridging on macOS 15.6+, `@Observable`, structured concurrency.
- **Storage**: GRDB.swift over SQLite with a migrator; schema kept tiny (`command_configs`, `sidebar_items`, `adhoc_history`).
- **Process management**: `Foundation.Process` with three `Pipe`s per process, a ring buffer, and an async task doing periodic memory sampling (`task_info(TASK_BASIC_INFO)`-style).
- **Output rendering**: `NSTextView` inside `NSViewRepresentable` with attributed strings for ANSI color runs, plus a highlight-range side table for search hits.
- **Command palette**: custom `NSTextField` subclass that surfaces Up/Down/Return/Escape/Delete to SwiftUI via delegate callbacks; everything else is plain SwiftUI lists and filtering.
- **Licensing**: Gumroad-generated keys validated against a tiny HTTP service with machine-bound activation (IOKit platform UUID), signed envelope in Keychain.
- **Updates**: Sparkle 2 with EdDSA-signed appcast; XPC split for sandboxing the download & install helpers.
- **CLI companion**: Swift Argument Parser executable shipped inside the bundle and symlinked into `~/.local/bin` on user consent, sharing the SQLite DB with the GUI in WAL mode.

The pieces that make Command Book feel polished — ANSI output with working find, URL scraping with a one-click menu, drag-orderable sidebar with user-defined separators, "Open in Terminal" across six terminals with Warp's YAML launch config — are all small, focused subsystems around the same AppState/Process/DB core.

---

## 10. Extracted Artifact Index

Files produced during this teardown (temporary workspace; provided only for reference — not re-packaged into the output folder):

- `CommandBook_extracted/Command Book.app/` — fully extracted bundle.
- `apfs_partition.img` — reconstructed Apple_APFS partition (`~21.5 MB`).
- `dmg_plist.xml` — decoded UDIF resource-fork plist.
- `CommandBook.strings.txt` — ~45,700 strings from the main binary.
- `cli.strings.txt` — ~34,300 strings from the CLI binary.

---

*Report generated from static analysis only. No runtime behavior was observed. Claims are grounded in plist keys, Mach-O load commands, code-signature-embedded entitlements, and cstring literals present in the shipped binaries; nothing has been inferred from vendor marketing material.*
