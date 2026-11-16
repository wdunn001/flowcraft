//! The FlowCraft flow document: triggers, steps, edges and conditions.
//!
//! This crate holds the file format and its validation. It does no IO and opens no sockets.
//! Execution semantics live in `flowcraft-engine`; the seams to real apps and to CollabCraft
//! live in `flowcraft-connectors`.

pub mod refspec;
mod validate;

pub use validate::{ValidateError, validate};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The newest flow file format this build reads and writes.
pub const FORMAT_VERSION: u32 = 1;
/// Upper bound on steps in one flow. Flow files are hostile input (AGENTS.md, Never crash).
pub const MAX_STEPS: usize = 4096;
/// Upper bound on edges in one flow.
pub const MAX_EDGES: usize = 16 * 1024;
/// Upper bound on condition nesting.
pub const MAX_CONDITION_DEPTH: usize = 64;
/// Upper bound on a serialized flow document, in bytes.
pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

/// A flow: one trigger, steps, and the edges that order them into a DAG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub format_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub trigger: Trigger,
    pub steps: Vec<Step>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<Edge>,
}

/// What starts a run. The payload a trigger delivers is readable through `$ref` values
/// (see [`refspec`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Trigger {
    /// Started by a person or by the CLI.
    Manual,
    /// An event from a Craft app, delivered through CollabCraft (for example `pdfcraft`,
    /// `document.signed`).
    AppEvent { app: String, event: String },
    /// Started on an interval.
    Schedule { every_minutes: u32 },
}

/// One unit of work in a flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub action: Action,
}

/// What a step does. Values inside `params`, `values` and cloud operations may be `$ref`
/// objects that read the trigger payload or an earlier step's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Action {
    /// Runs one engine command in a Craft app over its control port
    /// (the `engine.execute` envelope from the app's `docs/control-protocol.md`).
    AppCommand {
        app: String,
        command: String,
        #[serde(default)]
        params: Value,
    },
    /// Calls CollabCraft: files, sharing, notifications and signatures.
    Cloud { operation: CloudOperation },
    /// Builds an output object from literal values and `$ref` values, without calling anything.
    Compose {
        #[serde(default)]
        values: BTreeMap<String, Value>,
    },
}

/// A CollabCraft operation. The HTTP binding lives in `flowcraft-connectors`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CloudOperation {
    StoreFile { path: String, content: Value },
    Share { path: String, with: String },
    Notify { user: String, message: Value },
    RequestSignature { document: Value, signers: Vec<String> },
    SignatureStatus { request: Value },
}

/// An ordering edge between two steps, optionally gated by a condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Condition>,
}

/// A condition on an edge. `left`, `right`, `haystack` and `needle` may be `$ref` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Condition {
    Eq {
        left: Value,
        right: Value,
    },
    Ne {
        left: Value,
        right: Value,
    },
    /// Numeric comparison; both sides must resolve to numbers.
    Gt {
        left: Value,
        right: Value,
    },
    /// Numeric comparison; both sides must resolve to numbers.
    Lt {
        left: Value,
        right: Value,
    },
    /// String containment, array membership, or object key presence.
    Contains {
        haystack: Value,
        needle: Value,
    },
    All {
        conditions: Vec<Condition>,
    },
    Any {
        conditions: Vec<Condition>,
    },
    Not {
        condition: Box<Condition>,
    },
}

/// Why a flow document could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("flow document is {0} bytes, over the 4 MiB cap")]
    TooLarge(usize),
    #[error("not valid flow JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("flow format version {found} is newer than this build reads (newest is {newest})")]
    UnsupportedVersion { found: u32, newest: u32 },
}

impl Flow {
    /// Reads a flow document, enforcing the size cap and the format version gate.
    /// Call [`validate`] afterwards before executing it.
    pub fn from_json_str(text: &str) -> Result<Self, ParseError> {
        if text.len() > MAX_DOCUMENT_BYTES {
            return Err(ParseError::TooLarge(text.len()));
        }
        let flow: Self = serde_json::from_str(text)?;
        if flow.format_version > FORMAT_VERSION {
            return Err(ParseError::UnsupportedVersion { found: flow.format_version, newest: FORMAT_VERSION });
        }
        Ok(flow)
    }

    /// Writes the flow document as pretty JSON.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Condition, Edge, FORMAT_VERSION, Flow, ParseError, Step, Trigger};
    use serde_json::json;

    fn sample() -> Flow {
        Flow {
            format_version: FORMAT_VERSION,
            id: "signed-to-archive".into(),
            name: "Archive signed documents".into(),
            description: String::new(),
            trigger: Trigger::AppEvent { app: "pdfcraft".into(), event: "document.signed".into() },
            steps: vec![
                Step {
                    id: "export".into(),
                    name: "Export the signed PDF".into(),
                    action: Action::AppCommand {
                        app: "pdfcraft".into(),
                        command: "document.export".into(),
                        params: json!({"format": "pdf", "document": {"$ref": {"from": "trigger", "path": ["document"]}}}),
                    },
                },
                Step {
                    id: "note".into(),
                    name: String::new(),
                    action: Action::Compose { values: [("ok".to_string(), json!(true))].into_iter().collect() },
                },
            ],
            edges: vec![Edge {
                from: "export".into(),
                to: "note".into(),
                when: Some(Condition::Eq { left: json!({"$ref": {"from": "step", "step": "export", "path": ["ok"]}}), right: json!(true) }),
            }],
        }
    }

    #[test]
    fn round_trips_through_json() {
        let flow = sample();
        let text = flow.to_json_string().unwrap();
        let back = Flow::from_json_str(&text).unwrap();
        assert_eq!(flow, back);
    }

    #[test]
    fn rejects_newer_format_versions() {
        let mut flow = sample();
        flow.format_version = FORMAT_VERSION + 1;
        let text = flow.to_json_string().unwrap();
        match Flow::from_json_str(&text) {
            Err(ParseError::UnsupportedVersion { found, newest }) => {
                assert_eq!(found, FORMAT_VERSION + 1);
                assert_eq!(newest, FORMAT_VERSION);
            }
            other => panic!("wanted UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_top_level_fields() {
        let text = r#"{"format_version": 1, "id": "x", "name": "x", "surprise": 1, "trigger": {"type": "manual"}, "steps": []}"#;
        assert!(matches!(Flow::from_json_str(text), Err(ParseError::Json(_))));
    }

    #[test]
    fn rejects_oversized_documents() {
        let text = " ".repeat(super::MAX_DOCUMENT_BYTES + 1);
        assert!(matches!(Flow::from_json_str(&text), Err(ParseError::TooLarge(_))));
    }
}
