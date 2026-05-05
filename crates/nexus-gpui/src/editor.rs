//! Phase 3: Editor / markdown viewer pane (ADR 0026 Phase 3).
//!
//! [`EditorView`] renders a two-column layout:
//! - Left sidebar (220 px): file tree backed by `com.nexus.storage::list_dir`.
//! - Right panel: markdown viewer (comrak AST) or raw text, loaded via
//!   `com.nexus.storage::read_file`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use comrak::nodes::{AstNode, ListType, NodeValue};
use comrak::{parse_document, Arena, Options};

use gpui::{
    div, hsla, px, AnyElement, AsyncApp, ClickEvent, Context,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window,
};
use serde::Deserialize;

use crate::{theme::Theme, KernelBridge};

const STORAGE_PLUGIN: &str = "com.nexus.storage";

// ── IPC DTOs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TreeEntry {
    name:    String,
    relpath: String,
    is_dir:  bool,
}

#[derive(Deserialize)]
struct ReadFileResult {
    bytes: Vec<u8>,
}

// ── Markdown block model ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)] // `lang` stored for future syntax-highlighting pass
enum MdBlock {
    Heading { level: u8, text: String },
    Para    { text: String },
    Code    { lang: Option<String>, content: String },
    Bullet  { depth: u8, text: String },
    Number  { depth: u8, index: u32, text: String },
    Quote   { text: String },
    Ruler,
}

// ── EditorView ────────────────────────────────────────────────────────────────

pub struct EditorView {
    theme:         Theme,
    bridge:        Arc<KernelBridge>,
    root_entries:  Vec<TreeEntry>,
    sub_entries:   HashMap<String, Vec<TreeEntry>>,
    expanded_dirs: HashSet<String>,
    selected:      Option<String>,
    raw_text:      Option<String>,
    blocks:        Option<Vec<MdBlock>>,
    view_raw:      bool,
    loading:       bool,
}

