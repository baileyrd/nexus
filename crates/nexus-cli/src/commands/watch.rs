use anyhow::Result;
use nexus_kernel::{EventFilter, NexusEvent, PluginContext};

use crate::app::App;

/// Watch the forge for filesystem changes matching `glob`.
///
/// Subscribes to `com.nexus.storage.*` events on the kernel event bus and
/// prints each one until the user presses Ctrl+C.
pub fn run(app: &mut App, _glob: &str) -> Result<()> {
    let (runtime, rt) = app.runtime()?;
    let mut sub = runtime
        .context
        .subscribe(EventFilter::CustomPrefix("com.nexus.storage.".to_string()));

    println!("Watching for changes. Press Ctrl+C to stop.");

    rt.block_on(async {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
                maybe = sub.recv() => {
                    match maybe {
                        Ok(evt) => {
                            if let NexusEvent::Custom { type_id, payload, .. } = &evt.event {
                                print_event(type_id, payload);
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });

    println!("Stopped.");
    Ok(())
}

fn print_event(type_id: &str, payload: &serde_json::Value) {
    let path = payload.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let content_hash = payload
        .get("content_hash")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    match type_id {
        "com.nexus.storage.file_created" if is_file_path(path) => {
            println!("created  {path}  [{content_hash}]");
        }
        "com.nexus.storage.file_modified" if is_file_path(path) => {
            println!("modified {path}  [{content_hash}]");
        }
        "com.nexus.storage.file_deleted" if is_file_path(path) => {
            println!("deleted  {path}");
        }
        "com.nexus.storage.file_renamed" => {
            let from = payload.get("from").and_then(|v| v.as_str()).unwrap_or("");
            let to = payload.get("to").and_then(|v| v.as_str()).unwrap_or("");
            if is_file_path(from) || is_file_path(to) {
                println!("renamed  {from} -> {to}  [{content_hash}]");
            }
        }
        _ => {
            // indexing.* events, directory-level noise, and any future storage
            // events are suppressed here.
        }
    }
}

/// Treat a path as a real file event if it nests inside a forge subdirectory.
/// The storage watcher occasionally emits events for the top-level `notes/`
/// or `attachments/` directories themselves; those are not useful to surface.
fn is_file_path(path: &str) -> bool {
    path.contains('/')
}
