# Nexus Theming & UI Subsystem — PRD v1.0

**Status:** ✅ Shipped — Complete (see [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md), 2026-04-18) | **Target Release:** April 2026 | **Document Version:** 1.0

---

## Executive Summary

The Theming & UI subsystem delivers a sophisticated, extensible visual platform for Nexus, combining a 400+ CSS variable theming engine with a fully customizable workspace layout system. Core capabilities include live theme switching with platform-native chrome integration, infinitely nestable split-pane layouts, tabbed workspaces, and a plugin-first architecture enabling themes and UI customization without modifying core code.

**Key Components:**
- **CSS Variable Engine:** 400+ typed, hierarchical variables with hot-reload
- **Theme Packages:** TOML-based theme distribution with plugin overrides
- **Workspace Layout Manager:** State-persistent split-pane + tab system
- **Platform Integration:** macOS vibrancy, Windows Mica/Acrylic, Linux CSD/SSD
- **Zustand Store Architecture:** Per-plugin state management with cross-slice subscriptions
- **IPC Layer:** Tauri command → TypeScript binding → React hook pipeline
- **Component Library:** 14 core UI components with design tokens
- **Accessibility:** WCAG 2.1 AA compliance with keyboard navigation and screen reader support

---

## 1. CSS Variable Specification

### 1.1 Variable Taxonomy

All CSS variables follow the naming convention: `--nx-{category}-{property}-{variant}` or `--nx-{category}-{property}`.

**Four-Tier Hierarchy:**

1. **Base Palette** (`--nx-color-*`): 16 semantic colors (primary, secondary, success, warning, error, info, neutral/gray scale)
2. **Component Surfaces** (`--nx-bg-*`): Backgrounds (primary, secondary, tertiary, overlay, elevated)
3. **Semantic Foregrounds** (`--nx-text-*`): Text colors (primary, secondary, tertiary, inverted, muted)
4. **Interactive States** (`--nx-interactive-*`): Hover, active, focus, disabled, loading states
5. **Typography** (`--nx-type-*`): Font families, sizes, weights, line-heights, letter-spacing
6. **Editor & Syntax** (`--nx-editor-*`): Syntax tokens (keyword, string, comment, number), gutter, line-bg
7. **Component Tokens** (`--nx-button-*`, `--nx-input-*`, etc.): Component-specific overrides
8. **Spacing & Layout** (`--nx-space-*`): Gap, padding, margin units (xs=4px, sm=8px, md=16px, lg=32px, xl=64px)
9. **Effects** (`--nx-shadow-*`, `--nx-blur-*`): Shadows, blurs, backdrop filters
10. **Graph & Canvas** (`--nx-graph-*`): Node colors, edge colors, grid, selection

### 1.2 Complete Variable Registry

**Base Palette (16 colors):**
```css
/* Primary brand color */
--nx-color-primary: #4A90E2;
--nx-color-primary-light: #6BA3FF;
--nx-color-primary-dark: #2E5CB8;

/* Full palette: secondary, success, warning, error, info, neutral-50 through neutral-900 */
--nx-color-secondary: #9B59B6;
--nx-color-success: #27AE60;
--nx-color-warning: #F39C12;
--nx-color-error: #E74C3C;
--nx-color-info: #3498DB;
--nx-color-neutral-50: #FAFAFA;
/* ... neutral scale to neutral-900: #0F0F0F ... */
```

**Component Surfaces (Light Mode Example):**
```css
--nx-bg-primary: #FFFFFF;
--nx-bg-secondary: #F8F9FA;
--nx-bg-tertiary: #E8EAEF;
--nx-bg-overlay: rgba(0, 0, 0, 0.5);
--nx-bg-elevated: #FFFFFF;
```

**Text Foregrounds:**
```css
--nx-text-primary: #1A1A1A;
--nx-text-secondary: #4A4A4A;
--nx-text-tertiary: #7A7A7A;
--nx-text-muted: #A0A0A0;
--nx-text-inverted: #FFFFFF;
```

**Interactive States:**
```css
--nx-interactive-hover: rgba(74, 144, 226, 0.08);
--nx-interactive-active: rgba(74, 144, 226, 0.16);
--nx-interactive-focus-ring: 2px solid var(--nx-color-primary);
--nx-interactive-disabled: rgba(0, 0, 0, 0.38);
```

**Typography:**
```css
--nx-type-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
--nx-type-mono: "Monaco", "Courier New", monospace;
--nx-type-serif: "Georgia", "Times New Roman", serif;

--nx-type-h1-size: 32px;
--nx-type-h1-weight: 700;
--nx-type-h1-line-height: 1.2;

--nx-type-body-size: 14px;
--nx-type-body-weight: 400;
--nx-type-body-line-height: 1.5;

--nx-type-code-size: 12px;
--nx-type-code-weight: 400;
--nx-type-code-line-height: 1.4;
```

**Editor & Syntax:**
```css
--nx-editor-bg: var(--nx-bg-primary);
--nx-editor-gutter-bg: var(--nx-bg-secondary);
--nx-editor-line-number: var(--nx-text-tertiary);
--nx-editor-line-highlight: rgba(74, 144, 226, 0.1);
--nx-editor-cursor: var(--nx-text-primary);

--nx-syntax-keyword: #E74C3C;
--nx-syntax-string: #27AE60;
--nx-syntax-comment: #95A5A6;
--nx-syntax-number: #F39C12;
--nx-syntax-function: #3498DB;
--nx-syntax-variable: var(--nx-text-primary);
```

**Spacing Scale:**
```css
--nx-space-xs: 4px;
--nx-space-sm: 8px;
--nx-space-md: 16px;
--nx-space-lg: 32px;
--nx-space-xl: 64px;
```

**Effects:**
```css
--nx-shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.05);
--nx-shadow-md: 0 4px 6px rgba(0, 0, 0, 0.1);
--nx-shadow-lg: 0 10px 15px rgba(0, 0, 0, 0.1);
--nx-blur-sm: blur(4px);
--nx-blur-md: blur(8px);
```

**Graph & Canvas:**
```css
--nx-graph-node-bg: var(--nx-bg-elevated);
--nx-graph-node-border: var(--nx-color-primary);
--nx-graph-edge-stroke: var(--nx-text-tertiary);
--nx-graph-grid: rgba(0, 0, 0, 0.05);
--nx-graph-selection: rgba(74, 144, 226, 0.2);
```

### 1.3 Variable Inheritance & Fallbacks

Variables must cascade sensibly. Example:

```css
/* Button uses component-level override, falls back to semantic color */
--nx-button-primary-bg: var(--nx-color-primary);
--nx-button-primary-hover: var(--nx-color-primary-light);

/* Text color falls back to semantic foreground */
--nx-button-primary-text: var(--nx-text-inverted, #FFFFFF);
```

**Rule:** Every variable must have at least one fallback. Plugin-defined variables inherit from nearest parent scope.

### 1.4 Plugin Variable Extension

Plugins define new variables in their manifest (§2.2). Variables are namespaced:

```toml
[variables]
"--nx-myplugin-accent" = "#FF6B6B"
"--nx-myplugin-surface" = "var(--nx-bg-secondary)"
```

Plugins access core variables using standard CSS inheritance—no explicit registration needed.

---

## 2. Theme Package Format

### 2.1 Directory Structure

```
themes/
├── nexus-light/
│   ├── NEXUS.toml           # Manifest
│   ├── variables.css        # Variable overrides
│   ├── components.css       # Optional: component-specific styles
│   ├── syntax.css           # Optional: editor syntax highlighting
│   └── platform/
│       ├── macos.css        # macOS-specific
│       ├── windows.css      # Windows-specific
│       └── linux.css        # Linux-specific
└── nexus-dark/
    └── [same structure]
```

### 2.2 NEXUS.toml Manifest

```toml
[theme]
name = "Nexus Light"
version = "1.0.0"
author = "Anthropic"
description = "Clean, accessible light theme for focused work"
license = "MIT"
nexus_min_version = "0.1.0"
nexus_max_version = "*"

# Visual metadata
display_name = "Nexus Light"
icon = "base64-encoded-32x32-png-or-url"
category = "light"  # light, dark, sepia, high-contrast, custom

# Feature flags
supports = ["light", "dark"]  # Which modes this theme works in
platform_specific = ["macos", "windows"]  # Platforms with custom styles

# Variable overrides: complete list of CSS variables to override
[variables]
# Colors
"--nx-color-primary" = "#4A90E2"
"--nx-color-primary-light" = "#6BA3FF"
"--nx-color-primary-dark" = "#2E5CB8"
# ... (all 16 base colors)

# Surfaces
"--nx-bg-primary" = "#FFFFFF"
"--nx-bg-secondary" = "#F8F9FA"
# ... (all surface colors)

# Text
"--nx-text-primary" = "#1A1A1A"
# ... (all text colors)

# Typography overrides
[typography]
sans_font = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
mono_font = "'Monaco', 'Courier New', monospace"
serif_font = "'Georgia', 'Times New Roman', serif"

# Font imports: declare @import URLs
font_imports = [
    "https://fonts.googleapis.com/css2?family=Fira+Code:wght@400;600"
]

# Platform-specific variable overrides
[platforms.macos]
"--nx-color-primary" = "#006AFF"  # macOS blue
"--nx-bg-elevated" = "rgba(255, 255, 255, 0.95)"  # Vibrancy-friendly

[platforms.windows]
"--nx-bg-secondary" = "rgba(243, 243, 243, 0.5)"  # Mica-friendly transparency

[platforms.linux]
# CSD-specific adjustments
"--nx-titlebar-height" = "40px"

# Dependencies: list other themes or plugins this theme requires
[dependencies]
# "nexus-icons" = "1.0.0"

# Metadata for discovery
[tags]
keywords = ["light", "minimal", "accessible", "professional"]
color_temperature = "cool"
contrast_level = "aa"  # aa, aaa, low
use_case = ["writing", "coding", "reviewing"]

# Changelog
[version_history]
"1.0.0" = "Initial release"
"0.9.0" = "Beta release"
```

### 2.3 CSS Files Structure

**variables.css:**
```css
:root {
  /* Color palette */
  --nx-color-primary: #4A90E2;
  --nx-color-secondary: #9B59B6;
  /* ... all variables from manifest ... */
  
  /* Derived variables (can compute from base) */
  --nx-color-primary-transparent: rgba(74, 144, 226, 0.1);
}

/* Dark mode overrides via media query */
@media (prefers-color-scheme: dark) {
  :root {
    --nx-color-primary: #6BA3FF;
    --nx-text-primary: #F5F5F5;
    /* ... */
  }
}

/* High contrast mode */
@media (prefers-contrast: more) {
  :root {
    --nx-text-primary: #000000;
    --nx-bg-primary: #FFFFFF;
    --nx-interactive-focus-ring: 3px solid #000000;
  }
}
```

**components.css:**
```css
/* Override specific component styling beyond variables */
.nx-button {
  border-radius: 4px;  /* Theme-specific radius */
  letter-spacing: 0.3px;
}

.nx-modal {
  backdrop-filter: blur(var(--nx-blur-md));
  box-shadow: var(--nx-shadow-lg);
}
```

**syntax.css:**
```css
/* Language-specific syntax highlighting */
.editor .token.keyword {
  color: var(--nx-syntax-keyword);
  font-weight: 600;
}

.editor .token.string {
  color: var(--nx-syntax-string);
}
```

---

## 3. Theme Resolution Engine

### 3.1 Resolution Cascade

Themes are applied in strict order (highest priority last):

1. **Base Theme** (nexus-light or nexus-dark, built-in)
2. **User Theme** (user-selected theme from themes/ directory)
3. **CSS Snippets** (applied in order, individually toggleable)
4. **Plugin Overrides** (plugin-registered theme modifiers)
5. **System Preference** (light/dark mode media query)

**CSS Priority Example:**

```css
/* 1. Base theme: 0,0,1 (element) */
:root { --nx-color-primary: #4A90E2; }

/* 2. User theme: 0,1,0 (class) */
html.theme-solarized { --nx-color-primary: #268BD2; }

/* 3. CSS snippet: 0,1,1 (class + element) */
.snippet-intense-colors { --nx-color-primary: #E74C3C; }

/* 4. Plugin override: 1,0,0 (inline) */
[data-theme-override] { --nx-color-primary: #FF6B6B; }
```

In practice, CSS specificity conflicts are avoided—plugins use class selectors or data attributes, not inline styles.

### 3.2 Theme Switching

**On theme change (e.g., Light → Dark → Custom Theme):**

1. Invalidate component cache
2. Update `<html class="theme-{name}">` and `<html data-theme-mode="{light|dark|system}">`
3. Reload CSS snippets (those tagged for current mode)
4. Trigger Zustand state update: `setTheme({ id, name, mode, variables })`
5. Re-render affected components (via store subscription)
6. Persist selection to `~/.nexus/theme-config.json`

**Latency Target:** < 100ms from theme selection to visual update.

### 3.3 Hot-Reload

When a theme package or CSS snippet file changes (detected via file watcher):

1. Re-parse TOML/CSS
2. Re-compute variable cascade
3. Inject updated `<style>` tags
4. Components re-render via Zustand subscription

**Implementation:** Tauri file watcher → event → TypeScript handler → Zustand dispatch.

### 3.4 System Preference Switching

When OS switches from Light to Dark mode (or vice versa) while app is running:

1. Detect via `window.matchMedia("(prefers-color-scheme: dark)").addEventListener()`
2. If app theme is set to "System", trigger theme resolution for new mode
3. Otherwise, theme remains fixed until user manually switches

---

## 4. CSS Snippet System

### 4.1 File Format & Location

**Location:** `~/.nexus/snippets/` (user-writable)

**File naming:** `{name}.css` (e.g., `neon-accents.css`, `compact-ui.css`)

**File structure:**

