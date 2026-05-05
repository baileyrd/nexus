//! Phase 4: Knowledge-graph canvas (ADR 0026 Phase 4).
//!
//! [`GraphView`] loads the full graph snapshot via
//! `com.nexus.storage::list_all_links`, runs a simplified
//! Fruchterman-Reingold force layout, and renders nodes as
//! absolutely-positioned circles within a fixed-size canvas.
//! Edges are intentionally omitted — they'd require canvas drawing
//! or CSS transforms that add significant complexity; the node
//! scatter is enough to prove the pane works end-to-end.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    div, px, AnyElement, AsyncApp, Context, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window,
};
use serde::Deserialize;

use crate::{theme::Theme, KernelBridge};

const STORAGE_PLUGIN: &str = "com.nexus.storage";
const CANVAS_W: f32 = 760.;
const CANVAS_H: f32 = 560.;

// ── IPC DTOs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GraphSnapshot {
    nodes: Vec<GraphNodeEntry>,
    edges: Vec<GraphEdgeEntry>,
}

#[derive(Debug, Deserialize)]
struct GraphNodeEntry {
    path:       String,
    is_phantom: bool,
}

#[derive(Debug, Deserialize)]
struct GraphEdgeEntry {
    source: String,
    target: String,
}

// ── Force-layout node ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct LayoutNode {
    path:       String,
    is_phantom: bool,
    x:          f32,
    y:          f32,
}

// ── GraphView ─────────────────────────────────────────────────────────────────

pub struct GraphView {
    theme:         Theme,
    #[allow(dead_code)] // held for future refresh / re-layout
    bridge:        Arc<KernelBridge>,
    nodes:         Vec<LayoutNode>,
    loading:       bool,
    error:         Option<String>,
}

impl GraphView {
    pub fn new(bridge: Arc<KernelBridge>, cx: &mut Context<Self>) -> Self {
        let br = Arc::clone(&bridge);
        cx.spawn(async move |weak: WeakEntity<GraphView>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    br.call(
                        STORAGE_PLUGIN,
                        "list_all_links",
                        serde_json::json!({}),
                    )
                })
                .await;

            weak.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(v) => match serde_json::from_value::<GraphSnapshot>(v) {
                        Ok(snap) => this.nodes = compute_layout(&snap),
                        Err(e)   => this.error = Some(format!("decode: {e}")),
                    },
                    Err(e) => this.error = Some(format!("{e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        Self {
            theme:   Theme::dark(),
            bridge:  Arc::clone(&bridge),
            nodes:   Vec::new(),
            loading: true,
            error:   None,
        }
    }
}

// ── Render ────────────────────────────────────────────────────────────────────

impl Render for GraphView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;

        let body: AnyElement = if self.loading {
            div()
                .text_xs()
                .text_color(theme.fg_muted)
                .child("Loading graph…")
                .into_any_element()
        } else if let Some(ref e) = self.error {
            div()
                .text_xs()
                .text_color(theme.danger)
                .child(e.clone())
                .into_any_element()
        } else if self.nodes.is_empty() {
            div()
                .text_xs()
                .text_color(theme.fg_muted)
                .child("No nodes in knowledge graph yet")
                .into_any_element()
        } else {
            let node_count = self.nodes.len();
            let node_els: Vec<AnyElement> = self
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| render_node(i, n, theme))
                .collect();

            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.fg_muted)
                        .child(format!("{node_count} nodes")),
                )
                .child(
                    // Canvas: relative container for absolutely-positioned nodes.
                    div()
                        .id("graph-canvas")
                        .relative()
                        .w(px(CANVAS_W))
                        .h(px(CANVAS_H))
                        .bg(theme.bg_elevated)
                        .overflow_hidden()
                        .children(node_els),
                )
                .into_any_element()
        };

        div()
            .id("graph-scroll")
            .size_full()
            .overflow_y_scroll()
            .p(px(16.))
            .bg(theme.bg_base)
            .child(body)
    }
}

// ── Node element ──────────────────────────────────────────────────────────────

