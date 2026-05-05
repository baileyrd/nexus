//! Phase 2 shell: live terminal pane backed by `nexus-terminal` IPC.
//!
//! Layout:
//! ```text
//! ┌─ TopBar (32px) ────────────────────────────────────────────────┐
//! │ ActivityBar │ Primary Pane            │ Secondary Pane          │
//! │   (48px)    │  (flex: 1)              │  (flex: 1)              │
//! └─ StatusBar (22px) ─────────────────────────────────────────────┘
//! ```

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, hsla, px, AnyElement, AppContext, AsyncApp, ClickEvent, Context, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render,
    StatefulInteractiveElement, Styled, WeakEntity, Window,
};
use serde::Deserialize;

use nexus_terminal::term_grid::{CellColor, ScreenRow};
use nexus_terminal::{CreateSessionResponse, PLUGIN_ID};

use crate::{
    ai::AiView,
    editor::EditorView,
    graph::GraphView,
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

// ── WorkbenchView ─────────────────────────────────────────────────────────────

pub struct WorkbenchView {
    theme:       Theme,
    layout:      SplitLayout,
    stats:       Option<GraphStats>,
    stats_error: Option<String>,
    /// Shared IPC bridge — cloned into event listeners for key input dispatch.
    bridge:      Arc<KernelBridge>,
    /// Focus handle for the terminal pane. Key events route here when focused.
    term_focus:  FocusHandle,
    /// Active PTY session ID, set once `create_session` returns.
    session_id:  Option<String>,
    /// Latest rendered screen from `read_screen`, updated every ~50 ms.
    screen:      Option<Vec<ScreenRow>>,
    /// Phase 3 editor / markdown pane entity.
    editor:      Entity<EditorView>,
    /// Phase 4 AI chat panel entity.
    ai:          Entity<AiView>,
    /// Phase 4 knowledge-graph canvas entity.
    graph:       Entity<GraphView>,
}

impl WorkbenchView {
    pub fn new(bridge: Arc<KernelBridge>, cx: &mut Context<Self>) -> Self {
        // ── Graph stats fetch (status bar) ────────────────────────────────
        {
            let br = Arc::clone(&bridge);
            cx.spawn(async move |weak: WeakEntity<WorkbenchView>, cx: &mut AsyncApp| {
                let result = cx
                    .background_executor()
                    .spawn(async move { br.call_empty("com.nexus.storage", "graph_stats") })
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
        }

        // ── Terminal bootstrap + pump loop ────────────────────────────────
        let br = Arc::clone(&bridge);
        cx.spawn(async move |weak: WeakEntity<WorkbenchView>, cx: &mut AsyncApp| {
            // Create the PTY session on a background thread.
            let br_c = Arc::clone(&br);
            let create_result = cx
                .background_executor()
                .spawn(async move {
                    br_c.call(
                        PLUGIN_ID,
                        "create_session",
                        serde_json::json!({"name": "main"}),
                    )
                })
                .await;

            let id = match create_result {
                Ok(v) => match serde_json::from_value::<CreateSessionResponse>(v) {
                    Ok(r) => r.id,
                    Err(e) => {
                        tracing::error!("create_session decode failed: {e}");
                        return;
                    }
                },
                Err(e) => {
                    tracing::error!("create_session IPC failed: {e}");
                    return;
                }
            };

            // Store session ID so the UI can show it and key input can use it.
            weak.update(cx, |this, cx| {
                this.session_id = Some(id.clone());
                cx.notify();
            })
            .ok();

            // ~50 ms pump loop: drain PTY → read_screen → notify view.
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;

                let br2 = Arc::clone(&br);
                let id2 = id.clone();
                let screen_result = cx
                    .background_executor()
                    .spawn(async move {
                        // Drain PTY bytes into the session buffers + TermGrid.
                        let _ = br2.call(
                            PLUGIN_ID,
                            "pump",
                            serde_json::json!({"id": id2, "timeout_ms": 10}),
                        );
                        // Return the freshly-updated screen.
                        br2.call(
                            PLUGIN_ID,
                            "read_screen",
                            serde_json::json!({"id": id2}),
                        )
                    })
                    .await;

                weak.update(cx, |this, cx| {
                    if let Ok(v) = screen_result {
                        this.screen = serde_json::from_value(v).ok();
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();

        let editor = cx.new(|cx| EditorView::new(Arc::clone(&bridge), cx));
        let ai     = cx.new(|cx| AiView::new(Arc::clone(&bridge), cx));
        let graph  = cx.new(|cx| GraphView::new(Arc::clone(&bridge), cx));

        Self {
            theme:       Theme::dark(),
            layout:      SplitLayout::default(),
            stats:       None,
            stats_error: None,
            bridge:      Arc::clone(&bridge),
            term_focus:  cx.focus_handle(),
            session_id:  None,
            screen:      None,
            editor,
            ai,
            graph,
        }
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top_bar    = self.render_top_bar();
        let act_bar    = self.render_activity_bar(cx);
        let content    = self.render_content_area(cx);
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
                    .child("Nexus  ·  Phase 4 — Terminal + Editor + AI + Graph"),
            )
            .child(
                div()
                    .text_color(self.theme.fg_muted)
                    .text_xs()
                    .child("gpui"),
            )
    }

    fn render_activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active   = self.layout.primary;
        let accent   = self.theme.accent;
        let border   = self.theme.border;
        let panel    = self.theme.bg_panel;
        let elevated = self.theme.bg_elevated;
        let fg_muted = self.theme.fg_muted;
        let transparent = hsla(0., 0., 0., 0.);

        let items: Vec<AnyElement> = ACTIVITY_ENTRIES
            .iter()
            .map(|&(kind, id)| {
                let is_active   = kind == active;
                let item_bg     = if is_active { panel }       else { transparent };
                let item_fg     = if is_active { accent }      else { fg_muted };
                let left_border = if is_active { accent }      else { transparent };

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

    fn render_content_area(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let primary_pane   = self.render_pane(self.layout.primary, cx);
        let secondary_kind = self.layout.secondary;
        let border         = self.theme.border;

        let mut row = div().flex().flex_row().flex_1();
        row = row.child(div().flex_1().child(primary_pane));

        if let Some(kind) = secondary_kind {
            let secondary_pane = self.render_pane(kind, cx);
            row = row
                .child(div().w(px(1.)).bg(border))
                .child(div().flex_1().child(secondary_pane));
        }

        row
    }

    fn render_pane(&self, kind: PaneKind, cx: &mut Context<Self>) -> AnyElement {
        match kind {
            PaneKind::Terminal => self.render_terminal_pane(cx).into_any_element(),
            PaneKind::Editor   => self.editor.clone().into_any_element(),
            PaneKind::Ai       => self.ai.clone().into_any_element(),
            PaneKind::Graph    => self.graph.clone().into_any_element(),
            other              => self.render_placeholder_pane(other).into_any_element(),
        }
    }

    /// Renders the live terminal grid backed by `alacritty_terminal::Term`.
    fn render_terminal_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cell_w = px(9.);
        let cell_h = px(18.);
        let theme  = self.theme;

        let rows: Vec<AnyElement> = match &self.screen {
            Some(screen) => screen
                .iter()
                .map(|row| {
                    let cols: Vec<AnyElement> = row
                        .cells
                        .iter()
                        .map(|cell| {
                            let fg = cell_color_to_rgba(&cell.fg, false, &theme);
                            let bg = cell_color_to_rgba(&cell.bg, true,  &theme);
                            div()
                                .w(cell_w)
                                .h(cell_h)
                                .bg(bg)
                                .text_color(fg)
                                .text_xs()
                                .child(cell.text.clone())
                                .into_any_element()
                        })
                        .collect();
                    div().flex().flex_row().children(cols).into_any_element()
                })
                .collect(),
            None => vec![div()
                .text_xs()
                .text_color(theme.fg_muted)
                .child("Connecting to shell…")
                .into_any_element()],
        };

        div()
            .id("terminal-pane")
            .size_full()
            .p(px(4.))
            .flex()
            .flex_col()
            .bg(theme.bg_base)
            .track_focus(&self.term_focus)
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                window.focus(&this.term_focus, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, _cx| {
                let Some(ref id) = this.session_id else { return };
                let Some(input) = keystroke_to_input(&event.keystroke) else { return };
                let bridge = Arc::clone(&this.bridge);
                let id = id.clone();
                // fire-and-forget: key input dispatch is latency-sensitive
                // but doesn't need to block the render thread.
                std::thread::spawn(move || {
                    let _ = bridge.call(
                        PLUGIN_ID,
                        "send_input",
                        serde_json::json!({"id": id, "text": input}),
                    );
                });
            }))
            .children(rows)
    }

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
            (Some(s), None) => format!(
                "nodes {} · edges {} · unresolved {}",
                s.node_count, s.edge_count, s.unresolved_count
            ),
            (None, None) => "loading forge stats…".to_string(),
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

// ── Colour mapping ────────────────────────────────────────────────────────────

/// xterm-16 default ANSI colour palette (Tokyo Night-ish dark theme).
#[rustfmt::skip]
const ANSI16: [u32; 16] = [
    0x1a1b26, // 0  Black      (bg)
    0xf7768e, // 1  Red
    0x9ece6a, // 2  Green
    0xe0af68, // 3  Yellow
    0x7aa2f7, // 4  Blue
    0xbb9af7, // 5  Magenta
    0x7dcfff, // 6  Cyan
    0xa9b1d6, // 7  White
    0x414868, // 8  BrightBlack  (dark grey)
    0xff7a93, // 9  BrightRed
    0xb9f27c, // 10 BrightGreen
    0xff9e64, // 11 BrightYellow
    0x7dcfff, // 12 BrightBlue
    0xdb4b4b, // 13 BrightMagenta
    0x89ddff, // 14 BrightCyan
    0xc0caf5, // 15 BrightWhite
];

/// Map an `indexed` xterm-256 colour number to an 0xRRGGBB value.
fn indexed_to_rgb(i: u8) -> u32 {
    if i < 16 {
        return ANSI16[i as usize];
    }
    if i >= 232 {
        // 24-step greyscale ramp: 232 → #080808, 255 → #eeeeee
        let v = 8 + 10 * (i - 232) as u32;
        return (v << 16) | (v << 8) | v;
    }
    // 6×6×6 colour cube: indices 16–231
    let i = i as u32 - 16;
    let b_idx = i % 6;
    let g_idx = (i / 6) % 6;
    let r_idx = i / 36;
    let to_level = |n: u32| if n == 0 { 0 } else { 55 + 40 * n };
    (to_level(r_idx) << 16) | (to_level(g_idx) << 8) | to_level(b_idx)
}

/// Convert a `CellColor` to a `gpui::Rgba`.
fn cell_color_to_rgba(c: &CellColor, is_bg: bool, _theme: &Theme) -> gpui::Rgba {
    match c {
        CellColor::Default => {
            if is_bg { gpui::rgb(0x1a1b26) } else { gpui::rgb(0xc0caf5) }
        }
        CellColor::Named(n) => {
            let idx = *n as usize;
            gpui::rgb(if idx < 16 { ANSI16[idx] } else { 0xc0caf5 })
        }
        CellColor::Indexed(i) => gpui::rgb(indexed_to_rgb(*i)),
        CellColor::Rgb(r, g, b) => {
            gpui::rgb(((*r as u32) << 16) | ((*g as u32) << 8) | (*b as u32))
        }
    }
}

// ── Keystroke → terminal byte sequence ───────────────────────────────────────

/// Convert a gpui keystroke to the byte string the terminal should receive.
/// Returns `None` for keystrokes the terminal should ignore (e.g. pure modifier
/// presses, platform shortcuts).
fn keystroke_to_input(k: &gpui::Keystroke) -> Option<String> {
    // Platform-shortcut (Cmd/Super/Win) — never send to terminal.
    if k.modifiers.platform {
        return None;
    }

    // Ctrl+letter → control character (^A = 0x01 … ^Z = 0x1a).
    if k.modifiers.control && !k.modifiers.alt {
        let byte: Option<u8> = match k.key.as_str() {
            "a" => Some(1),  "b" => Some(2),  "c" => Some(3),
            "d" => Some(4),  "e" => Some(5),  "f" => Some(6),
            "g" => Some(7),  "h" => Some(8),  "i" => Some(9),
            "j" => Some(10), "k" => Some(11), "l" => Some(12),
            "m" => Some(13), "n" => Some(14), "o" => Some(15),
            "p" => Some(16), "q" => Some(17), "r" => Some(18),
            "s" => Some(19), "t" => Some(20), "u" => Some(21),
            "v" => Some(22), "w" => Some(23), "x" => Some(24),
            "y" => Some(25), "z" => Some(26),
            "[" => Some(27), // ESC
            "\\"=> Some(28), "]" => Some(29), "^" => Some(30),
            "_" => Some(31), "space" => Some(0),
            _ => None,
        };
        if let Some(b) = byte {
            return Some(String::from(char::from(b)));
        }
    }

    // Named / special keys → ANSI escape sequences.
    let seq = match k.key.as_str() {
        "enter"    => "\r",
        "backspace"=> "\x7f",
        "escape"   => "\x1b",
        "tab"      => "\t",
        "up"       => "\x1b[A",
        "down"     => "\x1b[B",
        "right"    => "\x1b[C",
        "left"     => "\x1b[D",
        "home"     => "\x1b[H",
        "end"      => "\x1b[F",
        "pageup"   => "\x1b[5~",
        "pagedown" => "\x1b[6~",
        "delete"   => "\x1b[3~",
        "insert"   => "\x1b[2~",
        "f1"  => "\x1bOP",   "f2"  => "\x1bOQ",
        "f3"  => "\x1bOR",   "f4"  => "\x1bOS",
        "f5"  => "\x1b[15~", "f6"  => "\x1b[17~",
        "f7"  => "\x1b[18~", "f8"  => "\x1b[19~",
        "f9"  => "\x1b[20~", "f10" => "\x1b[21~",
        "f11" => "\x1b[23~", "f12" => "\x1b[24~",
        _ => "",
    };
    if !seq.is_empty() {
        return Some(seq.to_string());
    }

    // Printable character: prefer key_char (handles shift, AltGr, IME).
    if let Some(ref kc) = k.key_char {
        if !k.modifiers.control {
            return Some(kc.clone());
        }
    }

    // Single-character key without modifiers.
    if k.key.len() == 1 && !k.modifiers.control && !k.modifiers.alt {
        return Some(k.key.clone());
    }

    None
}
