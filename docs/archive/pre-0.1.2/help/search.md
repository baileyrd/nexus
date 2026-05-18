# Full-text search

Nexus indexes every note with [Tantivy](https://github.com/quickwit-oss/tantivy),
a Rust-native Lucene-style search engine. The index lives at
`<forge>/.forge/search/` and is rebuilt incrementally as files change.

## Search from the shell

`Ctrl+Shift+F` opens the global **Search** panel. Type a query, hit
Enter, scroll through ranked results. Each hit shows the file path,
the matching excerpt, and the score.

Click a result to jump to the match in the editor with the matched
phrase highlighted.

## Search from the CLI

```bash
nexus content search "wikilinks"
nexus content search "wikilinks" --limit 50 --format json
nexus content search '"exact phrase"'
nexus content search 'wiki* AND markdown'
```

Output formats: `text`, `json`, `jsonl`, `table`.

## Search syntax

| Form | Meaning |
|---|---|
| `foo` | Documents containing "foo" |
| `foo bar` | Documents containing both terms |
| `"foo bar"` | The exact phrase |
| `foo OR bar` | Either term |
| `foo AND NOT bar` | "foo" but not "bar" |
| `wiki*` | Prefix match (anything starting with "wiki") |
| `path:projects/` | Restrict to a path prefix |
| `tag:project` | Notes carrying a tag |
| `prop:status:draft` | Notes with `status: draft` in frontmatter |

> **Note**: `path:`, `tag:`, and `prop:` operators **parse** today but
> their post-filtering pass is partial — see backlog item BL-003.
> Plain text and phrase queries are fully supported.

## Tantivy under the hood

- Schema: title, body, path, tags, properties, mtime.
- Indexing: incremental on the file-watcher tick (debounced 300 ms).
- Tokenization: standard analyzer with lowercase + stopword filter.
- Index format on disk; survives restarts; rebuildable with
  `nexus forge reindex` if it ever drifts.

## Scoring

Default ranking is BM25 over the body field with a small title-boost.
The shell does not currently expose scoring knobs in the UI; a future
release will add per-field weighting and recency boosts.

## Reindexing

You don't normally need to. If you do (after restoring from backup,
swapping disk, etc.):

```bash
nexus forge reindex                    # rebuild from files on disk
nexus forge reindex --drop             # drop and recreate from scratch
```

The forge stays usable during reindex (writes go to a side index that
swaps in atomically when complete).
