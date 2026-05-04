# Notion-export sample

A small fixture that mimics the shape of a real **Markdown & CSV**
Notion export: 32-hex UUID-suffixed filenames, a property table at the
top of each page, emoji-prefixed callouts, internal links between
pages, and a CSV-backed database with one markdown file per row.

Use it to exercise both directions of the round-trip:

```bash
# 1. Zip it up the same way Notion ships exports.
cd examples
zip -r notion-export.zip notion-export

# 2. Import into a forge (CLI).
nexus import notion-zip ../examples/notion-export.zip --dest imported

# 3. Or from the desktop shell:
#    Command Palette → "Notion: Import zip…"
#    Pick examples/notion-export.zip, accept the destination prompt.

# 4. Round-trip: export the imported tree back to a Notion-shaped folder.
nexus export notion-dir <forge>/imported --to /tmp/notion-out
```

What the importer does to each file:

- Strips the trailing 32-hex UUID from filenames and folder names.
- Lifts the leading 2-column property table into YAML frontmatter.
- Rewrites callouts (`> 💡 …`) into Obsidian-style `> [!tip]` blocks.
- Rewrites internal links between pages so the renamed targets resolve.
- Converts each `*.csv` into a sibling `*.bases` TOML file (Nexus'
  database format) while leaving the per-row markdown files in place.

Everything in this directory is hand-authored sample content — none of
it came from a real Notion workspace.
