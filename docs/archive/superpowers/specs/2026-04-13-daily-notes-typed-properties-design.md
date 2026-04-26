# Daily Notes + Typed Property Index Design

**Date:** 2026-04-13
**Status:** Approved
**Scope:** BL-001 (Daily Notes CLI command) + BL-002 (Typed Property Index schema migration)

---

## BL-001: Daily Notes

New CLI command `nexus content daily [--date YYYY-MM-DD]`.

- Creates `notes/daily/YYYY-MM-DD.md` using today's date or `--date` override
- If file already exists, prints its path without overwriting
- Template uses `chrono` for date formatting (already a workspace dependency)
- File goes through `StorageEngine::write_file` so it's indexed with tags and graph

Template:
```markdown
---
date: 2026-04-13
tags: [daily]
---
# April 13, 2026

## Tasks

## Notes
```

### Files
- Modify: `crates/nexus-cli/src/main.rs` — add `Daily` variant to `ContentCommand`
- Modify: `crates/nexus-cli/src/commands/content.rs` — add `daily` handler

---

## BL-002: Typed Property Index

Schema migration v3 adds typed columns to `properties`:

```sql
ALTER TABLE properties ADD COLUMN value_num REAL;
ALTER TABLE properties ADD COLUMN value_date INTEGER;
ALTER TABLE properties ADD COLUMN value_bool BOOLEAN;
```

The `insert_property` function in `index.rs` populates these columns based on `property_type`:
- `"number"` → parse JSON value to f64, store in `value_num`
- `"string"` with YYYY-MM-DD pattern → parse to unix timestamp, store in `value_date`
- JSON boolean → store in `value_bool`

The existing `value TEXT` column remains the source of truth. Typed columns are populated for future use.

### Files
- Modify: `crates/nexus-storage/src/schema.rs` — add `apply_migration_003`, bump `CURRENT_VERSION` to 3
- Modify: `crates/nexus-storage/src/index.rs` — update `insert_property` to populate typed columns

---

## Testing

- Daily Notes: integration test in `prd-06-smoke.rs` — create daily note, verify indexed with `daily` tag
- Typed Properties: unit tests in `schema.rs` and `index.rs` — verify columns exist, verify typed values populated

## Out of Scope

- Custom daily note templates
- Typed property query functions (future work)
- Date parsing beyond YYYY-MM-DD format
