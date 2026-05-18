# The knowledge graph

Every wikilink in the forge is an edge. Every note (and every
referenced block) is a node. Together they form a directed graph that
backlinks, embeds, and graph queries all read from.

## Inspect from the CLI

```bash
nexus graph status
# nodes: 412
# edges: 1086
# unresolved: 23

nexus graph neighbors README.md --depth 2
# 1-hop: 7 nodes
# 2-hop: 41 nodes

nexus graph unresolved
# every [[wikilink]] that doesn't resolve to a file
```

## Unresolved links

When a `[[Foo]]` doesn't match any file, it becomes an **unresolved
edge**. Useful workflows:

- `nexus graph unresolved` — surface the candidates for new notes you
  haven't written yet.
- Click an unresolved link in the editor to create the file with one
  action.

## How it's built

The graph is stored in SQLite (`.forge/index.db`) and held in memory
as a [petgraph](https://github.com/petgraph/petgraph) DAG-with-cycles.
It updates incrementally as the file watcher fires:

- A file edit re-extracts links from that file only.
- A file rename moves the node; existing edges stay attached.
- A file delete drops the node and all its outgoing edges.

Even on forges with tens of thousands of notes this stays sub-second.

## Graph view (UI)

A graph visualization plugin can render the petgraph as an interactive
canvas. The shipped shell exposes graph status and neighbor queries
through panels; a full force-directed view is contributed by an
optional plugin (see [Plugins overview](../plugins/overview.md)).

## Programmatic access

Plugins can call:

```ts
const neighbors = await context.ipc.call(
  'com.nexus.storage',
  'graph_neighbors',
  { path: 'README.md', depth: 2 }
);
```

Useful for building related-notes panels, maps of content, or custom
visualizations.

## Excluding folders

```toml
# .forge/app.toml
[graph]
exclude = ["Archive/", "Templates/"]
```

Excluded files are still indexed for search and full-text retrieval —
they just don't contribute graph edges, so they don't pollute backlink
panels or graph-neighbor queries.
