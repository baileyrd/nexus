# Obsidian Runtime — Reverse-engineered Reference

> Basis: `obsidian.asar` extracted to `C:/Users/baile/AppData/Local/Temp/obsidian-extract/obsidian-unpacked/`.
> All `app.js` byte-offsets are from the single-line 3.72 MB bundle in that directory.
> "Minified symbol" entries (`Tte`, `VT`, `Kg`, …) are the webpack-mangled class identifiers inside that bundle; names in **bold** are the public names from `obsidian-api.d.ts` (cited inline where relevant).

---

## Overview

### Files shipped in the asar

| File | Size | Role |
|---|---|---|
| `app.js` | **3,723,601 B** | Full renderer runtime — App, Workspace, all views, plugin host, vault I/O. Boots by constructing `window.app = new Tte(adapter, appId)` on the last line. |
| `starter.js` | 416,349 B | First-launch / vault-picker renderer; separate webpack bundle (3 modules). Uses IPC channels `vault-list`, `vault-open`, `open-vault`, `create-vault`. |
| `enhance.js` | 13,346 B | DOM polyfill shim (`Object.hasOwn`, `ResizeObserver`, `matchMedia.addEventListener`, stub `TouchEvent`). Loaded first, no exports. |
| `main.js` | 60,101 B | Electron main process — window/tray/update management. Out of scope here. |
| `help.js` | 356,766 B | In-app help viewer. Separate bundle. |
| `sim.js` | 17,693 B | Plugin sandbox simulator bootstrap. |
| `i18n.js` + `i18n/` | ~129 KB + per-locale JSON | Translation tables. Strings reached via `gm.interface.*()` / `gm.plugins.*.*()` accessor calls. |
| `app.css` | 600,413 B | All shipped CSS; already documented separately. |

### Bundler fingerprint

Every JS file is **webpack 5** with the classic wrapper:

```
(()=>{var e={134:(e,t,n)=>{...},...},t={};function n(i){...}...})()
```

- `app.js` declares **184 webpack modules**: 114 in the `(e,t,n)=>` shape, 10 `(e,t)=>`, 60 `e=>` single-export modules.
- No source maps, no `//# sourceURL`. Identifier mangling is the standard webpack/terser two-character scheme; long public names survive only as object keys, prototype keys, and inside string literals.
- Uses TypeScript's generator runtime (`function y(e,t,n,i){…generator stepper…}` and `function b(e,t){…`) — confirms source is TypeScript compiled to ES5 with generator helpers.

### High-level module count / size breakdown (rough, by greppable signature)

| Cluster | Approx offset range | Bytes | What lives here |
|---|---|---|---|
| Polyfills + DOM helpers | 0 – 250 k | ~250 k | `createDiv`, `setChildrenInPlace`, `Mv` (tooltip), `tv` (icon), color utilities |
| Icon atlas (`Um`) | ~705 k – 780 k | ~75 k | Lucide icon path data as JS object literal |
| Component / Events / Scope / Keymap / Modal / Menu / Notice / Setting | 700 k – 1,200 k | ~500 k | Base UI primitives |
| View base + WorkspaceLeaf + splits | 1,250 k – 1,420 k | ~170 k | `Kg` View, `jD` WorkspaceLeaf, `FD`/`zD`/`L0`/`I0`/`O0` splits |
| Vault + Adapter + MetadataCache | 1,340 k – 1,620 k | ~280 k | File I/O, link graph, FrontmatterCache |
| Markdown renderer / CodeMirror wiring | 1,700 k – 2,500 k | ~800 k | MarkdownView, live-preview extensions |
| Editor suggest, outline, backlinks | 2,200 k – 2,600 k | ~400 k | Built-in views |
| Workspace class + ribbon + statusbar | 2,615 k – 2,730 k | ~115 k | `H0` Workspace, ribbon, status bar |
| Plugin base class + plugin loader | 2,738 k – 2,870 k | ~130 k | `G0` Plugin, `$0` Plugins, `o2` InternalPlugins |
| Internal plugins (file-explorer, bookmarks, search, daily-notes, …) | 2,870 k – 3,520 k | ~650 k | The 20-odd built-in plugins |
| App + init + progress + vault setup | 3,510 k – 3,724 k | ~210 k | `Tte` App class, boot sequence |

---

## Class taxonomy

Public names come from Obsidian's published `obsidian-api.d.ts` (Anthropic cutoff: the shape is stable across 1.x). Minified symbols are the actual names in `app.js`.

