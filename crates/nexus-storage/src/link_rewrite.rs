//! C2 (capability-assessment 2026-07-02) — inbound-link rewriting on
//! note rename/move.
//!
//! Pure text transformation: given a note's markdown `content` and the
//! old/new forge-relative paths of a renamed file, rewrite every
//! wikilink / embed / markdown link that pointed at the old path so it
//! points at the new one, **preserving the author's link form**:
//!
//!   - `[[Stem]]`            → `[[NewStem]]`
//!   - `[[name.md]]`         → `[[new-name.md]]`
//!   - `[[dir/name]]`        → `[[new-dir/new-name]]`
//!   - `[[t#frag]]`/`[[t|a]]`→ target swapped, fragment/alias kept
//!   - `![[img.png]]`        → `![[new/img.png]]` (embeds, any ext)
//!   - `[label](dir/name.md)`→ destination swapped (raw or `%20`-form)
//!
//! Which files to rewrite is decided by the caller from the index's
//! `links` table (`target_file_id` join), so this module never guesses
//! at resolution precedence across the forge. Within a referencing
//! file, a stem-form target is rewritten when it matches the renamed
//! file's stem case-insensitively — mirroring `resolve_link`'s
//! third tier. (If a same-stem sibling also matches, the caller's
//! target-file gate has already established that this file's links
//! resolved to the renamed file.)
//!
//! Like the indexer's own link extraction (`parser.rs`), the wikilink
//! scan does not special-case code fences — rewrite semantics match
//! index semantics by construction.

/// Strip a markdown extension (`.md` / `.markdown`), if present.
fn strip_md_ext(path: &str) -> &str {
    path.strip_suffix(".md")
        .or_else(|| path.strip_suffix(".markdown"))
        .unwrap_or(path)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The old path's recognised authored forms, precomputed.
struct OldForms {
    relpath: String,
    relpath_no_ext: String,
    basename: String,
    stem_lower: String,
}

impl OldForms {
    fn new(old_path: &str) -> Self {
        let base = basename(old_path);
        Self {
            relpath: old_path.to_string(),
            relpath_no_ext: strip_md_ext(old_path).to_string(),
            basename: base.to_string(),
            stem_lower: strip_md_ext(base).to_lowercase(),
        }
    }

    /// Does an authored wikilink/embed target point at the old path?
    fn matches(&self, target: &str) -> bool {
        let t = target.trim();
        if t.is_empty() {
            return false;
        }
        if t == self.relpath || t == self.relpath_no_ext {
            return true;
        }
        if t.contains('/') {
            return false;
        }
        t == self.basename || t.to_lowercase() == self.stem_lower
    }
}

/// Rewrite an authored target into the equivalent form for `new_path`,
/// preserving the author's style: path forms stay path forms, bare
/// stems stay stems, and an explicit extension is kept.
fn map_form(target: &str, new_path: &str) -> String {
    let t = target.trim();
    let had_ext = basename(t).contains('.');
    if t.contains('/') {
        if had_ext {
            new_path.to_string()
        } else {
            strip_md_ext(new_path).to_string()
        }
    } else {
        let new_base = basename(new_path);
        if had_ext {
            new_base.to_string()
        } else {
            strip_md_ext(new_base).to_string()
        }
    }
}

/// Minimal percent-encoding for markdown destinations: spaces only —
/// the form the shell's own link insertion (`encodeURI`) and common
/// editors produce for forge paths.
fn encode_spaces(path: &str) -> String {
    path.replace(' ', "%20")
}

/// Rewrite every link in `content` that targets `old_path` so it
/// targets `new_path`. Returns `None` when nothing changed, else the
/// rewritten content and the number of link occurrences updated.
#[must_use]
pub fn rewrite_links(
    content: &str,
    old_path: &str,
    new_path: &str,
) -> Option<(String, usize)> {
    let old = OldForms::new(old_path);
    let mut replaced = 0usize;

    // ── pass 1: wikilinks + embeds ────────────────────────────────
    let mut out = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let start = i + 2;
            if let Some(rel) = content[start..].find("]]") {
                let inner = &content[start..start + rel];
                // Target = inner up to the first `#` (fragment) or `|`
                // (alias); the remainder is preserved verbatim.
                let cut = inner
                    .find(['#', '|'])
                    .unwrap_or(inner.len());
                let (target, rest) = inner.split_at(cut);
                if old.matches(target) {
                    out.push_str("[[");
                    out.push_str(&map_form(target, new_path));
                    out.push_str(rest);
                    out.push_str("]]");
                    replaced += 1;
                    i = start + rel + 2;
                    continue;
                }
            }
        }
        // Advance one UTF-8 code point.
        let ch_len = content[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&content[i..i + ch_len]);
        i += ch_len;
    }

    // ── pass 2: markdown link destinations ────────────────────────
    // Textual `](dest)` swaps for the destination spellings we can
    // map deterministically: the root-relative path, raw or with
    // `%20`-encoded spaces, with or without a `./` prefix. Source-dir
    // relative destinations (`../x.md`) are left alone in v1 — the
    // caller's report lets frontends surface how many links were
    // updated so a miss is visible rather than silent.
    let dest_pairs = [
        (old.relpath.clone(), new_path.to_string()),
        (encode_spaces(&old.relpath), encode_spaces(new_path)),
        (format!("./{}", old.relpath), format!("./{new_path}")),
        (
            format!("./{}", encode_spaces(&old.relpath)),
            format!("./{}", encode_spaces(new_path)),
        ),
    ];
    let mut current = out;
    for (old_dest, new_dest) in &dest_pairs {
        if old_dest == new_dest {
            continue;
        }
        for (open, close) in [("](", ")"), ("](<", ">)")] {
            let needle = format!("{open}{old_dest}{close}");
            if current.contains(&needle) {
                let swap = format!("{open}{new_dest}{close}");
                let n = current.matches(&needle).count();
                current = current.replace(&needle, &swap);
                replaced += n;
            }
        }
    }

    if replaced == 0 {
        None
    } else {
        Some((current, replaced))
    }
}

