// SPDX-License-Identifier: Apache-2.0
//! The records of a native body slice: the envelope and B-rep schema 4.
//!
//! The types mirror `SolidCraft`'s `solidcore_record::brep` field for field
//! (`docs/native-brep.md` in the `SolidCraft` repository). References are
//! indices into the body's own lists. Coordinates are millimetres and
//! angles radians. A blend definition is kept as its value: the face's
//! `nurbs` net is its geometry.

use serde::Deserialize;
use serde_json::Value;

/// The B-rep schema this decoder reads.
pub(super) const SCHEMA: u32 = 4;

/// One body slice: the solid, its metadata, and its runtime measures.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Saved {
    /// The body's B-rep.
    pub(super) solid: Brep,
    /// The metadata record: colour, paint, face sources, history.
    #[serde(rename = "metadata")]
    pub(super) _metadata: Value,
    /// What the body measured about itself.
    #[serde(rename = "runtime")]
    pub(super) _runtime: Value,
}

/// A body: its regions, shells, faces, edges, and vertices.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Brep {
    /// The schema version.
    pub(super) version: u32,
    /// The intersection tolerance, millimetres.
    #[serde(rename = "tolerance")]
    pub(super) _tolerance: f64,
    /// Regions; the first is the infinite one.
    pub(super) regions: Vec<Region>,
    /// Shells.
    pub(super) shells: Vec<Shell>,
    /// Faces.
    pub(super) faces: Vec<Face>,
    /// Edges.
    pub(super) edges: Vec<Edge>,
    /// Vertices.
    pub(super) vertices: Vec<Vertex>,
}

/// A region: its shells, outer first for a bounded region.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Region {
    /// Shell indices.
    pub(super) shells: Vec<u32>,
    /// Whether the bounded region is empty space.
    pub(super) void: bool,
}

/// A shell.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Shell {
    /// Oriented faces and the wire edges embedded with them.
    Faces {
        /// Oriented faces.
        faces: Vec<OrientedFace>,
        /// Wire edge indices.
        wires: Vec<u32>,
    },
    /// A connected graph of wire edges.
    Wire {
        /// Edge indices.
        edges: Vec<u32>,
    },
    /// One isolated vertex.
    Vertex {
        /// Vertex index.
        vertex: u32,
    },
}

/// A face as one shell uses it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OrientedFace {
    /// Face index.
    pub(super) face: u32,
    /// Whether the face normal in this shell is its surface's natural normal.
    pub(super) forward: bool,
}

cadmpeg_core::named_optional_field!(deserialize_face_nurbs, Surface, "nurbs");
cadmpeg_core::named_optional_field!(deserialize_edge_nurbs, Curve, "nurbs");

/// A face: its surface and its loops in that surface's parameters.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Face {
    /// What the face is.
    pub(super) surface: Surface,
    /// The NURBS form the face is parameterised on, when it has one.
    #[serde(default, deserialize_with = "deserialize_face_nurbs")]
    pub(super) nurbs: Option<Surface>,
    /// The parameter box `[[u0, u1], [v0, v1]]`.
    pub(super) domain: [[f64; 2]; 2],
    /// Loops, relative to the natural normal.
    pub(super) loops: Vec<Loop>,
}

/// A loop.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Loop {
    /// A closed chain of oriented edges.
    Edges {
        /// Oriented edges in traversal order.
        edges: Vec<OrientedEdge>,
    },
    /// One vertex at a pole or collapsed boundary.
    Vertex {
        /// Vertex index.
        vertex: u32,
    },
}

/// One use of an edge in a loop.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OrientedEdge {
    /// Edge index.
    pub(super) edge: u32,
    /// Whether the use runs from the edge's start to its end.
    pub(super) forward: bool,
    /// The edge in the face's (u, v), from interval min to max.
    pub(super) pcurve: Curve2,
}

/// An edge.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Edge {
    /// What the edge is.
    pub(super) curve: Curve,
    /// The NURBS form the edge is parameterised on, when it has one.
    #[serde(default, deserialize_with = "deserialize_edge_nurbs")]
    pub(super) nurbs: Option<Curve>,
    /// The covered parameter interval, min < max.
    pub(super) interval: [f64; 2],
    /// Start and end vertex indices, at interval min and max.
    pub(super) vertices: [u32; 2],
    /// Tolerance, millimetres.
    pub(super) tolerance: f64,
    /// Every use, the primary one first, then in radial order.
    pub(super) uses: Vec<EdgeUse>,
}

/// Where one use of an edge sits.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EdgeUse {
    /// Face index.
    pub(super) face: u32,
    /// Loop index in the face.
    #[serde(rename = "loop")]
    pub(super) loop_: u32,
    /// Position in the loop.
    pub(super) position: u32,
    /// Whether the radial cycle enters on the natural normal's side.
    #[serde(rename = "upper")]
    pub(super) _upper: bool,
}

/// A vertex.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Vertex {
    /// Position.
    pub(super) point: [f64; 3],
    /// Tolerance, millimetres.
    pub(super) tolerance: f64,
}

/// A frame: an origin and orthonormal x and y axes; z = x × y.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Frame {
    /// Origin.
    pub(super) origin: [f64; 3],
    /// X axis.
    pub(super) x_axis: [f64; 3],
    /// Y axis.
    pub(super) y_axis: [f64; 3],
}

