// SPDX-License-Identifier: Apache-2.0
//! The B-rep records of the intrinsic primitives (spec §11.5): faces on
//! analytic surfaces bounded by line and circle edges, in scene units.

pub(crate) mod primitive;

/// One edge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeRecord {
    pub(crate) vertices: [usize; 2],
    /// The index of the edge's curve.
    pub(crate) curve: usize,
    pub(crate) range: [f64; 2],
}

/// One use of an edge in a loop.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OrientedEdgeRecord {
    pub(crate) edge: usize,
    pub(crate) orientation: bool,
}

/// One loop.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoopRecord {
    pub(crate) oriented_edges: Vec<usize>,
}

/// One face.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FaceRecord {
    pub(crate) loops: Vec<usize>,
    /// The index of the face's surface.
    pub(crate) surface: usize,
}

/// One shell.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShellRecord {
    pub(crate) faces: Vec<usize>,
    pub(crate) closed: bool,
}

/// One solid.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SolidRecord {
    pub(crate) outer: usize,
    pub(crate) voids: Vec<usize>,
}

/// The analytic surface types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceKind {
    Plane,
    Cylinder,
    /// `params`: the radius at the frame origin and the semi-angle.
    Cone,
    Sphere,
}

/// The analytic curve types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurveKind {
    Line,
    Circle,
}

/// A frame-positioned analytic record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Frame {
    pub(crate) position: [f64; 3],
    pub(crate) axis: [f64; 3],
    pub(crate) ref_direction: [f64; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnalyticSurface {
    pub(crate) kind: SurfaceKind,
    pub(crate) frame: Frame,
    pub(crate) params: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnalyticCurve {
    pub(crate) kind: CurveKind,
    pub(crate) frame: Frame,
    pub(crate) params: Vec<f64>,
}

/// The records of one primitive.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Brep {
    pub(crate) vertices: Vec<[f64; 3]>,
    pub(crate) edges: Vec<EdgeRecord>,
    pub(crate) oriented_edges: Vec<OrientedEdgeRecord>,
    pub(crate) loops: Vec<LoopRecord>,
    pub(crate) faces: Vec<FaceRecord>,
    pub(crate) shells: Vec<ShellRecord>,
    pub(crate) solids: Vec<SolidRecord>,
    pub(crate) surfaces: Vec<AnalyticSurface>,
    pub(crate) curves: Vec<AnalyticCurve>,
    pub(crate) tolerance: f64,
}

/// The linear tolerance of a primitive's records.
pub(crate) const DEFAULT_TOLERANCE: f64 = 1e-6;
