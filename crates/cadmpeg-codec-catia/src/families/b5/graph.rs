// SPDX-License-Identifier: Apache-2.0
//! Object-id topology in the CATIA `b5 03` short-frame family.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::le::f64_at;
use cadmpeg_ir::eval::{nurbs_pcurve_uv, nurbs_surface_point};
use cadmpeg_ir::geometry::{NurbsSurface, ProceduralSurfaceDefinition, SurfaceGeometry};
use cadmpeg_ir::math::Point2;

use super::vecmath::{add, cross, scale};
use crate::analytic::{periodic_angular_range_is_valid, sphere_angular_ranges_are_valid};
use crate::wire;

/// Resolved `b5 03` object-stream topology graph: faces, loops, pcurves, and
/// surfaces bound through the in-stream `object_id` map ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)),
/// together with the `05 08 01` vertex points used to bind edge endpoints.
#[derive(Debug, Clone, PartialEq)]
pub struct B5Graph {
    /// `true` when every serialized face and loop node belongs to the resolved
    /// reference-closed graph; `false` when the graph is its maximal closed
    /// subset.
    pub complete: bool,
    /// `b5 03 5f` face nodes, in stream declaration order (equal to STEP
    /// `ADVANCED_FACE` order, [spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
    pub faces: Vec<B5Face>,
    /// Structurally complete `b5 03 5f` records, keyed by object id.
    pub face_records: BTreeMap<u32, B5FaceRecord>,
    /// `b5 03 62` loop nodes, keyed by `object_id`.
    pub loops: BTreeMap<u32, B5Loop>,
    /// `b5 03 21` pcurve nodes, keyed by `object_id`.
    pub pcurves: BTreeMap<u32, B5Pcurve>,
    /// Structurally bounded pcurve records whose parameter-space geometry is
    /// not yet assigned, keyed by `object_id`.
    pub opaque_pcurves: BTreeMap<u32, B5OpaquePcurve>,
    /// Pcurve occurrence ids whose support is bound by a loop and both native
    /// edge-endpoint incidence records, but which have no standalone geometry
    /// record.
    pub implicit_pcurves: BTreeMap<u32, u32>,
    /// `b5 03 27/28/2d` analytic surface nodes and `a8 03 34` NURBS
    /// surfaces, keyed by `object_id`.
    pub surfaces: BTreeMap<u32, B5Surface>,
    /// Resolved class-`2e`/`38` surface alias targets, keyed by alias identity.
    pub surface_aliases: BTreeMap<u32, u32>,
    /// `b5 03 30` offset constructions, keyed by their result surface id.
    pub offset_surfaces: BTreeMap<u32, B5OffsetSurface>,
    /// `b5 03 2c` extrusion constructions, keyed by their result surface id.
    pub extrusion_surfaces: BTreeMap<u32, B5ExtrusionSurface>,
    /// `b5 03 37/3b` support-bound constructions, keyed by result surface id.
    pub supported_surfaces: BTreeMap<u32, B5SupportedSurface>,
    /// Native class-`06` curve-parameter incidences, keyed by object id.
    pub parameter_incidences: BTreeMap<u32, B5ParameterIncidence>,
    /// Native class-`5e` physical-edge records, keyed by object id.
    pub edges: BTreeMap<u32, B5Edge>,
    /// Native class-`5d` vertex-to-incidence links, keyed by object id.
    pub vertex_incidence_links: BTreeMap<u32, B5VertexIncidenceLink>,
    /// World-frame `05 08 01` vertex coordinates, in stream order.
    pub vertex_points: Vec<[f64; 3]>,
    /// Logical vertex coordinates resolved from native `5d` identity. Their
    /// edge indices follow the raw `vertex_points` indices.
    pub logical_vertex_points: Vec<[f64; 3]>,
    /// Native `5d` object ids aligned with `logical_vertex_points`.
    pub logical_vertex_refs: Vec<u32>,
    /// Per-edge pair of vertex indices. Raw `vertex_points` occupy the first
    /// index range; native `5d` logical vertices occupy the following range.
    pub edge_vertices: BTreeMap<u32, [usize; 2]>,
    /// Ordered class-`06` start/end parameter-incidence references from each
    /// native class-`5e` edge.
    pub edge_parameter_incidences: BTreeMap<u32, [u32; 2]>,
    /// Maximum incident endpoint residual for each logical vertex, keyed by
    /// the combined vertex index used by `edge_vertices`.
    pub vertex_tolerances: BTreeMap<usize, f64>,
    /// `b5 03 0e`/`0f` line and arc profile curves, keyed by `object_id`;
    /// referenced by `B5Surface::Revolution::profile_curve`.
    pub profiles: BTreeMap<u32, B5Profile>,
}

impl B5Graph {
    /// Follow surface aliases to their canonical terminal identity.
    #[must_use]
    pub(crate) fn canonical_surface_id(&self, object_id: u32) -> Option<u32> {
        canonical_surface_id(&self.surface_aliases, object_id)
    }

    /// Return native edge identities proven to belong to the closed B-rep.
    ///
    /// A structurally parseable class-`5e` allocation is a physical edge only
    /// when a resolved face loop references it.  An incomplete graph cannot
    /// prove that the retained loop set is exhaustive, so callers must use
    /// their unresolved-association fallback in that case.
    #[must_use]
    pub(crate) fn referenced_edge_vertex_references(&self) -> Option<BTreeMap<u32, [u32; 2]>> {
        if !self.complete {
            return None;
        }
        let referenced_edges = self
            .loops
            .values()
            .flat_map(|loop_| loop_.edges.iter().copied())
            .collect::<HashSet<_>>();
        Some(
            referenced_edges
                .into_iter()
                .filter_map(|object_id| {
                    self.edges
                        .get(&object_id)
                        .map(|edge| (object_id, edge.vertices))
                })
                .collect(),
        )
    }
}

/// Return the ordered start/end stations for one edge's occurrence of a
/// pcurve when both native endpoint incidences name that pcurve consistently.
pub(crate) fn edge_pcurve_parameters(graph: &B5Graph, edge: u32, pcurve: u32) -> Option<[f64; 2]> {
    edge_pcurve_parameter_values(
        &graph.edge_parameter_incidences,
        &graph.parameter_incidences,
        edge,
        pcurve,
    )
}

fn edge_pcurve_parameter_values(
    edge_parameter_incidences: &BTreeMap<u32, [u32; 2]>,
    parameter_incidences: &BTreeMap<u32, B5ParameterIncidence>,
    edge: u32,
    pcurve: u32,
) -> Option<[f64; 2]> {
    edge_parameter_incidences
        .get(&edge)?
        .map(|incidence_id| {
            let incidence = parameter_incidences.get(&incidence_id)?;
            let mut parameters = incidence
                .curves
                .iter()
                .zip(&incidence.parameters)
                .filter_map(|(&curve, &parameter)| (curve == pcurve).then_some(parameter));
            let parameter = parameters.next()?;
            parameters
                .all(|other| other == parameter)
                .then_some(parameter)
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

/// Follow one surface identity through a direct alias map.
pub(crate) fn canonical_surface_id(
    aliases: &BTreeMap<u32, u32>,
    mut object_id: u32,
) -> Option<u32> {
    let mut visited = HashSet::new();
    while let Some(&target) = aliases.get(&object_id) {
        if !visited.insert(object_id) {
            return None;
        }
        object_id = target;
    }
    Some(object_id)
}

/// A profile curve swept by a `b5 03 2d` surface of revolution.
#[derive(Debug, Clone, PartialEq)]
pub enum B5Profile {
    /// `b5 03 0e`: a line through `point` along `direction`.
    Line {
        /// A point on the line.
        point: [f64; 3],
        /// Unit direction of the line.
        direction: [f64; 3],
        /// Complete native line parameter interval.
        parameter_range: [f64; 2],
    },
    /// `b5 03 0f`: an arc with a positive radius.
    Arc {
        /// Arc center.
        center: [f64; 3],
        /// Unit vector from `center` toward the zero-angle point.
        direction_x: [f64; 3],
        /// Unit vector orthogonal to `direction_x` completing the arc
        /// plane's basis.
        direction_y: [f64; 3],
        /// Positive arc radius.
        radius: f64,
        /// Complete native arc-length parameter interval.
        parameter_range: [f64; 2],
    },
}

impl B5Profile {
    pub(crate) fn parameter_range(&self) -> [f64; 2] {
        match self {
            Self::Line {
                parameter_range, ..
            }
            | Self::Arc {
                parameter_range, ..
            } => *parameter_range,
        }
    }
}

/// A resolved `b5 03` surface node ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum B5Surface {
    /// A NURBS surface whose parameter lattice is decoded but whose pole
    /// representation remains opaque.
    UnresolvedNurbs {
        /// Decoded degree, knot, multiplicity, and pole-cardinality fields.
        header: crate::families::a5a8::records::A8SurfaceHeader,
        /// Exact source payload, including the opaque pole representation.
        payload: Vec<u8>,
    },
    /// An identity-bearing surface record whose carrier geometry remains opaque.
    Unknown {
        /// Source record family.
        family: u8,
        /// Source surface class.
        class: u8,
        /// Exact source payload.
        payload: Vec<u8>,
    },
    /// `b5 03 27`: a plane spanned by `origin`, `direction_u`, and
    /// `direction_v`.
    Plane {
        /// A point on the plane.
        origin: [f64; 3],
        /// First in-plane unit direction.
        direction_u: [f64; 3],
        /// Second in-plane unit direction.
        direction_v: [f64; 3],
        /// Active native U interval.
        u_range: [f64; 2],
        /// Active native V interval.
        v_range: [f64; 2],
    },
    /// `b5 03 28`: a cylinder with a positive radius.
    Cylinder {
        /// A point on the cylinder axis.
        origin: [f64; 3],
        /// Unit reference direction orthogonal to `axis`, the zero-angle
        /// ray.
        reference_x: [f64; 3],
        /// Unit cylinder axis, `reference_x × stored_v` normalized.
        axis: [f64; 3],
        /// Positive cylinder radius.
        radius: f64,
        /// Active native circumferential interval.
        u_range: [f64; 2],
        /// Active native axial interval.
        v_range: [f64; 2],
        /// Divisor mapping native U to azimuth.
        angular_scale: f64,
        /// Origin of the full-turn native U chart.
        chart_origin: f64,
    },
    /// `b5 03 29`: a circular cone in its native arc-length/slant chart.
    Cone {
        /// Cone apex.
        apex: [f64; 3],
        /// First transverse unit direction.
        direction_x: [f64; 3],
        /// Second transverse unit direction.
        direction_y: [f64; 3],
        /// Cone-axis unit direction.
        axis: [f64; 3],
        /// Cone half-angle in radians.
        half_angle: f64,
        /// Scalar immediately preceding the active angular interval.
        pre_angular_range_scalar: f64,
        /// Active azimuth interval.
        angular_range: [f64; 2],
        /// Native slant-coordinate range.
        slant_range: [f64; 2],
        /// Divisor mapping native U to azimuth.
        angular_scale: f64,
        /// Full-turn azimuth chart domain.
        angular_domain: [f64; 2],
    },
    /// `b5 03 2a`: a sphere with a radius-scaled right-handed frame.
    Sphere {
        /// Sphere center.
        center: [f64; 3],
        /// Zero-azimuth unit direction.
        direction_x: [f64; 3],
        /// Quarter-turn azimuth unit direction.
        direction_y: [f64; 3],
        /// Polar-axis unit direction.
        axis: [f64; 3],
        /// Positive sphere radius.
        radius: f64,
        /// Active azimuth interval, in radians.
        azimuth_range: [f64; 2],
        /// Active latitude interval, in radians.
        latitude_range: [f64; 2],
        /// Positive radius of the enclosing support-bound construction.
        construction_radius: f64,
        /// Origin of the native periodic V coordinate, in length units.
        chart_origin: f64,
    },
    /// `b5 03 2b`: a torus in its two arc-length angular coordinates.
    Torus {
        /// Torus center.
        center: [f64; 3],
        /// Zero-major-angle direction.
        direction_x: [f64; 3],
        /// Quarter-turn major-angle direction.
        direction_y: [f64; 3],
        /// Torus axis.
        axis: [f64; 3],
        /// Major radius.
        major_radius: f64,
        /// Minor radius.
        minor_radius: f64,
        /// Active major-angle interval.
        major_angular_range: [f64; 2],
        /// Full-turn major-angle chart domain.
        major_angular_domain: [f64; 2],
        /// Active minor-angle interval.
        minor_angular_range: [f64; 2],
        /// Full-turn minor-angle chart domain.
        minor_angular_domain: [f64; 2],
        /// Divisor mapping native U to the major angle.
        major_scale: f64,
        /// Divisor mapping native V to the minor angle.
        minor_scale: f64,
    },
    /// `b5 03 2d`: a surface of revolution sweeping `profile_curve` about
    /// `axis_origin`/`axis_direction`.
    Revolution {
        /// `object_id` of the swept [`B5Profile`].
        profile_curve: u32,
        /// A point on the revolution axis.
        axis_origin: [f64; 3],
        /// First transverse unit direction of the stored revolution frame.
        reference_x: [f64; 3],
        /// Second transverse unit direction of the stored revolution frame.
        reference_y: [f64; 3],
        /// Unit revolution axis.
        axis_direction: [f64; 3],
        /// Active parameter interval of the swept profile.
        profile_range: [f64; 2],
        /// Active native arc-length interval of the revolution.
        angular_range: [f64; 2],
        /// Positive divisor mapping native V to a revolution angle.
        angular_scale: f64,
    },
    /// An `a8 03 34` inline-pole B-spline surface, resolved through
    /// [`crate::families::a5a8::records::a8_surfaces`] and merged into the same
    /// `object_id` namespace.
    Nurbs(NurbsSurface),
    /// An `a8 03 32` rolling-ball result carrier, resolved through its exact
    /// stored value and derivative jet.
    RollingBall {
        /// Persistent object id of the `a8 03 32` result carrier.
        carrier_object_id: u32,
        /// Exact procedural definition decoded from the stored jet.
        definition: ProceduralSurfaceDefinition,
    },
}

/// A `b5 03 30` offset construction with an explicit result carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct B5OffsetSurface {
    /// This construction's result surface id.
    pub object_id: u32,
    /// Explicit analytic carrier for the offset result.
    pub carrier_surface: u32,
    /// Surface from which the result is offset.
    pub source_surface: u32,
    /// Signed offset distance in millimetres.
    pub distance: f64,
    /// Native carrier-kind discriminator.
    pub carrier_kind: u8,
    /// Ordered native U and V bounds.
    pub parameter_bounds: [[f64; 2]; 2],
}

/// A `b5 03 2c` extrusion construction with a two-support directrix.
#[derive(Debug, Clone, PartialEq)]
pub struct B5ExtrusionSurface {
    /// This construction's result surface id.
    pub object_id: u32,
    /// Unit world-space extrusion direction.
    pub direction: [f64; 3],
    /// Increasing native U and V intervals.
    pub parameter_bounds: [[f64; 2]; 2],
    /// Exact directrix construction.
    pub directrix: B5ExtrusionDirectrix,
}

/// Exact directrix construction selected by a `b5 03 2c` extrusion.
#[derive(Debug, Clone, PartialEq)]
pub enum B5ExtrusionDirectrix {
    /// Two-support intersection carried by an `a8 03 25` record.
    Intersection {
        /// Persistent directrix object id.
        object_id: u32,
        /// Ordered `(surface, pcurve, pcurve range)` support sides.
        supports: [(u32, u32, [f64; 2]); 2],
        /// Increasing solved-curve parameter range.
        parameter_range: [f64; 2],
        /// Positive fit tolerance of the serialized sampled cache.
        cache_fit_tolerance: f64,
    },
    /// One-support curve carried by a `b5 03 24` wrapper.
    SurfaceCurve {
        /// Persistent wrapper object id.
        object_id: u32,
        /// `(surface, pcurve, pcurve range)` support side.
        support: (u32, u32, [f64; 2]),
        /// Increasing curve parameter range.
        parameter_range: [f64; 2],
    },
    /// Fixed-direction offset carried by a `b5 03 14` record.
    Offset {
        /// Persistent offset-curve object id.
        object_id: u32,
        /// Complete source curve construction.
        source: Box<B5ExtrusionDirectrix>,
        /// Increasing interval on the source curve.
        source_parameter_range: [f64; 2],
        /// Signed offset distance.
        distance: f64,
        /// Unit direction defining the positive offset side.
        direction: [f64; 3],
        /// Increasing result-curve parameter range.
        parameter_range: [f64; 2],
    },
}

impl B5ExtrusionDirectrix {
    pub(crate) fn object_id(&self) -> u32 {
        match self {
            Self::Intersection { object_id, .. }
            | Self::SurfaceCurve { object_id, .. }
            | Self::Offset { object_id, .. } => *object_id,
        }
    }

    fn parameter_range(&self) -> [f64; 2] {
        match self {
            Self::Intersection {
                parameter_range, ..
            }
            | Self::SurfaceCurve {
                parameter_range, ..
            }
            | Self::Offset {
                parameter_range, ..
            } => *parameter_range,
        }
    }

    fn reorigin_parameter_range(&mut self, range: [f64; 2]) -> bool {
        match self {
            Self::SurfaceCurve {
                parameter_range, ..
            }
            | Self::Offset {
                parameter_range, ..
            } => {
                *parameter_range = range;
                true
            }
            Self::Intersection { .. } => false,
        }
    }

    pub(crate) fn supports(&self) -> Vec<(u32, u32, [f64; 2])> {
        match self {
            Self::Intersection { supports, .. } => supports.to_vec(),
            Self::SurfaceCurve { support, .. } => vec![*support],
            Self::Offset { source, .. } => source.supports(),
        }
    }
}

/// A class-`37` support-bound surface construction with an explicit result carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct B5SupportedSurface {
    /// This construction's result surface id.
    pub object_id: u32,
    /// Explicit carrier for the result geometry and chart.
    pub carrier_surface: u32,
    /// Ordered construction support surfaces.
    pub support_surfaces: [u32; 2],
    /// Ordered pcurves, one bound to each support surface.
    pub support_pcurves: [u32; 2],
    /// Class-specific native controls and scalar parameters.
    pub parameters: B5SupportedSurfaceParameters,
}

/// Native parameter layouts of a support-bound surface construction.
#[derive(Debug, Clone, PartialEq)]
pub enum B5SupportedSurfaceParameters {
    /// Class `37`: interleaved controls, a positive construction radius, and
    /// a zero scalar.
    Radius {
        /// Six control bytes surrounding the scalar fields.
        controls: [u8; 6],
        /// Positive radius of the support-bound construction.
        construction_radius: f64,
    },
    /// Class `3b`: six contiguous controls followed by two positive scalars.
    ScalarPair {
        /// Six contiguous control bytes.
        controls: [u8; 6],
        /// Two positive construction scalars.
        scalars: [f64; 2],
    },
}

/// One class-`06` incidence lane connecting curves to parameters at a vertex.
#[derive(Debug, Clone, PartialEq)]
pub struct B5ParameterIncidence {
    /// This record's stream object id.
    pub object_id: u32,
    /// Ordered curve or pcurve references.
    pub curves: Vec<u32>,
    /// Finite native parameters aligned with `curves`.
    pub parameters: Vec<f64>,
    /// Compact native controls aligned with `curves`.
    pub controls: Vec<u32>,
}

/// One complete class-`5e` physical-edge reference production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct B5Edge {
    /// This record's stream object id.
    pub object_id: u32,
    /// Referenced curve-support wrapper.
    pub support: u32,
    /// Ordered start/end class-`5d` vertex identities.
    pub vertices: [u32; 2],
    /// Ordered start/end class-`06` parameter-incidence identities.
    pub parameter_incidences: [u32; 2],
    /// Exact admitted terminal control.
    pub terminal_control: u8,
}

/// One complete class-`5d` vertex-to-incidence reference production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct B5VertexIncidenceLink {
    /// This record's stream object id.
    pub object_id: u32,
    /// Referenced counted class-`05` incidence roster.
    pub incidence: u32,
    /// Exact admitted terminal control.
    pub terminal_control: u8,
}

/// A resolved `b5 03 18`, `b5 03 19`, or `b5 03 21` pcurve node, represented as a 2D
/// B-spline curve in a surface's
/// parameter space ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq)]
pub struct B5Pcurve {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// `object_id` of the owning surface, taken directly from the pcurve's
    /// `catia_support_ref` ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
    pub surface: u32,
    /// B-spline degree.
    pub degree: u32,
    /// Distinct knot values, strictly increasing.
    pub distinct_knots: Vec<f64>,
    /// Per-knot multiplicities, index-aligned with `distinct_knots`.
    pub multiplicities: Vec<u32>,
    /// `(u, v)` control points in the surface's parameter space.
    pub control_points: Vec<[f64; 2]>,
    /// Per-pole rational weights. `None` denotes a polynomial pcurve.
    pub weights: Option<Vec<f64>>,
    /// Explicit native parameter interval when the pcurve record stores one.
    pub parameter_range: Option<[f64; 2]>,
    /// Positive scalar stored in the exact class-`21` suffix.
    pub class_21_suffix_scalar: Option<f64>,
    /// The curve's two clamped-end poles lifted through `surface` into
    /// world-frame 3D points, or `None` before [`parse`] resolves them or
    /// when the lift fails (unresolved surface, degenerate revolution
    /// scale, or NURBS evaluation failure).
    pub lifted_endpoints: Option<[[f64; 3]; 2]>,
}

/// Exact great-circle fields carried by a class-`1d` sphere pcurve.
#[derive(Debug, Clone, PartialEq)]
pub struct B5SphereGreatCirclePcurve {
    /// Length-valued bounds of the curve in the sphere chart.
    pub chart_bounds: [[f64; 2]; 2],
    /// Length-valued shift contributing to the great-circle plane phase.
    pub chart_shift: f64,
    /// Length scale converting the sphere chart's angular coordinates.
    pub chart_scale: f64,
    /// Signed slope in `tan(latitude) = slope * cos(azimuth - phase)`.
    pub slope: f64,
    /// Stored phase term. The geometric phase is `chart_shift / chart_scale + phase`.
    pub phase: f64,
}

/// An identity- and support-resolved pcurve whose native chart equation is
/// opaque or only partly assigned.
#[derive(Debug, Clone, PartialEq)]
pub struct B5OpaquePcurve {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// Owning surface object id.
    pub surface: u32,
    /// Native pcurve class.
    pub class: u8,
    /// Exact source payload.
    pub payload: Vec<u8>,
    /// Exact great-circle carrier fields when this is a validated class-`1d`
    /// sphere pcurve.
    pub sphere_great_circle: Option<B5SphereGreatCirclePcurve>,
}

/// One length-framed `b5 03` record as found by the stream walk ([spec §6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#6-object-stream-record-framing-a5-03-a8-03-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5Record {
    /// Byte offset of the `b5 03` marker in the source stream.
    pub offset: usize,
    /// Record family byte (`0xb5` or `0xa8`).
    pub family: u8,
    /// Third header byte: the record's type/class code (`0x5f` face,
    /// `0x62` loop, `0x21` pcurve, `0x27`/`0x28`/`0x2d` surface, `0x5e`
    /// edge, `0x18` line pcurve, `0x0e`/`0x0f` profile, ...).
    pub class: u8,
    /// Dense creation-order `object_id` stored inline at `+4` ([spec §6.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#65-a8-03-common-object-stream-freeform-class)).
    pub object_id: u32,
    /// Raw record payload after the 8-byte header.
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(crate) struct ObjectFrame {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) family: u8,
    pub(crate) class: u8,
    pub(crate) object_id: u32,
}

/// A resolved `b5 03 5f` face node ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5Face {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// `object_id` of the face's surface, taken from the first reference
    /// token.
    pub surface: u32,
    /// `object_id`s of the face's `b5 03 62` loop nodes, in reference
    /// order.
    pub loops: Vec<u32>,
    /// Exact terminal control of the counted face production. Uncounted face
    /// framing has no terminal control.
    pub terminal_control: Option<u8>,
}

/// Count the face incidences for each object-stream loop.
pub(crate) fn face_loop_owner_counts(faces: &[B5Face]) -> HashMap<u32, usize> {
    let mut owners = HashMap::new();
    for face in faces {
        for &loop_id in &face.loops {
            *owners.entry(loop_id).or_insert(0) += 1;
        }
    }
    owners
}

/// One structurally complete class-`5f` face reference production.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5FaceRecord {
    /// This record's stream object id.
    pub object_id: u32,
    /// Ordered native references.
    pub references: Vec<u32>,
    /// Exact counted-production terminal control. Uncounted framing has no
    /// terminal control.
    pub terminal_control: Option<u8>,
}

/// A resolved `b5 03 62` loop node: payload `<0x80 + n_refs>
/// (pcurve_ref edge_ref)* surface_ref` ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq)]
pub struct B5Loop {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// `object_id`s of the loop's member pcurves (or `0x18` lines), in
    /// serialized order.
    pub pcurves: Vec<u32>,
    /// `object_id`s of the loop's member `b5 03 5e` edges, index-aligned
    /// with `pcurves`.
    pub edges: Vec<u32>,
    /// Complete source-native framing, per-occurrence controls, and optional
    /// numeric extension.
    pub metadata: B5LoopMetadata,
    /// `object_id` of the loop's surface (the trailing reference token).
    pub surface: u32,
}

/// Complete metadata following a class-`62` loop's reference lanes.
#[derive(Debug, Clone, PartialEq)]
pub struct B5LoopMetadata {
    /// Primary and secondary loop framing controls.
    pub framing_controls: [u8; 2],
    /// Three signed controls for each edge occurrence, in loop order.
    pub edge_controls: Vec<[i16; 3]>,
    /// Optional fixed-width numeric extension.
    pub extension: Option<B5LoopMetadataExtension>,
}

/// Optional fixed-width numeric extension of a class-`62` loop.
#[derive(Debug, Clone, PartialEq)]
pub struct B5LoopMetadataExtension {
    /// Four finite binary64 fields in serialized order.
    pub scalars: [f64; 4],
    /// Exact admitted odd extension control.
    pub control: u8,
    /// Six finite binary32 fields in serialized order.
    pub floats: [f32; 6],
}

impl B5Loop {
    pub(crate) fn edge_senses(&self) -> Vec<bool> {
        self.metadata
            .edge_controls
            .iter()
            .map(|controls| controls[0] == -1)
            .collect()
    }

    pub(crate) fn pcurve_senses(&self) -> Vec<bool> {
        self.metadata
            .edge_controls
            .iter()
            .map(|controls| controls[2] == -1)
            .collect()
    }
}

/// Resolve the dominant object-stream topology graph through inline object ids.
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<B5Graph> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    parse_from_records(bytes, &records, &frames, true)
}

pub(crate) fn parse_from_frames(bytes: &[u8], frames: &[ObjectFrame]) -> Option<B5Graph> {
    let records = records_from_frames(bytes, frames);
    parse_from_records(bytes, &records, frames, true)
}

pub(crate) fn parse_from_records(
    bytes: &[u8],
    records: &[B5Record],
    frames: &[ObjectFrame],
    require_topology: bool,
) -> Option<B5Graph> {
    let by_id: HashMap<u32, &B5Record> = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect();
    if records.is_empty() || by_id.len() != records.len() {
        return None;
    }
    let object_stream_pcurve_jets = crate::families::a5a8::records::object_stream_pcurves(bytes);
    let mut object_stream_pcurve_candidates = BTreeMap::new();
    let mut conflicting_object_stream_pcurves = HashSet::new();
    let mut object_stream_pcurve_classes = HashMap::<u32, Option<u8>>::new();
    for jet in &object_stream_pcurve_jets {
        let Some(candidate) = object_stream_pcurve_candidate(jet) else {
            continue;
        };
        object_stream_pcurve_classes
            .entry(candidate.object_id)
            .and_modify(|class| {
                if *class != Some(0x20) {
                    *class = None;
                }
            })
            .or_insert(Some(0x20));
        merge_pcurve_candidate(
            &mut object_stream_pcurve_candidates,
            &mut conflicting_object_stream_pcurves,
            candidate,
        );
    }
    for candidate in a8_class21_pcurves_from_frames(bytes, frames) {
        object_stream_pcurve_classes
            .entry(candidate.object_id)
            .and_modify(|class| {
                if *class != Some(0x21) {
                    *class = None;
                }
            })
            .or_insert(Some(0x21));
        merge_pcurve_candidate(
            &mut object_stream_pcurve_candidates,
            &mut conflicting_object_stream_pcurves,
            candidate,
        );
    }
    let a8_pcurve_supports = object_stream_pcurve_candidates
        .iter()
        .map(|(&object_id, pcurve)| (object_id, pcurve.surface))
        .collect::<HashMap<_, _>>();
    let a8_headers: BTreeMap<u32, crate::families::a5a8::records::A8SurfaceHeader> =
        crate::families::a5a8::records::a8_surface_headers(bytes)
            .into_iter()
            .map(|header| (header.object_id, header))
            .collect();
    let mut surfaces: BTreeMap<u32, B5Surface> = records
        .iter()
        .filter_map(|record| {
            surface_node(record, a8_headers.get(&record.object_id))
                .map(|surface| (record.object_id, surface))
        })
        .collect();
    let mut conflicting_surfaces = HashSet::new();
    for surface_id in topology_surface_references(records) {
        if surfaces.contains_key(&surface_id) {
            continue;
        }
        let Some(record) = by_id
            .get(&surface_id)
            .filter(|record| record.family == 0xb5 && is_opaque_surface_class(record.class))
        else {
            continue;
        };
        surfaces.insert(
            surface_id,
            B5Surface::Unknown {
                family: record.family,
                class: record.class,
                payload: record.payload.clone(),
            },
        );
    }
    for surface in crate::families::a5a8::records::resolved_a8_surfaces(bytes) {
        if let (Some(object_id), SurfaceGeometry::Nurbs(nurbs)) =
            (surface.object_id(), surface.geometry)
        {
            merge_surface_candidate(
                &mut surfaces,
                &mut conflicting_surfaces,
                object_id,
                B5Surface::Nurbs(nurbs),
            );
        }
    }
    for jet in crate::families::a5a8::records::a8_freeform_curves(bytes) {
        if let Some(definition) = crate::families::a5a8::records::rolling_ball_jet_definition(&jet)
        {
            merge_surface_candidate(
                &mut surfaces,
                &mut conflicting_surfaces,
                jet.object_id,
                B5Surface::RollingBall {
                    carrier_object_id: jet.object_id,
                    definition,
                },
            );
        }
    }
    let object_stream_pcurves = object_stream_pcurve_candidates
        .iter()
        .filter_map(|(&object_id, pcurve)| {
            let class = object_stream_pcurve_classes
                .get(&object_id)
                .copied()
                .flatten()?;
            Some((
                object_id,
                B5ObjectStreamPcurve {
                    class,
                    surface: pcurve.surface,
                    parameter_range: pcurve.parameter_range?,
                    class_21_suffix_scalar: pcurve.class_21_suffix_scalar,
                    distinct_knots: pcurve.distinct_knots.clone(),
                },
            ))
        })
        .collect();
    let offset_constructions = records
        .iter()
        .filter_map(parse_offset_surface_fields)
        .collect::<Vec<_>>();
    let mut extrusion_surfaces = BTreeMap::<u32, B5ExtrusionSurface>::new();
    loop {
        let mut changed = false;
        for record in records {
            if extrusion_surfaces.contains_key(&record.object_id) {
                continue;
            }
            let Some(extrusion) = parse_extrusion_surface_with_context(
                record,
                &by_id,
                &object_stream_pcurves,
                &offset_constructions,
                &extrusion_surfaces,
            ) else {
                continue;
            };
            extrusion_surfaces.insert(record.object_id, extrusion);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    let extrusion_pcurves = extrusion_surfaces
        .values()
        .flat_map(|extrusion| {
            extrusion
                .directrix
                .supports()
                .into_iter()
                .map(|(_, pcurve, _)| pcurve)
        })
        .collect::<HashSet<_>>();
    let mut offset_surfaces = BTreeMap::new();
    let mut supported_surfaces = BTreeMap::new();
    loop {
        let mut changed =
            resolve_surface_aliases(records, &by_id, &mut surfaces, &mut conflicting_surfaces);
        for record in records {
            let Some(offset) = parse_offset_surface(record, &surfaces, &extrusion_surfaces, &by_id)
            else {
                continue;
            };
            let carrier = if let Some(carrier) = surfaces.get(&offset.carrier_surface).cloned() {
                carrier
            } else {
                let Some(record) = by_id.get(&offset.carrier_surface) else {
                    continue;
                };
                B5Surface::Unknown {
                    family: record.family,
                    class: record.class,
                    payload: record.payload.clone(),
                }
            };
            let before = surfaces.get(&record.object_id).cloned();
            if !merge_surface_candidate(
                &mut surfaces,
                &mut conflicting_surfaces,
                offset.carrier_surface,
                carrier.clone(),
            ) || !merge_surface_candidate(
                &mut surfaces,
                &mut conflicting_surfaces,
                record.object_id,
                carrier,
            ) {
                continue;
            }
            let surface_changed = surfaces.get(&record.object_id) != before.as_ref();
            let metadata_changed = offset_surfaces.get(&record.object_id) != Some(&offset);
            offset_surfaces.insert(record.object_id, offset);
            changed |= surface_changed || metadata_changed;
        }
        for record in records {
            let Some(construction) = parse_supported_surface(record) else {
                continue;
            };
            let Some(carrier) = surfaces.get(&construction.carrier_surface).cloned() else {
                continue;
            };
            let parameters_match_carrier =
                supported_surface_parameters_match_carrier(&construction.parameters, &carrier);
            let before = surfaces.get(&record.object_id).cloned();
            if merge_surface_candidate(
                &mut surfaces,
                &mut conflicting_surfaces,
                record.object_id,
                carrier,
            ) && parameters_match_carrier
                && supported_surface_pcurves_match(&construction, &by_id, &a8_pcurve_supports)
                && construction
                    .support_surfaces
                    .iter()
                    .all(|surface| surfaces.contains_key(surface))
            {
                let surface_changed = surfaces.get(&record.object_id) != before.as_ref();
                let metadata_changed =
                    supported_surfaces.get(&record.object_id) != Some(&construction);
                supported_surfaces.insert(record.object_id, construction);
                changed |= surface_changed || metadata_changed;
            }
        }
        changed |=
            resolve_surface_aliases(records, &by_id, &mut surfaces, &mut conflicting_surfaces);
        if !changed {
            break;
        }
    }
    offset_surfaces.retain(|object_id, offset| {
        surfaces.get(object_id) == surfaces.get(&offset.carrier_surface)
    });
    supported_surfaces.retain(|object_id, construction| {
        surfaces.get(object_id) == surfaces.get(&construction.carrier_surface)
    });
    let profiles: BTreeMap<u32, B5Profile> = records
        .iter()
        .filter_map(|record| parse_profile(record).map(|profile| (record.object_id, profile)))
        .collect();
    let mut pcurves: BTreeMap<u32, B5Pcurve> = records
        .iter()
        .filter_map(|record| {
            let pcurve = match record.class {
                0x18 => parse_line_pcurve(record),
                0x19 => parse_circle_pcurve(record),
                0x1a => parse_class_1a_pcurve(record),
                0x21 => parse_pcurve(record),
                _ => None,
            }?;
            Some((record.object_id, pcurve))
        })
        .collect();
    let mut conflicting_pcurves = HashSet::new();
    let mut circle_candidates = BTreeMap::<u32, Vec<B5Pcurve>>::new();
    for pcurve in circle_pcurves_from_frames(bytes, frames) {
        if surfaces.contains_key(&pcurve.surface) {
            circle_candidates
                .entry(pcurve.object_id)
                .or_default()
                .push(pcurve);
        }
    }
    for (object_id, candidates) in circle_candidates {
        let mut distinct = candidates.into_iter();
        let Some(candidate) = distinct.next() else {
            continue;
        };
        if distinct.all(|other| other == candidate) {
            merge_pcurve_candidate(&mut pcurves, &mut conflicting_pcurves, candidate);
        } else {
            pcurves.remove(&object_id);
            conflicting_pcurves.insert(object_id);
        }
    }
    for candidate in object_stream_pcurve_candidates.into_values() {
        let directrix_reference = extrusion_pcurves.contains(&candidate.object_id);
        if !directrix_reference
            && by_id
                .get(&candidate.object_id)
                .is_none_or(|record| record.class != 0x20)
        {
            continue;
        }
        merge_pcurve_candidate(&mut pcurves, &mut conflicting_pcurves, candidate);
    }
    let mut opaque_pcurves: BTreeMap<u32, B5OpaquePcurve> = records
        .iter()
        .filter_map(|record| parse_opaque_pcurve(record).map(|pcurve| (record.object_id, pcurve)))
        .collect();
    for pcurve in opaque_pcurves.values_mut() {
        let Some(record) = by_id.get(&pcurve.object_id) else {
            continue;
        };
        pcurve.sphere_great_circle = surfaces
            .get(&pcurve.surface)
            .and_then(|surface| parse_sphere_great_circle_pcurve(record, surface));
    }
    let parameter_incidences: BTreeMap<u32, B5ParameterIncidence> = records
        .iter()
        .filter_map(|record| {
            parameter_incidence(record).map(|incidence| (record.object_id, incidence))
        })
        .collect();
    let edges: BTreeMap<u32, B5Edge> = records
        .iter()
        .filter(|record| record.class == 0x5e)
        .filter_map(|record| parse_edge(record).map(|edge| (record.object_id, edge)))
        .collect();
    let edge_parameter_incidences: BTreeMap<u32, [u32; 2]> = edges
        .iter()
        .filter_map(|(&object_id, edge)| {
            edge.parameter_incidences
                .iter()
                .all(|parameter| parameter_incidences.contains_key(parameter))
                .then_some((object_id, edge.parameter_incidences))
        })
        .collect();
    let implicit_pcurves =
        implicit_pcurve_bindings(records, &by_id, &pcurves, &opaque_pcurves, &surfaces);
    for pcurve in pcurves.values_mut() {
        pcurve.lifted_endpoints = surfaces
            .get(&pcurve.surface)
            .and_then(|surface| lift_pcurve_endpoints(surface, &profiles, &pcurve.control_points));
    }
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    let source_face_count = records.iter().filter(|record| record.class == 0x5f).count();
    let mut loops: BTreeMap<u32, B5Loop> = records
        .iter()
        .filter(|record| record.class == 0x62)
        .filter_map(|record| parse_loop_record(record).map(|loop_| (record.object_id, loop_)))
        .filter_map(|(object_id, record)| {
            parse_loop(
                &record,
                &by_id,
                &pcurves,
                &opaque_pcurves,
                &implicit_pcurves,
                &surfaces,
            )
            .map(|loop_| (object_id, loop_))
        })
        .collect();
    let face_records: BTreeMap<u32, B5FaceRecord> = records
        .iter()
        .filter(|record| record.class == 0x5f)
        .filter_map(|record| parse_face_record(record).map(|face| (record.object_id, face)))
        .collect();
    let faces: Vec<B5Face> = records
        .iter()
        .filter_map(|record| face_records.get(&record.object_id))
        .filter_map(|record| parse_face(record, &loops, &surfaces))
        .collect();
    if require_topology && (faces.is_empty() || loops.is_empty()) {
        return None;
    }
    let vertex_points = crate::families::consolidated::records::object_stream_vertices(bytes)
        .into_iter()
        .map(|point| [point.x, point.y, point.z])
        .collect::<Vec<_>>();
    let geometric_edge_vertices = bind_edge_vertices(&loops, &geometry, &vertex_points);
    let vertex_incidence_links: BTreeMap<u32, B5VertexIncidenceLink> = records
        .iter()
        .filter(|record| record.class == 0x5d)
        .filter_map(|record| {
            parse_vertex_incidence_link(record).map(|link| (record.object_id, link))
        })
        .collect();
    let native_edge_vertices: BTreeMap<u32, [u32; 2]> = edges
        .iter()
        .map(|(&object_id, edge)| (object_id, edge.vertices))
        .collect();
    let native_vertex_coordinates = incidence_vertex_coordinates(
        &native_edge_vertices,
        &vertex_incidence_links,
        &by_id,
        &geometry,
    );
    let bound_vertices = bind_native_vertices(
        &loops,
        &geometry,
        &native_edge_vertices,
        &geometric_edge_vertices,
        &native_vertex_coordinates,
        &vertex_points,
    );
    let edge_vertices = bound_vertices.edges;
    let logical_vertex_refs = bound_vertices.refs;
    let logical_vertex_points = bound_vertices.points;
    let vertex_tolerances = bound_vertices.tolerances;
    let referenced_loops: std::collections::HashSet<u32> = faces
        .iter()
        .flat_map(|face| face.loops.iter().copied())
        .collect();
    loops.retain(|loop_id, _| referenced_loops.contains(loop_id));
    let complete = faces.len() == source_face_count
        && face_loop_owner_counts(&faces)
            .values()
            .all(|count| *count == 1)
        && referenced_loops.iter().all(|loop_id| {
            loops.get(loop_id).is_some_and(|loop_| {
                loop_
                    .pcurves
                    .iter()
                    .zip(&loop_.edges)
                    .all(|(pcurve, edge)| {
                        (pcurves
                            .get(pcurve)
                            .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                            || opaque_pcurves
                                .get(pcurve)
                                .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                            || implicit_pcurves.get(pcurve) == Some(&loop_.surface))
                            && edge_vertices.contains_key(edge)
                    })
                    && loop_chain_closes(loop_, &edge_vertices)
            })
        });
    let surface_aliases = records
        .iter()
        .filter(|record| surfaces.contains_key(&record.object_id))
        .filter_map(|record| surface_alias_target(record).map(|target| (record.object_id, target)))
        .collect();
    Some(B5Graph {
        complete,
        faces,
        face_records,
        loops,
        pcurves,
        opaque_pcurves,
        implicit_pcurves,
        surfaces,
        surface_aliases,
        offset_surfaces,
        extrusion_surfaces,
        supported_surfaces,
        parameter_incidences,
        edges,
        vertex_incidence_links,
        vertex_points,
        logical_vertex_points,
        logical_vertex_refs,
        edge_vertices,
        edge_parameter_incidences,
        vertex_tolerances,
        profiles,
    })
}

fn merge_pcurve_candidate(
    pcurves: &mut BTreeMap<u32, B5Pcurve>,
    conflicts: &mut HashSet<u32>,
    candidate: B5Pcurve,
) {
    let object_id = candidate.object_id;
    if conflicts.contains(&object_id) {
        return;
    }
    match pcurves.entry(object_id) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(candidate);
        }
        std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &candidate => {}
        std::collections::btree_map::Entry::Occupied(entry) => {
            entry.remove();
            conflicts.insert(object_id);
        }
    }
}

