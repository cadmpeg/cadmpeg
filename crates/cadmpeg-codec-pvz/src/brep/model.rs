// SPDX-License-Identifier: Apache-2.0
//! Records of one exact-geometry stream, in source form.
//!
//! Scalars are widened to `f64`; lengths stay in metres and parameters in the
//! native parameterisation of their record. Indexes are zero-based ordinals
//! within their section and are checked against the section counts.

/// Scalar width selected by the stream identity (§8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Precision {
    /// Exact version 3: `f32` scalars.
    Single,
    /// Exact version 4: `f64` scalars.
    Double,
}

impl Precision {
    /// Encoded width of one scalar in bytes.
    pub(crate) const fn width(self) -> usize {
        match self {
            Self::Single => 4,
            Self::Double => 8,
        }
    }

    /// Exact version stored in the stream index record.
    pub(crate) const fn version(self) -> u32 {
        match self {
            Self::Single => 3,
            Self::Double => 4,
        }
    }
}

/// A point or vector in model space.
pub(crate) type Vec3 = [f64; 3];
/// A point or vector in a surface parameter domain.
pub(crate) type Vec2 = [f64; 2];

/// One exact-geometry stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExactModel {
    /// Scalar width of the stream.
    pub(crate) precision: Precision,
    /// Faces in record order.
    pub(crate) faces: Vec<Face>,
    /// Loops in record order; the position is the global loop index.
    pub(crate) loops: Vec<Loop>,
    /// Edge curves in record order.
    pub(crate) edges: Vec<EdgeCurve>,
    /// Pcurves in record order.
    pub(crate) pcurves: Vec<Pcurve>,
    /// Records with a defined frame and no settled meaning.
    pub(crate) opaque: Vec<OpaqueRecord>,
}

/// A record with a defined frame and no settled meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpaqueRecord {
    /// TLV tag.
    pub(crate) tag: u32,
}

/// One face: a supporting surface and the loops that follow it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Face {
    /// Supporting surface record.
    pub(crate) surface: SurfaceRecord,
    /// Global indexes of the loops of this face, in record order.
    pub(crate) loops: Vec<usize>,
    /// Singular edge uses of this face, in record order.
    pub(crate) singular_uses: Vec<SingularUse>,
    /// Payload offset of the surface record.
    pub(crate) offset: usize,
}

/// Geometry of a surface record (§8.3).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceRecord {
    /// Surface geometry.
    pub(crate) geometry: SurfaceGeometry,
    /// Embedded B-spline of an analytic surface (§8.10).
    pub(crate) embedded: Option<BsplineSurface>,
}

/// Native surface geometry (§8.4 to §8.9).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SurfaceGeometry {
    /// `O + u·U + v·V`, normal `N`.
    Plane {
        /// `O`.
        origin: Vec3,
        /// `U`.
        u_axis: Vec3,
        /// `V`.
        v_axis: Vec3,
        /// `N`.
        normal: Vec3,
    },
    /// `O + cos(u)·X + sin(u)·Y + v·A`.
    Cylinder {
        /// `O`.
        origin: Vec3,
        /// `X`.
        x: Vec3,
        /// `Y`.
        y: Vec3,
        /// `A`.
        axis: Vec3,
    },
    /// `O + v·A + (a + b·v)·(cos(u)·X + sin(u)·Y)`.
    Cone {
        /// `O`.
        origin: Vec3,
        /// `X`.
        x: Vec3,
        /// `Y`.
        y: Vec3,
        /// `A`.
        axis: Vec3,
        /// `a`.
        a: f64,
        /// `b`.
        b: f64,
    },
    /// `C + cos(v)·(cos(u)·X + sin(u)·Y) + sin(v)·Z`.
    Sphere {
        /// `C`.
        center: Vec3,
        /// `X`.
        x: Vec3,
        /// `Y`.
        y: Vec3,
        /// `Z`.
        z: Vec3,
    },
    /// The torus of §8.8.
    Torus(Torus),
    /// Rational B-spline surface (§8.9).
    Bspline(BsplineSurface),
    /// A surface type outside §8.3; the payload is kept unread.
    Undefined {
        /// `surface_type` head byte.
        surface_type: u8,
    },
}

/// Torus vectors of §8.8.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Torus {
    /// `O`.
    pub(crate) origin: Vec3,
    /// `M_c`.
    pub(crate) major_cos: Vec3,
    /// `M_s`.
    pub(crate) major_sin: Vec3,
    /// `axis`.
    pub(crate) axis: Vec3,
    /// `m_c`.
    pub(crate) minor_cos: Vec3,
    /// `m_a`.
    pub(crate) minor_axial: Vec3,
    /// `m_s`.
    pub(crate) minor_sin: Vec3,
}

