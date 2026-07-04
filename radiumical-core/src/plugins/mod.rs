//! Plugin system — extensible analysis and behavior hooks.
//!
//! Plugins can be registered with the [`Harness`](crate::harness::Harness) and are used by built-in
//! tools (e.g. `source_code`) to enrich the agent's view of the workspace.

use std::collections::HashMap;

pub mod source;

/// A unique plugin identifier.
pub type PluginId = String;

/// Generic plugin trait. All plugins must be Send + Sync so they can live in
/// the harness and be called from async tool executions.
pub trait Plugin: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
}

/// Registry of loaded plugins.
#[derive(Default)]
pub struct PluginRegistry {
    plugins: HashMap<PluginId, Box<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        let id = plugin.id().to_string();
        self.plugins.insert(id, plugin);
    }

    pub fn get(&self, id: &str) -> Option<&dyn Plugin> {
        self.plugins.get(id).map(|p| p.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Plugin> {
        self.plugins.values().map(|p| p.as_ref())
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}
