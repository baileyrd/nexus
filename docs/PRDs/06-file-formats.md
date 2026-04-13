# PRD: File Formats Specification for Nexus

**Version:** 1.0  
**Date:** April 2026  
**Status:** Implementation-Ready  
**Owner:** Nexus Core Team  
**Target Size:** 550 lines

---

## 1. Executive Summary

Nexus is a Rust-based, AI-native developer knowledge environment that uses open, human-readable file formats as the foundation for extensibility, portability, and AI interoperability. This PRD fully specifies the five core file formats (Markdown, MDX, Canvas, Bases, and Forge Configuration), plus supporting specifications for attachment handling, versioning, and migration.

The format choices prioritize:
- **Human readability:** Plain text first, JSON structures when nesting required
- **AI processability:** Unambiguous syntax, semantic clarity, standard tooling
- **Backward compatibility:** Graceful degradation, version headers, unknown-field tolerance
- **Ecosystem compatibility:** Based on CommonMark, Obsidian Canvas, industry standards

---

## 2. Markdown Format (.md)

### 2.1 Syntax Specification

**Base:** CommonMark 0.30 (https://spec.commonmark.org/)  
**Extensions:** GitHub Flavored Markdown (GFM) tables, strikethrough, autolinks, task lists

**Nexus-Specific Extensions:**

| Syntax | Meaning | Example |
|--------|---------|---------|
| `[[link]]` | Wikilink to file or block | `[[Agents]]` → "Agents.md" |
| `[[target\|display]]` | Wikilink with alias | `[[Agents\|AI Assistants]]` |
| `![[embed]]` | Embed file content inline | `![[shared-template.md]]` |
| `^block-id` | Block reference anchor | `## Heading ^intro` |
| `[[file#^id]]` | Cross-file block link | `[[README#^intro]]` |
| `#tag` | Inline tag (metadata, not heading) | `Processing #urgent #design` |
| `> [!NOTE]` | Callout/admonition | `> [!WARNING] Critical issue` |
| `$math` | Inline math (LaTeX) | `$E = mc^2$` |
| `$$math$$` | Block math | `$$\int_0^\infty e^{-x}dx = 1$$` |
| `[^1]` | Footnote reference | Text`[^1]` + `[^1]: Note` |

### 2.2 YAML Frontmatter

All `.md` files MAY have optional YAML frontmatter (delimited by `---`) at the start:

```yaml
---
title: "Document Title"
aliases: ["Alt Name", "Synonym"]
tags: [nexus, ai, database]
cssclass: "dark-mode"
type: "guide"
status: "draft"
date: 2026-04-11
created: 2026-03-01
modified: 2026-04-10
---

# Markdown content starts here
```

**Reserved Keys:**
- `title` (string): Display name
- `aliases` (array): Alternative names for wikilinks
- `tags` (array): Content tags
- `type` (string): Document type (guide, reference, spec, etc.)
- `status` (string): Workflow state (draft, published, archived)
- `cssclass` (string): Custom CSS class for rendering
- `date` (YYYY-MM-DD): Publication/reference date
- `created` (YYYY-MM-DD): File creation date
- `modified` (YYYY-MM-DD): Last modification date

**Custom Properties:** Plugins MAY register additional frontmatter keys via the plugin manifest. Unknown keys are preserved during save/load.

**Type Coercion:**
- Scalars are treated as strings unless parseable as numbers/booleans
- Lists with numbers `[1, 2, 3]` are array integers
- Nested objects allowed (e.g., `author: {name: "Bob", email: "bob@example.com"}`)

### 2.3 Parsing Rules

- **Wikilink resolution:** Links are resolved against the vault file tree, supporting relative paths (`[[./sibling]]`, `[[../parent/file]]`)
- **Missing links:** If target not found, render as broken link (styled differently, but not hidden)
- **Embed depth:** Prevent circular embeds via a global embed set during parse. Limit depth to 10 levels.
- **Tag extraction:** `#tag` tokens create a `tags` set; used for sidebar filtering and global search
- **Block references:** `^id` anchors create internal references; used for block-level backlinks

### 2.4 CommonMark Conformance

Nexus markdown is fully compliant with CommonMark 0.30 with these modifications:
- GFM autolink extension enabled (bare URLs recognized)
- GFM table extension enabled
- GFM strikethrough enabled
- GFM task list enabled

---

## 3. MDX Format (.mdx)

### 3.1 Syntax Overview

MDX is Markdown + JSX. Nexus MDX allows embedding interactive components within markdown:

```mdx
# Interactive Guide

<ComponentName prop="value" count={42}>
  **Markdown content is allowed here**
</ComponentName>

Regular markdown below.
```

### 3.2 Component Resolution

**Built-in Components:** Nexus provides core components (Callout, Alert, Tabs, CodeBlock, Diagram)

**Plugin Components:** Plugins register components via `manifest.json`:
```json
{
  "components": {
    "MyWidget": "./components/MyWidget.jsx"
  }
}
```

**Resolution Order:**
1. Built-in Nexus components
2. Plugin-provided components (by registration order)
3. Unknown components render as error placeholder

### 3.3 Import Syntax

```mdx
import { myData } from "./data.json"
export const metadata = { layout: "center" }

# Using imported data
<Table data={myData} />
```

**Rules:**
- `import` statements must appear before markdown content
- `export` statements define page-level metadata
- Relative paths resolved from MDX file location

### 3.4 Component Registry

Plugins define components in their manifest:

```json
{
  "name": "chart-plugin",
  "components": {
    "BarChart": "./dist/components/BarChart.jsx",
    "LineGraph": "./dist/components/LineGraph.jsx"
  }
}
```

Nexus validates JSX and provides props schema for IDE hints.

### 3.5 Sandboxing

- Components run in a sandboxed iframe when unsafe (external scripts)
- Component errors caught and logged; page continues rendering
- Network requests validated against vault security policy
- No direct filesystem access from components

### 3.6 Export to Standard Markdown

Algorithm to export MDX to `.md`:

```
1. Remove import/export statements
2. Replace component tags with markdown equivalents:
   - <Alert type="warning"> → > [!WARNING]
   - <CodeBlock lang="js"> → ```js
   - <Tabs> → render first tab with heading
3. Keep child markdown content
4. Write resulting markdown to .md file
```

---

## 4. Canvas Format (.canvas)

### 4.1 JSON Schema (v1.0)

```json
{
  "version": "1.0",
  "metadata": {
    "created": "2026-04-11T10:30:00Z",
    "modified": "2026-04-11T12:45:00Z",
    "name": "Project Overview"
  },
  "nodes": [
    {
      "id": "node-1",
      "type": "file",
      "file": "Design.md",
      "x": 0,
      "y": 0,
      "width": 250,
      "height": 300
    }
  ],
  "edges": [
    {
      "id": "edge-1",
      "from": "node-1",
      "to": "node-2",
      "label": "depends on",
      "color": "#FF6B6B"
    }
  ]
}
```

### 4.2 Node Types

| Type | Properties | Description |
|------|-----------|-------------|
| `file` | `file: string` | Embed file content (links to .md, .mdx, .canvas) |
| `text` | `text: string` | Free-form text card |
| `link` | `url: string`, `title?: string` | External link card |
| `group` | `label: string` | Container for organizing nodes |
| `database` | `source: string` | Reference to .bases file |
| `terminal` | `command?: string` | Code execution block |

**Common Properties:**
```json
{
  "id": "unique-string",
  "type": "file|text|link|group|database|terminal",
  "x": 100,
  "y": 200,
  "width": 300,
  "height": 400,
  "color": "#FFFFFF",
  "label": "Optional display label",
  "collapsed": false
}
```

### 4.3 Edge Types

```json
{
  "id": "edge-1",
  "from": "node-1",
  "to": "node-2",
  "label": "relationship type",
  "type": "solid|dashed|dotted",
  "color": "#000000"
}
```

### 4.4 Backward/Forward Compatibility

- **Version field:** Always present; current version is `"1.0"`
- **Backward compatibility:** Unknown node/edge fields ignored
- **Forward compatibility:** Gracefully degrade if newer fields present; warn user
- **Migration path:** Include migration tool if version bumped

---

## 5. Bases Format (.bases)

### 5.1 Format Decision: JSON + TOML Hybrid

Use JSON for schema and relations (complex nesting), TOML for configuration and views (human-readable).

**File Structure:**
```
MyDatabase.bases/
├── schema.json         # Field definitions
├── records.json        # Data records
├── views.toml          # View definitions
├── relations.toml      # Relation definitions
└── metadata.json       # Created, modified, version
```

### 5.2 Schema Definition (schema.json)

```json
{
  "version": "1.0",
  "fields": {
    "id": {
      "type": "uuid",
      "primary": true
    },
    "title": {
      "type": "text",
      "required": true
    },
    "status": {
      "type": "select",
      "options": ["todo", "in-progress", "done"]
    },
    "priority": {
      "type": "number",
      "min": 1,
      "max": 5
    },
    "tags": {
      "type": "multi-select",
      "options": ["bug", "feature", "docs"]
    },
    "dueDate": {
      "type": "date"
    },
    "assignee": {
      "type": "relation",
      "target": "Users.bases",
      "targetField": "id"
    }
  }
}
```

**Supported Field Types:**
- `text`, `long-text`, `number`, `currency`, `percent`, `checkbox`, `date`, `time`, `datetime`
- `select`, `multi-select`, `relation`, `formula`, `rollup`, `lookup`

### 5.3 Records Storage (records.json)

```json
[
  {
    "id": "uuid-1",
    "title": "Setup authentication",
    "status": "in-progress",
    "priority": 4,
    "tags": ["feature", "security"],
    "dueDate": "2026-04-20",
    "assignee": "user-42"
  },
  {
    "id": "uuid-2",
    "title": "Fix login bug",
    "status": "done",
    "priority": 5,
    "tags": ["bug"],
    "dueDate": "2026-04-11",
    "assignee": "user-7"
  }
]
```

### 5.4 Views Definition (views.toml)

```toml
[views.table-all]
name = "All Tasks"
type = "table"
fields = ["title", "status", "priority", "assignee"]
sort = [{ field = "priority", direction = "desc" }]
filter = []

[views.kanban-by-status]
name = "By Status"
type = "kanban"
groupField = "status"
fields = ["title", "priority"]

[views.calendar-due]
name = "Due Dates"
type = "calendar"
dateField = "dueDate"
```

### 5.5 Relations & Formulas (relations.toml)

```toml
[relations.task-assignee]
type = "many-to-one"
sourceField = "assignee"
targetBase = "Users.bases"
targetField = "id"

[formulas.urgency]
expression = "IF(priority >= 4 AND dueDate < TODAY(), 'URGENT', 'normal')"
resultType = "text"
```

### 5.6 Example: Project Tracker Database

```json
{
  "version": "1.0",
  "fields": {
    "id": { "type": "uuid", "primary": true },
    "name": { "type": "text", "required": true },
    "description": { "type": "long-text" },
    "status": {
      "type": "select",
      "options": ["planning", "active", "on-hold", "completed"]
    },
    "owner": { "type": "relation", "target": "Team.bases" },
    "tasks": { "type": "relation", "target": "Tasks.bases" },
    "startDate": { "type": "date" },
    "endDate": { "type": "date" },
    "budget": { "type": "currency" },
    "progress": { "type": "percent" }
  }
}
```

---

## 6. Forge Configuration Formats

### 6.1 app.toml

Application settings and plugin configuration:

```toml
[core]
name = "MyForge"
version = "1.0.0"
description = "Project knowledge base"
defaultLayout = "sidebar"
theme = "auto"
language = "en"

[editor]
fontSize = 14
fontFamily = "MonoLisa"
lineHeight = 1.6
enableVimMode = true
autoSave = true
autoSaveDelayMs = 3000

[preview]
enableMermaid = true
enableKatex = true
enableHighlight = true
enableWikilinks = true

[search]
enableFullText = true
indexIntervalMs = 5000
maxResults = 50

[plugins]
enabled = ["obsidian-sync", "ai-assistant"]

[[plugins.config]]
id = "ai-assistant"
apiKey = "${NEXUS_AI_KEY}"
modelId = "claude-opus"
```

**Types & Defaults:**
- All boolean fields default to `true`
- All numeric fields have defaults and min/max constraints
- String arrays default to empty
- Table keys must be documented in format spec

### 6.2 workspace.json

Layout and UI state (restored on app startup):

```json
{
  "version": "1.0",
  "activeFile": "README.md",
  "openFiles": [
    {
      "file": "README.md",
      "line": 42,
      "column": 0,
      "cursorPosition": 1024
    },
    {
      "file": "Agents.md",
      "line": 1,
      "column": 0
    }
  ],
  "sidebarCollapsed": false,
  "panelLayout": {
    "left": { "width": 250, "collapsed": false },
    "right": { "width": 300, "collapsed": true }
  },
  "recentFiles": ["README.md", "Agents.md", "Design.md"],
  "searchQuery": "",
  "theme": "dark"
}
```

### 6.3 mcp.toml

Model Context Protocol server configuration:

```toml
[mcp.local-database]
type = "local"
command = "nexus-mcp-database"
args = ["--vault-path", "${VAULT_PATH}"]
env = { "RUST_LOG" = "info" }

[mcp.anthropic-gateway]
type = "stdio"
command = "/usr/local/bin/claude-mcp"
timeout = 30000

[mcp.custom-plugin]
type = "http"
url = "http://localhost:8000/mcp"
apiKey = "${MCP_KEY}"
```

### 6.4 ai.toml

AI provider and model configuration:

```toml
[providers.default]
type = "anthropic"
apiKey = "${ANTHROPIC_API_KEY}"
baseUrl = "https://api.anthropic.com"

[[models]]
id = "claude-opus"
provider = "default"
maxTokens = 4096
temperature = 0.7
systemPrompt = "You are a helpful code assistant."

[[models]]
id = "claude-haiku"
provider = "default"
maxTokens = 1024
temperature = 0.5
```

---

## 7. Frontmatter Plugin Registration

Plugins register custom frontmatter keys via manifest:

```json
{
  "name": "custom-metadata-plugin",
  "frontmatterProperties": {
    "customField": {
      "type": "string",
      "description": "Custom field for this plugin",
      "default": ""
    },
    "complexData": {
      "type": "object",
      "properties": {
        "key1": { "type": "string" },
        "key2": { "type": "number" }
      }
    }
  }
}
```

Registered properties are:
- Validated on load
- Preserved on save
- Indexed for search
- Available in metadata queries

---

## 8. File Naming Conventions

### 8.1 Valid Characters

**Allowed:** `A-Z`, `a-z`, `0-9`, `-`, `_`, `.` (in extension only)  
**Forbidden:** `/ \ : * ? " < > |` (reserved by filesystems)  
**Not Recommended:** Spaces (convert to `-` in slugs)

### 8.2 Slug Generation Algorithm

```
1. Convert to lowercase
2. Replace spaces with hyphen
3. Remove non-alphanumeric + hyphen + underscore
4. Collapse multiple hyphens to single
5. Trim leading/trailing hyphens

Example: "My Great Document!" → "my-great-document"
```

### 8.3 Case Sensitivity

- **Linux/Mac:** Case-sensitive (important for imports)
- **Windows:** Case-insensitive (file operations) but preserve case
- **Cross-platform recommendation:** Use lowercase slugs, avoid duplicates differing only in case

### 8.4 Path Length Limits

- **Max file path:** 260 characters (Windows compatibility)
- **Max filename:** 255 characters (POSIX standard)
- **Recommendation:** Keep under 50 characters for usability

### 8.5 Reserved Names

**Cannot be used as filenames:**
- `.` (current dir), `..` (parent dir)
- `CON`, `PRN`, `AUX`, `NUL` (Windows reserved)
- Files matching `[Ll]ock*`, `*[Tt]emp*` treated as system files (hidden from UI)

---

## 9. File Versioning & Migration

### 9.1 Version Headers

All text formats include version field at top level:

```markdown
<!-- Format version 1.0 -->
# Document
```

```json
{ "version": "1.0", ... }
```

```toml
version = "1.0"
```

### 9.2 Backward Compatibility Guarantees

- **v1.0 → v1.x:** Always safe; new fields ignored
- **v1.x → v2.0:** Breaking; migration required
- **Unknown fields:** Preserved and passed through unchanged

### 9.3 Migration Tool

Nexus includes CLI tool for version migration:

```bash
nexus migrate --from 1.0 --to 2.0 --vault ./my-vault
```

Migrations are idempotent and create backups.

---

## 10. Import/Export Formats

### 10.1 Obsidian Vault Import

Import entire Obsidian vault:

```bash
nexus import-obsidian --source ~/Documents/ObsidianVault --dest ./nexus-vault
```

Conversion mapping:
- `.md` files → Nexus `.md` (wikilink syntax compatible)
- `.canvas` files → Nexus `.canvas` (Obsidian-compatible format)
- Attachments → Preserved in `attachments/` folder
- Frontmatter → Converted to Nexus YAML format

### 10.2 Notion Export Import

Import Notion markdown exports:

```bash
nexus import-notion --source ~/Downloads/NotionExport.zip --dest ./nexus-vault
```

Conversion mapping:
- Notion pages → `.md` files
- Nested pages → Directory hierarchy
- Databases → `.bases` files (schema inferred)
- Attachments → Extracted and linked

### 10.3 Export to Standard Markdown

Export entire vault to plain markdown:

```bash
nexus export-markdown --source ./nexus-vault --dest ./markdown-export
```

- `.md` → `.md` (unchanged)
- `.mdx` → `.md` (components removed, see section 3.6)
- `.canvas` → `.md` (canvas as text description)
- `.bases` → `.md` or `.csv` (tabular format)

### 10.4 Export to HTML

```bash
nexus export-html --source ./nexus-vault --dest ./html-export --theme dark
```

Features:
- Full navigation sidebar
- Search index (client-side)
- Syntax highlighting
- Responsive design

### 10.5 Export to PDF

```bash
nexus export-pdf --source ./nexus-vault --dest ./pdf-export
```

- Creates one PDF per markdown file
- Table of contents generated
- Cross-references converted to page numbers

---

## 11. Binary Attachment Handling

### 11.1 Storage

Attachments stored in vault root `attachments/` directory:

```
vault/
├── attachments/
│   ├── images/
│   │   ├── screenshot-2026-04-11-001.png
│   │   └── diagram-architecture.svg
│   ├── pdfs/
│   │   └── research-paper.pdf
│   └── videos/
│       └── tutorial.mp4
├── README.md
└── Design.md
```

### 11.2 Reference Syntax

From markdown:
```markdown
![alt text](attachments/images/screenshot.png)
[Download PDF](attachments/pdfs/paper.pdf)
```

### 11.3 Naming Convention

Auto-generated filenames:
```
{type}-{timestamp}-{hash}.{ext}

Examples:
- image-2026-04-11-12-30-45-a3f9.png
- video-2026-04-10-18-22-10-c2d4.mp4
```

User-provided names are preserved if valid.

### 11.4 Deduplication

Nexus uses content hash (SHA-256) to deduplicate:
- Calculate hash of uploaded file
- Check if hash exists in vault
- If yes, reuse existing; if no, store with hash suffix

Metadata file tracks all attachment references:

```json
{
  "attachments": {
    "screenshot-2026-04-11-001.png": {
      "hash": "abc123def456",
      "size": 125000,
      "created": "2026-04-11T10:30:00Z",
      "referencedIn": ["README.md", "Design.md"]
    }
  }
}
```

### 11.5 Supported Types

| Category | Types | Max Size |
|----------|-------|----------|
| Images | PNG, JPG, WEBP, SVG, GIF | 50 MB |
| Documents | PDF, DOCX, XLSX | 100 MB |
| Videos | MP4, WebM, OGG | 500 MB |
| Code | TXT, JSON, YAML, code files | 10 MB |
| Audio | MP3, WAV, OGG | 100 MB |

---

## 12. Format Versioning Strategy

### 12.1 Semantic Versioning

Formats follow semver: `MAJOR.MINOR.PATCH`

- **MAJOR (incompatible):** New required fields, removed fields, syntax changes
- **MINOR (backward-compatible):** New optional fields, new features
- **PATCH (bug fixes):** Parser fixes, clarifications

### 12.2 Version Declaration

Version declared in file header:
```
<!-- version: 1.0.0 -->
```

### 12.3 Compatibility Matrix

| Current | Can Read v1.0 | Can Read v2.0 | Can Read v3.0 |
|---------|---------------|---------------|---------------|
| v1.x    | Yes           | No (error)    | No (error)    |
| v2.x    | Yes           | Yes           | No (error)    |
| v3.x    | Yes           | Yes           | Yes           |

---

## 13. Parser Architecture

### 13.1 Format → Crate Mapping

| Format | Crate | Strategy |
|--------|-------|----------|
| Markdown | `comrak` | DOM parsing (CommonMark) |
| MDX | `swc` (JSX) + `comrak` | Separate JSX/markdown streams |
| Canvas | `serde_json` | Validate schema on load |
| Bases | `serde_json` + `toml` | Separate parsers per file |
| TOML | `toml` crate | Direct parse-to-struct |

### 13.2 Streaming vs DOM

- **Streaming:** Canvas, Bases (large files, line-by-line not needed)
- **DOM:** Markdown, MDX (require cross-document references)

### 13.3 Error Recovery

```rust
// Malformed markdown: recover by treating as code block
// Missing file link: render as broken reference (not error)
// Invalid TOML: log error, use defaults
// Bad schema: validate and reject with clear message
```

### 13.4 Partial Parsing

Nexus supports parsing file headers only (for frontmatter):

```rust
parse_frontmatter(file_path) → Result<HashMap<String, Value>>
```

Useful for sidebar filters, search indexing without full parse.

---

## 14. Performance Targets

### 14.1 Parse Times (Cold Load)

| File Size | Target Time | Notes |
|-----------|------------|-------|
| 1 KB      | < 1 ms     | Small note |
| 50 KB     | < 10 ms    | Typical document |
| 500 KB    | < 100 ms   | Large doc or database |
| 5 MB      | < 1 s      | Max recommended file |

### 14.2 Memory Usage

- Small file: < 5 MB resident
- Medium file: < 50 MB resident
- Large file: < 200 MB resident
- Streaming for files > 1 MB

### 14.3 Index Speed

Full vault reindex:
- 100 files (5 MB total): < 500 ms
- 1000 files (50 MB total): < 5 s
- 10000 files (500 MB total): < 60 s

---

## 15. Format Compatibility Messaging

### 15.1 Version Mismatch Display

When opening file from newer Nexus:

```
⚠️ This file was created in Nexus v2.0.
Some features may not display correctly.

Version compatibility:
- ✓ Basic markdown
- ⚠️ Custom callouts (requires update)
- ✗ AI annotations (requires Nexus v2.1+)

[Update Nexus] [Continue Anyway] [Learn More]
```

### 15.2 Feature Degradation

- Missing component → Show placeholder with name
- Unknown field in schema → Preserve, don't validate
- Newer callout syntax → Fall back to blockquote
- Unsupported prop → Omit from render, log warning

### 15.3 Migration Prompts

On first save with older format:

```
Upgrade file format?

This file uses Nexus Markdown v1.0.
Upgrading to v1.1 adds:
- Better math support
- Improved link syntax
- Performance improvements

[Upgrade] [Keep Current]
```

---

## 16. File Type Associations

### 16.1 Registration (Linux/Mac)

Nexus registers as default handler for:
- `.md` → Nexus (Markdown Editor)
- `.mdx` → Nexus (MDX Editor)
- `.canvas` → Nexus (Canvas Editor)
- `.bases` → Nexus (Database Viewer)

On Linux:
```
/usr/share/applications/nexus.desktop
```

### 16.2 Windows Registration

```
HKEY_CLASSES_ROOT\.md = nexusfile
HKEY_CLASSES_ROOT\nexusfile\shell\open\command = "nexus.exe" "%1"
```

### 16.3 Drag & Drop

- Dragging `.md` file → Open in editor
- Dragging `.png` file → Create image reference
- Dragging `.bases` file → Create database node in canvas

---

## 17. Acceptance Criteria

- [x] All five format specifications defined and complete
- [x] Schema examples for Markdown, MDX, Canvas, Bases provided
- [x] Forge configuration format fully detailed (app.toml, workspace.json, mcp.toml, ai.toml)
- [x] YAML frontmatter schema with reserved keys documented
- [x] File naming conventions, path limits, slug algorithm specified
- [x] Format versioning strategy and migration tool designed
- [x] Import/export algorithms defined (Obsidian, Notion, HTML, PDF, markdown)
- [x] Binary attachment handling with deduplication algorithm
- [x] Parser architecture with crate selection and error recovery
- [x] Performance targets (parse times, memory, index speed) defined
- [x] Format compatibility messaging UX detailed
- [x] File type associations specified

---

## 18. Dependencies

- **Formats:** CommonMark 0.30, GFM spec, Obsidian Canvas v1.0
- **Tooling:** `comrak`, `swc`, `serde_json`, `toml` crates
- **Export:** `pandoc` (for PDF/HTML generation)
- **Validation:** JSON Schema, TOML validation via `schema.toml`

---

## 19. Non-Scope / Out of Scope

- **Encryption:** File-level encryption deferred to vault security spec
- **Conflict resolution:** Multi-user sync conflicts addressed in collaboration spec
- **Custom DSLs:** Domain-specific languages (formula syntax, etc.) detailed in separate specs
- **Performance optimization:** Advanced caching, lazy loading detailed in implementation PRD

---

## 20. Success Metrics

1. All file formats support round-trip import/export without loss
2. Parse performance meets targets for 99% of vaults
3. Users can migrate from Obsidian without manual file editing
4. Format updates deploy with zero breaking changes in minor versions
5. Plugin authors can extend frontmatter and use all core formats

---

**Document End**
