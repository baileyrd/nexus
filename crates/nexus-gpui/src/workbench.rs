//! Root window view for the Phase 0 spike.
//!
//! Validates:
//! - gpui `Entity` + `Render` lifecycle.
//! - Async IPC call from gpui background executor → tokio bridge.
//! - Terminal cell grid rendering: a fixed-size colored-cell grid proves that
//!   gpui can drive the rendering pattern needed by Phase 2's `TerminalView`.
//!
//! The layout is intentionally minimal: a dark workbench with a top bar, a
//! stats panel, and a mock terminal cell grid. No real PTY is attached yet —
//! that lands in Phase 2 alongside the `alacritty-terminal` integration.

use std::sync::Arc;

use gpui::{
    div, px, rgb, AsyncApp, Context, IntoElement, ParentElement, Render, Styled, WeakEntity,
    Window,
};
use serde::Deserialize;

use crate::KernelBridge;

// ── Colours (matches the default Nexus dark theme tokens) ────────────────────

const BG_BASE: u32 = 0x1a1b26;
const BG_PANEL: u32 = 0x24253a;
const BG_TOPBAR: u32 = 0x1e1f2e;
const FG_MUTED: u32 = 0x6b7280;
const FG_TEXT: u32 = 0xc0caf5;
const ACCENT: u32 = 0x7aa2f7;

// ── Forge stats DTO ───────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct GraphStats {
    node_count: usize,
    edge_count: usize,
    unresolved_count: usize,
}

// ── Mock terminal cell ────────────────────────────────────────────────────────

struct Cell {
    ch: char,
    fg: u32,
    bg: u32,
}

impl Cell {
    fn new(ch: char, fg: u32, bg: u32) -> Self {
        Self { ch, fg, bg }
    }
}

// ── WorkbenchView ─────────────────────────────────────────────────────────────

/// Phase 0 spike root view.
pub struct WorkbenchView {
    stats: Option<GraphStats>,
    stats_error: Option<String>,
    /// Static mock terminal grid for cell-rendering validation.
    mock_cells: Vec<Vec<Cell>>,
}

