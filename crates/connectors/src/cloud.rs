//! The CollabCraft binding for cloud operations.
//!
//! The endpoint shapes are a draft contract for the flow lane of the CollabCraft service
//! (`craft.quasarke.net`); the service does not expose them yet. See `docs/gaps.md`. The
//! binding builds [`CloudRequest`] values and interprets replies; a [`CloudTransport`]
//! owns the HTTP client, the base URL and the session.

use crate::TransportError;
use flowcraft_engine::{Connector, ConnectorError};
use flowcraft_flow::CloudOperation;
use serde_json::Value;

/// One request to the CollabCraft API, relative to the service's base URL.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudRequest {
    pub method: &'static str,
    pub path: String,
    pub body: Value,
}

/// One reply from the CollabCraft API.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudReply {
    pub status: u16,
    pub body: Value,
}

/// Why a cloud operation could not be mapped or did not succeed.
#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("not a cloud operation: {0}")]
    NotAnOperation(serde_json::Error),
    #[error("refusing the file path {0:?}: paths are relative, with no empty or `..` segments")]
    BadPath(String),
    #[error("the service answered {status}: {message}")]
    Service { status: u16, message: String },
}

fn checked_path(path: &str) -> Result<&str, CloudError> {
    let hostile = path.is_empty() || path.starts_with('/') || path.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..");
    if hostile {
        return Err(CloudError::BadPath(path.to_string()));
    }
    Ok(path)
}

/// Maps one resolved, tagged operation value onto the API.
pub fn request_for(operation: &Value) -> Result<CloudRequest, CloudError> {
    let operation: CloudOperation = serde_json::from_value(operation.clone()).map_err(CloudError::NotAnOperation)?;
    Ok(match operation {
        CloudOperation::StoreFile { path, content } => {
            CloudRequest { method: "PUT", path: format!("/api/v1/files/{}", checked_path(&path)?), body: serde_json::json!({"content": content}) }
        }
        CloudOperation::Share { path, with } => {
            checked_path(&path)?;
            CloudRequest { method: "POST", path: "/api/v1/shares".into(), body: serde_json::json!({"path": path, "with": with}) }
        }
        CloudOperation::Notify { user, message } => {
            CloudRequest { method: "POST", path: "/api/v1/notifications".into(), body: serde_json::json!({"user": user, "message": message}) }
        }
        CloudOperation::RequestSignature { document, signers } => {
            CloudRequest { method: "POST", path: "/api/v1/signatures".into(), body: serde_json::json!({"document": document, "signers": signers}) }
        }
        CloudOperation::SignatureStatus { request } => {
            CloudRequest { method: "POST", path: "/api/v1/signatures/status".into(), body: serde_json::json!({"request": request}) }
        }
    })
}

/// Owns the HTTP side: base URL, TLS, the signed-in session. Implemented outside this
/// crate; apps hold no provider credential (the CollabCraft rule).
pub trait CloudTransport {
    fn send(&mut self, request: &CloudRequest) -> Result<CloudReply, TransportError>;
}

/// A [`Connector`] for cloud operations. App commands need an `AppConnector`; pair the two
/// with `SuiteConnector`.
pub struct CloudConnector<T> {
    transport: T,
}

impl<T: CloudTransport> CloudConnector<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
}

fn service_message(body: &Value) -> String {
    body.get("error").and_then(Value::as_str).unwrap_or("no error message").to_string()
}

impl<T: CloudTransport> Connector for CloudConnector<T> {
    fn app_command(&mut self, app: &str, _command: &str, _params: &Value) -> Result<Value, ConnectorError> {
        Err(ConnectorError::new(format!("this connector reaches CollabCraft only; app commands for {app:?} need control sessions")))
    }

    fn cloud(&mut self, operation: &Value) -> Result<Value, ConnectorError> {
        let request = request_for(operation).map_err(|error| ConnectorError::new(error.to_string()))?;
        let reply = self.transport.send(&request).map_err(|error| ConnectorError::new(error.to_string()))?;
        if (200..300).contains(&reply.status) {
            Ok(reply.body)
        } else {
            Err(ConnectorError::new(CloudError::Service { status: reply.status, message: service_message(&reply.body) }.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CloudConnector, CloudError, CloudReply, CloudRequest, CloudTransport, request_for};
    use crate::TransportError;
    use flowcraft_engine::Connector;
    use serde_json::json;

    #[test]
    fn maps_operations_onto_the_draft_api() {
        let request = request_for(&json!({"type": "store-file", "path": "signed/contract.pdf", "content": "DATA"})).unwrap();
        assert_eq!(request.method, "PUT");
        assert_eq!(request.path, "/api/v1/files/signed/contract.pdf");
        assert_eq!(request.body, json!({"content": "DATA"}));

        let request = request_for(&json!({"type": "request-signature", "document": "d1", "signers": ["ana@example.com"]})).unwrap();
        assert_eq!((request.method, request.path.as_str()), ("POST", "/api/v1/signatures"));
    }

    #[test]
    fn refuses_hostile_paths() {
        for path in ["", "/etc/passwd", "a//b", "a/../b", "."] {
            let result = request_for(&json!({"type": "store-file", "path": path, "content": 1}));
            assert!(matches!(result, Err(CloudError::BadPath(_))), "path {path:?} was accepted");
        }
    }

    #[test]
    fn refuses_values_that_are_not_operations() {
        assert!(matches!(request_for(&json!({"type": "rm-rf"})), Err(CloudError::NotAnOperation(_))));
    }

    struct Fake(u16);

    impl CloudTransport for Fake {
        fn send(&mut self, _request: &CloudRequest) -> Result<CloudReply, TransportError> {
            Ok(CloudReply { status: self.0, body: json!({"error": "nope", "stored": true}) })
        }
    }

    #[test]
    fn the_connector_surfaces_service_errors() {
        let mut ok = CloudConnector::new(Fake(201));
        assert!(ok.cloud(&json!({"type": "share", "path": "a/b", "with": "team"})).is_ok());
        let mut bad = CloudConnector::new(Fake(403));
        let error = bad.cloud(&json!({"type": "share", "path": "a/b", "with": "team"})).unwrap_err();
        assert!(error.message.contains("403"), "{error}");
    }
}
