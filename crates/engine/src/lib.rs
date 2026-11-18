//! Runs a flow against a [`Connector`].
//!
//! The engine owns ordering, conditions and `$ref` resolution. It opens no sockets and
//! spawns no processes: every effect goes through the `Connector` the caller passes in.
//! Real connectors live in `flowcraft-integrations`; tests use a mock.

use flowcraft_flow::refspec::{PathSeg, RefFrom, RefSpecError, as_ref_spec};
use flowcraft_flow::{Action, Condition, Edge, Flow, Step, ValidateError, validate};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Upper bound on value nesting while resolving `$ref` values.
pub const MAX_RESOLVE_DEPTH: usize = 128;

/// The seam to the outside world. One method per kind of effect a step can have.
pub trait Connector {
    /// Runs one engine command in a Craft app (the `engine.execute` envelope).
    fn app_command(&mut self, app: &str, command: &str, params: &Value) -> Result<Value, ConnectorError>;

    /// Calls CollabCraft. `operation` is a resolved, tagged `CloudOperation` value.
    fn cloud(&mut self, operation: &Value) -> Result<Value, ConnectorError>;
}

/// A connector's failure, in words a report can carry.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ConnectorError {
    pub message: String,
}

impl ConnectorError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

/// Why a `$ref` could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum RefError {
    #[error(transparent)]
    Spec(#[from] RefSpecError),
    #[error("references step {0:?}, which did not run")]
    SourceNotRun(String),
    #[error("path misses at {0}")]
    PathMiss(String),
    #[error("values nest deeper than {MAX_RESOLVE_DEPTH}")]
    TooDeep,
}

/// Why a run stopped. A failed step stops the run; the report up to that point is lost on
/// purpose, so a caller never mistakes a partial run for a finished one.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Validate(#[from] ValidateError),
    #[error("step {step:?}: {source}")]
    Ref {
        step: String,
        #[source]
        source: RefError,
    },
    #[error("the condition on {from:?} to {to:?} cannot be evaluated: {detail}")]
    Condition { from: String, to: String, detail: String },
    #[error("step {step:?} failed: {source}")]
    Connector {
        step: String,
        #[source]
        source: ConnectorError,
    },
    #[error("internal: {0}")]
    Internal(String),
}

