//! Structural validation of a flow document, before any execution.

use crate::refspec::{RefFrom, RefSpec, RefSpecError, collect_refs};
use crate::{Action, CloudOperation, Condition, Edge, Flow, MAX_CONDITION_DEPTH, MAX_EDGES, MAX_STEPS};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Why a flow document cannot run.
#[derive(Debug, thiserror::Error)]
pub enum ValidateError {
    #[error("the flow has an empty id")]
    EmptyFlowId,
    #[error("the flow has {0} steps, over the cap of {MAX_STEPS}")]
    TooManySteps(usize),
    #[error("the flow has {0} edges, over the cap of {MAX_EDGES}")]
    TooManyEdges(usize),
    #[error("a step has an empty id")]
    EmptyStepId,
    #[error("two steps share the id {0:?}")]
    DuplicateStepId(String),
    #[error("an edge names the unknown step {0:?}")]
    UnknownEdgeStep(String),
    #[error("the edge on {0:?} points at itself")]
    SelfEdge(String),
    #[error("the edges contain a cycle through {0:?}")]
    Cycle(String),
    #[error("the condition on the edge {from:?} to {to:?} nests deeper than {MAX_CONDITION_DEPTH}")]
    ConditionTooDeep { from: String, to: String },
    #[error("step {step:?} carries a bad reference: {source}")]
    BadRef {
        step: String,
        #[source]
        source: RefSpecError,
    },
    #[error("the edge {from:?} to {to:?} carries a bad reference: {source}")]
    BadEdgeRef {
        from: String,
        to: String,
        #[source]
        source: RefSpecError,
    },
    #[error("step {step:?} references the unknown step {missing:?}")]
    UnknownStepRef { step: String, missing: String },
    #[error("the edge {from:?} to {to:?} references the unknown step {missing:?}")]
    UnknownEdgeStepRef { from: String, to: String, missing: String },
}

/// Checks a flow's structure: caps, ids, edge endpoints, acyclicity, condition depth and
/// every `$ref` it carries. A flow that passes can be handed to `flowcraft-engine`.
pub fn validate(flow: &Flow) -> Result<(), ValidateError> {
    if flow.id.is_empty() {
        return Err(ValidateError::EmptyFlowId);
    }
    if flow.steps.len() > MAX_STEPS {
        return Err(ValidateError::TooManySteps(flow.steps.len()));
    }
    if flow.edges.len() > MAX_EDGES {
        return Err(ValidateError::TooManyEdges(flow.edges.len()));
    }
    let ids = step_ids(flow)?;
    check_edges(flow, &ids)?;
    check_acyclic(flow)?;
    check_refs(flow, &ids)?;
    Ok(())
}

fn step_ids(flow: &Flow) -> Result<BTreeSet<&str>, ValidateError> {
    let mut ids = BTreeSet::new();
    for step in &flow.steps {
        if step.id.is_empty() {
            return Err(ValidateError::EmptyStepId);
        }
        if !ids.insert(step.id.as_str()) {
            return Err(ValidateError::DuplicateStepId(step.id.clone()));
        }
    }
    Ok(ids)
}

fn check_edges(flow: &Flow, ids: &BTreeSet<&str>) -> Result<(), ValidateError> {
    for edge in &flow.edges {
        for end in [&edge.from, &edge.to] {
            if !ids.contains(end.as_str()) {
                return Err(ValidateError::UnknownEdgeStep(end.clone()));
            }
        }
        if edge.from == edge.to {
            return Err(ValidateError::SelfEdge(edge.from.clone()));
        }
        if let Some(condition) = &edge.when {
            check_condition_depth(condition, edge, 1)?;
        }
    }
    Ok(())
}

fn check_condition_depth(condition: &Condition, edge: &Edge, depth: usize) -> Result<(), ValidateError> {
    if depth > MAX_CONDITION_DEPTH {
        return Err(ValidateError::ConditionTooDeep { from: edge.from.clone(), to: edge.to.clone() });
    }
    match condition {
        Condition::All { conditions } | Condition::Any { conditions } => {
            for nested in conditions {
                check_condition_depth(nested, edge, depth + 1)?;
            }
        }
        Condition::Not { condition } => check_condition_depth(condition, edge, depth + 1)?,
        _ => {}
    }
    Ok(())
}

fn check_acyclic(flow: &Flow) -> Result<(), ValidateError> {
    let mut indegree: BTreeMap<&str, usize> = flow.steps.iter().map(|step| (step.id.as_str(), 0)).collect();
    let mut out: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &flow.edges {
        out.entry(edge.from.as_str()).or_default().push(edge.to.as_str());
        if let Some(count) = indegree.get_mut(edge.to.as_str()) {
            *count += 1;
        }
    }
    let mut ready: Vec<&str> = indegree.iter().filter(|(_, count)| **count == 0).map(|(id, _)| *id).collect();
    let mut done = 0usize;
    while let Some(id) = ready.pop() {
        done += 1;
        for next in out.get(id).into_iter().flatten() {
            if let Some(count) = indegree.get_mut(next) {
                *count -= 1;
                if *count == 0 {
                    ready.push(next);
                }
            }
        }
    }
    if done < flow.steps.len() {
        let stuck = indegree.iter().find(|(_, count)| **count > 0).map(|(id, _)| (*id).to_string()).unwrap_or_default();
        return Err(ValidateError::Cycle(stuck));
    }
    Ok(())
}