impl EditorView {
    pub fn new(bridge: Arc<KernelBridge>, cx: &mut Context<Self>) -> Self {
        // Fetch root directory listing on a background thread.
        let br = Arc::clone(&bridge);
        cx.spawn(async move |weak: WeakEntity<EditorView>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    br.call(
                        STORAGE_PLUGIN,
                        "list_dir",
                        serde_json::json!({"relpath": ""}),
                    )
                })
                .await;
            weak.update(cx, |this, cx| {
                if let Ok(v) = result {
                    this.root_entries = serde_json::from_value(v).unwrap_or_default();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        Self {
            theme:         Theme::dark(),
            bridge:        Arc::clone(&bridge),
            root_entries:  Vec::new(),
            sub_entries:   HashMap::new(),
            expanded_dirs: HashSet::new(),
            selected:      None,
            raw_text:      None,
            blocks:        None,
            view_raw:      false,
            loading:       false,
        }
    }

    fn load_file(&mut self, relpath: String, cx: &mut Context<Self>) {
        self.selected = Some(relpath.clone());
        self.loading  = true;
        self.blocks   = None;
        self.raw_text = None;
        cx.notify();

        let br = Arc::clone(&self.bridge);
        cx.spawn(async move |weak: WeakEntity<EditorView>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    br.call(
                        STORAGE_PLUGIN,
                        "read_file",
                        serde_json::json!({"path": relpath}),
                    )
                })
                .await;
            weak.update(cx, |this, cx| {
                this.loading = false;
                if let Ok(v) = result {
                    if let Ok(r) = serde_json::from_value::<ReadFileResult>(v) {
                        let text    = String::from_utf8_lossy(&r.bytes).into_owned();
                        this.blocks   = Some(parse_markdown(&text));
                        this.raw_text = Some(text);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_subdir(&mut self, relpath: String, cx: &mut Context<Self>) {
        if self.sub_entries.contains_key(&relpath) {
            return;
        }
        let br = Arc::clone(&self.bridge);
        let rp = relpath.clone();
        cx.spawn(async move |weak: WeakEntity<EditorView>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    br.call(
                        STORAGE_PLUGIN,
                        "list_dir",
                        serde_json::json!({"relpath": rp}),
                    )
                })
                .await;
            weak.update(cx, |this, cx| {
                if let Ok(v) = result {
                    let entries: Vec<TreeEntry> = serde_json::from_value(v).unwrap_or_default();
                    this.sub_entries.insert(relpath, entries);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

// ── Render ────────────────────────────────────────────────────────────────────

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = self.theme.border;
        div()
            .flex()
            .flex_row()
            .size_full()
            .child(self.render_sidebar(cx))
            .child(div().w(px(1.)).bg(border))
            .child(self.render_content(cx))
    }
}

impl EditorView {
    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let transparent = hsla(0., 0., 0., 0.);

        // Pre-collect the flat tree before cx.listener() closures borrow cx.
        let mut flat: Vec<(u8, TreeEntry, bool, bool)> = Vec::new();
        {
            let root     = &self.root_entries;
            let sub_map  = &self.sub_entries;
            let expanded = &self.expanded_dirs;
            let selected = &self.selected;
            collect_flat_tree(root, sub_map, expanded, selected, 0, &mut flat);
        }

        let items: Vec<AnyElement> = flat
            .into_iter()
            .map(|(depth, entry, is_expanded, is_selected)| {
                let is_dir  = entry.is_dir;
                let relpath = entry.relpath.clone();
                let name    = entry.name.clone();

                let prefix = if is_dir {
                    if is_expanded { "▾ " } else { "▸ " }
                } else {
                    "  "
                };

                let bg = if is_selected { theme.bg_elevated } else { transparent };
                let fg = if is_selected { theme.accent      } else { theme.fg_text };

                div()
                    .id(SharedString::from(format!("tree-{relpath}")))
                    .pl(px(6. + depth as f32 * 14.))
                    .py(px(2.))
                    .w_full()
                    .bg(bg)
                    .text_color(fg)
                    .text_xs()
                    .cursor_pointer()
                    .child(format!("{prefix}{name}"))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        if is_dir {
                            if this.expanded_dirs.contains(&relpath) {
                                this.expanded_dirs.remove(&relpath);
                            } else {
                                this.expanded_dirs.insert(relpath.clone());
                                this.load_subdir(relpath.clone(), cx);
                            }
                        } else {
                            this.load_file(relpath.clone(), cx);
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();

        div()
            .id("editor-sidebar")
            .w(px(220.))
            .h_full()
            .overflow_y_scroll()
            .bg(theme.bg_panel)
            .flex()
            .flex_col()
            .children(items)
    }

    fn render_content(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme      = self.theme;
        let loading    = self.loading;
        let view_raw   = self.view_raw;
        let has_file   = self.raw_text.is_some();
        let path_label = self.selected.clone().unwrap_or_default();

        let body: AnyElement = if loading {
            div()
                .text_xs()
                .text_color(theme.fg_muted)
                .child("Loading…")
                .into_any_element()
        } else if view_raw {
            let text = self.raw_text.clone().unwrap_or_default();
            div()
                .font_family("monospace")
                .text_xs()
                .text_color(theme.fg_text)
                .child(text)
                .into_any_element()
        } else if let Some(blocks) = &self.blocks {
            let block_els: Vec<AnyElement> = blocks
                .iter()
                .map(|b| render_md_block(b, theme))
                .collect();
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(block_els)
                .into_any_element()
        } else {
            div()
                .text_xs()
                .text_color(theme.fg_muted)
                .child("Select a file from the tree")
                .into_any_element()
        };

        let toggle_btn: AnyElement = if has_file {
            div()
                .id("toggle-raw")
                .px(px(8.))
                .py(px(2.))
                .text_xs()
                .text_color(if view_raw { theme.accent } else { theme.fg_muted })
                .cursor_pointer()
                .child(if view_raw { "Rendered" } else { "Raw" })
                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.view_raw = !this.view_raw;
                    cx.notify();
                }))
                .into_any_element()
        } else {
            div().into_any_element()
        };

        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.))
                    .px(px(12.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .bg(theme.bg_elevated)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.fg_muted)
                            .child(path_label),
                    )
                    .child(toggle_btn),
            )
            .child(
                div()
                    .id("editor-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .p(px(16.))
                    .child(body),
            )
    }
}

// ── Markdown block rendering ──────────────────────────────────────────────────

fn render_md_block(block: &MdBlock, theme: Theme) -> AnyElement {
    match block {
        MdBlock::Heading { level, text } => {
            let size = match level {
                1 => px(22.),
                2 => px(18.),
                3 => px(15.),
                _ => px(13.),
            };
            div()
                .mt(px(12.))
                .mb(px(4.))
                .text_size(size)
                .text_color(theme.fg_text)
                .child(text.clone())
                .into_any_element()
        }
        MdBlock::Para { text } => div()
            .text_xs()
            .text_color(theme.fg_text)
            .mb(px(6.))
            .child(text.clone())
            .into_any_element(),
        MdBlock::Code { content, .. } => div()
            .font_family("monospace")
            .text_xs()
            .text_color(theme.fg_text)
            .bg(theme.bg_elevated)
            .p(px(8.))
            .mb(px(8.))
            .child(content.clone())
            .into_any_element(),
        MdBlock::Bullet { depth, text } => div()
            .pl(px(8. + *depth as f32 * 16.))
            .text_xs()
            .text_color(theme.fg_text)
            .child(format!("• {text}"))
            .into_any_element(),
        MdBlock::Number { depth, index, text } => div()
            .pl(px(8. + *depth as f32 * 16.))
            .text_xs()
            .text_color(theme.fg_text)
            .child(format!("{index}. {text}"))
            .into_any_element(),
        MdBlock::Quote { text } => div()
            .pl(px(12.))
            .border_l_2()
            .border_color(theme.accent)
            .text_xs()
            .text_color(theme.fg_muted)
            .mb(px(6.))
            .child(text.clone())
            .into_any_element(),
        MdBlock::Ruler => div()
            .w_full()
            .h(px(1.))
            .bg(theme.border)
            .my(px(8.))
            .into_any_element(),
    }
}

// ── Flat tree builder ─────────────────────────────────────────────────────────

fn collect_flat_tree(
    entries:  &[TreeEntry],
    sub_map:  &HashMap<String, Vec<TreeEntry>>,
    expanded: &HashSet<String>,
    selected: &Option<String>,
    depth:    u8,
    out:      &mut Vec<(u8, TreeEntry, bool, bool)>,
) {
    for entry in entries {
        let is_expanded = expanded.contains(&entry.relpath);
        let is_selected = selected.as_deref() == Some(&entry.relpath);
        out.push((depth, entry.clone(), is_expanded, is_selected));
        if is_expanded && depth < 8 {
            if let Some(sub) = sub_map.get(&entry.relpath) {
                collect_flat_tree(sub, sub_map, expanded, selected, depth + 1, out);
            }
        }
    }
}

// ── Markdown → MdBlock ───────────────────────────────────────────────────────

fn parse_markdown(text: &str) -> Vec<MdBlock> {
    let arena = Arena::new();
    let opts  = Options::default();
    let root  = parse_document(&arena, text, &opts);
    let mut blocks = Vec::new();
    for child in root.children() {
        visit_md_node(child, 0, &mut blocks);
    }
    blocks
}

fn visit_md_node<'a>(node: &'a AstNode<'a>, depth: u8, out: &mut Vec<MdBlock>) {
    let value = node.data.borrow().value.clone();
    match value {
        NodeValue::Heading(h) => {
            out.push(MdBlock::Heading { level: h.level, text: flatten_text(node) });
        }
        NodeValue::Paragraph => {
            let text = flatten_text(node);
            if !text.trim().is_empty() {
                out.push(MdBlock::Para { text });
            }
        }
        NodeValue::CodeBlock(cb) => {
            let lang = if cb.info.is_empty() { None } else { Some(cb.info.clone()) };
            out.push(MdBlock::Code {
                lang,
                content: cb.literal.trim_end_matches('\n').to_string(),
            });
        }
        NodeValue::ThematicBreak => out.push(MdBlock::Ruler),
        NodeValue::List(list) => {
            let ordered = list.list_type == ListType::Ordered;
            let start   = list.start;
            for (i, item) in node.children().enumerate() {
                let index = (start + i) as u32;
                let text  = flatten_item_first_para(item);
                if ordered {
                    out.push(MdBlock::Number { depth, index, text });
                } else {
                    out.push(MdBlock::Bullet { depth, text });
                }
                // Recurse into nested lists within the item.
                for child in item.children() {
                    if matches!(child.data.borrow().value, NodeValue::List(_)) {
                        visit_md_node(child, depth.saturating_add(1), out);
                    }
                }
            }
        }
        NodeValue::BlockQuote => {
            out.push(MdBlock::Quote { text: flatten_text(node) });
        }
        _ => {}
    }
}

fn flatten_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut buf = String::new();
    flatten_into(node, &mut buf);
    buf
}

fn flatten_item_first_para<'a>(item: &'a AstNode<'a>) -> String {
    let mut buf = String::new();
    for child in item.children() {
        if matches!(child.data.borrow().value, NodeValue::Paragraph) {
            flatten_into(child, &mut buf);
            break;
        }
    }
    buf
}

fn flatten_into<'a>(node: &'a AstNode<'a>, buf: &mut String) {
    match node.data.borrow().value.clone() {
        NodeValue::Text(s)                          => buf.push_str(&s),
        NodeValue::SoftBreak | NodeValue::LineBreak => buf.push(' '),
        NodeValue::Code(c) => {
            buf.push('`');
            buf.push_str(&c.literal);
            buf.push('`');
        }
        _ => {
            for child in node.children() {
                flatten_into(child, buf);
            }
        }
    }
}

