//! Phase 4: AI chat panel (ADR 0026 Phase 4).
//!
//! [`AiView`] renders a two-row layout:
//! - Top: scrollable chat history (user/assistant bubbles).
//! - Bottom: focusable text-input bar; Enter submits, Shift+Enter is a
//!   literal newline (not yet supported — sends as-is).
//!
//! Calls `com.nexus.ai::stream_chat` in `mode:"complete"` + `tools:"none"` so
//! the call is a single provider round-trip with no tool dispatch.  The full
//! response arrives when the IPC call returns; streaming events are emitted
//! on the bus but not consumed here (Phase 5 can add that).

use std::sync::Arc;

use gpui::{
    div, px, AnyElement, AsyncApp, ClickEvent, Context, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render,
    StatefulInteractiveElement, Styled, WeakEntity, Window,
};

use crate::{theme::Theme, KernelBridge};

const AI_PLUGIN: &str = "com.nexus.ai";

// ── Message model ─────────────────────────────────────────────────────────────

#[derive(Clone)]
enum Role { User, Assistant }

#[derive(Clone)]
struct Msg {
    role:    Role,
    content: String,
}

// ── AiView ────────────────────────────────────────────────────────────────────

pub struct AiView {
    theme:       Theme,
    bridge:      Arc<KernelBridge>,
    messages:    Vec<Msg>,
    input:       String,
    loading:     bool,
    error:       Option<String>,
    input_focus: FocusHandle,
}

impl AiView {
    pub fn new(bridge: Arc<KernelBridge>, cx: &mut Context<Self>) -> Self {
        Self {
            theme:       Theme::dark(),
            bridge:      Arc::clone(&bridge),
            messages:    Vec::new(),
            input:       String::new(),
            loading:     false,
            error:       None,
            input_focus: cx.focus_handle(),
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() || self.loading {
            return;
        }
        self.messages.push(Msg { role: Role::User, content: text.clone() });
        self.input.clear();
        self.loading = true;
        self.error   = None;
        cx.notify();

        // Build message history for the IPC call.
        let wire_msgs: Vec<serde_json::Value> = self
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                    "content": m.content
                })
            })
            .collect();

        let br = Arc::clone(&self.bridge);
        cx.spawn(async move |weak: WeakEntity<AiView>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    br.call(
                        AI_PLUGIN,
                        "stream_chat",
                        serde_json::json!({
                            "messages": wire_msgs,
                            "mode":     "complete",
                            "tools":    "none"
                        }),
                    )
                })
                .await;

            weak.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(v) => {
                        let reply = v
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("(empty response)")
                            .to_string();
                        this.messages.push(Msg { role: Role::Assistant, content: reply });
                    }
                    Err(e) => {
                        this.error = Some(format!("{e}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

// ── Render ────────────────────────────────────────────────────────────────────

impl Render for AiView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.input_focus.is_focused(window);
        let theme   = self.theme;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg_base)
            .child(self.render_history(cx))
            .child(self.render_input_bar(focused, cx))
    }
}

impl AiView {
    fn render_history(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;

        let bubbles: Vec<AnyElement> = self
            .messages
            .iter()
            .map(|m| render_bubble(m, theme))
            .collect();

        // Loading indicator appended when waiting for a response.
        let mut children: Vec<AnyElement> = bubbles;
        if self.loading {
            children.push(
                div()
                    .px(px(12.))
                    .py(px(6.))
                    .text_xs()
                    .text_color(theme.fg_muted)
                    .child("Thinking…")
                    .into_any_element(),
            );
        }
        if let Some(ref err) = self.error {
            children.push(
                div()
                    .px(px(12.))
                    .py(px(4.))
                    .text_xs()
                    .text_color(theme.danger)
                    .child(err.clone())
                    .into_any_element(),
            );
        }
        if self.messages.is_empty() && !self.loading {
            children.push(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(theme.fg_muted)
                    .child("Ask the AI anything")
                    .into_any_element(),
            );
        }

        div()
            .id("ai-history")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(8.))
            .children(children)
    }

    fn render_input_bar(&mut self, focused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let theme        = self.theme;
        let border_color = if focused { theme.accent } else { theme.border };
        let placeholder  = if self.input.is_empty() {
            "Ask something… (Enter to send)".to_string()
        } else {
            format!("{}▋", self.input)
        };
        let text_color   = if self.input.is_empty() { theme.fg_muted } else { theme.fg_text };

        div()
            .border_t_1()
            .border_color(theme.border)
            .p(px(8.))
            .child(
                div()
                    .id("ai-input")
                    .w_full()
                    .min_h(px(32.))
                    .p(px(8.))
                    .bg(theme.bg_elevated)
                    .border_1()
                    .border_color(border_color)
                    .text_xs()
                    .text_color(text_color)
                    .cursor_text()
                    .track_focus(&self.input_focus)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        window.focus(&this.input_focus, cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                        let k = &event.keystroke;
                        if k.modifiers.platform { return; }
                        match k.key.as_str() {
                            "enter" if !k.modifiers.shift => { this.submit(cx); }
                            "backspace" => {
                                this.input.pop();
                                cx.notify();
                            }
                            "escape" => {
                                this.input.clear();
                                cx.notify();
                            }
                            _ => {
                                if let Some(ref c) = k.key_char {
                                    if !k.modifiers.control {
                                        this.input.push_str(c);
                                        cx.notify();
                                    }
                                }
                            }
                        }
                    }))
                    .child(placeholder),
            )
    }
}

// ── Chat bubble renderer ──────────────────────────────────────────────────────

fn render_bubble(msg: &Msg, theme: Theme) -> AnyElement {
    let is_user  = matches!(msg.role, Role::User);
    let bg       = if is_user { theme.bg_elevated } else { theme.bg_panel };
    let fg       = if is_user { theme.fg_text }     else { theme.fg_text };
    let label    = if is_user { "you" }             else { "ai" };
    let label_fg = if is_user { theme.accent }      else { theme.success };

    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(12.))
        .py(px(6.))
        .bg(bg)
        .child(
            div()
                .text_xs()
                .text_color(label_fg)
                .child(label),
        )
        .child(
            div()
                .text_xs()
                .text_color(fg)
                .child(msg.content.clone()),
        )
        .into_any_element()
}
