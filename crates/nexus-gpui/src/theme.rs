use gpui::{hsla, Hsla};

/// Nexus dark theme — Tokyo Night palette mapped to semantic tokens.
#[derive(Clone, Copy)]
pub struct Theme {
    pub bg_base: Hsla,
    pub bg_panel: Hsla,
    pub bg_elevated: Hsla,
    pub fg_text: Hsla,
    pub fg_muted: Hsla,
    pub accent: Hsla,
    pub border: Hsla,
    pub danger: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub info: Hsla,
    pub status_bar_bg: Hsla,
    pub status_bar_fg: Hsla,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            bg_base:       hsla(232. / 360., 0.23, 0.12, 1.0), // #1a1b26
            bg_panel:      hsla(234. / 360., 0.24, 0.18, 1.0), // #24253a
            bg_elevated:   hsla(232. / 360., 0.22, 0.15, 1.0), // #1e1f2e
            fg_text:       hsla(229. / 360., 0.79, 0.73, 1.0), // #c0caf5
            fg_muted:      hsla(220. / 360., 0.09, 0.45, 1.0), // #6b7280
            accent:        hsla(217. / 360., 0.89, 0.72, 1.0), // #7aa2f7
            border:        hsla(233. / 360., 0.27, 0.25, 1.0), // #2e3150
            danger:        hsla(351. / 360., 0.88, 0.72, 1.0), // #f7768e
            success:       hsla( 84. / 360., 0.46, 0.59, 1.0), // #9ece6a
            warning:       hsla( 37. / 360., 0.65, 0.64, 1.0), // #e0af68
            info:          hsla(204. / 360., 1.00, 0.74, 1.0), // #7dcfff
            status_bar_bg: hsla(217. / 360., 0.89, 0.72, 1.0), // accent
            status_bar_fg: hsla(232. / 360., 0.23, 0.12, 1.0), // dark text on accent
        }
    }
}
