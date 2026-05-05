/// Identifies which logical pane occupies a slot in the layout.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    Terminal,
    Editor,
    Ai,
    Graph,
    Settings,
}

impl PaneKind {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Terminal => ">_",
            Self::Editor   => "≡",
            Self::Ai       => "✦",
            Self::Graph    => "◎",
            Self::Settings => "⚙",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Editor   => "Editor",
            Self::Ai       => "AI Assistant",
            Self::Graph    => "Knowledge Graph",
            Self::Settings => "Settings",
        }
    }

    pub fn phase_note(self) -> &'static str {
        match self {
            Self::Terminal => "Phase 2 — alacritty-terminal + cell renderer",
            Self::Editor   => "Phase 3 — pulldown-cmark + file tree",
            Self::Ai       => "Phase 4 — stream_chat, multi-turn history",
            Self::Graph    => "Phase 4 — FR force layout, node canvas",
            Self::Settings => "Phase 5 — settings UI",
        }
    }
}

/// Describes the active split in the content area.
pub struct SplitLayout {
    /// The primary (left/top) pane.
    pub primary: PaneKind,
    /// Optional secondary (right/bottom) pane; `None` = single-pane view.
    pub secondary: Option<PaneKind>,
}

impl Default for SplitLayout {
    fn default() -> Self {
        Self {
            primary:   PaneKind::Terminal,
            secondary: Some(PaneKind::Ai),
        }
    }
}
