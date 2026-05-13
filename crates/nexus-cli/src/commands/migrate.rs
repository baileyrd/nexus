//! `nexus migrate scan|registered` — PRD-06 §9 (DG-43).
//!
//! `scan` walks the forge root and tallies how many markdown files
//! sit at each format version. `registered` prints the migrations
//! compiled into this build (empty until a forge-format-breaking
//! change ships). Both run against the forge root the app already
//! resolved.

use anyhow::{Context, Result};
use nexus_formats::{scan_versions, MigrationRegistry};

use crate::app::App;

/// `nexus migrate scan` — print version distribution.
///
/// # Errors
/// Returns `anyhow::Error` when the forge root can't be resolved
/// or the walk hits an I/O failure.
pub fn scan(app: &mut App) -> Result<()> {
    let forge_root = app.forge_root();
    let tally = scan_versions(forge_root)
        .with_context(|| format!("scanning {}", forge_root.display()))?;
    if tally.is_empty() {
        println!("(no markdown files under {})", forge_root.display());
        return Ok(());
    }
    let total: u64 = tally.iter().map(|t| t.count).sum();
    let version_w = tally
        .iter()
        .map(|t| t.version.len())
        .max()
        .unwrap_or(7)
        .max("VERSION".len());
    println!("{:width$}  COUNT", "VERSION", width = version_w);
    for entry in tally {
        println!("{:width$}  {}", entry.version, entry.count, width = version_w);
    }
    println!("\nTotal markdown files: {total}");
    Ok(())
}

/// `nexus migrate registered` — list the in-build migration table.
///
/// # Errors
/// None today — kept as `Result` for parity with `scan` and to leave
/// room for future external loaders.
pub fn registered() -> Result<()> {
    let registry = MigrationRegistry::new();
    if registry.is_empty() {
        println!("No migrations registered in this build.");
        println!("(No forge-format-breaking change has shipped yet.)");
        return Ok(());
    }
    println!("Registered migrations:");
    for (from, to) in registry.pairs() {
        println!("  {from} → {to}");
    }
    Ok(())
}