fn merge_surface_candidate(
    surfaces: &mut BTreeMap<u32, B5Surface>,
    conflicts: &mut HashSet<u32>,
    object_id: u32,
    candidate: B5Surface,
) -> bool {
    if conflicts.contains(&object_id) {
        return false;
    }
    match surfaces.entry(object_id) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(candidate);
        }
        std::collections::btree_map::Entry::Occupied(mut entry)
            if unresolved_surface_candidate(entry.get()) =>
        {
            entry.insert(candidate);
        }
        std::collections::btree_map::Entry::Occupied(entry)
            if unresolved_surface_candidate(&candidate) || entry.get() == &candidate => {}
        std::collections::btree_map::Entry::Occupied(entry) => {
            entry.remove();
            conflicts.insert(object_id);
            return false;
        }
    }
    true
}

fn unresolved_surface_candidate(surface: &B5Surface) -> bool {
    matches!(
        surface,
        B5Surface::Unknown { .. } | B5Surface::UnresolvedNurbs { .. }
    )
}

fn resolve_surface_aliases(
    records: &[B5Record],
    by_id: &HashMap<u32, &B5Record>,
    surfaces: &mut BTreeMap<u32, B5Surface>,
    conflicts: &mut HashSet<u32>,
) -> bool {
    let mut changed = false;
    for record in records {
        if surface_alias_target(record).is_none() {
            continue;
        }
        let Some(candidate) = surface_alias_carrier(record.object_id, by_id, surfaces) else {
            continue;
        };
        let before = surfaces.get(&record.object_id).cloned();
        if merge_surface_candidate(surfaces, conflicts, record.object_id, candidate)
            && surfaces.get(&record.object_id) != before.as_ref()
        {
            changed = true;
        }
    }
    changed
}

fn surface_alias_carrier(
    mut object_id: u32,
    by_id: &HashMap<u32, &B5Record>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Option<B5Surface> {
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(object_id) {
            return None;
        }
        let Some(record) = by_id.get(&object_id) else {
            return surfaces
                .get(&object_id)
                .filter(|surface| !unresolved_surface_candidate(surface))
                .cloned();
        };
        let Some(target) = surface_alias_target(record) else {
            return surfaces
                .get(&object_id)
                .filter(|surface| !unresolved_surface_candidate(surface))
                .cloned();
        };
        object_id = target;
    }
}

fn object_stream_pcurve_candidate(
    jet: &crate::families::a5a8::records::A8Pcurve,
) -> Option<B5Pcurve> {
    let (_, control_points) = crate::nurbs::quintic_jet_bspline(
        jet.degree,
        &jet.knots,
        &jet.points,
        &jet.first_derivatives,
        &jet.second_derivatives,
    )?;
    Some(B5Pcurve {
        object_id: jet.object_id,
        surface: jet.support_id,
        degree: jet.degree,
        distinct_knots: jet.knots.clone(),
        multiplicities: vec![jet.degree + 1; jet.knots.len()],
        control_points,
        weights: None,
        parameter_range: Some(jet.range),
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    })
}

#[cfg(test)]
fn a8_class21_pcurves(bytes: &[u8]) -> Vec<B5Pcurve> {
    let frames = object_stream_frames(bytes);
    a8_class21_pcurves_from_frames(bytes, &frames)
}

fn a8_class21_pcurves_from_frames(bytes: &[u8], frames: &[ObjectFrame]) -> Vec<B5Pcurve> {
    let mut pcurves = Vec::new();
    for frame in frames {
        if frame.family != 0xa8 || frame.class != 0x21 {
            continue;
        }
        if let Some(pcurve) =
            parse_a8_class21_pcurve(frame.object_id, &bytes[frame.start + 11..frame.end])
        {
            pcurves.push(pcurve);
        }
    }
    pcurves
}

fn parse_a8_class21_pcurve(object_id: u32, payload: &[u8]) -> Option<B5Pcurve> {
    (payload.first() == Some(&0x81)).then_some(())?;
    let mut position = 1;
    let surface = wire::object_ref(payload, &mut position, true)?;
    (payload.get(position) == Some(&0x01)).then_some(())?;
    position += 1;
    let degree = wire::compact_uint(payload, &mut position)?;
    (degree == 5 && payload.get(position..position + 2) == Some(&[0x01, 0x01])).then_some(())?;
    position += 2;
    let knot_count = usize::try_from(wire::compact_uint(payload, &mut position)?).ok()?;
    (knot_count >= 2).then_some(())?;
    matches!(payload.get(position), Some(0x01 | 0x11 | 0x19)).then_some(())?;
    position += 1;
    let scalar_bytes = knot_count.checked_mul(8)?;
    let minimum_known_bytes = scalar_bytes
        .checked_mul(7)?
        .checked_add(knot_count)?
        .checked_add(36)?;
    if position.checked_add(minimum_known_bytes)? > payload.len() {
        return None;
    }
    let read_values = |position: &mut usize| -> Option<Vec<f64>> {
        let mut values = Vec::with_capacity(knot_count);
        for _ in 0..knot_count {
            values.push(scalar(payload, *position)?);
            *position = position.checked_add(8)?;
        }
        Some(values)
    };
    let distinct_knots = read_values(&mut position)?;
    distinct_knots
        .windows(2)
        .all(|pair| pair[0] < pair[1])
        .then_some(())?;
    let multiplicities = (0..knot_count)
        .map(|_| wire::compact_uint(payload, &mut position))
        .collect::<Option<Vec<_>>>()?;
    (multiplicities.first() == Some(&(degree + 1))
        && multiplicities.last() == Some(&(degree + 1))
        && multiplicities[1..knot_count - 1]
            .iter()
            .all(|multiplicity| *multiplicity == 3))
    .then_some(())?;
    let u = read_values(&mut position)?;
    let v = read_values(&mut position)?;
    let du = read_values(&mut position)?;
    let dv = read_values(&mut position)?;
    let ddu = read_values(&mut position)?;
    let ddv = read_values(&mut position)?;
    let points = u
        .into_iter()
        .zip(v)
        .map(|(u, v)| [u, v])
        .collect::<Vec<_>>();
    let first = du
        .into_iter()
        .zip(dv)
        .map(|(u, v)| [u, v])
        .collect::<Vec<_>>();
    let second = ddu
        .into_iter()
        .zip(ddv)
        .map(|(u, v)| [u, v])
        .collect::<Vec<_>>();
    let (_, control_points) =
        crate::nurbs::quintic_jet_bspline(degree, &distinct_knots, &points, &first, &second)?;
    let tail = payload.get(position..)?;
    let tail_control = tail.get(..2);
    let extension_control = tail.get(34..36);
    (matches!(tail.len(), 36 | 38)
        && (tail_control == Some(&[0x05, 0x05]) || tail_control == Some(&[0x05, 0x11]))
        && scalar(tail, 2)? == 0.0
        && scalar(tail, 10)? > 0.0
        && scalar(tail, 18)? == 1.0
        && scalar(tail, 26)? == 0.0
        && (tail.len() == 36
            || extension_control == Some(&[0x01, 0x11])
            || extension_control == Some(&[0x01, 0x19]))
        && tail.get(tail.len() - 2..) == Some(&[0x00, 0x07]))
    .then_some(())?;
    let parameter_range = [*distinct_knots.first()?, *distinct_knots.last()?];
    Some(B5Pcurve {
        object_id,
        surface,
        degree,
        distinct_knots,
        multiplicities: vec![degree + 1; knot_count],
        control_points,
        weights: None,
        parameter_range: Some(parameter_range),
        class_21_suffix_scalar: Some(scalar(tail, 10)?),
        lifted_endpoints: None,
    })
}

/// Return native start/end vertex identities for every framed `b5 03 5e`
/// edge, keyed by the edge object id.
#[must_use]
pub fn edge_vertex_references(bytes: &[u8]) -> BTreeMap<u32, [u32; 2]> {
    let mut edges = BTreeMap::new();
    let mut ambiguous = HashSet::new();
    for frame in object_stream_frames(bytes) {
        if frame.family != 0xb5 || frame.class != 0x5e {
            continue;
        }
        let record = B5Record {
            offset: frame.start,
            family: 0xb5,
            class: 0x5e,
            object_id: frame.object_id,
            payload: bytes[frame.start + 8..frame.end].to_vec(),
        };
        let Some(edge) = parse_edge(&record) else {
            continue;
        };
        let vertices = edge.vertices;
        if edges
            .insert(frame.object_id, vertices)
            .is_some_and(|existing| existing != vertices)
        {
            ambiguous.insert(frame.object_id);
        }
    }
    edges.retain(|object_id, _| !ambiguous.contains(object_id));
    edges
}

/// Return the ordered pcurve pair owned by each requested native edge's
/// class-`23` curve-support wrapper.
#[must_use]
#[cfg(test)]
pub(crate) fn edge_support_pcurve_references(
    bytes: &[u8],
    edge_ids: &HashSet<u32>,
) -> BTreeMap<u32, [u32; 2]> {
    let frames = object_stream_frames(bytes);
    edge_support_pcurve_references_from_frames(bytes, edge_ids, &frames)
}

pub(crate) fn edge_support_pcurve_references_from_frames(
    bytes: &[u8],
    edge_ids: &HashSet<u32>,
    frames: &[ObjectFrame],
) -> BTreeMap<u32, [u32; 2]> {
    let mut edge_wrappers = HashMap::<u32, Option<u32>>::new();
    let mut wrappers = HashMap::<u32, Option<[u32; 2]>>::new();
    for frame in frames {
        let header = if frame.family == 0xa8 { 11 } else { 8 };
        let record = B5Record {
            offset: frame.start,
            family: frame.family,
            class: frame.class,
            object_id: frame.object_id,
            payload: bytes[frame.start + header..frame.end].to_vec(),
        };
        if frame.class == 0x5e && edge_ids.contains(&frame.object_id) {
            let Some(wrapper) = parse_edge(&record).map(|edge| edge.support) else {
                continue;
            };
            edge_wrappers
                .entry(frame.object_id)
                .and_modify(|stored| {
                    if stored.is_some_and(|stored| stored != wrapper) {
                        *stored = None;
                    }
                })
                .or_insert(Some(wrapper));
        } else if frame.family == 0xb5 && frame.class == 0x23 {
            let Some(references) = record_references(&record).try_into().ok() else {
                continue;
            };
            wrappers
                .entry(frame.object_id)
                .and_modify(|stored| {
                    if stored.is_some_and(|stored| stored != references) {
                        *stored = None;
                    }
                })
                .or_insert(Some(references));
        }
    }
    edge_wrappers
        .into_iter()
        .filter_map(|(edge, wrapper)| Some((edge, *wrappers.get(&wrapper?)?.as_ref()?)))
        .collect()
}

pub(crate) fn targeted_surfaces_from_frames(
    bytes: &[u8],
    object_ids: &HashSet<u32>,
    frames: &[ObjectFrame],
) -> BTreeMap<u32, B5Surface> {
    let mut resolved = HashMap::<u32, Option<B5Surface>>::new();
    for surface in crate::families::a5a8::records::resolved_a8_surfaces(bytes) {
        let Some(object_id) = surface.object_id() else {
            continue;
        };
        let SurfaceGeometry::Nurbs(nurbs) = surface.geometry else {
            continue;
        };
        merge_targeted_surface(&mut resolved, object_id, B5Surface::Nurbs(nurbs));
    }
    let headers = crate::families::a5a8::records::a8_surface_headers(bytes)
        .into_iter()
        .map(|header| (header.object_id, header))
        .collect::<HashMap<_, _>>();
    let mut records = HashMap::<u32, Option<B5Record>>::new();
    for frame in frames {
        if !is_surface_class(frame.class) {
            continue;
        }
        let header = if frame.family == 0xa8 { 11 } else { 8 };
        let record = B5Record {
            offset: frame.start,
            family: frame.family,
            class: frame.class,
            object_id: frame.object_id,
            payload: bytes[frame.start + header..frame.end].to_vec(),
        };
        records
            .entry(frame.object_id)
            .and_modify(|stored| {
                if stored.as_ref().is_some_and(|stored| {
                    stored.family != record.family
                        || stored.class != record.class
                        || stored.payload != record.payload
                }) {
                    *stored = None;
                }
            })
            .or_insert(Some(record));
    }
    let mut rolling = HashMap::<u32, Option<B5Surface>>::new();
    for jet in crate::families::a5a8::records::a8_freeform_curves(bytes) {
        let Some(definition) = crate::families::a5a8::records::rolling_ball_jet_definition(&jet)
        else {
            continue;
        };
        merge_targeted_surface(
            &mut rolling,
            jet.object_id,
            B5Surface::RollingBall {
                carrier_object_id: jet.object_id,
                definition,
            },
        );
    }
    object_ids
        .iter()
        .filter_map(|&object_id| {
            resolve_targeted_surface(object_id, &records, &headers, &resolved, &rolling)
                .map(|surface| (object_id, surface))
        })
        .collect()
}

/// Resolve the unique length-closed geometry construction frames independently
/// of the dominant topology run.
#[must_use]
#[cfg(test)]
pub(crate) fn targeted_geometry_graph(bytes: &[u8]) -> Option<B5Graph> {
    let frames = object_stream_frames(bytes);
    targeted_geometry_graph_from_frames(bytes, &frames)
}

pub(crate) fn targeted_geometry_graph_from_frames(
    bytes: &[u8],
    frames: &[ObjectFrame],
) -> Option<B5Graph> {
    let mut candidates = HashMap::<u32, Option<B5Record>>::new();
    for frame in frames {
        if !is_targeted_geometry_class(frame.family, frame.class) {
            continue;
        }
        let header = if frame.family == 0xa8 { 11 } else { 8 };
        let record = B5Record {
            offset: frame.start,
            family: frame.family,
            class: frame.class,
            object_id: frame.object_id,
            payload: bytes[frame.start + header..frame.end].to_vec(),
        };
        candidates
            .entry(frame.object_id)
            .and_modify(|stored| {
                if stored.as_ref().is_some_and(|stored| {
                    stored.family != record.family
                        || stored.class != record.class
                        || stored.payload != record.payload
                }) {
                    *stored = None;
                }
            })
            .or_insert(Some(record));
    }
    let mut records = candidates.into_values().flatten().collect::<Vec<_>>();
    records.sort_by_key(|record| record.offset);
    parse_from_records(bytes, &records, frames, false)
}

fn is_targeted_geometry_class(family: u8, class: u8) -> bool {
    match family {
        0xb5 => matches!(
            class,
            0x0e | 0x0f
                | 0x14
                | 0x18
                | 0x19
                | 0x1a
                | 0x1d
                | 0x21
                | 0x24
                | 0x27
                | 0x28
                | 0x29
                | 0x2a
                | 0x2c
                | 0x2d
                | 0x2e
                | 0x30
                | 0x31
                | 0x37
                | 0x38
                | 0x3b
        ),
        0xa8 => matches!(class, 0x20 | 0x21 | 0x25 | 0x32 | 0x34),
        _ => false,
    }
}

fn merge_targeted_surface(
    candidates: &mut HashMap<u32, Option<B5Surface>>,
    object_id: u32,
    surface: B5Surface,
) {
    candidates
        .entry(object_id)
        .and_modify(|stored| {
            if stored.as_ref().is_some_and(|stored| stored != &surface) {
                *stored = None;
            }
        })
        .or_insert(Some(surface));
}

fn resolve_targeted_surface(
    object_id: u32,
    records: &HashMap<u32, Option<B5Record>>,
    headers: &HashMap<u32, crate::families::a5a8::records::A8SurfaceHeader>,
    resolved: &HashMap<u32, Option<B5Surface>>,
    rolling: &HashMap<u32, Option<B5Surface>>,
) -> Option<B5Surface> {
    resolve_targeted_surface_inner(
        object_id,
        records,
        headers,
        resolved,
        rolling,
        HashSet::new(),
    )
}

fn resolve_targeted_surface_inner(
    mut object_id: u32,
    records: &HashMap<u32, Option<B5Record>>,
    headers: &HashMap<u32, crate::families::a5a8::records::A8SurfaceHeader>,
    resolved: &HashMap<u32, Option<B5Surface>>,
    rolling: &HashMap<u32, Option<B5Surface>>,
    mut visited: HashSet<u32>,
) -> Option<B5Surface> {
    loop {
        if !visited.insert(object_id) || records.get(&object_id).is_some_and(Option::is_none) {
            return None;
        }
        match (
            rolling.get(&object_id).cloned().flatten(),
            resolved.get(&object_id).cloned().flatten(),
        ) {
            (Some(left), Some(right)) if left != right => return None,
            (Some(surface), _) | (_, Some(surface)) => return Some(surface),
            (None, None) => {}
        }
        let record = records.get(&object_id)?.as_ref()?;
        if let Some(target) = surface_alias_target(record) {
            object_id = target;
            continue;
        }
        if let Some(construction) = parse_supported_surface(record) {
            object_id = construction.carrier_surface;
            continue;
        }
        if record.family == 0xb5 && record.class == 0x30 {
            return resolve_targeted_analytic_offset(
                record, records, headers, resolved, rolling, &visited,
            );
        }
        return surface_node(record, headers.get(&object_id));
    }
}

fn resolve_targeted_analytic_offset(
    record: &B5Record,
    records: &HashMap<u32, Option<B5Record>>,
    headers: &HashMap<u32, crate::families::a5a8::records::A8SurfaceHeader>,
    resolved: &HashMap<u32, Option<B5Surface>>,
    rolling: &HashMap<u32, Option<B5Surface>>,
    visited: &HashSet<u32>,
) -> Option<B5Surface> {
    (record.payload.first() == Some(&0x82)).then_some(())?;
    let mut position = 1;
    let carrier_id = wire::object_ref(&record.payload, &mut position, true)?;
    let source_id = wire::object_ref(&record.payload, &mut position, true)?;
    let carrier = resolve_targeted_surface_inner(
        carrier_id,
        records,
        headers,
        resolved,
        rolling,
        visited.clone(),
    )?;
    let mut surfaces = BTreeMap::from([(carrier_id, carrier.clone())]);
    if !matches!(carrier, B5Surface::RollingBall { .. }) {
        let source = resolve_targeted_surface_inner(
            source_id,
            records,
            headers,
            resolved,
            rolling,
            visited.clone(),
        )?;
        surfaces.insert(source_id, source);
    }
    parse_offset_surface(record, &surfaces, &BTreeMap::new(), &HashMap::new())?;
    Some(carrier)
}

fn parse_edge(record: &B5Record) -> Option<B5Edge> {
    (record.class == 0x5e && record.payload.first() == Some(&0x85)).then_some(())?;
    let mut position = 1;
    let references: [u32; 5] = (0..5)
        .map(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let &[terminal_control] = record.payload.get(position..)? else {
        return None;
    };
    matches!(
        terminal_control,
        0x01 | 0x02 | 0x21 | 0x22 | 0x25 | 0x26 | 0x29 | 0x2a
    )
    .then_some(B5Edge {
        object_id: record.object_id,
        support: references[0],
        vertices: [references[1], references[2]],
        parameter_incidences: [references[3], references[4]],
        terminal_control,
    })
}

struct B5PcurveContext<'a> {
    pcurves: &'a BTreeMap<u32, B5Pcurve>,
    opaque_pcurves: &'a BTreeMap<u32, B5OpaquePcurve>,
    surfaces: &'a BTreeMap<u32, B5Surface>,
    profiles: &'a BTreeMap<u32, B5Profile>,
    edge_parameter_incidences: &'a BTreeMap<u32, [u32; 2]>,
    parameter_incidences: &'a BTreeMap<u32, B5ParameterIncidence>,
}

fn incidence_vertex_coordinates(
    native_edges: &BTreeMap<u32, [u32; 2]>,
    vertex_incidence_links: &BTreeMap<u32, B5VertexIncidenceLink>,
    by_id: &HashMap<u32, &B5Record>,
    geometry: &B5PcurveContext<'_>,
) -> BTreeMap<u32, [f64; 3]> {
    native_edges
        .values()
        .flatten()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .filter_map(|vertex| {
            let incidence = vertex_incidence_links.get(&vertex)?.incidence;
            let incidence_records = counted_references(by_id.get(&incidence)?, 0x05)?;
            let points = incidence_records
                .into_iter()
                .map(|incidence_record| {
                    let incidence = parameter_incidence(by_id.get(&incidence_record)?)?;
                    let points = incidence
                        .curves
                        .into_iter()
                        .zip(incidence.parameters)
                        .map(|(pcurve_id, parameter)| {
                            lift_parameter_incidence(pcurve_id, parameter, geometry)
                        })
                        .collect::<Option<Vec<_>>>()?;
                    (!points.is_empty()).then_some(points)
                })
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            let point = *points.first()?;
            let tolerance_squared = POINT_TOLERANCE * POINT_TOLERANCE;
            points
                .iter()
                .all(|candidate| distance_squared(*candidate, point) <= tolerance_squared)
                .then_some((vertex, point))
        })
        .collect()
}

/// Evaluate one class-`06` curve/parameter pair at its native parameter
/// domain. A vertex roster is valid only when every pair evaluates, so one
/// unsupported, malformed, or out-of-domain member withholds the identity
/// coordinate instead of allowing an earlier member to win.
fn lift_parameter_incidence(
    pcurve_id: u32,
    parameter: f64,
    geometry: &B5PcurveContext<'_>,
) -> Option<[f64; 3]> {
    if let Some(pcurve) = geometry.pcurves.get(&pcurve_id) {
        let domain = pcurve_parameter_domain(pcurve)?;
        (parameter.is_finite() && parameter >= domain[0] && parameter <= domain[1]).then_some(())?;
        let uv = evaluate_pcurve(pcurve, parameter)?;
        let point = lift_pcurve_endpoints(
            geometry.surfaces.get(&pcurve.surface)?,
            geometry.profiles,
            &[uv, uv],
        )?[0];
        return point.into_iter().all(f64::is_finite).then_some(point);
    }
    let opaque = geometry.opaque_pcurves.get(&pcurve_id)?;
    let point = sphere_great_circle_point(
        opaque.sphere_great_circle.as_ref()?,
        geometry.surfaces.get(&opaque.surface)?,
        parameter,
    )?;
    point.into_iter().all(f64::is_finite).then_some(point)
}

fn parse_vertex_incidence_link(record: &B5Record) -> Option<B5VertexIncidenceLink> {
    (record.class == 0x5d && record.payload.first() == Some(&0x81)).then_some(())?;
    let mut position = 1;
    let incidence = wire::object_ref(&record.payload, &mut position, true)?;
    let &[terminal_control] = record.payload.get(position..)? else {
        return None;
    };
    matches!(terminal_control, 0x00 | 0x04).then_some(B5VertexIncidenceLink {
        object_id: record.object_id,
        incidence,
        terminal_control,
    })
}

fn counted_references(record: &B5Record, class: u8) -> Option<Vec<u32>> {
    (record.class == class).then_some(())?;
    let (references, position) = wire::counted_refs(&record.payload, true)?;
    (position == record.payload.len()).then_some(references)
}