```css
/* 
 * Nexus CSS Snippet
 * Name: Neon Accents
 * Description: Bright, high-contrast accent colors
 * Author: user@example.com
 * Version: 1.0
 * Mode: all  // light, dark, or all
 * Scope: global  // global or per-surface
 */

:root {
  --nx-color-primary: #00FF00;
  --nx-color-error: #FF00FF;
}

.nx-button:hover {
  text-shadow: 0 0 8px var(--nx-color-primary);
}
```

### 4.2 Metadata & Parsing

The header comment block is parsed as YAML frontmatter. Required fields: `Name`, `Description`. Optional: `Author`, `Version`, `Mode` (default: `all`), `Scope` (default: `global`).

### 4.3 Toggle Mechanism

Snippets are managed via `useThemeStore`:

```typescript
// Fetch all snippets
const snippets = await getAvailableSnippets();  // → Rust IPC

// Toggle a snippet on/off
toggleSnippet(snippetId);  // → Updates Zustand + persists to config

// Reorder snippets (affects cascade)
reorderSnippets([id1, id2, id3]);
```

**UI:** Settings > Theming > CSS Snippets shows toggleable list with live preview.

### 4.4 Load Order & Scope

- **Load order:** Snippets load in user-specified order (array persisted in config).
- **Scope (global):** Applied to `<html>` for all surfaces.
- **Scope (per-surface):** Only applied to panes matching selector (e.g., `.editor-pane` only).

---

## 5. Workspace Layout Manager

### 5.1 Data Model

**Core Types (TypeScript):**

```typescript
// Unique identifier for a pane or tab
type PaneId = string & { readonly brand: "PaneId" };
type TabId = string & { readonly brand: "TabId" };

// Node in the split tree
interface SplitNode {
  type: "split" | "leaf";
  id: PaneId;
  direction: "row" | "column";  // Only for type="split"
  children: (SplitNode | PaneNode)[];  // Only for type="split"
  sizes: number[];  // Proportional: [0.3, 0.7] means 30%/70% split
}

interface PaneNode {
  type: "leaf";
  id: PaneId;
  activeTabId: TabId;
  tabs: Tab[];
  collapsed?: boolean;
  minSize?: number;  // pixels
}

interface Tab {
  id: TabId;
  label: string;
  icon?: string;  // e.g., "file", "settings", "chat"
  surface: "editor" | "preview" | "terminal" | "sidepanel" | "custom";
  pinned: boolean;
  contentType: string;  // e.g., "file:///path", "settings:theme"
  isDirty: boolean;
}

interface Sidebar {
  side: "left" | "right";
  width: number;  // pixels
  collapsed: boolean;
  miniMode: boolean;  // Icons only
  panels: SidebarPanel[];
  panelOrder: string[];  // Panel IDs in order
}

interface SidebarPanel {
  id: string;
  title: string;
  icon: string;
  plugin?: string;  // Plugin that owns this panel
  visible: boolean;
}

interface BottomPanel {
  height: number;  // pixels
  collapsed: boolean;
  tabs: Tab[];  // Terminal, process manager, diagnostics, etc.
}

interface WorkspaceLayout {
  id: string;
  name: string;
  version: "1.0";
  root: SplitNode;
  leftSidebar: Sidebar;
  rightSidebar: Sidebar;
  bottomPanel: BottomPanel;
  focusedPaneId?: PaneId;
  metadata: {
    createdAt: string;  // ISO 8601
    lastModified: string;
    width: number;
    height: number;
  };
}
```

### 5.2 Serialization Format (workspace.json)

```json
{
  "id": "workspace-1",
  "name": "Coding",
  "version": "1.0",
  "root": {
    "type": "split",
    "id": "pane-root",
    "direction": "row",
    "children": [
      {
        "type": "split",
        "id": "pane-left-split",
        "direction": "column",
        "children": [
          {
            "type": "leaf",
            "id": "pane-editor-top",
            "activeTabId": "tab-main",
            "tabs": [
              {
                "id": "tab-main",
                "label": "main.rs",
                "icon": "file",
                "surface": "editor",
                "pinned": false,
                "contentType": "file:///home/user/project/src/main.rs",
                "isDirty": false
              }
            ]
          },
          {
            "type": "leaf",
            "id": "pane-terminal",
            "activeTabId": "tab-term",
            "tabs": [
              {
                "id": "tab-term",
                "label": "Terminal",
                "icon": "terminal",
                "surface": "terminal",
                "pinned": true,
                "contentType": "terminal://0",
                "isDirty": false
              }
            ]
          }
        ],
        "sizes": [0.6, 0.4]
      },
      {
        "type": "leaf",
        "id": "pane-preview",
        "activeTabId": "tab-preview",
        "tabs": [
          {
            "id": "tab-preview",
            "label": "Docs",
            "icon": "book",
            "surface": "preview",
            "pinned": false,
            "contentType": "doc:///guide.md",
            "isDirty": false
          }
        ]
      }
    ],
    "sizes": [0.7, 0.3]
  },
  "leftSidebar": {
    "side": "left",
    "width": 280,
    "collapsed": false,
    "miniMode": false,
    "panels": [
      {
        "id": "explorer",
        "title": "Explorer",
        "icon": "folder",
        "visible": true
      },
      {
        "id": "search",
        "title": "Search",
        "icon": "search",
        "visible": true
      }
    ],
    "panelOrder": ["explorer", "search"]
  },
  "rightSidebar": {
    "side": "right",
    "width": 0,
    "collapsed": true,
    "miniMode": false,
    "panels": [],
    "panelOrder": []
  },
  "bottomPanel": {
    "height": 200,
    "collapsed": false,
    "tabs": []
  },
  "focusedPaneId": "pane-editor-top",
  "metadata": {
    "createdAt": "2026-04-01T10:00:00Z",
    "lastModified": "2026-04-11T14:30:00Z",
    "width": 1920,
    "height": 1080
  }
}
```

### 5.3 Layout Manipulation API

**Zustand store slice (useLayoutStore):**

```typescript
interface LayoutStore {
  // State
  workspace: WorkspaceLayout;
  savedLayouts: WorkspaceLayout[];
  
  // Actions
  setSplitSize(paneId: PaneId, newSizes: number[]): void;
  splitPane(paneId: PaneId, direction: "horizontal" | "vertical"): PaneId;
  closePane(paneId: PaneId): void;
  addTab(paneId: PaneId, tab: Tab): TabId;
  closeTab(tabId: TabId): void;
  focusPane(paneId: PaneId): void;
  focusTab(tabId: TabId): void;
  collapseSidebar(side: "left" | "right"): void;
  setMiniMode(side: "left" | "right", enabled: boolean): void;
  resizeBottomPanel(height: number): void;
  saveLayout(name: string): Promise<void>;
  loadLayout(layoutId: string): Promise<void>;
  resetLayout(): Promise<void>;
}
```

---

## 6. Split Pane System

### 6.1 Implementation Details

**React Component:**

