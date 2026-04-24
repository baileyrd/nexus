//! `nexus tui` — dispatch into the `nexus-tui` library entry point.
//!
//! The real implementation lives in `crates/nexus-tui/src/lib.rs`. This
//! module is a one-liner that keeps the CLI's subcommand dispatch table
//! uniform (every subcommand calls into `commands::<name>::run`).

use anyhow::Result;

pub fn run() -> Result<()> {
    nexus_tui::run_tui()
}
