//! File viewer widget for nexus-tui.
//!
//! Renders the right-hand pane with the content of the currently loaded file,
//! including line numbers and markdown syntax highlighting.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::{Focus, TuiApp};

/// Render the file viewer into `area`.
///
/// The block border is blue when focused, dark-gray otherwise. The title shows
/// the current file path, or `" Preview "` when no file is loaded.
pub fn render(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let focused = app.focus == Focus::Viewer;
    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };

    let title = app
        .viewer
        .file_path
        .as_deref()
        .unwrap_or(" Preview ");

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color));

    if app.viewer.file_path.is_none() {
        let placeholder = Paragraph::new(Span::styled(
            "Select a file from the tree (Enter)",
            Style::default().fg(Color::DarkGray),
        ))
        .block(block);
        frame.render_widget(placeholder, area);
        return;
    }

    // Height inside borders.
    let inner_height = area.height.saturating_sub(2) as usize;
    let total = app.viewer.lines.len();
    let gutter_width = total.to_string().len().max(1);

    let start = app.viewer.scroll_offset.min(total.saturating_sub(1));
    let end = (start + inner_height).min(total);

    let rendered_lines: Vec<Line> = app.viewer.lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, raw_line)| {
            let line_num = start + i + 1; // 1-based
            let gutter_text = format!("{:>width$} \u{2502} ", line_num, width = gutter_width);
            let gutter_span = Span::styled(
                gutter_text,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            );

            let mut spans = vec![gutter_span];
            spans.extend(highlight_line(raw_line));
            Line::from(spans)
        })
        .collect();

    let viewer = Paragraph::new(rendered_lines).block(block);
    frame.render_widget(viewer, area);
}

/// Apply markdown block-level syntax highlighting to `line`.
///
/// Returns a `Vec<Span>` covering the entire line.
fn highlight_line(line: &str) -> Vec<Span<'static>> {
    // Headings (#, ##, …, ######)
    if let Some(rest) = line.strip_prefix("######") {
        let style = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
        return vec![Span::styled(format!("######{rest}"), style)];
    }
    if let Some(rest) = line.strip_prefix("#####") {
        let style = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
        return vec![Span::styled(format!("#####{rest}"), style)];
    }
    if let Some(rest) = line.strip_prefix("####") {
        let style = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
        return vec![Span::styled(format!("####{rest}"), style)];
    }
    if let Some(rest) = line.strip_prefix("###") {
        let style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        return vec![Span::styled(format!("###{rest}"), style)];
    }
    if let Some(rest) = line.strip_prefix("##") {
        let style = Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD);
        return vec![Span::styled(format!("##{rest}"), style)];
    }
    if let Some(rest) = line.strip_prefix('#') {
        let style = Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD);
        return vec![Span::styled(format!("#{rest}"), style)];
    }

    // Fenced code blocks
    if line.starts_with("```") {
        return vec![Span::styled(
            line.to_owned(),
            Style::default().fg(Color::DarkGray),
        )];
    }

    // Block quotes
    if line.starts_with('>') {
        return vec![Span::styled(
            line.to_owned(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC | Modifier::DIM),
        )];
    }

    // Horizontal rules
    if line == "---" || line == "***" || line == "___" {
        return vec![Span::styled(
            line.to_owned(),
            Style::default().fg(Color::DarkGray),
        )];
    }

    // Unordered list items and all other lines: fall through to inline highlighting
    // so [[wikilinks]], `code`, and #tags are colored within list items too.
    highlight_inline(line)
}

/// Scan `line` character by character to apply inline markdown styles.
///
/// Recognises:
/// - `` `inline code` `` → Yellow
/// - `[[wikilink]]` → Cyan + underlined
/// - `#tag` (at start or after whitespace) → Green
/// - Regular text → default (unstyled)
#[allow(unused_assignments)] // macro sets plain_start then control flow continues at that value
fn highlight_inline(line: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut plain_start = 0;

    /// Flush accumulated plain text into `spans`.
    macro_rules! flush_plain {
        ($end:expr) => {
            if plain_start < $end {
                let text: String = chars[plain_start..$end].iter().collect();
                spans.push(Span::raw(text));
            }
            plain_start = $end;
        };
    }

    while i < len {
        // ── backtick inline code ────────────────────────────────────────────
        if chars[i] == '`' {
            if let Some(close) = chars[i + 1..].iter().position(|&c| c == '`') {
                let close_abs = i + 1 + close;
                flush_plain!(i);
                let text: String = chars[i..=close_abs].iter().collect();
                spans.push(Span::styled(text, Style::default().fg(Color::Yellow)));
                i = close_abs + 1;
                plain_start = i;
                continue;
            }
        }

        // ── [[wikilink]] ────────────────────────────────────────────────────
        if i + 1 < len && chars[i] == '[' && chars[i + 1] == '[' {
            // Find closing "]]"
            let search_start = i + 2;
            let mut found = None;
            let mut j = search_start;
            while j + 1 < len {
                if chars[j] == ']' && chars[j + 1] == ']' {
                    found = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(close) = found {
                let close_abs = close + 1; // points to second ']'
                flush_plain!(i);
                let text: String = chars[i..=close_abs].iter().collect();
                spans.push(Span::styled(
                    text,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                i = close_abs + 1;
                plain_start = i;
                continue;
            }
        }

        // ── #tag ────────────────────────────────────────────────────────────
        if chars[i] == '#' {
            let at_boundary = i == 0 || chars[i - 1].is_whitespace();
            if at_boundary && i + 1 < len && !chars[i + 1].is_whitespace() {
                // Collect to end of tag (non-whitespace run).
                let tag_start = i;
                i += 1;
                while i < len && !chars[i].is_whitespace() {
                    i += 1;
                }
                flush_plain!(tag_start);
                let text: String = chars[tag_start..i].iter().collect();
                spans.push(Span::styled(text, Style::default().fg(Color::Green)));
                plain_start = i;
                continue;
            }
        }

        i += 1;
    }

    // Flush any remaining plain text.
    if plain_start < len {
        let text: String = chars[plain_start..].iter().collect();
        spans.push(Span::raw(text));
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }

    spans
}