```typescript
interface SplitPaneProps {
  children: React.ReactNode[];  // Exactly 2 children
  direction: "horizontal" | "vertical";
  sizes: [number, number];  // [0.3, 0.7]
  minSize?: [number, number];  // [200, 300] in pixels
  onResize?: (newSizes: [number, number]) => void;
  onCollapse?: (index: 0 | 1) => void;
}

export const SplitPane: React.FC<SplitPaneProps> = ({
  children,
  direction,
  sizes: [size1, size2],
  minSize = [200, 200],
  onResize,
  onCollapse
}) => {
  const [isDragging, setIsDragging] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  
  // Handle mouse down on divider
  const handleMouseDown = (e: React.MouseEvent) => {
    setIsDragging(true);
  };
  
  // Handle drag
  useEffect(() => {
    if (!isDragging) return;
    
    const handleMouseMove = (e: MouseEvent) => {
      // Calculate new sizes based on cursor position
      // Respect minSize constraints
      // Call onResize with new sizes
    };
    
    const handleMouseUp = () => setIsDragging(false);
    
    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isDragging, size1, size2, direction, minSize, onResize]);
  
  const isHorizontal = direction === "horizontal";
  const dividerSize = 4;  // pixels
  
  return (
    <div
      ref={ref}
      style={{
        display: "flex",
        flexDirection: isHorizontal ? "row" : "column",
        height: "100%",
        width: "100%"
      }}
    >
      <div style={{
        flex: `${size1} 1 0px`,
        minWidth: isHorizontal ? minSize[0] : "auto",
        minHeight: !isHorizontal ? minSize[0] : "auto",
        overflow: "auto"
      }}>
        {children[0]}
      </div>
      
      <div
        onMouseDown={handleMouseDown}
        style={{
          width: isHorizontal ? dividerSize : "100%",
          height: !isHorizontal ? dividerSize : "100%",
          cursor: isHorizontal ? "col-resize" : "row-resize",
          backgroundColor: "var(--nx-bg-tertiary)",
          userSelect: "none"
        }}
      />
      
      <div style={{
        flex: `${size2} 1 0px`,
        minWidth: isHorizontal ? minSize[1] : "auto",
        minHeight: !isHorizontal ? minSize[1] : "auto",
        overflow: "auto"
      }}>
        {children[1]}
      </div>
    </div>
  );
};
```

### 6.2 Nested Rendering

Recursively render SplitNode tree:

```typescript
interface RenderSplitNodeProps {
  node: SplitNode;
  onResize: (paneId: PaneId, newSizes: number[]) => void;
  onFocus: (paneId: PaneId) => void;
}

const RenderSplitNode: React.FC<RenderSplitNodeProps> = ({
  node,
  onResize,
  onFocus
}) => {
  if (node.type === "leaf") {
    return <Pane paneId={node.id} onFocus={onFocus} />;
  }
  
  // type === "split"
  const children = node.children.map((child) => (
    <RenderSplitNode
      key={child.id}
      node={child}
      onResize={onResize}
      onFocus={onFocus}
    />
  ));
  
  return (
    <SplitPane
      direction={node.direction}
      sizes={node.sizes as [number, number]}
      onResize={(newSizes) => onResize(node.id, newSizes)}
    >
      {children}
    </SplitPane>
  );
};
```

### 6.3 Focus Management

- Track `focusedPaneId` in Zustand store
- On pane click, update store
- Pane renders with focused style: `border: 2px solid var(--nx-color-primary)`
- Keyboard shortcuts (Alt+Tab) cycle through panes

### 6.4 Collapse Behavior

Collapsing a pane hides it but preserves its size in the layout. Re-expanding restores it to the same proportions.

---

## 7. Tab System

### 7.1 Tab Component

```typescript
interface TabBarProps {
  tabs: Tab[];
  activeTabId: TabId;
  onSelectTab: (tabId: TabId) => void;
  onCloseTab: (tabId: TabId) => void;
  onReorderTabs?: (newTabs: Tab[]) => void;
}

export const TabBar: React.FC<TabBarProps> = ({
  tabs,
  activeTabId,
  onSelectTab,
  onCloseTab,
  onReorderTabs
}) => {
  const [draggedId, setDraggedId] = useState<TabId | null>(null);
  
  const handleDragStart = (e: React.DragEvent, tabId: TabId) => {
    setDraggedId(tabId);
    e.dataTransfer.effectAllowed = "move";
  };
  
  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
  };
  
  const handleDrop = (e: React.DragEvent, targetId: TabId) => {
    e.preventDefault();
    if (!draggedId || draggedId === targetId) return;
    
    const draggedIdx = tabs.findIndex((t) => t.id === draggedId);
    const targetIdx = tabs.findIndex((t) => t.id === targetId);
    
    const newTabs = [...tabs];
    [newTabs[draggedIdx], newTabs[targetIdx]] = [newTabs[targetIdx], newTabs[draggedIdx]];
    onReorderTabs?.(newTabs);
    setDraggedId(null);
  };
  
  return (
    <div
      style={{
        display: "flex",
        backgroundColor: "var(--nx-bg-secondary)",
        borderBottom: "1px solid var(--nx-bg-tertiary)",
        overflowX: "auto",
        scrollBehavior: "smooth"
      }}
    >
      {tabs.map((tab) => (
        <div
          key={tab.id}
          draggable
          onDragStart={(e) => handleDragStart(e, tab.id)}
          onDragOver={handleDragOver}
          onDrop={(e) => handleDrop(e, tab.id)}
          onClick={() => onSelectTab(tab.id)}
          style={{
            padding: "var(--nx-space-sm) var(--nx-space-md)",
            cursor: "pointer",
            backgroundColor: 
              tab.id === activeTabId 
                ? "var(--nx-bg-primary)"
                : "var(--nx-bg-secondary)",
            borderBottom: 
              tab.id === activeTabId 
                ? "2px solid var(--nx-color-primary)"
                : "none",
            display: "flex",
            alignItems: "center",
            gap: "var(--nx-space-sm)",
            minWidth: "120px",
            maxWidth: "200px",
            whiteSpace: "nowrap",
            borderRight: "1px solid var(--nx-bg-tertiary)"
          }}
        >
          {tab.icon && <Icon name={tab.icon} size={16} />}
          <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>
            {tab.label}
          </span>
          {tab.isDirty && <span style={{ color: "var(--nx-color-warning)" }}>●</span>}
          <button
            onClick={(e) => {
              e.stopPropagation();
              onCloseTab(tab.id);
            }}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              padding: "2px",
              fontSize: "14px"
            }}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
};
```

### 7.2 Tab Actions

- **Drag-to-Reorder:** Within tab bar (implemented above)
- **Drag-to-Split:** Drag tab to edge of pane to create new split
- **Tab Overflow:** When tabs exceed visible width, enable scroll or show dropdown menu listing off-screen tabs
- **Pinned Tabs:** Pinned tabs render first in tab bar, non-closable
- **Tab Groups:** Collapsible groups (e.g., "All Files" group)

### 7.3 Context Menu

Right-click on tab shows:
- Close
- Close Others
- Close All to the Right
- Pin/Unpin
- Duplicate Tab
- Move to New Pane

---

## 8. Sidebar Architecture