impl WorkbenchView {
    pub fn new(bridge: Arc<KernelBridge>, cx: &mut Context<Self>) -> Self {
        // Kick off an async IPC call to fetch forge stats. This validates the
        // tokio bridge from within gpui's background executor.
        cx.spawn(async move |weak: WeakEntity<WorkbenchView>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async move { bridge.call_empty("com.nexus.storage", "graph_stats") })
                .await;

            weak.update(cx, |this, cx| {
                match result {
                    Ok(v) => this.stats = serde_json::from_value(v).ok(),
                    Err(e) => this.stats_error = Some(format!("IPC error: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        Self {
            stats: None,
            stats_error: None,
            mock_cells: build_mock_grid(),
        }
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG_BASE))
            .font_family("monospace")
            .child(render_topbar())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .child(render_stats_panel(&self.stats, &self.stats_error))
                    .child(render_terminal_grid(&self.mock_cells)),
            )
            .child(render_status_bar())
    }
}

// ── Sub-renders ───────────────────────────────────────────────────────────────

fn render_topbar() -> impl IntoElement {
    div()
        .h(px(32.))
        .px(px(16.))
        .flex()
        .items_center()
        .bg(rgb(BG_TOPBAR))
        .border_b_1()
        .border_color(rgb(0x2e3150))
        .child(
            div()
                .text_color(rgb(ACCENT))
                .text_sm()
                .child("Nexus  ·  Phase 0 Spike"),
        )
}

fn render_stats_panel(stats: &Option<GraphStats>, error: &Option<String>) -> impl IntoElement {
    let body: gpui::AnyElement = if let Some(e) = error {
        div()
            .text_color(rgb(0xf7768e))
            .child(e.clone())
            .into_any_element()
    } else if let Some(s) = stats {
        div()
            .text_color(rgb(FG_TEXT))
            .child(format!("nodes  {}", s.node_count))
            .child(div().child(format!("edges  {}", s.edge_count)))
            .child(div().child(format!("unresolved  {}", s.unresolved_count)))
            .into_any_element()
    } else {
        div()
            .text_color(rgb(FG_MUTED))
            .child("loading forge stats…")
            .into_any_element()
    };

    div()
        .w(px(220.))
        .p(px(16.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .bg(rgb(BG_PANEL))
        .border_r_1()
        .border_color(rgb(0x2e3150))
        .child(
            div()
                .text_sm()
                .text_color(rgb(FG_MUTED))
                .child("Forge stats"),
        )
        .child(body)
        .child(
            div()
                .mt(px(16.))
                .text_xs()
                .text_color(rgb(FG_MUTED))
                .child("IPC: com.nexus.storage/graph_stats"),
        )
}

/// Render a static mock terminal grid.
///
/// Each cell is a fixed-size rectangle (cell_w × cell_h) with a background
/// colour and a text character. This validates gpui's ability to lay out and
/// paint a dense grid of coloured monospace cells — the same rendering pattern
/// that Phase 2's `TerminalView` will use with live `alacritty_terminal::Term`
/// data.
fn render_terminal_grid(cells: &[Vec<Cell>]) -> impl IntoElement {
    let cell_w = px(9.);
    let cell_h = px(18.);

    let rows: Vec<_> = cells
        .iter()
        .map(|row| {
            let cols: Vec<_> = row
                .iter()
                .map(|cell| {
                    div()
                        .w(cell_w)
                        .h(cell_h)
                        .bg(rgb(cell.bg))
                        .text_color(rgb(cell.fg))
                        .text_xs()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(cell.ch.to_string())
                })
                .collect();
            div().flex().flex_row().children(cols)
        })
        .collect();

    div()
        .flex_1()
        .p(px(12.))
        .flex()
        .flex_col()
        .bg(rgb(BG_BASE))
        .child(
            div()
                .text_xs()
                .text_color(rgb(FG_MUTED))
                .mb(px(8.))
                .child("Terminal cell grid (mock — Phase 0 render validation)"),
        )
        .children(rows)
}

fn render_status_bar() -> impl IntoElement {
    div()
        .h(px(22.))
        .px(px(12.))
        .flex()
        .items_center()
        .justify_between()
        .bg(rgb(ACCENT))
        .child(
            div()
                .text_xs()
                .text_color(rgb(0x1a1b26))
                .child("Nexus — kernel booted"),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(0x1a1b26))
                .child("Phase 0 spike · gpui"),
        )
}

// ── Mock grid builder ─────────────────────────────────────────────────────────

/// Build a 6-row static grid that exercises the cell colour palette.
/// Mimics what a real terminal screen would look like after
/// `alacritty_terminal::Term::renderable_content()` is wired up in Phase 2.
fn build_mock_grid() -> Vec<Vec<Cell>> {
    const GRN: u32 = 0x9ece6a;
    const YLW: u32 = 0xe0af68;
    const RED: u32 = 0xf7768e;
    const CYN: u32 = 0x7dcfff;

    let rows: Vec<&str> = vec![
        "user@nexus:~/forge$ ls notes/",
        "architecture.md   daily/   index.md   projects/",
        "user@nexus:~/forge$ cat notes/index.md",
        "# Nexus Forge                                    ",
        "Welcome to your knowledge base.                  ",
        "user@nexus:~/forge$ _                            ",
    ];

    let palette: Vec<u32> = vec![GRN, FG_TEXT, GRN, CYN, FG_TEXT, YLW];

    rows.iter()
        .zip(palette.iter())
        .map(|(line, &fg)| {
            line.chars()
                .map(|ch| {
                    let cell_fg = match ch {
                        '$' => RED,
                        '_' => ACCENT,
                        '#' => YLW,
                        _ => fg,
                    };
                    Cell::new(ch, cell_fg, BG_BASE)
                })
                .collect()
        })
        .collect()
}
