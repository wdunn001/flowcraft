//! `$ref` values: how a step reads the trigger payload or an earlier step's output.
//!
//! Anywhere a flow carries a JSON value (step params, compose values, cloud operations,
//! condition operands), an object with the single key `$ref` is replaced at run time:
//!
//! ```json
//! {"$ref": {"from": "trigger", "path": ["document", "url"]}}
//! {"$ref": {"from": "step", "step": "export", "path": ["file", 0]}}
//! ```
//!
//! There is no string expression language on purpose: references stay structured, so a
//! builder UI can offer and validate them without a parser.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where a reference reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefFrom {
    Trigger,
    Step,
}

/// One step into a JSON value: an object key or an array index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathSeg {
    Key(String),
    Index(usize),
}

/// A parsed `$ref` object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefSpec {
    pub from: RefFrom,
    /// The source step's id. Required when `from` is `step`, forbidden otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<PathSeg>,
}

/// A `$ref` object that does not parse.
#[derive(Debug, thiserror::Error)]
pub enum RefSpecError {
    #[error("malformed $ref: {0}")]
    Malformed(serde_json::Error),
    #[error("a $ref with from \"step\" names no step")]
    MissingStep,
    #[error("a $ref with from \"trigger\" must not name a step")]
    StepOnTrigger,
}

/// Reads the reference out of a value shaped `{"$ref": {...}}`.
///
/// Returns `Ok(None)` for every other shape, including objects that carry `$ref` next to
/// other keys (those stay literal). An object whose only key is `$ref` but whose body does
/// not parse is an error, so a typo fails a run instead of passing through as data.
pub fn as_ref_spec(value: &Value) -> Result<Option<RefSpec>, RefSpecError> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    if object.len() != 1 {
        return Ok(None);
    }
    let Some(body) = object.get("$ref") else {
        return Ok(None);
    };
    let spec: RefSpec = serde_json::from_value(body.clone()).map_err(RefSpecError::Malformed)?;
    match (spec.from, &spec.step) {
        (RefFrom::Step, None) => Err(RefSpecError::MissingStep),
        (RefFrom::Trigger, Some(_)) => Err(RefSpecError::StepOnTrigger),
        _ => Ok(Some(spec)),
    }
}

/// Walks `value` and collects every `$ref` in it, malformed ones included as errors.
pub fn collect_refs(value: &Value, found: &mut Vec<RefSpec>) -> Result<(), RefSpecError> {
    if let Some(spec) = as_ref_spec(value)? {
        found.push(spec);
        return Ok(());
    }
    match value {
        Value::Array(items) => {
            for item in items {
                collect_refs(item, found)?;
            }
        }
        Value::Object(entries) => {
            for entry in entries.values() {
                collect_refs(entry, found)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PathSeg, RefFrom, RefSpecError, as_ref_spec, collect_refs};
    use serde_json::json;

    #[test]
    fn parses_a_trigger_ref() {
        let value = json!({"$ref": {"from": "trigger", "path": ["document", 2]}});
        let spec = as_ref_spec(&value).unwrap().unwrap();
        assert_eq!(spec.from, RefFrom::Trigger);
        assert_eq!(spec.path, vec![PathSeg::Key("document".into()), PathSeg::Index(2)]);
    }

    #[test]
    fn leaves_plain_objects_alone() {
        assert!(as_ref_spec(&json!({"a": 1})).unwrap().is_none());
        assert!(as_ref_spec(&json!({"$ref": {"from": "trigger"}, "b": 1})).unwrap().is_none());
        assert!(as_ref_spec(&json!(7)).unwrap().is_none());
    }

    #[test]
    fn rejects_malformed_refs() {
        assert!(matches!(as_ref_spec(&json!({"$ref": 5})), Err(RefSpecError::Malformed(_))));
        assert!(matches!(as_ref_spec(&json!({"$ref": {"from": "step"}})), Err(RefSpecError::MissingStep)));
        assert!(matches!(as_ref_spec(&json!({"$ref": {"from": "trigger", "step": "x"}})), Err(RefSpecError::StepOnTrigger)));
    }

    #[test]
    fn collects_nested_refs() {
        let value = json!({"a": [{"$ref": {"from": "trigger"}}], "b": {"$ref": {"from": "step", "step": "s1"}}});
        let mut found = Vec::new();
        collect_refs(&value, &mut found).unwrap();
        assert_eq!(found.len(), 2);
    }
}