### 8.1 Sidebar Components

```typescript
interface SidebarProps {
  side: "left" | "right";
  panels: SidebarPanel[];
  miniMode: boolean;
  collapsed: boolean;
  onToggleMiniMode: (enabled: boolean) => void;
  onTogglePanel: (panelId: string, visible: boolean) => void;
  onReorderPanels?: (newOrder: string[]) => void;
}

export const Sidebar: React.FC<SidebarProps> = ({
  side,
  panels,
  miniMode,
  collapsed,
  onToggleMiniMode,
  onTogglePanel,
  onReorderPanels
}) => {
  if (collapsed) {
    return <CollapsedSidebar side={side} />;
  }
  
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        backgroundColor: "var(--nx-bg-secondary)",
        borderRight: side === "left" ? "1px solid var(--nx-bg-tertiary)" : "none",
        borderLeft: side === "right" ? "1px solid var(--nx-bg-tertiary)" : "none",
        width: miniMode ? "48px" : "280px",
        transition: "width 0.2s ease",
        overflowY: "auto"
      }}
    >
      <div style={{ padding: "var(--nx-space-md)", borderBottom: "1px solid var(--nx-bg-tertiary)" }}>
        <button
          onClick={() => onToggleMiniMode(!miniMode)}
          title={miniMode ? "Expand" : "Collapse"}
          style={{
            background: "none",
            border: "1px solid var(--nx-bg-tertiary)",
            borderRadius: "4px",
            padding: "var(--nx-space-sm)",
            cursor: "pointer"
          }}
        >
          {miniMode ? "»" : "«"}
        </button>
      </div>
      
      {panels.map((panel) => (
        <SidebarPanelTab
          key={panel.id}
          panel={panel}
          miniMode={miniMode}
          visible={panel.visible}
          onToggle={() => onTogglePanel(panel.id, !panel.visible)}
        />
      ))}
    </div>
  );
};

interface SidebarPanelTabProps {
  panel: SidebarPanel;
  miniMode: boolean;
  visible: boolean;
  onToggle: () => void;
}

const SidebarPanelTab: React.FC<SidebarPanelTabProps> = ({
  panel,
  miniMode,
  visible,
  onToggle
}) => (
  <button
    onClick={onToggle}
    style={{
      display: "flex",
      alignItems: "center",
      gap: miniMode ? 0 : "var(--nx-space-sm)",
      padding: "var(--nx-space-md)",
      backgroundColor: visible ? "var(--nx-bg-primary)" : "transparent",
      border: "none",
      cursor: "pointer",
      borderRadius: "4px",
      margin: "var(--nx-space-xs)"
    }}
    title={panel.title}
  >
    <Icon name={panel.icon} size={20} />
    {!miniMode && <span>{panel.title}</span>}
  </button>
);
```

### 8.2 Panel Registration

Plugins register panels via IPC:

```typescript
// Plugin code (e.g., nexus-explorer)
await registerSidebarPanel({
  id: "explorer",
  title: "Explorer",
  icon: "folder",
  side: "left",
  component: ExplorerPanelComponent  // Lazy-loaded
});
```

### 8.3 Responsive Behavior

- At window width < 768px: sidebars auto-collapse to mini mode
- At width < 480px: sidebars completely hidden, swipe gesture to reveal
- Panel order is user-customizable via drag-to-reorder

---

## 9. Zustand Store Architecture

### 9.1 Store Structure

```typescript
// src/stores/theme.ts
interface ThemeState {
  // State
  currentTheme: Theme;
  mode: "light" | "dark" | "system";
  snippets: CSSSnippet[];
  enabledSnippets: string[];  // IDs of active snippets
  variables: Record<string, string>;  // Computed CSS variables
  
  // Actions
  setTheme: (theme: Theme) => Promise<void>;
  setMode: (mode: "light" | "dark" | "system") => Promise<void>;
  toggleSnippet: (snippetId: string) => Promise<void>;
  reloadVariables: () => Promise<void>;
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  currentTheme: defaultTheme,
  mode: "system",
  snippets: [],
  enabledSnippets: [],
  variables: {},
  
  setTheme: async (theme: Theme) => {
    try {
      await applyTheme(theme.id);  // Tauri IPC
      set({ currentTheme: theme });
      set((state) => ({
        variables: { ...state.variables, ...theme.variables }
      }));
    } catch (err) {
      console.error("Failed to apply theme:", err);
    }
  },
  
  setMode: async (mode: "light" | "dark" | "system") => {
    set({ mode });
    await persistConfig({ theme: { mode } });
  },
  
  toggleSnippet: async (snippetId: string) => {
    set((state) => {
      const enabled = state.enabledSnippets.includes(snippetId)
        ? state.enabledSnippets.filter((id) => id !== snippetId)
        : [...state.enabledSnippets, snippetId];
      return { enabledSnippets: enabled };
    });
    await persistConfig({ theme: { enabledSnippets: get().enabledSnippets } });
  },
  
  reloadVariables: async () => {
    const computed = await computeVariables();  // Tauri IPC
    set({ variables: computed });
  }
}));

// src/stores/layout.ts
interface LayoutState {
  workspace: WorkspaceLayout;
  focusedPaneId: PaneId | null;
  isDraggingSplit: boolean;
  
  // Actions
  setSplitSize: (paneId: PaneId, newSizes: number[]) => void;
  splitPane: (paneId: PaneId, direction: "horizontal" | "vertical") => void;
  focusPane: (paneId: PaneId) => void;
  addTab: (paneId: PaneId, tab: Tab) => void;
  closeTab: (tabId: TabId) => void;
  saveLayout: (name: string) => Promise<void>;
  loadLayout: (layoutId: string) => Promise<void>;
  persistLayout: () => Promise<void>;
}

export const useLayoutStore = create<LayoutState>((set, get) => ({
  workspace: defaultLayout,
  focusedPaneId: null,
  isDraggingSplit: false,
  
  setSplitSize: (paneId: PaneId, newSizes: number[]) => {
    set((state) => {
      const updated = updateSplitNode(state.workspace.root, paneId, {
        sizes: newSizes
      });
      return { workspace: { ...state.workspace, root: updated } };
    });
    get().persistLayout();  // Auto-save
  },
  
  // ... other actions
}));

// src/stores/index.ts
export const useStore = () => ({
  theme: useThemeStore(),
  layout: useLayoutStore(),
  editor: useEditorStore(),
  procMgr: useProcMgrStore(),
  // ... other plugins
});
```

### 9.2 Cross-Slice Subscriptions

```typescript
// Subscribe to theme changes and update editor colors
useThemeStore.subscribe(
  (state) => state.variables,
  (variables) => {
    // Dispatch to editor store if needed
    useEditorStore.setState({ syntaxHighlightTokens: computeTokens(variables) });
  }
);

// Auto-persist layout on changes
useLayoutStore.subscribe(
  (state) => state.workspace,
  (workspace) => {
    // Debounced save to disk
    persistWorkspace(workspace);
  }
);
```

### 9.3 Store Persistence

