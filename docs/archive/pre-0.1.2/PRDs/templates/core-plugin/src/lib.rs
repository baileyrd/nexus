//! {{plugin-name}} — Core Nexus Plugin
//!
//! Entry point for the plugin. The kernel calls `create_plugin()` to
//! instantiate the plugin during the load phase.

mod plugin;
mod events;
mod state;

use plugin::MyPlugin;
use nexus_core::PluginLifecycle;

/// Entry point called by the kernel to instantiate this plugin.
/// This is the only `#[no_mangle]` export required.
#[no_mangle]
pub extern "C" fn create_plugin() -> Box<dyn PluginLifecycle> {
    Box::new(MyPlugin::new())
}