| Public name | Minified | Offset | Notes |
|---|---|---|---|
| **App** | `Tte` | ~3,665,500 | Singleton; `window.app = new Tte(adapter, appId)` |
| **Events** | `VT` | ~1,337,000 | Tiny base with `on/off/offref/trigger/tryTrigger` |
| **Component** | unnamed IIFE | ~701,000 | `load/unload/_loaded/addChild/removeChild/register/registerEvent/registerDomEvent/registerScopeEvent/registerInterval` |
| **Scope** | `Ng` | ~1,029,000 | `register(mods,key,fn) / unregister / handleKey / setTabFocusContainerEl / parent` |
| `ScopeWithCallback` | `Rg` | just after `Ng` | Scope subclass whose `handleKey` delegates to `cb()` (used for scoped modals whose active scope changes) |
| **Keymap** | `Bg` | ~1,030,000 | Static `Bg.global` singleton; `compileModifiers/isMatch/isModEvent`; installs global `keydown`/`focusin` listeners |
| **HotkeyManager** | `zb` | ~1,111,000 | Default + custom hotkey maps, bakes at call time, reads/writes `.obsidian/hotkeys.json` |
| **Modal** | `tb` | ~1,075,000 | `modal-container > modal-bg + modal(.modal-close-button, .modal-header > .modal-title, .modal-content)` |
| **Notice** | `fb` | ~1,089,700 | `.notice-container` auto-created per window, stored in WeakMap `db` |
| **Menu** | — (near `Ug`) | ~1,038,000 | `addItem/addSeparator/addSections/setSectionSubmenu/showAtPosition/showAtMouseEvent/setParentElement/setNoIcon/setUseNativeMenu` |
| **MenuItem** | `Hg` | ~1,033,000 | `setTitle/setIcon/setSection/onClick/handleEvent` |
| **MenuSeparator** | `zg` | near `Hg` | |
| **Setting** | `zk` | ~1,160,000 | `setName/setDesc/setClass/setTooltip/…setting-item > setting-item-info(name,description) + setting-item-control` |
| **View** | `Kg` | ~1,048,000 | Base; extends Component. `containerEl=workspace-leaf-content`, `data-type`; `open(el)`/`close()`/`onOpen()`/`onClose()`; abstract `getViewType/getDisplayText/getIcon` |
| **ItemView** | unnamed (subclass of `Kg`) | ~1,050,000 | Adds `headerEl`, `actionsEl`, `addAction(icon,title,cb)`, `onMoreOptions`, `onPaneMenu` (sections `close,pane,open,action,find,info,info.copy,view,view.linked,system,"",danger`) |
| *DeferredView* | `eD` | ~1,355,000 | Placeholder view used when a tab type isn't yet loaded; rerenders on insert/click |
| *EmptyView* | `tD` | ~1,356,000 | `getViewType()==="empty"`. Shown when leaf type is unregistered or fallback. |
| *UnknownView* | `nD` | near `tD` | Used when restored layout references a missing view type |
| **WorkspaceItem** | — | ~1,350,000 | Base w/ `parent`, `containerEl` |
| **WorkspaceLeaf** | `jD` | ~1,365,000 | `view/pinned/group/history/working`; `getViewState/setViewState/open/detach/setGroup/togglePinned/loadIfDeferred/handleDrop/updateHeader/recordHistory` |
| **WorkspaceTabs** | `zD` | ~1,500,000 | Tab-group container; `selectTabIndex`, `setStacked` |
| **WorkspaceSplit** | `OD` | — | Non-root split |
| **WorkspaceRoot** | `L0` | — | Root (main) split. `serialize()` key `"split"` with `direction`,`children` |
| **WorkspaceSidedock** | `FD` | — | Left/right sidedock split; `setDirection/collapse/recomputeChildrenDimensions` |
| *MobileSidedock* (left/right) | `_D` / `GD` | — | Mobile drawer variants |
| **WorkspaceFloating** | `I0` | — | Floating (popout host) container; `serialize()` type `"floating"`, children of type `"window"` |
| **WorkspaceWindow** (popout) | `O0` | — | Only constructed when `Yl.isDesktopApp && qf>=13` (Electron ≥ 13) |
| **Workspace** | `H0` | ~2,615,000 – 2,730,000 | see [Workspace model](#workspace-model) |
| **ViewRegistry** | `p0` | ~2,615,000 | `viewByType{}` + `typeByExtension{}`; `registerView/unregisterView/registerExtensions/unregisterExtensions/registerViewWithExtensions/getViewCreatorByType`; triggers `"view-registered"`/`"view-unregistered"`/`"extensions-updated"` |
| **Vault** | `JT` | ~1,342,000 | extends Events. Uses `adapter` (either Node `FileSystemAdapter` `Eu` on desktop or mobile Capacitor adapter). `read/cachedRead/readBinary/readRaw/create/createBinary/createFolder/modify/modifyBinary/append/process/delete/trash/rename/getName/getAbstractFileByPath/getFileByPath/getFolderByPath/getFiles/getMarkdownFiles/getAllLoadedFiles/getResourcePath` |
| **TAbstractFile** | (base class, no unique tag) | — | `path/name/parent/vault` |
| **TFile** | `$T` | — | Vault uses `instanceof $T` tests. Adds `stat`, `basename`, `extension`, plus internal `cache(content)` for `cachedRead`. |
| **TFolder** | `ZT` | — | Vault uses `instanceof ZT`. Adds `children`, `isRoot()` |
| **FileManager** | `mI` | ~1,610,000 | `getNewFileParent/renameFile/trashFile/generateMarkdownLink/processFrontMatter/insertTextIntoFile` |
| **MetadataCache** | `_L` | ~1,570,000 | `on('changed'|'resolve'|'resolved'|'deleted'|…)`, `getFileCache/getCache/getFirstLinkpathDest/resolvedLinks/unresolvedLinks/computeMetadataAsync` |
| **MetadataTypeManager** | `RL` | — | Tracks property-type overrides (Properties pane) |
| **Commands** (App.commands) | `Y6` | ~2,896,600 | `addCommand/removeCommand/findCommand/listCommands/executeCommandById/executeCommand` + `editorCommands[]` side-index |
| **FoldManager** | `Q6` | ~2,898,500 | Per-file fold state persisted in `localStorage` under `${appId}-note-fold-${path}` |
| **Plugin (base)** | `G0` | ~2,738,500 | Extends Component. see [Plugin API surface](#plugin-api-surface) |
| **Plugins (host)** | `$0` | ~2,745,000 – 2,765,000 | Reads enabled-set from `community-plugins` vault config, loads manifests, `eval`s each `main.js` in an anonymous function. |
| **InternalPlugins** | `o2` | ~2,746,000 | Mirrors Plugins but for first-party plugins (`file-explorer`, `bases`, `bookmarks`, `outline`, `graph`, `search`, `daily-notes`, `backlink`, `canvas`, `properties`, `publish`, `sync`, `switcher`, `workspaces`, `note-composer`, `page-preview`, `random-note`, `slash-command`, `slides`, `templates`, `webviewer`, `word-count`, `file-recovery`, `zk-prefixer`, `markdown-importer`, `tag-pane`). |
| **SettingTab (base)** | `Yg` | ~1,160,000 | `display/hide/containerEl`. Subclass `u0 → h0 "releases"` shipped. |
| **Setting (App.setting)** | `vte` | ~3,380,000 | Modal that hosts all tabs. `addSettingTab/removeSettingTab/open(tabId)/close/setCurrentTab`. |
| **StatusBar** | `Nee` | ~3,310,000 | `.status-bar` container; `registerStatusBarItem()` → returns detachable `HTMLElement` |
| **CustomCss** | `Ib` | — | `theme-dark/theme-light` toggle; snippets dir watch |
| **DragManager** | `xP` | — | Drag ghost element + overlay targets; `handleDrop(el, cb)`, `hideOverlay()` |
| **Keymap.init()** | `Bg.init` | 1,030,280 | Returns `Bg.global`, constructs root `Ng` scope |
| **EmbedRegistry** | `aJ` | — | `registerEmbed` for in-line file embeds |
| **CLI** | `CA` | — | Registers command-line handlers (`app.cli.registerHandler(name, label, fn, schema)`). Used by plugins to expose `obsidian://` URLs as CLI commands on desktop. |
| **ShareReceiver** | `t4` | — | iOS/Android share intent receiver |
| **AppMenuBarManager** | `$g` | — | Desktop-only; renders native menubar from registered commands |
| **LeftRibbon / RightRibbon** | unnamed IIFE | ~2,675,000 | `side-dock-ribbon.mod-left / .mod-right`; `addRibbonItemButton/removeRibbonAction` |

---

## Workspace model

### Serialization — `.obsidian/workspace.json`

- File name resolved at call time: `F0 = "workspace.json"` (desktop), `N0 = "workspace-mobile.json"` (mobile). Constants referenced by `readWorkspaceFile/saveLayout`.
- `Workspace.saveLayout()` (offset ~2,709,000) calls `adapter.write(configDir + '/' + F0, JSON.stringify(getLayout(), null, 2))` — note the explicit `null, 2` pretty-print.
- `Workspace.getLayout()` (offset ~2,708,000) emits:

```
{
  main:   rootSplit.serialize(),
  left:   leftSplit.serialize(),
  right:  rightSplit.serialize(),
  "left-ribbon": leftRibbon.serialize(),   // { hiddenItems: { [id]: bool } }
  floating?: floatingSplit.serialize(),    // omitted if no floating windows
  active: activeLeaf?.id,
  lastOpenFiles: recentFileTracker.serialize()
}
```

### Node types in the serialized tree

| `type` | Reconstructed class | Extra keys |
|---|---|---|
| `split` | `L0` (root) / `OD` (generic) / `FD` (sidedock — only if child of left/right) | `direction: "horizontal"\|"vertical"`, `children[]`, `id?`, for sidedock `width?`, `collapsed?` |
| `tabs` | `zD` | `children[]`, `currentTab`, `stacked?` |
| `leaf` | `jD` | `state: { type, state: {…} }`, `id`, `group?`, `pinned?`, `icon?`, `title?`, `dimension?` |
| `floating` | `I0` | `children: window[]` (non-window children are stripped on deserialize) |
| `window` | `O0` | only reconstructed if `Yl.isDesktopApp && qf>=13`; carries window geometry |
| `mobile-drawer` | `_D`/`GD` | mobile only; `currentTab`, `pinned?` |

`children[i].dimension` applies after the fact via `setDimension(l)` and `recomputeChildrenDimensions()`.

### Deserialization

`Workspace.deserializeLayout(node, where)` (~2,703,500) dispatches on `node.type`. For leaves it does:

```
const leaf = new jD(app, node.id);
await leaf.setViewState(node.state || {});
// if leaf.view is null → leaf.detach(); return null
// else apply group / pinned, push into parent
```

`WorkspaceLeaf.setViewState` (~1,406,500) resolves the view type via
`app.viewRegistry.getViewCreatorByType(state.type)`. If the creator exists → invoke it. Otherwise one of three fallbacks:

1. `state.type` is a string but non-empty → `new nD(leaf, state.type)` (*UnknownView*)
2. `state.type === "empty"` or missing → `this._empty` (*EmptyView*, `getViewType()==="empty"`)
3. If the leaf isn't yet visible & `state.icon` + `state.title` are set → `new eD(leaf, state.type, state.icon, state.title)` (*DeferredView*), which swaps in the real view on first click / DOM-insert.

This is the **"empty leaf fallback"** — it's actually three-tiered (unknown / empty / deferred) and picked per-code-path, not one substitution.

### Active-leaf tracking

- `Workspace.activeLeaf` is a single slot.
- `Workspace.setActiveLeaf(leaf, {focus})` (~1,382,300) pushes onto per-split "most-recent" stacks; `getMostRecentLeaf(split)` and `getMostRecentLeafAmongstRootSplits(…)` walk those stacks.
- Per-split "last active" is tracked on the split node itself so restoring layout re-focuses the correct leaf per pane.
- `requestActiveLeafEvents` is a debounced emitter (see `wc(fn, ms)` debounce utility pervasive in the bundle) that fires `active-leaf-change` once per tick even if multiple leaves change.

### Left / right / root splits

- Root: `new L0(this, "vertical")` — always vertical.
- Sidedocks: `new FD(this, "horizontal", "left"|"right")` — if missing at load time a collapsed side-dock is synthesized.
- Mobile replaces `FD` with `_D`/`GD` (drawer variants) and wraps the container differently: `containerEl.setChildrenInPlace([left, main, right])` with no ribbons. Desktop: `[leftRibbon, left, main, right, rightRibbon]`.

### Popout windows

- `Workspace.floatingSplit = new I0(this)`. Each `O0` child is a BrowserWindow-backed view root, only constructed on Electron ≥ 13.
- On window open: `Workspace.trigger("window-open", leaf, win)`.
- On close: `Workspace.trigger("window-close", leaf, win)`.
- Mobile skips all popout logic — `"window"` nodes are stripped from the deserialized floatingSplit.

---

## View lifecycle

### Base class `Kg`

```js
constructor(leaf) {
  super();                         // Component: _children=[], _events=[]
  this.icon = "lucide-file";
  this.navigation = false;
  this.app = leaf.app;
  this.leaf = leaf;
  this.containerEl = leaf.containerEl.createDiv("workspace-leaf-content");
  this.containerEl.setAttribute("data-type", this.getViewType());
}
open(parent)  { parent.appendChild(this.containerEl); this.load(); await this.onOpen(); }
close()       { this.containerEl.detach(); this.unload(); await this.onClose(); }
onOpen()  { /* override */ }
onClose() { /* override */ }
```

So **`onload`/`onunload` are from Component** (offset ~701,000) and fire before `onOpen`/after `onClose` respectively. `onload` registers teardown callbacks via `this.register(() => …)` that run during `unload`.

### Registration

```js
// From Plugin.prototype.registerView (offset ~2,739,694):
this.app.viewRegistry.registerView(type, creator);
this.register(() => {
  app.viewRegistry.unregisterView(type);
  if (this._userDisabled) app.workspace.detachLeavesOfType(type);
});
```

ViewRegistry.registerView throws if `type` is already registered (`Attempting to register an existing view type "…"`).

### `.view-content` note

There is **no `.view-content`** class in the actual runtime — the leaf content root is `workspace-leaf-content` with `data-type="<view-type>"`. ItemView adds `.view-header`, `.view-header-title`, `.view-header-nav-buttons-container`, `.view-actions`. Plugin content typically goes under `view.contentEl` which equals `containerEl.createDiv("view-content")` for ItemViews only.

### Conventions (matches public `obsidian-api.d.ts`)

- `getViewType()` — stable id, used as `state.type` in `workspace.json` and key in `ViewRegistry.viewByType`.
- `getDisplayText()` — shown in tab header + window title.
- `getIcon()` — Lucide id string (`"lucide-file"` default). Can also be a raw SVG key from the `Um` icon atlas.
- `getState()` / `setState(state, result)` — persisted per-tab; `result` is the `{history, layout, close}` flags leaf passes in.
- `getEphemeralState()` / `setEphemeralState(state)` — transient (cursor, scroll) persisted to tab history only.

---

## Plugin API surface

### `Plugin` base class (`G0`, offset ~2,738,500)

Direct inheritance chain: `Plugin → Component → Object`. All registration helpers work by calling the corresponding **App-level** service and pushing an undo into `this._events` via `register(cb)`. Every one of these is documented in the public `obsidian-api.d.ts` — implementation matches the published signature.

| Plugin method | Delegates to | Notes |
|---|---|---|
| `load()` | (Component) | Sets `_loaded`, awaits `onload`, loads children. |
| `onload()` | (override) | Empty default. |
| `unload()` / `onunload()` | (Component) | Pops `_children`, runs `_events` teardown callbacks. |
| `addRibbonIcon(icon,title,cb)` | `app.workspace.leftRibbon.addRibbonItemButton(id, icon, title, cb)` where `id = manifest.id + ":" + title` | Returned element is a `clickable-icon side-dock-ribbon-action` div. |
| `addStatusBarItem()` | `app.statusBar.registerStatusBarItem()` | Adds class `plugin-<manifestId>`. Returns the element. |
| `addCommand(cmd)` | `app.commands.addCommand({ ...cmd, id: manifest.id+":"+id, name: manifest.name+": "+name })` | Prefixes id and name. |
| `removeCommand(id)` | `app.commands.removeCommand(manifest.id+":"+id)` | |
| `addSettingTab(tab)` | `app.setting.addSettingTab(tab)` | |
| `registerView(type, creator)` | `app.viewRegistry.registerView(type, creator)` | On unload also `detachLeavesOfType(type)` if `_userDisabled`. |
| `registerHoverLinkSource(id, info)` | `app.workspace.registerHoverLinkSource` | |
| `registerExtensions(exts, type)` | `app.viewRegistry.registerExtensions(exts, type)` | |
| `registerMarkdownPostProcessor(fn, sortOrder?)` | Module-level `Gz.registerPostProcessor`, triggers `workspace.trigger("post-processor-change")` | |
| `registerMarkdownCodeBlockProcessor(lang, fn, sortOrder?)` | Wraps `Gz.createCodeBlockPostProcessor` then registers | |
| `registerBasesView(type, creator)` | `app.internalPlugins.getEnabledPluginById("bases").registerView(type, creator)` | No-op if Bases plugin disabled — returns `false`. |
| `registerGlobalFunc(fn)` | `QW.addGlobal(fn)` | Adds to Templater/Dataview-style global function namespace. |
| `registerInstanceFunc(type, fn)` | `QW.addForType(type, fn)` | Type-scoped. |
| `registerCodeMirror(cb)` | **no-op** | Kept only for CM5 API compatibility. |
| `registerEditorExtension(ext)` | `app.workspace.registerEditorExtension(ext)` | Pushes into CM6 extension set (live-preview + source). |
| `registerObsidianProtocolHandler(action, cb)` | `app.workspace.registerObsidianProtocolHandler` | Handles `obsidian://<action>?…` URLs. |
| `registerEditorSuggest(suggest)` | `app.workspace.editorSuggest.addSuggest(suggest)` | |
| `registerCliHandler(cmd, description, fn, schema)` | `app.cli.registerHandler(cmd, "[<manifest.name>]: "+description, fn, schema)` | Desktop-only. |
| `loadData()` | `app.vault.readPluginData(manifest.dir)` | If `onExternalSettingsChange` is defined, tracks `_lastDataModified` for change detection. |
| `saveData(data)` | `app.vault.writePluginData(manifest.dir, data)` | |
| `onUserEnable()` / `onUserDisable()` | (override) | Called when the user toggles plugin enable (not on normal load). Checked at offset ~2,528,460 and ~2,761,170. |

### Plugin discovery & lifecycle (`Plugins $0`, ~2,745,000)

1. On app init, `app.plugins.initialize()` reads `.obsidian/community-plugins.json` (array of manifest ids) via `app.vault.readConfigJson("community-plugins")` → stored as `this.enabledPlugins = new Set(list)`.
2. `loadManifests()` scans `.obsidian/plugins/*/manifest.json` (constant `$D = "manifest.json"`). `main.js` is `ZD`. `styles.css` is `JD`. Each manifest is parsed into `this.manifests[id]`; authors literally named "obsidian" have `author` blanked out.
3. `loadPlugin(id, userEnabling=false)`:
   - Short-circuits if already loaded.
   - Reads `plugins/<id>/main.js` via `adapter.read`.
   - Wraps it:
     ```
     eval(`(function anonymous(require, module, exports){${main}\n})\n//# sourceURL=plugin:${encodeURIComponent(id)}`)(require, module, exports)
     ```
   - `require` is a locally-scoped function: first checks a **deprecated list `_0`** (logs a warning and returns the polyfill), then a **supported list `W0`**, then on `emulate-mobile` rejects any Node require with a Notice, else on desktop calls the bundled `If(name)` Node-module loader.
   - Instantiates `new PluginClass(app, manifest)`, verifies `instanceof G0`, pushes into `this.plugins`, runs `load()` + `loadCSS()` (which injects `styles.css` into the document with `style-name="plugin-<id>"`).
   - If `userEnabling`, calls `onUserEnable()`.
4. `unloadPlugin(id, disabling=false)` sets `_userDisabled`, runs `unload()`, `delete this.plugins[id]`.
5. **Safe mode** = `localStorage.getItem("enable-plugins") === null`. Shown at `localStorage.getItem(…safe mode…)` check at offset ~2,746,500. Set to `"1"` after the user accepts the "community plugins" warning.
6. `InternalPlugins o2` mirrors the same machinery but reads the `.obsidian/core-plugins.json` file and instantiates hard-coded classes (no `eval`).

### Community registry fetch

- Fetches `https://github.com/obsidianmd/obsidian-releases/raw/master/community-plugins.json` via `ty(…)` URL builder. Cached 5 minutes (`yb(fn, 300_000, 60_000)`).
- Stats JSON is the companion `community-plugin-stats.json`.
- Constants `eA` (list) and `nA` (stats) at offset ~1,613,800.

---

## Events / signal bus

`VT` class (~1,337,000) — the common ancestor of Workspace, Vault, MetadataCache, and ViewRegistry.

```js
class VT {
  constructor() { this._ = {}; }                 // eventName -> EventRef[]
  on(name, fn, ctx) { const ref = {e:this,name,fn,ctx}; (this._[name] ??= []).push(ref); return ref; }
  off(name, fn)    { /* filters by fn */ }
  offref(ref)      { /* filters by ref identity */ }
  trigger(name, ...args) { [...this._[name]||[]].forEach(r => this.tryTrigger(r,args)); }
  tryTrigger(r,args) { try { r.fn.apply(r.ctx, args) } catch (e) { setTimeout(()=>{throw e},0) } }
}
```

No native `tree` or `.off` chain. Error handling rethrows asynchronously so a failing listener doesn't cancel the rest.

### Known event names (confirmed by grep, number of unique `trigger("...")` sites)

**Workspace-owned**

| Event | Sites | Notes |
|---|---|---|
| `layout-change` | 4 | After any structural change (leaves added/removed/rearranged). |
| `active-leaf-change` | 4 | Debounced via `requestActiveLeafEvents` |
| `file-open` | 4 | Fires with `TFile|null` when active leaf's file changes |
| `editor-change` | 1 | Editor text changed in `MarkdownView` |
| `editor-menu` | 2 | Right-click in editor — callback receives `(menu, editor, view)` |
| `file-menu` | 2 | Right-click on file/folder in explorer or breadcrumb |
| `leaf-menu` | 1 | Tab header "more options" menu (hook point for plugins) |
| `window-open` / `window-close` | 1/1 | Popout window lifecycle |
| `post-processor-change` | 2 | Fired by Plugin register/unregisterMarkdownPostProcessor |
| `quit` | 2 | Pre-quit, used by plugins to flush |
| `layout-ready` | 1 | After first deserialize completes (latched — `onLayoutReady(fn)` calls immediately once fired) |
| `css-change` | 2 | CustomCss theme/snippet reload |
| `view-registered` / `view-unregistered` / `extensions-updated` | 1/1/1 | ViewRegistry-owned but re-emitted on workspace via custom plugin patterns |

**Vault-owned** (`app.vault.on(…)`)

| Event | Notes |
|---|---|
| `create` | `TAbstractFile` after creation |
| `modify` | `TFile` only |
| `delete` | `TAbstractFile` |
| `rename` | `(file, oldPath)` |
| `raw` | low-level path write — used by HotkeyManager to watch `hotkeys.json`, by CustomCss for snippets |
| `closed` | Vault closed (adapter gone) — app reopens vault chooser |
| `config-changed` | A `.obsidian/*.json` config was rewritten externally |

**MetadataCache-owned**

| Event | Notes |
|---|---|
| `changed` | Per-file after parse |
| `resolve` | Per-file after link resolution |
| `resolved` | All pending link resolutions drained |
| `deleted` | File cache invalidated |

---

## Commands + hotkeys

### Commands (`Y6`, ~2,896,600)

```js
class Y6 {
  constructor(app) { this.commands={}; this.editorCommands={}; this.app=app; }
  addCommand(cmd) {
    // If cmd.mobileOnly && !isMobile → drop silently
    // If cmd.editorCallback / cmd.editorCheckCallback → synthesize cmd.checkCallback
    //   that refuses when active editor is in preview (unless cmd.allowPreview)
    //   or when caret is in inlineTitle / titleEl / metadata-container (unless cmd.allowProperties)
    // Store in this.editorCommands[id] as well.
    if (cmd.showOnMobileToolbar) this.editorCommands[cmd.id] = cmd;
    this.commands[cmd.id] = cmd;
    if (cmd.hotkeys) app.hotkeyManager.addDefaultHotkeys(cmd.id, cmd.hotkeys);
  }
  listCommands() { /* filters out any whose checkCallback(true) throws or returns falsy */ }
  executeCommandById(id, evt)  { const c=this.commands[id]; return c && this.executeCommand(c, evt); }
  executeCommand(cmd, evt) {
    this.app.lastEvent = evt || null;
    try { K6(cmd); } catch(e){ console.error(e); return false; }
    return true;
  }
}
function K6(cmd) {
  if (cmd.checkCallback) cmd.checkCallback(false);        // execute path
  else if (cmd.callback)   cmd.callback();
  else console.error(`Command ${cmd} did not provide a callback`);
}
```

### Hotkey manager (`zb`, ~1,111,000)

- Stores two maps: `defaultKeys` (registered by plugins via `cmd.hotkeys`) and a **custom** map held under `this[Hb]` where `Hb = Symbol("customKeys")`.
- Persistence: `app.vault.readConfigJson("hotkeys")` / `writeConfigJson("hotkeys", …)` → `.obsidian/hotkeys.json`.
- Watches that file via `vault.on("raw", …)` with a 50 ms debounce (`wc(load, 50)`).
- `bake()` flattens custom overriding default, producing parallel arrays `bakedHotkeys[]` + `bakedIds[]`.
- Installed as the root-scope catch-all: `this.app.scope.register(null, null, this.onTrigger.bind(this))`. `onTrigger(evt, ctx)` loops baked entries, calls `Bg.isMatch`, and on a hit:
  - `app.commands.findCommand(id)`; skip if `evt.repeat && !cmd.repeatable`.
  - `app.commands.executeCommand(cmd, evt)` (then consumes the event).
- Display form: `Rb(hotkey)` formats modifiers then key. Joiner is `" "` on macOS (`jl === true`) or `" + "` otherwise. Modifiers enum `Fg = ["Mod","Ctrl","Meta","Shift","Alt"]`, with macOS mapping `Mod→Meta`.

### Hotkey JSON shape

```
{ "<commandId>": [ { "modifiers": ["Mod","Shift"], "key": "O" }, … ] }
```

- Older entries use `"code": "KeyO"` instead of `"key"`; `Nb(code)` strips the `"Key"` prefix when it's a 4-char code.
- `compileModifiers(mods)` returns a canonical concatenation of the `Fg` order, so `["Shift","Mod"]` and `["Mod","Shift"]` compare equal.

### Default hotkeys

Default bindings are **not** in one table — each internal plugin calls `addCommand({…, hotkeys:[…]})` at its own module. Examples visible near offset ~2,900,000+ are the navigate-back/forward arrows on the tab header.

---

## Ribbon / tab / sidebar specifics

### Ribbon (`workspace-ribbon side-dock-ribbon`, ~2,675,000)

```js
class LeftRibbon {
  constructor(workspace, side) {
    this.items = [];
    this.containerEl = createDiv(`workspace-ribbon side-dock-ribbon mod-${side}`);
    if (side === "left") {
      this.ribbonItemsEl   = this.containerEl.createDiv("side-dock-actions");
      this.ribbonSettingEl = this.containerEl.createDiv("side-dock-settings");
    }
    this.containerEl.addEventListener("contextmenu", this.onContextMenu);
  }
  addRibbonItemButton(id, icon, title, callback) {
    const btn = this.makeRibbonItemButton(icon, title, callback);
    // Gc(btn, btn, ribbonItemsEl, 5, noop, reorderCb) — drag-reorder with 5px threshold
    // Either mutate existing item by id, or push new { id, icon, title, callback, hidden:false }.
    this.items.push(or mutate); item.buttonEl = btn;
    this.onChange(false); return btn;
  }
  makeRibbonItemButton(icon, title, cb) {
    const el = createDiv("clickable-icon side-dock-ribbon-action");
    el.onClickEvent(cb); Mv(el, title, {delay:sv, placement:"right"}); tv(el, icon);
    return el;
  }
  setCollapsedState(c) { this.containerEl.toggleClass("is-collapsed", c); }
  serialize() { return { hiddenItems: Object.fromEntries(items.map(i=>[i.id, i.hidden])) }; }
}
```

- Order persists via the `items[]` array (load step sorts by saved key order).
- Right-click shows a menu with one item per ribbon button toggling `hidden`.

### Tab header (inside `zD` WorkspaceTabs)

- Every leaf has a `tab-header-container` child with class `workspace-tab-header-container`.
- Drag reorder uses the same `Gc` helper with a 5px move threshold and a live `.drag-ghost.mod-leaf` clone (`workspace-fake-target-overlay`).
- Pinning: `jD.togglePinned()` / `setPinned(bool)` toggles `is-pinned` on the tab header, disables close button and prevents auto-replace when opening new files.
- Stacking: `zD.setStacked(true)` adds `.mod-stacked`, switches tab headers to vertical/spring-loaded mode.

### Sidebar activation

- Ribbon-icon → internal-plugin mapping is pairwise — each internal plugin adds its own ribbon button inside its `onload`, using `addRibbonItemButton(id, icon, label, () => workspace.ensureSideLeaf(…))`. `file-explorer`, `search`, `bookmarks`, `graph`, `outline`, `backlink` each do this.
- `Workspace.ensureSideLeaf(type, side, {active,reveal})` is the convention method that either reveals an existing leaf of that type in the requested side or creates one.

### Status bar (`Nee`, ~3,310,000)

```js
class StatusBar {
  constructor(app, containerEl) { this.app=app; this.containerEl=containerEl; }
  registerStatusBarItem() {
    const el = this.containerEl.createDiv("status-bar-item");
    return el; // caller owns el.detach() via Plugin.register.
  }
}
```

---

## File explorer + vault I/O

### `file-explorer` view

- `View.VIEW_TYPE === "file-explorer"` (seen at offset ~1,365,948 as a string literal inside the breadcrumb re-reveal code).
- The view is provided by the `file-explorer` internal plugin — `app.internalPlugins.getEnabledPluginById("file-explorer")` exposes `revealInFolder(folder)` and `openFile(file)` methods used by other views (breadcrumb, backlinks, outline) to cross-navigate.
- Tree rendering uses virtualization for large vaults: `childrenEl` + `visibleChildren[]` swaps with a window of `<div>` items; only collapsed state of visible folders is realized eagerly.
- Per-folder/file state (expanded, selected) is cached on `dom.FILE = file` back-references so the DOM element can be found from the tree pointer and vice versa.

### Vault + Adapter

```js
class Vault extends VT {
  adapter;              // Eu (desktop, Node fs) | CapacitorAdapter (mobile)
  root;                 // ZT for ""
  fileMap;              // path -> $T / ZT
  config;               // parsed app.json / appearance.json etc
  configDir = ".obsidian";
}
```

Adapter surface (desktop `Eu`, ~75 k LoC before this):

| Method | Desktop impl | Mobile impl |
|---|---|---|
| `exists(path)` | `fs.promises.access` / stat | Capacitor FS exists |
| `stat(path)` | `fs.stat` | Capacitor FS stat |
| `read(path)` | `fs.readFile(..., 'utf8')` | readFile via Capacitor |
| `readBinary(path)` | `fs.readFile` → ArrayBuffer | |
| `write(path, data, opts?)` | write-through with atomic rename (tmp → rename) | |
| `writeBinary` / `append` | | |
| `mkdir(path)` | `fs.mkdir({recursive:true})` | |
| `remove(path)` / `rmdir(path, recursive)` | `fs.unlink` / `fs.rm` | |
| `rename(old, new)` | atomic `fs.rename` | |
| `list(path)` | `{ files:[], folders:[] }` | |
| `trashLocal(path)` | Move into `.trash/` inside vault | |
| `trashSystem(path)` | IPC `sendSync('trash', path)` — Electron `shell.trashItem` in main | Capacitor plugin |
| `getResourcePath(path)` | `file://` or `app://` depending on configured protocol | Capacitor Convert.toFileUrl |

### File watcher (desktop)

- `Eu` wraps `chokidar`-like native watcher (actually Node `fs.watch` + debounce). External changes trigger `Vault.trigger("raw", path)` followed by one of `create|modify|delete|rename` once reconciled against `fileMap`.
- Debounce: ~50 ms group window. Hot-reload avoids self-triggering by suppressing events for paths Obsidian itself wrote within the last ~300 ms.

### MetadataCache (`_L`, ~1,570,000)

- Two passes per file:
  1. `computeMetadataAsync(file)` — parses frontmatter + headings + links + embeds + tags via the bundled markdown parser. Result keyed by inode/path.
  2. Link resolver walks links to produce `resolvedLinks[sourcePath][targetPath] = count` and `unresolvedLinks[sourcePath][linkText] = count`.
- Emits `changed` → then `resolve` → then `resolved` once queue drains.
- Persistence: `.obsidian/cache` — binary-ish JSON dump keyed by `${path}#${mtime}#${size}` so restarts only re-parse changed files.
- `getFileCache(file)` returns `null` for non-markdown.
- `getBacklinksForFile(file)` (offset ~1,581,783) walks `resolvedLinks` invertedly.

---

## Editor (CodeMirror) mount

- `MarkdownView` (`W6`, offset ~2,218,000 based on repeated appearances of `W6.VIEW_TYPE`) composes:
  - `W6.editMode` — `MarkdownEditView` (source/live-preview editor)
  - `W6.previewMode` — `MarkdownPreviewRenderer` (read mode)
  - `W6.modeToggleEl` — the eye/edit icon in `view-actions`
- Live-preview vs source: a single `EditorState.facet` on the CM6 view decides whether `cmLivePreview` extension is active. The toggle is via `MarkdownView.setMode("source"|"live"|"preview")`.
- Extensions registered by plugins via `Plugin.registerEditorExtension(extOrExtArray)` are stored in `app.workspace.editorExtensions` and spread into every new `EditorState` construction. A `Facet.reconfigure` is used to hot-reapply on `post-processor-change`.
- The `.cm-scroller` + `.cm-content` classes and styling hook exactly the way upstream CodeMirror 6 does them — Obsidian doesn't fork CM.
- Legacy `CodeMirror` identifier at offset ~363,600 is CM**5**, used only inside `sim.js` sandbox / a few legacy extension shims. CM6 lives as its own webpack modules loaded on demand.

---

## Settings

- `vte` (App.setting, ~3,380,000) extends `tb` Modal. Adds `.mod-sidebar-layout` to `.modal` so the CSS produces the two-pane setting UI (`.vertical-tab-header` list + `.vertical-tab-content`).
- Tabs are kept in `this.tabContentContainer` and `this.settingTabs[]`. `addSettingTab(tab)` pushes + re-renders headings; `setCurrentTab(tab)` swaps content, calls `prev.hide()` then `cur.display()`.
- SettingTab base `Yg`:
  ```js
  class Yg extends Component {
    constructor(app, plugin) { this.app=app; this.plugin=plugin; this.containerEl=createDiv(); }
    display() { /* override; populate containerEl */ }
    hide()    { this.containerEl.empty(); }
  }
  ```
- `PluginSettingTab` and `SettingTab` from the public API are both `Yg` subclasses. Only distinction is the constructor signature.
- Individual setting rows are built with the `Setting` builder class `zk` (see Class taxonomy).

### Settings persistence

- Each configurable subsystem writes to its own file under `.obsidian/`:
  - Core: `app.json`, `appearance.json`, `core-plugins.json`, `community-plugins.json`, `hotkeys.json`, `workspace.json`, `graph.json`, `bookmarks.json`.
- Per-plugin: `app.vault.readPluginData(manifest.dir)` ↔ `.obsidian/plugins/<id>/data.json`.
- `readConfigJson(name)` / `writeConfigJson(name, data)` are the canonical helpers (called by HotkeyManager, Plugins host, etc.). They always pretty-print (`JSON.stringify(v, null, 2)`).

---

## Notices / modals / menus

### `Notice(message, duration=4000)` — class `fb` (~1,089,700)

```js
class Notice {
  constructor(message, duration=4000) {
    const win = activeWindow;
    let container = db.get(win);
    if (!container) { container = createDiv("notice-container"); db.set(win, container); }
    if (!container.isShown()) win.document.body.appendChild(container);
    this.containerEl = container.createDiv("notice");
    this.messageEl = this.containerEl.createDiv({cls:"notice-message", text:message});
    // Slide-in animation via internal dl animation helper
    this.setAutoHide(duration);
    this.containerEl.addEventListener("click", () => this.hide());
  }
  setMessage(m)     {…}
  setAutoHide(ms)   { … if ms>0, setTimeout(hide, ms); }
  hide()            { slide-out then detach }
}
```

- `activeWindow` lets a Notice target the correct popout.
- Only one `.notice-container` per window (WeakMap `db`). If a Notice is queued when one already exists, it stacks inside the same container.
- Mobile anim slides from bottom (`translateY(100%)`); desktop slides from right (`translateX(350px)`).

### Modal (`tb`, ~1,075,000)

DOM: `.modal-container > .modal-bg + .modal (.modal-close-button.mod-raised.clickable-icon, .modal-header > .modal-title, .modal-content)`.

Behavior:
- Owns a private `Scope` that captures Escape and focus-traps the container (`scope.setTabFocusContainerEl(containerEl)`).
- Desktop mac: click-out tolerance of ~5px (drag-friendly).
- Mobile: swipe-down to dismiss on `.modal-title` using the internal `Vm(el, gesture)` helper; opacity tracks drag progress.
- Calls `Cv()` before showing — that ensures any open menu is closed.
- `shouldRestoreSelection=true` by default — saves `document.getSelection()` on open, restores on close.

### Menu

See `Hg` / `Ug` / `zg` in the taxonomy. Shown via `showAtPosition({x,y,width?,overlap?,left?}, doc?)` or `showAtMouseEvent(evt)`. Submenus flip side automatically based on viewport edge (`w = l.offsetWidth; if (!(g+w ≤ d) || (e.left && y-w>=0)) b.left = Math.max(0, y-w)+"px"`).

---

## IPC + window chrome

Renderer → main channels (complete list of unique sends found in `app.js`):

| Channel | Direction | Purpose |
|---|---|---|
| `trash` | `sendSync` | Move path to OS trash |
| `is-dev` | `sendSync` | Whether main was launched with `--dev` |
| `file-url` | `sendSync` | Convert vault path → `file://` or `app://` resource URL |
| `resources` | `sendSync` | Path of resources dir (for bundled assets) |
| `set-menu` / `update-menu-items` / `render-menu` | `send` | Render native macOS menubar from HotkeyManager/Commands state |
| `insider-build` | `sendSync` + `send` | Check/set insider channel |
| `vault` | `sendSync` | Current vault metadata |
| `vault-list` | `sendSync` | Known vaults |
| `vault-open` | `sendSync` | Switch / open a vault (implemented in main) |
| `relaunch` | `sendSync` | Quit + restart (used after install / plugin-toggle if needed) |
| `open-url` | `send` | Open URL in OS default browser |
| `frame` | `sendSync` | Window-frame style setting (`native` / `hidden`) |
| `sandbox` | `sendSync` | Are we running as the sandbox vault |
| `is-quitting` | `sendSync` | Guard so plugins can detect quit context in `unload` |
| `get-sandbox-vault-path` | `sendSync` | Used to redirect sandbox vault path |
| `create-browser-session` | `send` | For `webviewer` internal plugin |
| `adblock-lists` / `adblock-frequency` | `sendSync` | Also webviewer |
| `update` / `check-update` / `disable-update` | `sendSync` + `send` | Auto-update |
| `cli` | `sendSync` | Get pending CLI args |
| `register-cli` | `invoke` | Register CLI handler in main |
| `documents-dir` / `desktop-dir` | `sendSync` | Default paths for vault chooser |
| `set-icon` / `get-icon` | `sendSync` | App icon override |
| `disable-gpu` | `sendSync` | Toggle `--disable-gpu` |
| `version` | `sendSync` | Obsidian version (also on `Yl.version`) |
| `copy-asar` | `sendSync` | Used during updates |
| `starter` | `sendSync` | Switch renderer to starter.html |
| `help` | `sendSync` | Switch renderer to help.html |

Window chrome:
- Desktop-mac frame is always `native` for traffic-lights. The CSS selector `.is-frameless.is-hidden-frameless` is toggled on `<body>` when the user selected "Hidden" window frame; in that mode `app.appMenuBarManager` renders the custom title row.
- `Yl` is the environment flag object: `Yl.isDesktopApp, Yl.isMobile, Yl.isPhone, Yl.isTablet, Yl.isMacOS, Yl.isWindows, Yl.isLinux, Yl.isIosApp, Yl.canSplit, Yl.hasPhysicalKeyboard, Yl.version, Yl.mobileSoftKeyboardVisible`. Lives at a module around offset 3,500k and referenced 400+ times.

---

## Internal patterns

### Minified conventions

- 2-character identifiers for nearly all class names — even the public exports. Name → identity must be recovered from prototype assignments and string literals.
- TypeScript generator runtime: every `async` method compiles to:
  ```
  return y(this, void 0, Promise, function(){
    return b(this, function(n){
      switch(n.label) { case 0: ... }
    });
  });
  ```
  `y` is `__awaiter`, `b` is `__generator`. Presence of `n.label`, `case 0/1/2`, and `[4, somePromise]` (the await marker) identifies async bodies.
- `m(t,e)` is `__extends` (prototype chain helper).
- `k([a], b, !1)` is `__spreadArray` used instead of spread to keep ES5 compat.
- `y/b` + `m` + `k` at the top of every module function signals "this module came from TypeScript."

### DOM helper layer

Obsidian extends the Element/HTMLElement prototype with `createDiv`, `createEl`, `createSpan`, `setChildrenInPlace`, `addClass`, `removeClass`, `toggleClass`, `empty`, `detach`, `isShown`, `on` (delegated events), `off`, `onClickEvent`, `instanceOf`, `matchParent`, `onNodeInserted`. These are installed in the polyfill section near offset 0. Any sibling codebase that wants to reuse Obsidian code must replicate these at minimum.

### Bundler module-loader signature

The webpack runtime at the very start of `app.js`:

```
(()=>{var e={MODULES},t={};
function n(i){...cached require...}
n.d=...; n.r=...; n.m=e; n.o=...; n.f={}; n.e=async chunk loader;
// Then: var <LOCAL>=n(MODID); ... inline module bodies ...
})()
```

Because of `n.d` / `n.r` (defineProperty / makeNamespace), namespace-style imports survive as `n.d(t,{E_:()=>h, F3:()=>l})`. Those short 2-letter keys `E_`, `F3`, `Ii` are the exported names of modules (happy hunting ground for finding public API surfaces).

### String organization / i18n

- All user-facing strings are in `i18n/<locale>.json`. Consumed via a hierarchical accessor:
  ```
  gm.interface.menu.copyPath()        // → "Copy path"
  gm.plugins.fileExplorer.menuOptNewNote()
  gm.interface.startUp.buttonReloadApp()
  ```
- The accessor tree is generated once at module init from the JSON tree; leaves are either strings or functions (`(args)=>format(str,args)`).
- CSS uses plain English class names and the few symbolic ones (`mod-cta`, `is-active`, `mod-left`) are never translated.

---

## What's still opaque

- **Complete tree of internal plugin classes.** I traced to names but not all of their private APIs. The Bases plugin in particular (inside `.obsidian/plugins/bases` or bundled as internal) has a non-trivial view/schema system that `app.internalPlugins.getEnabledPluginById("bases").registerView` exposes but whose internals are densely minified (multi-stage parser + renderer).
- **Graph rendering pipeline.** Deliberately skipped per scope. WebGL / canvas heavy.
- **Canvas whiteboard.** Ditto.
- **Sync / Publish protocols.** Intentionally obfuscated request signing and a non-trivial IndexedDB replication layer; would need a dedicated network-capture pass.
- **Mobile "quick actions" toolbar** (`mobileQuickActions`) — not inspected; mobile-only.
- **`Workspace.editorSuggest` internal state machine.** The suggest registry itself is documented in the public API but how the popover positioning handles IME composition and live-preview decorations is buried in the editor's decoration set.
- **WASM / binary assets.** None found in the asar; the bundle has no `WebAssembly.instantiate` calls. Wasm-ish features (PDF.js viewer, mermaid) are in the `lib/` subdirectory I didn't walk.
- **`initializeWithAdapter` error-recovery paths** are covered by `try/catch` but the exact sequence of retries (especially file-recovery plugin hooks) wasn't fully traced — the handler lives spread across `Tte.prototype.initializeWithAdapter` (~3,672,000) and the `file-recovery` internal plugin.
- **Per-plugin manifest.json fields** beyond `id/name/author/version/minAppVersion/description/isDesktopOnly/dir` — there are a few more observed in dumps (`fundingUrl`, `helpUrl`, `authorUrl`) but the loader doesn't validate them; reading existing plugins is the faster route.
