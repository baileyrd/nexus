//! Core terminal logic for rusty_term.
//!
//! This module is the platform-independent heart of the emulator, split by
//! standards layer into focused submodules:
//!
//! - [`cell`] — the [`Cell`] atom and Unicode width classification (L05)
//! - [`grid`] — the [`Grid`] screen buffer: scrollback, alt screen, scrolling
//!   region, cursor (L06 state)
//! - [`parser`] — the [`AnsiParser`], a VT100/ECMA-48 escape-sequence state
//!   machine driving the grid (L06)
//! - [`color`] — the ANSI palette and SGR color resolution (L06)
//! - [`osc`] — OSC dispatch: window title, cwd, hyperlinks, clipboard (L08)
//!
//! The parser drives the grid through its semantic API; the grid hands the
//! renderer a [`DirtyFrame`] snapshot. The parser intentionally implements a
//! pragmatic subset of the VT100/ECMA-48 escape repertoire.

// Vendored from rusty_term (RFC 0003). Some upstream API is consumed only by the
// GUI renderer, the in-band L13 channel transport, or the L13 render protocol —
// none of which Nexus compiles (see ../../ATTRIBUTION.md) — so parts of the core
// are unused here. These allows keep the vendored files byte-faithful to upstream
// for future re-syncs; the `Vt` facade in `lib.rs` is outside this module and
// stays fully lint-checked.
//   - dead_code: GUI/channel/render API isn't called in the headless build.
//   - field_reassign_with_default: a couple of upstream test helpers build a
//     `Default` then set fields.
//   - default_constructed_unit_structs: the no-op channel stub makes
//     `ChannelState` a unit struct, so its `::default()` at the grid call site
//     is flagged; not worth diverging the vendored grid.rs over.
#![allow(
    dead_code,
    clippy::field_reassign_with_default,
    clippy::default_constructed_unit_structs
)]

mod base64;
mod cell;
#[cfg(feature = "l13")]
mod channel;
mod charset;
mod color;
mod grid;
mod inflate;
mod iterm;
mod jpeg;
mod kitty;
mod osc;
mod parser;
mod png;
mod sixel;

#[cfg(feature = "gui")]
pub(crate) use base64::encode as base64_encode;
pub use cell::{
    ATTR_BLINK, ATTR_BOLD, ATTR_DIM, ATTR_HIDDEN, ATTR_ITALIC, ATTR_MASK, ATTR_REVERSE,
    ATTR_STRIKE, ATTR_UNDERLINE, WIDE_TRAILER,
};
#[cfg(feature = "gui")]
pub use cell::{Cell, char_width};
pub use color::Theme;
pub use grid::{CursorShape, DirtyFrame, Grid, LineAttr, SCROLLBACK_MAX};
#[cfg(feature = "gui")]
pub use grid::{MouseModes, Selection};
pub use parser::AnsiParser;

#[cfg(test)]
mod tests;
