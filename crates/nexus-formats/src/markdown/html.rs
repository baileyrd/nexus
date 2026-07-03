//! HTML export for markdown content.

use comrak::{format_html, parse_document, Arena, Options};

/// Extensions recognized as images for `![[target]]` embed syntax — mirrors
/// the shell's `EMBED_IMAGE_RE` (`markdownRender.ts`) so an export renders
/// the same embeds as real images the in-app preview does.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "avif",
];

fn is_image_target(target: &str) -> bool {
    target
        .rsplit('.')
        .next()
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Rewrite `![[target]]` / `![[target|display]]` embeds whose target looks
/// like an image into standard Markdown image syntax `![display](target)`,
/// which comrak's native `CommonMark` image support already renders
/// correctly. Non-image embeds (e.g. `![[another-note]]`) are left
/// untouched — comrak's wikilinks extension (enabled below) still turns the
/// `[[...]]` part into a real link, just prefixed with a literal `!`, a
/// minor artifact this pass doesn't address since comrak has no native
/// "note embed" concept to render into.
///
/// Byte-safe on multi-byte UTF-8 content: every guard character checked
/// here (`!`, `[`, `]`, `|`) is ASCII, so a byte equal to one of them is
/// never a continuation byte of a multi-byte sequence, and the computed
/// slice boundaries always land on valid `char` boundaries.
fn rewrite_image_embeds(content: &str) -> String {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    while i < len {
        if bytes[i] == b'!' && bytes.get(i + 1) == Some(&b'[') && bytes.get(i + 2) == Some(&b'[') {
            if let Some(close_rel) = content[i + 3..].find("]]") {
                let inner = &content[i + 3..i + 3 + close_rel];
                let (target, display) = match inner.split_once('|') {
                    Some((t, d)) => (t.trim(), d.trim()),
                    None => (inner.trim(), inner.trim()),
                };
                if !target.is_empty() && is_image_target(target) {
                    out.push_str("![");
                    out.push_str(display);
                    out.push_str("](");
                    out.push_str(target);
                    out.push(')');
                    i += 3 + close_rel + 2;
                    continue;
                }
            }
        }
        let ch_len = content[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&content[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// Render markdown `content` to a complete standalone HTML document,
/// rendering Nexus's own note conventions instead of leaving them mangled
/// (C67, #420):
///
/// - YAML frontmatter (`---\n...\n---` at the top of the file) is parsed
///   and stripped from the output instead of falling through `CommonMark`'s
///   thematic-break/heading rules and leaking raw YAML as a bogus `<hr>`
///   + `<h2>`.
/// - `[[target]]` / `[[target|display]]` wikilinks render as real `<a>`
///   links (comrak's `wikilinks_title_after_pipe`, matching Nexus's own
///   target-then-display pipe order) instead of staying literal bracket
///   text. The link `href` is the literal target text, unresolved to an
///   actual forge-relative path — full path resolution needs forge/source
///   context this pure `content -> String` function doesn't have; a real
///   fix is a separate, larger follow-up (see `resolve_wikilink`).
/// - `![[image.png]]` / `![[image.png|caption]]` image embeds render as
///   real `<img>` tags (rewritten to standard Markdown image syntax before
///   parsing — see [`rewrite_image_embeds`]) instead of literal bracket
///   text. Non-image embeds (embedding another note's content) are
///   unaffected by this fix and stay literal — comrak's wikilinks
///   extension does not recognize `[[...]]` preceded by `!` as a link at
///   all (verified empirically), and comrak has no native "note embed"
///   concept to render into. Not a regression: identical to the pre-fix
///   behavior for this one case.
/// - `> [!note]` / `[!tip]` / `[!important]` / `[!warning]` / `[!caution]`
///   callouts (comrak's built-in `alerts` extension, GitHub's fixed
///   5-keyword vocabulary) render as styled `.markdown-alert` divs instead
///   of a plain blockquote with the marker visible. Nexus's own callout
///   convention accepts any alphabetic type word and the shell's renderer
///   recognizes 12 kinds (`markdownRender.ts`) — the 7 kinds outside
///   comrak's 5 still fall through to an ordinary blockquote with the
///   marker text visible, exactly as before this fix (not a regression,
///   just not fully covered). Closing that gap needs a custom AST walk
///   rather than a comrak option flag — a larger follow-up.
///
/// Relative image `src`/embed targets are **not** resolved or inlined —
/// they pass through as-is and will 404 once the HTML leaves the forge
/// directory tree. Base64-inlining or copying a sibling `assets/` folder
/// is a real feature with no existing precedent in this codebase; out of
/// scope for this fix.
///
/// # Arguments
///
/// * `content` - Markdown source text (frontmatter included, if any).
/// * `title`   - Plain text used for the `<title>` element (HTML-escaped).
///
/// # Panics
///
/// Panics if the comrak HTML renderer fails to write to a `String` (extremely
/// rare internal allocation failure).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn export_to_html(content: &str, title: &str) -> String {
    let content = rewrite_image_embeds(content);

    let mut opts = Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    // C67 — strip YAML frontmatter from the rendered body instead of
    // letting it fall through as a bogus <hr>/<h2>; comrak recognizes and
    // discards a leading `---\n...\n---` block from HTML output when this
    // is set, with no extra parsing needed here.
    opts.extension.front_matter_delimiter = Some("---".to_string());
    // C67 — `[[target|display]]` (Nexus's own pipe order: target first,
    // display after the pipe) becomes a real link node instead of literal
    // bracket text.
    opts.extension.wikilinks_title_after_pipe = true;
    // C67 — the 5 GitHub-style callout keywords comrak recognizes natively.
    opts.extension.alerts = true;
    opts.render.r#unsafe = false; // safe mode: no raw HTML passthrough

    let arena = Arena::new();
    let root = parse_document(&arena, &content, &opts);

    let mut html_body = String::new();
    format_html(root, &opts, &mut html_body).expect("comrak HTML render failed");
    let body = &html_body;

    let escaped_title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{escaped_title}</title>
<style>
body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    max-width: 48rem;
    margin: 2rem auto;
    padding: 0 1rem;
    line-height: 1.6;
    color: #1a1a1a;
    background: #fff;
}}
h1, h2, h3, h4, h5, h6 {{
    margin-top: 1.5em;
    margin-bottom: 0.5em;
    line-height: 1.25;
}}
h1 {{ font-size: 2rem; border-bottom: 1px solid #e5e5e5; padding-bottom: 0.3em; }}
h2 {{ font-size: 1.5rem; border-bottom: 1px solid #e5e5e5; padding-bottom: 0.3em; }}
code {{
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
    background: #f5f5f5;
    padding: 0.15em 0.3em;
    border-radius: 3px;
    font-size: 0.9em;
}}
pre {{
    background: #f5f5f5;
    padding: 1em;
    border-radius: 6px;
    overflow-x: auto;
}}
pre code {{
    background: none;
    padding: 0;
}}
table {{
    border-collapse: collapse;
    width: 100%;
    margin: 1em 0;
}}
th, td {{
    border: 1px solid #ddd;
    padding: 0.5em 0.75em;
    text-align: left;
}}
th {{
    background: #f5f5f5;
    font-weight: 600;
}}
blockquote {{
    margin: 1em 0;
    padding: 0.5em 1em;
    border-left: 4px solid #ddd;
    color: #555;
    background: #fafafa;
}}
input[type="checkbox"] {{
    margin-right: 0.4em;
}}
ul.contains-task-list {{
    list-style: none;
    padding-left: 0;
}}
a {{
    color: #0366d6;
    text-decoration: none;
}}
a:hover {{
    text-decoration: underline;
}}
img {{
    max-width: 100%;
}}
hr {{
    border: none;
    border-top: 1px solid #e5e5e5;
    margin: 2em 0;
}}
.markdown-alert {{
    margin: 1em 0;
    padding: 0.5em 1em;
    border-left: 4px solid #888;
    background: #fafafa;
}}
.markdown-alert-title {{
    font-weight: 600;
    margin: 0 0 0.3em;
}}
.markdown-alert-note {{ border-left-color: #0366d6; }}
.markdown-alert-tip {{ border-left-color: #28a745; }}
.markdown-alert-important {{ border-left-color: #8250df; }}
.markdown-alert-warning {{ border-left-color: #d29922; }}
.markdown-alert-caution {{ border-left-color: #cf222e; }}
</style>
</head>
<body>
{body}</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_heading_to_h1() {
        let html = export_to_html("# Hello", "Test");
        assert!(
            html.contains("<h1>Hello</h1>"),
            "expected <h1>, got:\n{html}"
        );
    }

    #[test]
    fn renders_code_block() {
        let md = "```\nlet x = 1;\n```\n";
        let html = export_to_html(md, "Code");
        assert!(html.contains("<code>"), "expected <code>, got:\n{html}");
    }

    #[test]
    fn renders_task_list_with_checkboxes() {
        let md = "- [ ] Todo\n- [x] Done\n";
        let html = export_to_html(md, "Tasks");
        assert!(
            html.contains("checkbox"),
            "expected checkbox input, got:\n{html}"
        );
    }

    #[test]
    fn renders_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n";
        let html = export_to_html(md, "Table");
        assert!(html.contains("<table>"), "expected <table>, got:\n{html}");
    }

    #[test]
    fn has_complete_html_structure() {
        let html = export_to_html("Hello", "T");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<style>"));
    }

    #[test]
    fn title_is_escaped() {
        let html = export_to_html("Hi", "<script>alert(1)</script>");
        assert!(
            !html.contains("<script>"),
            "title should be escaped, got:\n{html}"
        );
        assert!(html.contains("&lt;script&gt;"));
    }

    // ── C67 (#420) — Nexus convention-aware rendering ───────────────────

    #[test]
    fn frontmatter_is_stripped_not_mangled() {
        let md = "---\ntitle: My Note\ntags: [a, b]\n---\n# Body\n";
        let html = export_to_html(md, "T");
        assert!(
            !html.contains("title: My Note"),
            "raw YAML must not leak into the body, got:\n{html}"
        );
        assert!(!html.contains("<hr"), "no bogus thematic break, got:\n{html}");
        assert!(html.contains("<h1>Body</h1>"));
    }

    #[test]
    fn wikilink_renders_as_a_real_link() {
        let html = export_to_html("See [[Other Note]] for details.", "T");
        assert!(
            html.contains(r#"data-wikilink="true""#),
            "expected a real wikilink anchor, got:\n{html}"
        );
        assert!(!html.contains("[[Other Note]]"));
    }

    #[test]
    fn wikilink_with_display_text_uses_nexus_pipe_order() {
        // Nexus order is [[target|display]] — target first.
        let html = export_to_html("[[notes/target|Friendly Name]]", "T");
        assert!(html.contains("Friendly Name"), "got:\n{html}");
        assert!(html.contains("notes/target"), "got:\n{html}");
    }

    #[test]
    fn image_embed_renders_as_a_real_img_tag() {
        let html = export_to_html("![[diagram.png]]", "T");
        assert!(
            html.contains(r#"<img src="diagram.png""#),
            "expected a real <img>, got:\n{html}"
        );
        assert!(!html.contains("[[diagram.png]]"));
    }

    #[test]
    fn image_embed_with_caption_uses_it_as_alt_text() {
        let html = export_to_html("![[diagram.png|Architecture Diagram]]", "T");
        assert!(html.contains(r#"alt="Architecture Diagram""#), "got:\n{html}");
    }

    #[test]
    fn non_image_embed_stays_literal_not_a_regression() {
        // comrak's wikilinks extension does not recognize `[[...]]`
        // preceded by `!` as a link at all — same literal-text behavior as
        // before this fix, just not additionally addressed by it.
        let html = export_to_html("![[another-note]]", "T");
        assert!(html.contains("![[another-note]]"), "got:\n{html}");
    }

    #[test]
    fn known_callout_kind_renders_as_a_styled_alert() {
        let md = "> [!warning] Be careful\n> This is risky.\n";
        let html = export_to_html(md, "T");
        assert!(
            html.contains(r#"class="markdown-alert markdown-alert-warning""#),
            "got:\n{html}"
        );
        assert!(html.contains("Be careful"));
        assert!(!html.contains("[!warning]"), "marker text must not leak, got:\n{html}");
    }

    #[test]
    fn unknown_callout_kind_falls_back_to_plain_blockquote() {
        // "risk" is a Nexus/shell callout kind but not one of comrak's 5 —
        // documented partial coverage, not a regression from pre-fix behavior.
        let md = "> [!risk] Heads up\n> Something risky.\n";
        let html = export_to_html(md, "T");
        assert!(html.contains("<blockquote>"), "got:\n{html}");
        assert!(html.contains("[!risk]"), "got:\n{html}");
    }

    #[test]
    fn regular_image_syntax_is_unaffected_by_the_embed_rewrite() {
        let html = export_to_html("![alt text](photo.jpg)", "T");
        assert!(html.contains(r#"<img src="photo.jpg" alt="alt text""#), "got:\n{html}");
    }

    #[test]
    fn embed_rewrite_is_byte_safe_on_multibyte_utf8_content() {
        // Non-ASCII text around/inside the embed must not panic on slicing;
        // the surrounding café text must survive untouched either side of
        // the rewritten embed.
        let html = export_to_html("café ![[café.png|é]] more café", "T");
        assert!(html.contains("café"), "got:\n{html}");
        assert!(html.contains("<img "), "expected the embed to still become an <img>, got:\n{html}");
    }
}