fn action_values(action: &Action) -> Vec<&Value> {
    match action {
        Action::AppCommand { params, .. } => vec![params],
        Action::Compose { values } => values.values().collect(),
        Action::Cloud { operation } => match operation {
            CloudOperation::StoreFile { content, .. } => vec![content],
            CloudOperation::Share { .. } => Vec::new(),
            CloudOperation::Notify { message, .. } => vec![message],
            CloudOperation::RequestSignature { document, .. } => vec![document],
            CloudOperation::SignatureStatus { request } => vec![request],
        },
    }
}

fn condition_values(condition: &Condition, found: &mut Vec<RefSpec>) -> Result<(), RefSpecError> {
    match condition {
        Condition::Eq { left, right } | Condition::Ne { left, right } | Condition::Gt { left, right } | Condition::Lt { left, right } => {
            collect_refs(left, found)?;
            collect_refs(right, found)?;
        }
        Condition::Contains { haystack, needle } => {
            collect_refs(haystack, found)?;
            collect_refs(needle, found)?;
        }
        Condition::All { conditions } | Condition::Any { conditions } => {
            for nested in conditions {
                condition_values(nested, found)?;
            }
        }
        Condition::Not { condition } => condition_values(condition, found)?,
    }
    Ok(())
}

fn unknown_step_ref(refs: &[RefSpec], ids: &BTreeSet<&str>) -> Option<String> {
    refs.iter().filter(|spec| spec.from == RefFrom::Step).filter_map(|spec| spec.step.as_deref()).find(|step| !ids.contains(step)).map(str::to_string)
}

fn check_refs(flow: &Flow, ids: &BTreeSet<&str>) -> Result<(), ValidateError> {
    for step in &flow.steps {
        let mut refs = Vec::new();
        for value in action_values(&step.action) {
            collect_refs(value, &mut refs).map_err(|source| ValidateError::BadRef { step: step.id.clone(), source })?;
        }
        if let Some(missing) = unknown_step_ref(&refs, ids) {
            return Err(ValidateError::UnknownStepRef { step: step.id.clone(), missing });
        }
    }
    for edge in &flow.edges {
        let mut refs = Vec::new();
        if let Some(condition) = &edge.when {
            condition_values(condition, &mut refs).map_err(|source| ValidateError::BadEdgeRef {
                from: edge.from.clone(),
                to: edge.to.clone(),
                source,
            })?;
        }
        if let Some(missing) = unknown_step_ref(&refs, ids) {
            return Err(ValidateError::UnknownEdgeStepRef { from: edge.from.clone(), to: edge.to.clone(), missing });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ValidateError, validate};
    use crate::{Action, Condition, Edge, FORMAT_VERSION, Flow, Step, Trigger};
    use serde_json::json;

    fn step(id: &str) -> Step {
        Step { id: id.into(), name: String::new(), action: Action::Compose { values: Default::default() } }
    }

    fn flow(steps: Vec<Step>, edges: Vec<Edge>) -> Flow {
        Flow { format_version: FORMAT_VERSION, id: "f".into(), name: "f".into(), description: String::new(), trigger: Trigger::Manual, steps, edges }
    }

    #[test]
    fn accepts_a_linear_flow() {
        let f = flow(vec![step("a"), step("b")], vec![Edge { from: "a".into(), to: "b".into(), when: None }]);
        validate(&f).unwrap();
    }

    #[test]
    fn rejects_duplicate_step_ids() {
        let f = flow(vec![step("a"), step("a")], Vec::new());
        assert!(matches!(validate(&f), Err(ValidateError::DuplicateStepId(_))));
    }

    #[test]
    fn rejects_cycles() {
        let f = flow(
            vec![step("a"), step("b")],
            vec![Edge { from: "a".into(), to: "b".into(), when: None }, Edge { from: "b".into(), to: "a".into(), when: None }],
        );
        assert!(matches!(validate(&f), Err(ValidateError::Cycle(_))));
    }

    #[test]
    fn rejects_unknown_edge_steps_and_self_edges() {
        let f = flow(vec![step("a")], vec![Edge { from: "a".into(), to: "ghost".into(), when: None }]);
        assert!(matches!(validate(&f), Err(ValidateError::UnknownEdgeStep(_))));
        let f = flow(vec![step("a")], vec![Edge { from: "a".into(), to: "a".into(), when: None }]);
        assert!(matches!(validate(&f), Err(ValidateError::SelfEdge(_))));
    }

    #[test]
    fn rejects_refs_to_unknown_steps() {
        let mut bad = step("a");
        bad.action = Action::AppCommand {
            app: "wordcraft".into(),
            command: "text.insert".into(),
            params: json!({"text": {"$ref": {"from": "step", "step": "ghost"}}}),
        };
        let f = flow(vec![bad], Vec::new());
        assert!(matches!(validate(&f), Err(ValidateError::UnknownStepRef { .. })));
    }

    #[test]
    fn rejects_conditions_nested_past_the_cap() {
        let mut condition = Condition::Eq { left: json!(1), right: json!(1) };
        for _ in 0..crate::MAX_CONDITION_DEPTH + 1 {
            condition = Condition::Not { condition: Box::new(condition) };
        }
        let f = flow(vec![step("a"), step("b")], vec![Edge { from: "a".into(), to: "b".into(), when: Some(condition) }]);
        assert!(matches!(validate(&f), Err(ValidateError::ConditionTooDeep { .. })));
    }
}
