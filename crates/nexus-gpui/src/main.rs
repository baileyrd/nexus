use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "nexus-gpui", about = "Nexus desktop shell")]
struct Args {
    /// Path to the forge (directory of markdown files).
    #[arg(long, env = "NEXUS_FORGE_PATH")]
    forge_path: PathBuf,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nexus_gpui=info,nexus_bootstrap=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();
    nexus_gpui::run_app(args.forge_path)
}
