//! Phase 1 shell skeleton.
//!
//! Layout:
//! ```text
//! ┌─ TopBar (32px) ────────────────────────────────────────────────┐
//! │ ActivityBar │ Primary Pane            │ Secondary Pane          │
//! │   (48px)    │  (flex: 1)              │  (flex: 1)              │
//! └─ StatusBar (22px) ─────────────────────────────────────────────┘
//! ```
//!
//! The activity bar icons update `SplitLayout::primary` via click listeners.
//! Each pane slot dispatches to either the terminal cell mock (Phase 0 grid,
//! retained for render validation) or a labelled placeholder for panes that
//! land in later phases.

use std::sync::Arc;

use gpui::{
    div, hsla, px, AnyElement, AsyncApp, ClickEvent, Context, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, WeakEntity, Window,
};
use serde::Deserialize;

use crate::{
    pane::{PaneKind, SplitLayout},
    theme::Theme,
    KernelBridge,
};

// ── Activity-bar entry table ──────────────────────────────────────────────────

const ACTIVITY_ENTRIES: &[(PaneKind, &str)] = &[
    (PaneKind::Terminal, "act-terminal"),
    (PaneKind::Editor,   "act-editor"),
    (PaneKind::Ai,       "act-ai"),
    (PaneKind::Graph,    "act-graph"),
    (PaneKind::Settings, "act-settings"),
];

// ── Forge stats DTO ───────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct GraphStats {
    node_count: usize,
    edge_count: usize,
    unresolved_count: usize,
}

// ── Terminal cell (Phase 0 mock, retained for render validation) ──────────────

struct Cell {
    ch:  char,
    fg:  u32,
    bg:  u32,
}

impl Cell {
    fn new(ch: char, fg: u32, bg: u32) -> Self {
        Self { ch, fg, bg }
    }
}

// ── WorkbenchView ─────────────────────────────────────────────────────────────

pub struct WorkbenchView {
    theme:       Theme,
    layout:      SplitLayout,
    stats:       Option<GraphStats>,
    stats_error: Option<String>,
    mock_cells:  Vec<Vec<Cell>>,
}