#[cfg(test)]
mod tests {
    use super::rewrite_links;

    #[test]
    fn stem_wikilink_rewrites_to_new_stem() {
        let (out, n) =
            rewrite_links("See [[Old Note]] here.", "notes/Old Note.md", "notes/New Note.md")
                .unwrap();
        assert_eq!(out, "See [[New Note]] here.");
        assert_eq!(n, 1);
    }

    #[test]
    fn stem_match_is_case_insensitive_like_resolve_link() {
        let (out, _) =
            rewrite_links("[[old note]]", "notes/Old Note.md", "notes/New.md").unwrap();
        assert_eq!(out, "[[New]]");
    }

    #[test]
    fn alias_and_fragment_are_preserved() {
        let (out, n) = rewrite_links(
            "[[Old#Heading|shown text]] and [[Old#Other]]",
            "Old.md",
            "sub/New.md",
        )
        .unwrap();
        assert_eq!(out, "[[New#Heading|shown text]] and [[New#Other]]");
        assert_eq!(n, 2);
    }

    #[test]
    fn path_form_keeps_extension_presence() {
        let (out, _) = rewrite_links(
            "[[notes/Old.md]] and [[notes/Old]]",
            "notes/Old.md",
            "archive/New.md",
        )
        .unwrap();
        assert_eq!(out, "[[archive/New.md]] and [[archive/New]]");
    }

    #[test]
    fn embeds_rewrite_including_non_markdown_targets() {
        let (out, _) = rewrite_links(
            "![[shot.png]]",
            "attachments/shot.png",
            "attachments/renamed.png",
        )
        .unwrap();
        assert_eq!(out, "![[renamed.png]]");
    }

    #[test]
    fn markdown_destinations_rewrite_raw_and_encoded() {
        let (out, n) = rewrite_links(
            "[a](notes/Old Note.md) [b](notes/Old%20Note.md) [c](./notes/Old%20Note.md)",
            "notes/Old Note.md",
            "notes/New Note.md",
        )
        .unwrap();
        assert_eq!(
            out,
            "[a](notes/New Note.md) [b](notes/New%20Note.md) [c](./notes/New%20Note.md)"
        );
        assert_eq!(n, 3);
    }

    #[test]
    fn unrelated_links_are_untouched() {
        assert!(rewrite_links(
            "[[Other]] [x](other.md) ![[pic.png]]",
            "notes/Old.md",
            "notes/New.md"
        )
        .is_none());
    }

    #[test]
    fn same_stem_different_dir_path_form_is_untouched() {
        // `[[elsewhere/Old]]` names a different file explicitly.
        assert!(rewrite_links("[[elsewhere/Old]]", "notes/Old.md", "notes/New.md").is_none());
    }

    #[test]
    fn basename_form_with_extension_rewrites_to_new_basename() {
        let (out, _) = rewrite_links("[[Old.md]]", "notes/Old.md", "notes/New.md").unwrap();
        assert_eq!(out, "[[New.md]]");
    }
}