```typescript
// src/lib/persistence.ts
async function persistConfig(partial: Partial<Config>) {
  const current = await loadConfig();  // Read from ~/.nexus/config.json
  const updated = deepMerge(current, partial);
  await writeConfig(updated);  // Tauri IPC
}

async function loadConfig(): Promise<Config> {
  const raw = await readConfig();  // Tauri IPC
  return parseConfig(raw);
}
```

### 9.4 Devtools Integration

```typescript
const useThemeStore = create<ThemeState>(
  persist(
    devtools((set, get) => ({
      // ... store implementation
    }), { name: "ThemeStore" }),
    { name: "theme-storage", storage: asyncStorage }
  )
);
```

Enable Redux DevTools for store inspection in development.

---

## 10. IPC Layer

### 10.1 Tauri Command Pattern

**Rust (src-tauri/src/commands/theme.rs):**

```rust
#[tauri::command]
pub async fn apply_theme(
    theme_id: String,
    app_handle: AppHandle,
) -> Result<AppliedTheme, String> {
    let themes_dir = get_themes_dir()?;
    let theme_path = themes_dir.join(&theme_id);
    
    if !theme_path.exists() {
        return Err(format!("Theme not found: {}", theme_id));
    }
    
    let manifest = load_theme_manifest(&theme_path)?;
    let variables = resolve_variables(&manifest)?;
    
    // Persist to config
    let config = AppConfig::load()?;
    let mut updated = config.clone();
    updated.theme.current = theme_id;
    config.save()?;
    
    Ok(AppliedTheme {
        id: manifest.id,
        name: manifest.name,
        variables,
    })
}

#[tauri::command]
pub async fn compute_variables(
    theme_id: String,
    snippets_enabled: Vec<String>,
) -> Result<VariableMap, String> {
    // Compute cascade: base → theme → snippets
    // Return as JSON
}

#[tauri::command]
pub async fn get_available_themes() -> Result<Vec<ThemeMetadata>, String> {
    // List all .toml manifests in themes/
}
```

**TypeScript Binding Generation (ts-rs):**

The Rust types are auto-exported to `src/bindings/`:

```typescript
// Auto-generated from Rust types
export interface AppliedTheme {
  id: string;
  name: string;
  variables: Record<string, string>;
}

export interface ThemeMetadata {
  id: string;
  name: string;
  author: string;
  description: string;
}
```

### 10.2 TypeScript IPC Wrapper

```typescript
// src/ipc/theme.ts
export async function applyTheme(themeId: string): Promise<AppliedTheme> {
  try {
    const result = await invoke<AppliedTheme>("apply_theme", { 
      theme_id: themeId 
    });
    return result;
  } catch (err) {
    throw new IpcError("Failed to apply theme", err);
  }
}

export async function getAvailableThemes(): Promise<ThemeMetadata[]> {
  return invoke<ThemeMetadata[]>("get_available_themes");
}

export async function computeVariables(
  themeId: string,
  enabledSnippets: string[]
): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("compute_variables", {
    theme_id: themeId,
    snippets_enabled: enabledSnippets
  });
}
```

### 10.3 React Hook Consumption

```typescript
// src/hooks/useTheme.ts
export function useTheme() {
  const { currentTheme, variables, setTheme } = useThemeStore();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  
  const applyTheme = async (themeId: string) => {
    setLoading(true);
    setError(null);
    try {
      const applied = await applyTheme(themeId);  // IPC call
      setTheme(applied);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error");
    } finally {
      setLoading(false);
    }
  };
  
  return {
    currentTheme,
    variables,
    applyTheme,
    loading,
    error
  };
}

// Usage in component
const MyComponent = () => {
  const { currentTheme, applyTheme } = useTheme();
  
  return (
    <div>
      <p>Current: {currentTheme.name}</p>
      <button onClick={() => applyTheme("nexus-dark")}>
        Switch to Dark
      </button>
    </div>
  );
};
```

### 10.4 Streaming Events

For high-throughput data (e.g., editor content, terminal output):

```typescript
// src/ipc/editor.ts
export function subscribeToEditorContent(
  filePath: string,
  onChunk: (chunk: EditorChunk) => void
): () => void {
  const unlisten = listen<EditorChunk>("editor-content-chunk", (event) => {
    onChunk(event.payload);
  });
  
  invoke("subscribe_editor_content", { file_path: filePath });
  
  return unlisten;
}

// Usage
const EditorView = ({ filePath }) => {
  useEffect(() => {
    const unsubscribe = subscribeToEditorContent(filePath, (chunk) => {
      // Update editor with new chunk
    });
    return unsubscribe;
  }, [filePath]);
};
```

### 10.5 Error Propagation

All IPC errors include context:

```typescript
class IpcError extends Error {
  constructor(
    public operation: string,
    public originalError: unknown,
    public context?: Record<string, any>
  ) {
    super(`IPC error in ${operation}: ${String(originalError)}`);
  }
}

// Usage
try {
  await applyTheme(themeId);
} catch (err) {
  if (err instanceof IpcError) {
    console.error(`Operation: ${err.operation}`);
    console.error(`Error: ${err.message}`);
    // Show toast notification to user
  }
}
```

---

## 11. Platform Chrome

### 11.1 macOS Integration

**Features:**
- Native traffic light buttons (red/yellow/green)
- Vibrancy effects (blurred background behind panels)
- Native menus (File, Edit, View, etc.)
- Rounded window corners
- Native titlebar

**Implementation:**

```toml
# Tauri.conf.json
"macos": {
  "titleBarStyle": "transparent",
  "fullscreen": false,
  "hiddenTitle": true,
  "decorations": true
}
```

**CSS:**

```css
/* macOS-specific */
@supports (-webkit-app-region: drag) {
  .titlebar {
    -webkit-app-region: drag;
    height: 28px;
  }
  
  .titlebar button {
    -webkit-app-region: no-drag;
  }
  
  .sidebar {
    background: rgba(255, 255, 255, 0.5);
    backdrop-filter: blur(10px);
  }
}
```

### 11.2 Windows Integration

**Features:**
- Custom titlebar with native buttons
- Mica or Acrylic material (dynamically chosen)
- Snap layouts support
- Native context menus

**Implementation:**

```toml
# Tauri.conf.json
"windows": [{
  "fullscreen": false,
  "decorations": false,
  "transparent": true
}]
```

**CSS:**

```css
/* Windows-specific */
.titlebar {
  background: var(--nx-bg-primary);
  backdrop-filter: blur(20px);
  height: 32px;
  display: flex;
  align-items: center;
}

.titlebar-controls {
  position: absolute;
  right: 0;
  display: flex;
}

/* Mica/Acrylic-friendly transparency */
body {
  background: rgba(245, 245, 245, 0.9);
}
```

### 11.3 Linux Integration

**Features:**
- CSD (Client-Side Decorations) detection
- System tray icon (if supported)
- D-Bus integration for theme follow

**Implementation:**

