//! CSS variable registry, variable maps, and `var(...)` substitution.
//!
//! The [`DEFAULT_VARIABLES`] constant encodes the light-mode base palette
//! described in PRD §1.2. Plugins extend this set by declaring additional
//! `--nx-<plugin>-*` variables in their manifest; the resolver then merges
//! them into the final [`VariableMap`].

use std::collections::BTreeMap;

use crate::{Result, ThemeError};

/// Required prefix for every Nexus CSS variable.
pub const VARIABLE_PREFIX: &str = "--nx-";

/// Maximum depth of nested `var(...)` substitution before we declare a cycle.
const MAX_SUBSTITUTION_DEPTH: usize = 16;

/// Ordered mapping of CSS variable name → value.
///
/// Uses a [`BTreeMap`] so that serialized output (and thus diffs and snapshot
/// tests) is deterministic regardless of insertion order.
pub type VariableMap = BTreeMap<String, String>;

/// Built-in light-mode variable defaults — the baseline cascade layer.
///
/// Derived verbatim from PRD §1.2. Values that reference other variables use
/// standard CSS `var(...)` syntax; the resolver substitutes them lazily.
pub const DEFAULT_VARIABLES: &[(&str, &str)] = &[
    // --- Base palette --------------------------------------------------
    ("--nx-color-primary", "#4A90E2"),
    ("--nx-color-primary-light", "#6BA3FF"),
    ("--nx-color-primary-dark", "#2E5CB8"),
    ("--nx-color-secondary", "#9B59B6"),
    ("--nx-color-success", "#27AE60"),
    ("--nx-color-warning", "#F39C12"),
    ("--nx-color-error", "#E74C3C"),
    ("--nx-color-info", "#3498DB"),
    ("--nx-color-neutral-50", "#FAFAFA"),
    ("--nx-color-neutral-100", "#F5F5F5"),
    ("--nx-color-neutral-200", "#E8E8E8"),
    ("--nx-color-neutral-300", "#D4D4D4"),
    ("--nx-color-neutral-400", "#A0A0A0"),
    ("--nx-color-neutral-500", "#737373"),
    ("--nx-color-neutral-600", "#525252"),
    ("--nx-color-neutral-700", "#3F3F3F"),
    ("--nx-color-neutral-800", "#262626"),
    ("--nx-color-neutral-900", "#0F0F0F"),
    // --- Surfaces ------------------------------------------------------
    ("--nx-bg-primary", "#FFFFFF"),
    ("--nx-bg-secondary", "#F8F9FA"),
    ("--nx-bg-tertiary", "#E8EAEF"),
    ("--nx-bg-overlay", "rgba(0, 0, 0, 0.5)"),
    ("--nx-bg-elevated", "#FFFFFF"),
    // --- Text ----------------------------------------------------------
    ("--nx-text-primary", "#1A1A1A"),
    ("--nx-text-secondary", "#4A4A4A"),
    ("--nx-text-tertiary", "#7A7A7A"),
    ("--nx-text-muted", "#A0A0A0"),
    ("--nx-text-inverted", "#FFFFFF"),
    // --- Interactive states -------------------------------------------
    ("--nx-interactive-hover", "rgba(74, 144, 226, 0.08)"),
    ("--nx-interactive-active", "rgba(74, 144, 226, 0.16)"),
    ("--nx-interactive-focus-ring", "2px solid var(--nx-color-primary)"),
    ("--nx-interactive-disabled", "rgba(0, 0, 0, 0.38)"),
    // --- Typography ---------------------------------------------------
    (
        "--nx-type-sans",
        "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, \"Helvetica Neue\", sans-serif",
    ),
    ("--nx-type-mono", "\"Monaco\", \"Courier New\", monospace"),
    ("--nx-type-serif", "\"Georgia\", \"Times New Roman\", serif"),
    ("--nx-type-h1-size", "32px"),
    ("--nx-type-h1-weight", "700"),
    ("--nx-type-h1-line-height", "1.2"),
    ("--nx-type-body-size", "14px"),
    ("--nx-type-body-weight", "400"),
    ("--nx-type-body-line-height", "1.5"),
    ("--nx-type-code-size", "12px"),
    ("--nx-type-code-weight", "400"),
    ("--nx-type-code-line-height", "1.4"),
    // --- Editor & syntax ----------------------------------------------
    ("--nx-editor-bg", "var(--nx-bg-primary)"),
    ("--nx-editor-gutter-bg", "var(--nx-bg-secondary)"),
    ("--nx-editor-line-number", "var(--nx-text-tertiary)"),
    ("--nx-editor-line-highlight", "rgba(74, 144, 226, 0.1)"),
    ("--nx-editor-cursor", "var(--nx-text-primary)"),
    ("--nx-syntax-keyword", "#E74C3C"),
    ("--nx-syntax-string", "#27AE60"),
    ("--nx-syntax-comment", "#95A5A6"),
    ("--nx-syntax-number", "#F39C12"),
    ("--nx-syntax-function", "#3498DB"),
    ("--nx-syntax-variable", "var(--nx-text-primary)"),
    // --- Spacing ------------------------------------------------------
    ("--nx-space-xs", "4px"),
    ("--nx-space-sm", "8px"),
    ("--nx-space-md", "16px"),
    ("--nx-space-lg", "32px"),
    ("--nx-space-xl", "64px"),
    // --- Effects ------------------------------------------------------
    ("--nx-shadow-sm", "0 1px 2px rgba(0, 0, 0, 0.05)"),
    ("--nx-shadow-md", "0 4px 6px rgba(0, 0, 0, 0.1)"),
    ("--nx-shadow-lg", "0 10px 15px rgba(0, 0, 0, 0.1)"),
    ("--nx-blur-sm", "blur(4px)"),
    ("--nx-blur-md", "blur(8px)"),
    // --- Graph & canvas -----------------------------------------------
    ("--nx-graph-node-bg", "var(--nx-bg-elevated)"),
    ("--nx-graph-node-border", "var(--nx-color-primary)"),
    ("--nx-graph-edge-stroke", "var(--nx-text-tertiary)"),
    ("--nx-graph-grid", "rgba(0, 0, 0, 0.05)"),
    ("--nx-graph-selection", "rgba(74, 144, 226, 0.2)"),
    // === Extended palette scales (PRD-07 §1.2) ========================
    // Nine-step tonal ramps for every semantic color so plugins can pick
    // the right contrast for a given surface without hard-coding hex.
    ("--nx-color-primary-50", "#EDF3FC"),
    ("--nx-color-primary-100", "#D4E3F8"),
    ("--nx-color-primary-200", "#A9C7F0"),
    ("--nx-color-primary-300", "#7DABE9"),
    ("--nx-color-primary-400", "#528FE1"),
    ("--nx-color-primary-500", "var(--nx-color-primary)"),
    ("--nx-color-primary-600", "#3A76C6"),
    ("--nx-color-primary-700", "#2E5CB8"),
    ("--nx-color-primary-800", "#1F4486"),
    ("--nx-color-primary-900", "#112A56"),
    ("--nx-color-secondary-50", "#F5EEF8"),
    ("--nx-color-secondary-100", "#EAD5F3"),
    ("--nx-color-secondary-200", "#D5ABE7"),
    ("--nx-color-secondary-300", "#C080DB"),
    ("--nx-color-secondary-400", "#AD68CE"),
    ("--nx-color-secondary-500", "var(--nx-color-secondary)"),
    ("--nx-color-secondary-600", "#8449A1"),
    ("--nx-color-secondary-700", "#6E3C88"),
    ("--nx-color-secondary-800", "#4F2A62"),
    ("--nx-color-secondary-900", "#2F193A"),
    ("--nx-color-success-50", "#E8F7EE"),
    ("--nx-color-success-100", "#C6EAD2"),
    ("--nx-color-success-200", "#9DDAB1"),
    ("--nx-color-success-300", "#6CC78C"),
    ("--nx-color-success-400", "#45B675"),
    ("--nx-color-success-500", "var(--nx-color-success)"),
    ("--nx-color-success-600", "#219653"),
    ("--nx-color-success-700", "#1A7A43"),
    ("--nx-color-success-800", "#115A30"),
    ("--nx-color-success-900", "#083B1F"),
    ("--nx-color-warning-50", "#FEF4E4"),
    ("--nx-color-warning-100", "#FDE4BC"),
    ("--nx-color-warning-200", "#FBCE85"),
    ("--nx-color-warning-300", "#F8B84F"),
    ("--nx-color-warning-400", "#F6A829"),
    ("--nx-color-warning-500", "var(--nx-color-warning)"),
    ("--nx-color-warning-600", "#D17E06"),
    ("--nx-color-warning-700", "#A46305"),
    ("--nx-color-warning-800", "#744704"),
    ("--nx-color-warning-900", "#472A02"),
    ("--nx-color-error-50", "#FCEBE9"),
    ("--nx-color-error-100", "#F7C9C3"),
    ("--nx-color-error-200", "#F0A197"),
    ("--nx-color-error-300", "#EA796A"),
    ("--nx-color-error-400", "#E86253"),
    ("--nx-color-error-500", "var(--nx-color-error)"),
    ("--nx-color-error-600", "#C8372A"),
    ("--nx-color-error-700", "#9E2B21"),
    ("--nx-color-error-800", "#701F18"),
    ("--nx-color-error-900", "#44130F"),
    ("--nx-color-info-50", "#EBF5FB"),
    ("--nx-color-info-100", "#C5E1F4"),
    ("--nx-color-info-200", "#9BCCEC"),
    ("--nx-color-info-300", "#6DB5E3"),
    ("--nx-color-info-400", "#49A2DB"),
    ("--nx-color-info-500", "var(--nx-color-info)"),
    ("--nx-color-info-600", "#2581BF"),
    ("--nx-color-info-700", "#1E6A9D"),
    ("--nx-color-info-800", "#164C70"),
    ("--nx-color-info-900", "#0D2F45"),
    ("--nx-color-neutral-25", "#FDFDFD"),
    ("--nx-color-neutral-950", "#080808"),
    // === Extended surfaces ============================================
    ("--nx-bg-hover", "var(--nx-interactive-hover)"),
    ("--nx-bg-pressed", "var(--nx-interactive-active)"),
    ("--nx-bg-selected", "rgba(74, 144, 226, 0.12)"),
    ("--nx-bg-sunken", "#F0F1F5"),
    ("--nx-bg-code-block", "var(--nx-color-neutral-100)"),
    ("--nx-bg-inline-code", "var(--nx-color-neutral-100)"),
    ("--nx-bg-disabled", "var(--nx-color-neutral-100)"),
    ("--nx-bg-backdrop", "rgba(0, 0, 0, 0.35)"),
    ("--nx-bg-highlight", "rgba(243, 156, 18, 0.18)"),
    // === Borders ======================================================
    ("--nx-border-subtle", "var(--nx-color-neutral-200)"),
    ("--nx-border-default", "var(--nx-color-neutral-300)"),
    ("--nx-border-strong", "var(--nx-color-neutral-400)"),
    ("--nx-border-focus", "var(--nx-color-primary)"),
    ("--nx-border-error", "var(--nx-color-error)"),
    ("--nx-border-success", "var(--nx-color-success)"),
    ("--nx-border-warning", "var(--nx-color-warning)"),
    ("--nx-border-info", "var(--nx-color-info)"),
    ("--nx-border-primary", "var(--nx-color-primary)"),
    ("--nx-border-inverted", "var(--nx-color-neutral-800)"),
    ("--nx-border-width-sm", "1px"),
    ("--nx-border-width-md", "2px"),
    ("--nx-border-width-lg", "4px"),
    // === Extended text ================================================
    ("--nx-text-link", "var(--nx-color-primary)"),
    ("--nx-text-link-hover", "var(--nx-color-primary-light)"),
    ("--nx-text-link-visited", "var(--nx-color-secondary)"),
    ("--nx-text-code", "var(--nx-text-primary)"),
    ("--nx-text-error", "var(--nx-color-error)"),
    ("--nx-text-success", "var(--nx-color-success)"),
    ("--nx-text-warning", "var(--nx-color-warning)"),
    ("--nx-text-info", "var(--nx-color-info)"),
    ("--nx-text-placeholder", "var(--nx-text-muted)"),
    ("--nx-text-on-primary", "#FFFFFF"),
    ("--nx-text-on-secondary", "#FFFFFF"),
    ("--nx-text-on-error", "#FFFFFF"),
    ("--nx-text-on-success", "#FFFFFF"),
    ("--nx-text-on-warning", "#1A1A1A"),
    ("--nx-text-on-info", "#FFFFFF"),
    ("--nx-text-disabled", "var(--nx-interactive-disabled)"),
    // === Icons ========================================================
    ("--nx-icon-primary", "var(--nx-text-primary)"),
    ("--nx-icon-secondary", "var(--nx-text-secondary)"),
    ("--nx-icon-tertiary", "var(--nx-text-tertiary)"),
    ("--nx-icon-inverted", "var(--nx-text-inverted)"),
    ("--nx-icon-disabled", "var(--nx-interactive-disabled)"),
    ("--nx-icon-success", "var(--nx-color-success)"),
    ("--nx-icon-warning", "var(--nx-color-warning)"),
    ("--nx-icon-error", "var(--nx-color-error)"),
    ("--nx-icon-info", "var(--nx-color-info)"),
    ("--nx-icon-accent", "var(--nx-color-primary)"),
    // === Typography scale =============================================
    ("--nx-type-size-xxs", "10px"),
    ("--nx-type-size-xs", "11px"),
    ("--nx-type-size-sm", "12px"),
    ("--nx-type-size-base", "14px"),
    ("--nx-type-size-md", "15px"),
    ("--nx-type-size-lg", "16px"),
    ("--nx-type-size-xl", "18px"),
    ("--nx-type-size-2xl", "20px"),
    ("--nx-type-size-3xl", "24px"),
    ("--nx-type-size-4xl", "28px"),
    ("--nx-type-size-5xl", "32px"),
    ("--nx-type-size-6xl", "40px"),
    ("--nx-type-weight-thin", "100"),
    ("--nx-type-weight-extralight", "200"),
    ("--nx-type-weight-light", "300"),
    ("--nx-type-weight-regular", "400"),
    ("--nx-type-weight-medium", "500"),
    ("--nx-type-weight-semibold", "600"),
    ("--nx-type-weight-bold", "700"),
    ("--nx-type-weight-extrabold", "800"),
    ("--nx-type-weight-black", "900"),
    ("--nx-type-line-none", "1"),
    ("--nx-type-line-tight", "1.2"),
    ("--nx-type-line-snug", "1.35"),
    ("--nx-type-line-normal", "1.5"),
    ("--nx-type-line-relaxed", "1.65"),
    ("--nx-type-line-loose", "1.85"),
    ("--nx-type-letter-tighter", "-0.04em"),
    ("--nx-type-letter-tight", "-0.02em"),
    ("--nx-type-letter-normal", "0"),
    ("--nx-type-letter-wide", "0.02em"),
    ("--nx-type-letter-wider", "0.04em"),
    ("--nx-type-letter-widest", "0.08em"),
    ("--nx-type-h2-size", "24px"),
    ("--nx-type-h2-weight", "700"),
    ("--nx-type-h2-line-height", "1.25"),
    ("--nx-type-h3-size", "20px"),
    ("--nx-type-h3-weight", "600"),
    ("--nx-type-h3-line-height", "1.3"),
    ("--nx-type-h4-size", "18px"),
    ("--nx-type-h4-weight", "600"),
    ("--nx-type-h4-line-height", "1.35"),
    ("--nx-type-h5-size", "16px"),
    ("--nx-type-h5-weight", "600"),
    ("--nx-type-h5-line-height", "1.4"),
    ("--nx-type-h6-size", "14px"),
    ("--nx-type-h6-weight", "600"),
    ("--nx-type-h6-line-height", "1.45"),
    ("--nx-type-subtitle-size", "15px"),
    ("--nx-type-subtitle-weight", "500"),
    ("--nx-type-subtitle-line-height", "1.45"),
    ("--nx-type-caption-size", "12px"),
    ("--nx-type-caption-weight", "400"),
    ("--nx-type-caption-line-height", "1.4"),
    ("--nx-type-overline-size", "11px"),
    ("--nx-type-overline-weight", "600"),
    ("--nx-type-overline-line-height", "1.5"),
    ("--nx-type-overline-letter-spacing", "0.08em"),
    // === Radii ========================================================
    ("--nx-radius-2xs", "1px"),
    ("--nx-radius-xs", "2px"),
    ("--nx-radius-sm", "4px"),
    ("--nx-radius-md", "6px"),
    ("--nx-radius-lg", "8px"),
    ("--nx-radius-xl", "12px"),
    ("--nx-radius-2xl", "16px"),
    ("--nx-radius-3xl", "24px"),
    ("--nx-radius-full", "9999px"),
    ("--nx-radius-pill", "9999px"),
    // === Extended spacing =============================================
    ("--nx-space-3xs", "1px"),
    ("--nx-space-2xs", "2px"),
    ("--nx-space-2xl", "96px"),
    ("--nx-space-3xl", "128px"),
    ("--nx-space-4xl", "192px"),
    ("--nx-space-5xl", "256px"),
    // === Z-index ladder ===============================================
    ("--nx-z-base", "0"),
    ("--nx-z-docked", "10"),
    ("--nx-z-sticky", "100"),
    ("--nx-z-overlay", "500"),
    ("--nx-z-dropdown", "1000"),
    ("--nx-z-modal", "1100"),
    ("--nx-z-popover", "1200"),
    ("--nx-z-toast", "1300"),
    ("--nx-z-tooltip", "1400"),
    ("--nx-z-max", "2147483647"),
    // === Motion =======================================================
    ("--nx-duration-instant", "0ms"),
    ("--nx-duration-faster", "75ms"),
    ("--nx-duration-fast", "150ms"),
    ("--nx-duration-normal", "200ms"),
    ("--nx-duration-slow", "300ms"),
    ("--nx-duration-slower", "400ms"),
    ("--nx-duration-slowest", "600ms"),
    ("--nx-easing-linear", "linear"),
    ("--nx-easing-in", "cubic-bezier(0.4, 0, 1, 1)"),
    ("--nx-easing-out", "cubic-bezier(0, 0, 0.2, 1)"),
    ("--nx-easing-in-out", "cubic-bezier(0.4, 0, 0.2, 1)"),
    ("--nx-easing-bounce", "cubic-bezier(0.68, -0.55, 0.27, 1.55)"),
    ("--nx-easing-smooth", "cubic-bezier(0.25, 0.1, 0.25, 1)"),
    // === Extended effects =============================================
    ("--nx-shadow-xs", "0 1px 1px rgba(0, 0, 0, 0.04)"),
    ("--nx-shadow-xl", "0 20px 25px rgba(0, 0, 0, 0.12)"),
    ("--nx-shadow-2xl", "0 25px 50px rgba(0, 0, 0, 0.18)"),
    ("--nx-shadow-inner", "inset 0 1px 2px rgba(0, 0, 0, 0.06)"),
    ("--nx-shadow-focus", "0 0 0 3px rgba(74, 144, 226, 0.35)"),
    ("--nx-shadow-elevated", "0 8px 24px rgba(0, 0, 0, 0.12)"),
    ("--nx-shadow-dialog", "0 16px 48px rgba(0, 0, 0, 0.18)"),
    ("--nx-shadow-dropdown", "0 6px 16px rgba(0, 0, 0, 0.12)"),
    ("--nx-shadow-toast", "0 10px 32px rgba(0, 0, 0, 0.2)"),
    ("--nx-blur-lg", "blur(16px)"),
    ("--nx-blur-xl", "blur(24px)"),
    ("--nx-blur-2xl", "blur(40px)"),
    // === Buttons ======================================================
    ("--nx-button-radius", "var(--nx-radius-md)"),
    ("--nx-button-padding-sm", "4px 10px"),
    ("--nx-button-padding-md", "8px 14px"),
    ("--nx-button-padding-lg", "12px 20px"),
    ("--nx-button-primary-bg", "var(--nx-color-primary)"),
    ("--nx-button-primary-bg-hover", "var(--nx-color-primary-light)"),
    ("--nx-button-primary-bg-active", "var(--nx-color-primary-dark)"),
    ("--nx-button-primary-bg-disabled", "var(--nx-color-primary-200)"),
    ("--nx-button-primary-text", "var(--nx-text-on-primary)"),
    ("--nx-button-primary-text-disabled", "rgba(255, 255, 255, 0.7)"),
    ("--nx-button-primary-border", "transparent"),
    ("--nx-button-primary-border-hover", "transparent"),
    ("--nx-button-primary-shadow", "var(--nx-shadow-sm)"),
    ("--nx-button-secondary-bg", "var(--nx-bg-tertiary)"),
    ("--nx-button-secondary-bg-hover", "var(--nx-color-neutral-300)"),
    ("--nx-button-secondary-bg-active", "var(--nx-color-neutral-400)"),
    ("--nx-button-secondary-bg-disabled", "var(--nx-color-neutral-100)"),
    ("--nx-button-secondary-text", "var(--nx-text-primary)"),
    ("--nx-button-secondary-text-disabled", "var(--nx-text-disabled)"),
    ("--nx-button-secondary-border", "var(--nx-border-default)"),
    ("--nx-button-secondary-border-hover", "var(--nx-border-strong)"),
    ("--nx-button-secondary-shadow", "none"),
    ("--nx-button-danger-bg", "var(--nx-color-error)"),
    ("--nx-button-danger-bg-hover", "var(--nx-color-error-400)"),
    ("--nx-button-danger-bg-active", "var(--nx-color-error-700)"),
    ("--nx-button-danger-bg-disabled", "var(--nx-color-error-200)"),
    ("--nx-button-danger-text", "var(--nx-text-on-error)"),
    ("--nx-button-danger-text-disabled", "rgba(255, 255, 255, 0.7)"),
    ("--nx-button-danger-border", "transparent"),
    ("--nx-button-danger-border-hover", "transparent"),
    ("--nx-button-danger-shadow", "var(--nx-shadow-sm)"),
    ("--nx-button-ghost-bg", "transparent"),
    ("--nx-button-ghost-bg-hover", "var(--nx-interactive-hover)"),
    ("--nx-button-ghost-bg-active", "var(--nx-interactive-active)"),
    ("--nx-button-ghost-bg-disabled", "transparent"),
    ("--nx-button-ghost-text", "var(--nx-text-primary)"),
    ("--nx-button-ghost-text-disabled", "var(--nx-text-disabled)"),
    ("--nx-button-ghost-border", "transparent"),
    ("--nx-button-ghost-border-hover", "transparent"),
    ("--nx-button-ghost-shadow", "none"),
    ("--nx-button-link-text", "var(--nx-text-link)"),
    ("--nx-button-link-text-hover", "var(--nx-text-link-hover)"),
    ("--nx-button-link-text-active", "var(--nx-color-primary-dark)"),
    // === Inputs =======================================================
    ("--nx-input-bg", "var(--nx-bg-primary)"),
    ("--nx-input-bg-disabled", "var(--nx-bg-disabled)"),
    ("--nx-input-bg-focus", "var(--nx-bg-primary)"),
    ("--nx-input-border", "var(--nx-border-default)"),
    ("--nx-input-border-hover", "var(--nx-border-strong)"),
    ("--nx-input-border-focus", "var(--nx-border-focus)"),
    ("--nx-input-border-error", "var(--nx-border-error)"),
    ("--nx-input-border-disabled", "var(--nx-color-neutral-200)"),
    ("--nx-input-text", "var(--nx-text-primary)"),
    ("--nx-input-text-placeholder", "var(--nx-text-placeholder)"),
    ("--nx-input-text-disabled", "var(--nx-text-disabled)"),
    ("--nx-input-padding-x", "10px"),
    ("--nx-input-padding-y", "6px"),
    ("--nx-input-radius", "var(--nx-radius-md)"),
    ("--nx-input-height-sm", "24px"),
    ("--nx-input-height-md", "32px"),
    ("--nx-input-height-lg", "40px"),
    ("--nx-input-label", "var(--nx-text-secondary)"),
    ("--nx-input-helper-text", "var(--nx-text-tertiary)"),
    ("--nx-input-error-text", "var(--nx-color-error)"),
    // === Modals / dialogs =============================================
    ("--nx-modal-bg", "var(--nx-bg-elevated)"),
    ("--nx-modal-border", "var(--nx-border-subtle)"),
    ("--nx-modal-shadow", "var(--nx-shadow-dialog)"),
    ("--nx-modal-backdrop", "var(--nx-bg-backdrop)"),
    ("--nx-modal-radius", "var(--nx-radius-xl)"),
    ("--nx-modal-padding", "24px"),
    ("--nx-modal-header-bg", "transparent"),
    ("--nx-modal-footer-bg", "var(--nx-bg-secondary)"),
    // === Tooltips =====================================================
    ("--nx-tooltip-bg", "var(--nx-color-neutral-800)"),
    ("--nx-tooltip-text", "var(--nx-text-inverted)"),
    ("--nx-tooltip-border", "transparent"),
    ("--nx-tooltip-shadow", "var(--nx-shadow-md)"),
    ("--nx-tooltip-padding", "6px 8px"),
    ("--nx-tooltip-radius", "var(--nx-radius-sm)"),
    ("--nx-tooltip-arrow-size", "6px"),
    // === Toasts =======================================================
    ("--nx-toast-bg", "var(--nx-bg-elevated)"),
    ("--nx-toast-text", "var(--nx-text-primary)"),
    ("--nx-toast-border", "var(--nx-border-subtle)"),
    ("--nx-toast-shadow", "var(--nx-shadow-toast)"),
    ("--nx-toast-radius", "var(--nx-radius-lg)"),
    ("--nx-toast-info-bg", "var(--nx-color-info-50)"),
    ("--nx-toast-info-border", "var(--nx-color-info)"),
    ("--nx-toast-success-bg", "var(--nx-color-success-50)"),
    ("--nx-toast-success-border", "var(--nx-color-success)"),
    ("--nx-toast-warning-bg", "var(--nx-color-warning-50)"),
    ("--nx-toast-warning-border", "var(--nx-color-warning)"),
    ("--nx-toast-error-bg", "var(--nx-color-error-50)"),
    ("--nx-toast-error-border", "var(--nx-color-error)"),
    // === Context menu =================================================
    ("--nx-menu-bg", "var(--nx-bg-elevated)"),
    ("--nx-menu-text", "var(--nx-text-primary)"),
    ("--nx-menu-border", "var(--nx-border-subtle)"),
    ("--nx-menu-shadow", "var(--nx-shadow-dropdown)"),
    ("--nx-menu-radius", "var(--nx-radius-md)"),
    ("--nx-menu-item-hover-bg", "var(--nx-interactive-hover)"),
    ("--nx-menu-item-active-bg", "var(--nx-interactive-active)"),
    ("--nx-menu-separator", "var(--nx-border-subtle)"),
    ("--nx-menu-padding", "4px 0"),
    ("--nx-menu-item-padding", "6px 12px"),
    // === Status bar ===================================================
    ("--nx-statusbar-bg", "var(--nx-bg-secondary)"),
    ("--nx-statusbar-text", "var(--nx-text-secondary)"),
    ("--nx-statusbar-border", "var(--nx-border-subtle)"),
    ("--nx-statusbar-item-hover", "var(--nx-interactive-hover)"),
    ("--nx-statusbar-item-active", "var(--nx-interactive-active)"),
    ("--nx-statusbar-height", "24px"),
    // === Tab bar ======================================================
    ("--nx-tabbar-bg", "var(--nx-bg-secondary)"),
    ("--nx-tabbar-active-bg", "var(--nx-bg-primary)"),
    ("--nx-tabbar-active-border", "var(--nx-color-primary)"),
    ("--nx-tabbar-inactive-text", "var(--nx-text-secondary)"),
    ("--nx-tabbar-active-text", "var(--nx-text-primary)"),
    ("--nx-tabbar-hover-bg", "var(--nx-interactive-hover)"),
    ("--nx-tabbar-close-hover", "var(--nx-color-error)"),
    ("--nx-tabbar-dirty", "var(--nx-color-warning)"),
    ("--nx-tabbar-border", "var(--nx-border-subtle)"),
    ("--nx-tabbar-height", "32px"),
    // === Ribbon =======================================================
    ("--nx-ribbon-bg", "var(--nx-bg-secondary)"),
    ("--nx-ribbon-text", "var(--nx-text-secondary)"),
    ("--nx-ribbon-border", "var(--nx-border-subtle)"),
    ("--nx-ribbon-item-hover", "var(--nx-interactive-hover)"),
    ("--nx-ribbon-item-active", "var(--nx-interactive-active)"),
    ("--nx-ribbon-width", "44px"),
    // === Sidebar ======================================================
    ("--nx-sidebar-bg", "var(--nx-bg-secondary)"),
    ("--nx-sidebar-border", "var(--nx-border-subtle)"),
    ("--nx-sidebar-item-hover", "var(--nx-interactive-hover)"),
    ("--nx-sidebar-item-active-bg", "var(--nx-bg-selected)"),
    ("--nx-sidebar-item-active-text", "var(--nx-text-primary)"),
    ("--nx-sidebar-selector-bg", "var(--nx-bg-secondary)"),
    ("--nx-sidebar-footer-bg", "var(--nx-bg-secondary)"),
    ("--nx-sidebar-width", "240px"),
    ("--nx-sidebar-mini-width", "44px"),
    // === Scrollbar ====================================================
    ("--nx-scrollbar-thumb", "var(--nx-color-neutral-300)"),
    ("--nx-scrollbar-thumb-hover", "var(--nx-color-neutral-400)"),
    ("--nx-scrollbar-track", "transparent"),
    ("--nx-scrollbar-width", "10px"),
    ("--nx-scrollbar-radius", "var(--nx-radius-full)"),
    // === Selection ====================================================
    ("--nx-selection-bg", "rgba(74, 144, 226, 0.3)"),
    ("--nx-selection-text", "inherit"),
    ("--nx-selection-inactive-bg", "rgba(74, 144, 226, 0.15)"),
    // === Links ========================================================
    ("--nx-link-text", "var(--nx-text-link)"),
    ("--nx-link-text-hover", "var(--nx-text-link-hover)"),
    ("--nx-link-text-visited", "var(--nx-text-link-visited)"),
    ("--nx-link-text-active", "var(--nx-color-primary-dark)"),
    ("--nx-link-underline", "underline"),
    // === Code blocks ==================================================
    ("--nx-code-block-bg", "var(--nx-bg-code-block)"),
    ("--nx-code-block-text", "var(--nx-text-code)"),
    ("--nx-code-block-border", "var(--nx-border-subtle)"),
    ("--nx-code-block-line-number", "var(--nx-text-tertiary)"),
    ("--nx-code-block-line-highlight", "var(--nx-editor-line-highlight)"),
    // === Graph & canvas (extended) ====================================
    ("--nx-graph-node-bg-hover", "var(--nx-color-primary-50)"),
    ("--nx-graph-node-bg-selected", "var(--nx-color-primary-100)"),
    ("--nx-graph-node-border-hover", "var(--nx-color-primary-light)"),
    ("--nx-graph-node-border-selected", "var(--nx-color-primary-dark)"),
    ("--nx-graph-edge-stroke-hover", "var(--nx-text-secondary)"),
    ("--nx-graph-edge-stroke-selected", "var(--nx-color-primary)"),
    ("--nx-graph-edge-arrow", "var(--nx-text-tertiary)"),
    ("--nx-graph-grid-line-major", "rgba(0, 0, 0, 0.08)"),
    ("--nx-graph-grid-line-minor", "rgba(0, 0, 0, 0.03)"),
    ("--nx-graph-selection-border", "var(--nx-color-primary)"),
    ("--nx-graph-minimap-bg", "rgba(255, 255, 255, 0.9)"),
    ("--nx-graph-minimap-viewport", "rgba(74, 144, 226, 0.2)"),
    // === Editor (extended) ============================================
    ("--nx-editor-selection", "rgba(74, 144, 226, 0.3)"),
    ("--nx-editor-active-line", "rgba(74, 144, 226, 0.08)"),
    ("--nx-editor-find-match", "rgba(243, 156, 18, 0.35)"),
    ("--nx-editor-find-match-highlight", "rgba(243, 156, 18, 0.2)"),
    ("--nx-editor-gutter-border", "var(--nx-border-subtle)"),
    ("--nx-editor-gutter-active-line", "var(--nx-text-primary)"),
    ("--nx-editor-indent-guide", "rgba(0, 0, 0, 0.06)"),
    ("--nx-editor-whitespace", "rgba(0, 0, 0, 0.2)"),
    ("--nx-editor-ruler", "rgba(0, 0, 0, 0.1)"),
    ("--nx-editor-fold-marker", "var(--nx-text-tertiary)"),
    ("--nx-editor-matching-bracket", "rgba(74, 144, 226, 0.25)"),
    ("--nx-editor-linked-edit", "rgba(155, 89, 182, 0.2)"),
    // === Syntax (extended) ============================================
    ("--nx-syntax-type", "#2980B9"),
    ("--nx-syntax-class", "#16A085"),
    ("--nx-syntax-namespace", "#8E44AD"),
    ("--nx-syntax-operator", "#2C3E50"),
    ("--nx-syntax-punctuation", "#4A4A4A"),
    ("--nx-syntax-tag", "#C0392B"),
    ("--nx-syntax-attr-name", "#E67E22"),
    ("--nx-syntax-attr-value", "#27AE60"),
    ("--nx-syntax-regex", "#D35400"),
    ("--nx-syntax-escape", "#E74C3C"),
    ("--nx-syntax-constant", "#8E44AD"),
    ("--nx-syntax-builtin", "#2980B9"),
    ("--nx-syntax-label", "#7F8C8D"),
    ("--nx-syntax-property", "#3498DB"),
    ("--nx-syntax-parameter", "#2C3E50"),
    ("--nx-syntax-decorator", "#9B59B6"),
    ("--nx-syntax-annotation", "#9B59B6"),
    ("--nx-syntax-deleted", "#C0392B"),
    ("--nx-syntax-inserted", "#27AE60"),
    ("--nx-syntax-heading", "#2C3E50"),
    ("--nx-syntax-link-text", "var(--nx-color-primary)"),
    ("--nx-syntax-link-url", "var(--nx-color-info)"),
    ("--nx-syntax-url", "var(--nx-color-info)"),
    ("--nx-syntax-bold", "var(--nx-text-primary)"),
    ("--nx-syntax-italic", "var(--nx-text-primary)"),
    ("--nx-syntax-underline", "var(--nx-text-primary)"),
    ("--nx-syntax-strikethrough", "var(--nx-text-muted)"),
    ("--nx-syntax-quote", "var(--nx-text-secondary)"),
    ("--nx-syntax-macro", "#9B59B6"),
    ("--nx-syntax-enum", "#16A085"),
    ("--nx-syntax-variant", "#1ABC9C"),
    // === Form validation ==============================================
    ("--nx-form-valid", "var(--nx-color-success)"),
    ("--nx-form-invalid", "var(--nx-color-error)"),
    ("--nx-form-pending", "var(--nx-color-warning)"),
    ("--nx-form-loading", "var(--nx-color-info)"),
];