/// Tensor-product rational B-spline surface with homogeneous poles.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BsplineSurface {
    /// `rational` byte is 1.
    pub(crate) rational: bool,
    /// Order (degree + 1) in u.
    pub(crate) u_order: usize,
    /// Order in v.
    pub(crate) v_order: usize,
    /// Full u knot vector.
    pub(crate) u_knots: Vec<f64>,
    /// Full v knot vector.
    pub(crate) v_knots: Vec<f64>,
    /// Pole count in u.
    pub(crate) u_count: usize,
    /// Pole count in v.
    pub(crate) v_count: usize,
    /// Homogeneous poles `(w·x, w·y, w·z, w)`, u-major.
    pub(crate) poles: Vec<[f64; 4]>,
    /// Periodic in u and in v, with the knot-domain length as period.
    pub(crate) periodic: [bool; 2],
}

/// One loop record and its coedges (§8.11, §8.12).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Loop {
    /// Owning face index.
    pub(crate) face: usize,
    /// Loop group: the global index of the group's first loop.
    pub(crate) group: usize,
    /// Coedges in record order.
    pub(crate) coedges: Vec<Coedge>,
    /// Payload offset of the loop record.
    pub(crate) offset: usize,
}

/// One coedge record (§8.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Coedge {
    /// Edge index.
    pub(crate) edge: Option<usize>,
    /// `orientation` byte.
    pub(crate) orientation: u8,
    /// Native pcurve index.
    pub(crate) pcurve: Option<usize>,
    /// Secondary pcurve index, in the embedded B-spline domain.
    pub(crate) secondary: Option<usize>,
    /// Payload offset of the coedge record.
    pub(crate) offset: usize,
}

/// One singular edge use (§8.13).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SingularUse {
    /// Edge index.
    pub(crate) edge: usize,
    /// Payload offset of the record.
    pub(crate) offset: usize,
}

/// One edge curve record (§8.15).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeCurve {
    /// Curve geometry.
    pub(crate) geometry: CurveGeometry<3>,
    /// Payload offset of the record.
    pub(crate) offset: usize,
}

/// One pcurve record (§8.16).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pcurve {
    /// Curve geometry in a surface parameter domain.
    pub(crate) geometry: CurveGeometry<2>,
    /// Payload offset of the record.
    pub(crate) offset: usize,
}

/// Curve geometry shared by edge curves (`N` = 3) and pcurves (`N` = 2).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CurveGeometry<const N: usize> {
    /// `C + cos(t)·X + sin(t)·Y` with `|X| = |Y|` (edge curves only).
    Circle(Conic<N>),
    /// `C + cos(t)·X + sin(t)·Y`.
    Ellipse(Conic<N>),
    /// Member indexes of the same section.
    Composite(Vec<usize>),
    /// One point (edge curves only).
    Point([f64; N]),
    /// `O + t·D`, `t` in `[0, 1]`.
    Line {
        /// `O`.
        origin: [f64; N],
        /// `D`.
        direction: [f64; N],
    },
    /// Rational B-spline with homogeneous poles.
    Bspline(BsplineCurve<N>),
    /// Linear interpolation of the points.
    Polyline(Vec<[f64; N]>),
    /// A curve type outside the defined set; the payload is kept unread.
    Undefined {
        /// `curve_type` head byte.
        curve_type: u8,
    },
}

/// Circle or ellipse parameters with an optional range pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Conic<const N: usize> {
    /// `C`.
    pub(crate) center: [f64; N],
    /// `X`.
    pub(crate) x: [f64; N],
    /// `Y`.
    pub(crate) y: [f64; N],
    /// `t_start`, `t_end`, when flag `0x08` is set.
    pub(crate) range: Option<Vec2>,
}

/// Rational B-spline curve. Each homogeneous pole holds the weighted
/// coordinates followed by the weight.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BsplineCurve<const N: usize> {
    /// `rational` byte is 1.
    pub(crate) rational: bool,
    /// Order (degree + 1).
    pub(crate) order: usize,
    /// Full knot vector.
    pub(crate) knots: Vec<f64>,
    /// Weighted coordinates, one entry per pole.
    pub(crate) weighted: Vec<[f64; N]>,
    /// Weights, one entry per pole.
    pub(crate) weights: Vec<f64>,
    /// Range pair, when flag `0x08` is set (pcurves only).
    pub(crate) range: Option<Vec2>,
}