fn parameter_incidence(record: &B5Record) -> Option<B5ParameterIncidence> {
    (record.class == 0x06).then_some(())?;
    let count = usize::from(record.payload.first()?.checked_sub(0x80)?);
    let mut position = 1;
    let references = (0..count)
        .map(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?;
    (record.payload.get(position) == Some(&(0x80u8.checked_add(u8::try_from(count).ok()?)?)))
        .then_some(())?;
    position += 1;
    let mut parameters = Vec::with_capacity(count);
    let mut controls = Vec::with_capacity(count);
    for _ in 0..count {
        let parameter = scalar(&record.payload, position)?;
        if !parameter.is_finite() {
            return None;
        }
        parameters.push(parameter);
        position += 8;
        controls.push(wire::compact_uint(&record.payload, &mut position)?);
    }
    (position == record.payload.len()).then_some(B5ParameterIncidence {
        object_id: record.object_id,
        curves: references,
        parameters,
        controls,
    })
}

fn implicit_pcurve_bindings(
    records: &[B5Record],
    by_id: &HashMap<u32, &B5Record>,
    pcurves: &BTreeMap<u32, B5Pcurve>,
    opaque_pcurves: &BTreeMap<u32, B5OpaquePcurve>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> BTreeMap<u32, u32> {
    let mut bindings = BTreeMap::new();
    let mut ambiguous = HashSet::new();
    for record in records.iter().filter(|record| record.class == 0x62) {
        let Some(references) = loop_references(record) else {
            continue;
        };
        let Some((&surface, occurrences)) = references.split_last() else {
            continue;
        };
        if !surfaces.contains_key(&surface) {
            continue;
        }
        for occurrence in occurrences.chunks_exact(2) {
            let pcurve = occurrence[0];
            if pcurves.contains_key(&pcurve)
                || opaque_pcurves.contains_key(&pcurve)
                || by_id.contains_key(&pcurve)
            {
                continue;
            }
            let Some(edge) = by_id.get(&occurrence[1]).and_then(|edge| parse_edge(edge)) else {
                continue;
            };
            let endpoint_incidence_contains = |reference_id| {
                by_id
                    .get(&reference_id)
                    .and_then(|incidence| parameter_incidence(incidence))
                    .is_some_and(|incidence| incidence.curves.contains(&pcurve))
            };
            let curve_wrapper_contains = by_id.get(&edge.support).is_some_and(|wrapper| {
                matches!(wrapper.class, 0x23..=0x25) && record_references(wrapper).contains(&pcurve)
            });
            if !(curve_wrapper_contains
                || endpoint_incidence_contains(edge.parameter_incidences[0])
                    && endpoint_incidence_contains(edge.parameter_incidences[1]))
            {
                continue;
            }
            if bindings
                .insert(pcurve, surface)
                .is_some_and(|existing| existing != surface)
            {
                ambiguous.insert(pcurve);
            }
        }
    }
    bindings.retain(|pcurve, _| !ambiguous.contains(pcurve));
    bindings
}

pub(crate) fn evaluate_pcurve(pcurve: &B5Pcurve, parameter: f64) -> Option<[f64; 2]> {
    let knots = pcurve_knots(pcurve)?;
    let control_points: Vec<Point2> = pcurve
        .control_points
        .iter()
        .map(|point| Point2::new(point[0], point[1]))
        .collect();
    let point = nurbs_pcurve_uv(
        pcurve.degree,
        &knots,
        &control_points,
        pcurve.weights.as_deref(),
        parameter,
    )?;
    Some([point.u, point.v])
}

fn pcurve_knots(pcurve: &B5Pcurve) -> Option<Vec<f64>> {
    let mut knots = Vec::new();
    for (&knot, &multiplicity) in pcurve.distinct_knots.iter().zip(&pcurve.multiplicities) {
        knots.extend(std::iter::repeat_n(
            knot,
            usize::try_from(multiplicity).ok()?,
        ));
    }
    Some(knots)
}

fn pcurve_parameter_domain(pcurve: &B5Pcurve) -> Option<[f64; 2]> {
    let knots = pcurve_knots(pcurve)?;
    let degree = usize::try_from(pcurve.degree).ok()?;
    let spline_domain = [
        *knots.get(degree)?,
        *knots
            .len()
            .checked_sub(degree + 1)
            .and_then(|index| knots.get(index))?,
    ];
    if !spline_domain.into_iter().all(f64::is_finite) || spline_domain[0] >= spline_domain[1] {
        return None;
    }
    match pcurve.parameter_range {
        Some(range) => bounded_occurrence_range(range, spline_domain),
        None => Some(spline_domain),
    }
}

/// Clamp a finite occurrence range to a finite, increasing native domain.
pub(crate) fn bounded_occurrence_range(parameters: [f64; 2], domain: [f64; 2]) -> Option<[f64; 2]> {
    const RELATIVE_PARAMETER_TOLERANCE: f64 = 1e-10;

    let domain_span = domain[1] - domain[0];
    if !domain.into_iter().all(f64::is_finite)
        || !domain_span.is_finite()
        || domain_span <= 0.0
        || !parameters.into_iter().all(f64::is_finite)
        || parameters[0] == parameters[1]
    {
        return None;
    }
    let tolerance = RELATIVE_PARAMETER_TOLERANCE * domain_span;
    if parameters
        .iter()
        .any(|parameter| *parameter < domain[0] - tolerance || *parameter > domain[1] + tolerance)
    {
        return None;
    }
    Some(parameters.map(|parameter| parameter.clamp(domain[0], domain[1])))
}

struct BoundNativeVertices {
    edges: BTreeMap<u32, [usize; 2]>,
    refs: Vec<u32>,
    points: Vec<[f64; 3]>,
    tolerances: BTreeMap<usize, f64>,
}

fn bind_native_vertices(
    loops: &BTreeMap<u32, B5Loop>,
    geometry: &B5PcurveContext<'_>,
    native_edges: &BTreeMap<u32, [u32; 2]>,
    geometric_edges: &BTreeMap<u32, [usize; 2]>,
    native_coordinates: &BTreeMap<u32, [f64; 3]>,
    points: &[[f64; 3]],
) -> BoundNativeVertices {
    let constraints: Vec<([u32; 2], [usize; 2])> = native_edges
        .iter()
        .filter_map(|(edge, vertices)| geometric_edges.get(edge).map(|points| (*vertices, *points)))
        .collect();
    let mut adjacency = HashMap::<u32, Vec<usize>>::new();
    for (index, (vertices, _)) in constraints.iter().enumerate() {
        adjacency.entry(vertices[0]).or_default().push(index);
        adjacency.entry(vertices[1]).or_default().push(index);
    }
    let vertex_points = propagate_vertex_points(&constraints, &adjacency, points);
    let mut logical_coordinates: HashMap<u32, [f64; 3]> = vertex_points
        .into_iter()
        .map(|(vertex, point)| (vertex, points[point]))
        .collect();
    logical_coordinates.extend(native_coordinates);
    let mut endpoint_candidates = HashMap::<u32, Vec<(u32, [f64; 3])>>::new();
    let mut invalid_edges = HashSet::new();
    for loop_ in loops.values() {
        for (&pcurve, &edge) in loop_.pcurves.iter().zip(&loop_.edges) {
            let (Some(vertices), Some(lifted)) = (
                native_edges.get(&edge),
                pcurve_endpoints(pcurve, edge, geometry),
            ) else {
                continue;
            };
            if lifted
                .iter()
                .flatten()
                .any(|coordinate| !coordinate.is_finite())
            {
                invalid_edges.insert(edge);
                continue;
            }
            for lane in 0..2 {
                if let Some(existing) = logical_coordinates.get(&vertices[lane]) {
                    if !endpoint_matches(*existing, lifted[lane]) {
                        invalid_edges.insert(edge);
                    }
                } else {
                    endpoint_candidates
                        .entry(vertices[lane])
                        .or_default()
                        .push((edge, lifted[lane]));
                }
            }
        }
    }
    for (vertex, candidates) in endpoint_candidates {
        let Some((_, point)) = candidates.first().copied() else {
            continue;
        };
        if candidates
            .iter()
            .all(|(_, candidate)| endpoint_matches(point, *candidate))
        {
            logical_coordinates.insert(vertex, point);
        } else {
            invalid_edges.extend(candidates.into_iter().map(|(edge, _)| edge));
        }
    }
    let mut logical_vertices: Vec<_> = logical_coordinates.into_iter().collect();
    logical_vertices.sort_unstable_by_key(|(vertex, _)| *vertex);
    let logical_vertex_indices: HashMap<u32, usize> = logical_vertices
        .iter()
        .enumerate()
        .map(|(rank, (vertex, _))| (*vertex, points.len() + rank))
        .collect();
    let logical_vertex_points: Vec<[f64; 3]> =
        logical_vertices.iter().map(|(_, point)| *point).collect();
    let logical_vertex_refs = logical_vertices.iter().map(|(vertex, _)| *vertex).collect();
    let mut edge_vertices = geometric_edges.clone();
    edge_vertices.retain(|edge, _| !invalid_edges.contains(edge));
    for (&edge, vertices) in native_edges {
        if invalid_edges.contains(&edge) {
            continue;
        }
        if let (Some(&start), Some(&end)) = (
            logical_vertex_indices.get(&vertices[0]),
            logical_vertex_indices.get(&vertices[1]),
        ) {
            edge_vertices.insert(edge, [start, end]);
        }
    }
    let mut tolerances = BTreeMap::<usize, f64>::new();
    for loop_ in loops.values() {
        for (&pcurve, &edge) in loop_.pcurves.iter().zip(&loop_.edges) {
            let Some(lifted) = pcurve_endpoints(pcurve, edge, geometry) else {
                continue;
            };
            let Some(&loci) = edge_vertices.get(&edge) else {
                continue;
            };
            let residuals = [
                (
                    loci[0],
                    distance_squared(
                        vertex_coordinate(points, &logical_vertex_points, loci[0]),
                        lifted[0],
                    )
                    .sqrt(),
                ),
                (
                    loci[1],
                    distance_squared(
                        vertex_coordinate(points, &logical_vertex_points, loci[1]),
                        lifted[1],
                    )
                    .sqrt(),
                ),
            ];
            for (locus, residual) in residuals {
                if residual > POINT_TOLERANCE && residual.is_finite() {
                    tolerances
                        .entry(locus)
                        .and_modify(|tolerance| *tolerance = tolerance.max(residual + 1e-9))
                        .or_insert(residual + 1e-9);
                }
            }
        }
    }
    BoundNativeVertices {
        edges: edge_vertices,
        refs: logical_vertex_refs,
        points: logical_vertex_points,
        tolerances,
    }
}

fn propagate_vertex_points(
    constraints: &[([u32; 2], [usize; 2])],
    adjacency: &HashMap<u32, Vec<usize>>,
    points: &[[f64; 3]],
) -> HashMap<u32, usize> {
    let mut mapping = HashMap::<u32, usize>::new();
    let mut completed = HashSet::new();
    for seed in 0..constraints.len() {
        if completed.contains(&seed) {
            continue;
        }
        let (component, members, consistent) =
            propagate_vertex_component(seed, constraints, adjacency, points);
        completed.extend(members);
        if consistent {
            mapping.extend(component);
        }
    }
    mapping
}

fn propagate_vertex_component(
    seed: usize,
    constraints: &[([u32; 2], [usize; 2])],
    adjacency: &HashMap<u32, Vec<usize>>,
    points: &[[f64; 3]],
) -> (HashMap<u32, usize>, Vec<usize>, bool) {
    let mut mapping = HashMap::new();
    let mut members = Vec::new();
    let mut pending = std::collections::VecDeque::new();
    let mut visited = HashSet::new();
    let mut consistent = true;
    let assign = |mapping: &mut HashMap<u32, usize>,
                  pending: &mut std::collections::VecDeque<u32>,
                  consistent: &mut bool,
                  vertex,
                  locus| {
        if let Some(&previous) = mapping.get(&vertex) {
            let residual = distance_squared(points[previous], points[locus]);
            if !residual.is_finite() || residual > POINT_TOLERANCE * POINT_TOLERANCE {
                *consistent = false;
            }
        } else {
            mapping.insert(vertex, locus);
            pending.push_back(vertex);
        }
    };
    let (seed_vertices, seed_loci) = constraints[seed];
    assign(
        &mut mapping,
        &mut pending,
        &mut consistent,
        seed_vertices[0],
        seed_loci[0],
    );
    assign(
        &mut mapping,
        &mut pending,
        &mut consistent,
        seed_vertices[1],
        seed_loci[1],
    );
    while let Some(vertex) = pending.pop_front() {
        for &index in adjacency.get(&vertex).into_iter().flatten() {
            if !visited.insert(index) {
                continue;
            }
            members.push(index);
            let (vertices, loci) = constraints[index];
            for lane in 0..2 {
                assign(
                    &mut mapping,
                    &mut pending,
                    &mut consistent,
                    vertices[lane],
                    loci[lane],
                );
            }
        }
    }
    (mapping, members, consistent)
}

fn vertex_coordinate(points: &[[f64; 3]], logical_points: &[[f64; 3]], index: usize) -> [f64; 3] {
    if index < points.len() {
        points[index]
    } else {
        logical_points[index - points.len()]
    }
}

pub(crate) fn loop_chain_closes(loop_: &B5Loop, edge_vertices: &BTreeMap<u32, [usize; 2]>) -> bool {
    let edge_senses = loop_.edge_senses();
    if loop_.edges.is_empty() || loop_.edges.len() != edge_senses.len() {
        return false;
    }
    let Some(first) = edge_vertices.get(&loop_.edges[0]) else {
        return false;
    };
    let first_reversed = usize::from(edge_senses[0]);
    let initial = first[first_reversed];
    let mut current = first[1 - first_reversed];
    for (edge, reversed) in loop_.edges[1..].iter().zip(&edge_senses[1..]) {
        let Some(endpoints) = edge_vertices.get(edge) else {
            return false;
        };
        let reversed = usize::from(*reversed);
        if endpoints[reversed] != current {
            return false;
        }
        current = endpoints[1 - reversed];
    }
    current == initial
}

fn parse_profile(record: &B5Record) -> Option<B5Profile> {
    (record.family == 0xb5).then_some(())?;
    match record.class {
        0x0e => {
            (record.payload.len() == 73 && record.payload.first() == Some(&0x80)).then_some(())?;
            let direction = point(&record.payload, 25)?;
            let parameter_range = [scalar(&record.payload, 57)?, scalar(&record.payload, 65)?];
            (direction_is_unit(direction)
                && scalar(&record.payload, 49)? == 1.0
                && parameter_range[0] < parameter_range[1])
                .then_some(B5Profile::Line {
                    point: point(&record.payload, 1)?,
                    direction,
                    parameter_range,
                })
        }
        0x0f => {
            (record.payload.len() == 113 && record.payload.first() == Some(&0x80)).then_some(())?;
            let direction_x = point(&record.payload, 25)?;
            let direction_y = point(&record.payload, 49)?;
            let radius = scalar(&record.payload, 73)?;
            let parameter_range = [scalar(&record.payload, 81)?, scalar(&record.payload, 89)?];
            let chart_origin = scalar(&record.payload, 105)?;
            (radius > 0.0
                && directions_are_unit_and_orthogonal(direction_x, direction_y)
                && periodic_angular_range_is_valid(
                    [parameter_range[0] / radius, parameter_range[1] / radius],
                    [
                        chart_origin / radius,
                        chart_origin / radius + std::f64::consts::TAU,
                    ],
                )
                && scalar(&record.payload, 97)? == 1.0)
                .then_some(B5Profile::Arc {
                    center: point(&record.payload, 1)?,
                    direction_x,
                    direction_y,
                    radius,
                    parameter_range,
                })
        }
        _ => None,
    }
}

fn bind_edge_vertices(
    loops: &BTreeMap<u32, B5Loop>,
    geometry: &B5PcurveContext<'_>,
    points: &[[f64; 3]],
) -> BTreeMap<u32, [usize; 2]> {
    let point_index = point_index(points);
    let mut edges: BTreeMap<u32, [usize; 2]> = BTreeMap::new();
    let mut conflicts = HashSet::new();
    for loop_ in loops.values() {
        for (&pcurve_id, &edge_id) in loop_.pcurves.iter().zip(&loop_.edges) {
            if conflicts.contains(&edge_id) {
                continue;
            }
            let Some(endpoints) = pcurve_endpoints(pcurve_id, edge_id, geometry) else {
                continue;
            };
            let indices: Option<[usize; 2]> = endpoints
                .map(|endpoint| canonical_point(points, &point_index, endpoint))
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .and_then(|indices| indices.try_into().ok());
            let Some(indices) = indices else {
                continue;
            };
            if let Some(previous) = edges.get(&edge_id) {
                let mut previous_sorted = *previous;
                let mut current_sorted = indices;
                previous_sorted.sort_unstable();
                current_sorted.sort_unstable();
                if previous_sorted != current_sorted {
                    edges.remove(&edge_id);
                    conflicts.insert(edge_id);
                }
            } else {
                edges.insert(edge_id, indices);
            }
        }
    }
    edges
}

fn pcurve_endpoints(
    pcurve_id: u32,
    edge_id: u32,
    geometry: &B5PcurveContext<'_>,
) -> Option<[[f64; 3]; 2]> {
    if let Some(pcurve) = geometry.pcurves.get(&pcurve_id) {
        let Some(parameters) = edge_pcurve_parameter_values(
            geometry.edge_parameter_incidences,
            geometry.parameter_incidences,
            edge_id,
            pcurve_id,
        )
        .and_then(|parameters| {
            bounded_occurrence_range(parameters, pcurve_parameter_domain(pcurve)?)
        }) else {
            return pcurve.lifted_endpoints;
        };
        let uv = [
            evaluate_pcurve(pcurve, parameters[0])?,
            evaluate_pcurve(pcurve, parameters[1])?,
        ];
        return lift_pcurve_endpoints(
            geometry.surfaces.get(&pcurve.surface)?,
            geometry.profiles,
            &uv,
        );
    }
    let opaque = geometry.opaque_pcurves.get(&pcurve_id)?;
    let pcurve = opaque.sphere_great_circle.as_ref()?;
    let surface = geometry.surfaces.get(&opaque.surface)?;
    let [start, end] = edge_pcurve_parameter_values(
        geometry.edge_parameter_incidences,
        geometry.parameter_incidences,
        edge_id,
        pcurve_id,
    )
    .and_then(|parameters| bounded_occurrence_range(parameters, pcurve.chart_bounds[0]))
    .unwrap_or(pcurve.chart_bounds[0]);
    Some([
        sphere_great_circle_point(pcurve, surface, start)?,
        sphere_great_circle_point(pcurve, surface, end)?,
    ])
}

/// CATIA's object-stream on-carrier incidence tolerance, in millimetres.
const POINT_TOLERANCE: f64 = 1e-3;

fn point_cell(point: [f64; 3]) -> [i64; 3] {
    point.map(|coordinate| (coordinate / POINT_TOLERANCE).floor() as i64)
}

fn point_index(points: &[[f64; 3]]) -> HashMap<[i64; 3], Vec<usize>> {
    let mut index = HashMap::<[i64; 3], Vec<usize>>::new();
    for (point_index, point) in points.iter().enumerate() {
        index
            .entry(point_cell(*point))
            .or_default()
            .push(point_index);
    }
    index
}

fn canonical_point(
    points: &[[f64; 3]],
    index: &HashMap<[i64; 3], Vec<usize>>,
    endpoint: [f64; 3],
) -> Option<usize> {
    let cell = point_cell(endpoint);
    let mut matches = Vec::new();
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let neighbor = [
                    cell[0].saturating_add(dx),
                    cell[1].saturating_add(dy),
                    cell[2].saturating_add(dz),
                ];
                matches.extend(index.get(&neighbor).into_iter().flatten().filter_map(
                    |&point_index| {
                        (distance_squared(points[point_index], endpoint)
                            <= POINT_TOLERANCE * POINT_TOLERANCE)
                            .then_some(point_index)
                    },
                ));
            }
        }
    }
    matches.into_iter().min()
}

fn distance_squared(left: [f64; 3], right: [f64; 3]) -> f64 {
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2)
}

fn endpoint_matches(left: [f64; 3], right: [f64; 3]) -> bool {
    let residual = distance_squared(left, right);
    residual.is_finite() && residual <= POINT_TOLERANCE * POINT_TOLERANCE
}

fn direction_is_unit(direction: [f64; 3]) -> bool {
    (distance_squared(direction, [0.0; 3]) - 1.0).abs() <= 1e-12
}

fn directions_are_unit_and_orthogonal(first: [f64; 3], second: [f64; 3]) -> bool {
    let dot = first
        .iter()
        .zip(second)
        .map(|(left, right)| left * right)
        .sum::<f64>();
    direction_is_unit(first) && direction_is_unit(second) && dot.abs() <= 1e-12
}

fn directions_form_right_handed_orthonormal_frame(
    direction_x: [f64; 3],
    direction_y: [f64; 3],
    axis: [f64; 3],
) -> bool {
    directions_are_unit_and_orthogonal(direction_x, direction_y)
        && distance_squared(cross(direction_x, direction_y), axis) <= 1e-24
}

fn parse_surface(record: &B5Record) -> Option<B5Surface> {
    (record.family == 0xb5).then_some(())?;
    match record.class {
        0x27 => {
            (record.payload.len() == 121 && record.payload.first() == Some(&0x80)).then_some(())?;
            let direction_u = point(&record.payload, 25)?;
            let direction_v = point(&record.payload, 49)?;
            let u_range = [scalar(&record.payload, 89)?, scalar(&record.payload, 97)?];
            let v_range = [scalar(&record.payload, 105)?, scalar(&record.payload, 113)?];
            (directions_are_unit_and_orthogonal(direction_u, direction_v)
                && scalar(&record.payload, 73)? == 1.0
                && scalar(&record.payload, 81)? == 1.0
                && u_range[0] < u_range[1]
                && v_range[0] < v_range[1])
                .then_some(B5Surface::Plane {
                    origin: point(&record.payload, 1)?,
                    direction_u,
                    direction_v,
                    u_range,
                    v_range,
                })
        }
        0x28 => {
            (record.payload.len() == 137 && record.payload.first() == Some(&0x80)).then_some(())?;
            let stored_u = point(&record.payload, 25)?;
            let stored_v = point(&record.payload, 49)?;
            let radius = scalar(&record.payload, 73)?;
            let u_range = [scalar(&record.payload, 81)?, scalar(&record.payload, 89)?];
            let v_range = [scalar(&record.payload, 97)?, scalar(&record.payload, 105)?];
            let angular_factor = scalar(&record.payload, 113)?;
            let chart_origin = scalar(&record.payload, 129)?;
            let angular_scale = radius / angular_factor;
            let chart_domain = [
                chart_origin,
                chart_origin + std::f64::consts::TAU * angular_scale,
            ];
            chart_domain[1].is_finite().then_some(())?;
            let chart_tolerance = 1e-12
                * u_range
                    .into_iter()
                    .chain(chart_domain)
                    .chain([angular_scale])
                    .map(f64::abs)
                    .fold(1.0, f64::max);
            (radius > 0.0
                && directions_are_unit_and_orthogonal(stored_u, stored_v)
                && angular_factor > 0.0
                && angular_scale.is_finite()
                && scalar(&record.payload, 121)? == 1.0
                && u_range[0] < u_range[1]
                && u_range[0] >= chart_domain[0] - chart_tolerance
                && u_range[1] <= chart_domain[1] + chart_tolerance
                && v_range[0] < v_range[1])
                .then_some(B5Surface::Cylinder {
                    origin: point(&record.payload, 1)?,
                    reference_x: unit(stored_u)?,
                    axis: unit(cross(stored_u, stored_v))?,
                    radius,
                    u_range,
                    v_range,
                    angular_scale,
                    chart_origin,
                })
        }
        0x29 => {
            (record.payload.len() == 185 && record.payload.first() == Some(&0x80)).then_some(())?;
            let apex = point(&record.payload, 1)?;
            let direction_x = point(&record.payload, 25)?;
            let direction_y = point(&record.payload, 49)?;
            let axis = point(&record.payload, 73)?;
            let frame_cross = cross(direction_x, direction_y);
            let opposite_axis = [-axis[0], -axis[1], -axis[2]];
            let half_angle = scalar(&record.payload, 97)?;
            let pre_angular_range_scalar = scalar(&record.payload, 105)?;
            let angular_range = [scalar(&record.payload, 113)?, scalar(&record.payload, 121)?];
            let mut slant_range = [scalar(&record.payload, 129)?, scalar(&record.payload, 137)?];
            if slant_range[0].abs() <= 1e-12 {
                slant_range[0] = 0.0;
            }
            let angular_scale = scalar(&record.payload, 145)?;
            let angular_domain = [scalar(&record.payload, 169)?, scalar(&record.payload, 177)?];
            ((distance_squared(frame_cross, axis) <= 4e-24
                || distance_squared(frame_cross, opposite_axis) <= 4e-24)
                && directions_are_unit_and_orthogonal(direction_x, direction_y)
                && 0.0 < half_angle
                && half_angle < std::f64::consts::FRAC_PI_2
                && periodic_angular_range_is_valid(angular_range, angular_domain)
                && slant_range[0] >= 0.0
                && slant_range[0] < slant_range[1]
                && angular_scale > 0.0
                && scalar(&record.payload, 153)? == 1.0
                && scalar(&record.payload, 161)? == 0.0)
                .then_some(B5Surface::Cone {
                    apex,
                    direction_x,
                    direction_y,
                    axis,
                    half_angle,
                    pre_angular_range_scalar,
                    angular_range,
                    slant_range,
                    angular_scale,
                    angular_domain,
                })
        }
        0x2a => {
            (record.payload.len() == 153 && record.payload.first() == Some(&0x80)).then_some(())?;
            let center = point(&record.payload, 1)?;
            let stored_x = point(&record.payload, 25)?;
            let stored_y = point(&record.payload, 49)?;
            let stored_axis = point(&record.payload, 73)?;
            let radius = scalar(&record.payload, 97)?;
            let [azimuth_lo, azimuth_hi, latitude_lo, latitude_hi, construction_radius, chart_origin] =
                line_values::<6>(&record.payload, 105)?;
            let azimuth_range = [azimuth_lo, azimuth_hi];
            let latitude_range = [latitude_lo, latitude_hi];
            let vector_length = |value: [f64; 3]| value[0].hypot(value[1]).hypot(value[2]);
            let direction_x = unit(stored_x)?;
            let direction_y = unit(stored_y)?;
            let axis = unit(stored_axis)?;
            let expected_chart_angle =
                (azimuth_range[0] + azimuth_range[1]) * 0.5 - std::f64::consts::PI;
            let expected_chart_origin = construction_radius * expected_chart_angle;
            let chart_origin_tolerance =
                2.0 * f64::EPSILON * chart_origin.abs().max(expected_chart_origin.abs()).max(1.0);
            (radius > 0.0
                && construction_radius > 0.0
                && sphere_angular_ranges_are_valid(azimuth_range, latitude_range)
                && expected_chart_origin.is_finite()
                && (chart_origin - expected_chart_origin).abs() <= chart_origin_tolerance
                && [stored_x, stored_y, stored_axis]
                    .iter()
                    .all(|direction| ((vector_length(*direction) / radius) - 1.0).abs() <= 1e-12)
                && directions_form_right_handed_orthonormal_frame(direction_x, direction_y, axis))
            .then_some(B5Surface::Sphere {
                center,
                direction_x,
                direction_y,
                axis,
                radius,
                azimuth_range,
                latitude_range,
                construction_radius,
                chart_origin,
            })
        }
        0x2b => {
            (record.payload.len() == 201
                && record.payload.first() == Some(&0x80)
                && record.payload.get(193..201) == Some(&[0; 8]))
            .then_some(())?;
            let direction_x = point(&record.payload, 25)?;
            let direction_y = point(&record.payload, 49)?;
            let axis = point(&record.payload, 73)?;
            let major_radius = scalar(&record.payload, 97)?;
            let minor_radius = scalar(&record.payload, 105)?;
            let major_angular_range =
                [scalar(&record.payload, 113)?, scalar(&record.payload, 121)?];
            let major_angular_domain =
                [scalar(&record.payload, 129)?, scalar(&record.payload, 137)?];
            let minor_angular_range =
                [scalar(&record.payload, 145)?, scalar(&record.payload, 153)?];
            let minor_angular_domain =
                [scalar(&record.payload, 161)?, scalar(&record.payload, 169)?];
            let major_scale = scalar(&record.payload, 177)?;
            let minor_scale = scalar(&record.payload, 185)?;
            (directions_form_right_handed_orthonormal_frame(direction_x, direction_y, axis)
                && major_radius > 0.0
                && minor_radius > 0.0
                && periodic_angular_range_is_valid(major_angular_range, major_angular_domain)
                && periodic_angular_range_is_valid(minor_angular_range, minor_angular_domain)
                && major_scale > 0.0
                && minor_scale > 0.0)
                .then_some(())?;
            Some(B5Surface::Torus {
                center: point(&record.payload, 1)?,
                direction_x,
                direction_y,
                axis,
                major_radius,
                minor_radius,
                major_angular_range,
                major_angular_domain,
                minor_angular_range,
                minor_angular_domain,
                major_scale,
                minor_scale,
            })
        }
        0x2d => {
            let mut position = 1;
            let profile_curve = wire::object_ref(&record.payload, &mut position, true)?;
            (record.payload.len() == position.checked_add(171)?
                && record.payload.first() == Some(&0x81))
            .then_some(())?;
            let angular_range = [
                scalar(&record.payload, position.checked_add(96)?)?,
                scalar(&record.payload, position.checked_add(104)?)?,
            ];
            let profile_range = [
                scalar(&record.payload, position.checked_add(112)?)?,
                scalar(&record.payload, position.checked_add(120)?)?,
            ];
            let angular_scale = scalar(&record.payload, position.checked_add(130)?)?;
            let angular_half_turn = scalar(&record.payload, position.checked_add(163)?)?;
            let reference_x = point(&record.payload, position.checked_add(24)?)?;
            let reference_y = point(&record.payload, position.checked_add(48)?)?;
            let axis_direction = point(&record.payload, position.checked_add(72)?)?;
            (record.payload.get(position + 128..position + 130) == Some(&[0x05, 0x05])
                && angular_scale > 0.0
                && directions_form_right_handed_orthonormal_frame(
                    reference_x,
                    reference_y,
                    axis_direction,
                ))
            .then_some(())?;
            (angular_range[0] < angular_range[1]
                && angular_range[0] >= 0.0
                && angular_range[1] <= 2.0 * angular_half_turn
                && profile_range[0] < profile_range[1]
                && scalar(&record.payload, position + 138)? == 1.0
                && scalar(&record.payload, position + 146)? == 1.0
                && scalar(&record.payload, position + 154)? == 0.0
                && record.payload.get(position + 162) == Some(&0x01)
                && angular_half_turn.to_bits() == (std::f64::consts::PI * angular_scale).to_bits())
            .then_some(B5Surface::Revolution {
                profile_curve,
                axis_origin: point(&record.payload, position)?,
                reference_x,
                reference_y,
                axis_direction,
                profile_range,
                angular_range,
                angular_scale,
            })
        }
        _ => None,
    }
}

fn surface_node(
    record: &B5Record,
    header: Option<&crate::families::a5a8::records::A8SurfaceHeader>,
) -> Option<B5Surface> {
    parse_surface(record).or_else(|| {
        (record.family == 0xa8 && record.class == 0x34).then(|| {
            header.map_or_else(
                || B5Surface::Unknown {
                    family: record.family,
                    class: record.class,
                    payload: record.payload.clone(),
                },
                |header| B5Surface::UnresolvedNurbs {
                    header: header.clone(),
                    payload: record.payload.clone(),
                },
            )
        })
    })
}

fn surface_alias_target(record: &B5Record) -> Option<u32> {
    (record.family == 0xb5 && matches!(record.class, 0x2e | 0x38)).then_some(())?;
    let mut position = 0;
    if record.payload.first() == Some(&0x81) {
        position += 1;
    }
    let target = wire::object_ref(&record.payload, &mut position, true)?;
    if record.class == 0x38 {
        (record.payload.get(position..) == Some(&[0x05, 0x05, 0x09])).then_some(())?;
        position += 3;
    }
    (position == record.payload.len()).then_some(target)
}

fn parse_offset_surface_fields(record: &B5Record) -> Option<B5OffsetSurface> {
    (record.family == 0xb5 && record.class == 0x30 && record.payload.first() == Some(&0x82))
        .then_some(())?;
    let mut position = 1;
    let carrier_surface = wire::object_ref(&record.payload, &mut position, true)?;
    let source_surface = wire::object_ref(&record.payload, &mut position, true)?;
    let distance = scalar(&record.payload, position)?;
    position += 8;
    let carrier_kind = *record.payload.get(position)?;
    position += 1;
    let [u0, u1, v0, v1] = line_values::<4>(&record.payload, position)?;
    position += 32;
    (position == record.payload.len() && u0 < u1 && v0 < v1).then_some(B5OffsetSurface {
        object_id: record.object_id,
        carrier_surface,
        source_surface,
        distance,
        carrier_kind,
        parameter_bounds: [[u0, u1], [v0, v1]],
    })
}

fn parse_offset_surface(
    record: &B5Record,
    surfaces: &BTreeMap<u32, B5Surface>,
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
    records: &HashMap<u32, &B5Record>,
) -> Option<B5OffsetSurface> {
    let B5OffsetSurface {
        object_id,
        carrier_surface,
        source_surface,
        distance,
        carrier_kind,
        parameter_bounds: [[u0, u1], [v0, v1]],
    } = parse_offset_surface_fields(record)?;
    if carrier_kind == 0x21 {
        if let (Some(source), Some(carrier)) = (
            extrusion_surfaces.get(&source_surface),
            extrusion_surfaces.get(&carrier_surface),
        ) {
            return extrusion_offset_construction_agrees(
                source,
                carrier,
                distance,
                [[u0, u1], [v0, v1]],
            )
            .then_some(B5OffsetSurface {
                object_id,
                carrier_surface,
                source_surface,
                distance,
                carrier_kind,
                parameter_bounds: [[u0, u1], [v0, v1]],
            });
        }
    }
    let expected_kind = match surfaces.get(&carrier_surface) {
        Some(carrier @ B5Surface::Plane { .. }) => {
            analytic_offset_magnitude_agrees(carrier, surfaces.get(&source_surface)?, distance)
                .then_some(0x15)?
        }
        Some(carrier @ B5Surface::Cylinder { .. }) => {
            analytic_offset_magnitude_agrees(carrier, surfaces.get(&source_surface)?, distance)
                .then_some(0x05)?
        }
        Some(carrier @ B5Surface::Sphere { .. }) => {
            analytic_offset_magnitude_agrees(carrier, surfaces.get(&source_surface)?, distance)
                .then_some(0x09)?
        }
        Some(carrier @ B5Surface::Torus { .. }) => {
            analytic_offset_magnitude_agrees(carrier, surfaces.get(&source_surface)?, distance)
                .then_some(0x0d)?
        }
        Some(B5Surface::RollingBall { .. }) => 0x19,
        Some(B5Surface::Unknown {
            family: 0xb5,
            class: 0x2c,
            ..
        }) => return None,
        Some(_) => return None,
        None => {
            if let (Some(source), Some(carrier)) = (
                extrusion_surfaces.get(&source_surface),
                records
                    .get(&carrier_surface)
                    .and_then(|record| extrusion_carrier(record)),
            ) {
                if carrier.direction != source.direction
                    || carrier.parameter_bounds != [[v0, v1], [u0, u1]]
                {
                    return None;
                }
                return (carrier_kind == 0x21).then_some(B5OffsetSurface {
                    object_id,
                    carrier_surface,
                    source_surface,
                    distance,
                    carrier_kind,
                    parameter_bounds: [[u0, u1], [v0, v1]],
                });
            }
            let cache = parse_offset_cache(records.get(&carrier_surface)?)?;
            let source = surfaces.get(&source_surface)?;
            let cached_source = surfaces.get(&cache.source_surface)?;
            if source != cached_source
                || distance.to_bits() != cache.distance.to_bits()
                || [u0, v0, u1, v1]
                    .into_iter()
                    .zip(cache.interleaved_bounds)
                    .any(|(left, right)| left.to_bits() != right.to_bits())
            {
                return None;
            }
            0x01
        }
    };
    (carrier_kind == expected_kind).then_some(B5OffsetSurface {
        object_id,
        carrier_surface,
        source_surface,
        distance,
        carrier_kind,
        parameter_bounds: [[u0, u1], [v0, v1]],
    })
}

fn extrusion_offset_construction_agrees(
    source: &B5ExtrusionSurface,
    carrier: &B5ExtrusionSurface,
    distance: f64,
    parameter_bounds: [[f64; 2]; 2],
) -> bool {
    if carrier.direction != source.direction {
        return false;
    }
    let [[u0, u1], [v0, v1]] = parameter_bounds;
    let B5ExtrusionDirectrix::Offset {
        source: offset_source,
        source_parameter_range,
        distance: curve_distance,
        direction,
        parameter_range,
        ..
    } = &carrier.directrix
    else {
        return carrier.parameter_bounds == [[v0, v1], [u0, u1]];
    };
    offset_source.object_id() == source.directrix.object_id()
        && offset_source.supports().first().is_some_and(|support| {
            source_parameter_range
                .iter()
                .copied()
                .zip(support.2)
                .all(|(left, right)| left.to_bits() == right.to_bits())
        })
        && curve_distance.to_bits() == distance.to_bits()
        && *direction == source.direction
        && carrier.parameter_bounds[0]
            .into_iter()
            .zip([v0, v1])
            .all(|(left, right)| left.to_bits() == right.to_bits())
        && parameter_range
            .iter()
            .copied()
            .zip([u0, u1])
            .all(|(left, right)| left.to_bits() == right.to_bits())
}