/// An unbounded curve.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Curve {
    /// `start + t (end - start)`.
    Line {
        /// Point at t = 0.
        start: [f64; 3],
        /// Point at t = 1.
        end: [f64; 3],
    },
    /// `origin + r (cos t x + sin t y)`.
    Circle {
        /// Frame.
        frame: Frame,
        /// Radius.
        radius: f64,
    },
    /// `origin + r0 cos t x + r1 sin t y`.
    Ellipse {
        /// Frame.
        frame: Frame,
        /// Radii along x and y.
        radii: [f64; 2],
    },
    /// Segment i spans [i, i + 1].
    Polyline {
        /// Points.
        points: Vec<[f64; 3]>,
    },
    /// A B-spline on its full knot vector.
    Spline {
        /// Knots.
        knots: Vec<f64>,
        /// Control points.
        points: Vec<[f64; 3]>,
    },
    /// A rational B-spline with homogeneous `[x w, y w, z w, w]` coefficients.
    RationalSpline {
        /// Knots.
        knots: Vec<f64>,
        /// Homogeneous coefficients.
        homogeneous: Vec<[f64; 4]>,
    },
    /// The intersection of two surfaces, traced from its leader.
    SurfaceIntersection {
        /// The two surfaces.
        #[serde(rename = "surfaces")]
        _surfaces: Value,
        /// A polyline or spline near the intersection, whose parameter it takes.
        leader: Box<Curve>,
        /// Trace tolerance, millimetres.
        #[serde(rename = "tolerance")]
        _tolerance: f64,
    },
}

/// An unbounded surface.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Surface {
    /// `origin + u u_axis + v v_axis`.
    Plane {
        /// Origin.
        origin: [f64; 3],
        /// U axis.
        u_axis: [f64; 3],
        /// V axis.
        v_axis: [f64; 3],
    },
    /// `origin + u (u_point - origin) + v (v_point - origin)`.
    PlaneThrough {
        /// Origin.
        origin: [f64; 3],
        /// Point at (1, 0).
        u_point: [f64; 3],
        /// Point at (0, 1).
        v_point: [f64; 3],
    },
    /// `origin + v z + r (cos u x + sin u y)`.
    Cylinder {
        /// Frame.
        frame: Frame,
        /// Radius.
        radius: f64,
    },
    /// `origin + v z + r(v) (cos u x + sin u y)`, `r(v) = r0 + v tan(half_angle)`.
    Cone {
        /// Frame.
        frame: Frame,
        /// Radius at v = 0.
        radius_at_origin: f64,
        /// Half angle; negative narrows along z.
        half_angle: f64,
    },
    /// `origin + r (cos v (cos u x + sin u y) + sin v z)`.
    Sphere {
        /// Frame.
        frame: Frame,
        /// Radius.
        radius: f64,
    },
    /// `origin + (R + r cos w)(cos u x + sin u y) + r sin w z`, `w = v + tube_start`.
    Torus {
        /// Frame.
        frame: Frame,
        /// Major radius.
        major_radius: f64,
        /// Minor radius.
        minor_radius: f64,
        /// Meridian angle at v = 0.
        #[serde(default)]
        tube_start: f64,
    },
    /// A patch on a sphere; the face's `nurbs` is its geometry.
    BallPatch {
        /// Sphere centre.
        #[serde(rename = "center")]
        _center: [f64; 3],
        /// Sphere radius.
        #[serde(rename = "radius")]
        _radius: f64,
    },
    /// A tensor-product B-spline, `points[i][j]` with i along u.
    Spline {
        /// U knots.
        u_knots: Vec<f64>,
        /// V knots.
        v_knots: Vec<f64>,
        /// Control net.
        points: Vec<Vec<[f64; 3]>>,
    },
    /// A rational tensor-product B-spline with homogeneous coefficients.
    RationalSpline {
        /// U knots.
        u_knots: Vec<f64>,
        /// V knots.
        v_knots: Vec<f64>,
        /// Homogeneous control net.
        homogeneous: Vec<Vec<[f64; 4]>>,
    },
    /// A profile revolved about an axis, then mapped by an affine matrix.
    PlacedRevolution {
        /// Profile.
        #[serde(rename = "profile")]
        _profile: Value,
        /// Axis origin.
        #[serde(rename = "origin")]
        _origin: [f64; 3],
        /// Unit axis.
        #[serde(rename = "axis")]
        _axis: [f64; 3],
        /// Column-major affine matrix.
        #[serde(rename = "matrix")]
        _matrix: [[f64; 4]; 4],
        /// Whether u is the profile's parameter.
        #[serde(rename = "forward")]
        _forward: bool,
    },
    /// A round by its definition; the face's `nurbs` is its net.
    Blend {
        /// The definition.
        #[serde(rename = "definition")]
        _definition: Value,
    },
}

/// A pcurve: a 2D B-spline in the face's (u, v).
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Curve2 {
    /// Polynomial.
    Spline {
        /// Knots.
        knots: Vec<f64>,
        /// Control points.
        points: Vec<[f64; 2]>,
    },
    /// Rational, homogeneous `[u w, v w, w]` coefficients.
    RationalSpline {
        /// Knots.
        knots: Vec<f64>,
        /// Homogeneous coefficients.
        homogeneous: Vec<[f64; 3]>,
    },
}
