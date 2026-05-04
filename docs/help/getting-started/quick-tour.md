# A 10-minute tour

This walks through the things you'll do every day. The examples assume
you're in the desktop shell (`nexus desktop`), but every action also has
a CLI form.

## 1. Create a note

In the shell, click **+** in the file tree or press the **Create note**
command from the palette (`Ctrl+Shift+P` → "Create note"). Name it
`hello.md`.

CLI:

```bash
nexus content create hello.md --content "# Hello\n\nMy first note."
```

## 2. Write some markdown

Type CommonMark + GitHub-flavored markdown. Live-preview shows headings,
code blocks, lists, tables, and tasks rendered as you type. See
[Markdown formatting and blocks](../editing/markdown-and-blocks.md).

```markdown
# Hello

I'm exploring [[Nexus]] today. See also #demo.

- [ ] Try wikilinks
- [ ] Try search
- [ ] Try the AI panel
```

## 3. Link to another note

`[[Nexus]]` is a **wikilink**. If a note called `Nexus.md` exists
anywhere in the forge, the link resolves to it. If not, it's an
**unresolved link** — click it and Nexus will offer to create the file.

See [Wikilinks](../linking/wikilinks.md).

## 4. See what links to this note

Open the **Backlinks** panel (right sidebar). Every other note that
links to the current one is listed with the surrounding excerpt.

CLI: `nexus content backlinks hello.md`.

## 5. Tag it

Inline `#demo` or YAML frontmatter:

```markdown
---
tags: [demo, getting-started]
status: draft
---
```

Open the **Tags** panel to see all tags in the forge with counts. See
[Tags and properties](../linking/tags-and-properties.md).

## 6. Search

`Ctrl+Shift+F` opens the global search panel. Tantivy full-text search
returns ranked results across the whole forge. CLI:
`nexus content search "wikilinks"`. See [Search](../search.md).

## 7. Ask the AI

Open the **AI Chat** panel. Ask "What's in my forge about wikilinks?".
RAG retrieves the most relevant chunks from your notes and cites them.
You need at least one provider configured first — see
[Configuring providers](../ai/providers.md).

Inline: place your cursor at the end of a paragraph and press
`Ctrl+Shift+Space` (or `Cmd+Shift+Space`) for an inline completion.

## 8. Toggle a task

Click any `- [ ]` checkbox in live-preview. The file on disk updates to
`- [x]`. CLI: `nexus content task-toggle hello.md --line 7`.

## 9. Drop into the terminal

Open the **Terminal** panel. It's a real PTY. Save commands you run
often as snippets in the sidebar. See [Terminal](../advanced/terminal.md).

## 10. Look at the graph

Open the **Graph** view (when wired by your theme/layout) or run
`nexus graph status` to see node and edge counts, and
`nexus graph neighbors hello.md --depth 2` to see who's two hops away.

---

That's the loop. From here:

- For day-to-day editing, dig into [The editor](../editing/editor.md).
- For AI workflows, [AI overview](../ai/overview.md).
- For automation and external integrations, look at
  [Workflows](../advanced/workflows.md), [Agents](../advanced/agents.md),
  and the [MCP server](../advanced/mcp-server.md).
