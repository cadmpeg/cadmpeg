// SPDX-License-Identifier: Apache-2.0
//! B-rep records: the carriers and topology of one body, converted from and
//! to the IR, and encoded as a `BrepArray` (spec §17).

pub(crate) mod convert;
pub(crate) mod frame;
pub(crate) mod nurbs;
pub(crate) mod primitive;
mod scale;
pub(crate) mod solid;

/// The catalog that a two-part reference selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Catalog {
    Analytic,
    Nurbs,
}

impl Catalog {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Analytic => "analytic",
            Self::Nurbs => "nurbs",
        }
    }
}

/// A two-part reference into a catalog.
pub(crate) type Reference = (Catalog, usize);

/// One edge of `brep:edge:`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeRecord {
    pub(crate) vertices: [usize; 2],
    pub(crate) curve: Option<Reference>,
    pub(crate) range: [f64; 2],
    pub(crate) same_sense: bool,
    pub(crate) tolerance: Option<f64>,
}

/// One oriented edge of `brep:orientedEdge:`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OrientedEdgeRecord {
    pub(crate) edge: usize,
    pub(crate) orientation: bool,
    pub(crate) pcurve: Option<Reference>,
}

/// One loop of `brep:loop:`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoopRecord {
    pub(crate) oriented_edges: Vec<usize>,
    /// The vertex of a vertex loop.
    pub(crate) vertex: Option<usize>,
}

/// One face of `brep:face:`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FaceRecord {
    pub(crate) loops: Vec<usize>,
    pub(crate) surface: Reference,
    pub(crate) same_sense: bool,
    pub(crate) id: i64,
}

/// One shell of `brep:shell:`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShellRecord {
    pub(crate) faces: Vec<usize>,
    pub(crate) closed: bool,
}

/// One solid of `brep:solid:`.
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
    Cone,
    Sphere,
    Torus,
    Extrusion,
    Revolution,
}

impl SurfaceKind {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Plane => "plane",
            Self::Cylinder => "cylinder",
            Self::Cone => "cone",
            Self::Sphere => "sphere",
            Self::Torus => "torus",
            Self::Extrusion => "extrusion",
            Self::Revolution => "revolution",
        }
    }
}

/// The analytic curve types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurveKind {
    Line,
    Circle,
    Ellipse,
    Parabola,
    Hyperbola,
}

impl CurveKind {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Circle => "circle",
            Self::Ellipse => "ellipse",
            Self::Parabola => "parabola",
            Self::Hyperbola => "hyperbola",
        }
    }
}

/// The parameter-space analytic curve types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PcurveKind {
    Circle,
    Ellipse,
}

/// The closure assertion of a NURBS parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Form {
    #[default]
    Open,
    Closed,
    Periodic,
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
    pub(crate) profile: Option<Reference>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnalyticCurve {
    pub(crate) kind: CurveKind,
    pub(crate) frame: Frame,
    pub(crate) params: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnalyticPcurve {
    pub(crate) kind: PcurveKind,
    pub(crate) position: [f64; 2],
    pub(crate) ref_direction: [f64; 2],
    pub(crate) params: Vec<f64>,
    /// The directed range `(t0, t1)`.
    pub(crate) range: [f64; 2],
}

/// A NURBS surface; `points[i * v_count + j]` is the control point at U
/// index `i` and V index `j`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NurbsSurfaceRecord {
    pub(crate) u_order: usize,
    pub(crate) v_order: usize,
    pub(crate) u_count: usize,
    pub(crate) v_count: usize,
    pub(crate) u_form: Form,
    pub(crate) v_form: Form,
    pub(crate) points: Vec<[f64; 3]>,
    pub(crate) weights: Vec<f64>,
    pub(crate) u_knots: Vec<f64>,
    pub(crate) v_knots: Vec<f64>,
}

/// A NURBS curve of `N` coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NurbsCurveRecord<const N: usize> {
    pub(crate) order: usize,
    pub(crate) points: Vec<[f64; N]>,
    pub(crate) weights: Vec<f64>,
    pub(crate) knots: Vec<f64>,
    pub(crate) form: Form,
}

/// The records of one B-rep.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Brep {
    pub(crate) vertices: Vec<[f64; 3]>,
    pub(crate) edges: Vec<EdgeRecord>,
    pub(crate) oriented_edges: Vec<OrientedEdgeRecord>,
    pub(crate) loops: Vec<LoopRecord>,
    pub(crate) faces: Vec<FaceRecord>,
    pub(crate) shells: Vec<ShellRecord>,
    pub(crate) solids: Vec<SolidRecord>,
    pub(crate) analytic_surfaces: Vec<AnalyticSurface>,
    pub(crate) nurbs_surfaces: Vec<NurbsSurfaceRecord>,
    pub(crate) analytic_curves: Vec<AnalyticCurve>,
    pub(crate) nurbs_curves: Vec<NurbsCurveRecord<3>>,
    pub(crate) analytic_pcurves: Vec<AnalyticPcurve>,
    pub(crate) nurbs_pcurves: Vec<NurbsCurveRecord<2>>,
    pub(crate) tolerance: f64,
    /// Radians per unit of an angular parameter.
    pub(crate) angle_scale: f64,
    pub(crate) valid: bool,
    pub(crate) identities: Identities,
}

/// The CADIR identity of the body and of each face, edge, and vertex
/// record, where one is known (spec §18.1).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Identities {
    pub(crate) body: Option<String>,
    pub(crate) faces: Vec<Option<String>>,
    pub(crate) edges: Vec<Option<String>>,
    pub(crate) vertices: Vec<Option<String>>,
}

/// `brep:tolerance` when it is not authored.
pub(crate) const DEFAULT_TOLERANCE: f64 = 1e-6;