impl WorkbenchView {
    pub fn new(bridge: Arc<KernelBridge>, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |weak: WeakEntity<WorkbenchView>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async move { bridge.call_empty("com.nexus.storage", "graph_stats") })
                .await;
            weak.update(cx, |this, cx| {
                match result {
                    Ok(v)  => this.stats = serde_json::from_value(v).ok(),
                    Err(e) => this.stats_error = Some(format!("IPC error: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        Self {
            theme:       Theme::dark(),
            layout:      SplitLayout::default(),
            stats:       None,
            stats_error: None,
            mock_cells:  build_mock_grid(),
        }
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top_bar    = self.render_top_bar();
        let act_bar    = self.render_activity_bar(cx);
        let content    = self.render_content_area();
        let status_bar = self.render_status_bar();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(self.theme.bg_base)
            .font_family("monospace")
            .child(top_bar)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .child(act_bar)
                    .child(content),
            )
            .child(status_bar)
    }
}

// ── Sub-renders ───────────────────────────────────────────────────────────────

impl WorkbenchView {
    fn render_top_bar(&self) -> impl IntoElement {
        div()
            .h(px(32.))
            .px(px(16.))
            .flex()
            .items_center()
            .justify_between()
            .bg(self.theme.bg_elevated)
            .border_b_1()
            .border_color(self.theme.border)
            .child(
                div()
                    .text_color(self.theme.accent)
                    .text_sm()
                    .child("Nexus  ·  Phase 1 Shell Skeleton"),
            )
            .child(
                div()
                    .text_color(self.theme.fg_muted)
                    .text_xs()
                    .child("gpui"),
            )
    }

    fn render_activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active  = self.layout.primary;
        let accent  = self.theme.accent;
        let border  = self.theme.border;
        let panel   = self.theme.bg_panel;
        let elevated = self.theme.bg_elevated;
        let fg_muted = self.theme.fg_muted;
        let transparent = hsla(0., 0., 0., 0.);

        let items: Vec<AnyElement> = ACTIVITY_ENTRIES
            .iter()
            .map(|&(kind, id)| {
                let is_active    = kind == active;
                let item_bg      = if is_active { panel }       else { transparent };
                let item_fg      = if is_active { accent }      else { fg_muted };
                let left_border  = if is_active { accent }      else { transparent };

                div()
                    .id(id)
                    .w(px(48.))
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(item_bg)
                    .border_l_2()
                    .border_color(left_border)
                    .text_color(item_fg)
                    .text_sm()
                    .cursor_pointer()
                    .child(kind.icon())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.layout.primary = kind;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();

        div()
            .w(px(48.))
            .flex()
            .flex_col()
            .bg(elevated)
            .border_r_1()
            .border_color(border)
            .children(items)
    }

    fn render_content_area(&self) -> impl IntoElement {
        let primary_pane   = self.render_pane(self.layout.primary);
        let secondary_kind = self.layout.secondary;
        let border         = self.theme.border;

        let mut row = div().flex().flex_row().flex_1();
        row = row.child(div().flex_1().child(primary_pane));

        if let Some(kind) = secondary_kind {
            let secondary_pane = self.render_pane(kind);
            row = row
                .child(div().w(px(1.)).bg(border))
                .child(div().flex_1().child(secondary_pane));
        }

        row
    }

    fn render_pane(&self, kind: PaneKind) -> AnyElement {
        match kind {
            PaneKind::Terminal => self.render_terminal_pane().into_any_element(),
            other              => self.render_placeholder_pane(other).into_any_element(),
        }
    }

    /// Renders the Phase 0 mock terminal grid, proving cell-grid rendering works.
    fn render_terminal_pane(&self) -> impl IntoElement {
        let cell_w = px(9.);
        let cell_h = px(18.);

        let rows: Vec<AnyElement> = self
            .mock_cells
            .iter()
            .map(|row| {
                let cols: Vec<AnyElement> = row
                    .iter()
                    .map(|cell| {
                        div()
                            .w(cell_w)
                            .h(cell_h)
                            .bg(gpui::rgb(cell.bg))
                            .text_color(gpui::rgb(cell.fg))
                            .text_xs()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(cell.ch.to_string())
                            .into_any_element()
                    })
                    .collect();
                div()
                    .flex()
                    .flex_row()
                    .children(cols)
                    .into_any_element()
            })
            .collect();

        div()
            .size_full()
            .p(px(12.))
            .flex()
            .flex_col()
            .bg(self.theme.bg_base)
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.fg_muted)
                    .mb(px(8.))
                    .child("Terminal cell grid (mock — Phase 0 render validation)"),
            )
            .children(rows)
    }

    /// Renders a labelled placeholder for panes not yet implemented.
    fn render_placeholder_pane(&self, kind: PaneKind) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .bg(self.theme.bg_base)
            .child(
                div()
                    .text_size(px(28.))
                    .text_color(self.theme.accent)
                    .child(kind.icon()),
            )
            .child(
                div()
                    .text_color(self.theme.fg_text)
                    .text_sm()
                    .child(kind.label()),
            )
            .child(
                div()
                    .text_color(self.theme.fg_muted)
                    .text_xs()
                    .child(kind.phase_note()),
            )
    }

    fn render_status_bar(&self) -> impl IntoElement {
        let stats_text = match (&self.stats, &self.stats_error) {
            (_, Some(e))    => format!("IPC error: {e}"),
            (Some(s), None) => format!("nodes {} · edges {} · unresolved {}", s.node_count, s.edge_count, s.unresolved_count),
            (None, None)    => "loading forge stats…".to_string(),
        };

        div()
            .h(px(22.))
            .px(px(12.))
            .flex()
            .items_center()
            .justify_between()
            .bg(self.theme.status_bar_bg)
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.status_bar_fg)
                    .child("Nexus — kernel booted"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.status_bar_fg)
                    .child(stats_text),
            )
    }
}

// ── Mock grid builder ─────────────────────────────────────────────────────────

fn build_mock_grid() -> Vec<Vec<Cell>> {
    const GRN: u32 = 0x9ece6a;
    const YLW: u32 = 0xe0af68;
    const RED: u32 = 0xf7768e;
    const CYN: u32 = 0x7dcfff;
    const FG:  u32 = 0xc0caf5;
    const ACC: u32 = 0x7aa2f7;
    const BG:  u32 = 0x1a1b26;

    let rows: &[&str] = &[
        "user@nexus:~/forge$ ls notes/",
        "architecture.md   daily/   index.md   projects/",
        "user@nexus:~/forge$ cat notes/index.md",
        "# Nexus Forge                                    ",
        "Welcome to your knowledge base.                  ",
        "user@nexus:~/forge$ _                            ",
    ];
    let palette: &[u32] = &[GRN, FG, GRN, CYN, FG, YLW];

    rows.iter()
        .zip(palette.iter())
        .map(|(line, &fg)| {
            line.chars()
                .map(|ch| {
                    let cell_fg = match ch {
                        '$' => RED,
                        '_' => ACC,
                        '#' => YLW,
                        _   => fg,
                    };
                    Cell::new(ch, cell_fg, BG)
                })
                .collect()
        })
        .collect()
}
