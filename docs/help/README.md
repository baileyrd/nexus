# Nexus Help

Nexus is a local-first, plugin-first knowledge workspace built around plain
markdown files. Your notes live as `.md` files in a folder you choose
(called a **forge**), and Nexus indexes them, links them, searches them,
and lets you extend the workspace with first-party and community plugins.

This help is structured the way you'd discover Nexus: install it, open a
forge, learn the editor, learn how notes link together, then branch into
plugins, AI, and the more advanced subsystems (canvas, bases, agents,
workflows, MCP).

## Getting started

- [Install Nexus](getting-started/install.md)
- [Create your first forge](getting-started/first-forge.md)
- [A 10-minute tour](getting-started/quick-tour.md)
- [Pick your frontend: shell, CLI, TUI, MCP](getting-started/frontends.md)

## Working with notes

- [Forge layout, files, and folders](forge-and-files.md)
- [The editor](editing/editor.md)
- [Markdown formatting and blocks](editing/markdown-and-blocks.md)
- [Embeds and MDX components](editing/embeds-and-mdx.md)
- [Comments and annotations](editing/comments.md)

## Linking and organizing

- [Wikilinks and outgoing links](linking/wikilinks.md)
- [Backlinks](linking/backlinks.md)
- [Tags and properties](linking/tags-and-properties.md)
- [The knowledge graph](linking/graph.md)

## Search

- [Full-text search](search.md)

## Plugins

- [Plugins overview](plugins/overview.md)
- [Installing community plugins](plugins/install-community.md)
- [Building your own plugin](plugins/build-your-own.md)

## AI

- [AI overview and privacy](ai/overview.md)
- [Chat and RAG](ai/chat.md)
- [Inline completion in the editor](ai/inline-completion.md)
- [Configuring providers (Claude, OpenAI, Ollama)](ai/providers.md)

## Advanced

- [Skills (prompt templates)](advanced/skills.md)
- [Agents (tool-using AI loops)](advanced/agents.md)
- [Workflows (automation)](advanced/workflows.md)
- [Canvas (visual outliner)](advanced/canvas.md)
- [Bases (databases and views)](advanced/bases.md)
- [Terminal and process manager](advanced/terminal.md)
- [Git integration](advanced/git.md)
- [MCP server (Claude Code, Cursor, …)](advanced/mcp-server.md)

## Customization

- [Settings](customize/settings.md)
- [Themes](customize/themes.md)
- [Keybindings and the command palette](customize/keybindings.md)

## Reference

If you want the dry "every flag, every key" reference, see:

- [`../users/cli.md`](../users/cli.md) — every CLI subcommand and flag
- [`../users/tui.md`](../users/tui.md) — every TUI keybinding
- [`../users/mcp.md`](../users/mcp.md) — every MCP tool

---

**File-as-truth.** Whatever you see in Nexus comes from markdown on disk.
You can edit a note in any other editor and Nexus will pick the change up.
You can delete the entire `.forge/` directory and Nexus will rebuild its
index from your files. Your notes are never trapped in a database.
