# Project Hub

| Property | Value |
| --- | --- |
| Status | Active |
| Lead | Grace Hopper |
| Quarter | 2026 Q2 |

A page that links to its siblings. After import, every link below
should resolve to the new (UUID-stripped) filename — that's the
`LinkIndex` rewrite in action.

## Related pages

- [Welcome](Welcome%201a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d.md) — landing
  page.
- [Tasks](Tasks%203c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f) — embedded
  database; click into individual rows from the bases view.

## Notes

> 📝 Sub-pages live next to their parent, not inside it. Notion only
> nests when the parent is itself a folder (i.e. has children of its
> own), and Nexus preserves that exactly.

```python
# Code blocks survive round-tripping, fences and language tags intact.
def hello(name: str) -> str:
    return f"hi {name}"
```