fn analytic_offset_magnitude_agrees(
    carrier: &B5Surface,
    source: &B5Surface,
    distance: f64,
) -> bool {
    const RELATIVE_TOLERANCE: f64 = 1e-10;
    let relative_close = |left: f64, right: f64| {
        (left - right).abs() <= RELATIVE_TOLERANCE * left.abs().max(right.abs())
    };
    let dot = |left: [f64; 3], right: [f64; 3]| {
        left.into_iter().zip(right).map(|(a, b)| a * b).sum::<f64>()
    };
    let difference = |left: [f64; 3], right: [f64; 3]| {
        [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
    };
    let length = |value: [f64; 3]| value[0].hypot(value[1]).hypot(value[2]);
    let parallel = |left: [f64; 3], right: [f64; 3]| relative_close(dot(left, right).abs(), 1.0);
    let projected_distance_close = |measured: f64, expected: f64, delta_length: f64| {
        (measured - expected).abs()
            <= RELATIVE_TOLERANCE * measured.abs().max(expected.abs())
                + 4.0 * f64::EPSILON * delta_length
    };
    let collinear = |delta: [f64; 3], axis: [f64; 3]| {
        let delta_length = length(delta);
        if delta_length == 0.0 {
            return true;
        }
        let axial_distance = dot(delta, axis);
        let transverse = [
            delta[0] - axial_distance * axis[0],
            delta[1] - axial_distance * axis[1],
            delta[2] - axial_distance * axis[2],
        ];
        length(transverse) <= RELATIVE_TOLERANCE * delta_length
    };
    let same_point = |left: [f64; 3], right: [f64; 3], geometric_scale: f64| {
        length(difference(left, right)) <= RELATIVE_TOLERANCE * geometric_scale
    };
    match (carrier, source) {
        (
            B5Surface::Plane {
                origin: carrier_origin,
                direction_u: carrier_u,
                direction_v: carrier_v,
                ..
            },
            B5Surface::Plane {
                origin: source_origin,
                direction_u: source_u,
                direction_v: source_v,
                ..
            },
        ) => {
            let (Some(carrier_normal), Some(source_normal)) = (
                unit(cross(*carrier_u, *carrier_v)),
                unit(cross(*source_u, *source_v)),
            ) else {
                return false;
            };
            let delta = difference(*carrier_origin, *source_origin);
            parallel(carrier_normal, source_normal)
                && projected_distance_close(
                    dot(delta, source_normal).abs(),
                    distance.abs(),
                    length(delta),
                )
        }
        (
            B5Surface::Cylinder {
                origin: carrier_origin,
                axis: carrier_axis,
                radius: carrier_radius,
                ..
            },
            B5Surface::Cylinder {
                origin: source_origin,
                axis: source_axis,
                radius: source_radius,
                ..
            },
        ) => {
            let delta = difference(*carrier_origin, *source_origin);
            parallel(*carrier_axis, *source_axis)
                && collinear(delta, *source_axis)
                && relative_close((carrier_radius - source_radius).abs(), distance.abs())
        }
        (
            B5Surface::Sphere {
                center: carrier_center,
                radius: carrier_radius,
                ..
            },
            B5Surface::Sphere {
                center: source_center,
                radius: source_radius,
                ..
            },
        ) => {
            let geometric_scale = carrier_radius
                .abs()
                .max(source_radius.abs())
                .max(distance.abs());
            same_point(*carrier_center, *source_center, geometric_scale)
                && relative_close((carrier_radius - source_radius).abs(), distance.abs())
        }
        (
            B5Surface::Torus {
                center: carrier_center,
                axis: carrier_axis,
                major_radius: carrier_major,
                minor_radius: carrier_minor,
                ..
            },
            B5Surface::Torus {
                center: source_center,
                axis: source_axis,
                major_radius: source_major,
                minor_radius: source_minor,
                ..
            },
        ) => {
            let geometric_scale = carrier_major
                .abs()
                .max(source_major.abs())
                .max(carrier_minor.abs())
                .max(source_minor.abs())
                .max(distance.abs());
            same_point(*carrier_center, *source_center, geometric_scale)
                && parallel(*carrier_axis, *source_axis)
                && relative_close(*carrier_major, *source_major)
                && relative_close((carrier_minor - source_minor).abs(), distance.abs())
        }
        _ => false,
    }
}

struct B5OffsetCache {
    source_surface: u32,
    distance: f64,
    interleaved_bounds: [f64; 4],
}

struct B5ObjectStreamPcurve {
    class: u8,
    surface: u32,
    parameter_range: [f64; 2],
    class_21_suffix_scalar: Option<f64>,
    distinct_knots: Vec<f64>,
}

fn parse_offset_cache(record: &B5Record) -> Option<B5OffsetCache> {
    (record.family == 0xb5 && record.class == 0x31 && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let source_surface = wire::object_ref(&record.payload, &mut position, true)?;
    let [distance, u0, v0, u1, v1] = line_values::<5>(&record.payload, position)?;
    position += 40;
    (position == record.payload.len() && u0 < u1 && v0 < v1).then_some(B5OffsetCache {
        source_surface,
        distance,
        interleaved_bounds: [u0, v0, u1, v1],
    })
}

#[cfg(test)]
fn parse_extrusion_surface(
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Option<B5ExtrusionSurface> {
    parse_extrusion_surface_with_context(
        record,
        records,
        object_stream_pcurves,
        &[],
        &BTreeMap::new(),
    )
}

fn parse_extrusion_surface_with_context(
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
    offset_constructions: &[B5OffsetSurface],
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
) -> Option<B5ExtrusionSurface> {
    let carrier = extrusion_carrier(record)?;
    let terminal_span_chart = matches!(carrier.controls, [0x05, 0x15 | 0x19]);
    let mut directrix = if terminal_span_chart {
        terminal_span_directrix(
            carrier.directrix_id,
            carrier.parameter_bounds[1],
            carrier.controls,
            object_stream_pcurves,
        )?
    } else {
        parse_extrusion_directrix(
            records.get(&carrier.directrix_id)?,
            records,
            object_stream_pcurves,
        )?
    };
    if matches!(carrier.controls, [0x01, 0x09 | 0x15]) {
        let parameter_bounds = contextual_offset_extrusion_bounds(
            record.object_id,
            &carrier,
            &directrix,
            offset_constructions,
            extrusion_surfaces,
        )?;
        return Some(B5ExtrusionSurface {
            object_id: record.object_id,
            direction: carrier.direction,
            parameter_bounds,
            directrix,
        });
    }
    let directrix_contains_active =
        parameter_range_contains(directrix.parameter_range(), carrier.parameter_bounds[1]);
    let translated_chart = carrier.controls == [0x05, 0x11];
    if translated_chart {
        translated_directrix_span_count(
            &directrix,
            carrier.parameter_bounds[1],
            carrier.controls,
            object_stream_pcurves,
        )?;
        if !directrix.reorigin_parameter_range(carrier.parameter_bounds[1]) {
            return None;
        }
    } else if !directrix_contains_active {
        let source_span = directrix.parameter_range()[1] - directrix.parameter_range()[0];
        let active_span = carrier.parameter_bounds[1][1] - carrier.parameter_bounds[1][0];
        let suffix_span = directrix
            .supports()
            .first()
            .and_then(|support| object_stream_pcurves.get(&support.1))
            .and_then(|candidate| candidate.class_21_suffix_scalar);
        if !parameter_spans_agree(source_span, active_span)
            || !suffix_span.is_some_and(|span| parameter_spans_agree(span, active_span))
        {
            return None;
        }
        if !directrix.reorigin_parameter_range(carrier.parameter_bounds[1]) {
            return None;
        }
    }
    Some(B5ExtrusionSurface {
        object_id: record.object_id,
        direction: carrier.direction,
        parameter_bounds: carrier.parameter_bounds,
        directrix,
    })
}

fn contextual_offset_extrusion_bounds(
    carrier_surface: u32,
    carrier: &B5ExtrusionCarrier,
    directrix: &B5ExtrusionDirectrix,
    offset_constructions: &[B5OffsetSurface],
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
) -> Option<[[f64; 2]; 2]> {
    let B5ExtrusionDirectrix::Offset {
        source,
        distance,
        direction,
        parameter_range,
        ..
    } = directrix
    else {
        return None;
    };
    let mut resolved = None;
    for construction in offset_constructions.iter().filter(|construction| {
        construction.carrier_surface == carrier_surface && construction.carrier_kind == 0x21
    }) {
        let source_extrusion = extrusion_surfaces.get(&construction.source_surface)?;
        let bounds = [
            construction.parameter_bounds[1],
            construction.parameter_bounds[0],
        ];
        if source.object_id() != source_extrusion.directrix.object_id()
            || carrier.direction != source_extrusion.direction
            || *direction != source_extrusion.direction
            || distance.to_bits() != construction.distance.to_bits()
            || carrier.parameter_bounds[0] != bounds[0]
            || parameter_range != &bounds[1]
        {
            return None;
        }
        if resolved.is_some_and(|previous| previous != bounds) {
            return None;
        }
        resolved = Some(bounds);
    }
    resolved
}

fn terminal_span_directrix(
    directrix_id: u32,
    active: [f64; 2],
    controls: [u8; 2],
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Option<B5ExtrusionDirectrix> {
    let mut first_position = 0;
    let target_span_count = wire::compact_uint(&controls[..1], &mut first_position)?;
    let mut second_position = 0;
    let source_span_count =
        usize::try_from(wire::compact_uint(&controls[1..], &mut second_position)?).ok()?;
    (target_span_count == 1 && matches!(source_span_count, 5 | 6)).then_some(())?;
    let pcurve = object_stream_pcurves.get(&directrix_id)?;
    (pcurve.class == 0x20 && pcurve.distinct_knots.len() == source_span_count + 1).then_some(())?;
    let source_range = [
        *pcurve.distinct_knots.get(source_span_count - 1)?,
        *pcurve.distinct_knots.get(source_span_count)?,
    ];
    pcurve
        .parameter_range
        .into_iter()
        .zip([
            *pcurve.distinct_knots.first()?,
            *pcurve.distinct_knots.last()?,
        ])
        .all(|(left, right)| left.to_bits() == right.to_bits())
        .then_some(())?;
    parameter_spans_agree(source_range[1] - source_range[0], active[1] - active[0]).then_some(
        B5ExtrusionDirectrix::SurfaceCurve {
            object_id: directrix_id,
            support: (pcurve.surface, directrix_id, source_range),
            parameter_range: active,
        },
    )
}

fn translated_directrix_span_count(
    directrix: &B5ExtrusionDirectrix,
    active: [f64; 2],
    controls: [u8; 2],
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Option<usize> {
    let mut first_position = 0;
    let target_span_count = wire::compact_uint(&controls[..1], &mut first_position)?;
    let mut second_position = 0;
    let source_span_count =
        usize::try_from(wire::compact_uint(&controls[1..], &mut second_position)?).ok()?;
    (target_span_count == 1 && source_span_count > 1).then_some(())?;
    let [support] = directrix.supports().try_into().ok()?;
    let pcurve = object_stream_pcurves.get(&support.1)?;
    (pcurve.class == 0x21).then_some(())?;
    let suffix_span = pcurve.class_21_suffix_scalar?;
    let active_span = active[1] - active[0];
    parameter_spans_agree(suffix_span, active_span).then_some(())?;
    let source = directrix.parameter_range();
    let start = pcurve
        .distinct_knots
        .iter()
        .position(|knot| knot.to_bits() == source[0].to_bits())?;
    let end = pcurve
        .distinct_knots
        .iter()
        .position(|knot| knot.to_bits() == source[1].to_bits())?;
    (end.checked_sub(start)? == source_span_count).then_some(())?;
    pcurve.distinct_knots[start..=end]
        .windows(2)
        .all(|knots| parameter_spans_agree(knots[1] - knots[0], suffix_span))
        .then_some(source_span_count)
}

fn parameter_range_contains(domain: [f64; 2], active: [f64; 2]) -> bool {
    let scale = domain
        .into_iter()
        .chain(active)
        .map(f64::abs)
        .fold(1.0, f64::max);
    let tolerance = 64.0 * f64::EPSILON * scale;
    domain[0] <= active[0] + tolerance && active[1] <= domain[1] + tolerance
}

fn parameter_spans_agree(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= 64.0 * f64::EPSILON * scale
}

struct B5ExtrusionCarrier {
    directrix_id: u32,
    direction: [f64; 3],
    parameter_bounds: [[f64; 2]; 2],
    controls: [u8; 2],
}

fn extrusion_carrier(record: &B5Record) -> Option<B5ExtrusionCarrier> {
    (record.family == 0xb5 && record.class == 0x2c && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let directrix_id = wire::object_ref(&record.payload, &mut position, true)?;
    let values = line_values::<9>(&record.payload, position)?;
    position += 72;
    let controls: [u8; 2] = record.payload.get(position..)?.try_into().ok()?;
    let direction = [values[0], values[1], values[2]];
    let contextual_offset_chart = matches!(controls, [0x01, 0x09 | 0x15]);
    ((matches!(controls, [0x05, 0x05 | 0x11 | 0x15 | 0x19])
        || contextual_offset_chart
        || (matches!(controls[0], 0x01 | 0x05) && controls[1] == 0x29))
        && direction_is_unit(direction)
        && values[3] < values[4]
        && values[5].to_bits() == 1.0f64.to_bits()
        && values[6].to_bits() == 0.0f64.to_bits()
        && if contextual_offset_chart {
            values[7] > values[8]
        } else {
            values[7] < values[8]
        })
    .then_some(B5ExtrusionCarrier {
        directrix_id,
        direction,
        parameter_bounds: [[values[3], values[4]], [values[7], values[8]]],
        controls,
    })
}

fn parse_extrusion_directrix(
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Option<B5ExtrusionDirectrix> {
    if record.family == 0xb5 && record.class == 0x24 {
        return parse_surface_curve_directrix(record, records, object_stream_pcurves);
    }
    if record.family == 0xb5 && record.class == 0x14 {
        return parse_offset_curve_directrix(record, records, object_stream_pcurves);
    }
    (record.family == 0xa8 && record.class == 0x25 && record.payload.first() == Some(&0x82))
        .then_some(())?;
    let mut position = 1;
    let wrapper_id = wire::object_ref(&record.payload, &mut position, true)?;
    let second_pcurve = wire::object_ref(&record.payload, &mut position, true)?;
    let tail = record.payload.len().checked_sub(25)?;
    (position < tail).then_some(())?;
    let parameter_range = line_values::<2>(&record.payload, tail)?;
    let cache_fit_tolerance = scalar(&record.payload, tail + 16)?;
    if record.payload.get(tail + 24) != Some(&0x01)
        || parameter_range[0] >= parameter_range[1]
        || cache_fit_tolerance <= 0.0
    {
        return None;
    }
    let wrapper = records.get(&wrapper_id)?;
    (wrapper.family == 0xb5 && wrapper.class == 0x24 && wrapper.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut wrapper_position = 1;
    let first_pcurve = wire::object_ref(&wrapper.payload, &mut wrapper_position, true)?;
    if wrapper.payload.get(wrapper_position..wrapper_position + 2) != Some(&[0x81, 0x01]) {
        return None;
    }
    wrapper_position += 2;
    let wrapper_values = line_values::<3>(&wrapper.payload, wrapper_position)?;
    wrapper_position += 24;
    if wrapper.payload.get(wrapper_position..) != Some(&[0x01])
        || wrapper_values[2].to_bits() != 0.0f64.to_bits()
        || wrapper_values[..2]
            .iter()
            .zip(parameter_range)
            .any(|(left, right)| left.to_bits() != right.to_bits())
    {
        return None;
    }
    let first = records.get(&first_pcurve)?;
    let first_surface = pcurve_surface_reference(first)?;
    let first_range = analytic_pcurve_range(first)?;
    let second = object_stream_pcurves.get(&second_pcurve)?;
    Some(B5ExtrusionDirectrix::Intersection {
        object_id: record.object_id,
        supports: [
            (first_surface, first_pcurve, first_range),
            (second.surface, second_pcurve, second.parameter_range),
        ],
        parameter_range,
        cache_fit_tolerance,
    })
}

fn parse_surface_curve_directrix(
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Option<B5ExtrusionDirectrix> {
    (record.family == 0xb5 && record.class == 0x24 && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let pcurve = wire::object_ref(&record.payload, &mut position, true)?;
    if record.payload.get(position..position + 2) != Some(&[0x81, 0x01]) {
        return None;
    }
    position += 2;
    let [start, end, zero] = line_values::<3>(&record.payload, position)?;
    position += 24;
    if record.payload.get(position..) != Some(&[0x01])
        || start >= end
        || zero.to_bits() != 0.0f64.to_bits()
    {
        return None;
    }
    let (surface, pcurve_range) = object_stream_pcurves
        .get(&pcurve)
        .map(|pcurve| (pcurve.surface, pcurve.parameter_range))
        .or_else(|| {
            let pcurve_record = records.get(&pcurve)?;
            Some((
                pcurve_surface_reference(pcurve_record)?,
                analytic_pcurve_range(pcurve_record)?,
            ))
        })?;
    let parameter_range = [start, end];
    parameter_range_contains(pcurve_range, parameter_range).then_some(
        B5ExtrusionDirectrix::SurfaceCurve {
            object_id: record.object_id,
            support: (surface, pcurve, parameter_range),
            parameter_range,
        },
    )
}

fn parse_offset_curve_directrix(
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Option<B5ExtrusionDirectrix> {
    (record.family == 0xb5 && record.class == 0x14 && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let source_id = wire::object_ref(&record.payload, &mut position, true)?;
    let source_parameter_range = line_values::<2>(&record.payload, position)?;
    position += 16;
    if record.payload.get(position) != Some(&0x05) {
        return None;
    }
    position += 1;
    let [distance, x, y, z, start, end] = line_values::<6>(&record.payload, position)?;
    position += 48;
    let direction = [x, y, z];
    let source_record = records.get(&source_id)?;
    if !((source_record.family == 0xb5 && source_record.class == 0x24)
        || (source_record.family == 0xa8 && source_record.class == 0x25))
    {
        return None;
    }
    let source = parse_extrusion_directrix(source_record, records, object_stream_pcurves)?;
    if position != record.payload.len()
        || !distance.is_finite()
        || distance == 0.0
        || !direction_is_unit(direction)
        || source_parameter_range[0] >= source_parameter_range[1]
        || start >= end
        || !source.supports().iter().any(|support| {
            support
                .2
                .into_iter()
                .zip(source_parameter_range)
                .all(|(left, right)| left.to_bits() == right.to_bits())
        })
    {
        return None;
    }
    Some(B5ExtrusionDirectrix::Offset {
        object_id: record.object_id,
        source: Box::new(source),
        source_parameter_range,
        distance,
        direction,
        parameter_range: [start, end],
    })
}

fn pcurve_surface_reference(record: &B5Record) -> Option<u32> {
    (matches!(record.class, 0x18..=0x21)).then_some(())?;
    let mut position = usize::from(record.payload.first() == Some(&0x81));
    wire::object_ref(&record.payload, &mut position, true)
}

fn analytic_pcurve_range(record: &B5Record) -> Option<[f64; 2]> {
    match record.class {
        0x18 => parse_line_pcurve(record).and_then(|pcurve| {
            Some([
                *pcurve.distinct_knots.first()?,
                *pcurve.distinct_knots.last()?,
            ])
        }),
        0x19 => parse_circle_pcurve(record).and_then(|pcurve| {
            Some([
                *pcurve.distinct_knots.first()?,
                *pcurve.distinct_knots.last()?,
            ])
        }),
        _ => None,
    }
}

fn parse_supported_surface(record: &B5Record) -> Option<B5SupportedSurface> {
    (record.family == 0xb5
        && matches!(record.class, 0x37 | 0x3b)
        && record.payload.first() == Some(&0x85))
    .then_some(())?;
    let mut position = 1;
    let references: [u32; 5] = (0..5)
        .map(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    (record.payload.len() == position.checked_add(22)?).then_some(())?;
    let parameters = match record.class {
        0x37 => {
            let controls = [
                record.payload[position],
                record.payload[position + 1],
                record.payload[position + 10],
                record.payload[position + 11],
                record.payload[position + 20],
                record.payload[position + 21],
            ];
            let construction_radius = scalar(&record.payload, position + 2)?;
            let zero = scalar(&record.payload, position + 12)?;
            (construction_radius > 0.0 && zero == 0.0).then_some(())?;
            B5SupportedSurfaceParameters::Radius {
                controls,
                construction_radius,
            }
        }
        0x3b => {
            let controls = record.payload[position..position + 6].try_into().ok()?;
            let scalars = [
                scalar(&record.payload, position + 6)?,
                scalar(&record.payload, position + 14)?,
            ];
            scalars.iter().all(|scalar| *scalar > 0.0).then_some(())?;
            B5SupportedSurfaceParameters::ScalarPair { controls, scalars }
        }
        _ => unreachable!(),
    };
    Some(B5SupportedSurface {
        object_id: record.object_id,
        carrier_surface: references[0],
        support_surfaces: [references[1], references[2]],
        support_pcurves: [references[3], references[4]],
        parameters,
    })
}

fn supported_surface_parameters_match_carrier(
    parameters: &B5SupportedSurfaceParameters,
    carrier: &B5Surface,
) -> bool {
    let relative_close =
        |left: f64, right: f64| (left - right).abs() <= 1e-12 * left.abs().max(right.abs());
    match (parameters, carrier) {
        (
            B5SupportedSurfaceParameters::Radius {
                construction_radius,
                ..
            },
            B5Surface::Cylinder { radius, .. },
        ) => relative_close(*construction_radius, *radius),
        (
            B5SupportedSurfaceParameters::Radius {
                construction_radius,
                ..
            },
            B5Surface::Torus { minor_radius, .. },
        ) => relative_close(*construction_radius, *minor_radius),
        (
            B5SupportedSurfaceParameters::Radius {
                construction_radius,
                ..
            },
            B5Surface::Sphere {
                construction_radius: carrier_radius,
                ..
            },
        ) => relative_close(*construction_radius, *carrier_radius),
        (B5SupportedSurfaceParameters::Radius { .. }, B5Surface::RollingBall { .. }) => true,
        (B5SupportedSurfaceParameters::ScalarPair { .. }, B5Surface::Plane { .. }) => true,
        (
            B5SupportedSurfaceParameters::ScalarPair { scalars, .. },
            B5Surface::Cone { half_angle, .. },
        ) => relative_close(scalars[1], *half_angle),
        _ => false,
    }
}

fn supported_surface_pcurves_match(
    construction: &B5SupportedSurface,
    by_id: &HashMap<u32, &B5Record>,
    a8_pcurve_supports: &HashMap<u32, u32>,
) -> bool {
    construction
        .support_pcurves
        .iter()
        .zip(construction.support_surfaces)
        .all(|(pcurve_id, support_id)| {
            let Some(pcurve) = by_id.get(pcurve_id) else {
                return false;
            };
            if pcurve.family == 0xa8 && pcurve.class == 0x20 {
                return a8_pcurve_supports.get(pcurve_id) == Some(&support_id);
            }
            let mut position = 1;
            pcurve.payload.first() == Some(&0x81)
                && wire::object_ref(&pcurve.payload, &mut position, true) == Some(support_id)
        })
}

fn lift_pcurve_endpoints(
    surface: &B5Surface,
    profiles: &BTreeMap<u32, B5Profile>,
    control_points: &[[f64; 2]],
) -> Option<[[f64; 3]; 2]> {
    let endpoints = [*control_points.first()?, *control_points.last()?];
    let lifted = match surface {
        B5Surface::UnresolvedNurbs { .. }
        | B5Surface::Unknown { .. }
        | B5Surface::RollingBall { .. } => None,
        B5Surface::Plane {
            origin,
            direction_u,
            direction_v,
            ..
        } => Some(
            endpoints
                .map(|[u, v]| add(*origin, add(scale(*direction_u, u), scale(*direction_v, v)))),
        ),
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
            angular_scale,
            ..
        } => {
            let reference_y = cross(*axis, *reference_x);
            Some(endpoints.map(|[u, v]| {
                let angle = u / angular_scale;
                add(
                    *origin,
                    add(
                        scale(
                            add(
                                scale(*reference_x, angle.cos()),
                                scale(reference_y, angle.sin()),
                            ),
                            *radius,
                        ),
                        scale(*axis, v),
                    ),
                )
            }))
        }
        B5Surface::Cone {
            apex,
            direction_x,
            direction_y,
            axis,
            half_angle,
            angular_scale,
            ..
        } => Some(endpoints.map(|[u, v]| {
            let angle = u / angular_scale;
            let radial = add(
                scale(*direction_x, angle.cos()),
                scale(*direction_y, angle.sin()),
            );
            add(
                *apex,
                scale(
                    add(
                        scale(*axis, half_angle.cos()),
                        scale(radial, half_angle.sin()),
                    ),
                    v,
                ),
            )
        })),
        B5Surface::Torus {
            center,
            direction_x,
            direction_y,
            axis,
            major_radius,
            minor_radius,
            major_scale,
            minor_scale,
            ..
        } => Some(endpoints.map(|[u, v]| {
            let major_angle = u / major_scale;
            let minor_angle = v / minor_scale;
            let radial = add(
                scale(*direction_x, major_angle.cos()),
                scale(*direction_y, major_angle.sin()),
            );
            add(
                *center,
                add(
                    scale(radial, major_radius + minor_radius * minor_angle.cos()),
                    scale(*axis, minor_radius * minor_angle.sin()),
                ),
            )
        })),
        B5Surface::Sphere { .. } => None,
        B5Surface::Revolution {
            profile_curve,
            axis_origin,
            axis_direction,
            profile_range,
            angular_scale,
            ..
        } => {
            let profile = profiles.get(profile_curve)?;
            (profile
                .parameter_range()
                .into_iter()
                .zip(*profile_range)
                .all(|(profile, surface)| profile.to_bits() == surface.to_bits()))
            .then_some(())?;
            Some(endpoints.map(|[u, v]| {
                let point = match profile {
                    B5Profile::Line {
                        point, direction, ..
                    } => add(*point, scale(*direction, u)),
                    B5Profile::Arc {
                        center,
                        direction_x,
                        direction_y,
                        radius,
                        ..
                    } => {
                        let angle = u / radius;
                        add(
                            *center,
                            scale(
                                add(
                                    scale(*direction_x, angle.cos()),
                                    scale(*direction_y, angle.sin()),
                                ),
                                *radius,
                            ),
                        )
                    }
                };
                rotate_about_axis(point, *axis_origin, *axis_direction, v / angular_scale)
            }))
        }
        B5Surface::Nurbs(surface) => Some([
            evaluate_nurbs(surface, endpoints[0][0], endpoints[0][1])?,
            evaluate_nurbs(surface, endpoints[1][0], endpoints[1][1])?,
        ]),
    };
    lifted.filter(|points| {
        points
            .iter()
            .flatten()
            .all(|coordinate| coordinate.is_finite())
    })
}

fn evaluate_nurbs(surface: &NurbsSurface, u: f64, v: f64) -> Option<[f64; 3]> {
    let point = nurbs_surface_point(surface, u, v)?;
    Some([point.x, point.y, point.z])
}

fn rotate_about_axis(point: [f64; 3], origin: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    let relative = [
        point[0] - origin[0],
        point[1] - origin[1],
        point[2] - origin[2],
    ];
    let cross_term = cross(axis, relative);
    let dot = axis[0] * relative[0] + axis[1] * relative[1] + axis[2] * relative[2];
    add(
        origin,
        add(
            scale(relative, angle.cos()),
            add(
                scale(cross_term, angle.sin()),
                scale(axis, dot * (1.0 - angle.cos())),
            ),
        ),
    )
}

fn scalar(bytes: &[u8], offset: usize) -> Option<f64> {
    let value = f64_at(bytes, offset)?;
    value.is_finite().then_some(value)
}

fn point(bytes: &[u8], offset: usize) -> Option<[f64; 3]> {
    Some([
        scalar(bytes, offset)?,
        scalar(bytes, offset + 8)?,
        scalar(bytes, offset + 16)?,
    ])
}

// `unit` divides by reciprocal-multiply (`scale(value, 1.0 / length)`), a
// bit-level-distinct normalization from the transfer module's per-component
// division. The two must NOT be unified: the affected profiles depend on the
// exact rounding of each form. See `transfer::unit` for the sibling copy.
fn unit(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = value[0].hypot(value[1]).hypot(value[2]);
    (length.is_finite() && length != 0.0).then(|| scale(value, 1.0 / length))
}

fn parse_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.family != 0xb5 || record.class != 0x21 || record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(&record.payload, &mut position, true)?;
    if record.payload.get(position) != Some(&0x01) {
        return None;
    }
    position += 1;
    let degree = wire::compact_uint(&record.payload, &mut position)?;
    if !matches!(degree, 1 | 2 | 5)
        || record.payload.get(position..position + 2) != Some(&[0x01, 0x01])
    {
        return None;
    }
    position += 2;
    let knot_count = usize::try_from(wire::compact_uint(&record.payload, &mut position)?).ok()?;
    if knot_count != 2 || record.payload.get(position) != Some(&0x01) {
        return None;
    }
    position += 1;
    let mut distinct_knots = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        let value = f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        );
        if !value.is_finite() {
            return None;
        }
        distinct_knots.push(value);
        position += 8;
    }
    if distinct_knots.windows(2).any(|pair| pair[0] >= pair[1]) {
        return None;
    }
    let mut multiplicities = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        multiplicities.push(wire::compact_uint(&record.payload, &mut position)?);
    }
    let endpoint_multiplicity = degree + 1;
    if multiplicities != [endpoint_multiplicity; 2] {
        return None;
    }
    let pole_count = endpoint_multiplicity;
    let mut control_points = Vec::with_capacity(usize::try_from(pole_count).ok()?);
    for _ in 0..pole_count {
        let u = f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        );
        let v = f64::from_le_bytes(
            record
                .payload
                .get(position + 8..position + 16)?
                .try_into()
                .ok()?,
        );
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        control_points.push([u, v]);
        position += 16;
    }
    let tail = record.payload.get(position..)?;
    let suffix_scalar = scalar(tail, 10)?;
    if tail.len() != 36
        || tail.get(..2) != Some(&[0x05, 0x05])
        || scalar(tail, 2)? != 0.0
        || suffix_scalar <= 0.0
        || scalar(tail, 18)? != 1.0
        || scalar(tail, 26)? != 0.0
        || tail.get(34..) != Some(&[0x00, 0x07])
    {
        return None;
    }
    Some(B5Pcurve {
        object_id: record.object_id,
        surface,
        degree,
        distinct_knots,
        multiplicities,
        control_points,
        weights: None,
        parameter_range: None,
        class_21_suffix_scalar: Some(suffix_scalar),
        lifted_endpoints: None,
    })
}

fn parse_circle_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.family != 0xb5 || record.class != 0x19 || record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(&record.payload, &mut position, true)?;
    if record.payload.len() != position.checked_add(58)? {
        return None;
    }
    let center = line_values::<2>(&record.payload, position)?;
    position += 16;
    if record.payload.get(position..position + 2) != Some(&[0x05, 0x05]) {
        return None;
    }
    position += 2;
    let [radius, start, end, orientation, phase] = line_values::<5>(&record.payload, position)?;
    if radius <= 0.0 || start >= end || !matches!(orientation, -1.0 | 1.0) {
        return None;
    }
    let start_angle = phase + orientation * start / radius;
    let end_angle = phase + orientation * end / radius;
    rational_arc_pcurve(
        record,
        surface,
        center,
        [1.0, 0.0],
        [0.0, 1.0],
        radius,
        [start, end],
        [start_angle, end_angle],
    )
}

fn parse_class_1a_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.family != 0xb5 || record.class != 0x1a || record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(&record.payload, &mut position, true)?;
    if record.payload.len() != position.checked_add(74)? {
        return None;
    }
    let center = line_values::<2>(&record.payload, position)?;
    position += 16;
    if record.payload.get(position..position + 2) != Some(&[0x05, 0x05]) {
        return None;
    }
    let [diameter_u, diameter_v, conjugate_angle, start, end, orientation, period] =
        line_values::<7>(&record.payload, position + 2)?;
    let diameter = diameter_u.hypot(diameter_v);
    let relative_period = period / (std::f64::consts::PI * diameter);
    if diameter <= 0.0
        || start >= end
        || period <= 0.0
        || !matches!(orientation, -1.0 | 1.0)
        || (conjugate_angle - std::f64::consts::FRAC_PI_2).abs() > 1e-12
        || !relative_period.is_finite()
        || (relative_period - 1.0).abs() > 1e-12
    {
        return None;
    }
    let reference_x = [diameter_u / diameter, diameter_v / diameter];
    let reference_y = [-reference_x[1], reference_x[0]];
    let angles = [
        orientation * std::f64::consts::TAU * start / period,
        orientation * std::f64::consts::TAU * end / period,
    ];
    rational_arc_pcurve(
        record,
        surface,
        center,
        reference_x,
        reference_y,
        diameter * 0.5,
        [start, end],
        angles,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Decode/encode helper keeps one parameter per independent arena, table, or control flag rather than a catch-all context struct."
)]
fn rational_arc_pcurve(
    record: &B5Record,
    surface: u32,
    center: [f64; 2],
    reference_x: [f64; 2],
    reference_y: [f64; 2],
    radius: f64,
    parameter_range: [f64; 2],
    angle_range: [f64; 2],
) -> Option<B5Pcurve> {
    let [start, end] = parameter_range;
    let [start_angle, end_angle] = angle_range;
    let span_count = ((end_angle - start_angle).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let span_count = (span_count as usize).max(1);
    let control_count = span_count.checked_mul(2)?.checked_add(1)?;
    let mut control_points = Vec::with_capacity(control_count);
    let mut weights = Vec::with_capacity(control_count);
    let mut distinct_knots = vec![start];
    let mut multiplicities = vec![3];
    for span in 0..span_count {
        let fraction0 = span as f64 / span_count as f64;
        let fraction1 = (span + 1) as f64 / span_count as f64;
        let angle0 = start_angle + (end_angle - start_angle) * fraction0;
        let angle1 = start_angle + (end_angle - start_angle) * fraction1;
        let middle = (angle0 + angle1) * 0.5;
        let middle_weight = ((angle1 - angle0) * 0.5).cos();
        if middle_weight <= f64::EPSILON {
            return None;
        }
        if span == 0 {
            control_points.push([
                center[0]
                    + radius * (reference_x[0] * angle0.cos() + reference_y[0] * angle0.sin()),
                center[1]
                    + radius * (reference_x[1] * angle0.cos() + reference_y[1] * angle0.sin()),
            ]);
            weights.push(1.0);
        }
        control_points.push([
            center[0]
                + radius / middle_weight
                    * (reference_x[0] * middle.cos() + reference_y[0] * middle.sin()),
            center[1]
                + radius / middle_weight
                    * (reference_x[1] * middle.cos() + reference_y[1] * middle.sin()),
        ]);
        control_points.push([
            center[0] + radius * (reference_x[0] * angle1.cos() + reference_y[0] * angle1.sin()),
            center[1] + radius * (reference_x[1] * angle1.cos() + reference_y[1] * angle1.sin()),
        ]);
        weights.extend([middle_weight, 1.0]);
        if span + 1 < span_count {
            distinct_knots.push(start + (end - start) * fraction1);
            multiplicities.push(2);
        }
    }
    distinct_knots.push(end);
    multiplicities.push(3);
    if distinct_knots.iter().any(|knot| !knot.is_finite())
        || control_points
            .iter()
            .flatten()
            .any(|coordinate| !coordinate.is_finite())
        || weights.iter().any(|weight| !weight.is_finite())
    {
        return None;
    }
    Some(B5Pcurve {
        object_id: record.object_id,
        surface,
        degree: 2,
        distinct_knots,
        multiplicities,
        control_points,
        weights: Some(weights),
        parameter_range: None,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    })
}

fn parse_opaque_pcurve(record: &B5Record) -> Option<B5OpaquePcurve> {
    if parse_class_1a_pcurve(record).is_some() {
        return None;
    }
    if record.family != 0xb5
        || !matches!(record.class, 0x1a | 0x1d)
        || record.payload.first() != Some(&0x81)
    {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(&record.payload, &mut position, true)?;
    match record.class {
        0x1a => {
            (record.payload.len() == position.checked_add(74)?).then_some(())?;
            line_values::<2>(&record.payload, position)?;
            position += 16;
            (record.payload.get(position..position + 2) == Some(&[0x05, 0x05])).then_some(())?;
            line_values::<7>(&record.payload, position + 2)?;
        }
        0x1d => {
            (record.payload.len() == position.checked_add(99)?).then_some(())?;
            line_values::<4>(&record.payload, position)?;
            position += 32;
            (record.payload.get(position..position + 2) == Some(&[0x05, 0x81])).then_some(())?;
            line_values::<3>(&record.payload, position + 2)?;
            position += 26;
            (record.payload.get(position) == Some(&0x1d)).then_some(())?;
            line_values::<5>(&record.payload, position + 1)?;
        }
        _ => unreachable!(),
    }
    Some(B5OpaquePcurve {
        object_id: record.object_id,
        surface,
        class: record.class,
        payload: record.payload.clone(),
        sphere_great_circle: None,
    })
}

fn parse_sphere_great_circle_pcurve(
    record: &B5Record,
    surface: &B5Surface,
) -> Option<B5SphereGreatCirclePcurve> {
    let B5Surface::Sphere {
        construction_radius: sphere_chart_scale,
        azimuth_range,
        chart_origin,
        ..
    } = surface
    else {
        return None;
    };
    (record.family == 0xb5 && record.class == 0x1d && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    wire::object_ref(&record.payload, &mut position, true)?;
    (record.payload.len() == position.checked_add(99)?).then_some(())?;
    let [u0, u1, v0, v1] = line_values::<4>(&record.payload, position)?;
    position += 32;
    (record.payload.get(position..position + 2) == Some(&[0x05, 0x81])).then_some(())?;
    let [chart_shift, direction, zero0] = line_values::<3>(&record.payload, position + 2)?;
    position += 26;
    (record.payload.get(position) == Some(&0x1d)).then_some(())?;
    let [chart_scale, slope, reciprocal_scale, phase, zero1] =
        line_values::<5>(&record.payload, position + 1)?;

    let surface_u_bounds = azimuth_range.map(|angle| chart_scale * angle);
    let u_scale = surface_u_bounds
        .into_iter()
        .chain([u0, u1])
        .map(f64::abs)
        .fold(1.0, f64::max);
    let u_tolerance = 1e-12 * u_scale;
    (direction.abs() == 1.0
        && zero0 == 0.0
        && zero1 == 0.0
        && chart_scale > 0.0
        && u0 < u1
        && chart_scale == *sphere_chart_scale
        && reciprocal_scale == -direction / chart_scale
        && u0 >= surface_u_bounds[0] - u_tolerance
        && u1 <= surface_u_bounds[1] + u_tolerance
        && v0 == *chart_origin
        && v1 == chart_origin + std::f64::consts::TAU * chart_scale)
        .then_some(B5SphereGreatCirclePcurve {
            chart_bounds: [[u0, u1], [v0, v1]],
            chart_shift,
            chart_scale,
            slope,
            phase,
        })
}

fn sphere_great_circle_point(
    pcurve: &B5SphereGreatCirclePcurve,
    surface: &B5Surface,
    parameter: f64,
) -> Option<[f64; 3]> {
    let B5Surface::Sphere {
        center,
        direction_x,
        direction_y,
        axis,
        radius,
        construction_radius,
        ..
    } = surface
    else {
        return None;
    };
    let [start, end] = pcurve.chart_bounds[0];
    if ![start, end, parameter].into_iter().all(f64::is_finite)
        || start >= end
        || parameter < start
        || parameter > end
        || !pcurve.chart_scale.is_finite()
        || pcurve.chart_scale <= 0.0
        || pcurve.chart_scale != *construction_radius
        || !pcurve.chart_shift.is_finite()
        || !pcurve.slope.is_finite()
        || !pcurve.phase.is_finite()
        || !radius.is_finite()
        || *radius <= 0.0
    {
        return None;
    }
    let azimuth = parameter / pcurve.chart_scale;
    let phase = pcurve.chart_shift / pcurve.chart_scale + pcurve.phase;
    let latitude = (pcurve.slope * (azimuth - phase).cos()).atan();
    let cos_latitude = latitude.cos();
    let sin_latitude = latitude.sin();
    let cos_azimuth = azimuth.cos();
    let sin_azimuth = azimuth.sin();
    let point = [
        center[0]
            + radius
                * (cos_latitude * (cos_azimuth * direction_x[0] + sin_azimuth * direction_y[0])
                    + sin_latitude * axis[0]),
        center[1]
            + radius
                * (cos_latitude * (cos_azimuth * direction_x[1] + sin_azimuth * direction_y[1])
                    + sin_latitude * axis[1]),
        center[2]
            + radius
                * (cos_latitude * (cos_azimuth * direction_x[2] + sin_azimuth * direction_y[2])
                    + sin_latitude * axis[2]),
    ];
    point.into_iter().all(f64::is_finite).then_some(point)
}

fn circle_pcurves_from_frames(bytes: &[u8], frames: &[ObjectFrame]) -> Vec<B5Pcurve> {
    let mut pcurves = Vec::new();
    for frame in frames {
        if frame.family != 0xb5 || frame.class != 0x19 {
            continue;
        }
        let record = B5Record {
            offset: frame.start,
            family: 0xb5,
            class: 0x19,
            object_id: frame.object_id,
            payload: bytes[frame.start + 8..frame.end].to_vec(),
        };
        if let Some(pcurve) = parse_circle_pcurve(&record) {
            pcurves.push(pcurve);
        }
    }
    pcurves
}

fn parse_line_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.family != 0xb5 || record.class != 0x18 || record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(&record.payload, &mut position, true)?;
    let mode = *record.payload.get(position)?;
    position += 1;
    let (start, end, control_points) = match mode {
        0x01 if record.payload.len() == position.checked_add(48)? => {
            let [u, v, du, dv, start, end] = line_values::<6>(&record.payload, position)?;
            if du == 0.0 && dv == 0.0 {
                return None;
            }
            (
                start,
                end,
                vec![
                    [u + start * du, v + start * dv],
                    [u + end * du, v + end * dv],
                ],
            )
        }
        0x05 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = line_values::<3>(&record.payload, position)?;
            (start, end, vec![[constant, start], [constant, end]])
        }
        0x09 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = line_values::<3>(&record.payload, position)?;
            (start, end, vec![[start, constant], [end, constant]])
        }
        _ => return None,
    };
    if start >= end {
        return None;
    }
    if control_points
        .iter()
        .flatten()
        .any(|coordinate| !coordinate.is_finite())
    {
        return None;
    }
    Some(B5Pcurve {
        object_id: record.object_id,
        surface,
        degree: 1,
        distinct_knots: vec![start, end],
        multiplicities: vec![2, 2],
        control_points,
        weights: None,
        parameter_range: None,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    })
}

fn line_values<const N: usize>(payload: &[u8], mut position: usize) -> Option<[f64; N]> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = f64::from_le_bytes(payload.get(position..position + 8)?.try_into().ok()?);
        if !value.is_finite() {
            return None;
        }
        position += 8;
    }
    Some(values)
}

#[cfg(test)]
fn records(bytes: &[u8]) -> Vec<B5Record> {
    let frames = object_stream_frames(bytes);
    records_from_frames(bytes, &frames)
}

pub(crate) fn records_from_frames(bytes: &[u8], frames: &[ObjectFrame]) -> Vec<B5Record> {
    let mut records = framed_records(bytes, frames);
    let existing: HashSet<u32> = records.iter().map(|record| record.object_id).collect();
    let mut pending: HashSet<u32> = records.iter().flat_map(record_references).collect();
    let mut admitted = HashSet::new();
    loop {
        pending.retain(|object_id| !existing.contains(object_id) && !admitted.contains(object_id));
        if pending.is_empty() {
            break;
        }
        let mut candidates = HashMap::<u32, Option<B5Record>>::new();
        for frame in frames {
            if !pending.contains(&frame.object_id)
                || !is_reference_dependency_class(frame.family, frame.class)
            {
                continue;
            }
            let header = if frame.family == 0xa8 { 11 } else { 8 };
            let candidate = B5Record {
                offset: frame.start,
                family: frame.family,
                class: frame.class,
                object_id: frame.object_id,
                payload: bytes[frame.start + header..frame.end].to_vec(),
            };
            candidates
                .entry(frame.object_id)
                .and_modify(|slot| {
                    if slot.as_ref().is_some_and(|existing| {
                        existing.family != candidate.family
                            || existing.class != candidate.class
                            || existing.payload != candidate.payload
                    }) {
                        *slot = None;
                    }
                })
                .or_insert(Some(candidate));
        }
        let mut found = candidates.into_values().flatten().collect::<Vec<_>>();
        if found.is_empty() {
            break;
        }
        found.sort_unstable_by_key(|record| record.offset);
        pending.clear();
        for candidate in found {
            admitted.insert(candidate.object_id);
            pending.extend(record_references(&candidate));
            records.push(candidate);
        }
    }
    records
}

fn framed_records(bytes: &[u8], frames: &[ObjectFrame]) -> Vec<B5Record> {
    let mut records = Vec::new();
    let mut seen = HashMap::<u32, (u8, Vec<u8>)>::new();
    for ObjectFrame {
        start,
        end,
        family,
        class,
        object_id,
    } in frames.iter().copied()
    {
        if !((family == 0xb5 && is_topology_class(class))
            || (family == 0xa8 && matches!(class, 0x34 | 0x62)))
        {
            continue;
        }
        let header = if family == 0xa8 { 11 } else { 8 };
        let Some(payload) = bytes.get(start + header..end).map(ToOwned::to_owned) else {
            continue;
        };
        if seen
            .get(&object_id)
            .is_some_and(|(seen_class, seen_payload)| {
                *seen_class == class && *seen_payload == payload
            })
        {
            continue;
        }
        seen.insert(object_id, (class, payload.clone()));
        records.push((
            end,
            B5Record {
                offset: start,
                family,
                class,
                object_id,
                payload,
            },
        ));
    }
    // Preserve the historical child-before-wrapper order for records nested in
    // an A8 frame while retaining the walker's bounded admission rules.
    records.sort_unstable_by(|(left_end, left), (right_end, right)| {
        left_end
            .cmp(right_end)
            .then_with(|| right.offset.cmp(&left.offset))
    });
    records.into_iter().map(|(_, record)| record).collect()
}

/// Return complete byte ranges for length-closed object-stream records.
#[must_use]
pub(crate) fn framed_ranges(bytes: &[u8]) -> Vec<std::ops::Range<usize>> {
    object_stream_frames(bytes)
        .into_iter()
        .map(|frame| frame.start..frame.end)
        .collect()
}

pub(crate) fn object_stream_frames(bytes: &[u8]) -> Vec<ObjectFrame> {
    fn walk_b5_run(bytes: &[u8], base: usize, frames: &mut Vec<ObjectFrame>) {
        let mut position = 0usize;
        while position + 8 <= bytes.len() {
            let Some((end, family, class, object_id)) = object_frame(bytes, position) else {
                break;
            };
            if family != 0xb5 {
                break;
            }
            frames.push(ObjectFrame {
                start: base + position,
                end: base + end,
                family,
                class,
                object_id,
            });
            position = end;
        }
    }

    fn walk(bytes: &[u8], base: usize, frames: &mut Vec<ObjectFrame>) {
        let mut position = 0usize;
        while position + 8 <= bytes.len() {
            let Some(frame) = object_frame(bytes, position) else {
                position += 1;
                continue;
            };
            let (end, family, class, object_id) = frame;
            let absolute = ObjectFrame {
                start: base + position,
                end: base + end,
                family,
                class,
                object_id,
            };
            match family {
                0xa8 => {
                    frames.push(absolute);
                    if let Some(child_start) =
                        crate::families::a5a8::records::a8_nested_b5_run_start(bytes, position, end)
                    {
                        walk_b5_run(&bytes[child_start..end], base + child_start, frames);
                    }
                    position = end;
                }
                0xb5 => {
                    frames.push(absolute);
                    position = end;
                }
                _ => position += 1,
            }
        }
    }

    let mut frames = Vec::new();
    walk(bytes, 0, &mut frames);
    frames
}

fn record_references(record: &B5Record) -> Vec<u32> {
    let mut position = 0;
    let Some(count) = counted_cardinality(&record.payload, &mut position) else {
        return Vec::new();
    };
    (0..count)
        .map_while(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect()
}

fn topology_surface_references(records: &[B5Record]) -> HashSet<u32> {
    records
        .iter()
        .filter_map(|record| {
            let references = record_references(record);
            match record.class {
                0x5f => references.first().copied(),
                0x62 => references.last().copied(),
                _ => None,
            }
        })
        .collect()
}

fn is_referenced_geometry_class(family: u8, class: u8) -> bool {
    (family == 0xa8 && matches!(class, 0x25 | 0x32 | 0x34))
        || (family == 0xb5 && (matches!(class, 0x18..=0x21) || is_surface_class(class)))
}

fn is_reference_dependency_class(family: u8, class: u8) -> bool {
    is_referenced_geometry_class(family, class)
        || (family == 0xb5 && matches!(class, 0x05 | 0x06 | 0x14 | 0x23..=0x25 | 0x5d))
}

fn is_surface_class(class: u8) -> bool {
    matches!(
        class,
        0x27 | 0x28
            | 0x29
            | 0x2a
            | 0x2b
            | 0x2c
            | 0x2d
            | 0x2e
            | 0x30
            | 0x31
            | 0x34
            | 0x37
            | 0x38
            | 0x3b
    )
}

fn is_opaque_surface_class(class: u8) -> bool {
    matches!(class, 0x2c | 0x2e | 0x30 | 0x37 | 0x38 | 0x3b)
}

fn object_frame(bytes: &[u8], start: usize) -> Option<(usize, u8, u8, u32)> {
    if !matches!(bytes.get(start + 1), Some(0x03 | 0x13 | 0x83)) {
        return None;
    }
    let family = *bytes.get(start)?;
    let class = *bytes.get(start + 2)?;
    let (header, length, object_id) = match family {
        0xb5 => (
            8usize,
            usize::from(*bytes.get(start + 3)?),
            u32::from_le_bytes(bytes.get(start + 4..start + 8)?.try_into().ok()?),
        ),
        0xa8 => (
            11usize,
            usize::try_from(u32::from_le_bytes(
                bytes.get(start + 3..start + 7)?.try_into().ok()?,
            ))
            .ok()?,
            u32::from_le_bytes(bytes.get(start + 7..start + 11)?.try_into().ok()?),
        ),
        _ => return None,
    };
    let end = start.checked_add(header)?.checked_add(length)?;
    (end <= bytes.len()).then_some((end, family, class, object_id))
}

fn is_topology_class(class: u8) -> bool {
    matches!(
        class,
        0x0e | 0x0f | 0x18 | 0x20 | 0x21 | 0x27 | 0x28 | 0x29 | 0x2b | 0x2d | 0x5e | 0x5f | 0x62
    )
}

fn parse_face(
    record: &B5FaceRecord,
    loops: &BTreeMap<u32, B5Loop>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Option<B5Face> {
    let references = &record.references;
    let surface = *references.first()?;
    if !surfaces.contains_key(&surface) {
        return None;
    }
    let loop_ids: Vec<u32> = references[1..]
        .iter()
        .copied()
        .filter(|reference| loops.contains_key(reference))
        .collect();
    if loop_ids.is_empty() || loop_ids.len() + 1 != references.len() {
        return None;
    }
    Some(B5Face {
        object_id: record.object_id,
        surface,
        loops: loop_ids,
        terminal_control: record.terminal_control,
    })
}

fn parse_face_record(record: &B5Record) -> Option<B5FaceRecord> {
    if record.class != 0x5f {
        return None;
    }
    if let Some(count) = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x80))
    {
        (count != 0).then_some(())?;
        let mut position = 1;
        let references = (0..count)
            .map(|_| wire::object_ref(&record.payload, &mut position, true))
            .collect::<Option<Vec<_>>>()?;
        let &[terminal_control] = record.payload.get(position..)? else {
            return None;
        };
        matches!(terminal_control, 0x03 | 0x05).then_some(B5FaceRecord {
            object_id: record.object_id,
            references,
            terminal_control: Some(terminal_control),
        })
    } else {
        let references = uncounted_references(&record.payload)?;
        (!references.is_empty()).then_some(B5FaceRecord {
            object_id: record.object_id,
            references,
            terminal_control: None,
        })
    }
}

/// Read every structurally complete face record independently of target
/// resolution.
#[cfg(test)]
pub(crate) fn typed_face_records(bytes: &[u8]) -> BTreeMap<u32, B5FaceRecord> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_face_records_from_records(&records)
}

pub(crate) fn typed_face_records_from_records(records: &[B5Record]) -> BTreeMap<u32, B5FaceRecord> {
    records
        .iter()
        .filter_map(|record| parse_face_record(record).map(|face| (record.object_id, face)))
        .collect()
}

/// Read every structurally complete loop record independently of target
/// resolution.
#[cfg(test)]
pub(crate) fn typed_loop_records(bytes: &[u8]) -> BTreeMap<u32, B5Loop> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_loop_records_from_records(&records)
}

pub(crate) fn typed_loop_records_from_records(records: &[B5Record]) -> BTreeMap<u32, B5Loop> {
    records
        .iter()
        .filter_map(|record| parse_loop_record(record).map(|loop_| (record.object_id, loop_)))
        .collect()
}

/// Read every structurally complete physical-edge record independently of
/// topology resolution.
#[cfg(test)]
pub(crate) fn typed_edge_records(bytes: &[u8]) -> BTreeMap<u32, B5Edge> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_edge_records_from_records(&records)
}