/// Returns the default variables as a fresh owned [`VariableMap`].
#[must_use]
pub fn default_variables() -> VariableMap {
    DEFAULT_VARIABLES
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

/// Validate that `name` starts with the required `--nx-` prefix.
///
/// # Errors
/// Returns [`ThemeError::InvalidVariableName`] if the name is not prefixed.
pub fn validate_variable_name(name: &str) -> Result<()> {
    if name.starts_with(VARIABLE_PREFIX) {
        Ok(())
    } else {
        Err(ThemeError::InvalidVariableName(name.to_string()))
    }
}

/// Substitute every `var(--nx-foo)` reference in `value` with the concrete
/// value from `vars`.
///
/// Unknown variables are left as-is so downstream CSS still sees a valid
/// `var(...)` fallback. Detects cycles up to [`MAX_SUBSTITUTION_DEPTH`].
///
/// # Errors
/// Returns [`ThemeError::CircularReference`] if the same variable is
/// substituted recursively more than [`MAX_SUBSTITUTION_DEPTH`] times.
pub fn substitute(value: &str, vars: &VariableMap) -> Result<String> {
    substitute_inner(value, vars, 0)
}

fn substitute_inner(value: &str, vars: &VariableMap, depth: usize) -> Result<String> {
    if depth > MAX_SUBSTITUTION_DEPTH {
        return Err(ThemeError::CircularReference(value.to_string()));
    }

    let mut out = String::with_capacity(value.len());
    let mut remaining = value;

    while let Some(idx) = remaining.find("var(") {
        out.push_str(&remaining[..idx]);
        let after = &remaining[idx + 4..];

        let Some(end_rel) = find_matching_paren(after) else {
            out.push_str("var(");
            out.push_str(after);
            return Ok(out);
        };

        let inside = &after[..end_rel];
        let name_end = inside.find(',').unwrap_or(inside.len());
        let name = inside[..name_end].trim();

        if let Some(raw) = vars.get(name) {
            let substituted = substitute_inner(raw, vars, depth + 1)?;
            out.push_str(&substituted);
        } else {
            // Unknown variable — preserve the original `var(...)` call so the
            // browser can still honour any CSS fallback.
            out.push_str("var(");
            out.push_str(inside);
            out.push(')');
        }

        remaining = &after[end_rel + 1..];
    }

    out.push_str(remaining);
    Ok(out)
}

/// Scans `s` starting just after a `var(` and returns the offset of the
/// matching closing paren (relative to the start of `s`).
fn find_matching_paren(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 1_usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_well_formed() {
        let vars = default_variables();
        assert!(vars.contains_key("--nx-color-primary"));
        for name in vars.keys() {
            validate_variable_name(name).unwrap();
        }
        // Spot-check: the PRD lists these explicitly.
        assert_eq!(vars["--nx-space-md"], "16px");
        assert_eq!(vars["--nx-color-success"], "#27AE60");
    }

    #[test]
    fn defaults_cover_prd_minimum() {
        // PRD-07 §1 calls for 400+ semantically-named variables. Lock in
        // the floor so future deletions show up as a failing test rather
        // than silent regression.
        let vars = default_variables();
        assert!(
            vars.len() >= 400,
            "expected at least 400 default variables, got {}",
            vars.len()
        );
    }

    #[test]
    fn defaults_have_no_duplicate_keys() {
        // The raw slice allows duplicates; the BTreeMap would silently
        // overwrite. Guard against accidental collisions during expansion.
        let mut seen = std::collections::HashSet::new();
        for (name, _) in DEFAULT_VARIABLES {
            assert!(
                seen.insert(*name),
                "duplicate default variable: {name}"
            );
        }
    }

    #[test]
    fn defaults_resolve_without_cycles() {
        // Every var(...) reference in the defaults must resolve or remain
        // a literal unknown. A cycle would have the resolver bail out.
        let vars = default_variables();
        for (name, value) in vars.iter() {
            if value.contains("var(") {
                substitute(value, &vars).unwrap_or_else(|e| {
                    panic!("failed to resolve {name} = {value}: {e:?}")
                });
            }
        }
    }

    #[test]
    fn substitute_resolves_known_references() {
        let vars = default_variables();
        let resolved = substitute("var(--nx-bg-primary)", &vars).unwrap();
        assert_eq!(resolved, "#FFFFFF");
    }

    #[test]
    fn substitute_resolves_nested_references() {
        let mut vars = default_variables();
        vars.insert(
            "--nx-editor-bg".into(),
            "var(--nx-bg-primary)".into(),
        );
        let resolved = substitute("var(--nx-editor-bg)", &vars).unwrap();
        assert_eq!(resolved, "#FFFFFF");
    }

    #[test]
    fn substitute_leaves_unknown_variables_intact() {
        let vars = default_variables();
        let out = substitute("var(--nx-unknown-thing)", &vars).unwrap();
        assert_eq!(out, "var(--nx-unknown-thing)");
    }

    #[test]
    fn substitute_detects_cycles() {
        let mut vars = VariableMap::new();
        vars.insert("--nx-a".into(), "var(--nx-b)".into());
        vars.insert("--nx-b".into(), "var(--nx-a)".into());
        let err = substitute("var(--nx-a)", &vars).unwrap_err();
        assert!(matches!(err, ThemeError::CircularReference(_)));
    }

    #[test]
    fn substitute_handles_plain_text() {
        let vars = default_variables();
        let out = substitute("#FF00FF", &vars).unwrap();
        assert_eq!(out, "#FF00FF");
    }

    #[test]
    fn validate_rejects_bad_prefix() {
        assert!(validate_variable_name("--other-thing").is_err());
        assert!(validate_variable_name("--nx-color-primary").is_ok());
    }
}
