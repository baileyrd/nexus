//! Thin wrapper around `nexus_tui::run_tui()`. The real implementation lives
//! in `lib.rs` so that `nexus-cli`'s `nexus tui` subcommand can call it
//! directly without subprocess overhead.

use anyhow::Result;

fn main() -> Result<()> {
    nexus_tui::run_tui()
}