/// What happened to each step, in execution order.
#[derive(Debug, Clone, PartialEq)]
pub struct RunReport {
    pub flow_id: String,
    pub steps: Vec<StepRun>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepRun {
    pub id: String,
    pub status: StepStatus,
    pub output: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Ran,
    Skipped,
}

struct RunState {
    trigger: Value,
    outputs: BTreeMap<String, Value>,
}

/// Validates the flow, orders its steps, and runs them through `connector`.
///
/// A step runs when it has no incoming edges, or when at least one incoming edge comes from
/// a step that ran and whose condition (if any) holds. Everything else is skipped.
pub fn run(flow: &Flow, trigger_payload: &Value, connector: &mut dyn Connector) -> Result<RunReport, RunError> {
    validate(flow)?;
    let order = topo_order(flow).map_err(|id| RunError::Internal(format!("cycle through {id:?} survived validation")))?;
    let incoming = incoming_edges(flow);
    let mut state = RunState { trigger: trigger_payload.clone(), outputs: BTreeMap::new() };
    let mut ran: BTreeSet<&str> = BTreeSet::new();
    let mut report = RunReport { flow_id: flow.id.clone(), steps: Vec::new() };
    for step in order {
        let edges = incoming.get(step.id.as_str()).map(Vec::as_slice).unwrap_or_default();
        if !should_run(edges, &ran, &state)? {
            report.steps.push(StepRun { id: step.id.clone(), status: StepStatus::Skipped, output: None });
            continue;
        }
        let output = execute(step, &state, connector)?;
        state.outputs.insert(step.id.clone(), output.clone());
        ran.insert(step.id.as_str());
        report.steps.push(StepRun { id: step.id.clone(), status: StepStatus::Ran, output: Some(output) });
    }
    Ok(report)
}

/// Kahn's algorithm, deterministic: ready steps run in document order.
fn topo_order(flow: &Flow) -> Result<Vec<&Step>, String> {
    let position: BTreeMap<&str, usize> = flow.steps.iter().enumerate().map(|(index, step)| (step.id.as_str(), index)).collect();
    let mut indegree: BTreeMap<&str, usize> = flow.steps.iter().map(|step| (step.id.as_str(), 0)).collect();
    let mut out: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &flow.edges {
        out.entry(edge.from.as_str()).or_default().push(edge.to.as_str());
        if let Some(count) = indegree.get_mut(edge.to.as_str()) {
            *count += 1;
        }
    }
    let mut ready: BTreeSet<(usize, &str)> =
        indegree.iter().filter(|(_, count)| **count == 0).filter_map(|(id, _)| position.get(id).map(|index| (*index, *id))).collect();
    let mut order = Vec::with_capacity(flow.steps.len());
    while let Some((index, id)) = ready.pop_first() {
        if let Some(step) = flow.steps.get(index) {
            order.push(step);
        }
        for next in out.get(id).into_iter().flatten() {
            if let Some(count) = indegree.get_mut(next) {
                *count -= 1;
                if *count == 0
                    && let Some(next_index) = position.get(next)
                {
                    ready.insert((*next_index, next));
                }
            }
        }
    }
    if order.len() < flow.steps.len() {
        return Err(indegree.iter().find(|(_, count)| **count > 0).map(|(id, _)| (*id).to_string()).unwrap_or_default());
    }
    Ok(order)
}

fn incoming_edges(flow: &Flow) -> BTreeMap<&str, Vec<&Edge>> {
    let mut incoming: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();
    for edge in &flow.edges {
        incoming.entry(edge.to.as_str()).or_default().push(edge);
    }
    incoming
}

fn should_run(edges: &[&Edge], ran: &BTreeSet<&str>, state: &RunState) -> Result<bool, RunError> {
    if edges.is_empty() {
        return Ok(true);
    }
    for edge in edges {
        if !ran.contains(edge.from.as_str()) {
            continue;
        }
        let holds = match &edge.when {
            None => true,
            Some(condition) => {
                eval_condition(condition, state).map_err(|detail| RunError::Condition { from: edge.from.clone(), to: edge.to.clone(), detail })?
            }
        };
        if holds {
            return Ok(true);
        }
    }
    Ok(false)
}

fn execute(step: &Step, state: &RunState, connector: &mut dyn Connector) -> Result<Value, RunError> {
    let fail_ref = |source: RefError| RunError::Ref { step: step.id.clone(), source };
    match &step.action {
        Action::AppCommand { app, command, params } => {
            let params = resolve(params, state, 0).map_err(fail_ref)?;
            connector.app_command(app, command, &params).map_err(|source| RunError::Connector { step: step.id.clone(), source })
        }
        Action::Cloud { operation } => {
            let raw = serde_json::to_value(operation).map_err(|error| RunError::Internal(format!("cloud operation does not serialize: {error}")))?;
            let resolved = resolve(&raw, state, 0).map_err(fail_ref)?;
            connector.cloud(&resolved).map_err(|source| RunError::Connector { step: step.id.clone(), source })
        }
        Action::Compose { values } => {
            let mut object = Map::new();
            for (key, value) in values {
                object.insert(key.clone(), resolve(value, state, 0).map_err(&fail_ref)?);
            }
            Ok(Value::Object(object))
        }
    }
}

fn resolve(value: &Value, state: &RunState, depth: usize) -> Result<Value, RefError> {
    if depth > MAX_RESOLVE_DEPTH {
        return Err(RefError::TooDeep);
    }
    if let Some(spec) = as_ref_spec(value)? {
        let root = match spec.from {
            RefFrom::Trigger => &state.trigger,
            RefFrom::Step => {
                let Some(id) = spec.step.as_deref() else {
                    return Err(RefError::Spec(RefSpecError::MissingStep));
                };
                state.outputs.get(id).ok_or_else(|| RefError::SourceNotRun(id.to_string()))?
            }
        };
        let mut current = root;
        for seg in &spec.path {
            current = match seg {
                PathSeg::Key(key) => current.get(key),
                PathSeg::Index(index) => current.get(index),
            }
            .ok_or_else(|| RefError::PathMiss(seg_name(seg)))?;
        }
        return Ok(current.clone());
    }
    match value {
        Value::Array(items) => items.iter().map(|item| resolve(item, state, depth + 1)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        Value::Object(entries) => {
            let mut object = Map::new();
            for (key, entry) in entries {
                object.insert(key.clone(), resolve(entry, state, depth + 1)?);
            }
            Ok(Value::Object(object))
        }
        other => Ok(other.clone()),
    }
}

fn seg_name(seg: &PathSeg) -> String {
    match seg {
        PathSeg::Key(key) => format!("key {key:?}"),
        PathSeg::Index(index) => format!("index {index}"),
    }
}

fn eval_condition(condition: &Condition, state: &RunState) -> Result<bool, String> {
    match condition {
        Condition::Eq { left, right } => Ok(pair(left, right, state)?.into_eq()),
        Condition::Ne { left, right } => Ok(!pair(left, right, state)?.into_eq()),
        Condition::Gt { left, right } => pair(left, right, state)?.into_numeric().map(|(a, b)| a > b),
        Condition::Lt { left, right } => pair(left, right, state)?.into_numeric().map(|(a, b)| a < b),
        Condition::Contains { haystack, needle } => {
            let resolved = pair(haystack, needle, state)?;
            contains(&resolved.0, &resolved.1)
        }
        Condition::All { conditions } => {
            for nested in conditions {
                if !eval_condition(nested, state)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Condition::Any { conditions } => {
            for nested in conditions {
                if eval_condition(nested, state)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Condition::Not { condition } => Ok(!eval_condition(condition, state)?),
    }
}

struct Pair(Value, Value);

impl Pair {
    fn into_eq(self) -> bool {
        self.0 == self.1
    }

    fn into_numeric(self) -> Result<(f64, f64), String> {
        match (self.0.as_f64(), self.1.as_f64()) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => Err("both sides of a numeric comparison must be numbers".into()),
        }
    }
}

fn pair(left: &Value, right: &Value, state: &RunState) -> Result<Pair, String> {
    let left = resolve(left, state, 0).map_err(|error| error.to_string())?;
    let right = resolve(right, state, 0).map_err(|error| error.to_string())?;
    Ok(Pair(left, right))
}

fn contains(haystack: &Value, needle: &Value) -> Result<bool, String> {
    match haystack {
        Value::String(text) => match needle.as_str() {
            Some(sub) => Ok(text.contains(sub)),
            None => Err("a string haystack needs a string needle".into()),
        },
        Value::Array(items) => Ok(items.contains(needle)),
        Value::Object(entries) => match needle.as_str() {
            Some(key) => Ok(entries.contains_key(key)),
            None => Err("an object haystack needs a string key as the needle".into()),
        },
        _ => Err("contains works on strings, arrays and objects".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Connector, ConnectorError, RunError, StepStatus, run};
    use flowcraft_flow::{Action, Condition, Edge, FORMAT_VERSION, Flow, Step, Trigger};
    use serde_json::{Value, json};

    struct Mock {
        calls: Vec<(String, String, Value)>,
        fail_on: Option<String>,
    }

    impl Mock {
        fn new() -> Self {
            Self { calls: Vec::new(), fail_on: None }
        }
    }

    impl Connector for Mock {
        fn app_command(&mut self, app: &str, command: &str, params: &Value) -> Result<Value, ConnectorError> {
            if self.fail_on.as_deref() == Some(command) {
                return Err(ConnectorError::new("boom"));
            }
            self.calls.push((app.to_string(), command.to_string(), params.clone()));
            Ok(json!({"ok": true, "command": command}))
        }

        fn cloud(&mut self, operation: &Value) -> Result<Value, ConnectorError> {
            self.calls.push(("cloud".into(), "cloud".into(), operation.clone()));
            Ok(json!({"stored": true}))
        }
    }

    fn approval_flow() -> Flow {
        let gate = |over: bool| {
            let base = Condition::Gt { left: json!({"$ref": {"from": "step", "step": "check", "path": ["amount"]}}), right: json!(100) };
            if over { base } else { Condition::Not { condition: Box::new(base) } }
        };
        Flow {
            format_version: FORMAT_VERSION,
            id: "approval".into(),
            name: "Approval".into(),
            description: String::new(),
            trigger: Trigger::Manual,
            steps: vec![
                Step {
                    id: "check".into(),
                    name: String::new(),
                    action: Action::Compose {
                        values: [("amount".to_string(), json!({"$ref": {"from": "trigger", "path": ["amount"]}}))].into_iter().collect(),
                    },
                },
                Step {
                    id: "escalate".into(),
                    name: String::new(),
                    action: Action::AppCommand { app: "wordcraft".into(), command: "review.open".into(), params: json!({}) },
                },
                Step {
                    id: "approve".into(),
                    name: String::new(),
                    action: Action::AppCommand { app: "wordcraft".into(), command: "review.approve".into(), params: json!({}) },
                },
            ],
            edges: vec![
                Edge { from: "check".into(), to: "escalate".into(), when: Some(gate(true)) },
                Edge { from: "check".into(), to: "approve".into(), when: Some(gate(false)) },
            ],
        }
    }

    #[test]
    fn branches_on_the_trigger_payload() {
        let mut mock = Mock::new();
        let report = run(&approval_flow(), &json!({"amount": 250}), &mut mock).unwrap();
        let statuses: Vec<(&str, StepStatus)> = report.steps.iter().map(|step| (step.id.as_str(), step.status)).collect();
        assert_eq!(statuses, vec![("check", StepStatus::Ran), ("escalate", StepStatus::Ran), ("approve", StepStatus::Skipped)]);

        let mut mock = Mock::new();
        let report = run(&approval_flow(), &json!({"amount": 10}), &mut mock).unwrap();
        let approve = report.steps.iter().find(|step| step.id == "approve").unwrap();
        assert_eq!(approve.status, StepStatus::Ran);
    }

    #[test]
    fn downstream_of_a_skipped_step_is_skipped() {
        let mut flow = approval_flow();
        flow.steps.push(Step {
            id: "after-escalate".into(),
            name: String::new(),
            action: Action::AppCommand { app: "wordcraft".into(), command: "review.remind".into(), params: json!({}) },
        });
        flow.edges.push(Edge { from: "escalate".into(), to: "after-escalate".into(), when: None });
        let mut mock = Mock::new();
        let report = run(&flow, &json!({"amount": 10}), &mut mock).unwrap();
        let tail = report.steps.iter().find(|step| step.id == "after-escalate").unwrap();
        assert_eq!(tail.status, StepStatus::Skipped);
    }

    #[test]
    fn a_connector_failure_names_the_step() {
        let mut mock = Mock::new();
        mock.fail_on = Some("review.open".into());
        let error = run(&approval_flow(), &json!({"amount": 250}), &mut mock).unwrap_err();
        match error {
            RunError::Connector { step, .. } => assert_eq!(step, "escalate"),
            other => panic!("wanted Connector, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_ref_path_fails_the_step() {
        let error = run(&approval_flow(), &json!({"wrong": 1}), &mut Mock::new()).unwrap_err();
        match error {
            RunError::Ref { step, .. } => assert_eq!(step, "check"),
            other => panic!("wanted Ref, got {other:?}"),
        }
    }

    #[test]
    fn cloud_operations_reach_the_connector_resolved() {
        let flow = Flow {
            format_version: FORMAT_VERSION,
            id: "store".into(),
            name: "Store".into(),
            description: String::new(),
            trigger: Trigger::Manual,
            steps: vec![Step {
                id: "store".into(),
                name: String::new(),
                action: Action::Cloud {
                    operation: flowcraft_flow::CloudOperation::StoreFile {
                        path: "signed/contract.pdf".into(),
                        content: json!({"$ref": {"from": "trigger", "path": ["body"]}}),
                    },
                },
            }],
            edges: Vec::new(),
        };
        let mut mock = Mock::new();
        run(&flow, &json!({"body": "PDFDATA"}), &mut mock).unwrap();
        assert_eq!(mock.calls[0].2, json!({"type": "store-file", "path": "signed/contract.pdf", "content": "PDFDATA"}));
    }
}