fn render_node(idx: usize, node: &LayoutNode, theme: Theme) -> AnyElement {
    let dot_r = if node.is_phantom { 4. } else { 6. };
    let color = if node.is_phantom { theme.fg_muted } else { theme.accent };

    // Trim path to just the filename for the tooltip-style label.
    let label = node
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&node.path)
        .to_string();

    div()
        .id(SharedString::from(format!("gnode-{idx}")))
        .absolute()
        // Centre the dot on the computed position.
        .left(px(node.x - dot_r))
        .top(px(node.y - dot_r))
        .w(px(dot_r * 2.))
        .h(px(dot_r * 2.))
        .rounded_full()
        .bg(color)
        .cursor_pointer()
        // Inline label to the right of the dot.
        .child(
            div()
                .absolute()
                .left(px(dot_r * 2. + 3.))
                .top(px(-2.))
                .text_size(px(9.))
                .text_color(theme.fg_muted)
                .overflow_hidden()
                .w(px(80.))
                .child(label),
        )
        .into_any_element()
}

// ── Force layout (Fruchterman-Reingold, simplified) ───────────────────────────

fn compute_layout(snap: &GraphSnapshot) -> Vec<LayoutNode> {
    let n = snap.nodes.len();
    if n == 0 {
        return Vec::new();
    }

    // Build index map path → usize.
    let idx: HashMap<&str, usize> = snap
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.path.as_str(), i))
        .collect();

    // Edge list as index pairs.
    let edges: Vec<(usize, usize)> = snap
        .edges
        .iter()
        .filter_map(|e| {
            let a = idx.get(e.source.as_str())?;
            let b = idx.get(e.target.as_str())?;
            Some((*a, *b))
        })
        .collect();

    // Initial positions: concentric rings.
    let margin = 40.;
    let cx_c = CANVAS_W / 2.;
    let cy_c = CANVAS_H / 2.;
    let max_r = (CANVAS_W.min(CANVAS_H) / 2. - margin).max(1.);
    let mut px_arr: Vec<f32> = (0..n)
        .map(|i| {
            let angle = 2. * std::f32::consts::PI * i as f32 / n as f32;
            cx_c + max_r * angle.cos()
        })
        .collect();
    let mut py_arr: Vec<f32> = (0..n)
        .map(|i| {
            let angle = 2. * std::f32::consts::PI * i as f32 / n as f32;
            cy_c + max_r * angle.sin()
        })
        .collect();

    // FR ideal spring length: area = canvas, k = sqrt(area / n).
    let k = ((CANVAS_W * CANVAS_H) / n.max(1) as f32).sqrt();
    let iterations = 80_usize.min(n * 3 + 30);

    for iter in 0..iterations {
        let temp = max_r * (1. - iter as f32 / iterations as f32);
        let mut fx = vec![0_f32; n];
        let mut fy = vec![0_f32; n];

        // Repulsion: all pairs.
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = px_arr[i] - px_arr[j];
                let dy = py_arr[i] - py_arr[j];
                let dist = (dx * dx + dy * dy).sqrt().max(0.01);
                let f = k * k / dist;
                fx[i] += f * dx / dist;
                fy[i] += f * dy / dist;
                fx[j] -= f * dx / dist;
                fy[j] -= f * dy / dist;
            }
        }

        // Attraction: edges.
        for &(a, b) in &edges {
            let dx = px_arr[b] - px_arr[a];
            let dy = py_arr[b] - py_arr[a];
            let dist = (dx * dx + dy * dy).sqrt().max(0.01);
            let f = dist * dist / k;
            fx[a] += f * dx / dist;
            fy[a] += f * dy / dist;
            fx[b] -= f * dx / dist;
            fy[b] -= f * dy / dist;
        }

        // Apply with cooling.
        for i in 0..n {
            let fmag = (fx[i] * fx[i] + fy[i] * fy[i]).sqrt().max(0.01);
            let scale = fmag.min(temp) / fmag;
            px_arr[i] = (px_arr[i] + fx[i] * scale).clamp(margin, CANVAS_W - margin);
            py_arr[i] = (py_arr[i] + fy[i] * scale).clamp(margin, CANVAS_H - margin);
        }
    }

    snap.nodes
        .iter()
        .enumerate()
        .map(|(i, node)| LayoutNode {
            path:       node.path.clone(),
            is_phantom: node.is_phantom,
            x:          px_arr[i],
            y:          py_arr[i],
        })
        .collect()
}
