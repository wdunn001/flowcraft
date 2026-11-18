//! A diagram document: nodes, connectors and labels.
//!
//! The document is what an app saves and what a generator (FlowCraft's flow projection)
//! emits. Shape ids follow the DrawingML preset catalog's `Category::Flowchart`
//! (`flowChartProcess`, `flowChartDecision` and the rest), so shapes stay expressible
//! toward OOXML. Routing is computed, never stored, except for hand-reshaped paths.

use crate::route::{self, RouteEnd};
use crate::{ConnectorPath, Point, Rect, Routing, Side};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The newest diagram file format this build reads and writes.
pub const FORMAT_VERSION: u32 = 1;
/// Upper bound on nodes. Diagram files are hostile input.
pub const MAX_NODES: usize = 4096;
/// Upper bound on connectors.
pub const MAX_CONNECTORS: usize = 16 * 1024;
/// Upper bound on a label, in bytes.
pub const MAX_TEXT: usize = 4096;
/// Upper bound on a serialized diagram document, in bytes.
pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagram {
    pub format_version: u32,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub connectors: Vec<Connector>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub rect: Rect,
    /// A DrawingML preset id from `Category::Flowchart`.
    pub shape: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connector {
    pub from: Attach,
    pub to: Attach,
    #[serde(default)]
    pub routing: Routing,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// A path reshaped by hand; absent, the router decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<ConnectorPath>,
}

/// Where a connector's end sits: on a node's side, or free on the page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Attach {
    Node {
        node: String,
        #[serde(default)]
        side: Side,
    },
    Point {
        x: f64,
        y: f64,
    },
}

/// Why a diagram document could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("diagram document is {0} bytes, over the 4 MiB cap")]
    TooLarge(usize),
    #[error("not valid diagram JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("diagram format version {found} is newer than this build reads (newest is {newest})")]
    UnsupportedVersion { found: u32, newest: u32 },
}

/// Why a diagram cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum ValidateError {
    #[error("the diagram has {0} nodes, over the cap of {MAX_NODES}")]
    TooManyNodes(usize),
    #[error("the diagram has {0} connectors, over the cap of {MAX_CONNECTORS}")]
    TooManyConnectors(usize),
    #[error("a node has an empty id")]
    EmptyNodeId,
    #[error("two nodes share the id {0:?}")]
    DuplicateNodeId(String),
    #[error("node {0:?} has a box that is not finite")]
    NodeNotFinite(String),
    #[error("a label is over {MAX_TEXT} bytes")]
    TextTooLong,
    #[error("a connector attaches to the unknown node {0:?}")]
    UnknownAttachNode(String),
}

impl Diagram {
    /// Reads a diagram document, enforcing the size cap and the format version gate.
    pub fn from_json_str(text: &str) -> Result<Self, ParseError> {
        if text.len() > MAX_DOCUMENT_BYTES {
            return Err(ParseError::TooLarge(text.len()));
        }
        let diagram: Self = serde_json::from_str(text)?;
        if diagram.format_version > FORMAT_VERSION {
            return Err(ParseError::UnsupportedVersion { found: diagram.format_version, newest: FORMAT_VERSION });
        }
        Ok(diagram)
    }

    /// Writes the diagram document as pretty JSON.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Checks ids, caps, finiteness and attachment targets.
    pub fn validate(&self) -> Result<(), ValidateError> {
        if self.nodes.len() > MAX_NODES {
            return Err(ValidateError::TooManyNodes(self.nodes.len()));
        }
        if self.connectors.len() > MAX_CONNECTORS {
            return Err(ValidateError::TooManyConnectors(self.connectors.len()));
        }
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            if node.id.is_empty() {
                return Err(ValidateError::EmptyNodeId);
            }
            if !ids.insert(node.id.as_str()) {
                return Err(ValidateError::DuplicateNodeId(node.id.clone()));
            }
            let r = node.rect;
            if ![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) {
                return Err(ValidateError::NodeNotFinite(node.id.clone()));
            }
            if node.label.len() > MAX_TEXT {
                return Err(ValidateError::TextTooLong);
            }
        }
        for connector in &self.connectors {
            if connector.label.len() > MAX_TEXT {
                return Err(ValidateError::TextTooLong);
            }
            for end in [&connector.from, &connector.to] {
                if let Attach::Node { node, .. } = end
                    && !ids.contains(node.as_str())
                {
                    return Err(ValidateError::UnknownAttachNode(node.clone()));
                }
            }
        }
        Ok(())
    }

    /// The node with this id.
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// The points one connector runs through, as routed now. `None` when an attachment is
    /// unknown or a point is not finite.
    pub fn route(&self, connector: &Connector) -> Option<Vec<Point>> {
        let end = |attach: &Attach| match attach {
            Attach::Point { x, y } => Some(RouteEnd::Point(Point::new(*x, *y))),
            Attach::Node { node, side } => self.node(node).map(|found| RouteEnd::Box(found.rect, *side)),
        };
        route::route_along(end(&connector.from)?, end(&connector.to)?, connector.routing, connector.path.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::{Attach, Connector, Diagram, FORMAT_VERSION, Node, ParseError, Rect, Routing, ValidateError};
    use crate::Side;

    fn diagram() -> Diagram {
        Diagram {
            format_version: FORMAT_VERSION,
            nodes: vec![
                Node { id: "start".into(), rect: Rect::new(0.0, 0.0, 160.0, 48.0), shape: "flowChartTerminator".into(), label: "Start".into() },
                Node { id: "work".into(), rect: Rect::new(248.0, 0.0, 428.0, 64.0), shape: "flowChartProcess".into(), label: "Do the work".into() },
            ],
            connectors: vec![Connector {
                from: Attach::Node { node: "start".into(), side: Side::Auto },
                to: Attach::Node { node: "work".into(), side: Side::Auto },
                routing: Routing::Elbow,
                label: String::new(),
                path: None,
            }],
        }
    }

    #[test]
    fn round_trips_and_validates() {
        let d = diagram();
        d.validate().unwrap();
        let text = d.to_json_string().unwrap();
        assert_eq!(d, Diagram::from_json_str(&text).unwrap());
    }

    #[test]
    fn rejects_unknown_attachments_and_newer_versions() {
        let mut d = diagram();
        d.connectors[0].to = Attach::Node { node: "ghost".into(), side: Side::Auto };
        assert!(matches!(d.validate(), Err(ValidateError::UnknownAttachNode(_))));
        let mut d = diagram();
        d.format_version = FORMAT_VERSION + 1;
        let text = d.to_json_string().unwrap();
        assert!(matches!(Diagram::from_json_str(&text), Err(ParseError::UnsupportedVersion { .. })));
    }

    #[test]
    fn routes_a_connector_between_its_nodes() {
        let d = diagram();
        let pts = d.route(&d.connectors[0]).unwrap();
        assert!(pts.len() >= 2);
        assert_eq!(pts.first().map(|p| p.x), Some(160.0 + crate::route::END_GAP));
        assert_eq!(pts.last().map(|p| p.x), Some(248.0 - crate::route::END_GAP));
    }
}
