// SH-002: named z-index scale for the shell.
//
// Six tiers replace the 17+ scattered literals. CSS custom properties
// (--z-*) in index.html mirror these values so shell.css can use
// var(--z-overlay-fatal) instead of a bare `9999`.
//
// Editor-internal layers (CM6 decorations, block handle menus, etc.) are
// self-contained stacking contexts inside the editor view and are not
// governed by this scale.

export const zIndex = {
  /** Window controls, resize handles anchored to the chrome frame. */
  chromeControls: 100,
  /** Dropdown menus, context menus, autocomplete popups. */
  dropdown: 200,
  /** Floating banners / toasts that sit above content but below modals. */
  overlayFloating: 900,
  /** Standard modal dialogs (confirm, capture, tool-call, base dialog). */
  overlayModal: 1100,
  /** High-priority blocking modals (capability consent). */
  overlayToast: 1200,
  /** Fatal overlays and the shell-overlay slot container. */
  overlayFatal: 9999,
} as const

export type ZIndexTier = keyof typeof zIndex
