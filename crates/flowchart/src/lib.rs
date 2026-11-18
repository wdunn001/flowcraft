//! The flowchart kernel: nodes, sides, connector routing and auto-layout.
//!
//! Pure geometry, no UI: the kernel returns points and positions, an app draws them. The
//! router and the mind-map layout are lifted from FigCraft's boards
//! (`crates/doc/src/board_route.rs` and `board_mindmap.rs`, same-suite MIT OR Apache-2.0),
//! so FigCraft can cut over to this crate and delete its copies once the crate moves to
//! craft-libs (RFC-0042). Shape vocabulary follows the DrawingML preset catalog's
//! `Category::Flowchart` ids (`flowChartProcess`, `flowChartDecision` and the rest), so a
//! diagram round-trips toward OOXML rather than inventing shapes.
//!
//! This crate incubates inside FlowCraft; its API is written as the craft-libs crate it is
//! meant to become. It must never depend on egui or craft-ui.

pub mod doc;
pub mod layout;
pub mod route;

#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;
#[cfg(test)]
#[path = "route_tests.rs"]
mod route_tests;

use serde::{Deserialize, Serialize};

pub use kurbo::{Point, Rect, Size, Vec2};

/// A node's identity within one diagram or layout call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

/// How a connector runs between its ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Routing {
    #[default]
    Straight,
    /// Right angles only: out of each end along its side's normal, turning once or twice,
    /// around the objects it joins when the short way would cross them.
    Elbow,
    /// One smooth curve, leaving and arriving along the sides' normals.
    Curved,
}

/// A side of a node's box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    /// The side facing the other end.
    #[default]
    Auto,
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    pub fn from_id(s: &str) -> Option<Side> {
        Some(match s.to_ascii_lowercase().as_str() {
            "auto" => Side::Auto,
            "top" => Side::Top,
            "right" => Side::Right,
            "bottom" => Side::Bottom,
            "left" => Side::Left,
            _ => return None,
        })
    }

    /// The side's middle on `b`.
    pub fn point(self, b: Rect) -> Point {
        let c = b.center();
        match self {
            Side::Top => Point::new(c.x, b.y0),
            Side::Right => Point::new(b.x1, c.y),
            Side::Bottom => Point::new(c.x, b.y1),
            Side::Left => Point::new(b.x0, c.y),
            Side::Auto => c,
        }
    }

    pub fn horizontal(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }

    /// The way out of the side (y down; none for [`Side::Auto`]).
    pub fn normal(self) -> Vec2 {
        match self {
            Side::Top => Vec2::new(0.0, -1.0),
            Side::Right => Vec2::new(1.0, 0.0),
            Side::Bottom => Vec2::new(0.0, 1.0),
            Side::Left => Vec2::new(-1.0, 0.0),
            Side::Auto => Vec2::ZERO,
        }
    }
}

/// A connector path reshaped by hand, kept beside its routing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ConnectorPath {
    /// An elbow's straight runs, each at right angles to the one before: whether the first
    /// runs across (else up or down), and where the runs between the first and last lie (x
    /// of one that runs up or down, y of one that runs across). The first and last lie on
    /// the ends.
    Runs { across: bool, at: Vec<f64> },
    /// How far a curve's middle was moved.
    Bow { x: f64, y: f64 },
}
