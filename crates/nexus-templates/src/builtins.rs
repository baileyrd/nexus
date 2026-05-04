//! Built-in templates seeded into every forge.
//!
//! These are loaded by [`crate::TemplateRegistry`] without writing to disk
//! so a fresh forge has a useful default set immediately. Users can
//! override a built-in by creating a same-named template in
//! `<forge>/.forge/templates/`.

use crate::template::{parse_template_text, Template};

/// All built-in templates as (filename, body) pairs.
#[must_use]
pub fn all() -> Vec<(&'static str, &'static str)> {
    vec![
        ("notion-page.template.md", NOTION_PAGE),
        ("notion-database-row.template.md", NOTION_DB_ROW),
        ("daily-journal.template.md", DAILY_JOURNAL),
        ("meeting-notes.template.md", MEETING_NOTES),
    ]
}

/// Parse every built-in. Panics on malformed built-ins (would only happen
/// if the source strings below got corrupted in development — the tests
/// catch this).
#[must_use]
pub fn parsed() -> Vec<Template> {
    all()
        .into_iter()
        .map(|(name, body)| {
            parse_template_text(body, name)
                .unwrap_or_else(|e| panic!("malformed built-in template {name}: {e}"))
        })
        .collect()
}

// ── Template bodies ─────────────────────────────────────────────────────────

const NOTION_PAGE: &str = r#"---
name: notion-page
description: A Notion-style page with a property table and body sections.
target_path: "{{title}}.md"
parameters:
  - name: title
    required: true
    description: The page title.
  - name: status
    default: draft
    description: One of draft / in-progress / done.
  - name: tags
    default: ""
    description: Comma-separated list of tags.
---
---
title: {{title}}
status: {{status}}
tags: [{{tags}}]
created: {{today}}
---

# {{title}}

## Overview

## Notes
"#;

const NOTION_DB_ROW: &str = r#"---
name: notion-database-row
description: A markdown record for a Notion-style database. Pair with a `.bases` file in the same folder.
target_path: "{{database}}/{{title}}.md"
parameters:
  - name: database
    required: true
    description: Folder of the database (matches the `.bases` filename stem).
  - name: title
    required: true
    description: Row title — used for the filename and H1.
  - name: status
    default: todo
  - name: priority
    default: "3"
---
---
title: {{title}}
status: {{status}}
priority: {{priority}}
created: {{today}}
---

# {{title}}

"#;

const DAILY_JOURNAL: &str = r#"---
name: daily-journal
description: A daily-journal scaffold with sections for tasks, notes, and gratitude.
target_path: "daily/{{today}}.md"
parameters: []
---
# {{today}}

## Today

- [ ]

## Notes

## Gratitude

-
"#;

const MEETING_NOTES: &str = r#"---
name: meeting-notes
description: A meeting-notes scaffold with attendees, agenda, and action items.
target_path: "meetings/{{today}} - {{title}}.md"
parameters:
  - name: title
    required: true
    description: Meeting title.
  - name: attendees
    default: ""
    description: Comma-separated list of attendees.
---
---
title: {{title}}
date: {{today}}
attendees: [{{attendees}}]
---

# {{title}}

- **Date**: {{today}}
- **Attendees**: {{attendees}}

## Agenda

-

## Notes

## Action items

- [ ]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_parses() {
        let parsed = parsed();
        assert!(parsed.len() >= 4);
    }

    #[test]
    fn builtin_names_are_unique() {
        let parsed = parsed();
        let mut seen = std::collections::HashSet::new();
        for t in &parsed {
            assert!(
                seen.insert(t.meta.name.clone()),
                "duplicate built-in name: {}",
                t.meta.name
            );
        }
    }

    #[test]
    fn builtin_filenames_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in all() {
            assert!(seen.insert(name), "duplicate built-in filename: {name}");
        }
    }
}
