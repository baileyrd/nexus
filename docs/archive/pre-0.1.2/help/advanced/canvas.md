# Canvas (visual outliner)

A **canvas** is a 2D space of nodes and edges. Drop notes, group them,
draw connections — useful for mind maps, project boards, sketching
relationships, and visual outlines.

The canvas file format is compatible with Obsidian's `.canvas` JSON,
so you can import canvases from an existing Obsidian vault.

## Create a canvas

In the shell: **+** in the file tree → **New canvas**. The file is
saved as `My Canvas.canvas` (a JSON document).

CLI:

```bash
nexus canvas render path/to/file.canvas    # render a textual outline
```

## Node types

| Type | What it is |
|---|---|
| **Text** | Free-form markdown that renders inline |
| **File** | A reference to a note in the forge — embeds the rendered note |
| **Link** | An external URL with a preview card |
| **Group** | A box you can drop other nodes into |

## Editing

- **Click empty space** to deselect.
- **Double-click** to add a text node.
- **Drag** a node to move it; corners to resize.
- **Drag from a node's edge handle** to draw an edge to another node.
- **Right-click** for color, layer, delete.

## Edges

Edges are arrows between nodes. They can be labeled. Edge style
(straight, curved, orthogonal) is per-canvas in the canvas's settings
panel.

## Embedding notes

Drop a `.md` file from the file tree into the canvas to create a
**File node**. The note renders inline; it stays editable from the
canvas (changes write back to the source file).

## Groups

A **Group** node holds other nodes. Drag-select a region, right-click
→ **Group**. Useful for swim lanes, project columns, or bounded
contexts.

## Compatibility with Obsidian

The `.canvas` file is JSON with the same schema Obsidian uses. You can:

- Open Nexus canvases in Obsidian.
- Drop Obsidian canvases into a Nexus forge and edit them.

## Limitations

- Performance is good up to a few hundred nodes; very large canvases
  (1000+ nodes) may slow.
- No collaborative editing — single-user only.
- No infinite zoom yet — the canvas is bounded to a large fixed area.
