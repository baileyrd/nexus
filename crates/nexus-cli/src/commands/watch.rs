use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use nexus_storage::StorageEvent;

use crate::app::App;

/// Watch the forge for filesystem changes matching `glob`.
pub fn run(app: &mut App, _glob: &str) -> Result<()> {
    let rx = app
        .storage()?
        .watch_changes()
        .ok_or_else(|| anyhow::anyhow!("file watcher not available"))?;

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    ctrlc::set_handler(move || {
        running_clone.store(false, Ordering::SeqCst);
    })
    .map_err(|e| anyhow::anyhow!("failed to set Ctrl+C handler: {e}"))?;

    println!("Watching for changes. Press Ctrl+C to stop.");

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => {
                print_event(&event);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // No event; check running flag and loop.
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    println!("Stopped.");
    Ok(())
}

fn print_event(event: &StorageEvent) {
    match event {
        StorageEvent::FileCreated { path, content_hash } => {
            println!("created  {path}  [{content_hash}]");
        }
        StorageEvent::FileModified { path, content_hash } => {
            println!("modified {path}  [{content_hash}]");
        }
        StorageEvent::FileDeleted { path } => {
            println!("deleted  {path}");
        }
        StorageEvent::FileRenamed { from, to, content_hash } => {
            println!("renamed  {from} -> {to}  [{content_hash}]");
        }
    }
}
