//! HTML export for markdown content.

use comrak::{format_html, parse_document, Arena, Options};

/// Render markdown `content` to a complete standalone HTML document.
///
/// Parses with the same comrak extensions used elsewhere (strikethrough,
/// table, autolink, tasklist) and wraps the result in a styled HTML page.
/// Raw HTML in the source is escaped (safe mode).
///
/// # Arguments
///
/// * `content` - Markdown source text.
/// * `title`   - Plain text used for the `<title>` element (HTML-escaped).
///
/// # Panics
///
/// Panics if the comrak HTML renderer fails to write to a `String` (extremely
/// rare internal allocation failure).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn export_to_html(content: &str, title: &str) -> String {
    let mut opts = Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    opts.render.r#unsafe = false; // safe mode: no raw HTML passthrough

    let arena = Arena::new();
    let root = parse_document(&arena, content, &opts);

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
}
