//! Audio plugin registration.
//!
//! BL-117 STT + TTS subsystem. `on_init` loads the
//! `<forge>/.forge/config.toml::[audio]` block and builds the
//! configured backend pair (local / provider / platform). The
//! shipped build stubs `local` + `platform` so a forge without
//! an `OPENAI_API_KEY` surfaces a clear "backend not enabled"
//! error from the first dispatch rather than a panic.

use std::sync::Arc;

use anyhow::Result;
use nexus_audio::AudioCorePlugin;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.audio",
                "Audio",
                LifecycleFlags {
                    on_init: true,
                    on_start: false,
                    on_stop: false,
                },
                &with_v1_aliases(nexus_audio::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(AudioCorePlugin::new(forge_root.to_path_buf())),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.audio")?;
    Ok(())
}
