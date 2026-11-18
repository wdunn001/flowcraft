//! The Craft app control protocol, as every app documents it in `docs/control-protocol.md`:
//! one JSON object per line to a loopback port, one reply line each, authorized by a key
//! from `<settings>/control-key.<instance>`.
//!
//! This module builds request lines and parses reply lines. A [`ControlSessions`]
//! implementation owns the sockets and the keys; it should authenticate once per
//! connection with [`auth_line`], so request lines built here never carry the key.

use crate::TransportError;
use flowcraft_engine::{Connector, ConnectorError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The apps close the connection on a line over 1 MiB.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// The name of the key file an app writes next to its preferences, outside Flatpak.
pub fn key_file_name(port: u16) -> String {
    format!("control-key.local-{port}")
}

/// Why a control line could not be built or a reply could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("the request line is {0} bytes; the apps close the connection over 1 MiB")]
    LineTooLong(usize),
    #[error("the request does not serialize: {0}")]
    BadRequest(serde_json::Error),
    #[error("the reply is not valid JSON: {0}")]
    BadReply(serde_json::Error),
    #[error("the app refused the request: {0}")]
    Refused(String),
    #[error("the reply answers id {got:?}, not {want}")]
    WrongId { want: u64, got: Option<u64> },
}

#[derive(Serialize)]
struct Envelope<'a> {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<&'a str>,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

fn to_line(envelope: &Envelope<'_>) -> Result<String, ControlError> {
    let line = serde_json::to_string(envelope).map_err(ControlError::BadRequest)?;
    if line.len() > MAX_LINE_BYTES {
        return Err(ControlError::LineTooLong(line.len()));
    }
    Ok(line)
}

/// One `engine.execute` request line, without a key.
pub fn execute_line(id: u64, command: &str, params: &Value) -> Result<String, ControlError> {
    let body = serde_json::json!({"command": command, "params": params});
    to_line(&Envelope { id, key: None, method: "engine.execute", params: Some(body) })
}

/// One `auth` request line, sent first on each connection so later lines need no key.
pub fn auth_line(id: u64, key: &str) -> Result<String, ControlError> {
    let body = serde_json::json!({"key": key});
    to_line(&Envelope { id, key: None, method: "auth", params: Some(body) })
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    id: Option<u64>,
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<String>,
}

/// Reads one reply line, checks it answers `want_id`, and returns its result.
pub fn parse_reply(line: &str, want_id: u64) -> Result<Value, ControlError> {
    let reply: Reply = serde_json::from_str(line).map_err(ControlError::BadReply)?;
    if !reply.ok {
        return Err(ControlError::Refused(reply.error.unwrap_or_else(|| "no error message".into())));
    }
    if reply.id != Some(want_id) {
        return Err(ControlError::WrongId { want: want_id, got: reply.id });
    }
    Ok(reply.result)
}

/// Owns the connections to the apps' control ports. `flowcraft-cli` implements this over
/// TCP to `127.0.0.1`; tests implement it in memory.
pub trait ControlSessions {
    /// Sends one request line to `app`'s control port and returns the one reply line.
    fn round_trip(&mut self, app: &str, line: &str) -> Result<String, TransportError>;
}

/// A [`Connector`] for app commands. Cloud operations need a `CloudConnector`; pair the
/// two with `SuiteConnector`.
pub struct AppConnector<S> {
    sessions: S,
    next_id: u64,
}

impl<S: ControlSessions> AppConnector<S> {
    pub fn new(sessions: S) -> Self {
        Self { sessions, next_id: 1 }
    }
}

impl<S: ControlSessions> Connector for AppConnector<S> {
    fn app_command(&mut self, app: &str, command: &str, params: &Value) -> Result<Value, ConnectorError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let line = execute_line(id, command, params).map_err(|error| ConnectorError::new(error.to_string()))?;
        let reply = self.sessions.round_trip(app, &line).map_err(|error| ConnectorError::new(format!("{app}: {error}")))?;
        parse_reply(&reply, id).map_err(|error| ConnectorError::new(format!("{app}: {error}")))
    }

    fn cloud(&mut self, _operation: &Value) -> Result<Value, ConnectorError> {
        Err(ConnectorError::new("this connector reaches apps only; cloud operations need a CollabCraft transport"))
    }
}

#[cfg(test)]
mod tests {
    use super::{AppConnector, ControlError, ControlSessions, MAX_LINE_BYTES, auth_line, execute_line, key_file_name, parse_reply};
    use crate::TransportError;
    use flowcraft_engine::Connector;
    use serde_json::json;

    #[test]
    fn builds_the_documented_envelope() {
        let line = execute_line(1, "text.insert", &json!({"text": "Hello"})).unwrap();
        assert_eq!(line, r#"{"id":1,"method":"engine.execute","params":{"command":"text.insert","params":{"text":"Hello"}}}"#);
        let auth = auth_line(1, "k").unwrap();
        assert_eq!(auth, r#"{"id":1,"method":"auth","params":{"key":"k"}}"#);
    }

    #[test]
    fn refuses_lines_over_the_cap() {
        let text = "x".repeat(MAX_LINE_BYTES);
        assert!(matches!(execute_line(1, "c", &json!({"text": text})), Err(ControlError::LineTooLong(_))));
    }

    #[test]
    fn parses_replies() {
        assert_eq!(parse_reply(r#"{"id":7,"ok":true,"result":{"a":1}}"#, 7).unwrap(), json!({"a":1}));
        assert!(matches!(parse_reply(r#"{"id":7,"ok":false,"error":"unauthorized"}"#, 7), Err(ControlError::Refused(_))));
        assert!(matches!(parse_reply(r#"{"id":8,"ok":true,"result":{}}"#, 7), Err(ControlError::WrongId { .. })));
        assert!(matches!(parse_reply("not json", 7), Err(ControlError::BadReply(_))));
    }

    #[test]
    fn names_the_key_file_like_the_apps_do() {
        assert_eq!(key_file_name(7981), "control-key.local-7981");
    }

    struct Fake;

    impl ControlSessions for Fake {
        fn round_trip(&mut self, app: &str, line: &str) -> Result<String, TransportError> {
            assert_eq!(app, "wordcraft");
            let sent: serde_json::Value = serde_json::from_str(line).unwrap();
            let id = sent.get("id").and_then(serde_json::Value::as_u64).unwrap();
            Ok(format!(r#"{{"id":{id},"ok":true,"result":{{"done":true}}}}"#))
        }
    }

    #[test]
    fn the_connector_round_trips() {
        let mut connector = AppConnector::new(Fake);
        let result = connector.app_command("wordcraft", "text.insert", &json!({"text": "hi"})).unwrap();
        assert_eq!(result, json!({"done": true}));
        assert!(connector.cloud(&json!({})).is_err());
    }
}
