//! The flow drawn: a deterministic projection of a flow document onto a
//! `craft-flowchart` diagram, auto-arranged left to right.
//!
//! The diagram is derived state: step ids are node ids, the trigger is one terminator
//! node, and a designer stores only layout overrides beside the flow, never meaning.

use craft_flowchart::doc::{Attach, Connector, Diagram, Node};
use craft_flowchart::layout::{LAYER_GAP_Y, LayerBox, layered_layout};
use craft_flowchart::{NodeId, Point, Rect, Routing, Side, Size};
use flowcraft_flow::{Action, Flow, Trigger};
use std::collections::{BTreeMap, BTreeSet};

const STEP_SIZE: Size = Size::new(180.0, 64.0);
const TRIGGER_SIZE: Size = Size::new(160.0, 48.0);
/// Where the drawing's vertical centre line sits.
const ORIGIN: Point = Point::new(40.0, 320.0);

/// The diagram for a flow: one terminator for the trigger, one node per step (shape by
/// action kind), one elbow connector per edge, with conditional edges labelled.
pub fn diagram_for(flow: &Flow) -> Diagram {
    let mut boxes = vec![LayerBox { id: NodeId(0), size: TRIGGER_SIZE }];
    boxes.extend((0..flow.steps.len()).map(|i| LayerBox { id: NodeId(i as u64 + 1), size: STEP_SIZE }));
    let index: BTreeMap<&str, usize> = flow.steps.iter().enumerate().map(|(i, step)| (step.id.as_str(), i + 1)).collect();
    let targets: BTreeSet<&str> = flow.edges.iter().map(|edge| edge.to.as_str()).collect();
    let mut graph_edges: Vec<(usize, usize)> = flow
        .steps
        .iter()
        .filter(|step| !targets.contains(step.id.as_str()))
        .filter_map(|step| index.get(step.id.as_str()).map(|i| (0, *i)))
        .collect();
    graph_edges.extend(flow.edges.iter().filter_map(|edge| Some((*index.get(edge.from.as_str())?, *index.get(edge.to.as_str())?))));
    let placed: BTreeMap<u64, Point> = layered_layout(&boxes, &graph_edges, ORIGIN).into_iter().map(|(id, p)| (id.0, p)).collect();
    let place = |i: u64, size: Size| {
        // The layered layout places everything within its caps; past them, stack down.
        let p = placed.get(&i).copied().unwrap_or_else(|| Point::new(ORIGIN.x, ORIGIN.y + i as f64 * (STEP_SIZE.height + LAYER_GAP_Y)));
        Rect::from_origin_size(p, size)
    };
    let mut nodes =
        vec![Node { id: "trigger".into(), rect: place(0, TRIGGER_SIZE), shape: "flowChartTerminator".into(), label: trigger_label(&flow.trigger) }];
    for (i, step) in flow.steps.iter().enumerate() {
        nodes.push(Node {
            id: step.id.clone(),
            rect: place(i as u64 + 1, STEP_SIZE),
            shape: shape_for(&step.action).into(),
            label: if step.name.is_empty() { step.id.clone() } else { step.name.clone() },
        });
    }
    let attach = |id: &str| Attach::Node { node: id.to_string(), side: Side::Auto };
    let mut connectors: Vec<Connector> = flow
        .steps
        .iter()
        .filter(|step| !targets.contains(step.id.as_str()))
        .map(|step| Connector { from: attach("trigger"), to: attach(&step.id), routing: Routing::Elbow, label: String::new(), path: None })
        .collect();
    connectors.extend(flow.edges.iter().map(|edge| Connector {
        from: attach(&edge.from),
        to: attach(&edge.to),
        routing: Routing::Elbow,
        label: if edge.when.is_some() { "when".into() } else { String::new() },
        path: None,
    }));
    Diagram { format_version: craft_flowchart::doc::FORMAT_VERSION, nodes, connectors }
}

fn trigger_label(trigger: &Trigger) -> String {
    match trigger {
        Trigger::Manual => "Manual".into(),
        Trigger::AppEvent { app, event } => format!("{app}: {event}"),
        Trigger::Schedule { every_minutes } => format!("Every {every_minutes} min"),
    }
}

fn shape_for(action: &Action) -> &'static str {
    match action {
        Action::AppCommand { .. } => "flowChartProcess",
        Action::Cloud { .. } => "flowChartOnlineStorage",
        Action::Compose { .. } => "flowChartPreparation",
    }
}

#[cfg(test)]
mod tests {
    use super::diagram_for;
    use flowcraft_flow::Flow;
    use serde_json::json;

    fn flow() -> Flow {
        serde_json::from_value(json!({
            "format_version": 1,
            "id": "f",
            "name": "F",
            "trigger": {"type": "app-event", "app": "pdfcraft", "event": "document.signed"},
            "steps": [
                {"id": "check", "action": {"type": "compose", "values": {}}},
                {"id": "store", "action": {"type": "cloud", "operation": {"type": "store-file", "path": "a/b", "content": 1}}},
                {"id": "open", "action": {"type": "app-command", "app": "wordcraft", "command": "review.open", "params": {}}}
            ],
            "edges": [
                {"from": "check", "to": "store", "when": {"type": "eq", "left": 1, "right": 1}},
                {"from": "check", "to": "open"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn projects_nodes_edges_and_shapes() {
        let d = diagram_for(&flow());
        d.validate().unwrap();
        assert_eq!(d.nodes.len(), 4);
        assert_eq!(d.nodes[0].shape, "flowChartTerminator");
        assert_eq!(d.node("check").unwrap().shape, "flowChartPreparation");
        assert_eq!(d.node("store").unwrap().shape, "flowChartOnlineStorage");
        assert_eq!(d.node("open").unwrap().shape, "flowChartProcess");
        assert_eq!(d.connectors.len(), 3);
        assert!(d.connectors.iter().any(|c| c.label == "when"));
        let trigger = d.nodes[0].rect;
        let check = d.node("check").unwrap().rect;
        assert!(check.x0 > trigger.x1, "steps run rightward from the trigger");
        for connector in &d.connectors {
            assert!(d.route(connector).is_some(), "every connector routes");
        }
    }

    #[test]
    fn the_projection_is_deterministic() {
        let a = diagram_for(&flow());
        let b = diagram_for(&flow());
        assert_eq!(a, b);
    }
}