pub(crate) fn typed_edge_records_from_records(records: &[B5Record]) -> BTreeMap<u32, B5Edge> {
    records
        .iter()
        .filter_map(|record| parse_edge(record).map(|edge| (record.object_id, edge)))
        .collect()
}

/// Read every structurally complete vertex-incidence link independently of
/// topology resolution.
#[cfg(test)]
pub(crate) fn typed_vertex_incidence_links(bytes: &[u8]) -> BTreeMap<u32, B5VertexIncidenceLink> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_vertex_incidence_links_from_records(&records)
}

pub(crate) fn typed_vertex_incidence_links_from_records(
    records: &[B5Record],
) -> BTreeMap<u32, B5VertexIncidenceLink> {
    records
        .iter()
        .filter_map(|record| {
            parse_vertex_incidence_link(record).map(|link| (record.object_id, link))
        })
        .collect()
}

/// Read every structurally complete class-`21` pcurve independently of
/// support and topology resolution.
#[cfg(test)]
pub(crate) fn typed_class_21_pcurves(bytes: &[u8]) -> BTreeMap<u32, B5Pcurve> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_class_21_pcurves_from_records(&records)
}

pub(crate) fn typed_class_21_pcurves_from_records(records: &[B5Record]) -> BTreeMap<u32, B5Pcurve> {
    records
        .iter()
        .filter_map(|record| parse_pcurve(record).map(|pcurve| (record.object_id, pcurve)))
        .collect()
}

/// Read every structurally complete parameter incidence independently of
/// curve, edge, and topology resolution.
#[cfg(test)]
pub(crate) fn typed_parameter_incidences(bytes: &[u8]) -> BTreeMap<u32, B5ParameterIncidence> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_parameter_incidences_from_records(&records)
}

pub(crate) fn typed_parameter_incidences_from_records(
    records: &[B5Record],
) -> BTreeMap<u32, B5ParameterIncidence> {
    records
        .iter()
        .filter_map(|record| {
            parameter_incidence(record).map(|incidence| (record.object_id, incidence))
        })
        .collect()
}

/// Read every structurally complete vertex-incidence roster independently of
/// member and topology resolution.
#[cfg(test)]
pub(crate) fn typed_vertex_incidence_rosters(bytes: &[u8]) -> BTreeMap<u32, Vec<u32>> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_vertex_incidence_rosters_from_records(&records)
}

pub(crate) fn typed_vertex_incidence_rosters_from_records(
    records: &[B5Record],
) -> BTreeMap<u32, Vec<u32>> {
    records
        .iter()
        .filter_map(|record| {
            counted_references(record, 0x05).map(|members| (record.object_id, members))
        })
        .collect()
}

/// Read each face's leading surface reference independently of its loop grammar.
#[cfg(test)]
pub(crate) fn face_surface_references(bytes: &[u8]) -> Vec<(u32, u32)> {
    let frames = object_stream_frames(bytes);
    face_surface_references_from_frames(bytes, &frames)
}

pub(crate) fn face_surface_references_from_frames(
    bytes: &[u8],
    frames: &[ObjectFrame],
) -> Vec<(u32, u32)> {
    let mut references = Vec::new();
    for frame in frames {
        if frame.family != 0xb5 || frame.class != 0x5f {
            continue;
        }
        let payload = &bytes[frame.start + 8..frame.end];
        let Some(&lead) = payload.first() else {
            continue;
        };
        let mut position = usize::from(lead >= 0x80);
        if position == 1 && lead == 0x80 {
            continue;
        }
        let Some(surface) = wire::object_ref(payload, &mut position, true) else {
            continue;
        };
        references.push((frame.object_id, surface));
    }
    references
}

