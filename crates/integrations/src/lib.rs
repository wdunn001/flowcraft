//! FlowCraft's seams to the outside world.
//!
//! Following suite style (as `craft-nextcloud` does for WebDAV), this crate builds requests
//! and parses replies but opens no sockets and spawns no processes. The caller implements
//! [`control::ControlSessions`] and [`cloud::CloudTransport`]; `flowcraft-cli` carries a
//! TCP implementation for app control ports.

pub mod cloud;
pub mod control;

pub use cloud::CloudConnector;
pub use control::AppConnector;

use flowcraft_engine::{Connector, ConnectorError};
use serde_json::Value;

/// A transport's failure: connecting, sending or receiving. The transport writes the
/// message; it must never include a control key (AGENTS.md, secrets).
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct TransportError {
    pub message: String,
}

impl TransportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

/// Both seams in one connector: app commands to control ports, cloud operations to
/// CollabCraft.
pub struct SuiteConnector<S, T> {
    pub apps: AppConnector<S>,
    pub cloud: CloudConnector<T>,
}

impl<S: control::ControlSessions, T: cloud::CloudTransport> Connector for SuiteConnector<S, T> {
    fn app_command(&mut self, app: &str, command: &str, params: &Value) -> Result<Value, ConnectorError> {
        self.apps.app_command(app, command, params)
    }

    fn cloud(&mut self, operation: &Value) -> Result<Value, ConnectorError> {
        self.cloud.cloud(operation)
    }
}
