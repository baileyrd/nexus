//! Audio plugin registration.
//!
//! BL-117 STT + TTS subsystem. `on_init` loads the
//! `<forge>/.forge/config.toml::[audio]` block and builds the
//! configured backend pair (local / provider / platform). Default
//! is `platform` (Web Speech) — the lightest setup ask of the three
//! backends; the Rust side ships a stub that the `nexus.audio` shell
//! plugin replaces at runtime via the BL-113 contribution path. When
//! the shell plugin isn't enabled, the first dispatch surfaces a
//! clear `BackendNotEnabled` error rather than a panic, mirroring
//! the behaviour of the `local` stub (without the `local-whisper`
//! cargo feature) and the `provider` backend (without an API key).

use std::sync::Arc;

use anyhow::Result;
use nexus_audio::AudioCorePlugin;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;

use super::{
    core_manifest_with_ipc_and_deps, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt,
};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc_and_deps(
                "com.nexus.audio",
                "Audio",
                LifecycleFlags {
                    on_init: true,
                    on_start: false,
                    on_stop: false,
                },
                &with_v1_aliases(nexus_audio::core_plugin::IPC_HANDLERS),
                nexus_audio::core_plugin::MANIFEST_DEPS,
            ),
            forge_root,
            Box::new(AudioCorePlugin::new(forge_root.to_path_buf())),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.audio")?;
    Ok(())
}