```rust
#[cfg(target_os = "linux")]
pub fn setup_linux_chrome(app: &mut App) {
    // Detect if running under Wayland or X11
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    
    // Register app in freedesktop for theme following
    if session_type == "wayland" {
        setup_wayland_theme_follow();
    }
}
```

---

## 12. Responsive & Adaptive Design

### 12.1 Breakpoints

```typescript
const BREAKPOINTS = {
  xs: 0,      // Mobile
  sm: 480,    // Small tablet
  md: 768,    // Tablet
  lg: 1024,   // Desktop
  xl: 1440,   // Large desktop
  xxl: 1920   // Ultra-wide
};
```

### 12.2 Panel Auto-Collapse Logic

```typescript
const getResponsiveLayout = (windowWidth: number): LayoutConfig => {
  if (windowWidth < 480) {
    return {
      leftSidebar: { collapsed: true },
      rightSidebar: { collapsed: true },
      bottomPanel: { collapsed: true },
      tabBarMode: "dropdown"  // Instead of scrolling
    };
  } else if (windowWidth < 768) {
    return {
      leftSidebar: { collapsed: false, miniMode: true },
      rightSidebar: { collapsed: true },
      bottomPanel: { collapsed: false },
      tabBarMode: "scroll"
    };
  } else if (windowWidth < 1024) {
    return {
      leftSidebar: { collapsed: false, miniMode: false },
      rightSidebar: { collapsed: true },
      bottomPanel: { collapsed: false },
      tabBarMode: "scroll"
    };
  } else {
    // Full desktop layout
    return {
      leftSidebar: { collapsed: false, miniMode: false },
      rightSidebar: { collapsed: false },
      bottomPanel: { collapsed: false },
      tabBarMode: "scroll"
    };
  }
};
```

### 12.3 Touch Gestures

For mobile/tablet modes:
- Swipe right: toggle left sidebar
- Swipe left: toggle right sidebar
- Swipe down: toggle bottom panel
- Pinch: zoom editor

---

## 13. Component Library

### 13.1 Core Components

**Button:**
```typescript
interface ButtonProps {
  variant?: "primary" | "secondary" | "danger" | "ghost";
  size?: "sm" | "md" | "lg";
  disabled?: boolean;
  loading?: boolean;
  icon?: string;
  children: React.ReactNode;
}

export const Button: React.FC<ButtonProps> = ({
  variant = "primary",
  size = "md",
  disabled,
  loading,
  icon,
  children
}) => {
  const variantStyles = {
    primary: {
      bg: "var(--nx-color-primary)",
      text: "var(--nx-text-inverted)"
    },
    secondary: {
      bg: "var(--nx-bg-tertiary)",
      text: "var(--nx-text-primary)"
    },
    // ...
  };
  
  return (
    <button
      disabled={disabled || loading}
      style={{
        ...variantStyles[variant],
        padding: size === "sm" ? "4px 8px" : size === "md" ? "8px 16px" : "12px 24px",
        borderRadius: "4px",
        border: "none",
        cursor: disabled ? "not-allowed" : "pointer",
        display: "flex",
        alignItems: "center",
        gap: "var(--nx-space-sm)"
      }}
    >
      {loading && <Spinner size={size} />}
      {icon && <Icon name={icon} />}
      {children}
    </button>
  );
};
```

**Additional Components:**
- Input: text, password, email, with validation
- Select: dropdown, multi-select
- Toggle: switch component
- Modal: centered dialog with backdrop
- Tooltip: hover popup with arrow
- ContextMenu: right-click popup
- CommandPalette: fuzzy searchable command list
- Toast: notification popup (bottom-right)
- Popover: floating panel with arrow positioning
- TabBar: (covered in §7)
- SplitPane: (covered in §6)

### 13.2 Design Tokens

```typescript
export const designTokens = {
  spacing: {
    xs: "4px",
    sm: "8px",
    md: "16px",
    lg: "32px",
    xl: "64px"
  },
  radius: {
    sm: "2px",
    md: "4px",
    lg: "8px",
    xl: "12px"
  },
  duration: {
    fast: "100ms",
    normal: "200ms",
    slow: "400ms"
  },
  easing: {
    linear: "linear",
    in: "cubic-bezier(0.4, 0, 1, 1)",
    out: "cubic-bezier(0, 0, 0.2, 1)",
    inOut: "cubic-bezier(0.4, 0, 0.2, 1)"
  }
};
```

---

## 14. Accessibility (WCAG 2.1 AA)

### 14.1 Keyboard Navigation

- Tab through all interactive elements
- Enter/Space to activate buttons
- Arrow keys in select dropdowns and tab bars
- Escape to close modals/popovers
- Alt+Shift+P to open command palette

### 14.2 Focus Management

```typescript
// Always render visible focus indicators
const focusStyles = {
  outline: "2px solid var(--nx-color-primary)",
  outlineOffset: "2px"
};

// Use useEffect to manage focus on modal open
useEffect(() => {
  const firstInput = modalRef.current?.querySelector("input");
  firstInput?.focus();
}, [isOpen]);
```

### 14.3 Screen Reader Support

```typescript
export const Modal: React.FC<ModalProps> = ({ title, isOpen, children }) => (
  <div
    role="dialog"
    aria-modal="true"
    aria-labelledby="modal-title"
    aria-hidden={!isOpen}
  >
    <h2 id="modal-title">{title}</h2>
    {children}
  </div>
);

// Button with loading state
<button aria-busy={loading}>
  {loading ? "Loading..." : "Submit"}
</button>
```

### 14.4 Contrast Ratios

- Normal text: 4.5:1 (AA), 7:1 (AAA)
- Large text (18pt+): 3:1 (AA), 4.5:1 (AAA)
- Interactive elements: 3:1

All color variables are checked against their backgrounds.

### 14.5 Reduced Motion

```css
@media (prefers-reduced-motion: reduce) {
  * {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
```

---

## 15. Animation System

### 15.1 Transition Tokens

```typescript
export const transitions = {
  // Component interactions
  buttonHover: "background-color 200ms cubic-bezier(0.4, 0, 0.2, 1)",
  modalEnter: "opacity 300ms cubic-bezier(0, 0, 0.2, 1), transform 300ms cubic-bezier(0, 0, 0.2, 1)",
  sidebarCollapse: "width 200ms cubic-bezier(0.4, 0, 0.2, 1)",
  tabActive: "border-bottom-color 100ms ease",
  
  // Duration presets
  durations: {
    instant: "0ms",
    faster: "100ms",
    fast: "150ms",
    base: "200ms",
    slow: "300ms",
    slower: "500ms"
  }
};
```

### 15.2 Animated Components

- Button: hover scale (1.02x)
- Modal: fade-in + slide-up
- Tooltip: fade-in
- Sidebar collapse: width transition
- Theme switch: fade transition
- Tab close: slide-out + fade

All use GPU-accelerated transforms (scale, translate, opacity).

### 15.3 Prefers-Reduced-Motion

Already covered in §14.5. All transitions are disabled for users with this preference.

---

