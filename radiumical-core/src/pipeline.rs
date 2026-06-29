//! Agent pipeline — backwards-compatible wrapper around the agent harness.
//!
//! New code should prefer [`Harness`][crate::harness::Harness] directly so it
//! can choose an [`Agent`][crate::agent::Agent]. This module keeps the old
//! `PipelineRunner` API alive.

pub use crate::harness::ToolHook;

use crate::agent::Agent;
use crate::harness::Harness;
use crate::provider::Provider;
use crate::types::{AgentMode, SessionConfig, UiEvent};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

pub struct PipelineRunner {
    harness: Harness,
}

impl PipelineRunner {
    pub fn new(config: SessionConfig, provider: Arc<dyn Provider>) -> Self {
        Self {
            harness: Harness::new(config, provider),
        }
    }

    pub fn set_model(&mut self, model: String) {
        self.harness.set_model(model);
    }

    pub fn set_mode(&mut self, mode: AgentMode) {
        self.harness.set_mode(mode);
    }

    pub async fn run(
        &mut self,
        task: String,
        workspace: PathBuf,
        _hb_cancel: Option<tokio::sync::mpsc::UnboundedSender<()>>,
        ui_tx: mpsc::Sender<UiEvent>,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let agent = Agent::default_coder();
        self.harness
            .run(task, workspace, &agent, _hb_cancel, ui_tx, cancel_rx)
            .await
    }
}