fn parse_loop(
    record: &B5Loop,
    by_id: &HashMap<u32, &B5Record>,
    parsed_pcurves: &BTreeMap<u32, B5Pcurve>,
    opaque_pcurves: &BTreeMap<u32, B5OpaquePcurve>,
    implicit_pcurves: &BTreeMap<u32, u32>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Option<B5Loop> {
    let surface = record.surface;
    if !surfaces.contains_key(&surface) {
        return None;
    }
    for (&pcurve, &edge) in record.pcurves.iter().zip(&record.edges) {
        if (parsed_pcurves
            .get(&pcurve)
            .is_none_or(|pcurve| pcurve.surface != surface)
            && opaque_pcurves
                .get(&pcurve)
                .is_none_or(|pcurve| pcurve.surface != surface)
            && implicit_pcurves.get(&pcurve) != Some(&surface))
            || by_id.get(&edge)?.class != 0x5e
        {
            return None;
        }
    }
    Some(record.clone())
}

fn parse_loop_record(record: &B5Record) -> Option<B5Loop> {
    let (references, metadata) = loop_references_and_metadata(record)?;
    let surface = *references.last()?;
    let mut pcurves = Vec::with_capacity((references.len() - 1) / 2);
    let mut edges = Vec::with_capacity((references.len() - 1) / 2);
    for pair in references[..references.len() - 1].chunks_exact(2) {
        pcurves.push(pair[0]);
        edges.push(pair[1]);
    }
    Some(B5Loop {
        object_id: record.object_id,
        pcurves,
        edges,
        metadata,
        surface,
    })
}

fn loop_references(record: &B5Record) -> Option<Vec<u32>> {
    loop_references_and_metadata(record).map(|(references, _)| references)
}

fn loop_references_and_metadata(record: &B5Record) -> Option<(Vec<u32>, B5LoopMetadata)> {
    (record.class == 0x62).then_some(())?;
    let mut position = 0;
    let count = counted_cardinality(&record.payload, &mut position)?;
    if count < 3 || count % 2 == 0 {
        return None;
    }
    let references = (0..count)
        .map(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?;
    let edge_count = (count - 1) / 2;
    if counted_cardinality(&record.payload, &mut position)? != edge_count {
        return None;
    }
    let metadata = loop_metadata(record.payload.get(position..)?, edge_count)?;
    Some((references, metadata))
}

fn loop_metadata(bytes: &[u8], edge_count: usize) -> Option<B5LoopMetadata> {
    let controls_len = edge_count.checked_mul(3)?.checked_mul(2)?;
    let controls_end = 3usize.checked_add(controls_len)?;
    if !matches!(bytes.first(), Some(0x03 | 0x05))
        || !matches!(bytes.get(1..3), Some([0x03 | 0x05, 0x03]))
        || controls_end > bytes.len()
    {
        return None;
    }
    let edge_controls = bytes[3..controls_end]
        .chunks_exact(6)
        .map(|controls| {
            let controls = [
                i16::from_le_bytes(controls[0..2].try_into().ok()?),
                i16::from_le_bytes(controls[2..4].try_into().ok()?),
                i16::from_le_bytes(controls[4..6].try_into().ok()?),
            ];
            controls
                .iter()
                .all(|control| matches!(control, -1 | 1))
                .then_some(controls)
        })
        .collect::<Option<Vec<_>>>()?;
    let extension = match bytes.get(controls_end..)? {
        [0x01] => None,
        extended
            if extended.len() == 62
                && extended[0] == 0x0d
                && extended.get(33..35) == Some(&[0x05, 0x05])
                && extended[35] & 1 == 1
                && extended.get(36..38) == Some(&[0x05, 0x01]) =>
        {
            let scalars: [f64; 4] = std::array::from_fn(|index| {
                f64::from_le_bytes(
                    extended[1 + index * 8..9 + index * 8]
                        .try_into()
                        .expect("checked extended loop scalar extent"),
                )
            });
            let floats: [f32; 6] = std::array::from_fn(|index| {
                f32::from_le_bytes(
                    extended[38 + index * 4..42 + index * 4]
                        .try_into()
                        .expect("checked extended loop float extent"),
                )
            });
            if scalars.iter().any(|value| !value.is_finite())
                || floats.iter().any(|value| !value.is_finite())
            {
                return None;
            }
            Some(B5LoopMetadataExtension {
                scalars,
                control: extended[35],
                floats,
            })
        }
        _ => return None,
    };
    Some(B5LoopMetadata {
        framing_controls: [bytes[0], bytes[1]],
        edge_controls,
        extension,
    })
}

fn counted_cardinality(bytes: &[u8], position: &mut usize) -> Option<usize> {
    let lead = *bytes.get(*position)?;
    if lead >= 0x80 {
        *position += 1;
        Some(usize::from(lead - 0x80))
    } else {
        usize::try_from(wire::object_ref(bytes, position, true)?).ok()
    }
}

fn uncounted_references(bytes: &[u8]) -> Option<Vec<u32>> {
    let mut position = 0;
    let mut references = Vec::new();
    while position < bytes.len() {
        references.push(wire::object_ref(bytes, &mut position, true)?);
    }
    Some(references)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_preserves_tiny_finite_direction() {
        assert_eq!(unit([1e-200, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
        assert_eq!(unit([0.0, 0.0, 0.0]), None);
    }

    fn test_pcurve(object_id: u32, surface: u32) -> B5Pcurve {
        B5Pcurve {
            object_id,
            surface,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 0.0], [1.0, 0.0]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        }
    }

    fn object_stream_pcurve(
        surface: u32,
        distinct_knots: Vec<f64>,
        suffix: Option<f64>,
    ) -> B5ObjectStreamPcurve {
        B5ObjectStreamPcurve {
            class: 0x21,
            surface,
            parameter_range: [
                *distinct_knots.first().expect("test knot"),
                *distinct_knots.last().expect("test knot"),
            ],
            class_21_suffix_scalar: suffix,
            distinct_knots,
        }
    }

    fn test_loop_metadata(edge_count: usize) -> B5LoopMetadata {
        B5LoopMetadata {
            framing_controls: [0x05, 0x05],
            edge_controls: vec![[1, 1, 1]; edge_count],
            extension: None,
        }
    }

    fn extended_loop_metadata(metadata_control: u8) -> Vec<u8> {
        let mut bytes = vec![0x03, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00];
        bytes.push(0x0d);
        for value in [1.0_f64, -2.0, 3.5, 4.25] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0x05, 0x05, metadata_control, 0x05, 0x01]);
        for value in [1.0_f32, -2.0, 3.5, 4.25, 5.5, -6.75] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn loop_metadata_accepts_exact_base_and_extended_forms() {
        let base = [
            0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff,
            0xff, 0x01,
        ];
        assert_eq!(
            loop_metadata(&base, 2),
            Some(B5LoopMetadata {
                framing_controls: [0x05, 0x05],
                edge_controls: vec![[1, -1, 1], [-1, 1, -1]],
                extension: None,
            })
        );

        for metadata_control in [0x05, 0x09, 0x21, 0x41, 0x71] {
            let extended = extended_loop_metadata(metadata_control);
            let metadata = loop_metadata(&extended, 1).expect("complete extended metadata");
            assert_eq!(metadata.framing_controls, [0x03, 0x05]);
            assert_eq!(metadata.edge_controls, [[1, -1, 1]]);
            assert_eq!(
                metadata.extension,
                Some(B5LoopMetadataExtension {
                    scalars: [1.0, -2.0, 3.5, 4.25],
                    control: metadata_control,
                    floats: [1.0, -2.0, 3.5, 4.25, 5.5, -6.75],
                })
            );
        }

        let alternate_framing_control = [
            0x05, 0x03, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff,
            0xff, 0x01,
        ];
        let metadata =
            loop_metadata(&alternate_framing_control, 2).expect("alternate framing control");
        assert_eq!(metadata.framing_controls, [0x05, 0x03]);
        assert_eq!(metadata.edge_controls, [[1, -1, 1], [-1, 1, -1]]);
        assert_eq!(metadata.extension, None);
    }

    #[test]
    fn loop_references_require_exact_matching_edge_count_and_metadata() {
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x62,
            object_id: 400,
            payload: vec![
                0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
                0x01,
            ],
        };
        let (references, metadata) =
            loop_references_and_metadata(&record).expect("exact loop payload");
        assert_eq!(references, [9, 10, 11]);
        assert_eq!(metadata.edge_controls, [[1, -1, 1]]);

        let mut mismatched = record.clone();
        mismatched.payload[4] = 0x82;
        assert!(loop_references(&mismatched).is_none());

        let mut residual = record;
        residual.payload.push(0);
        assert!(loop_references(&residual).is_none());
    }

    #[test]
    fn loop_metadata_rejects_every_malformed_boundary_and_numeric_domain() {
        assert!(loop_metadata(&[], 0).is_none());
        assert!(loop_metadata(&[0x05, 0x05, 0x03], 0).is_none());
        assert!(loop_metadata(&[0x05, 0x05, 0x03, 0x01, 0x00, 0x01], 0).is_none());
        assert!(loop_metadata(&[0x09, 0x05, 0x03, 0x01], 0).is_none());
        assert!(loop_metadata(&[0x05, 0x03, 0x05, 0x01], 0).is_none());
        assert!(loop_metadata(
            &[0x05, 0x05, 0x03, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01],
            1
        )
        .is_none());

        for metadata_control in [0x00, 0x04, 0x20, 0x70] {
            assert!(loop_metadata(&extended_loop_metadata(metadata_control), 1).is_none());
        }

        let mut non_finite_scalar = extended_loop_metadata(0x05);
        non_finite_scalar[10..18].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(loop_metadata(&non_finite_scalar, 1).is_none());

        let mut non_finite_float = extended_loop_metadata(0x05);
        non_finite_float[47..51].copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert!(loop_metadata(&non_finite_float, 1).is_none());
    }

    #[test]
    fn pcurve_candidate_merge_collapses_repeats_and_permanently_rejects_conflicts() {
        let mut pcurves = BTreeMap::new();
        let mut conflicts = HashSet::new();
        merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 10));
        merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 10));
        assert_eq!(pcurves.get(&1), Some(&test_pcurve(1, 10)));

        merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 11));
        merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 10));
        assert!(!pcurves.contains_key(&1));
        assert!(conflicts.contains(&1));
    }

    #[test]
    fn loop_rejects_a_pcurve_bound_to_another_surface() {
        let loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![2],
            edges: vec![3],
            metadata: test_loop_metadata(1),
            surface: 10,
        };
        let edge = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x5e,
            object_id: 3,
            payload: Vec::new(),
        };
        let records = HashMap::from([(3, &edge)]);
        let pcurves = BTreeMap::from([(2, test_pcurve(2, 11))]);
        let surfaces = BTreeMap::from([(
            10,
            B5Surface::Unknown {
                family: 0xb5,
                class: 0x27,
                payload: Vec::new(),
            },
        )]);

        assert!(parse_loop(
            &loop_,
            &records,
            &pcurves,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &surfaces,
        )
        .is_none());
    }

    #[test]
    fn pcurve_requires_one_complete_clamped_bezier_frame() {
        let payload = crate::tests::b5_linear_pcurve_payload(1, [0.0, 0.0], [1.0, 0.0]);
        let record = |payload| B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x21,
            object_id: 2,
            payload,
        };
        assert_eq!(
            parse_pcurve(&record(payload.clone()))
                .expect("complete class-21 pcurve")
                .class_21_suffix_scalar,
            Some(1.0)
        );
        let tail = payload.len() - 36;
        let mut alternate_scalar = payload.clone();
        alternate_scalar[tail + 10..tail + 18].copy_from_slice(&2.5_f64.to_le_bytes());
        assert_eq!(
            parse_pcurve(&record(alternate_scalar))
                .expect("alternate positive suffix scalar")
                .class_21_suffix_scalar,
            Some(2.5)
        );
        let mut wrong_family = record(payload.clone());
        wrong_family.family = 0xa8;
        assert!(parse_pcurve(&wrong_family).is_none());

        for (offset, value) in [(6, 0), (7, 0), (8, 0x0d), (9, 0x08), (26, 0x0d)] {
            let mut malformed = payload.clone();
            malformed[offset] = value;
            assert!(parse_pcurve(&record(malformed)).is_none());
        }

        let mut truncated = payload.clone();
        truncated.pop();
        assert!(parse_pcurve(&record(truncated)).is_none());

        let mut residual = payload.clone();
        residual.push(0);
        assert!(parse_pcurve(&record(residual)).is_none());

        for (offset, value) in [
            (tail + 2, 1.0_f64),
            (tail + 10, 0.0),
            (tail + 18, 0.0),
            (tail + 26, 1.0),
        ] {
            let mut malformed = payload.clone();
            malformed[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            assert!(parse_pcurve(&record(malformed)).is_none());
        }

        let mut non_finite = payload;
        non_finite[tail + 10..tail + 18].copy_from_slice(&f64::INFINITY.to_le_bytes());
        assert!(parse_pcurve(&record(non_finite)).is_none());
    }

    #[test]
    fn surface_candidate_merge_refines_opaque_wrappers_and_rejects_exact_conflicts() {
        let unknown = B5Surface::Unknown {
            family: 0xb5,
            class: 0x2e,
            payload: vec![0x81, 0x82],
        };
        let plane = B5Surface::Plane {
            origin: [0.0; 3],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let cylinder = B5Surface::Cylinder {
            origin: [0.0; 3],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 1.0,
            u_range: [0.0, std::f64::consts::TAU],
            v_range: [-1.0, 1.0],
            angular_scale: 1.0,
            chart_origin: 0.0,
        };
        let mut surfaces = BTreeMap::from([(1, unknown)]);
        let mut conflicts = HashSet::new();
        assert!(merge_surface_candidate(
            &mut surfaces,
            &mut conflicts,
            1,
            plane.clone(),
        ));
        assert_eq!(surfaces.get(&1), Some(&plane));

        assert!(!merge_surface_candidate(
            &mut surfaces,
            &mut conflicts,
            1,
            cylinder,
        ));
        assert!(!merge_surface_candidate(
            &mut surfaces,
            &mut conflicts,
            1,
            plane,
        ));
        assert!(!surfaces.contains_key(&1));
        assert!(conflicts.contains(&1));
    }

    #[test]
    fn full_surface_alias_closure_is_order_independent_unbounded_and_cycle_safe() {
        let alias = |object_id: u32, target: u32| B5Record {
            offset: usize::try_from(object_id).expect("small object id"),
            family: 0xb5,
            class: 0x2e,
            object_id,
            payload: vec![0x81, 0x80 + u8::try_from(target).expect("compact target")],
        };
        let mut records = (1..30)
            .rev()
            .map(|object_id| alias(object_id, object_id + 1))
            .collect::<Vec<_>>();
        let cycle_start = records.len();
        records.push(alias(40, 41));
        records.push(alias(41, 40));
        let by_id = records
            .iter()
            .map(|record| (record.object_id, record))
            .collect::<HashMap<_, _>>();
        let plane = B5Surface::Plane {
            origin: [0.0; 3],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let mut surfaces = BTreeMap::from([(30, plane.clone())]);
        let mut conflicts = HashSet::new();
        assert!(resolve_surface_aliases(
            &records,
            &by_id,
            &mut surfaces,
            &mut conflicts,
        ));
        assert_eq!(surfaces.get(&1), Some(&plane));
        assert_eq!(surfaces.get(&29), Some(&plane));
        assert_eq!(
            surface_alias_carrier(records[cycle_start].object_id, &by_id, &surfaces),
            None
        );
        assert!(!surfaces.contains_key(&40));
        assert!(!surfaces.contains_key(&41));
    }

    #[test]
    fn canonical_surface_identity_follows_unbounded_aliases_and_rejects_cycles() {
        let aliases = (1..30)
            .map(|object_id| (object_id, object_id + 1))
            .chain([(40, 41), (41, 40)])
            .collect();

        assert_eq!(canonical_surface_id(&aliases, 1), Some(30));
        assert_eq!(canonical_surface_id(&aliases, 29), Some(30));
        assert_eq!(canonical_surface_id(&aliases, 30), Some(30));
        assert_eq!(canonical_surface_id(&aliases, 40), None);
        assert_eq!(canonical_surface_id(&aliases, 41), None);
    }

    #[test]
    fn surface_alias_closes_after_its_terminal_construction_resolves() {
        let records = vec![B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2e,
            object_id: 1,
            payload: vec![0x81, 0x82],
        }];
        let by_id = HashMap::from([(1, &records[0])]);
        let mut surfaces = BTreeMap::new();
        let mut conflicts = HashSet::new();
        assert!(!resolve_surface_aliases(
            &records,
            &by_id,
            &mut surfaces,
            &mut conflicts,
        ));

        let plane = B5Surface::Plane {
            origin: [0.0; 3],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        surfaces.insert(2, plane.clone());
        assert!(resolve_surface_aliases(
            &records,
            &by_id,
            &mut surfaces,
            &mut conflicts,
        ));
        assert_eq!(surfaces.get(&1), Some(&plane));
    }

    #[test]
    fn targeted_surface_resolution_follows_a_supported_surface_to_a_rolling_ball_carrier() {
        let mut payload = vec![0x85];
        for reference in 1u16..=5 {
            payload.push(0x18);
            payload.extend_from_slice(&reference.to_le_bytes());
        }
        payload.extend_from_slice(&[0x01, 0x02]);
        payload.extend_from_slice(&2.0f64.to_le_bytes());
        payload.extend_from_slice(&[0x03, 0x04]);
        payload.extend_from_slice(&0.0f64.to_le_bytes());
        payload.extend_from_slice(&[0x05, 0x06]);
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x37,
            object_id: 10,
            payload,
        };
        let records = HashMap::from([(10, Some(record))]);
        let rolling = HashMap::from([(
            1,
            Some(B5Surface::RollingBall {
                carrier_object_id: 1,
                definition: ProceduralSurfaceDefinition::Unknown { record: None },
            }),
        )]);
        assert_eq!(
            resolve_targeted_surface(10, &records, &HashMap::new(), &HashMap::new(), &rolling),
            rolling.get(&1).cloned().flatten()
        );
    }

    #[test]
    fn targeted_surface_resolution_validates_an_analytic_offset_carrier() {
        let carrier = B5Surface::Plane {
            origin: [0.0; 3],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let source = B5Surface::Plane {
            origin: [0.0, 0.0, 0.5],
            direction_u: [0.0, 1.0, 0.0],
            direction_v: [-1.0, 0.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let mut payload = vec![0x82, 0x82, 0x83];
        payload.extend_from_slice(&(-0.5f64).to_le_bytes());
        payload.push(0x15);
        for value in [-2.0f64, 3.0, -4.0, 5.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let offset = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x30,
            object_id: 9,
            payload,
        };
        let records = HashMap::from([(9, Some(offset.clone()))]);
        let resolved = HashMap::from([(2, Some(carrier.clone())), (3, Some(source))]);

        assert_eq!(
            resolve_targeted_surface(9, &records, &HashMap::new(), &resolved, &HashMap::new(),),
            Some(carrier)
        );

        let mut wrong_distance = offset.clone();
        wrong_distance.payload[3..11].copy_from_slice(&(-0.25f64).to_le_bytes());
        assert!(resolve_targeted_surface(
            9,
            &HashMap::from([(9, Some(wrong_distance))]),
            &HashMap::new(),
            &resolved,
            &HashMap::new(),
        )
        .is_none());
        let mut wrong_kind = offset;
        wrong_kind.payload[11] = 0x05;
        assert!(resolve_targeted_surface(
            9,
            &HashMap::from([(9, Some(wrong_kind))]),
            &HashMap::new(),
            &resolved,
            &HashMap::new(),
        )
        .is_none());
    }

    #[test]
    fn targeted_surface_resolution_has_no_alias_depth_limit() {
        let records = (1u32..=20)
            .map(|object_id| {
                (
                    object_id,
                    Some(B5Record {
                        offset: usize::try_from(object_id).expect("small object id"),
                        family: 0xb5,
                        class: 0x2e,
                        object_id,
                        payload: vec![
                            0x81,
                            0x80 + u8::try_from(object_id + 1).expect("compact target"),
                        ],
                    }),
                )
            })
            .collect();
        let rolling = HashMap::from([(
            21,
            Some(B5Surface::RollingBall {
                carrier_object_id: 21,
                definition: ProceduralSurfaceDefinition::Unknown { record: None },
            }),
        )]);
        assert_eq!(
            resolve_targeted_surface(1, &records, &HashMap::new(), &HashMap::new(), &rolling),
            rolling.get(&21).cloned().flatten()
        );
    }

    #[test]
    fn targeted_surface_resolution_rejects_alias_cycles() {
        let alias = |object_id, target| {
            Some(B5Record {
                offset: usize::try_from(object_id).expect("small object id"),
                family: 0xb5,
                class: 0x2e,
                object_id,
                payload: vec![0x81, 0x80 + u8::try_from(target).expect("compact target")],
            })
        };
        let records = HashMap::from([(1, alias(1, 2)), (2, alias(2, 1))]);
        assert!(resolve_targeted_surface(
            1,
            &records,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
        .is_none());
    }

    #[test]
    fn targeted_surface_resolution_rejects_conflicting_exact_carriers() {
        let rolling = HashMap::from([(
            1,
            Some(B5Surface::RollingBall {
                carrier_object_id: 1,
                definition: ProceduralSurfaceDefinition::Unknown { record: None },
            }),
        )]);
        let resolved = HashMap::from([(
            1,
            Some(B5Surface::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 1,
                u_count: 2,
                v_count: 2,
                u_knots: vec![0.0, 0.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 4],
                weights: None,
                u_periodic: false,
                v_periodic: false,
            })),
        )]);
        assert!(
            resolve_targeted_surface(1, &HashMap::new(), &HashMap::new(), &resolved, &rolling,)
                .is_none()
        );
    }

    #[test]
    fn requested_edge_support_scan_closes_through_its_unique_wrapper() {
        let mut bytes = Vec::new();
        let append = |bytes: &mut Vec<u8>, class, object_id: u32, payload: &[u8]| {
            bytes.extend_from_slice(&[0xb5, 0x03, class, payload.len() as u8]);
            bytes.extend_from_slice(&object_id.to_le_bytes());
            bytes.extend_from_slice(payload);
        };
        append(
            &mut bytes,
            0x23,
            20,
            &[0x82, 0x18, 30, 0, 0x18, 31, 0, 0x01],
        );
        append(
            &mut bytes,
            0x5e,
            40,
            &[
                0x85, 0x18, 20, 0, 0x18, 1, 0, 0x18, 2, 0, 0x18, 3, 0, 0x18, 4, 0, 0x22,
            ],
        );
        assert_eq!(
            edge_support_pcurve_references(&bytes, &HashSet::from([40])),
            BTreeMap::from([(40, [30, 31])])
        );
        assert!(edge_support_pcurve_references(&bytes, &HashSet::from([41])).is_empty());
    }

    #[test]
    fn face_surface_references_do_not_require_resolved_loops() {
        let mut bytes = Vec::new();
        for (object_id, surface_id) in [(500u32, 100u8), (501, 100), (500, 101)] {
            bytes.extend_from_slice(&[0xb5, 0x03, 0x5f, 5]);
            bytes.extend_from_slice(&object_id.to_le_bytes());
            bytes.extend_from_slice(&[0x82, 0x08, surface_id, 0x00, 0x05]);
        }
        assert_eq!(
            face_surface_references(&bytes),
            vec![(500, 100), (501, 100), (500, 101)]
        );
    }

    #[test]
    fn counted_face_references_accept_both_exact_terminal_controls() {
        for terminal_control in [0x03, 0x05] {
            let record = B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x5f,
                object_id: 3,
                payload: vec![0x82, 0x81, 0x82, terminal_control],
            };
            assert_eq!(
                parse_face_record(&record),
                Some(B5FaceRecord {
                    object_id: 3,
                    references: vec![1, 2],
                    terminal_control: Some(terminal_control),
                })
            );

            let mut overlong = record;
            overlong.payload.push(terminal_control);
            assert_eq!(parse_face_record(&overlong), None);
        }
    }

    #[test]
    fn counted_face_references_reject_unknown_terminal_controls() {
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x5f,
            object_id: 3,
            payload: vec![0x82, 0x81, 0x82, 0x04],
        };
        assert_eq!(parse_face_record(&record), None);

        let mut empty = record;
        empty.payload.clear();
        assert_eq!(parse_face_record(&empty), None);
        empty.payload.extend_from_slice(&[0x80, 0x03]);
        assert_eq!(parse_face_record(&empty), None);
    }

    #[test]
    fn one_edge_loop_closes_on_one_native_vertex() {
        let loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![2],
            edges: vec![3],
            metadata: test_loop_metadata(1),
            surface: 4,
        };

        assert!(loop_chain_closes(&loop_, &BTreeMap::from([(3, [0, 0])])));
        assert!(!loop_chain_closes(&loop_, &BTreeMap::from([(3, [0, 1])])));
    }

    #[test]
    fn loop_chain_requires_each_source_native_edge_sense() {
        let mut loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![4, 5, 6],
            edges: vec![1, 2, 3],
            metadata: test_loop_metadata(3),
            surface: 7,
        };
        loop_.metadata.edge_controls[1][0] = -1;
        let edge_vertices = BTreeMap::from([(1, [0, 1]), (2, [2, 1]), (3, [2, 0])]);
        assert!(loop_chain_closes(&loop_, &edge_vertices));

        loop_.metadata.edge_controls[1][0] = 1;
        assert!(!loop_chain_closes(&loop_, &edge_vertices));
    }

    #[test]
    fn opaque_pcurve_occurrences_defer_endpoint_binding_to_native_edges() {
        let loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![2],
            edges: vec![3],
            metadata: test_loop_metadata(1),
            surface: 4,
        };
        let pcurves = BTreeMap::new();
        let opaque_pcurves = BTreeMap::new();
        let surfaces = BTreeMap::new();
        let profiles = BTreeMap::new();
        let edge_parameter_incidences = BTreeMap::new();
        let parameter_incidences = BTreeMap::new();
        let geometry = B5PcurveContext {
            pcurves: &pcurves,
            opaque_pcurves: &opaque_pcurves,
            surfaces: &surfaces,
            profiles: &profiles,
            edge_parameter_incidences: &edge_parameter_incidences,
            parameter_incidences: &parameter_incidences,
        };
        assert_eq!(
            bind_edge_vertices(&BTreeMap::from([(1, loop_)]), &geometry, &[],),
            BTreeMap::new()
        );
    }

    #[test]
    fn sphere_great_circle_pcurve_binds_endpoint_rows() {
        let chart_scale = 8.0;
        let parameter_end = chart_scale * std::f64::consts::FRAC_PI_2;
        let pcurve = B5OpaquePcurve {
            object_id: 2,
            surface: 4,
            class: 0x1d,
            payload: Vec::new(),
            sphere_great_circle: Some(B5SphereGreatCirclePcurve {
                chart_bounds: [
                    [0.0, parameter_end],
                    [0.0, chart_scale * std::f64::consts::TAU],
                ],
                chart_shift: 0.0,
                chart_scale,
                slope: 0.0,
                phase: 0.0,
            }),
        };
        let loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![2],
            edges: vec![3],
            metadata: test_loop_metadata(1),
            surface: 4,
        };
        let surface = B5Surface::Sphere {
            center: [0.0, 0.0, 0.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 5.0,
            azimuth_range: [0.0, std::f64::consts::TAU],
            latitude_range: [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
            construction_radius: chart_scale,
            chart_origin: 0.0,
        };
        let opaque_pcurves = BTreeMap::from([(2, pcurve.clone())]);
        let surfaces = BTreeMap::from([(4, surface)]);
        let pcurves = BTreeMap::new();
        let profiles = BTreeMap::new();
        let edge_parameter_incidences = BTreeMap::new();
        let parameter_incidences = BTreeMap::new();
        let geometry = B5PcurveContext {
            pcurves: &pcurves,
            opaque_pcurves: &opaque_pcurves,
            surfaces: &surfaces,
            profiles: &profiles,
            edge_parameter_incidences: &edge_parameter_incidences,
            parameter_incidences: &parameter_incidences,
        };
        let endpoints =
            pcurve_endpoints(2, 3, &geometry).expect("validated sphere pcurve endpoints");
        assert!(distance_squared(endpoints[0], [5.0, 0.0, 0.0]) < 1e-24);
        assert!(distance_squared(endpoints[1], [0.0, 5.0, 0.0]) < 1e-24);

        assert_eq!(
            bind_edge_vertices(
                &BTreeMap::from([(1, loop_.clone())]),
                &geometry,
                &[[5.0, 0.0, 0.0], [0.0, 5.0, 0.0]],
            ),
            BTreeMap::from([(3, [0, 1])])
        );

        let trimmed_start = parameter_end * 0.25;
        let trimmed_end = parameter_end * 0.75;
        let edge_parameter_incidences = BTreeMap::from([(3, [20, 21])]);
        let parameter_incidences = BTreeMap::from([
            (
                20,
                B5ParameterIncidence {
                    object_id: 20,
                    curves: vec![2],
                    parameters: vec![trimmed_start],
                    controls: vec![1],
                },
            ),
            (
                21,
                B5ParameterIncidence {
                    object_id: 21,
                    curves: vec![2],
                    parameters: vec![trimmed_end],
                    controls: vec![1],
                },
            ),
        ]);
        let trimmed_geometry = B5PcurveContext {
            pcurves: &pcurves,
            opaque_pcurves: &opaque_pcurves,
            surfaces: &surfaces,
            profiles: &profiles,
            edge_parameter_incidences: &edge_parameter_incidences,
            parameter_incidences: &parameter_incidences,
        };
        let trimmed_points = [
            sphere_great_circle_point(
                opaque_pcurves[&2]
                    .sphere_great_circle
                    .as_ref()
                    .expect("great circle"),
                &surfaces[&4],
                trimmed_start,
            )
            .expect("trimmed start"),
            sphere_great_circle_point(
                opaque_pcurves[&2]
                    .sphere_great_circle
                    .as_ref()
                    .expect("great circle"),
                &surfaces[&4],
                trimmed_end,
            )
            .expect("trimmed end"),
        ];
        assert_eq!(
            pcurve_endpoints(2, 3, &trimmed_geometry),
            Some(trimmed_points)
        );
        assert_eq!(
            bind_edge_vertices(
                &BTreeMap::from([(1, loop_)]),
                &trimmed_geometry,
                &trimmed_points,
            ),
            BTreeMap::from([(3, [0, 1])])
        );
    }

    #[test]
    fn finite_large_lifted_endpoints_bind_native_vertices() {
        let endpoints = [[1.0e8, 2.0e8, 3.0e8], [1.0e8 + 5.0, 2.0e8, 3.0e8]];
        let pcurves = BTreeMap::from([(
            2,
            B5Pcurve {
                object_id: 2,
                surface: 4,
                degree: 1,
                distinct_knots: vec![0.0, 1.0],
                multiplicities: vec![2, 2],
                control_points: vec![[0.0, 0.0], [1.0, 0.0]],
                weights: None,
                parameter_range: None,
                class_21_suffix_scalar: None,
                lifted_endpoints: Some(endpoints),
            },
        )]);
        let opaque_pcurves = BTreeMap::new();
        let surfaces = BTreeMap::new();
        let profiles = BTreeMap::new();
        let edge_parameter_incidences = BTreeMap::new();
        let parameter_incidences = BTreeMap::new();
        let geometry = B5PcurveContext {
            pcurves: &pcurves,
            opaque_pcurves: &opaque_pcurves,
            surfaces: &surfaces,
            profiles: &profiles,
            edge_parameter_incidences: &edge_parameter_incidences,
            parameter_incidences: &parameter_incidences,
        };
        let loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![2],
            edges: vec![3],
            metadata: test_loop_metadata(1),
            surface: 4,
        };

        let bound = bind_native_vertices(
            &BTreeMap::from([(1, loop_.clone())]),
            &geometry,
            &BTreeMap::from([(3, [10, 11])]),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &[],
        );

        assert_eq!(bound.edges, BTreeMap::from([(3, [0, 1])]));
        assert_eq!(bound.refs, vec![10, 11]);
        assert_eq!(bound.points, endpoints);
        assert!(bound.tolerances.is_empty());

        let mismatched = bind_native_vertices(
            &BTreeMap::from([(1, loop_)]),
            &geometry,
            &BTreeMap::from([(3, [10, 11])]),
            &BTreeMap::new(),
            &BTreeMap::from([(10, endpoints[1])]),
            &[],
        );
        assert!(mismatched.edges.is_empty());
    }

    #[test]
    fn canonical_point_uses_the_on_carrier_tolerance() {
        let points = [[0.0, 0.0, 0.0]];
        let index = point_index(&points);
        assert_eq!(canonical_point(&points, &index, [1e-3, 0.0, 0.0]), Some(0));
        assert_eq!(
            canonical_point(&points, &index, [1.0001e-3, 0.0, 0.0]),
            None
        );
    }

    #[test]
    fn edge_parameter_incidences_select_typed_pcurve_endpoint_loci() {
        let pcurves = BTreeMap::from([(
            2,
            B5Pcurve {
                object_id: 2,
                surface: 4,
                degree: 1,
                distinct_knots: vec![0.0, 1.0],
                multiplicities: vec![2, 2],
                control_points: vec![[0.0, 0.0], [10.0, 0.0]],
                weights: None,
                parameter_range: None,
                class_21_suffix_scalar: None,
                lifted_endpoints: Some([[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]),
            },
        )]);
        let surfaces = BTreeMap::from([(
            4,
            B5Surface::Plane {
                origin: [0.0, 0.0, 0.0],
                direction_u: [1.0, 0.0, 0.0],
                direction_v: [0.0, 1.0, 0.0],
                u_range: [0.0, 10.0],
                v_range: [-1.0, 1.0],
            },
        )]);
        let edge_parameter_incidences = BTreeMap::from([(3, [20, 21])]);
        let parameter_incidences = BTreeMap::from([
            (
                20,
                B5ParameterIncidence {
                    object_id: 20,
                    curves: vec![2],
                    parameters: vec![0.25],
                    controls: vec![1],
                },
            ),
            (
                21,
                B5ParameterIncidence {
                    object_id: 21,
                    curves: vec![2],
                    parameters: vec![0.75],
                    controls: vec![1],
                },
            ),
        ]);
        let opaque_pcurves = BTreeMap::new();
        let profiles = BTreeMap::new();
        let geometry = B5PcurveContext {
            pcurves: &pcurves,
            opaque_pcurves: &opaque_pcurves,
            surfaces: &surfaces,
            profiles: &profiles,
            edge_parameter_incidences: &edge_parameter_incidences,
            parameter_incidences: &parameter_incidences,
        };
        let loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![2],
            edges: vec![3],
            metadata: test_loop_metadata(1),
            surface: 4,
        };

        assert_eq!(
            pcurve_endpoints(2, 3, &geometry),
            Some([[2.5, 0.0, 0.0], [7.5, 0.0, 0.0]])
        );
        assert_eq!(
            bind_edge_vertices(
                &BTreeMap::from([(1, loop_)]),
                &geometry,
                &[[2.5, 0.0, 0.0], [7.5, 0.0, 0.0]],
            ),
            BTreeMap::from([(3, [0, 1])])
        );
    }

    #[test]
    fn sphere_great_circle_pcurve_binds_native_incidence_coordinates() {
        let chart_scale = 8.0;
        let parameter = chart_scale * std::f64::consts::FRAC_PI_2;
        let opaque_pcurves = BTreeMap::from([(
            2,
            B5OpaquePcurve {
                object_id: 2,
                surface: 4,
                class: 0x1d,
                payload: Vec::new(),
                sphere_great_circle: Some(B5SphereGreatCirclePcurve {
                    chart_bounds: [[0.0, parameter], [0.0, chart_scale * std::f64::consts::TAU]],
                    chart_shift: 0.0,
                    chart_scale,
                    slope: 0.0,
                    phase: 0.0,
                }),
            },
        )]);
        let surfaces = BTreeMap::from([(
            4,
            B5Surface::Sphere {
                center: [0.0, 0.0, 0.0],
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                radius: 5.0,
                azimuth_range: [0.0, std::f64::consts::TAU],
                latitude_range: [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
                construction_radius: chart_scale,
                chart_origin: 0.0,
            },
        )]);
        let mut incidence_payload = vec![0x81, 0x82, 0x81];
        incidence_payload.extend_from_slice(&parameter.to_le_bytes());
        incidence_payload.push(0x01);
        let mut records = [
            B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x05,
                object_id: 20,
                payload: vec![0x82, 0x9e, 0x9f],
            },
            B5Record {
                offset: 1,
                family: 0xb5,
                class: 0x06,
                object_id: 30,
                payload: incidence_payload.clone(),
            },
            B5Record {
                offset: 2,
                family: 0xb5,
                class: 0x06,
                object_id: 31,
                payload: incidence_payload,
            },
        ];
        let by_id = records
            .iter()
            .map(|record| (record.object_id, record))
            .collect::<HashMap<_, _>>();
        let pcurves = BTreeMap::new();
        let profiles = BTreeMap::new();
        let edge_parameter_incidences = BTreeMap::new();
        let parameter_incidences = BTreeMap::new();
        let geometry = B5PcurveContext {
            pcurves: &pcurves,
            opaque_pcurves: &opaque_pcurves,
            surfaces: &surfaces,
            profiles: &profiles,
            edge_parameter_incidences: &edge_parameter_incidences,
            parameter_incidences: &parameter_incidences,
        };
        assert_eq!(counted_references(&records[0], 0x05), Some(vec![30, 31]));
        let incidence = parameter_incidence(&records[1]).expect("parameter incidence");
        assert_eq!(incidence.curves, [2]);
        assert_eq!(incidence.parameters, [parameter]);
        assert!(
            distance_squared(
                sphere_great_circle_point(
                    opaque_pcurves[&2]
                        .sphere_great_circle
                        .as_ref()
                        .expect("great circle"),
                    &surfaces[&4],
                    parameter,
                )
                .expect("sphere endpoint"),
                [0.0, 5.0, 0.0]
            ) < 1e-24
        );
        let coordinates = incidence_vertex_coordinates(
            &BTreeMap::from([(40, [10, 11])]),
            &BTreeMap::from([(
                10,
                B5VertexIncidenceLink {
                    object_id: 10,
                    incidence: 20,
                    terminal_control: 0x00,
                },
            )]),
            &by_id,
            &geometry,
        );

        assert_eq!(coordinates.len(), 1);
        assert!(
            distance_squared(
                *coordinates.get(&10).expect("native vertex coordinate"),
                [0.0, 5.0, 0.0]
            ) < 1e-24
        );

        drop(by_id);
        records[2].payload[3..11].copy_from_slice(&0.0f64.to_le_bytes());
        let conflicting_by_id = records
            .iter()
            .map(|record| (record.object_id, record))
            .collect::<HashMap<_, _>>();
        let conflicting = incidence_vertex_coordinates(
            &BTreeMap::from([(40, [10, 11])]),
            &BTreeMap::from([(
                10,
                B5VertexIncidenceLink {
                    object_id: 10,
                    incidence: 20,
                    terminal_control: 0x00,
                },
            )]),
            &conflicting_by_id,
            &geometry,
        );
        assert!(conflicting.is_empty());

        drop(conflicting_by_id);
        records[2].payload[3..11].copy_from_slice(&(parameter + 1.0).to_le_bytes());
        let out_of_domain_by_id = records
            .iter()
            .map(|record| (record.object_id, record))
            .collect::<HashMap<_, _>>();
        let out_of_domain = incidence_vertex_coordinates(
            &BTreeMap::from([(40, [10, 11])]),
            &BTreeMap::from([(
                10,
                B5VertexIncidenceLink {
                    object_id: 10,
                    incidence: 20,
                    terminal_control: 0x00,
                },
            )]),
            &out_of_domain_by_id,
            &geometry,
        );
        assert!(out_of_domain.is_empty());
    }

    #[test]
    fn conflicting_geometric_endpoints_defer_one_edge_to_native_identity() {
        let loops = BTreeMap::from([
            (
                1,
                B5Loop {
                    object_id: 1,
                    pcurves: vec![10],
                    edges: vec![20],
                    metadata: test_loop_metadata(1),
                    surface: 30,
                },
            ),
            (
                2,
                B5Loop {
                    object_id: 2,
                    pcurves: vec![11],
                    edges: vec![20],
                    metadata: test_loop_metadata(1),
                    surface: 31,
                },
            ),
            (
                3,
                B5Loop {
                    object_id: 3,
                    pcurves: vec![12],
                    edges: vec![21],
                    metadata: test_loop_metadata(1),
                    surface: 32,
                },
            ),
        ]);
        let pcurve = |object_id, endpoints| B5Pcurve {
            object_id,
            surface: object_id + 20,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 0.0], [1.0, 0.0]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: Some(endpoints),
        };
        let pcurves = BTreeMap::from([
            (10, pcurve(10, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]])),
            (11, pcurve(11, [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]])),
            (12, pcurve(12, [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]])),
        ]);
        let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let opaque_pcurves = BTreeMap::new();
        let surfaces = BTreeMap::new();
        let profiles = BTreeMap::new();
        let edge_parameter_incidences = BTreeMap::new();
        let parameter_incidences = BTreeMap::new();
        let geometry = B5PcurveContext {
            pcurves: &pcurves,
            opaque_pcurves: &opaque_pcurves,
            surfaces: &surfaces,
            profiles: &profiles,
            edge_parameter_incidences: &edge_parameter_incidences,
            parameter_incidences: &parameter_incidences,
        };

        assert_eq!(
            bind_edge_vertices(&loops, &geometry, &points),
            BTreeMap::from([(21, [0, 2])])
        );
    }

    #[test]
    fn circle_pcurve_rejects_unbounded_subdivision_counts() {
        let mut payload = vec![0x81, 0x81];
        payload.extend_from_slice(&[0; 16]);
        payload.extend_from_slice(&[0x05, 0x05]);
        for value in [1.0e-300, 0.0, 1.0, 1.0, 0.0] {
            payload.extend_from_slice(&f64::to_le_bytes(value));
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x19,
            object_id: 0,
            payload,
        };
        assert!(parse_circle_pcurve(&record).is_none());
    }

    #[test]
    fn sparse_reference_tokens_fill_selected_id_bytes() {
        let mut position = 0;
        assert_eq!(
            wire::object_ref(&[0x28, 0x34, 0x02], &mut position, true),
            Some(0x02_0034)
        );
        assert_eq!(position, 3);
        position = 0;
        assert_eq!(
            wire::object_ref(&[0x20, 0x07], &mut position, true),
            Some(0x07_0000)
        );
        assert_eq!(position, 2);
        position = 0;
        assert_eq!(wire::object_ref(&[0x8b], &mut position, true), Some(11));
        assert_eq!(position, 1);
    }

    #[test]
    fn counted_cardinality_widens_with_reference_tokens() {
        let mut position = 0;
        assert_eq!(counted_cardinality(&[0x81], &mut position), Some(1));
        assert_eq!(position, 1);
        position = 0;
        assert_eq!(counted_cardinality(&[0x08, 0x81], &mut position), Some(129));
        assert_eq!(position, 2);
        position = 0;
        assert_eq!(
            counted_cardinality(&[0x18, 0x35, 0x01], &mut position),
            Some(309)
        );
        assert_eq!(position, 3);
    }

    #[test]
    fn revolution_surface_requires_complete_sparse_reference_chart() {
        let mut payload = vec![0; 175];
        payload[0] = 0x81;
        payload[1..4].copy_from_slice(&[0x30, 0x86, 0x16]);
        payload[4..12].copy_from_slice(&1.0f64.to_le_bytes());
        payload[28..36].copy_from_slice(&1.0f64.to_le_bytes());
        payload[60..68].copy_from_slice(&1.0f64.to_le_bytes());
        payload[92..100].copy_from_slice(&1.0f64.to_le_bytes());
        for (offset, value) in [
            (100, 0.0f64),
            (108, 2.0 * std::f64::consts::PI),
            (116, -1.0),
            (124, 1.0),
            (134, 2.0),
            (142, 1.0),
            (150, 1.0),
            (158, 0.0),
            (167, 2.0 * std::f64::consts::PI),
        ] {
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        payload[132..134].copy_from_slice(&[0x05, 0x05]);
        payload[166] = 0x01;
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2d,
            object_id: 0x16_8601,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Revolution {
                profile_curve: 0x16_8600,
                axis_origin: [1.0, 0.0, 0.0],
                reference_x: [1.0, 0.0, 0.0],
                reference_y: [0.0, 1.0, 0.0],
                axis_direction: [0.0, 0.0, 1.0],
                profile_range: [-1.0, 1.0],
                angular_range: [0.0, 2.0 * std::f64::consts::PI],
                angular_scale: 2.0,
            })
        );
        let mut left_handed = record.clone();
        left_handed.payload[92..100].copy_from_slice(&(-1.0f64).to_le_bytes());
        assert_eq!(parse_surface(&left_handed), None);
        let mut wrong_lead = record.clone();
        wrong_lead.payload[0] = 0x80;
        assert_eq!(parse_surface(&wrong_lead), None);
        let mut wrong_half_period = record;
        wrong_half_period.payload[167..175].copy_from_slice(&1.0f64.to_le_bytes());
        assert_eq!(parse_surface(&wrong_half_period), None);
    }

    #[test]
    fn line_profile_requires_its_complete_unit_metric_chart() {
        let mut payload = vec![0; 73];
        payload[0] = 0x80;
        for (offset, values) in [(1, [1.0f64, 2.0, 3.0]), (25, [0.0, 0.0, 1.0])] {
            for (index, value) in values.into_iter().enumerate() {
                payload[offset + 8 * index..offset + 8 * index + 8]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        payload[49..57].copy_from_slice(&1.0f64.to_le_bytes());
        payload[57..65].copy_from_slice(&(-2.0f64).to_le_bytes());
        payload[65..73].copy_from_slice(&4.0f64.to_le_bytes());
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x0e,
            object_id: 7,
            payload,
        };
        assert_eq!(
            parse_profile(&record),
            Some(B5Profile::Line {
                point: [1.0, 2.0, 3.0],
                direction: [0.0, 0.0, 1.0],
                parameter_range: [-2.0, 4.0],
            })
        );

        let mut nonunit = record.clone();
        nonunit.payload[41..49].copy_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(parse_profile(&nonunit), None);
        let mut wrong_metric = record.clone();
        wrong_metric.payload[49..57].copy_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(parse_profile(&wrong_metric), None);
        let mut unordered = record.clone();
        unordered.payload[65..73].copy_from_slice(&(-3.0f64).to_le_bytes());
        assert_eq!(parse_profile(&unordered), None);
        let mut wrong_lead = record;
        wrong_lead.payload[0] = 0x81;
        assert_eq!(parse_profile(&wrong_lead), None);
    }

    #[test]
    fn arc_profile_requires_its_complete_centered_periodic_chart() {
        let mut payload = vec![0; 113];
        payload[0] = 0x80;
        for (offset, values) in [
            (1, [1.0f64, 2.0, 3.0]),
            (25, [1.0, 0.0, 0.0]),
            (49, [0.0, 1.0, 0.0]),
        ] {
            for (index, value) in values.into_iter().enumerate() {
                payload[offset + 8 * index..offset + 8 * index + 8]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        let radius = 2.0f64;
        let parameter_range = [0.5 * radius, 1.5 * radius];
        let chart_origin =
            (parameter_range[0] + parameter_range[1]) * 0.5 - std::f64::consts::PI * radius;
        payload[73..81].copy_from_slice(&radius.to_le_bytes());
        payload[81..89].copy_from_slice(&parameter_range[0].to_le_bytes());
        payload[89..97].copy_from_slice(&parameter_range[1].to_le_bytes());
        payload[97..105].copy_from_slice(&1.0f64.to_le_bytes());
        payload[105..113].copy_from_slice(&chart_origin.to_le_bytes());
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x0f,
            object_id: 8,
            payload,
        };
        assert_eq!(
            parse_profile(&record),
            Some(B5Profile::Arc {
                center: [1.0, 2.0, 3.0],
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                radius,
                parameter_range,
            })
        );

        let mut nonorthogonal = record.clone();
        nonorthogonal.payload[49..57].copy_from_slice(&1.0f64.to_le_bytes());
        assert_eq!(parse_profile(&nonorthogonal), None);
        let mut overlong = record.clone();
        overlong.payload[89..97].copy_from_slice(
            &(parameter_range[0] + std::f64::consts::TAU * radius + 1.0).to_le_bytes(),
        );
        assert_eq!(parse_profile(&overlong), None);
        let mut wrong_fixed = record.clone();
        wrong_fixed.payload[97..105].copy_from_slice(&0.0f64.to_le_bytes());
        assert_eq!(parse_profile(&wrong_fixed), None);
        let mut wrong_origin = record;
        wrong_origin.payload[105..113].copy_from_slice(&0.0f64.to_le_bytes());
        assert_eq!(parse_profile(&wrong_origin), None);
    }

    #[test]
    fn cone_surface_reads_the_native_slant_chart() {
        let mut payload = vec![0; 185];
        payload[0] = 0x80;
        for (offset, values) in [
            (1, [1.0f64, 2.0, 3.0]),
            (25, [1.0, 0.0, 0.0]),
            (49, [0.0, 1.0, 0.0]),
            (73, [0.0, 0.0, 1.0]),
        ] {
            for (index, value) in values.into_iter().enumerate() {
                payload[offset + 8 * index..offset + 8 * index + 8]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        for (offset, value) in [
            (97, 0.25f64),
            (105, 4.0),
            (113, 0.5),
            (121, 0.5 + std::f64::consts::PI),
            (129, 0.0),
            (137, 8.0),
            (145, 3.0),
            (153, 1.0),
            (169, 0.5 - std::f64::consts::FRAC_PI_2),
            (177, 0.5 + 3.0 * std::f64::consts::FRAC_PI_2),
        ] {
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x29,
            object_id: 7,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Cone {
                apex: [1.0, 2.0, 3.0],
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                half_angle: 0.25,
                pre_angular_range_scalar: 4.0,
                angular_range: [0.5, 0.5 + std::f64::consts::PI],
                slant_range: [0.0, 8.0],
                angular_scale: 3.0,
                angular_domain: [
                    0.5 - std::f64::consts::FRAC_PI_2,
                    0.5 + 3.0 * std::f64::consts::FRAC_PI_2,
                ],
            })
        );

        let mut opposite_handed = record.clone();
        opposite_handed.payload[89..97].copy_from_slice(&(-1.0f64).to_le_bytes());
        assert!(parse_surface(&opposite_handed).is_some());

        let mut malformed = record.clone();
        malformed.payload[169..177].copy_from_slice(&0.0f64.to_le_bytes());
        assert_eq!(parse_surface(&malformed), None);
        let mut degenerate = record.clone();
        degenerate.payload[97..105].copy_from_slice(&0.0_f64.to_le_bytes());
        assert_eq!(parse_surface(&degenerate), None);
        let mut nonunit = record.clone();
        nonunit.payload[25..33].copy_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(parse_surface(&nonunit), None);
        let mut nonorthogonal = record.clone();
        nonorthogonal.payload[49..57].copy_from_slice(&1.0f64.to_le_bytes());
        assert_eq!(parse_surface(&nonorthogonal), None);
        let mut invalid_axis = record.clone();
        invalid_axis.payload[73..81].copy_from_slice(&1.0f64.to_le_bytes());
        assert_eq!(parse_surface(&invalid_axis), None);
        let mut wrong_lead = record;
        wrong_lead.payload[0] = 0x81;
        assert_eq!(parse_surface(&wrong_lead), None);
    }

    #[test]
    fn plane_surface_requires_complete_unit_chart_frame() {
        let mut payload = vec![0; 121];
        payload[0] = 0x80;
        for (offset, value) in [
            (1usize, 1.0f64),
            (9, 2.0),
            (17, 3.0),
            (25, 1.0),
            (57, 1.0),
            (73, 1.0),
            (81, 1.0),
            (89, -4.0),
            (97, 8.0),
            (105, -2.0),
            (113, 6.0),
        ] {
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x27,
            object_id: 7,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Plane {
                origin: [1.0, 2.0, 3.0],
                direction_u: [1.0, 0.0, 0.0],
                direction_v: [0.0, 1.0, 0.0],
                u_range: [-4.0, 8.0],
                v_range: [-2.0, 6.0],
            })
        );
        let mut wrong_family = record.clone();
        wrong_family.family = 0xa8;
        assert_eq!(parse_surface(&wrong_family), None);

        let mut nonunit = record.clone();
        nonunit.payload[25..33].copy_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(parse_surface(&nonunit), None);
        let mut reversed_range = record;
        reversed_range.payload[97..105].copy_from_slice(&(-5.0f64).to_le_bytes());
        assert_eq!(parse_surface(&reversed_range), None);
    }

    #[test]
    fn cylinder_surface_retains_independent_angular_gauge_and_domain() {
        let radius = 6.0;
        let angular_factor = 2.0;
        let angular_scale = radius / angular_factor;
        let chart_origin = 1.0;
        let mut payload = vec![0; 137];
        payload[0] = 0x80;
        for (offset, value) in [
            (1usize, 1.0f64),
            (9, 2.0),
            (17, 3.0),
            (25, 1.0),
            (57, 1.0),
            (73, radius),
            (81, 2.0),
            (89, 10.0),
            (97, -4.0),
            (105, 5.0),
            (113, angular_factor),
            (121, 1.0),
            (129, chart_origin),
        ] {
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x28,
            object_id: 7,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Cylinder {
                origin: [1.0, 2.0, 3.0],
                reference_x: [1.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                radius,
                u_range: [2.0, 10.0],
                v_range: [-4.0, 5.0],
                angular_scale,
                chart_origin,
            })
        );

        let mut outside_domain = record.clone();
        outside_domain.payload[89..97].copy_from_slice(
            &(chart_origin + std::f64::consts::TAU * angular_scale + 1.0).to_le_bytes(),
        );
        assert_eq!(parse_surface(&outside_domain), None);

        let mut overflowing_domain = record.clone();
        overflowing_domain.payload[73..81].copy_from_slice(&f64::MAX.to_le_bytes());
        overflowing_domain.payload[113..121].copy_from_slice(&1.0f64.to_le_bytes());
        assert_eq!(parse_surface(&overflowing_domain), None);

        let mut wrong_fixed_scalar = record;
        wrong_fixed_scalar.payload[121..129].copy_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(parse_surface(&wrong_fixed_scalar), None);
    }

    #[test]
    fn sphere_surface_validates_radius_scaled_frame_and_chart() {
        let construction_radius = 3.0;
        let azimuth_range = [0.0, 1.0];
        let chart_origin = construction_radius
            * ((azimuth_range[0] + azimuth_range[1]) * 0.5 - std::f64::consts::PI);
        let mut payload = vec![0; 153];
        payload[0] = 0x80;
        for (offset, value) in [
            (1usize, 1.0f64),
            (9, 2.0),
            (17, 3.0),
            (25, 2.0),
            (57, 2.0),
            (89, 2.0),
            (97, 2.0),
            (105, 0.0),
            (113, 1.0),
            (121, -1.0),
            (129, 1.0),
            (137, construction_radius),
            (145, chart_origin),
        ] {
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2a,
            object_id: 7,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Sphere {
                center: [1.0, 2.0, 3.0],
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                radius: 2.0,
                azimuth_range,
                latitude_range: [-1.0, 1.0],
                construction_radius,
                chart_origin,
            })
        );
        let tiny_radius = 1e-200_f64;
        let mut tiny = record.clone();
        for (offset, value) in [
            (25usize, tiny_radius),
            (57, tiny_radius),
            (89, tiny_radius),
            (97, tiny_radius),
        ] {
            tiny.payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        assert!(matches!(
            parse_surface(&tiny),
            Some(B5Surface::Sphere {
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                radius,
                ..
            }) if radius == tiny_radius
        ));
        tiny.payload[25..33].copy_from_slice(&(2.0 * tiny_radius).to_le_bytes());
        assert_eq!(parse_surface(&tiny), None);

        let tiny_construction_radius = 1e-200_f64;
        let tiny_chart_origin = tiny_construction_radius
            * ((azimuth_range[0] + azimuth_range[1]) * 0.5 - std::f64::consts::PI);
        let mut tiny_chart = record.clone();
        tiny_chart.payload[137..145].copy_from_slice(&tiny_construction_radius.to_le_bytes());
        tiny_chart.payload[145..153].copy_from_slice(&tiny_chart_origin.to_le_bytes());
        assert!(parse_surface(&tiny_chart).is_some());
        tiny_chart.payload[145..153]
            .copy_from_slice(&(tiny_chart_origin + f64::EPSILON).to_le_bytes());
        assert!(parse_surface(&tiny_chart).is_some());
        tiny_chart.payload[145..153].copy_from_slice(&1e-12_f64.to_le_bytes());
        assert_eq!(parse_surface(&tiny_chart), None);

        let mut left_handed = record.clone();
        left_handed.payload[89..97].copy_from_slice(&(-2.0f64).to_le_bytes());
        assert_eq!(parse_surface(&left_handed), None);

        let mut overlong_azimuth = record.clone();
        overlong_azimuth.payload[113..121]
            .copy_from_slice(&(std::f64::consts::TAU + 1.0).to_le_bytes());
        assert_eq!(parse_surface(&overlong_azimuth), None);

        let mut invalid_latitude = record.clone();
        invalid_latitude.payload[121..129].copy_from_slice(&(-std::f64::consts::PI).to_le_bytes());
        assert_eq!(parse_surface(&invalid_latitude), None);

        let mut wrong_chart_origin = record;
        wrong_chart_origin.payload[145..153].copy_from_slice(&0.0f64.to_le_bytes());
        assert_eq!(parse_surface(&wrong_chart_origin), None);
    }

    #[test]
    fn torus_surface_separates_geometric_radii_from_chart_scales() {
        let mut payload = vec![0; 201];
        payload[0] = 0x80;
        for (offset, values) in [
            (1, [1.0f64, 2.0, 3.0]),
            (25, [1.0, 0.0, 0.0]),
            (49, [0.0, 1.0, 0.0]),
            (73, [0.0, 0.0, 1.0]),
        ] {
            for (index, value) in values.into_iter().enumerate() {
                payload[offset + 8 * index..offset + 8 * index + 8]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        payload[97..105].copy_from_slice(&5.0f64.to_le_bytes());
        payload[105..113].copy_from_slice(&2.0f64.to_le_bytes());
        for (offset, value) in [
            (113, 0.0),
            (121, std::f64::consts::TAU),
            (129, 0.0),
            (137, std::f64::consts::TAU),
            (145, 0.0),
            (153, std::f64::consts::PI),
            (161, -std::f64::consts::FRAC_PI_2),
            (169, 3.0 * std::f64::consts::FRAC_PI_2),
        ] {
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        payload[177..185].copy_from_slice(&4.0f64.to_le_bytes());
        payload[185..193].copy_from_slice(&3.0f64.to_le_bytes());
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2b,
            object_id: 7,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Torus {
                center: [1.0, 2.0, 3.0],
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                major_radius: 5.0,
                minor_radius: 2.0,
                major_angular_range: [0.0, std::f64::consts::TAU],
                major_angular_domain: [0.0, std::f64::consts::TAU],
                minor_angular_range: [0.0, std::f64::consts::PI],
                minor_angular_domain: [
                    -std::f64::consts::FRAC_PI_2,
                    3.0 * std::f64::consts::FRAC_PI_2,
                ],
                major_scale: 4.0,
                minor_scale: 3.0,
            })
        );

        let mut malformed = record.clone();
        malformed.payload[129..137].copy_from_slice(&0.5f64.to_le_bytes());
        assert_eq!(parse_surface(&malformed), None);
        let mut left_handed = record.clone();
        left_handed.payload[89..97].copy_from_slice(&(-1.0f64).to_le_bytes());
        assert_eq!(parse_surface(&left_handed), None);
        let mut wrong_lead = record.clone();
        wrong_lead.payload[0] = 0x81;
        assert_eq!(parse_surface(&wrong_lead), None);
        let mut nonzero_tail = record;
        nonzero_tail.payload[200] = 1;
        assert_eq!(parse_surface(&nonzero_tail), None);
    }

    #[test]
    fn line_pcurve_decodes_every_complete_mode() {
        let record = |mode: u8, values: &[f64]| {
            let mut payload = vec![0x81, 0x82, mode];
            for value in values {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x18,
                object_id: 7,
                payload,
            }
        };

        let general = parse_line_pcurve(&record(0x01, &[2.0, 3.0, 4.0, -2.0, 1.0, 5.0]))
            .expect("general line pcurve");
        assert_eq!(general.surface, 2);
        assert_eq!(general.distinct_knots, [1.0, 5.0]);
        assert_eq!(general.control_points, [[6.0, 1.0], [22.0, -7.0]]);
        let tiny = parse_line_pcurve(&record(0x01, &[0.0, 0.0, 1e-200, -1e-200, 1.0, 5.0]))
            .expect("tiny nonzero line direction");
        assert_eq!(tiny.control_points, [[1e-200, -1e-200], [5e-200, -5e-200]]);
        let mut wrong_family = record(0x01, &[2.0, 3.0, 4.0, -2.0, 1.0, 5.0]);
        wrong_family.family = 0xa8;
        assert!(parse_line_pcurve(&wrong_family).is_none());

        let constant_u =
            parse_line_pcurve(&record(0x05, &[3.0, -2.0, 7.0])).expect("constant-U pcurve");
        assert_eq!(constant_u.distinct_knots, [-2.0, 7.0]);
        assert_eq!(constant_u.control_points, [[3.0, -2.0], [3.0, 7.0]]);

        let constant_v =
            parse_line_pcurve(&record(0x09, &[3.0, -2.0, 7.0])).expect("constant-V pcurve");
        assert_eq!(constant_v.distinct_knots, [-2.0, 7.0]);
        assert_eq!(constant_v.control_points, [[-2.0, 3.0], [7.0, 3.0]]);
    }

    #[test]
    fn line_pcurve_rejects_degenerate_or_unclosed_payloads() {
        let record = |mode: u8, values: &[f64]| {
            let mut payload = vec![0x81, 0x82, mode];
            for value in values {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x18,
                object_id: 7,
                payload,
            }
        };

        assert!(parse_line_pcurve(&record(0x01, &[2.0, 3.0, 0.0, 0.0, 1.0, 5.0])).is_none());
        assert!(parse_line_pcurve(&record(0x05, &[3.0, 2.0, 2.0])).is_none());
        assert!(parse_line_pcurve(&record(0x0d, &[3.0, -2.0, 7.0])).is_none());

        let mut tailed = record(0x09, &[3.0, -2.0, 7.0]);
        tailed.payload.push(0);
        assert!(parse_line_pcurve(&tailed).is_none());
    }

    #[test]
    fn circle_pcurve_preserves_arc_length_parameterization() {
        let mut payload = vec![0x81, 0x18, 0x34, 0x12];
        for value in [0.0, 0.0, 2.0, 0.0, 2.0 * std::f64::consts::PI, 1.0, 0.0] {
            if payload.len() == 20 {
                payload.extend_from_slice(&[0x05, 0x05]);
            }
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x19,
            object_id: 0x1235,
            payload,
        };
        let pcurve = parse_circle_pcurve(&record).expect("circle pcurve");
        assert_eq!(pcurve.surface, 0x1234);
        assert_eq!(pcurve.degree, 2);
        assert_eq!(
            pcurve.distinct_knots,
            [0.0, std::f64::consts::PI, 2.0 * std::f64::consts::PI]
        );
        assert_eq!(pcurve.multiplicities, [3, 2, 3]);
        assert_eq!(pcurve.control_points.len(), 5);
        let weights = pcurve.weights.expect("rational weights");
        assert_eq!(weights.len(), 5);
        assert!((weights[1] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        assert!((pcurve.control_points[0][0] - 2.0).abs() < 1e-12);
        assert!((pcurve.control_points[4][0] + 2.0).abs() < 1e-12);
        let mut wrong_family = record;
        wrong_family.family = 0xa8;
        assert!(parse_circle_pcurve(&wrong_family).is_none());
    }

    #[test]
    fn class_1a_pcurve_uses_diameter_period_parameterization() {
        let mut payload = vec![0x81, 0x18, 0x34, 0x12];
        for value in [3.0_f64, 4.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        for value in [
            2.0_f64,
            0.0,
            std::f64::consts::FRAC_PI_2,
            0.0,
            std::f64::consts::PI,
            1.0,
            2.0 * std::f64::consts::PI,
        ] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x1a,
            object_id: 0x1235,
            payload,
        };
        let pcurve = parse_class_1a_pcurve(&record).expect("class-1a pcurve");
        assert_eq!(pcurve.surface, 0x1234);
        assert_eq!(
            pcurve.distinct_knots,
            [0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI]
        );
        assert_eq!(pcurve.multiplicities, [3, 2, 3]);
        assert_eq!(pcurve.control_points.len(), 5);
        assert_eq!(evaluate_pcurve(&pcurve, 0.0), Some([4.0, 4.0]));
        let end = evaluate_pcurve(&pcurve, std::f64::consts::PI).expect("end point");
        assert!((end[0] - 2.0).abs() < 1e-12);
        assert!((end[1] - 4.0).abs() < 1e-12);

        let diameter = 1e-200_f64;
        let period = std::f64::consts::PI * diameter;
        let mut tiny = record.clone();
        tiny.payload[22..30].copy_from_slice(&diameter.to_le_bytes());
        tiny.payload[54..62].copy_from_slice(&(period * 0.5).to_le_bytes());
        tiny.payload[70..78].copy_from_slice(&period.to_le_bytes());
        assert!(parse_class_1a_pcurve(&tiny).is_some());

        let wrong_period = 1e-13_f64;
        tiny.payload[54..62].copy_from_slice(&(wrong_period * 0.5).to_le_bytes());
        tiny.payload[70..78].copy_from_slice(&wrong_period.to_le_bytes());
        assert!(parse_class_1a_pcurve(&tiny).is_none());

        let mut wrong_family = record;
        wrong_family.family = 0xa8;
        assert!(parse_class_1a_pcurve(&wrong_family).is_none());
    }

    #[test]
    fn class_1a_pcurve_accepts_a_finite_nonzero_diameter() {
        for diameter in [1e-200_f64, 1e200] {
            let period = std::f64::consts::PI * diameter;
            let mut payload = vec![0x81, 0x82];
            for value in [0.0_f64, 0.0] {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            payload.extend_from_slice(&[0x05, 0x05]);
            for value in [
                diameter,
                0.0,
                std::f64::consts::FRAC_PI_2,
                0.0,
                period * 0.25,
                1.0,
                period,
            ] {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            let record = B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x1a,
                object_id: 7,
                payload,
            };
            let pcurve = parse_class_1a_pcurve(&record).expect("class-1a pcurve");
            assert_eq!(pcurve.control_points[0], [diameter * 0.5, 0.0]);
        }
    }

    #[test]
    fn noncanonical_class_1a_payload_remains_opaque() {
        let mut payload = vec![0x81, 0x82];
        for value in [0.0_f64, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        for value in [2.0_f64, 0.0, 1.0, 0.0, 1.0, 1.0, 3.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x1a,
            object_id: 7,
            payload,
        };
        assert!(parse_class_1a_pcurve(&record).is_none());
        assert!(parse_opaque_pcurve(&record).is_some());
    }

    #[test]
    fn pcurve_evaluation_preserves_the_native_parameter() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![-1.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[2.0, 3.0], [4.0, 7.0]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        assert_eq!(evaluate_pcurve(&pcurve, 0.0), Some([3.0, 5.0]));
        assert_eq!(evaluate_pcurve(&pcurve, -1.0), Some([2.0, 3.0]));
        assert_eq!(evaluate_pcurve(&pcurve, 1.0), Some([4.0, 7.0]));
    }

    #[test]
    fn opaque_conic_pcurves_retain_support_identity_and_payload() {
        let mut ellipse = vec![0x81, 0x82];
        ellipse.extend_from_slice(&[0; 16]);
        ellipse.extend_from_slice(&[0x05, 0x05]);
        ellipse.extend_from_slice(&[0; 56]);
        let ellipse_record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x1a,
            object_id: 7,
            payload: ellipse.clone(),
        };
        assert_eq!(
            parse_opaque_pcurve(&ellipse_record),
            Some(B5OpaquePcurve {
                object_id: 7,
                surface: 2,
                class: 0x1a,
                payload: ellipse,
                sphere_great_circle: None,
            })
        );

        let mut class_1d = vec![0x81, 0x82];
        class_1d.extend_from_slice(&[0; 32]);
        class_1d.extend_from_slice(&[0x05, 0x81]);
        class_1d.extend_from_slice(&[0; 24]);
        class_1d.push(0x1d);
        class_1d.extend_from_slice(&[0; 40]);
        let class_1d_record = B5Record {
            class: 0x1d,
            payload: class_1d.clone(),
            ..ellipse_record
        };
        assert_eq!(
            parse_opaque_pcurve(&class_1d_record),
            Some(B5OpaquePcurve {
                object_id: 7,
                surface: 2,
                class: 0x1d,
                payload: class_1d,
                sphere_great_circle: None,
            })
        );
        let mut wrong_family = class_1d_record;
        wrong_family.family = 0xa8;
        assert_eq!(parse_opaque_pcurve(&wrong_family), None);
    }

    #[test]
    fn class_1d_pcurve_decodes_a_sphere_great_circle_plane() {
        let radius = 5.0;
        let chart_scale = 8.0;
        let chart_origin = 11.0;
        let mut payload = vec![0x81, 0x82];
        for value in [
            chart_scale * 0.25,
            chart_scale * 1.25,
            chart_origin,
            chart_origin + std::f64::consts::TAU * chart_scale,
        ] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x05, 0x81]);
        for value in [2.0_f64, -1.0, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.push(0x1d);
        for value in [chart_scale, -0.75, 1.0 / chart_scale, -1.25, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x1d,
            object_id: 7,
            payload,
        };
        let sphere = B5Surface::Sphere {
            center: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius,
            azimuth_range: [0.0, 1.5],
            latitude_range: [-1.0, 1.0],
            construction_radius: chart_scale,
            chart_origin,
        };

        assert_eq!(
            parse_sphere_great_circle_pcurve(&record, &sphere),
            Some(B5SphereGreatCirclePcurve {
                chart_bounds: [
                    [chart_scale * 0.25, chart_scale * 1.25],
                    [
                        chart_origin,
                        chart_origin + std::f64::consts::TAU * chart_scale,
                    ],
                ],
                chart_shift: 2.0,
                chart_scale,
                slope: -0.75,
                phase: -1.25,
            })
        );

        let mut outside_surface_chart = record.clone();
        outside_surface_chart.payload[2..10].copy_from_slice(&(-1.0_f64).to_le_bytes());
        assert_eq!(
            parse_sphere_great_circle_pcurve(&outside_surface_chart, &sphere),
            None
        );

        let mut approximate_reciprocal = outside_surface_chart.clone();
        approximate_reciprocal.payload[2..10].copy_from_slice(&(chart_scale * 0.25).to_le_bytes());
        let reciprocal_offset = 2 + 32 + 2 + 24 + 1 + 16;
        approximate_reciprocal.payload[reciprocal_offset..reciprocal_offset + 8]
            .copy_from_slice(&f64::from_bits((1.0 / chart_scale).to_bits() + 1).to_le_bytes());
        assert_eq!(
            parse_sphere_great_circle_pcurve(&approximate_reciprocal, &sphere),
            None
        );

        let mut approximate_chart_scale = outside_surface_chart;
        approximate_chart_scale.payload[2..10].copy_from_slice(&(chart_scale * 0.25).to_le_bytes());
        let scale_offset = 2 + 32 + 2 + 24 + 1;
        approximate_chart_scale.payload[scale_offset..scale_offset + 8]
            .copy_from_slice(&f64::from_bits(chart_scale.to_bits() + 1).to_le_bytes());
        assert_eq!(
            parse_sphere_great_circle_pcurve(&approximate_chart_scale, &sphere),
            None
        );

        let mut rounded_surface_bounds = sphere;
        let rounded_lower = f64::from_bits(0.25_f64.to_bits() + 1);
        let B5Surface::Sphere { azimuth_range, .. } = &mut rounded_surface_bounds else {
            unreachable!()
        };
        *azimuth_range = [rounded_lower, 1.5];
        assert!(parse_sphere_great_circle_pcurve(&record, &rounded_surface_bounds).is_some());
    }

    #[test]
    fn surface_aliases_require_their_complete_class_layout() {
        let alias = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2e,
            object_id: 9,
            payload: vec![0x38, 0x34, 0x12, 0x00],
        };
        assert_eq!(surface_alias_target(&alias), Some(0x1234));

        let counted = B5Record {
            payload: vec![0x81, 0x38, 0x34, 0x12, 0x00],
            ..alias.clone()
        };
        assert_eq!(surface_alias_target(&counted), Some(0x1234));

        let mut tailed = alias.clone();
        tailed.payload.push(0x05);
        assert_eq!(surface_alias_target(&tailed), None);

        let chart_alias = B5Record {
            class: 0x38,
            payload: vec![0x81, 0x38, 0x34, 0x12, 0x00, 0x05, 0x05, 0x09],
            ..alias
        };
        assert_eq!(surface_alias_target(&chart_alias), Some(0x1234));

        let mut truncated_chart_alias = chart_alias;
        truncated_chart_alias.payload.pop();
        assert_eq!(surface_alias_target(&truncated_chart_alias), None);
    }

    #[test]
    fn offset_surface_separates_result_carrier_source_and_bounds() {
        let carrier = B5Surface::Plane {
            origin: [0.0; 3],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let source = B5Surface::Plane {
            origin: [0.0, 0.0, 0.5],
            direction_u: [0.0, 1.0, 0.0],
            direction_v: [-1.0, 0.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let surfaces = BTreeMap::from([(2, carrier), (3, source)]);
        let mut payload = vec![0x82, 0x82, 0x83];
        payload.extend_from_slice(&(-0.5f64).to_le_bytes());
        payload.push(0x15);
        for value in [-2.0f64, 3.0, -4.0, 5.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x30,
            object_id: 9,
            payload,
        };
        assert_eq!(
            parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &HashMap::new()),
            Some(B5OffsetSurface {
                object_id: 9,
                carrier_surface: 2,
                source_surface: 3,
                distance: -0.5,
                carrier_kind: 0x15,
                parameter_bounds: [[-2.0, 3.0], [-4.0, 5.0]],
            })
        );
    }

    #[test]
    fn offset_surface_accepts_a_sphere_result_carrier() {
        let carrier = B5Surface::Sphere {
            center: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
            azimuth_range: [0.0, 1.0],
            latitude_range: [-1.0, 1.0],
            construction_radius: 2.0,
            chart_origin: -2.0,
        };
        let source = B5Surface::Sphere {
            center: [0.0; 3],
            direction_x: [0.0, 1.0, 0.0],
            direction_y: [-1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 8.5,
            azimuth_range: [0.0, 1.0],
            latitude_range: [-1.0, 1.0],
            construction_radius: 8.5,
            chart_origin: -2.0,
        };
        let surfaces = BTreeMap::from([(2, carrier), (3, source)]);
        let mut payload = vec![0x82, 0x82, 0x83];
        payload.extend_from_slice(&(-6.5_f64).to_le_bytes());
        payload.push(0x09);
        for value in [0.0_f64, 2.0, -2.0, 4.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x30,
            object_id: 9,
            payload,
        };

        assert_eq!(
            parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &HashMap::new()),
            Some(B5OffsetSurface {
                object_id: 9,
                carrier_surface: 2,
                source_surface: 3,
                distance: -6.5,
                carrier_kind: 0x09,
                parameter_bounds: [[0.0, 2.0], [-2.0, 4.0]],
            })
        );
    }

    #[test]
    fn offset_surface_does_not_infer_cone_construction_from_result_class() {
        let carrier = B5Surface::Cone {
            apex: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle: 0.25,
            pre_angular_range_scalar: 0.0,
            angular_range: [0.0, std::f64::consts::TAU],
            slant_range: [-2.0, 4.0],
            angular_scale: 3.0,
            angular_domain: [0.0, std::f64::consts::TAU],
        };
        let surfaces = BTreeMap::from([(2, carrier)]);
        let mut payload = vec![0x82, 0x82, 0x83];
        payload.extend_from_slice(&1.5_f64.to_le_bytes());
        payload.push(0x11);
        for value in [-3.0_f64, 3.0, -2.0, 4.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x30,
            object_id: 9,
            payload,
        };

        assert_eq!(
            parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &HashMap::new()),
            None
        );
    }

    #[test]
    fn analytic_offset_gate_requires_coaxial_equal_family_carriers() {
        let plane = |origin| B5Surface::Plane {
            origin,
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let tiny = 1e-200_f64;
        assert!(analytic_offset_magnitude_agrees(
            &plane([0.0, 0.0, tiny]),
            &plane([0.0; 3]),
            tiny
        ));
        assert!(!analytic_offset_magnitude_agrees(
            &plane([0.0, 0.0, tiny]),
            &plane([0.0; 3]),
            1e-13
        ));

        let cylinder = |origin, radius| B5Surface::Cylinder {
            origin,
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius,
            u_range: [0.0, std::f64::consts::TAU * radius],
            v_range: [-1.0, 1.0],
            angular_scale: radius,
            chart_origin: 0.0,
        };
        assert!(analytic_offset_magnitude_agrees(
            &cylinder([0.0, 0.0, 4.0], 3.0),
            &cylinder([0.0; 3], 5.0),
            -2.0
        ));
        assert!(!analytic_offset_magnitude_agrees(
            &cylinder([0.25, 0.0, 4.0], 3.0),
            &cylinder([0.0; 3], 5.0),
            -2.0
        ));
        assert!(analytic_offset_magnitude_agrees(
            &cylinder([0.0, 0.0, tiny], 2.0 * tiny),
            &cylinder([0.0; 3], tiny),
            tiny
        ));
        assert!(!analytic_offset_magnitude_agrees(
            &cylinder([0.0, 0.0, tiny], 2.0 * tiny),
            &cylinder([0.0; 3], tiny),
            1e-13
        ));
        assert!(!analytic_offset_magnitude_agrees(
            &cylinder([tiny, 0.0, tiny], 2.0 * tiny),
            &cylinder([0.0; 3], tiny),
            tiny
        ));

        let torus = |center, major_radius, minor_radius| B5Surface::Torus {
            center,
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            major_radius,
            minor_radius,
            major_angular_range: [0.0, std::f64::consts::TAU],
            major_angular_domain: [0.0, std::f64::consts::TAU],
            minor_angular_range: [0.0, std::f64::consts::TAU],
            minor_angular_domain: [0.0, std::f64::consts::TAU],
            major_scale: major_radius,
            minor_scale: minor_radius,
        };
        assert!(analytic_offset_magnitude_agrees(
            &torus([0.0; 3], 8.0, 3.0),
            &torus([0.0; 3], 8.0, 2.5),
            0.5
        ));
        assert!(!analytic_offset_magnitude_agrees(
            &torus([0.0; 3], 9.0, 3.0),
            &torus([0.0; 3], 8.0, 2.5),
            0.5
        ));
        assert!(!analytic_offset_magnitude_agrees(
            &torus([0.0; 3], 2.0 * tiny, 2.0 * tiny),
            &torus([0.0; 3], tiny, tiny),
            tiny
        ));

        let sphere = |center, radius| B5Surface::Sphere {
            center,
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius,
            azimuth_range: [0.0, 1.0],
            latitude_range: [-1.0, 1.0],
            construction_radius: radius,
            chart_origin: 0.0,
        };
        assert!(analytic_offset_magnitude_agrees(
            &sphere([0.0; 3], 2.0 * tiny),
            &sphere([0.0; 3], tiny),
            tiny
        ));
        assert!(!analytic_offset_magnitude_agrees(
            &sphere([1e-13, 0.0, 0.0], 2.0 * tiny),
            &sphere([0.0; 3], tiny),
            tiny
        ));
    }

    #[test]
    fn offset_surface_accepts_an_identity_checked_class_31_cache() {
        assert!(is_referenced_geometry_class(0xb5, 0x31));
        let source = B5Surface::Nurbs(NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_count: 2,
            v_count: 2,
            control_points: vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 4],
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        });
        let surfaces = BTreeMap::from([(3, source.clone()), (4, source)]);
        let mut cache_payload = vec![0x81, 0x84];
        for value in [-0.5f64, -2.0, -4.0, 3.0, 5.0] {
            cache_payload.extend_from_slice(&value.to_le_bytes());
        }
        let cache = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x31,
            object_id: 2,
            payload: cache_payload,
        };
        let records = HashMap::from([(2, &cache)]);
        let mut payload = vec![0x82, 0x82, 0x83];
        payload.extend_from_slice(&(-0.5f64).to_le_bytes());
        payload.push(0x01);
        for value in [-2.0f64, 3.0, -4.0, 5.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x30,
            object_id: 9,
            payload,
        };

        assert_eq!(
            parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &records),
            Some(B5OffsetSurface {
                object_id: 9,
                carrier_surface: 2,
                source_surface: 3,
                distance: -0.5,
                carrier_kind: 0x01,
                parameter_bounds: [[-2.0, 3.0], [-4.0, 5.0]],
            })
        );
    }

    #[test]
    fn extrusion_surface_binds_two_mapped_directrix_supports() {
        let mut pcurve_payload = vec![0x81, 0x86, 0x05];
        for value in [2.0f64, -3.0, 4.0] {
            pcurve_payload.extend_from_slice(&value.to_le_bytes());
        }
        let pcurve = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x18,
            object_id: 3,
            payload: pcurve_payload,
        };
        let mut wrapper_payload = vec![0x81, 0x83, 0x81, 0x01];
        for value in [-3.0f64, 4.0, 0.0] {
            wrapper_payload.extend_from_slice(&value.to_le_bytes());
        }
        wrapper_payload.push(0x01);
        let wrapper = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x24,
            object_id: 2,
            payload: wrapper_payload,
        };
        let mut directrix_payload = vec![0x82, 0x82, 0x84, 0x00];
        for value in [-3.0f64, 4.0, 0.01] {
            directrix_payload.extend_from_slice(&value.to_le_bytes());
        }
        directrix_payload.push(0x01);
        let directrix = B5Record {
            offset: 0,
            family: 0xa8,
            class: 0x25,
            object_id: 5,
            payload: directrix_payload,
        };
        let records = HashMap::from([(2, &wrapper), (3, &pcurve), (5, &directrix)]);
        let pcurves = BTreeMap::from([(4, object_stream_pcurve(7, vec![10.0, 20.0], None))]);
        let mut payload = vec![0x81, 0x85];
        for value in [0.0f64, 0.0, 1.0, -2.0, 6.0, 1.0, 0.0, -3.0, 4.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2c,
            object_id: 8,
            payload,
        };

        assert_eq!(
            parse_extrusion_surface(&record, &records, &pcurves),
            Some(B5ExtrusionSurface {
                object_id: 8,
                direction: [0.0, 0.0, 1.0],
                parameter_bounds: [[-2.0, 6.0], [-3.0, 4.0]],
                directrix: B5ExtrusionDirectrix::Intersection {
                    object_id: 5,
                    supports: [(6, 3, [-3.0, 4.0]), (7, 4, [10.0, 20.0])],
                    parameter_range: [-3.0, 4.0],
                    cache_fit_tolerance: 0.01,
                },
            })
        );
        let directrix_lower_bound = 2 + 7 * 8;
        let mut trimmed_interval = record.clone();
        trimmed_interval.payload[directrix_lower_bound..directrix_lower_bound + 8]
            .copy_from_slice(&(-2.0_f64).to_le_bytes());
        assert!(
            parse_extrusion_surface(&trimmed_interval, &records, &pcurves).is_some(),
            "the extrusion may use a strict subinterval of its directrix"
        );
        let mut outside_interval = record.clone();
        outside_interval.payload[directrix_lower_bound..directrix_lower_bound + 8]
            .copy_from_slice(&(-3.0_f64 - 1.0e-9).to_le_bytes());
        assert_eq!(
            parse_extrusion_surface(&outside_interval, &records, &pcurves),
            None,
            "the active interval must remain inside the directrix domain"
        );

        for controls in [[0x05, 0x05], [0x01, 0x29], [0x05, 0x29]] {
            let mut candidate = record.clone();
            let tail = candidate.payload.len() - 2;
            candidate.payload[tail..].copy_from_slice(&controls);
            assert!(
                parse_extrusion_surface(&candidate, &records, &pcurves).is_some(),
                "terminal controls {controls:02x?}"
            );
        }
        for controls in [
            [0x01, 0x05],
            [0x05, 0x09],
            [0x05, 0x11],
            [0x05, 0x15],
            [0x05, 0x19],
            [0x01, 0x09],
            [0x01, 0x15],
            [0x01, 0x19],
            [0x09, 0x29],
        ] {
            let mut candidate = record.clone();
            let tail = candidate.payload.len() - 2;
            candidate.payload[tail..].copy_from_slice(&controls);
            assert_eq!(
                parse_extrusion_surface(&candidate, &records, &pcurves),
                None,
                "terminal controls {controls:02x?}"
            );
        }
    }

    #[test]
    fn a8_class21_jet_decodes_a_piecewise_quintic_pcurve() {
        let mut payload = a8_class21_test_payload();

        let pcurve = parse_a8_class21_pcurve(7, &payload).expect("complete class-21 jet");
        assert_eq!(pcurve.object_id, 7);
        assert_eq!(pcurve.surface, 3);
        assert_eq!(pcurve.distinct_knots, [10.0, 20.0]);
        assert_eq!(pcurve.multiplicities, [6, 6]);
        assert_eq!(pcurve.control_points.len(), 6);
        assert_eq!(pcurve.parameter_range, Some([10.0, 20.0]));
        assert_eq!(pcurve.class_21_suffix_scalar, Some(10.0));

        payload[6] = 0x0d;
        assert_eq!(parse_a8_class21_pcurve(7, &payload), None);
    }

    fn a8_class21_large_test_payload(knot_count: usize) -> Vec<u8> {
        let mut payload = vec![0x81, 0x83, 0x01, 0x15, 0x01, 0x01, 0x08, 0x01, 0x20, 0x01];
        for index in 0..knot_count {
            payload.extend_from_slice(
                &f64::from(u32::try_from(index).expect("test knot index fits u32")).to_le_bytes(),
            );
        }
        payload.push(0x19);
        payload.extend(std::iter::repeat_n(0x0d, knot_count - 2));
        payload.push(0x19);
        for _ in 0..6 {
            for _ in 0..knot_count {
                payload.extend_from_slice(&0.0f64.to_le_bytes());
            }
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        for value in [0.0f64, 10.0, 1.0, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x00, 0x07]);
        payload
    }

    #[test]
    fn a8_class21_jet_uses_frame_extent_for_knot_count() {
        let knot_count = 8193;
        let payload = a8_class21_large_test_payload(knot_count);
        let pcurve = parse_a8_class21_pcurve(7, &payload).expect("frame-sized class-21 jet");

        assert_eq!(pcurve.distinct_knots.len(), knot_count);
        assert_eq!(pcurve.multiplicities.len(), knot_count);
        assert_eq!(pcurve.control_points.len(), 6 * (knot_count - 1));
    }

    #[test]
    fn a8_class21_jet_rejects_count_without_frame_extent() {
        let payload = vec![
            0x81, 0x83, 0x01, 0x15, 0x01, 0x01, 0x10, 0xff, 0xff, 0xff, 0xff, 0x01,
        ];

        assert_eq!(parse_a8_class21_pcurve(7, &payload), None);
    }

    #[test]
    fn a8_class21_scan_ignores_marker_shaped_nested_payload() {
        let child_payload = a8_class21_test_payload();
        let mut nested = vec![0xa8, 0x03, 0x21];
        nested.extend_from_slice(
            &u32::try_from(child_payload.len())
                .expect("small nested A8 payload")
                .to_le_bytes(),
        );
        nested.extend_from_slice(&7u32.to_le_bytes());
        nested.extend_from_slice(&child_payload);

        let mut wrapper = vec![0xa8, 0x03, 0x34];
        wrapper.extend_from_slice(
            &u32::try_from(nested.len())
                .expect("small wrapper A8 payload")
                .to_le_bytes(),
        );
        wrapper.extend_from_slice(&8u32.to_le_bytes());
        wrapper.extend_from_slice(&nested);

        let mut peer = vec![0xa8, 0x03, 0x21];
        peer.extend_from_slice(
            &u32::try_from(child_payload.len())
                .expect("small peer A8 payload")
                .to_le_bytes(),
        );
        peer.extend_from_slice(&9u32.to_le_bytes());
        peer.extend_from_slice(&child_payload);
        wrapper.extend_from_slice(&peer);

        let pcurves = a8_class21_pcurves(&wrapper);
        assert_eq!(pcurves.len(), 1);
        assert_eq!(pcurves[0].object_id, 9);
    }

    #[test]
    fn object_stream_frame_walk_descends_only_into_a8_b5_children() {
        let b5 = |class: u8, object_id: u32, payload: &[u8]| {
            let mut frame = vec![0xb5, 0x03, class, payload.len() as u8];
            frame.extend_from_slice(&object_id.to_le_bytes());
            frame.extend_from_slice(payload);
            frame
        };
        let nested_b5 = b5(0x5e, 7, &[0x00]);
        let mut wrapper = vec![0xa8, 0x03, 0x34];
        wrapper.extend_from_slice(
            &u32::try_from(nested_b5.len())
                .expect("small wrapper payload")
                .to_le_bytes(),
        );
        wrapper.extend_from_slice(&8u32.to_le_bytes());
        wrapper.extend_from_slice(&nested_b5);

        let mut nested_a8 = vec![0xa8, 0x03, 0x21];
        nested_a8.extend_from_slice(&1u32.to_le_bytes());
        nested_a8.extend_from_slice(&10u32.to_le_bytes());
        nested_a8.push(0x00);
        let peer_b5 = b5(0x5e, 9, &nested_a8);
        wrapper.extend_from_slice(&peer_b5);

        let frames = object_stream_frames(&wrapper);
        assert_eq!(
            frames
                .iter()
                .map(|frame| (frame.family, frame.class, frame.object_id))
                .collect::<Vec<_>>(),
            vec![(0xa8, 0x34, 8), (0xb5, 0x5e, 7), (0xb5, 0x5e, 9)]
        );
    }

    #[test]
    fn object_stream_frame_walk_ignores_marker_shaped_a8_payload_bytes() {
        let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
        nested_b5.extend_from_slice(&7u32.to_le_bytes());
        nested_b5.push(0x00);

        let mut payload = vec![0x00; 4];
        payload.extend_from_slice(&nested_b5);
        let mut wrapper = vec![0xa8, 0x03, 0x34];
        wrapper.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("small wrapper payload")
                .to_le_bytes(),
        );
        wrapper.extend_from_slice(&8u32.to_le_bytes());
        wrapper.extend_from_slice(&payload);

        assert_eq!(
            object_stream_frames(&wrapper)
                .iter()
                .map(|frame| (frame.family, frame.class, frame.object_id))
                .collect::<Vec<_>>(),
            vec![(0xa8, 0x34, 8)]
        );
    }

    #[test]
    fn object_stream_frame_walk_requires_a_length_closed_a8_child_run() {
        let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
        nested_b5.extend_from_slice(&7u32.to_le_bytes());
        nested_b5.push(0x00);
        let mut wrapper = vec![0xa8, 0x03, 0x34];
        let payload_len = nested_b5.len() + 1;
        wrapper.extend_from_slice(
            &u32::try_from(payload_len)
                .expect("small wrapper payload")
                .to_le_bytes(),
        );
        wrapper.extend_from_slice(&8u32.to_le_bytes());
        wrapper.extend_from_slice(&nested_b5);
        wrapper.push(0x00);

        assert_eq!(
            object_stream_frames(&wrapper)
                .iter()
                .map(|frame| (frame.family, frame.class, frame.object_id))
                .collect::<Vec<_>>(),
            vec![(0xa8, 0x34, 8)]
        );
    }

    #[test]
    fn object_stream_frame_walk_ignores_marker_shaped_inline_surface_poles() {
        let mut bytes = crate::tests::a8_surface_stream();
        let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
        nested_b5.extend_from_slice(&7u32.to_le_bytes());
        nested_b5.push(0x00);
        bytes[11 + 100..11 + 100 + nested_b5.len()].copy_from_slice(&nested_b5);

        assert_eq!(
            object_stream_frames(&bytes)
                .iter()
                .map(|frame| (frame.family, frame.class, frame.object_id))
                .collect::<Vec<_>>(),
            vec![(0xa8, 0x34, 0xdeca_fbad)]
        );
    }

    #[test]
    fn object_stream_frame_walk_descends_after_inline_surface_poles() {
        let mut bytes = crate::tests::a8_surface_stream();
        let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
        nested_b5.extend_from_slice(&7u32.to_le_bytes());
        nested_b5.push(0x00);
        bytes.extend_from_slice(&nested_b5);
        let payload_len = u32::try_from(bytes.len() - 11).expect("small surface payload");
        bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

        assert_eq!(
            object_stream_frames(&bytes)
                .iter()
                .map(|frame| (frame.family, frame.class, frame.object_id))
                .collect::<Vec<_>>(),
            vec![(0xa8, 0x34, 0xdeca_fbad), (0xb5, 0x5e, 7)]
        );
    }

    #[test]
    fn object_stream_frame_walk_descends_after_inline_surface_tail() {
        let mut bytes = crate::tests::a8_inline_tail_surface_stream();
        let mut nested_b5 = vec![0xb5, 0x03, 0x5e, 0x01];
        nested_b5.extend_from_slice(&7u32.to_le_bytes());
        nested_b5.push(0x00);
        bytes.extend_from_slice(&nested_b5);
        let payload_len = u32::try_from(bytes.len() - 11).expect("small surface payload");
        bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

        assert_eq!(
            object_stream_frames(&bytes)
                .iter()
                .map(|frame| (frame.family, frame.class, frame.object_id))
                .collect::<Vec<_>>(),
            vec![(0xa8, 0x34, 0xdeca_fbad), (0xb5, 0x5e, 7)]
        );
    }

    #[test]
    fn framed_records_ignore_marker_shaped_bytes_inside_b5_payloads() {
        let mut nested_a8 = vec![0xa8, 0x03, 0x62];
        nested_a8.extend_from_slice(&0u32.to_le_bytes());
        nested_a8.extend_from_slice(&7u32.to_le_bytes());

        let mut bytes = vec![0xb5, 0x03, 0x5f, nested_a8.len() as u8];
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&nested_a8);
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x01]);
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.push(0x00);

        let frames = object_stream_frames(&bytes);
        let records = framed_records(&bytes, &frames);
        assert_eq!(
            records
                .iter()
                .map(|record| (record.family, record.class, record.object_id))
                .collect::<Vec<_>>(),
            vec![(0xb5, 0x5f, 8), (0xb5, 0x5e, 9)]
        );
    }

    #[test]
    fn indexed_frame_parse_matches_one_shot_parse() {
        let bytes = crate::tests::b5_closed_triangle_stream();
        let frames = object_stream_frames(&bytes);
        let records = records_from_frames(&bytes, &frames);

        assert_eq!(parse(&bytes), parse_from_frames(&bytes, &frames));
        assert_eq!(
            parse(&bytes),
            parse_from_records(&bytes, &records, &frames, true)
        );
        assert_eq!(
            typed_face_records(&bytes),
            typed_face_records_from_records(&records)
        );
        assert_eq!(
            typed_loop_records(&bytes),
            typed_loop_records_from_records(&records)
        );
        assert_eq!(
            typed_edge_records(&bytes),
            typed_edge_records_from_records(&records)
        );
        assert_eq!(
            typed_vertex_incidence_links(&bytes),
            typed_vertex_incidence_links_from_records(&records)
        );
        assert_eq!(
            typed_class_21_pcurves(&bytes),
            typed_class_21_pcurves_from_records(&records)
        );
        assert_eq!(
            typed_parameter_incidences(&bytes),
            typed_parameter_incidences_from_records(&records)
        );
        assert_eq!(
            typed_vertex_incidence_rosters(&bytes),
            typed_vertex_incidence_rosters_from_records(&records)
        );
    }

    fn a8_class21_test_payload() -> Vec<u8> {
        let mut payload = vec![0x81, 0x83, 0x01, 0x15, 0x01, 0x01, 0x09, 0x01];
        for value in [10.0f64, 20.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x19, 0x19]);
        for channel in 0..6 {
            for station in 0..2 {
                payload.extend_from_slice(&(f64::from(channel * 2 + station)).to_le_bytes());
            }
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        for value in [0.0f64, 10.0, 1.0, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x00, 0x07]);
        payload
    }

    #[test]
    fn targeted_geometry_graph_closes_a_four_span_extrusion_without_topology() {
        let append_b5 = |bytes: &mut Vec<u8>, class, object_id: u32, payload: &[u8]| {
            bytes.extend_from_slice(&[
                0xb5,
                0x03,
                class,
                u8::try_from(payload.len()).expect("small B5 payload"),
            ]);
            bytes.extend_from_slice(&object_id.to_le_bytes());
            bytes.extend_from_slice(payload);
        };
        let append_a8 = |bytes: &mut Vec<u8>, class, object_id: u32, payload: &[u8]| {
            bytes.extend_from_slice(&[0xa8, 0x03, class]);
            bytes.extend_from_slice(
                &u32::try_from(payload.len())
                    .expect("small A8 payload")
                    .to_le_bytes(),
            );
            bytes.extend_from_slice(&object_id.to_le_bytes());
            bytes.extend_from_slice(payload);
        };
        let mut bytes = Vec::new();
        let mut plane = vec![0; 121];
        plane[0] = 0x80;
        for (offset, value) in [
            (25usize, 1.0f64),
            (57, 1.0),
            (73, 1.0),
            (81, 1.0),
            (89, -1.0),
            (97, 1.0),
            (105, -1.0),
            (113, 1.0),
        ] {
            plane[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        append_b5(&mut bytes, 0x27, 7, &plane);

        let knots = [0.0f64, 10.0, 20.0, 30.0, 40.0];
        let mut pcurve = vec![0x81, 0x87, 0x01, 0x15, 0x01, 0x01, 0x15, 0x01];
        for value in knots {
            pcurve.extend_from_slice(&value.to_le_bytes());
        }
        pcurve.extend_from_slice(&[0x19, 0x0d, 0x0d, 0x0d, 0x19]);
        for channel in 0..6 {
            for station in 0..knots.len() {
                pcurve.extend_from_slice(
                    &f64::from(
                        u32::try_from(channel * knots.len() + station)
                            .expect("small channel station"),
                    )
                    .to_le_bytes(),
                );
            }
        }
        pcurve.extend_from_slice(&[0x05, 0x05]);
        for value in [0.0f64, 10.0, 1.0, 0.0] {
            pcurve.extend_from_slice(&value.to_le_bytes());
        }
        pcurve.extend_from_slice(&[0x00, 0x07]);
        append_a8(&mut bytes, 0x21, 3, &pcurve);

        let mut wrapper = vec![0x81, 0x83, 0x81, 0x01];
        for value in [0.0f64, 40.0, 0.0] {
            wrapper.extend_from_slice(&value.to_le_bytes());
        }
        wrapper.push(0x01);
        append_b5(&mut bytes, 0x24, 2, &wrapper);

        let mut extrusion = vec![0x81, 0x82];
        for value in [0.0f64, 0.0, 1.0, -2.0, 6.0, 1.0, 0.0, 0.0, 10.0] {
            extrusion.extend_from_slice(&value.to_le_bytes());
        }
        extrusion.extend_from_slice(&[0x05, 0x11]);
        append_b5(&mut bytes, 0x2c, 8, &extrusion);

        let graph = targeted_geometry_graph(&bytes).expect("geometry-only graph");
        assert!(graph.faces.is_empty());
        assert_eq!(
            graph
                .extrusion_surfaces
                .get(&8)
                .map(|surface| surface.parameter_bounds),
            Some([[-2.0, 6.0], [0.0, 10.0]])
        );
        assert!(graph.pcurves.contains_key(&3));
    }

    #[test]
    fn extrusion_reparameters_a_class21_surface_curve_from_validated_knot_spans() {
        let mut wrapper_payload = vec![0x81, 0x83, 0x81, 0x01];
        for value in [10.0f64, 20.0, 0.0] {
            wrapper_payload.extend_from_slice(&value.to_le_bytes());
        }
        wrapper_payload.push(0x01);
        let wrapper = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x24,
            object_id: 2,
            payload: wrapper_payload,
        };
        let records = HashMap::from([(2, &wrapper)]);
        let pcurves = BTreeMap::from([(
            3,
            object_stream_pcurve(7, vec![-10.0, 10.0, 20.0, 30.0], Some(10.0)),
        )]);
        let mut payload = vec![0x81, 0x82];
        for value in [0.0f64, 0.0, 1.0, -2.0, 6.0, 1.0, 0.0, 0.0, 10.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2c,
            object_id: 8,
            payload,
        };

        assert_eq!(
            parse_extrusion_surface(&record, &records, &pcurves),
            Some(B5ExtrusionSurface {
                object_id: 8,
                direction: [0.0, 0.0, 1.0],
                parameter_bounds: [[-2.0, 6.0], [0.0, 10.0]],
                directrix: B5ExtrusionDirectrix::SurfaceCurve {
                    object_id: 2,
                    support: (7, 3, [10.0, 20.0]),
                    parameter_range: [0.0, 10.0],
                },
            })
        );

        let mut nonunit_direction = record.clone();
        nonunit_direction.payload[2..10].copy_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(
            parse_extrusion_surface(&nonunit_direction, &records, &pcurves),
            None
        );

        let mut translated_wrapper = wrapper.clone();
        translated_wrapper.payload[12..20].copy_from_slice(&50.0f64.to_le_bytes());
        let translated_records = HashMap::from([(2, &translated_wrapper)]);
        let translated_pcurves = BTreeMap::from([(
            3,
            object_stream_pcurve(
                7,
                vec![-10.0, 10.0, 20.0, 30.0, 40.0, 50.0, 70.0],
                Some(10.0),
            ),
        )]);
        let mut translated_control = record.clone();
        let tail = translated_control.payload.len() - 2;
        translated_control.payload[tail..].copy_from_slice(&[0x05, 0x11]);
        assert_eq!(
            parse_extrusion_surface(
                &translated_control,
                &translated_records,
                &translated_pcurves,
            ),
            Some(B5ExtrusionSurface {
                object_id: 8,
                direction: [0.0, 0.0, 1.0],
                parameter_bounds: [[-2.0, 6.0], [0.0, 10.0]],
                directrix: B5ExtrusionDirectrix::SurfaceCurve {
                    object_id: 2,
                    support: (7, 3, [10.0, 50.0]),
                    parameter_range: [0.0, 10.0],
                },
            })
        );

        let missing_suffix = BTreeMap::from([(
            3,
            object_stream_pcurve(7, vec![-10.0, 10.0, 20.0, 30.0, 40.0, 50.0, 70.0], None),
        )]);
        assert_eq!(
            parse_extrusion_surface(&translated_control, &translated_records, &missing_suffix),
            None
        );
        let mismatched_suffix = BTreeMap::from([(
            3,
            object_stream_pcurve(
                7,
                vec![-10.0, 10.0, 20.0, 30.0, 40.0, 50.0, 70.0],
                Some(9.0),
            ),
        )]);
        assert_eq!(
            parse_extrusion_surface(&translated_control, &translated_records, &mismatched_suffix,),
            None
        );
        let mut mismatched_span = translated_control.clone();
        let active_end = 2 + 8 * 8;
        mismatched_span.payload[active_end..active_end + 8].copy_from_slice(&9.0f64.to_le_bytes());
        assert_eq!(
            parse_extrusion_surface(&mismatched_span, &translated_records, &translated_pcurves,),
            None
        );

        let nonuniform_pcurve = BTreeMap::from([(
            3,
            object_stream_pcurve(
                7,
                vec![-10.0, 10.0, 20.0, 31.0, 40.0, 50.0, 70.0],
                Some(10.0),
            ),
        )]);
        assert_eq!(
            parse_extrusion_surface(&translated_control, &translated_records, &nonuniform_pcurve,),
            None,
            "05 11 requires four uniform source spans"
        );
    }

    #[test]
    fn extrusion_selects_the_terminal_span_of_a_direct_class20_pcurve() {
        let records = HashMap::new();
        for (controls, knots) in [
            ([0x05, 0x15], vec![0.0, 2.0, 5.0, 9.0, 12.0, 14.5]),
            ([0x05, 0x19], vec![0.0, 1.0, 3.0, 6.0, 10.0, 13.0, 15.5]),
        ] {
            let mut pcurve = object_stream_pcurve(7, knots.clone(), None);
            pcurve.class = 0x20;
            let pcurves = BTreeMap::from([(3, pcurve)]);
            let mut payload = vec![0x81, 0x83];
            for value in [0.0f64, 0.0, 1.0, -2.0, 6.0, 1.0, 0.0, 0.0, 2.5] {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            payload.extend_from_slice(&controls);
            let record = B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x2c,
                object_id: 8,
                payload,
            };
            let source_range = [knots[knots.len() - 2], knots[knots.len() - 1]];

            assert_eq!(
                parse_extrusion_surface(&record, &records, &pcurves),
                Some(B5ExtrusionSurface {
                    object_id: 8,
                    direction: [0.0, 0.0, 1.0],
                    parameter_bounds: [[-2.0, 6.0], [0.0, 2.5]],
                    directrix: B5ExtrusionDirectrix::SurfaceCurve {
                        object_id: 3,
                        support: (7, 3, source_range),
                        parameter_range: [0.0, 2.5],
                    },
                })
            );

            let wrong_class = BTreeMap::from([(3, object_stream_pcurve(7, knots.clone(), None))]);
            assert_eq!(
                parse_extrusion_surface(&record, &records, &wrong_class),
                None
            );

            let mut wrong_span = record.clone();
            let active_end = 2 + 8 * 8;
            wrong_span.payload[active_end..active_end + 8].copy_from_slice(&2.0f64.to_le_bytes());
            assert_eq!(
                parse_extrusion_surface(&wrong_span, &records, &pcurves),
                None
            );

            let mut extra_knot = knots;
            extra_knot.insert(1, 0.5);
            let mut pcurve = object_stream_pcurve(7, extra_knot, None);
            pcurve.class = 0x20;
            let wrong_cardinality = BTreeMap::from([(3, pcurve)]);
            assert_eq!(
                parse_extrusion_surface(&record, &records, &wrong_cardinality),
                None
            );
        }
    }

    #[test]
    fn offset_curve_directrix_binds_source_support_and_exact_ranges() {
        let mut source_payload = vec![0x81, 0x83, 0x81, 0x01];
        for value in [-3.0f64, 4.0, 0.0] {
            source_payload.extend_from_slice(&value.to_le_bytes());
        }
        source_payload.push(0x01);
        let source = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x24,
            object_id: 2,
            payload: source_payload,
        };
        let records = HashMap::from([(2, &source)]);
        let pcurves = BTreeMap::from([(3, object_stream_pcurve(7, vec![-3.0, 4.0], None))]);
        let mut payload = vec![0x81, 0x82];
        for value in [-3.0f64, 4.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.push(0x05);
        for value in [-1.5f64, 0.0, 0.0, 1.0, -5.0, 6.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x14,
            object_id: 4,
            payload,
        };

        assert_eq!(
            parse_extrusion_directrix(&record, &records, &pcurves),
            Some(B5ExtrusionDirectrix::Offset {
                object_id: 4,
                source: Box::new(B5ExtrusionDirectrix::SurfaceCurve {
                    object_id: 2,
                    support: (7, 3, [-3.0, 4.0]),
                    parameter_range: [-3.0, 4.0],
                }),
                source_parameter_range: [-3.0, 4.0],
                distance: -1.5,
                direction: [0.0, 0.0, 1.0],
                parameter_range: [-5.0, 6.0],
            })
        );

        let mut nonunit_direction = record.clone();
        nonunit_direction.payload[27..35].copy_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(
            parse_extrusion_directrix(&nonunit_direction, &records, &pcurves),
            None
        );

        let mut wrong_control = record.clone();
        wrong_control.payload[18] = 0x01;
        assert_eq!(
            parse_extrusion_directrix(&wrong_control, &records, &pcurves),
            None
        );
        let mut mismatched_range = record;
        mismatched_range.payload[1..9].copy_from_slice(&(-2.0f64).to_le_bytes());
        assert_eq!(
            parse_extrusion_directrix(&mismatched_range, &records, &pcurves),
            None
        );
    }

    #[test]
    fn contextual_offset_extrusion_uses_the_class30_result_chart() {
        let mut source_payload = vec![0x81, 0x83, 0x81, 0x01];
        for value in [-3.0f64, 4.0, 0.0] {
            source_payload.extend_from_slice(&value.to_le_bytes());
        }
        source_payload.push(0x01);
        let source = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x24,
            object_id: 2,
            payload: source_payload,
        };
        let mut offset_payload = vec![0x81, 0x82];
        for value in [-3.0f64, 4.0] {
            offset_payload.extend_from_slice(&value.to_le_bytes());
        }
        offset_payload.push(0x05);
        for value in [-1.5f64, 0.0, 0.0, 1.0, -5.0, 6.0] {
            offset_payload.extend_from_slice(&value.to_le_bytes());
        }
        let offset_directrix = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x14,
            object_id: 4,
            payload: offset_payload,
        };
        let records = HashMap::from([(2, &source), (4, &offset_directrix)]);
        let pcurves = BTreeMap::from([(3, object_stream_pcurve(7, vec![-3.0, 4.0], None))]);
        let source_extrusion = B5ExtrusionSurface {
            object_id: 10,
            direction: [0.0, 0.0, 1.0],
            parameter_bounds: [[0.0, 1.0], [0.0, 7.0]],
            directrix: B5ExtrusionDirectrix::SurfaceCurve {
                object_id: 2,
                support: (7, 3, [-3.0, 4.0]),
                parameter_range: [0.0, 7.0],
            },
        };
        let source_extrusions = BTreeMap::from([(10, source_extrusion)]);
        let offset_construction = B5OffsetSurface {
            object_id: 11,
            carrier_surface: 8,
            source_surface: 10,
            distance: -1.5,
            carrier_kind: 0x21,
            parameter_bounds: [[-5.0, 6.0], [2.0, 9.0]],
        };
        let mut carrier_payload = vec![0x81, 0x84];
        for value in [0.0f64, 0.0, 1.0, 2.0, 9.0, 1.0, 0.0, 35.0, 7.0] {
            carrier_payload.extend_from_slice(&value.to_le_bytes());
        }
        carrier_payload.extend_from_slice(&[0x01, 0x09]);
        let carrier = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2c,
            object_id: 8,
            payload: carrier_payload,
        };

        assert_eq!(
            parse_extrusion_surface_with_context(
                &carrier,
                &records,
                &pcurves,
                std::slice::from_ref(&offset_construction),
                &source_extrusions,
            ),
            Some(B5ExtrusionSurface {
                object_id: 8,
                direction: [0.0, 0.0, 1.0],
                parameter_bounds: [[2.0, 9.0], [-5.0, 6.0]],
                directrix: B5ExtrusionDirectrix::Offset {
                    object_id: 4,
                    source: Box::new(B5ExtrusionDirectrix::SurfaceCurve {
                        object_id: 2,
                        support: (7, 3, [-3.0, 4.0]),
                        parameter_range: [-3.0, 4.0],
                    }),
                    source_parameter_range: [-3.0, 4.0],
                    distance: -1.5,
                    direction: [0.0, 0.0, 1.0],
                    parameter_range: [-5.0, 6.0],
                },
            })
        );

        let mut wrong_distance = offset_construction.clone();
        wrong_distance.distance = -1.0;
        assert_eq!(
            parse_extrusion_surface_with_context(
                &carrier,
                &records,
                &pcurves,
                &[wrong_distance],
                &source_extrusions,
            ),
            None
        );

        let mut wrong_bounds = offset_construction.clone();
        wrong_bounds.parameter_bounds[0][1] = 7.0;
        assert_eq!(
            parse_extrusion_surface_with_context(
                &carrier,
                &records,
                &pcurves,
                &[wrong_bounds],
                &source_extrusions,
            ),
            None
        );

        let mut increasing_auxiliary_scalars = carrier;
        let auxiliary_start = 2 + 7 * 8;
        increasing_auxiliary_scalars.payload[auxiliary_start..auxiliary_start + 8]
            .copy_from_slice(&7.0f64.to_le_bytes());
        increasing_auxiliary_scalars.payload[auxiliary_start + 8..auxiliary_start + 16]
            .copy_from_slice(&35.0f64.to_le_bytes());
        assert_eq!(
            parse_extrusion_surface_with_context(
                &increasing_auxiliary_scalars,
                &records,
                &pcurves,
                std::slice::from_ref(&offset_construction),
                &source_extrusions,
            ),
            None
        );
    }

    #[test]
    fn supported_surface_preserves_ordered_support_pcurves() {
        let pcurve0 = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x18,
            object_id: 5,
            payload: vec![0x81, 0x83],
        };
        let mut payload = vec![0x85, 0x82, 0x83, 0x84, 0x85, 0x86, 0x09, 0x05];
        payload.extend_from_slice(&2.5f64.to_le_bytes());
        payload.extend_from_slice(&[0x03, 0x05]);
        payload.extend_from_slice(&0.0f64.to_le_bytes());
        payload.extend_from_slice(&[0x01, 0x05]);
        let record = B5Record {
            class: 0x37,
            object_id: 7,
            payload,
            ..pcurve0.clone()
        };
        assert_eq!(
            parse_supported_surface(&record),
            Some(B5SupportedSurface {
                object_id: 7,
                carrier_surface: 2,
                support_surfaces: [3, 4],
                support_pcurves: [5, 6],
                parameters: B5SupportedSurfaceParameters::Radius {
                    controls: [0x09, 0x05, 0x03, 0x05, 0x01, 0x05],
                    construction_radius: 2.5,
                },
            })
        );
        let mut scalar_pair_payload = vec![0x85, 0x82, 0x83, 0x84, 0x85, 0x86];
        scalar_pair_payload.extend_from_slice(&[0x09, 0x01, 0x01, 0x05, 0x05, 0x0d]);
        scalar_pair_payload.extend_from_slice(&101.6f64.to_le_bytes());
        scalar_pair_payload.extend_from_slice(&20.0f64.to_le_bytes());
        let scalar_pair = B5Record {
            class: 0x3b,
            payload: scalar_pair_payload,
            ..record.clone()
        };
        assert_eq!(
            parse_supported_surface(&scalar_pair),
            Some(B5SupportedSurface {
                object_id: 7,
                carrier_surface: 2,
                support_surfaces: [3, 4],
                support_pcurves: [5, 6],
                parameters: B5SupportedSurfaceParameters::ScalarPair {
                    controls: [0x09, 0x01, 0x01, 0x05, 0x05, 0x0d],
                    scalars: [101.6, 20.0],
                },
            })
        );
        let scalar_pair =
            parse_supported_surface(&scalar_pair).expect("two-scalar supported surface");
        let plane = B5Surface::Plane {
            origin: [0.0; 3],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        assert!(supported_surface_parameters_match_carrier(
            &scalar_pair.parameters,
            &plane
        ));
        let cone_parameters = B5SupportedSurfaceParameters::ScalarPair {
            controls: [0x05, 0x05, 0x01, 0x03, 0x05, 0x11],
            scalars: [0.76, std::f64::consts::FRAC_PI_4],
        };
        let cone = B5Surface::Cone {
            apex: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle: std::f64::consts::FRAC_PI_4,
            pre_angular_range_scalar: 0.0,
            angular_range: [0.0, std::f64::consts::TAU],
            slant_range: [0.0, 1.0],
            angular_scale: 1.0,
            angular_domain: [0.0, std::f64::consts::TAU],
        };
        assert!(supported_surface_parameters_match_carrier(
            &cone_parameters,
            &cone
        ));
        let wrong_cone = B5Surface::Cone {
            apex: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle: std::f64::consts::FRAC_PI_6,
            pre_angular_range_scalar: 0.0,
            angular_range: [0.0, std::f64::consts::TAU],
            slant_range: [0.0, 1.0],
            angular_scale: 1.0,
            angular_domain: [0.0, std::f64::consts::TAU],
        };
        assert!(!supported_surface_parameters_match_carrier(
            &cone_parameters,
            &wrong_cone
        ));
        let construction = parse_supported_surface(&record).expect("supported surface");
        let pcurve0 = B5Record {
            object_id: 5,
            payload: vec![0x81, 0x83],
            ..pcurve0.clone()
        };
        let pcurve1 = B5Record {
            object_id: 6,
            payload: vec![0x81, 0x84],
            ..pcurve0.clone()
        };
        let records = HashMap::from([(5, &pcurve0), (6, &pcurve1)]);
        assert!(supported_surface_pcurves_match(
            &construction,
            &records,
            &HashMap::new()
        ));

        let wrong = B5Record {
            payload: vec![0x81, 0x82],
            ..pcurve1
        };
        assert!(!supported_surface_pcurves_match(
            &construction,
            &HashMap::from([(5, &pcurve0), (6, &wrong)]),
            &HashMap::new()
        ));
    }

    #[test]
    fn supported_surface_parameter_matching_is_scale_independent() {
        let radius = 1e-200_f64;
        let parameters = B5SupportedSurfaceParameters::Radius {
            controls: [0; 6],
            construction_radius: radius,
        };
        let cylinder = |carrier_radius| B5Surface::Cylinder {
            origin: [0.0; 3],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: carrier_radius,
            u_range: [0.0, std::f64::consts::TAU * carrier_radius],
            v_range: [-1.0, 1.0],
            angular_scale: carrier_radius,
            chart_origin: 0.0,
        };
        let torus = |carrier_radius| B5Surface::Torus {
            center: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            major_radius: 1.0,
            minor_radius: carrier_radius,
            major_angular_range: [0.0, std::f64::consts::TAU],
            major_angular_domain: [0.0, std::f64::consts::TAU],
            minor_angular_range: [0.0, std::f64::consts::TAU],
            minor_angular_domain: [0.0, std::f64::consts::TAU],
            major_scale: 1.0,
            minor_scale: carrier_radius,
        };
        let sphere = |carrier_radius| B5Surface::Sphere {
            center: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 1.0,
            azimuth_range: [0.0, 1.0],
            latitude_range: [-1.0, 1.0],
            construction_radius: carrier_radius,
            chart_origin: 0.0,
        };

        for carrier in [cylinder(radius), torus(radius), sphere(radius)] {
            assert!(supported_surface_parameters_match_carrier(
                &parameters,
                &carrier
            ));
        }
        for carrier in [
            cylinder(2.0 * radius),
            torus(2.0 * radius),
            sphere(2.0 * radius),
        ] {
            assert!(!supported_surface_parameters_match_carrier(
                &parameters,
                &carrier
            ));
        }

        let half_angle = 1e-200_f64;
        let cone_parameters = B5SupportedSurfaceParameters::ScalarPair {
            controls: [0; 6],
            scalars: [1.0, half_angle],
        };
        let cone = |carrier_half_angle| B5Surface::Cone {
            apex: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle: carrier_half_angle,
            pre_angular_range_scalar: 0.0,
            angular_range: [0.0, std::f64::consts::TAU],
            slant_range: [0.0, 1.0],
            angular_scale: 1.0,
            angular_domain: [0.0, std::f64::consts::TAU],
        };
        assert!(supported_surface_parameters_match_carrier(
            &cone_parameters,
            &cone(half_angle)
        ));
        assert!(!supported_surface_parameters_match_carrier(
            &cone_parameters,
            &cone(2.0 * half_angle)
        ));
    }

    #[test]
    fn record_walk_includes_wide_header_loop_nodes() {
        let mut bytes = vec![0xa8, 0x03, 0x62];
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&[0x83, 0x81, 0x82]);
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        bytes.extend_from_slice(&8u32.to_le_bytes());
        assert_eq!(
            records(&bytes),
            vec![
                B5Record {
                    offset: 0,
                    family: 0xa8,
                    class: 0x62,
                    object_id: 7,
                    payload: vec![0x83, 0x81, 0x82],
                },
                B5Record {
                    offset: 14,
                    family: 0xb5,
                    class: 0x5e,
                    object_id: 8,
                    payload: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn record_walk_retains_opaque_a8_surface_nodes() {
        let mut bytes = vec![0xa8, 0x03, 0x34];
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3]);
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        bytes.extend_from_slice(&8u32.to_le_bytes());
        let records = records(&bytes);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].family, 0xa8);
        assert_eq!(records[0].class, 0x34);
        assert_eq!(records[0].object_id, 7);
        assert_eq!(records[0].payload, [1, 2, 3]);
        assert_eq!(
            surface_node(&records[0], None),
            Some(B5Surface::Unknown {
                family: 0xa8,
                class: 0x34,
                payload: vec![1, 2, 3],
            })
        );
    }

    #[test]
    fn record_walk_descends_into_length_bounded_a8_wrappers() {
        let mut payload = vec![0xb5, 0x03, 0x27, 0x00];
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        payload.extend_from_slice(&2u32.to_le_bytes());

        let mut bytes = vec![0xa8, 0x03, 0x34];
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        bytes.extend_from_slice(&3u32.to_le_bytes());

        assert_eq!(
            records(&bytes)
                .iter()
                .map(|record| (record.offset, record.object_id, record.class))
                .collect::<Vec<_>>(),
            vec![(11, 1, 0x27), (19, 2, 0x5e), (0, 7, 0x34), (27, 3, 0x5e)]
        );
    }

    #[test]
    fn record_walk_crosses_alternate_flag_bridge_records() {
        let mut bytes = vec![0xb5, 0x03, 0x27, 0x00];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0xb5, 0x13, 0x5b, 0x00]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        assert_eq!(
            records(&bytes)
                .iter()
                .map(|record| (record.object_id, record.class))
                .collect::<Vec<_>>(),
            vec![(1, 0x27), (3, 0x5e)]
        );
    }

    #[test]
    fn record_walk_admits_unique_isolated_geometry_by_topology_reference() {
        fn append(bytes: &mut Vec<u8>, class: u8, object_id: u32, payload: &[u8]) {
            bytes.extend_from_slice(&[
                0xb5,
                0x03,
                class,
                u8::try_from(payload.len()).expect("test payload fits the B5 length lane"),
            ]);
            bytes.extend_from_slice(&object_id.to_le_bytes());
            bytes.extend_from_slice(payload);
        }

        let mut bytes = Vec::new();
        append(&mut bytes, 0x27, 1, &[]);
        bytes.push(0xff);
        append(&mut bytes, 0x19, 2, &[]);
        bytes.push(0xff);
        append(&mut bytes, 0x62, 4, &[0x83, 0x82, 0x83, 0x81]);
        append(&mut bytes, 0x5e, 3, &[]);
        append(&mut bytes, 0x5f, 5, &[0x82, 0x81, 0x84]);

        let parsed = records(&bytes);
        assert_eq!(
            parsed
                .iter()
                .map(|record| (record.object_id, record.class))
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                (1, 0x27),
                (2, 0x19),
                (3, 0x5e),
                (4, 0x62),
                (5, 0x5f),
            ])
        );

        bytes.push(0xff);
        append(&mut bytes, 0x19, 2, &[0x01]);
        assert!(!records(&bytes).iter().any(|record| record.object_id == 2));
    }

    #[test]
    fn record_walk_closes_native_vertex_incidence_dependencies() {
        fn append(bytes: &mut Vec<u8>, class: u8, object_id: u32, payload: &[u8]) {
            bytes.extend_from_slice(&[0xb5, 0x03, class, payload.len() as u8]);
            bytes.extend_from_slice(&object_id.to_le_bytes());
            bytes.extend_from_slice(payload);
        }

        let mut bytes = Vec::new();
        append(&mut bytes, 0x18, 2, &[0x81, 0x81]);
        bytes.push(0xff);
        append(&mut bytes, 0x5d, 6, &[0x81, 0x87, 0x00]);
        bytes.push(0xff);
        append(&mut bytes, 0x05, 7, &[0x81, 0x88]);
        bytes.push(0xff);
        let mut parameter = vec![0x81, 0x82, 0x81];
        parameter.extend_from_slice(&0.5f64.to_le_bytes());
        parameter.push(0x05);
        append(&mut bytes, 0x06, 8, &parameter);
        bytes.push(0xff);
        append(
            &mut bytes,
            0x5e,
            10,
            &[0x85, 0x82, 0x86, 0x86, 0x88, 0x88, 0x21],
        );
        append(&mut bytes, 0x5f, 11, &[]);

        assert_eq!(
            records(&bytes)
                .iter()
                .map(|record| (record.object_id, record.class))
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                (2, 0x18),
                (6, 0x5d),
                (7, 0x05),
                (8, 0x06),
                (10, 0x5e),
                (11, 0x5f),
            ])
        );
    }

    #[test]
    fn native_vertex_graph_rejects_inconsistent_ordered_loci() {
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 5e-3, 0.0],
            [0.0, 5e-3, 0.0],
        ];
        let constraints = [([10, 11], [0, 1]), ([11, 12], [2, 3]), ([12, 10], [3, 0])];
        let adjacency = HashMap::from([(10, vec![0, 2]), (11, vec![0, 1]), (12, vec![1, 2])]);
        let mapping = propagate_vertex_points(&constraints, &adjacency, &points);
        assert!(mapping.is_empty());
    }

    #[test]
    fn edge_record_retains_references_and_each_admitted_terminal_control() {
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x5e,
            object_id: 17,
            payload: vec![0x85, 0x92, 0x8f, 0x95, 0x93, 0x94, 0x21],
        };
        assert_eq!(
            parse_edge(&record),
            Some(B5Edge {
                object_id: 17,
                support: 18,
                vertices: [15, 21],
                parameter_incidences: [19, 20],
                terminal_control: 0x21,
            })
        );

        let mut standard = record;
        for terminal_control in [0x01, 0x02, 0x21, 0x22, 0x25, 0x26, 0x29, 0x2a] {
            *standard.payload.last_mut().expect("tail") = terminal_control;
            assert_eq!(
                parse_edge(&standard).map(|edge| edge.terminal_control),
                Some(terminal_control)
            );
        }
        standard.payload.pop();
        assert!(parse_edge(&standard).is_none());
        standard.payload.extend_from_slice(&[0x21, 0x00]);
        assert!(parse_edge(&standard).is_none());
        standard.payload.truncate(6);
        standard.payload.push(0x03);
        assert!(parse_edge(&standard).is_none());
        *standard.payload.last_mut().expect("tail") = 0x01;

        let mut bytes = vec![0xb5, 0x03, 0x5e, 7];
        bytes.extend_from_slice(&standard.object_id.to_le_bytes());
        bytes.extend_from_slice(&standard.payload);
        assert_eq!(
            edge_vertex_references(&bytes),
            BTreeMap::from([(17, [15, 21])])
        );
    }

    #[test]
    fn referenced_edge_vertex_references_excludes_unreferenced_allocations() {
        let mut graph = parse(&crate::tests::b5_closed_triangle_stream()).expect("B5 graph");
        assert!(graph.complete);
        graph.edges.insert(
            301,
            B5Edge {
                object_id: 301,
                support: 600,
                vertices: [10, 11],
                parameter_incidences: [20, 21],
                terminal_control: 0x01,
            },
        );
        graph.edges.insert(
            900,
            B5Edge {
                object_id: 900,
                support: 601,
                vertices: [12, 13],
                parameter_incidences: [22, 23],
                terminal_control: 0x01,
            },
        );

        assert_eq!(
            graph.referenced_edge_vertex_references(),
            Some(BTreeMap::from([(301, [10, 11])]))
        );

        graph.complete = false;
        assert_eq!(graph.referenced_edge_vertex_references(), None);
    }

    #[test]
    fn duplicate_face_loop_ownership_does_not_close_the_graph() {
        let mut bytes = crate::tests::b5_closed_triangle_stream();
        let mut face_payload = vec![0x82];
        face_payload.extend_from_slice(&crate::tests::b5_object_ref(100));
        face_payload.extend_from_slice(&crate::tests::b5_object_ref(400));
        face_payload.push(0x03);
        crate::tests::append_b5_record(&mut bytes, 0x5f, 902, &face_payload);

        let graph = parse(&bytes).expect("structurally parseable B5 graph");

        assert_eq!(graph.faces.len(), 2);
        assert_eq!(graph.loops.len(), 1);
        assert!(!graph.complete);
        assert_eq!(face_loop_owner_counts(&graph.faces).get(&400), Some(&2));
    }

    #[test]
    fn vertex_incidence_link_accepts_both_exact_terminal_controls() {
        let record = |terminal_control| B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x5d,
            object_id: 17,
            payload: vec![0x81, 0x92, terminal_control],
        };
        for terminal_control in [0x00, 0x04] {
            assert_eq!(
                parse_vertex_incidence_link(&record(terminal_control)),
                Some(B5VertexIncidenceLink {
                    object_id: 17,
                    incidence: 18,
                    terminal_control,
                })
            );
        }
        assert_eq!(parse_vertex_incidence_link(&record(0x01)), None);

        let mut missing = record(0x00);
        missing.payload.pop();
        assert_eq!(parse_vertex_incidence_link(&missing), None);

        let mut residual = record(0x04);
        residual.payload.push(0);
        assert_eq!(parse_vertex_incidence_link(&residual), None);
    }

    #[test]
    fn loop_and_endpoint_incidences_bind_an_unframed_pcurve_occurrence() {
        let incidence_payload = |parameter: f64, control| {
            let mut payload = vec![0x81, 0x89, 0x81];
            payload.extend_from_slice(&parameter.to_le_bytes());
            payload.push(control);
            payload
        };
        let records = vec![
            B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x62,
                object_id: 1,
                payload: vec![
                    0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01,
                    0x00, 0x01,
                ],
            },
            B5Record {
                offset: 1,
                family: 0xb5,
                class: 0x5e,
                object_id: 10,
                payload: vec![0x85, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x21],
            },
            B5Record {
                offset: 2,
                family: 0xb5,
                class: 0x06,
                object_id: 15,
                payload: incidence_payload(0.0, 0x15),
            },
            B5Record {
                offset: 3,
                family: 0xb5,
                class: 0x06,
                object_id: 16,
                payload: incidence_payload(1.0, 0x05),
            },
        ];
        let by_id = records
            .iter()
            .map(|record| (record.object_id, record))
            .collect();
        let surfaces = BTreeMap::from([(
            11,
            B5Surface::Unknown {
                family: 0xb5,
                class: 0x34,
                payload: Vec::new(),
            },
        )]);

        assert_eq!(
            implicit_pcurve_bindings(
                &records,
                &by_id,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &surfaces,
            ),
            BTreeMap::from([(9, 11)])
        );
    }

    #[test]
    fn parameter_incidence_retains_aligned_compact_controls() {
        let mut payload = vec![0x82, 0x89, 0x8a, 0x82];
        payload.extend_from_slice(&1.25f64.to_le_bytes());
        payload.push(0x15);
        payload.extend_from_slice(&2.5f64.to_le_bytes());
        payload.push(0x2d);
        let incidence = parameter_incidence(&B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x06,
            object_id: 17,
            payload,
        })
        .expect("parameter incidence");

        assert_eq!(incidence.object_id, 17);
        assert_eq!(incidence.curves, [9, 10]);
        assert_eq!(incidence.parameters, [1.25, 2.5]);
        assert_eq!(incidence.controls, [5, 11]);
    }

    #[test]
    fn loop_and_edge_curve_wrapper_bind_an_unframed_pcurve_occurrence() {
        let records = vec![
            B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x62,
                object_id: 1,
                payload: vec![
                    0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01,
                    0x00, 0x01,
                ],
            },
            B5Record {
                offset: 1,
                family: 0xb5,
                class: 0x5e,
                object_id: 10,
                payload: vec![0x85, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x22],
            },
            B5Record {
                offset: 2,
                family: 0xb5,
                class: 0x25,
                object_id: 12,
                payload: vec![0x82, 0x89, 0x91, 0x81],
            },
        ];
        let by_id = records
            .iter()
            .map(|record| (record.object_id, record))
            .collect();
        let surfaces = BTreeMap::from([(
            11,
            B5Surface::Unknown {
                family: 0xb5,
                class: 0x34,
                payload: Vec::new(),
            },
        )]);

        assert_eq!(
            implicit_pcurve_bindings(
                &records,
                &by_id,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &surfaces,
            ),
            BTreeMap::from([(9, 11)])
        );
    }
}