## 16. Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Frame budget | 16ms (60fps) | Smooth resize, scroll |
| Initial render | < 2s | Cold start |
| Theme switch latency | < 100ms | UI feels instant |
| Layout resize | < 50ms | Drag-to-resize smooth |
| Tab open | < 200ms | Including content fetch |
| Sidebar toggle | < 150ms | Collapse animation + layout shift |

**Implementation:**
- Memoize expensive components (useMemo, memo)
- Debounce resize handlers (100ms)
- Lazy-load sidebar panels
- Virtual scrolling for large tab bars
- CSS containment for isolated re-renders

---

## 17. Theme Picker UI

**Location:** Settings > Appearance > Theme

```typescript
export const ThemePicker: React.FC = () => {
  const { currentTheme, setTheme } = useThemeStore();
  const [themes, setThemes] = useState<ThemeMetadata[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [previewId, setPreviewId] = useState<string | null>(null);
  
  const filtered = themes.filter(
    (t) => t.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
           t.tags.some((tag) => tag.toLowerCase().includes(searchQuery.toLowerCase()))
  );
  
  return (
    <div>
      <Input
        placeholder="Search themes..."
        value={searchQuery}
        onChange={(e) => setSearchQuery(e.target.value)}
      />
      
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))", gap: "var(--nx-space-md)" }}>
        {filtered.map((theme) => (
          <ThemeCard
            key={theme.id}
            theme={theme}
            isActive={theme.id === currentTheme.id}
            onPreview={() => setPreviewId(theme.id)}
            onApply={() => setTheme(theme)}
          />
        ))}
      </div>
    </div>
  );
};
```

---

## 18. Layout Customization

**Presets:**
- "Writing" mode: Full editor, minimal sidebars
- "Reviewing" mode: Split panes (code + preview), right sidebar for comments
- "Coding" mode: Explorer on left, editor in center, debugger on right, terminal at bottom

Users can save custom layouts via "Save Layout" button:

```typescript
const saveCurrentLayout = async (name: string) => {
  const layout = useLayoutStore.getState().workspace;
  await saveLayout({ ...layout, name });
};
```

---

## 19. Command Palette

**Spec:**

```typescript
interface CommandPaletteItem {
  id: string;
  title: string;
  category?: string;
  icon?: string;
  keybinding?: string;
  action: () => void | Promise<void>;
}

export const CommandPalette: React.FC = () => {
  const [query, setQuery] = useState("");
  const [isOpen, setIsOpen] = useState(false);
  const [results, setResults] = useState<CommandPaletteItem[]>([]);
  
  // Fuzzy search
  useEffect(() => {
    const filtered = searchCommands(query);
    setResults(filtered);
  }, [query]);
  
  // Keybinding: Cmd+K (macOS) or Ctrl+K (Windows/Linux)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setIsOpen(!isOpen);
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isOpen]);
  
  return isOpen ? (
    <Modal isOpen onClose={() => setIsOpen(false)}>
      <Input
        autoFocus
        placeholder="Command..."
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      <ul>
        {results.map((item) => (
          <li key={item.id} onClick={() => {
            item.action();
            setIsOpen(false);
          }}>
            {item.icon && <Icon name={item.icon} />}
            <span>{item.title}</span>
            {item.category && <span style={{ fontSize: "12px", color: "var(--nx-text-tertiary)" }}>{item.category}</span>}
            {item.keybinding && <span style={{ fontSize: "12px" }}>{item.keybinding}</span>}
          </li>
        ))}
      </ul>
    </Modal>
  ) : null;
};
```

---

## 20. Settings UI

**Architecture:**

```typescript
interface SettingsCategory {
  id: string;
  title: string;
  icon: string;
  settings: Setting[];
  pluginId?: string;
}

interface Setting {
  id: string;
  label: string;
  description?: string;
  type: "toggle" | "select" | "text" | "number" | "slider" | "color";
  value: any;
  options?: Array<{ label: string; value: any }>;
  onChange: (value: any) => void;
  preview?: boolean;  // Show live preview
}

export const SettingsPanel: React.FC = () => {
  const [activeCategory, setActiveCategory] = useState("appearance");
  const [searchQuery, setSearchQuery] = useState("");
  
  const categories = getSettingsCategories();  // From plugins + core
  const filtered = filterSettings(categories, searchQuery);
  
  return (
    <div style={{ display: "flex", height: "100%" }}>
      <nav style={{ width: "200px", backgroundColor: "var(--nx-bg-secondary)" }}>
        {categories.map((cat) => (
          <button
            key={cat.id}
            onClick={() => setActiveCategory(cat.id)}
            style={{
              width: "100%",
              textAlign: "left",
              padding: "var(--nx-space-md)",
              backgroundColor: activeCategory === cat.id ? "var(--nx-bg-primary)" : "transparent",
              border: "none",
              cursor: "pointer"
            }}
          >
            {cat.icon && <Icon name={cat.icon} />}
            {cat.title}
          </button>
        ))}
      </nav>
      
      <div style={{ flex: 1, padding: "var(--nx-space-lg)", overflowY: "auto" }}>
        <Input
          placeholder="Search settings..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
        />
        
        {filtered[activeCategory]?.settings.map((setting) => (
          <SettingRow key={setting.id} setting={setting} />
        ))}
      </div>
    </div>
  );
};
```

---

## Acceptance Criteria

- [ ] All 400+ CSS variables defined in §1.2
- [ ] Theme package format (TOML + CSS) fully documented with example
- [ ] Theme resolution cascade implemented and tested
- [ ] CSS snippet system with toggle UI functional
- [ ] Workspace layout data model serializes/deserializes correctly
- [ ] Split pane system supports nested splits and drag-to-resize
- [ ] Tab system supports drag-to-reorder, drag-to-split, context menu
- [ ] Sidebar panels register via plugin API
- [ ] Zustand stores persist and restore correctly
- [ ] Tauri IPC commands work end-to-end (Rust → TypeScript → React)
- [ ] All 14 core components implemented and styled
- [ ] WCAG 2.1 AA compliance verified (audit in §14)
- [ ] Theme switch latency < 100ms measured
- [ ] Platform chrome (macOS vibrancy, Windows Mica) working
- [ ] Responsive breakpoints working (test at 480px, 768px, 1024px)
- [ ] Command palette with fuzzy search functional
- [ ] Settings panel discoverable and searchable

## Dependencies

- **Runtime:** React 18+, Zustand 4+, Tauri 2.x
- **Build:** TypeScript 5+, ts-rs (for Rust→TS bindings)
- **Styling:** CSS custom properties (no CSS-in-JS required)
- **Accessibility:** axe-core for WCAG audit

## Timeline

- **Week 1:** CSS variable spec + theme package format finalized
- **Week 2:** Theme resolution engine + hot-reload
- **Week 3:** Split pane + tab systems
- **Week 4:** Sidebar architecture + responsive layout
- **Week 5:** Zustand stores + IPC layer
- **Week 6:** Component library (14 components)
- **Week 7:** Platform chrome + accessibility audit
- **Week 8:** Performance testing + polish

**Target Release:** April 30, 2026

---

## Revision History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026-04-11 | Initial comprehensive spec |
