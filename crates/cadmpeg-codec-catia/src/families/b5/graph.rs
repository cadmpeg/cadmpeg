// SPDX-License-Identifier: Apache-2.0
//! Object-id topology in the CATIA `b5 03` short-frame family.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use cadmpeg_core::decode::{alloc_filled, View, WorkBudget};
use cadmpeg_ir::eval::{nurbs_pcurve_uv, nurbs_surface_point};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    analytic::ConeSurface,
    nurbs::{knots_strictly_increasing, NurbsSurface},
    ProceduralSurfaceDefinition,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, FiniteReal, NonNegativeLength, PositiveLength, PositiveReal};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

/// Admitted topology control bytes.
pub(in crate::families) mod controls;
/// Vertex tables and typed endpoint references.
pub(in crate::families) mod vertex_refs;
use vertex_refs::{B5VertexRef, B5Vertices};

use controls::{B5EdgeTerminalControl, B5FramingControl, B5VertexIncidenceControl};

use super::vecmath::{add, components, coordinates, cross, scale};
use crate::analytic::{periodic_angular_range_is_valid, sphere_angular_ranges_are_valid};
use crate::checked::ExactUnitVector3;
use crate::wire;
use crate::wire::bytes::{f64_le, f64_point, read_f64_array};

const EPS_B5_GRAPH_GEOMETRY: f64 = 1.0e-9;
const EPS_B5_GRAPH_DEGENERATE: f64 = 1.0e-10;
const EPS_B5_GRAPH_EXACT_GEOMETRY: f64 = 1.0e-12;

/// Maximum frame-index, record-materialization, census, and graph-selection
/// operations admitted for one free-form object population. The allowance
/// covers one indexed-frame pass, topology materialization, dependency
/// closure, and one bounded graph-resolution pass.
pub(in crate::families) const MAX_OBJECT_STREAM_SELECTION_WORK: usize = 2_000_000;

/// Resolved `b5 03` object-stream topology graph: faces, loops, pcurves, and
/// surfaces bound through the in-stream `object_id` map ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)),
/// together with the `05 08 01` vertex points used to bind edge endpoints.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct B5Graph {
    /// `true` when every serialized face and loop node belongs to the resolved
    /// reference-closed graph; `false` when the graph is its maximal closed
    /// subset.
    pub(in crate::families) complete: bool,
    /// `b5 03 5f` face nodes, in stream declaration order (equal to STEP
    /// `ADVANCED_FACE` order, [spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
    pub(in crate::families) faces: Vec<B5Face>,
    /// Structurally complete `b5 03 5f` records, keyed by object id.
    pub(in crate::families) face_records: BTreeMap<u32, B5FaceRecord>,
    /// `b5 03 62` loop nodes, keyed by `object_id`.
    pub(in crate::families) loops: BTreeMap<u32, B5Loop>,
    /// `b5 03 21` pcurve nodes, keyed by `object_id`.
    pub(in crate::families) pcurves: BTreeMap<u32, B5Pcurve>,
    /// Structurally bounded pcurve records whose parameter-space geometry is
    /// not yet assigned, keyed by `object_id`.
    pub(in crate::families) opaque_pcurves: BTreeMap<u32, B5OpaquePcurve>,
    /// Pcurve occurrence ids whose support is bound by a loop and both native
    /// edge-endpoint incidence records, but which have no standalone geometry
    /// record.
    pub(in crate::families) implicit_pcurves: BTreeMap<u32, u32>,
    /// `b5 03 27/28/2d` analytic surface nodes and `a8 03 34` NURBS
    /// surfaces, keyed by `object_id`.
    pub(in crate::families) surfaces: BTreeMap<u32, B5Surface>,
    /// Resolved class-`2e`/`38` surface alias targets, keyed by alias identity.
    pub(in crate::families) surface_aliases: BTreeMap<u32, u32>,
    /// `b5 03 30` offset constructions, keyed by their result surface id.
    pub(in crate::families) offset_surfaces: BTreeMap<u32, B5OffsetSurface>,
    /// `b5 03 2c` extrusion constructions, keyed by their result surface id.
    pub(in crate::families) extrusion_surfaces: BTreeMap<u32, B5ExtrusionSurface>,
    /// `b5 03 37/3b` support-bound constructions, keyed by result surface id.
    pub(in crate::families) supported_surfaces: BTreeMap<u32, B5SupportedSurface>,
    /// Native class-`06` curve-parameter incidences, keyed by object id.
    pub(in crate::families) parameter_incidences: BTreeMap<u32, B5ParameterIncidence>,
    /// Native class-`5e` physical-edge records, keyed by object id.
    pub(in crate::families) edges: BTreeMap<u32, B5Edge>,
    /// Native class-`5d` vertex-to-incidence links, keyed by object id.
    pub(in crate::families) vertex_incidence_links: BTreeMap<u32, B5VertexIncidenceLink>,
    /// Vertex tables with bounds-checked edge bindings.
    pub(in crate::families) vertices: B5Vertices,
    /// Ordered class-`06` start/end parameter-incidence references from each
    /// native class-`5e` edge.
    pub(in crate::families) edge_parameter_incidences: BTreeMap<u32, [u32; 2]>,
    /// Maximum incident endpoint residual for each logical vertex, keyed by
    /// the combined vertex index used by `edge_vertices`.
    pub(in crate::families) vertex_tolerances: BTreeMap<usize, PositiveReal>,
    /// `b5 03 0e`/`0f` line and arc profile curves, keyed by `object_id`;
    /// referenced by `B5Surface::Revolution::profile_curve`.
    pub(in crate::families) profiles: BTreeMap<u32, B5Profile>,
}

/// One native `5d` logical vertex: its object id and resolved world-frame
/// coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::families) struct B5LogicalVertex {
    /// Native `5d` object id.
    pub(in crate::families) object_id: u32,
    /// World-frame coordinate resolved from native incidence.
    pub(in crate::families) point: [f64; 3],
}

impl B5Graph {
    /// Follow surface aliases to their canonical terminal identity.
    #[must_use]
    pub(super) fn canonical_surface_id(&self, object_id: u32) -> Option<u32> {
        canonical_surface_id(&self.surface_aliases, object_id)
    }

    /// Return native edge identities proven to belong to the closed B-rep.
    ///
    /// A structurally parseable class-`5e` allocation is a physical edge only
    /// when a resolved face loop references it.  An incomplete graph cannot
    /// prove that the retained loop set is exhaustive, so callers must use
    /// their unresolved-association fallback in that case.
    #[must_use]
    pub(in crate::families) fn referenced_edge_vertex_references(
        &self,
    ) -> Option<BTreeMap<u32, [u32; 2]>> {
        if !self.complete {
            return None;
        }
        let referenced_edges = self
            .loops
            .values()
            .flat_map(|loop_| loop_.members.iter().map(|member| member.edge))
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
pub(super) fn edge_pcurve_parameters(
    graph: &B5Graph,
    edge: u32,
    pcurve: u32,
) -> Option<[FiniteReal; 2]> {
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
) -> Option<[FiniteReal; 2]> {
    edge_parameter_incidences
        .get(&edge)?
        .map(|incidence_id| {
            let incidence = parameter_incidences.get(&incidence_id)?;
            let mut parameters = incidence
                .lanes
                .iter()
                .filter_map(|lane| (lane.curve == pcurve).then_some(lane.parameter));
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
pub(in crate::families) fn canonical_surface_id(
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
pub(in crate::families) enum B5Profile {
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
    pub(super) fn parameter_range(&self) -> [f64; 2] {
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
pub(in crate::families) enum B5Surface {
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
    /// `b5 03 27`: a plane spanned by `origin`, the stored first in-plane
    /// direction and `direction_v`.
    Plane {
        /// A point on the plane.
        origin: FinitePoint3,
        /// The unit normal `direction_u × direction_v`, normalized by its
        /// largest component, and the stored first in-plane unit direction
        /// `direction_u`.
        frame: OrthonormalFrame3,
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
        origin: FinitePoint3,
        /// The unit cylinder axis `stored_u × stored_v`, normalized by its
        /// largest component, and the stored unit direction `stored_u`, the
        /// zero-angle ray.
        frame: OrthonormalFrame3,
        /// Positive cylinder radius.
        radius: PositiveLength,
        /// Active native circumferential interval.
        u_range: [f64; 2],
        /// Active native axial interval.
        v_range: [f64; 2],
        /// Divisor mapping native U to azimuth.
        angular_scale: FiniteReal,
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
        /// Reference radius of the conical surface, independent of the active chart ranges.
        reference_radius: f64,
        /// Active azimuth interval.
        angular_range: [f64; 2],
        /// Native slant-coordinate range.
        slant_range: [f64; 2],
        /// Positive divisor mapping native U to azimuth.
        angular_scale: PositiveReal,
        /// Full-turn azimuth chart domain.
        angular_domain: [f64; 2],
        /// Neutral carrier: its origin is the axis point at the slant-interval
        /// start and its radius is the cross-section radius there. Absent
        /// when that origin is not finite.
        surface: Option<ConeSurface>,
    },
    /// `b5 03 2a`: a sphere with a radius-scaled right-handed frame.
    Sphere {
        /// Sphere center.
        center: FinitePoint3,
        /// The polar-axis unit direction and the zero-azimuth unit direction.
        frame: OrthonormalFrame3,
        /// Quarter-turn azimuth unit direction.
        direction_y: [f64; 3],
        /// Positive sphere radius.
        radius: PositiveLength,
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
        center: FinitePoint3,
        /// The torus axis and the zero-major-angle direction.
        frame: OrthonormalFrame3,
        /// Quarter-turn major-angle direction.
        direction_y: [f64; 3],
        /// Positive major radius.
        major_radius: PositiveLength,
        /// Positive minor radius.
        minor_radius: PositiveLength,
        /// Active major-angle interval.
        major_angular_range: [f64; 2],
        /// Full-turn major-angle chart domain.
        major_angular_domain: [f64; 2],
        /// Active minor-angle interval.
        minor_angular_range: [f64; 2],
        /// Full-turn minor-angle chart domain.
        minor_angular_domain: [f64; 2],
        /// Positive divisor mapping native U to the major angle.
        major_scale: PositiveReal,
        /// Positive divisor mapping native V to the minor angle.
        minor_scale: PositiveReal,
    },
    /// `b5 03 2d`: a surface of revolution sweeping `profile_curve` about
    /// `axis_origin`/`axis_direction`.
    Revolution {
        /// `object_id` of the swept [`B5Profile`].
        profile_curve: u32,
        /// A point on the revolution axis.
        axis_origin: FinitePoint3,
        /// Unit revolution axis of the stored right-handed frame.
        axis_direction: UnitVector3,
        /// Active parameter interval of the swept profile.
        profile_range: [f64; 2],
        /// Active native arc-length interval of the revolution.
        angular_range: [f64; 2],
        /// Positive divisor mapping native V to a revolution angle.
        angular_scale: PositiveReal,
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

/// An offset result carrier kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum B5OffsetCarrierKind {
    /// The cache carrier form.
    Cache,
    /// The cylinder carrier form.
    Cylinder,
    /// The sphere carrier form.
    Sphere,
    /// The torus carrier form.
    Torus,
    /// The plane carrier form.
    Plane,
    /// The rollingball carrier form.
    RollingBall,
    /// The extrusion carrier form.
    Extrusion,
}

impl B5OffsetCarrierKind {
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Self::Cache),
            0x05 => Some(Self::Cylinder),
            0x09 => Some(Self::Sphere),
            0x0d => Some(Self::Torus),
            0x15 => Some(Self::Plane),
            0x19 => Some(Self::RollingBall),
            0x21 => Some(Self::Extrusion),
            _ => None,
        }
    }
}

/// A `b5 03 30` offset construction with an explicit result carrier.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct B5OffsetSurface {
    /// This construction's result surface id.
    pub(super) object_id: u32,
    /// Explicit analytic carrier for the offset result.
    pub(super) carrier_surface: u32,
    /// Surface from which the result is offset.
    pub(super) source_surface: u32,
    /// Signed offset distance in millimetres.
    pub(super) distance: FiniteReal,
    /// Native carrier-kind discriminator.
    pub(super) carrier_kind: B5OffsetCarrierKind,
    /// Increasing native U and V bounds.
    pub(super) parameter_bounds: [IncreasingParameterInterval; 2],
}

/// A `b5 03 2c` extrusion construction with a two-support directrix.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct B5ExtrusionSurface {
    /// This construction's result surface id.
    pub(super) object_id: u32,
    /// Unit world-space extrusion direction.
    pub(super) direction: UnitVector3,
    /// Increasing native U and V intervals.
    pub(super) parameter_bounds: [IncreasingParameterInterval; 2],
    /// Exact directrix construction.
    pub(super) directrix: B5ExtrusionDirectrix,
}

/// Exact directrix construction selected by a `b5 03 2c` extrusion.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum B5ExtrusionDirectrix {
    /// Two-support intersection carried by an `a8 03 25` record.
    Intersection {
        /// Persistent directrix object id.
        object_id: u32,
        /// Ordered `(surface, pcurve, pcurve range)` support sides.
        supports: [(u32, u32, [FiniteReal; 2]); 2],
        /// Increasing solved-curve parameter range.
        parameter_range: [f64; 2],
        /// Positive fit tolerance of the serialized sampled cache.
        cache_fit_tolerance: PositiveReal,
    },
    /// One-support curve carried by a `b5 03 24` wrapper.
    SurfaceCurve {
        /// Persistent wrapper object id.
        object_id: u32,
        /// `(surface, pcurve, pcurve range)` support side.
        support: (u32, u32, [FiniteReal; 2]),
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
        /// Signed nonzero offset distance.
        distance: FiniteReal,
        /// Unit direction defining the positive offset side.
        direction: UnitVector3,
        /// Increasing result-curve parameter range.
        parameter_range: [f64; 2],
    },
}

impl B5ExtrusionDirectrix {
    pub(super) fn object_id(&self) -> u32 {
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

    pub(super) fn supports(&self) -> Vec<(u32, u32, [FiniteReal; 2])> {
        match self {
            Self::Intersection { supports, .. } => supports.to_vec(),
            Self::SurfaceCurve { support, .. } => vec![*support],
            Self::Offset { source, .. } => source.supports(),
        }
    }
}

/// A class-`37` support-bound surface construction with an explicit result carrier.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct B5SupportedSurface {
    /// This construction's result surface id.
    pub(super) object_id: u32,
    /// Explicit carrier for the result geometry and chart.
    pub(super) carrier_surface: u32,
    /// Ordered construction support surfaces.
    pub(super) support_surfaces: [u32; 2],
    /// Ordered pcurves, one bound to each support surface.
    pub(super) support_pcurves: [u32; 2],
    /// Class-specific native controls and scalar parameters.
    pub(super) parameters: B5SupportedSurfaceParameters,
}

/// Native parameter layouts of a support-bound surface construction.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum B5SupportedSurfaceParameters {
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
pub(in crate::families) struct B5ParameterIncidence {
    /// This record's stream object id.
    pub(super) object_id: u32,
    /// Curve, parameter, and control triples in serialized order.
    pub(in crate::families) lanes: Vec<B5IncidenceLane>,
}

/// One curve/parameter/control triple of a class-`06` incidence record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::families) struct B5IncidenceLane {
    /// Referenced curve or pcurve object id.
    pub(super) curve: u32,
    /// Finite native parameter on that curve.
    pub(super) parameter: FiniteReal,
    /// Compact native control for this lane.
    pub(super) control: u32,
}

/// One complete class-`5e` physical-edge reference production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::families) struct B5Edge {
    /// This record's stream object id.
    object_id: u32,
    /// Referenced curve-support wrapper.
    support: u32,
    /// Ordered start/end class-`5d` vertex identities.
    vertices: [u32; 2],
    /// Ordered start/end class-`06` parameter-incidence identities.
    parameter_incidences: [u32; 2],
    /// Exact admitted terminal control.
    pub(in crate::families) terminal_control: B5EdgeTerminalControl,
}

/// One complete class-`5d` vertex-to-incidence reference production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::families) struct B5VertexIncidenceLink {
    /// This record's stream object id.
    object_id: u32,
    /// Referenced counted class-`05` incidence roster.
    incidence: u32,
    /// Exact admitted terminal control.
    pub(in crate::families) terminal_control: B5VertexIncidenceControl,
}

/// A resolved `b5 03 18`, `b5 03 19`, or `b5 03 21` pcurve node, represented as a 2D
/// B-spline curve in a surface's
/// parameter space ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct B5Pcurve {
    /// This record's stream `object_id`.
    pub(super) object_id: u32,
    /// `object_id` of the owning surface, taken directly from the pcurve's
    /// `catia_support_ref` ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
    pub(super) surface: u32,
    /// B-spline degree.
    pub(super) degree: u32,
    /// Distinct knot values, strictly increasing.
    pub(super) distinct_knots: Vec<FiniteReal>,
    /// Per-knot multiplicities, index-aligned with `distinct_knots`.
    pub(super) multiplicities: Vec<u32>,
    /// `(u, v)` control points in the surface's parameter space.
    pub(super) control_points: Vec<[f64; 2]>,
    /// Positive per-pole rational weights. `None` denotes a polynomial
    /// pcurve.
    pub(super) weights: Option<Vec<PositiveReal>>,
    /// Explicit occurrence parameter interval when the pcurve record stores one.
    pub(super) parameter_range: Option<[FiniteReal; 2]>,
    /// Coordinate convention for evaluating the stored knot vector.
    pub(super) parameterization: B5PcurveParameterization,
    /// Positive scalar stored in the exact class-`21` suffix. When a class-`21`
    /// pcurve is present, it is the length of the zero-based occurrence interval.
    pub(in crate::families) class_21_suffix_scalar: Option<f64>,
    /// The curve's two clamped-end poles lifted through `surface` into
    /// world-frame 3D points, or `None` before [`parse`] resolves them or
    /// when the lift fails (unresolved surface, degenerate revolution
    /// scale, or NURBS evaluation failure).
    pub(super) lifted_endpoints: Option<[FinitePoint3; 2]>,
}

/// Parameter coordinates used by one B5 pcurve record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum B5PcurveParameterization {
    /// The pcurve's occurrence coordinate is its serialized knot coordinate.
    Native,
    /// The occurrence coordinate starts at zero while the serialized knot
    /// vector starts at `native_origin`.
    Translated {
        /// Native knot coordinate corresponding to occurrence station zero.
        native_origin: FiniteReal,
    },
}

/// Exact great-circle fields carried by a class-`1d` sphere pcurve.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct B5SphereGreatCirclePcurve {
    /// Increasing length-valued U bounds of the curve in the sphere chart.
    pub(super) u_bounds: IncreasingParameterInterval,
    /// Length-valued V bounds of the curve in the sphere chart.
    pub(super) v_bounds: [FiniteReal; 2],
    /// Length-valued shift contributing to the great-circle plane phase.
    pub(super) chart_shift: FiniteReal,
    /// Positive length scale converting the sphere chart's angular
    /// coordinates.
    pub(super) chart_scale: PositiveReal,
    /// Signed slope in `tan(latitude) = slope * cos(azimuth - phase)`.
    pub(super) slope: FiniteReal,
    /// Stored phase term. The geometric phase is `chart_shift / chart_scale + phase`.
    pub(super) phase: FiniteReal,
}

/// An identity- and support-resolved pcurve whose native chart equation is
/// opaque or only partly assigned.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct B5OpaquePcurve {
    /// This record's stream `object_id`.
    pub(super) object_id: u32,
    /// Owning surface object id.
    pub(super) surface: u32,
    /// Native pcurve class.
    pub(super) class: u8,
    /// Exact source payload.
    pub(super) payload: Vec<u8>,
    /// Exact great-circle carrier fields when this is a validated class-`1d`
    /// sphere pcurve.
    pub(super) sphere_great_circle: Option<B5SphereGreatCirclePcurve>,
}

/// One length-framed `b5 03` record as found by the stream walk ([spec §6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#6-object-stream-record-framing-a5-03-a8-03-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::families) struct B5Record {
    /// Byte offset of the `b5 03` marker in the source stream.
    pub(in crate::families) offset: usize,
    /// Record family byte (`0xb5` or `0xa8`).
    pub(in crate::families) family: u8,
    /// Third header byte: the record's type/class code (`0x5f` face,
    /// `0x62` loop, `0x21` pcurve, `0x27`/`0x28`/`0x2d` surface, `0x5e`
    /// edge, `0x18` line pcurve, `0x0e`/`0x0f` profile, ...).
    pub(in crate::families) class: u8,
    /// Dense creation-order `object_id` stored inline at `+4` ([spec §6.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#65-a8-03-common-object-stream-freeform-class)).
    pub(in crate::families) object_id: u32,
    /// Raw record payload after the 8-byte header.
    pub(in crate::families) payload: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(in crate::families) struct ObjectFrame {
    pub(in crate::families) start: usize,
    pub(in crate::families) end: usize,
    family: u8,
    pub(in crate::families) class: u8,
    pub(in crate::families) object_id: u32,
}

type DependencyCandidates = HashMap<u32, Option<ObjectFrame>>;

/// A resolved `b5 03 5f` face node ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::families) struct B5Face {
    /// This record's stream `object_id`.
    pub(in crate::families) object_id: u32,
    /// `object_id` of the face's surface, taken from the first reference
    /// token.
    pub(super) surface: u32,
    /// `object_id`s of the face's `b5 03 62` loop nodes, in reference
    /// order.
    pub(super) loops: Vec<u32>,
    /// Exact terminal control of the counted face production. Uncounted face
    /// framing has no terminal control.
    pub(in crate::families) terminal_control: Option<B5FramingControl>,
}

/// Count the face incidences for each object-stream loop.
pub(super) fn face_loop_owner_counts(faces: &[B5Face]) -> HashMap<u32, usize> {
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
pub(in crate::families) struct B5FaceRecord {
    /// This record's stream object id.
    pub(in crate::families) object_id: u32,
    /// Ordered native references.
    pub(in crate::families) references: Vec<u32>,
    /// Exact counted-production terminal control. Uncounted framing has no
    /// terminal control.
    pub(in crate::families) terminal_control: Option<B5FramingControl>,
}

/// A resolved `b5 03 62` loop node: payload `<0x80 + n_refs>
/// (pcurve_ref edge_ref)* surface_ref` ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct B5Loop {
    /// This record's stream `object_id`.
    pub(super) object_id: u32,
    /// Pcurve/edge occurrences in serialized order, each with its native
    /// control triple.
    pub(super) members: Vec<B5LoopMember>,
    /// Source-native framing and optional numeric extension.
    pub(in crate::families) metadata: B5LoopMetadata,
    /// `object_id` of the loop's surface (the trailing reference token).
    pub(super) surface: u32,
}

/// One pcurve/edge occurrence of a class-`62` loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct B5LoopMember {
    /// `object_id` of the member pcurve (or `0x18` line).
    pub(super) pcurve: u32,
    /// `object_id` of the member `b5 03 5e` edge.
    pub(super) edge: u32,
    /// Three signed controls for this occurrence.
    pub(super) controls: [i16; 3],
}

/// Complete metadata following a class-`62` loop's reference lanes.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct B5LoopMetadata {
    /// Primary and secondary loop framing controls.
    pub(in crate::families) framing_controls: [B5FramingControl; 2],
    /// Optional fixed-width numeric extension.
    pub(in crate::families) extension: Option<B5LoopMetadataExtension>,
}

/// Optional fixed-width numeric extension of a class-`62` loop.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct B5LoopMetadataExtension {
    /// Four finite binary64 fields in serialized order.
    scalars: [f64; 4],
    /// Exact admitted odd extension control.
    control: u8,
    /// Six finite binary32 fields in serialized order.
    floats: [f32; 6],
}

impl B5Loop {
    pub(super) fn edge_senses(&self) -> Vec<bool> {
        self.members
            .iter()
            .map(|member| member.controls[0] == -1)
            .collect()
    }

    pub(super) fn pcurve_senses(&self) -> Vec<bool> {
        self.members
            .iter()
            .map(|member| member.controls[2] == -1)
            .collect()
    }
}

/// Resolve the dominant object-stream topology graph through inline object ids.
#[must_use]
pub(crate) fn parse(bytes: &[u8], refusal: &mut crate::nurbs::LaneRefusals) -> Option<B5Graph> {
    let mut graphs = topology_runs(bytes, refusal)
        .into_iter()
        .map(|(_, graph)| graph);
    let graph = graphs.next()?;
    graphs.next().is_none().then_some(graph)
}

/// Resolve each contiguous object-stream run independently.
fn topology_runs(
    bytes: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Vec<(Range<usize>, B5Graph)> {
    let root_runs = topology_root_run_ranges(bytes);
    let candidates = if root_runs.is_empty() {
        object_stream_run_ranges(bytes)
    } else {
        root_runs
    };
    candidates
        .into_iter()
        .filter_map(|range| {
            let population = owned_object_stream_population(bytes, range.clone());
            parse_flat(&population, refusal).map(|graph| (range, graph))
        })
        .collect()
}

fn parse_flat(bytes: &[u8], refusal: &mut crate::nurbs::LaneRefusals) -> Option<B5Graph> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    parse_from_records(bytes, &records, &frames, true, refusal)
}

pub(in crate::families) fn parse_from_frames(
    bytes: &[u8],
    frames: &[ObjectFrame],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<B5Graph> {
    let records = records_from_frames(bytes, frames);
    parse_from_records(bytes, &records, frames, true, refusal)
}

pub(in crate::families) fn parse_from_records(
    bytes: &[u8],
    records: &[B5Record],
    frames: &[ObjectFrame],
    require_topology: bool,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<B5Graph> {
    parse_from_records_budgeted(bytes, records, frames, require_topology, None, refusal)
}

pub(in crate::families) fn parse_from_records_budgeted(
    bytes: &[u8],
    records: &[B5Record],
    frames: &[ObjectFrame],
    require_topology: bool,
    budget: Option<&WorkBudget<'_>>,
    refusal: &mut crate::nurbs::LaneRefusals,
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
    let a8_headers: BTreeMap<u32, crate::families::a5a8::records::A8SurfaceHeader> = frames
        .iter()
        .filter_map(|frame| {
            crate::families::a5a8::records::a8_surface_header_from_object_frame(
                bytes,
                frame.start,
                frame.end,
                frame.object_id,
            )
        })
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
    for surface in frames.iter().filter_map(|frame| {
        crate::families::a5a8::records::resolved_a8_surface_from_object_frame(
            bytes,
            frame.start,
            frame.end,
            frame.object_id,
            refusal,
        )
    }) {
        if let Some(object_id) = surface.object_id() {
            merge_surface_candidate(
                &mut surfaces,
                &mut conflicting_surfaces,
                object_id,
                B5Surface::Nurbs(surface.geometry),
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
    let has_extrusion_candidates = records
        .iter()
        .any(|record| record.family == 0xb5 && record.class == 0x2c);
    if has_extrusion_candidates {
        loop {
            if budget.is_some_and(|budget| !budget.charge_by(records.len())) {
                return None;
            }
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
    let has_surface_fixpoint_candidates = !offset_constructions.is_empty()
        || records.iter().any(|record| {
            record.family == 0xb5 && matches!(record.class, 0x2e | 0x30 | 0x37 | 0x38 | 0x3b)
        });
    if has_surface_fixpoint_candidates {
        loop {
            if budget.is_some_and(|budget| !budget.charge_by(records.len())) {
                return None;
            }
            let mut changed =
                resolve_surface_aliases(records, &by_id, &mut surfaces, &mut conflicting_surfaces);
            for record in records {
                let Some(offset) =
                    parse_offset_surface(record, &surfaces, &extrusion_surfaces, &by_id)
                else {
                    continue;
                };
                let carrier = if let Some(carrier) = surfaces.get(&offset.carrier_surface).cloned()
                {
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
    let surface_aliases = records
        .iter()
        .filter(|record| surfaces.contains_key(&record.object_id))
        .filter_map(|record| surface_alias_target(record).map(|target| (record.object_id, target)))
        .collect();
    let faces: Vec<B5Face> = records
        .iter()
        .filter_map(|record| face_records.get(&record.object_id))
        .filter_map(|record| parse_face(record, &loops, &surfaces, &surface_aliases))
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
    let logical_vertices = bound_vertices.vertices;
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
                loop_.members.iter().all(|member| {
                    (pcurves
                        .get(&member.pcurve)
                        .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                        || opaque_pcurves
                            .get(&member.pcurve)
                            .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                        || implicit_pcurves.get(&member.pcurve) == Some(&loop_.surface))
                        && edge_vertices.contains_key(&member.edge)
                }) && loop_chain_closes(loop_, &edge_vertices)
            })
        });
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
        vertices: B5Vertices::try_new(vertex_points, logical_vertices, edge_vertices).ok()?,
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
    let (_, control_points) = jet.bspline()?;
    Some(B5Pcurve {
        object_id: jet.object_id,
        surface: jet.support_id,
        degree: crate::families::a5a8::records::A8Pcurve::DEGREE,
        distinct_knots: jet.knots(),
        multiplicities: vec![crate::families::a5a8::records::A8Pcurve::DEGREE + 1; jet.sites.len()],
        control_points,
        weights: None,
        parameter_range: Some(jet.range),
        parameterization: B5PcurveParameterization::Native,
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
    let surface = wire::tokens::object_ref(payload, &mut position, true)?;
    (payload.get(position) == Some(&0x01)).then_some(())?;
    position += 1;
    let degree = wire::tokens::compact_uint(payload, &mut position)?;
    (degree == 5 && payload.get(position..position + 2) == Some(&[0x01, 0x01])).then_some(())?;
    position += 2;
    let knot_count = usize::try_from(wire::tokens::compact_uint(payload, &mut position)?).ok()?;
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
    let read_values = |position: &mut usize| -> Option<Vec<FiniteReal>> {
        let mut values = Vec::with_capacity(knot_count);
        for _ in 0..knot_count {
            values.push(f64_le(payload, *position)?);
            *position = position.checked_add(8)?;
        }
        Some(values)
    };
    let distinct_knots = read_values(&mut position)?;
    let knot_values = distinct_knots
        .iter()
        .copied()
        .map(FiniteReal::get)
        .collect::<Vec<_>>();
    knots_strictly_increasing(&knot_values).then_some(())?;
    let multiplicities = (0..knot_count)
        .map(|_| wire::tokens::compact_uint(payload, &mut position))
        .collect::<Option<Vec<_>>>()?;
    (multiplicities.first() == Some(&(degree + 1))
        && multiplicities.last() == Some(&(degree + 1))
        && multiplicities[1..knot_count - 1]
            .iter()
            .all(|multiplicity| *multiplicity == 3))
    .then_some(())?;
    let read_lane = |position: &mut usize| {
        read_values(position)
            .map(|values| values.into_iter().map(FiniteReal::get).collect::<Vec<_>>())
    };
    let u = read_lane(&mut position)?;
    let v = read_lane(&mut position)?;
    let du = read_lane(&mut position)?;
    let dv = read_lane(&mut position)?;
    let ddu = read_lane(&mut position)?;
    let ddv = read_lane(&mut position)?;
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
        crate::nurbs::quintic_jet_bspline(degree, &knot_values, &points, &first, &second)?;
    let tail = payload.get(position..)?;
    let tail_control = tail.get(..2);
    let extension_control = tail.get(34..36);
    (matches!(tail.len(), 36 | 38)
        && (tail_control == Some(&[0x05, 0x05]) || tail_control == Some(&[0x05, 0x11]))
        && f64_le(tail, 2)?.get() == 0.0
        && f64_le(tail, 10)?.get() > 0.0
        && f64_le(tail, 18)?.get() == 1.0
        && f64_le(tail, 26)?.get() == 0.0
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
        multiplicities: alloc_filled(knot_count, degree + 1, "catia B5 pcurve multiplicities")
            .ok()?,
        control_points,
        weights: None,
        parameter_range: Some(parameter_range),
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: Some(f64_le(tail, 10)?.get()),
        lifted_endpoints: None,
    })
}

/// Return native start/end vertex identities for every framed `b5 03 5e`
/// edge, keyed by the edge object id.
#[must_use]
pub(in crate::families) fn edge_vertex_references(bytes: &[u8]) -> BTreeMap<u32, [u32; 2]> {
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
fn edge_support_pcurve_references(
    bytes: &[u8],
    edge_ids: &HashSet<u32>,
) -> BTreeMap<u32, [u32; 2]> {
    let frames = object_stream_frames(bytes);
    edge_support_pcurve_references_from_frames(bytes, edge_ids, &frames)
}

pub(in crate::families) fn edge_support_pcurve_references_from_frames(
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

pub(in crate::families) fn targeted_surfaces_from_frames(
    bytes: &[u8],
    object_ids: &HashSet<u32>,
    frames: &[ObjectFrame],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> BTreeMap<u32, B5Surface> {
    let mut resolved = HashMap::<u32, Option<B5Surface>>::new();
    for surface in frames.iter().filter_map(|frame| {
        crate::families::a5a8::records::resolved_a8_surface_from_object_frame(
            bytes,
            frame.start,
            frame.end,
            frame.object_id,
            refusal,
        )
    }) {
        let Some(object_id) = surface.object_id() else {
            continue;
        };
        merge_targeted_surface(&mut resolved, object_id, B5Surface::Nurbs(surface.geometry));
    }
    let headers = frames
        .iter()
        .filter_map(|frame| {
            crate::families::a5a8::records::a8_surface_header_from_object_frame(
                bytes,
                frame.start,
                frame.end,
                frame.object_id,
            )
        })
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
fn targeted_geometry_graph(bytes: &[u8]) -> Option<B5Graph> {
    let frames = object_stream_frames(bytes);
    targeted_geometry_graph_from_frames(bytes, &frames, &mut crate::nurbs::LaneRefusals::new())
}

pub(in crate::families) fn targeted_geometry_graph_from_frames(
    bytes: &[u8],
    frames: &[ObjectFrame],
    refusal: &mut crate::nurbs::LaneRefusals,
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
    parse_from_records(bytes, &records, frames, false, refusal)
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
    let carrier_id = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let source_id = wire::tokens::object_ref(&record.payload, &mut position, true)?;
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
        .map(|_| wire::tokens::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let &[terminal_control] = record.payload.get(position..)? else {
        return None;
    };
    let terminal_control = B5EdgeTerminalControl::from_byte(terminal_control)?;
    Some(B5Edge {
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
                        .lanes
                        .into_iter()
                        .map(|lane| {
                            lift_parameter_incidence(lane.curve, lane.parameter, geometry)
                                .map(coordinates)
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
    parameter: FiniteReal,
    geometry: &B5PcurveContext<'_>,
) -> Option<FinitePoint3> {
    if let Some(pcurve) = geometry.pcurves.get(&pcurve_id) {
        let domain = pcurve_parameter_domain(pcurve)?;
        (parameter >= domain[0] && parameter <= domain[1]).then_some(())?;
        let uv = evaluate_pcurve(pcurve, parameter.get())?;
        return lift_pcurve_endpoints(
            geometry.surfaces.get(&pcurve.surface)?,
            geometry.profiles,
            &[uv, uv],
        )
        .map(|[point, _]| point);
    }
    let opaque = geometry.opaque_pcurves.get(&pcurve_id)?;
    sphere_great_circle_point(
        opaque.sphere_great_circle.as_ref()?,
        geometry.surfaces.get(&opaque.surface)?,
        parameter,
    )
}

fn parse_vertex_incidence_link(record: &B5Record) -> Option<B5VertexIncidenceLink> {
    (record.class == 0x5d && record.payload.first() == Some(&0x81)).then_some(())?;
    let mut position = 1;
    let incidence = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let &[terminal_control] = record.payload.get(position..)? else {
        return None;
    };
    let terminal_control = B5VertexIncidenceControl::from_byte(terminal_control)?;
    Some(B5VertexIncidenceLink {
        object_id: record.object_id,
        incidence,
        terminal_control,
    })
}

fn counted_references(record: &B5Record, class: u8) -> Option<Vec<u32>> {
    (record.class == class).then_some(())?;
    let (references, position) = wire::tokens::counted_refs(&record.payload, true)?;
    (position == record.payload.len()).then_some(references)
}

fn parameter_incidence(record: &B5Record) -> Option<B5ParameterIncidence> {
    (record.class == 0x06).then_some(())?;
    let count = usize::from(record.payload.first()?.checked_sub(0x80)?);
    let mut position = 1;
    let references = (0..count)
        .map(|_| wire::tokens::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?;
    (record.payload.get(position) == Some(&(0x80u8.checked_add(u8::try_from(count).ok()?)?)))
        .then_some(())?;
    position += 1;
    let mut parameters = Vec::with_capacity(count);
    let mut controls = Vec::with_capacity(count);
    for _ in 0..count {
        parameters.push(f64_le(&record.payload, position)?);
        position += 8;
        controls.push(wire::tokens::compact_uint(&record.payload, &mut position)?);
    }
    (position == record.payload.len()).then_some(B5ParameterIncidence {
        object_id: record.object_id,
        lanes: references
            .into_iter()
            .zip(parameters)
            .zip(controls)
            .map(|((curve, parameter), control)| B5IncidenceLane {
                curve,
                parameter,
                control,
            })
            .collect(),
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
                    .is_some_and(|incidence| {
                        incidence.lanes.iter().any(|lane| lane.curve == pcurve)
                    })
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

pub(super) fn evaluate_pcurve(pcurve: &B5Pcurve, parameter: f64) -> Option<[f64; 2]> {
    let knots = pcurve_nurbs_knots(pcurve)?
        .into_iter()
        .map(FiniteReal::get)
        .collect::<Vec<_>>();
    let control_points: Vec<Point2> = pcurve
        .control_points
        .iter()
        .map(|point| Point2::new(point[0], point[1]))
        .collect();
    let weights = pcurve.weights.as_ref().map(|weights| {
        weights
            .iter()
            .copied()
            .map(PositiveReal::get)
            .collect::<Vec<_>>()
    });
    let point = nurbs_pcurve_uv(
        pcurve.degree,
        &knots,
        &control_points,
        weights.as_deref(),
        parameter,
    )?;
    Some([point.u, point.v])
}

fn pcurve_knots(pcurve: &B5Pcurve) -> Option<Vec<FiniteReal>> {
    let mut knots = Vec::new();
    for (&knot, &multiplicity) in pcurve.distinct_knots.iter().zip(&pcurve.multiplicities) {
        knots.extend(std::iter::repeat_n(
            knot,
            usize::try_from(multiplicity).ok()?,
        ));
    }
    Some(knots)
}

/// Return the knot vector in the pcurve's occurrence coordinate system. A
/// translated knot is admitted finite, since the translation can overflow.
pub(super) fn pcurve_nurbs_knots(pcurve: &B5Pcurve) -> Option<Vec<FiniteReal>> {
    let knots = pcurve_knots(pcurve)?;
    match pcurve.parameterization {
        B5PcurveParameterization::Native => Some(knots),
        B5PcurveParameterization::Translated { native_origin } => knots
            .into_iter()
            .map(|knot| FiniteReal::new(knot.get() - native_origin.get()))
            .collect(),
    }
}

pub(super) fn pcurve_parameter_domain(pcurve: &B5Pcurve) -> Option<[FiniteReal; 2]> {
    let knots = pcurve_nurbs_knots(pcurve)?;
    let degree = usize::try_from(pcurve.degree).ok()?;
    let spline_domain = [
        *knots.get(degree)?,
        *knots
            .len()
            .checked_sub(degree + 1)
            .and_then(|index| knots.get(index))?,
    ];
    if spline_domain[0] >= spline_domain[1] {
        return None;
    }
    match pcurve.parameter_range {
        Some(range) => bounded_occurrence_range(range, spline_domain),
        None => Some(spline_domain),
    }
}

/// Clamp an occurrence range to an increasing native domain.
pub(super) fn bounded_occurrence_range(
    parameters: [FiniteReal; 2],
    domain: [FiniteReal; 2],
) -> Option<[FiniteReal; 2]> {
    const RELATIVE_PARAMETER_TOLERANCE: f64 = EPS_B5_GRAPH_DEGENERATE;

    let [lower, upper] = domain;
    let domain_span = upper.get() - lower.get();
    if !domain_span.is_finite() || domain_span <= 0.0 || parameters[0] == parameters[1] {
        return None;
    }
    let tolerance = RELATIVE_PARAMETER_TOLERANCE * domain_span;
    if parameters.iter().any(|parameter| {
        parameter.get() < lower.get() - tolerance || parameter.get() > upper.get() + tolerance
    }) {
        return None;
    }
    Some(parameters.map(|parameter| {
        if parameter < lower {
            lower
        } else if parameter > upper {
            upper
        } else {
            parameter
        }
    }))
}

struct BoundNativeVertices {
    edges: BTreeMap<u32, [B5VertexRef; 2]>,
    vertices: Vec<B5LogicalVertex>,
    tolerances: BTreeMap<usize, PositiveReal>,
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
    // Native vertex identity fixes topology even when incident lifted endpoints
    // are separated. Keep the first deterministic finite lift as the logical
    // coordinate; the pass below records every separation in vertex tolerance.
    for loop_ in loops.values() {
        for member in &loop_.members {
            let (Some(vertices), Some(lifted)) = (
                native_edges.get(&member.edge),
                pcurve_endpoints(member.pcurve, member.edge, geometry),
            ) else {
                continue;
            };
            for lane in 0..2 {
                logical_coordinates
                    .entry(vertices[lane])
                    .or_insert(coordinates(lifted[lane]));
            }
        }
    }
    let mut ranked: Vec<_> = logical_coordinates.into_iter().collect();
    ranked.sort_unstable_by_key(|(vertex, _)| *vertex);
    let logical_vertex_indices: HashMap<u32, B5VertexRef> = ranked
        .iter()
        .enumerate()
        .map(|(rank, (vertex, _))| (*vertex, B5VertexRef::Logical(rank)))
        .collect();
    let logical_vertices: Vec<B5LogicalVertex> = ranked
        .into_iter()
        .map(|(object_id, point)| B5LogicalVertex { object_id, point })
        .collect();
    let mut edge_vertices = geometric_edges
        .iter()
        .map(|(&edge, vertices)| (edge, vertices.map(B5VertexRef::Raw)))
        .collect::<BTreeMap<_, _>>();
    for (&edge, vertices) in native_edges {
        if let (Some(&start), Some(&end)) = (
            logical_vertex_indices.get(&vertices[0]),
            logical_vertex_indices.get(&vertices[1]),
        ) {
            edge_vertices.insert(edge, [start, end]);
        }
    }
    let mut tolerances = BTreeMap::<usize, PositiveReal>::new();
    for loop_ in loops.values() {
        for member in &loop_.members {
            let Some(lifted) = pcurve_endpoints(member.pcurve, member.edge, geometry) else {
                continue;
            };
            let lifted = lifted.map(coordinates);
            let Some(&loci) = edge_vertices.get(&member.edge) else {
                continue;
            };
            let residuals = [
                (
                    loci[0],
                    distance_squared(
                        vertex_coordinate(points, &logical_vertices, loci[0]),
                        lifted[0],
                    )
                    .sqrt(),
                ),
                (
                    loci[1],
                    distance_squared(
                        vertex_coordinate(points, &logical_vertices, loci[1]),
                        lifted[1],
                    )
                    .sqrt(),
                ),
            ];
            for (locus, residual) in residuals {
                if residual <= POINT_TOLERANCE {
                    continue;
                }
                let Some(candidate) = PositiveReal::new(residual + EPS_B5_GRAPH_GEOMETRY) else {
                    continue;
                };
                tolerances
                    .entry(locus.combined_index(points.len()))
                    .and_modify(|tolerance| {
                        if candidate > *tolerance {
                            *tolerance = candidate;
                        }
                    })
                    .or_insert(candidate);
            }
        }
    }
    BoundNativeVertices {
        edges: edge_vertices,
        vertices: logical_vertices,
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

fn vertex_coordinate(
    points: &[[f64; 3]],
    logical_vertices: &[B5LogicalVertex],
    vertex: B5VertexRef,
) -> [f64; 3] {
    match vertex {
        B5VertexRef::Raw(index) => points[index],
        B5VertexRef::Logical(index) => logical_vertices[index].point,
    }
}

pub(super) fn loop_chain_closes(
    loop_: &B5Loop,
    edge_vertices: &BTreeMap<u32, [B5VertexRef; 2]>,
) -> bool {
    let mut members = loop_.members.iter();
    let Some(first_member) = members.next() else {
        return false;
    };
    let Some(first) = edge_vertices.get(&first_member.edge) else {
        return false;
    };
    let first_reversed = usize::from(first_member.controls[0] == -1);
    let initial = first[first_reversed];
    let mut current = first[1 - first_reversed];
    for member in members {
        let Some(endpoints) = edge_vertices.get(&member.edge) else {
            return false;
        };
        let reversed = usize::from(member.controls[0] == -1);
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
            let direction = read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get);
            let parameter_range = [
                f64_le(&record.payload, 57)?.get(),
                f64_le(&record.payload, 65)?.get(),
            ];
            (direction_is_unit(direction)
                && f64_le(&record.payload, 49)?.get() == 1.0
                && parameter_range[0] < parameter_range[1])
                .then_some(B5Profile::Line {
                    point: read_f64_array::<3>(&record.payload, 1)?.map(FiniteReal::get),
                    direction,
                    parameter_range,
                })
        }
        0x0f => {
            (record.payload.len() == 113 && record.payload.first() == Some(&0x80)).then_some(())?;
            let direction_x = read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get);
            let direction_y = read_f64_array::<3>(&record.payload, 49)?.map(FiniteReal::get);
            let radius = f64_le(&record.payload, 73)?.get();
            let parameter_range = [
                f64_le(&record.payload, 81)?.get(),
                f64_le(&record.payload, 89)?.get(),
            ];
            let chart_origin = f64_le(&record.payload, 105)?.get();
            (radius > 0.0
                && directions_are_unit_and_orthogonal(direction_x, direction_y)
                && periodic_angular_range_is_valid(
                    [parameter_range[0] / radius, parameter_range[1] / radius],
                    [
                        chart_origin / radius,
                        chart_origin / radius + std::f64::consts::TAU,
                    ],
                )
                && f64_le(&record.payload, 97)?.get() == 1.0)
                .then_some(B5Profile::Arc {
                    center: read_f64_array::<3>(&record.payload, 1)?.map(FiniteReal::get),
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
        for member in &loop_.members {
            if conflicts.contains(&member.edge) {
                continue;
            }
            let Some(endpoints) = pcurve_endpoints(member.pcurve, member.edge, geometry) else {
                continue;
            };
            let indices: Option<[usize; 2]> = endpoints
                .map(|endpoint| canonical_point(points, &point_index, coordinates(endpoint)))
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .and_then(|indices| indices.try_into().ok());
            let Some(indices) = indices else {
                continue;
            };
            if let Some(previous) = edges.get(&member.edge) {
                let mut previous_sorted = *previous;
                let mut current_sorted = indices;
                previous_sorted.sort_unstable();
                current_sorted.sort_unstable();
                if previous_sorted != current_sorted {
                    edges.remove(&member.edge);
                    conflicts.insert(member.edge);
                }
            } else {
                edges.insert(member.edge, indices);
            }
        }
    }
    edges
}

fn pcurve_endpoints(
    pcurve_id: u32,
    edge_id: u32,
    geometry: &B5PcurveContext<'_>,
) -> Option<[FinitePoint3; 2]> {
    if let Some(pcurve) = geometry.pcurves.get(&pcurve_id) {
        let parameters = edge_pcurve_parameter_values(
            geometry.edge_parameter_incidences,
            geometry.parameter_incidences,
            edge_id,
            pcurve_id,
        )
        .and_then(|parameters| {
            bounded_occurrence_range(parameters, pcurve_parameter_domain(pcurve)?)
        })
        // A missing or invalid edge incidence selects the complete pcurve
        // knot domain. This is the native object-stream fallback; it is not
        // permission to use an arbitrary control-polygon endpoint.
        .or_else(|| pcurve_parameter_domain(pcurve));
        let Some(parameters) = parameters else {
            return pcurve.lifted_endpoints;
        };
        let uv = [
            evaluate_pcurve(pcurve, parameters[0].get())?,
            evaluate_pcurve(pcurve, parameters[1].get())?,
        ];
        let Some(surface) = geometry.surfaces.get(&pcurve.surface) else {
            return pcurve.lifted_endpoints;
        };
        return lift_pcurve_endpoints(surface, geometry.profiles, &uv).or(pcurve.lifted_endpoints);
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
    .and_then(|parameters| bounded_occurrence_range(parameters, pcurve.u_bounds.finite_endpoints()))
    .unwrap_or(pcurve.u_bounds.finite_endpoints());
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

fn direction_is_unit(direction: [f64; 3]) -> bool {
    (distance_squared(direction, [0.0; 3]) - 1.0).abs() <= EPS_B5_GRAPH_EXACT_GEOMETRY
}

fn directions_are_unit_and_orthogonal(first: [f64; 3], second: [f64; 3]) -> bool {
    let dot = first
        .iter()
        .zip(second)
        .map(|(left, right)| left * right)
        .sum::<f64>();
    direction_is_unit(first)
        && direction_is_unit(second)
        && dot.abs() <= EPS_B5_GRAPH_EXACT_GEOMETRY
}

/// Admit a `b5 03 2b` or `2d` frame: the two transverse directions are unit
/// length, perpendicular, and `direction_x × direction_y` lies within `1e-12`
/// of the axis. The frame holds the stored axis and `direction_x`.
fn right_handed_frame(
    axis: [f64; 3],
    direction_x: [f64; 3],
    direction_y: [f64; 3],
) -> Option<OrthonormalFrame3> {
    OrthonormalFrame3::right_handed_euclidean_1e12(
        UnitVector3::new(Vector3::from(axis))?,
        ExactUnitVector3::new(direction_x)?.into(),
        ExactUnitVector3::new(direction_y)?.into(),
    )
}

/// Admit a `b5 03 27` or `28` frame: the two stored directions are unit
/// length and perpendicular. The frame holds the unit normal of
/// `first × second` and the stored `first` direction.
fn completed_frame(first: [f64; 3], second: [f64; 3]) -> Option<OrthonormalFrame3> {
    OrthonormalFrame3::completing_by_largest_component(
        ExactUnitVector3::new(first)?.into(),
        ExactUnitVector3::new(second)?.into(),
    )
}

/// Admit a `b5 03 29` cone frame: the directions are unit length, the two
/// transverse directions are perpendicular, and `direction_x × direction_y`
/// lies within `2e-12` of the axis or of its reverse. The frame holds the
/// stored axis and `direction_x`.
fn cone_frame(
    axis: [f64; 3],
    direction_x: [f64; 3],
    direction_y: [f64; 3],
) -> Option<OrthonormalFrame3> {
    let axis = UnitVector3::new(Vector3::from(axis))?;
    let direction_x = ExactUnitVector3::new(direction_x)?.into();
    let direction_y = ExactUnitVector3::new(direction_y)?.into();
    OrthonormalFrame3::right_handed_euclidean(axis, direction_x, direction_y).or_else(|| {
        let mut frame =
            OrthonormalFrame3::right_handed_euclidean(axis.reversed(), direction_x, direction_y)?;
        frame.reverse_axis();
        Some(frame)
    })
}

fn parse_surface(record: &B5Record) -> Option<B5Surface> {
    (record.family == 0xb5).then_some(())?;
    match record.class {
        0x27 => {
            (record.payload.len() == 121 && record.payload.first() == Some(&0x80)).then_some(())?;
            let direction_u = read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get);
            let direction_v = read_f64_array::<3>(&record.payload, 49)?.map(FiniteReal::get);
            let u_range = [
                f64_le(&record.payload, 89)?.get(),
                f64_le(&record.payload, 97)?.get(),
            ];
            let v_range = [
                f64_le(&record.payload, 105)?.get(),
                f64_le(&record.payload, 113)?.get(),
            ];
            let frame = completed_frame(direction_u, direction_v)?;
            (f64_le(&record.payload, 73)?.get() == 1.0
                && f64_le(&record.payload, 81)?.get() == 1.0
                && u_range[0] < u_range[1]
                && v_range[0] < v_range[1])
                .then_some(B5Surface::Plane {
                    origin: f64_point(&record.payload, 1)?,
                    frame,
                    direction_v,
                    u_range,
                    v_range,
                })
        }
        0x28 => {
            (record.payload.len() == 137 && record.payload.first() == Some(&0x80)).then_some(())?;
            let stored_u = read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get);
            let stored_v = read_f64_array::<3>(&record.payload, 49)?.map(FiniteReal::get);
            let radius = f64_le(&record.payload, 73)?.get();
            let u_range = [
                f64_le(&record.payload, 81)?.get(),
                f64_le(&record.payload, 89)?.get(),
            ];
            let v_range = [
                f64_le(&record.payload, 97)?.get(),
                f64_le(&record.payload, 105)?.get(),
            ];
            let angular_factor = f64_le(&record.payload, 113)?.get();
            let chart_origin = f64_le(&record.payload, 129)?.get();
            let angular_scale = FiniteReal::new(radius / angular_factor)?;
            let chart_domain = [
                chart_origin,
                chart_origin + std::f64::consts::TAU * angular_scale.get(),
            ];
            chart_domain[1].is_finite().then_some(())?;
            let chart_tolerance = EPS_B5_GRAPH_EXACT_GEOMETRY
                * u_range
                    .into_iter()
                    .chain(chart_domain)
                    .chain([angular_scale.get()])
                    .map(f64::abs)
                    .fold(1.0, f64::max);
            let admitted_radius = PositiveLength::new(radius)?;
            let frame = completed_frame(stored_u, stored_v)?;
            (angular_factor > 0.0
                && f64_le(&record.payload, 121)?.get() == 1.0
                && u_range[0] < u_range[1]
                && u_range[0] >= chart_domain[0] - chart_tolerance
                && u_range[1] <= chart_domain[1] + chart_tolerance
                && v_range[0] < v_range[1])
                .then_some(B5Surface::Cylinder {
                    origin: f64_point(&record.payload, 1)?,
                    frame,
                    radius: admitted_radius,
                    u_range,
                    v_range,
                    angular_scale,
                    chart_origin,
                })
        }
        0x29 => {
            (record.payload.len() == 185 && record.payload.first() == Some(&0x80)).then_some(())?;
            let apex = read_f64_array::<3>(&record.payload, 1)?.map(FiniteReal::get);
            let direction_x = read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get);
            let direction_y = read_f64_array::<3>(&record.payload, 49)?.map(FiniteReal::get);
            let axis = read_f64_array::<3>(&record.payload, 73)?.map(FiniteReal::get);
            let half_angle = f64_le(&record.payload, 97)?;
            let reference_radius = f64_le(&record.payload, 105)?.get();
            let angular_range = [
                f64_le(&record.payload, 113)?.get(),
                f64_le(&record.payload, 121)?.get(),
            ];
            let mut slant_range = [
                f64_le(&record.payload, 129)?.get(),
                f64_le(&record.payload, 137)?.get(),
            ];
            if slant_range[0].abs() <= EPS_B5_GRAPH_EXACT_GEOMETRY {
                slant_range[0] = 0.0;
            }
            let angular_scale = PositiveReal::new(f64_le(&record.payload, 145)?.get())?;
            let angular_domain = [
                f64_le(&record.payload, 169)?.get(),
                f64_le(&record.payload, 177)?.get(),
            ];
            let frame = cone_frame(axis, direction_x, direction_y)?;
            let slant_start = NonNegativeLength::new(slant_range[0])?;
            (0.0 < half_angle.get()
                && half_angle.get() < std::f64::consts::FRAC_PI_2
                && periodic_angular_range_is_valid(angular_range, angular_domain)
                && slant_range[0] < slant_range[1]
                && f64_le(&record.payload, 153)?.get() == 1.0
                && f64_le(&record.payload, 161)?.get() == 0.0)
                .then_some(())?;
            let origin = add(apex, scale(axis, slant_range[0] * half_angle.get().cos()));
            let half_angle_radians = Angle::from_assigned_real(half_angle);
            Some(B5Surface::Cone {
                apex,
                direction_x,
                direction_y,
                axis,
                half_angle: half_angle.get(),
                reference_radius,
                angular_range,
                slant_range,
                angular_scale,
                angular_domain,
                surface: FinitePoint3::new(Point3::from(origin)).map(|origin| {
                    ConeSurface::new(
                        origin,
                        frame,
                        slant_start.scaled_by_sine(half_angle_radians),
                        PositiveReal::ONE,
                        half_angle_radians,
                    )
                }),
            })
        }
        0x2a => {
            (record.payload.len() == 153 && record.payload.first() == Some(&0x80)).then_some(())?;
            let center = f64_point(&record.payload, 1)?;
            let stored_x = read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get);
            let stored_y = read_f64_array::<3>(&record.payload, 49)?.map(FiniteReal::get);
            let stored_axis = read_f64_array::<3>(&record.payload, 73)?.map(FiniteReal::get);
            let radius = f64_le(&record.payload, 97)?.get();
            let [azimuth_lo, azimuth_hi, latitude_lo, latitude_hi, construction_radius, chart_origin] =
                read_f64_array::<6>(&record.payload, 105)?.map(FiniteReal::get);
            let azimuth_range = [azimuth_lo, azimuth_hi];
            let latitude_range = [latitude_lo, latitude_hi];
            let vector_length = |value: [f64; 3]| value[0].hypot(value[1]).hypot(value[2]);
            let direction_x =
                UnitVector3::normalized_by_largest_component(Vector3::from(stored_x))?;
            let direction_y =
                UnitVector3::normalized_by_largest_component(Vector3::from(stored_y))?;
            let axis = UnitVector3::normalized_by_largest_component(Vector3::from(stored_axis))?;
            let expected_chart_angle =
                (azimuth_range[0] + azimuth_range[1]) * 0.5 - std::f64::consts::PI;
            let expected_chart_origin = construction_radius * expected_chart_angle;
            let chart_origin_tolerance =
                2.0 * f64::EPSILON * chart_origin.abs().max(expected_chart_origin.abs()).max(1.0);
            let admitted_radius = PositiveLength::new(radius)?;
            let frame =
                OrthonormalFrame3::right_handed_euclidean_1e12(axis, direction_x, direction_y)?;
            (construction_radius > 0.0
                && sphere_angular_ranges_are_valid(azimuth_range, latitude_range)
                && expected_chart_origin.is_finite()
                && (chart_origin - expected_chart_origin).abs() <= chart_origin_tolerance
                && [stored_x, stored_y, stored_axis].iter().all(|direction| {
                    ((vector_length(*direction) / radius) - 1.0).abs()
                        <= EPS_B5_GRAPH_EXACT_GEOMETRY
                }))
            .then_some(B5Surface::Sphere {
                center,
                frame,
                direction_y: components(&direction_y),
                radius: admitted_radius,
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
            let direction_x = read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get);
            let direction_y = read_f64_array::<3>(&record.payload, 49)?.map(FiniteReal::get);
            let axis = read_f64_array::<3>(&record.payload, 73)?.map(FiniteReal::get);
            let major_radius = f64_le(&record.payload, 97)?.get();
            let minor_radius = f64_le(&record.payload, 105)?.get();
            let major_angular_range = [
                f64_le(&record.payload, 113)?.get(),
                f64_le(&record.payload, 121)?.get(),
            ];
            let major_angular_domain = [
                f64_le(&record.payload, 129)?.get(),
                f64_le(&record.payload, 137)?.get(),
            ];
            let minor_angular_range = [
                f64_le(&record.payload, 145)?.get(),
                f64_le(&record.payload, 153)?.get(),
            ];
            let minor_angular_domain = [
                f64_le(&record.payload, 161)?.get(),
                f64_le(&record.payload, 169)?.get(),
            ];
            let major_scale = PositiveReal::new(f64_le(&record.payload, 177)?.get())?;
            let minor_scale = PositiveReal::new(f64_le(&record.payload, 185)?.get())?;
            let frame = right_handed_frame(axis, direction_x, direction_y)?;
            let admitted_major_radius = PositiveLength::new(major_radius)?;
            let admitted_minor_radius = PositiveLength::new(minor_radius)?;
            (periodic_angular_range_is_valid(major_angular_range, major_angular_domain)
                && periodic_angular_range_is_valid(minor_angular_range, minor_angular_domain))
            .then_some(())?;
            Some(B5Surface::Torus {
                center: f64_point(&record.payload, 1)?,
                frame,
                direction_y,
                major_radius: admitted_major_radius,
                minor_radius: admitted_minor_radius,
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
            let profile_curve = wire::tokens::object_ref(&record.payload, &mut position, true)?;
            (record.payload.len() == position.checked_add(171)?
                && record.payload.first() == Some(&0x81))
            .then_some(())?;
            let angular_range = [
                f64_le(&record.payload, position.checked_add(96)?)?.get(),
                f64_le(&record.payload, position.checked_add(104)?)?.get(),
            ];
            let profile_range = [
                f64_le(&record.payload, position.checked_add(112)?)?.get(),
                f64_le(&record.payload, position.checked_add(120)?)?.get(),
            ];
            let angular_scale =
                PositiveReal::new(f64_le(&record.payload, position.checked_add(130)?)?.get())?;
            let angular_half_turn = f64_le(&record.payload, position.checked_add(163)?)?.get();
            let reference_x = read_f64_array::<3>(&record.payload, position.checked_add(24)?)?
                .map(FiniteReal::get);
            let reference_y = read_f64_array::<3>(&record.payload, position.checked_add(48)?)?
                .map(FiniteReal::get);
            let axis_direction = read_f64_array::<3>(&record.payload, position.checked_add(72)?)?
                .map(FiniteReal::get);
            (record.payload.get(position + 128..position + 130) == Some(&[0x05, 0x05]))
                .then_some(())?;
            let frame = right_handed_frame(axis_direction, reference_x, reference_y)?;
            (angular_range[0] < angular_range[1]
                && angular_range[0] >= 0.0
                && angular_range[1] <= 2.0 * angular_half_turn
                && profile_range[0] < profile_range[1]
                && f64_le(&record.payload, position + 138)?.get() == 1.0
                && f64_le(&record.payload, position + 146)?.get() == 1.0
                && f64_le(&record.payload, position + 154)?.get() == 0.0
                && record.payload.get(position + 162) == Some(&0x01)
                && angular_half_turn.to_bits()
                    == (std::f64::consts::PI * angular_scale.get()).to_bits())
            .then_some(B5Surface::Revolution {
                profile_curve,
                axis_origin: f64_point(&record.payload, position)?,
                axis_direction: *frame.axis(),
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
    let target = wire::tokens::object_ref(&record.payload, &mut position, true)?;
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
    let carrier_surface = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let source_surface = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let distance = f64_le(&record.payload, position)?;
    position += 8;
    let carrier_kind = B5OffsetCarrierKind::from_byte(*record.payload.get(position)?)?;
    position += 1;
    let [u0, u1, v0, v1] = read_f64_array::<4>(&record.payload, position)?.map(FiniteReal::get);
    position += 32;
    (position == record.payload.len()).then_some(())?;
    Some(B5OffsetSurface {
        object_id: record.object_id,
        carrier_surface,
        source_surface,
        distance,
        carrier_kind,
        parameter_bounds: [
            IncreasingParameterInterval::new([u0, u1])?,
            IncreasingParameterInterval::new([v0, v1])?,
        ],
    })
}

fn parse_offset_surface(
    record: &B5Record,
    surfaces: &BTreeMap<u32, B5Surface>,
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
    records: &HashMap<u32, &B5Record>,
) -> Option<B5OffsetSurface> {
    let fields = parse_offset_surface_fields(record)?;
    let B5OffsetSurface {
        carrier_surface,
        source_surface,
        distance,
        carrier_kind,
        parameter_bounds: [u_bounds, v_bounds],
        ..
    } = fields;
    if carrier_kind == B5OffsetCarrierKind::Extrusion {
        if let (Some(source), Some(carrier)) = (
            extrusion_surfaces.get(&source_surface),
            extrusion_surfaces.get(&carrier_surface),
        ) {
            return extrusion_offset_construction_agrees(
                source,
                carrier,
                distance,
                [u_bounds, v_bounds],
            )
            .then_some(fields);
        }
    }
    let expected_kind = match surfaces.get(&carrier_surface) {
        Some(carrier @ B5Surface::Plane { .. }) => analytic_offset_magnitude_agrees(
            carrier,
            surfaces.get(&source_surface)?,
            distance.get(),
        )
        .then_some(B5OffsetCarrierKind::Plane)?,
        Some(carrier @ B5Surface::Cylinder { .. }) => analytic_offset_magnitude_agrees(
            carrier,
            surfaces.get(&source_surface)?,
            distance.get(),
        )
        .then_some(B5OffsetCarrierKind::Cylinder)?,
        Some(carrier @ B5Surface::Sphere { .. }) => analytic_offset_magnitude_agrees(
            carrier,
            surfaces.get(&source_surface)?,
            distance.get(),
        )
        .then_some(B5OffsetCarrierKind::Sphere)?,
        Some(carrier @ B5Surface::Torus { .. }) => analytic_offset_magnitude_agrees(
            carrier,
            surfaces.get(&source_surface)?,
            distance.get(),
        )
        .then_some(B5OffsetCarrierKind::Torus)?,
        Some(B5Surface::RollingBall { .. }) => B5OffsetCarrierKind::RollingBall,
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
                    || carrier.u_bounds != v_bounds
                    || carrier.v_bounds != Some(u_bounds)
                {
                    return None;
                }
                return (carrier_kind == B5OffsetCarrierKind::Extrusion).then_some(fields);
            }
            let cache = parse_offset_cache(records.get(&carrier_surface)?)?;
            let source = surfaces.get(&source_surface)?;
            let cached_source = surfaces.get(&cache.source_surface)?;
            let [u0, u1] = u_bounds.endpoints();
            let [v0, v1] = v_bounds.endpoints();
            if source != cached_source
                || distance.get().to_bits() != cache.distance.to_bits()
                || [u0, v0, u1, v1]
                    .into_iter()
                    .zip(cache.interleaved_bounds)
                    .any(|(left, right)| left.to_bits() != right.to_bits())
            {
                return None;
            }
            B5OffsetCarrierKind::Cache
        }
    };
    (carrier_kind == expected_kind).then_some(fields)
}

fn extrusion_offset_construction_agrees(
    source: &B5ExtrusionSurface,
    carrier: &B5ExtrusionSurface,
    distance: FiniteReal,
    [u_bounds, v_bounds]: [IncreasingParameterInterval; 2],
) -> bool {
    if carrier.direction != source.direction {
        return false;
    }
    let B5ExtrusionDirectrix::Offset {
        source: offset_source,
        source_parameter_range,
        distance: curve_distance,
        direction,
        parameter_range,
        ..
    } = &carrier.directrix
    else {
        return carrier.parameter_bounds == [v_bounds, u_bounds];
    };
    offset_source.object_id() == source.directrix.object_id()
        && offset_source.supports().first().is_some_and(|support| {
            source_parameter_range
                .iter()
                .copied()
                .zip(support.2)
                .all(|(left, right)| left.to_bits() == right.get().to_bits())
        })
        && curve_distance.get().to_bits() == distance.get().to_bits()
        && *direction == source.direction
        && carrier.parameter_bounds[0]
            .endpoints()
            .into_iter()
            .zip(v_bounds.endpoints())
            .all(|(left, right)| left.to_bits() == right.to_bits())
        && parameter_range
            .iter()
            .copied()
            .zip(u_bounds.endpoints())
            .all(|(left, right)| left.to_bits() == right.to_bits())
}

fn analytic_offset_magnitude_agrees(
    carrier: &B5Surface,
    source: &B5Surface,
    distance: f64,
) -> bool {
    const RELATIVE_TOLERANCE: f64 = EPS_B5_GRAPH_DEGENERATE;
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
                frame: carrier_frame,
                ..
            },
            B5Surface::Plane {
                origin: source_origin,
                frame: source_frame,
                ..
            },
        ) => {
            let source_normal = components(source_frame.axis());
            let delta = difference(coordinates(*carrier_origin), coordinates(*source_origin));
            parallel(components(carrier_frame.axis()), source_normal)
                && projected_distance_close(
                    dot(delta, source_normal).abs(),
                    distance.abs(),
                    length(delta),
                )
        }
        (
            B5Surface::Cylinder {
                origin: carrier_origin,
                frame: carrier_frame,
                radius: carrier_radius,
                ..
            },
            B5Surface::Cylinder {
                origin: source_origin,
                frame: source_frame,
                radius: source_radius,
                ..
            },
        ) => {
            let source_axis = components(source_frame.axis());
            let delta = difference(coordinates(*carrier_origin), coordinates(*source_origin));
            parallel(components(carrier_frame.axis()), source_axis)
                && collinear(delta, source_axis)
                && relative_close(
                    (carrier_radius.get() - source_radius.get()).abs(),
                    distance.abs(),
                )
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
            let (carrier_radius, source_radius) = (carrier_radius.get(), source_radius.get());
            let geometric_scale = carrier_radius
                .abs()
                .max(source_radius.abs())
                .max(distance.abs());
            same_point(
                coordinates(*carrier_center),
                coordinates(*source_center),
                geometric_scale,
            ) && relative_close((carrier_radius - source_radius).abs(), distance.abs())
        }
        (
            B5Surface::Torus {
                center: carrier_center,
                frame: carrier_frame,
                major_radius: carrier_major,
                minor_radius: carrier_minor,
                ..
            },
            B5Surface::Torus {
                center: source_center,
                frame: source_frame,
                major_radius: source_major,
                minor_radius: source_minor,
                ..
            },
        ) => {
            let (carrier_major, source_major) = (carrier_major.get(), source_major.get());
            let (carrier_minor, source_minor) = (carrier_minor.get(), source_minor.get());
            let geometric_scale = carrier_major
                .abs()
                .max(source_major.abs())
                .max(carrier_minor.abs())
                .max(source_minor.abs())
                .max(distance.abs());
            same_point(
                coordinates(*carrier_center),
                coordinates(*source_center),
                geometric_scale,
            ) && parallel(
                components(carrier_frame.axis()),
                components(source_frame.axis()),
            ) && relative_close(carrier_major, source_major)
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
    parameter_range: [FiniteReal; 2],
    class_21_suffix_scalar: Option<f64>,
    distinct_knots: Vec<FiniteReal>,
}

fn parse_offset_cache(record: &B5Record) -> Option<B5OffsetCache> {
    (record.family == 0xb5 && record.class == 0x31 && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let source_surface = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let [distance, u0, v0, u1, v1] =
        read_f64_array::<5>(&record.payload, position)?.map(FiniteReal::get);
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
    let Some(active_bounds) = carrier.v_bounds else {
        let directrix = parse_extrusion_directrix(
            records.get(&carrier.directrix_id)?,
            records,
            object_stream_pcurves,
        )?;
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
    };
    let active = active_bounds.endpoints();
    let terminal_span_chart = matches!(carrier.controls, [0x05, 0x15 | 0x19]);
    let mut directrix = if terminal_span_chart {
        terminal_span_directrix(
            carrier.directrix_id,
            active,
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
    let directrix_contains_active = active.into_iter().all(|value| {
        cadmpeg_ir::math::parameter_in_domain(
            value,
            directrix.parameter_range(),
            64.0 * f64::EPSILON,
        )
    });
    let translated_chart = carrier.controls == [0x05, 0x11];
    if translated_chart {
        translated_directrix_span_count(
            &directrix,
            active,
            carrier.controls,
            object_stream_pcurves,
        )?;
        if !directrix.reorigin_parameter_range(active) {
            return None;
        }
    } else if !directrix_contains_active {
        let source_span = directrix.parameter_range()[1] - directrix.parameter_range()[0];
        let active_span = active[1] - active[0];
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
        if !directrix.reorigin_parameter_range(active) {
            return None;
        }
    }
    Some(B5ExtrusionSurface {
        object_id: record.object_id,
        direction: carrier.direction,
        parameter_bounds: [carrier.u_bounds, active_bounds],
        directrix,
    })
}

fn contextual_offset_extrusion_bounds(
    carrier_surface: u32,
    carrier: &B5ExtrusionCarrier,
    directrix: &B5ExtrusionDirectrix,
    offset_constructions: &[B5OffsetSurface],
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
) -> Option<[IncreasingParameterInterval; 2]> {
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
        construction.carrier_surface == carrier_surface
            && construction.carrier_kind == B5OffsetCarrierKind::Extrusion
    }) {
        let source_extrusion = extrusion_surfaces.get(&construction.source_surface)?;
        let bounds = [
            construction.parameter_bounds[1],
            construction.parameter_bounds[0],
        ];
        if source.object_id() != source_extrusion.directrix.object_id()
            || carrier.direction != source_extrusion.direction
            || *direction != source_extrusion.direction
            || distance.get().to_bits() != construction.distance.get().to_bits()
            || carrier.u_bounds != bounds[0]
            || *parameter_range != bounds[1].endpoints()
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
    let target_span_count = wire::tokens::compact_uint(&controls[..1], &mut first_position)?;
    let mut second_position = 0;
    let source_span_count = usize::try_from(wire::tokens::compact_uint(
        &controls[1..],
        &mut second_position,
    )?)
    .ok()?;
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
        .all(|(left, right)| left.get().to_bits() == right.get().to_bits())
        .then_some(())?;
    parameter_spans_agree(
        source_range[1].get() - source_range[0].get(),
        active[1] - active[0],
    )
    .then_some(B5ExtrusionDirectrix::SurfaceCurve {
        object_id: directrix_id,
        support: (pcurve.surface, directrix_id, source_range),
        parameter_range: active,
    })
}

fn translated_directrix_span_count(
    directrix: &B5ExtrusionDirectrix,
    active: [f64; 2],
    controls: [u8; 2],
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Option<usize> {
    let mut first_position = 0;
    let target_span_count = wire::tokens::compact_uint(&controls[..1], &mut first_position)?;
    let mut second_position = 0;
    let source_span_count = usize::try_from(wire::tokens::compact_uint(
        &controls[1..],
        &mut second_position,
    )?)
    .ok()?;
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
        .position(|knot| knot.get().to_bits() == source[0].to_bits())?;
    let end = pcurve
        .distinct_knots
        .iter()
        .position(|knot| knot.get().to_bits() == source[1].to_bits())?;
    (end.checked_sub(start)? == source_span_count).then_some(())?;
    pcurve.distinct_knots[start..=end]
        .windows(2)
        .all(|knots| parameter_spans_agree(knots[1].get() - knots[0].get(), suffix_span))
        .then_some(source_span_count)
}

fn parameter_spans_agree(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= 64.0 * f64::EPSILON * scale
}

struct B5ExtrusionCarrier {
    directrix_id: u32,
    direction: UnitVector3,
    u_bounds: IncreasingParameterInterval,
    /// The increasing V interval, absent on a contextual offset chart, which
    /// stores its V bounds in decreasing order and takes its chart from the
    /// offset construction.
    v_bounds: Option<IncreasingParameterInterval>,
    controls: [u8; 2],
}

fn extrusion_carrier(record: &B5Record) -> Option<B5ExtrusionCarrier> {
    (record.family == 0xb5 && record.class == 0x2c && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let directrix_id = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let values = read_f64_array::<9>(&record.payload, position)?;
    position += 72;
    let controls: [u8; 2] = record.payload.get(position..)?.try_into().ok()?;
    let values = values.map(FiniteReal::get);
    let contextual_offset_chart = matches!(controls, [0x01, 0x09 | 0x15]);
    ((matches!(controls, [0x05, 0x05 | 0x11 | 0x15 | 0x19])
        || contextual_offset_chart
        || (matches!(controls[0], 0x01 | 0x05) && controls[1] == 0x29))
        && values[5].to_bits() == 1.0f64.to_bits()
        && values[6].to_bits() == 0.0f64.to_bits())
    .then_some(())?;
    let direction = ExactUnitVector3::new([values[0], values[1], values[2]])?.into();
    let u_bounds = IncreasingParameterInterval::new([values[3], values[4]])?;
    let v_bounds = if contextual_offset_chart {
        (values[7] > values[8]).then_some(None)?
    } else {
        Some(IncreasingParameterInterval::new([values[7], values[8]])?)
    };
    Some(B5ExtrusionCarrier {
        directrix_id,
        direction,
        u_bounds,
        v_bounds,
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
    let wrapper_id = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let second_pcurve = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let tail = record.payload.len().checked_sub(25)?;
    (position < tail).then_some(())?;
    let parameter_range = read_f64_array::<2>(&record.payload, tail)?.map(FiniteReal::get);
    let cache_fit_tolerance = PositiveReal::new(f64_le(&record.payload, tail + 16)?.get())?;
    if record.payload.get(tail + 24) != Some(&0x01) || parameter_range[0] >= parameter_range[1] {
        return None;
    }
    let wrapper = records.get(&wrapper_id)?;
    (wrapper.family == 0xb5 && wrapper.class == 0x24 && wrapper.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut wrapper_position = 1;
    let first_pcurve = wire::tokens::object_ref(&wrapper.payload, &mut wrapper_position, true)?;
    if wrapper.payload.get(wrapper_position..wrapper_position + 2) != Some(&[0x81, 0x01]) {
        return None;
    }
    wrapper_position += 2;
    let wrapper_values =
        read_f64_array::<3>(&wrapper.payload, wrapper_position)?.map(FiniteReal::get);
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
    let pcurve = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    if record.payload.get(position..position + 2) != Some(&[0x81, 0x01]) {
        return None;
    }
    position += 2;
    let [start, end, zero] = read_f64_array::<3>(&record.payload, position)?;
    position += 24;
    if record.payload.get(position..) != Some(&[0x01])
        || start >= end
        || zero.get().to_bits() != 0.0f64.to_bits()
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
    parameter_range
        .into_iter()
        .all(|value| {
            cadmpeg_ir::math::parameter_in_domain(
                value.get(),
                pcurve_range.map(FiniteReal::get),
                64.0 * f64::EPSILON,
            )
        })
        .then_some(B5ExtrusionDirectrix::SurfaceCurve {
            object_id: record.object_id,
            support: (surface, pcurve, parameter_range),
            parameter_range: parameter_range.map(FiniteReal::get),
        })
}

fn parse_offset_curve_directrix(
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Option<B5ExtrusionDirectrix> {
    (record.family == 0xb5 && record.class == 0x14 && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let source_id = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let source_parameter_range =
        read_f64_array::<2>(&record.payload, position)?.map(FiniteReal::get);
    position += 16;
    if record.payload.get(position) != Some(&0x05) {
        return None;
    }
    position += 1;
    let [distance, x, y, z, start, end] = read_f64_array::<6>(&record.payload, position)?;
    let [x, y, z, start, end] = [x, y, z, start, end].map(FiniteReal::get);
    position += 48;
    let source_record = records.get(&source_id)?;
    if !((source_record.family == 0xb5 && source_record.class == 0x24)
        || (source_record.family == 0xa8 && source_record.class == 0x25))
    {
        return None;
    }
    let source = parse_extrusion_directrix(source_record, records, object_stream_pcurves)?;
    let direction = ExactUnitVector3::new([x, y, z])?.into();
    if position != record.payload.len()
        || distance.get() == 0.0
        || source_parameter_range[0] >= source_parameter_range[1]
        || start >= end
        || !source.supports().iter().any(|support| {
            support
                .2
                .into_iter()
                .zip(source_parameter_range)
                .all(|(left, right)| left.get().to_bits() == right.to_bits())
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
    wire::tokens::object_ref(&record.payload, &mut position, true)
}

fn analytic_pcurve_range(record: &B5Record) -> Option<[FiniteReal; 2]> {
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
    (record.family == 0xb5 && record.payload.first() == Some(&0x85)).then_some(())?;
    let mut position = 1;
    let references: [u32; 5] = (0..5)
        .map(|_| wire::tokens::object_ref(&record.payload, &mut position, true))
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
            let construction_radius = f64_le(&record.payload, position + 2)?.get();
            let zero = f64_le(&record.payload, position + 12)?.get();
            (construction_radius > 0.0 && zero == 0.0).then_some(())?;
            B5SupportedSurfaceParameters::Radius {
                controls,
                construction_radius,
            }
        }
        0x3b => {
            let controls = record.payload[position..position + 6].try_into().ok()?;
            let scalars = [
                f64_le(&record.payload, position + 6)?.get(),
                f64_le(&record.payload, position + 14)?.get(),
            ];
            scalars.iter().all(|scalar| *scalar > 0.0).then_some(())?;
            B5SupportedSurfaceParameters::ScalarPair { controls, scalars }
        }
        _ => return None,
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
    let relative_close = |left: f64, right: f64| {
        (left - right).abs() <= EPS_B5_GRAPH_EXACT_GEOMETRY * left.abs().max(right.abs())
    };
    match (parameters, carrier) {
        (
            B5SupportedSurfaceParameters::Radius {
                construction_radius,
                ..
            },
            B5Surface::Cylinder { radius, .. },
        ) => relative_close(*construction_radius, radius.get()),
        (
            B5SupportedSurfaceParameters::Radius {
                construction_radius,
                ..
            },
            B5Surface::Torus { minor_radius, .. },
        ) => relative_close(*construction_radius, minor_radius.get()),
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
                && wire::tokens::object_ref(&pcurve.payload, &mut position, true)
                    == Some(support_id)
        })
}

fn lift_pcurve_endpoints(
    surface: &B5Surface,
    profiles: &BTreeMap<u32, B5Profile>,
    control_points: &[[f64; 2]],
) -> Option<[FinitePoint3; 2]> {
    let endpoints = [*control_points.first()?, *control_points.last()?];
    let lifted = match surface {
        B5Surface::UnresolvedNurbs { .. }
        | B5Surface::Unknown { .. }
        | B5Surface::RollingBall { .. } => None,
        B5Surface::Plane {
            origin,
            frame,
            direction_v,
            ..
        } => {
            let (origin, direction_u) = (coordinates(*origin), components(frame.reference()));
            Some(
                endpoints
                    .map(|[u, v]| add(origin, add(scale(direction_u, u), scale(*direction_v, v)))),
            )
        }
        B5Surface::Cylinder {
            origin,
            frame,
            radius,
            angular_scale,
            ..
        } => {
            let (origin, axis, reference_x) = (
                coordinates(*origin),
                components(frame.axis()),
                components(frame.reference()),
            );
            let reference_y = cross(axis, reference_x);
            Some(endpoints.map(|[u, v]| {
                let angle = u / angular_scale.get();
                add(
                    origin,
                    add(
                        scale(
                            add(
                                scale(reference_x, angle.cos()),
                                scale(reference_y, angle.sin()),
                            ),
                            radius.get(),
                        ),
                        scale(axis, v),
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
            let angle = u / angular_scale.get();
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
            frame,
            direction_y,
            major_radius,
            minor_radius,
            major_scale,
            minor_scale,
            ..
        } => {
            let (center, axis, direction_x) = (
                coordinates(*center),
                components(frame.axis()),
                components(frame.reference()),
            );
            let (major_radius, minor_radius) = (major_radius.get(), minor_radius.get());
            Some(endpoints.map(|[u, v]| {
                let major_angle = u / major_scale.get();
                let minor_angle = v / minor_scale.get();
                let radial = add(
                    scale(direction_x, major_angle.cos()),
                    scale(*direction_y, major_angle.sin()),
                );
                add(
                    center,
                    add(
                        scale(radial, major_radius + minor_radius * minor_angle.cos()),
                        scale(axis, minor_radius * minor_angle.sin()),
                    ),
                )
            }))
        }
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
                rotate_about_axis(
                    point,
                    coordinates(*axis_origin),
                    components(axis_direction),
                    v / angular_scale.get(),
                )
            }))
        }
        B5Surface::Nurbs(surface) => Some([
            evaluate_nurbs(surface, endpoints[0][0], endpoints[0][1])?,
            evaluate_nurbs(surface, endpoints[1][0], endpoints[1][1])?,
        ]),
    };
    let [start, end] = lifted?;
    Some([
        FinitePoint3::new(Point3::from(start))?,
        FinitePoint3::new(Point3::from(end))?,
    ])
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

fn parse_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.family != 0xb5 || record.class != 0x21 || record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    if record.payload.get(position) != Some(&0x01) {
        return None;
    }
    position += 1;
    let degree = wire::tokens::compact_uint(&record.payload, &mut position)?;
    if !matches!(degree, 1 | 2 | 5)
        || record.payload.get(position..position + 2) != Some(&[0x01, 0x01])
    {
        return None;
    }
    position += 2;
    let knot_count =
        usize::try_from(wire::tokens::compact_uint(&record.payload, &mut position)?).ok()?;
    if knot_count != 2 || record.payload.get(position) != Some(&0x01) {
        return None;
    }
    position += 1;
    let mut view = View::over_retained(&record.payload);
    view.seek(position)?;
    let mut distinct_knots = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        distinct_knots.push(FiniteReal::new(view.f64_le()?)?);
    }
    position = view.position();
    if !distinct_knots.windows(2).all(|pair| pair[0] < pair[1]) {
        return None;
    }
    let mut multiplicities = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        multiplicities.push(wire::tokens::compact_uint(&record.payload, &mut position)?);
    }
    let endpoint_multiplicity = degree + 1;
    if multiplicities != [endpoint_multiplicity; 2] {
        return None;
    }
    let pole_count = endpoint_multiplicity;
    view.seek(position)?;
    let mut control_points = Vec::with_capacity(usize::try_from(pole_count).ok()?);
    for _ in 0..pole_count {
        let u = view.f64_le()?;
        let v = view.f64_le()?;
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        control_points.push([u, v]);
    }
    position = view.position();
    let tail = record.payload.get(position..)?;
    let suffix_scalar = f64_le(tail, 10)?.get();
    let native_origin = *distinct_knots.first()?;
    let native_span = distinct_knots[1].get() - native_origin.get();
    if tail.len() != 36
        || tail.get(..2) != Some(&[0x05, 0x05])
        || f64_le(tail, 2)?.get() != 0.0
        || suffix_scalar <= 0.0
        || suffix_scalar.to_bits() != native_span.to_bits()
        || f64_le(tail, 18)?.get() != 1.0
        || f64_le(tail, 26)?.get() != 0.0
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
        parameterization: B5PcurveParameterization::Translated { native_origin },
        class_21_suffix_scalar: Some(suffix_scalar),
        lifted_endpoints: None,
    })
}

fn parse_circle_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.family != 0xb5 || record.class != 0x19 || record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    if record.payload.len() != position.checked_add(58)? {
        return None;
    }
    let center = read_f64_array::<2>(&record.payload, position)?.map(FiniteReal::get);
    position += 16;
    if record.payload.get(position..position + 2) != Some(&[0x05, 0x05]) {
        return None;
    }
    position += 2;
    let [radius, start, end, orientation, phase] =
        read_f64_array::<5>(&record.payload, position)?.map(FiniteReal::get);
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
    let surface = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    if record.payload.len() != position.checked_add(74)? {
        return None;
    }
    let center = read_f64_array::<2>(&record.payload, position)?.map(FiniteReal::get);
    position += 16;
    if record.payload.get(position..position + 2) != Some(&[0x05, 0x05]) {
        return None;
    }
    let [diameter_u, diameter_v, conjugate_angle, start, end, orientation, period] =
        read_f64_array::<7>(&record.payload, position + 2)?.map(FiniteReal::get);
    let diameter = diameter_u.hypot(diameter_v);
    let relative_period = period / (std::f64::consts::PI * diameter);
    if diameter <= 0.0
        || start >= end
        || period <= 0.0
        || !matches!(orientation, -1.0 | 1.0)
        || (conjugate_angle - std::f64::consts::FRAC_PI_2).abs() > EPS_B5_GRAPH_EXACT_GEOMETRY
        || !relative_period.is_finite()
        || (relative_period - 1.0).abs() > EPS_B5_GRAPH_EXACT_GEOMETRY
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

#[allow(clippy::too_many_arguments)]
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
    // `ceil` answers zero only for an angular span of exactly zero: an arc that
    // sweeps no angle states no span, which this route refuses as it refuses
    // every other degeneracy.
    let span_count = std::num::NonZeroUsize::new(span_count as usize)?.get();
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
    let distinct_knots = distinct_knots
        .into_iter()
        .map(FiniteReal::new)
        .collect::<Option<Vec<_>>>()?;
    let weights = weights
        .into_iter()
        .map(PositiveReal::new)
        .collect::<Option<Vec<_>>>()?;
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
        degree: 2,
        distinct_knots,
        multiplicities,
        control_points,
        weights: Some(weights),
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    })
}

fn parse_opaque_pcurve(record: &B5Record) -> Option<B5OpaquePcurve> {
    if parse_class_1a_pcurve(record).is_some() {
        return None;
    }
    if record.family != 0xb5 || record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    match record.class {
        0x1a => {
            (record.payload.len() == position.checked_add(74)?).then_some(())?;
            read_f64_array::<2>(&record.payload, position)?;
            position += 16;
            (record.payload.get(position..position + 2) == Some(&[0x05, 0x05])).then_some(())?;
            read_f64_array::<7>(&record.payload, position + 2)?;
        }
        0x1d => {
            (record.payload.len() == position.checked_add(99)?).then_some(())?;
            read_f64_array::<4>(&record.payload, position)?;
            position += 32;
            (record.payload.get(position..position + 2) == Some(&[0x05, 0x81])).then_some(())?;
            read_f64_array::<3>(&record.payload, position + 2)?;
            position += 26;
            (record.payload.get(position) == Some(&0x1d)).then_some(())?;
            read_f64_array::<5>(&record.payload, position + 1)?;
        }
        _ => return None,
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
    wire::tokens::object_ref(&record.payload, &mut position, true)?;
    (record.payload.len() == position.checked_add(99)?).then_some(())?;
    let [u0, u1, v0, v1] = read_f64_array::<4>(&record.payload, position)?;
    position += 32;
    (record.payload.get(position..position + 2) == Some(&[0x05, 0x81])).then_some(())?;
    let [chart_shift, direction, zero0] = read_f64_array::<3>(&record.payload, position + 2)?;
    let (direction, zero0) = (direction.get(), zero0.get());
    position += 26;
    (record.payload.get(position) == Some(&0x1d)).then_some(())?;
    let [chart_scale, slope, reciprocal_scale, phase, zero1] =
        read_f64_array::<5>(&record.payload, position + 1)?;
    let u_bounds = IncreasingParameterInterval::new([u0.get(), u1.get()])?;
    let v_bounds = [v0, v1];
    let chart_scale = PositiveReal::new(chart_scale.get())?;
    let (u0, u1, v0, v1) = (u0.get(), u1.get(), v0.get(), v1.get());
    let (reciprocal_scale, zero1) = (reciprocal_scale.get(), zero1.get());

    let surface_u_bounds = azimuth_range.map(|angle| chart_scale.get() * angle);
    let u_scale = surface_u_bounds
        .into_iter()
        .chain([u0, u1])
        .map(f64::abs)
        .fold(1.0, f64::max);
    let u_tolerance = EPS_B5_GRAPH_EXACT_GEOMETRY * u_scale;
    (direction.abs() == 1.0
        && zero0 == 0.0
        && zero1 == 0.0
        && chart_scale.get() == *sphere_chart_scale
        && reciprocal_scale == -direction / chart_scale.get()
        && u0 >= surface_u_bounds[0] - u_tolerance
        && u1 <= surface_u_bounds[1] + u_tolerance
        && v0 == *chart_origin
        && v1 == chart_origin + std::f64::consts::TAU * chart_scale.get())
    .then_some(B5SphereGreatCirclePcurve {
        u_bounds,
        v_bounds,
        chart_shift,
        chart_scale,
        slope,
        phase,
    })
}

fn sphere_great_circle_point(
    pcurve: &B5SphereGreatCirclePcurve,
    surface: &B5Surface,
    parameter: FiniteReal,
) -> Option<FinitePoint3> {
    let B5Surface::Sphere {
        center,
        frame,
        direction_y,
        radius,
        construction_radius,
        ..
    } = surface
    else {
        return None;
    };
    let (center, direction_x, axis, radius) = (
        coordinates(*center),
        components(frame.reference()),
        components(frame.axis()),
        radius.get(),
    );
    let parameter = parameter.get();
    let chart_scale = pcurve.chart_scale.get();
    if parameter < pcurve.u_bounds.lower()
        || parameter > pcurve.u_bounds.upper()
        || chart_scale != *construction_radius
    {
        return None;
    }
    let azimuth = parameter / chart_scale;
    let phase = pcurve.chart_shift.get() / chart_scale + pcurve.phase.get();
    let latitude = (pcurve.slope.get() * (azimuth - phase).cos()).atan();
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
    FinitePoint3::new(Point3::from(point))
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
    let surface = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let mode = *record.payload.get(position)?;
    position += 1;
    let (start, end, control_points) = match mode {
        0x01 if record.payload.len() == position.checked_add(48)? => {
            let [u, v, du, dv, start, end] = read_f64_array::<6>(&record.payload, position)?;
            let [u, v, du, dv] = [u, v, du, dv].map(FiniteReal::get);
            if du == 0.0 && dv == 0.0 {
                return None;
            }
            (
                start,
                end,
                vec![
                    [u + start.get() * du, v + start.get() * dv],
                    [u + end.get() * du, v + end.get() * dv],
                ],
            )
        }
        0x05 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = read_f64_array::<3>(&record.payload, position)?;
            let constant = constant.get();
            (
                start,
                end,
                vec![[constant, start.get()], [constant, end.get()]],
            )
        }
        0x09 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = read_f64_array::<3>(&record.payload, position)?;
            let constant = constant.get();
            (
                start,
                end,
                vec![[start.get(), constant], [end.get(), constant]],
            )
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
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    })
}

#[cfg(test)]
fn records(bytes: &[u8]) -> Vec<B5Record> {
    let frames = object_stream_frames(bytes);
    records_from_frames(bytes, &frames)
}

fn records_from_frames(bytes: &[u8], frames: &[ObjectFrame]) -> Vec<B5Record> {
    records_from_frames_budgeted(bytes, frames, None)
}

/// Dependency-admission fixpoint with an optional session work budget.
///
/// The dependency candidates are indexed once by object identity. Earlier
/// passes rescanned every frame for every newly discovered dependency, which
/// made a large population spend its bounded work slice on repeated scans
/// before the selected topology graph could be parsed.
fn records_from_frames_budgeted(
    bytes: &[u8],
    frames: &[ObjectFrame],
    budget: Option<&WorkBudget<'_>>,
) -> Vec<B5Record> {
    let (mut records, candidates) = framed_records_and_dependency_candidates(bytes, frames, budget);
    admit_dependency_records(bytes, &mut records, &candidates, budget)
}

/// Materialize one already-indexed topology population.
///
/// The caller has already charged the frame index. This path charges each
/// topology record as it is materialized, then charges each admitted
/// dependency in the existing closure fixpoint.
fn records_from_indexed_frames_budgeted(
    bytes: &[u8],
    frames: &[ObjectFrame],
    budget: Option<&WorkBudget<'_>>,
) -> Option<Vec<B5Record>> {
    let (mut records, candidates) =
        indexed_topology_records_and_dependency_candidates(bytes, frames, budget)?;
    let records = admit_dependency_records(bytes, &mut records, &candidates, budget);
    if budget.is_some_and(WorkBudget::exhausted) {
        None
    } else {
        Some(records)
    }
}

fn admit_dependency_records(
    bytes: &[u8],
    records: &mut Vec<B5Record>,
    candidates: &DependencyCandidates,
    budget: Option<&WorkBudget<'_>>,
) -> Vec<B5Record> {
    let existing: HashSet<u32> = records.iter().map(|record| record.object_id).collect();
    let mut pending: HashSet<u32> = records
        .iter()
        .flat_map(record_references)
        .filter(|object_id| candidates.get(object_id).is_some_and(Option::is_some))
        .collect();
    let mut admitted = HashSet::new();
    loop {
        pending.retain(|object_id| !existing.contains(object_id) && !admitted.contains(object_id));
        if pending.is_empty() {
            break;
        }
        if budget.is_some_and(|budget| !budget.charge_by(pending.len())) {
            break;
        }
        let mut found = pending
            .iter()
            .filter_map(|object_id| {
                candidates
                    .get(object_id)
                    .and_then(Option::as_ref)
                    .and_then(|frame| record_from_frame(bytes, frame))
            })
            .collect::<Vec<_>>();
        if found.is_empty() {
            break;
        }
        found.sort_unstable_by_key(|record| record.offset);
        pending.clear();
        for candidate in found {
            admitted.insert(candidate.object_id);
            pending.extend(
                record_references(&candidate)
                    .into_iter()
                    .filter(|object_id| candidates.get(object_id).is_some_and(Option::is_some)),
            );
            records.push(candidate);
        }
    }
    std::mem::take(records)
}

/// Build the topology records and the exact dependency index in one frame
/// pass. The two views used to scan the same frame slice independently, which
/// spent the bounded selection budget twice before dependency closure began.
fn framed_records_and_dependency_candidates(
    bytes: &[u8],
    frames: &[ObjectFrame],
    budget: Option<&WorkBudget<'_>>,
) -> (Vec<B5Record>, DependencyCandidates) {
    if budget.is_some_and(|budget| !budget.charge_by(frames.len())) {
        return (Vec::new(), HashMap::new());
    }

    let mut records = Vec::<(usize, B5Record)>::new();
    let mut seen = HashMap::<u32, (u8, Vec<u8>)>::new();
    let mut candidates = DependencyCandidates::new();
    for frame in frames {
        if is_reference_dependency_class(frame.family, frame.class)
            && frame_payload(bytes, frame).is_some()
        {
            candidates
                .entry(frame.object_id)
                .and_modify(|slot| {
                    if slot
                        .as_ref()
                        .is_some_and(|existing| !same_object_frame(bytes, existing, frame))
                    {
                        *slot = None;
                    }
                })
                .or_insert(Some(*frame));
        }
        if !((frame.family == 0xb5 && is_topology_class(frame.class))
            || (frame.family == 0xa8 && matches!(frame.class, 0x34 | 0x62)))
        {
            continue;
        }
        let Some(record) = record_from_frame(bytes, frame) else {
            continue;
        };
        if seen
            .get(&frame.object_id)
            .is_some_and(|(seen_class, seen_payload)| {
                *seen_class == record.class && *seen_payload == record.payload
            })
        {
            continue;
        }
        seen.insert(frame.object_id, (record.class, record.payload.clone()));
        records.push((frame.end, record));
    }
    records.sort_unstable_by(|(left_end, left), (right_end, right)| {
        left_end
            .cmp(right_end)
            .then_with(|| right.offset.cmp(&left.offset))
    });
    (
        records.into_iter().map(|(_, record)| record).collect(),
        candidates,
    )
}

/// Build topology records and dependency candidates from an existing frame
/// index without charging a second frame walk. Candidate identity conflicts
/// remain ambiguous until the dependency is requested.
fn indexed_topology_records_and_dependency_candidates(
    bytes: &[u8],
    frames: &[ObjectFrame],
    budget: Option<&WorkBudget<'_>>,
) -> Option<(Vec<B5Record>, DependencyCandidates)> {
    let mut records = Vec::<(usize, B5Record)>::new();
    let mut seen = HashMap::<u32, (u8, Vec<u8>)>::new();
    let mut candidates = DependencyCandidates::new();
    for frame in frames {
        if is_reference_dependency_class(frame.family, frame.class)
            && frame_payload(bytes, frame).is_some()
        {
            candidates
                .entry(frame.object_id)
                .and_modify(|slot| {
                    if slot
                        .as_ref()
                        .is_some_and(|existing| !same_object_frame(bytes, existing, frame))
                    {
                        *slot = None;
                    }
                })
                .or_insert(Some(*frame));
        }
        if !((frame.family == 0xb5 && is_topology_class(frame.class))
            || (frame.family == 0xa8 && matches!(frame.class, 0x34 | 0x62)))
        {
            continue;
        }
        if budget.is_some_and(|budget| !budget.charge()) {
            return None;
        }
        let record = record_from_frame(bytes, frame)?;
        if seen
            .get(&frame.object_id)
            .is_some_and(|(seen_class, seen_payload)| {
                *seen_class == record.class && *seen_payload == record.payload
            })
        {
            continue;
        }
        seen.insert(frame.object_id, (record.class, record.payload.clone()));
        records.push((frame.end, record));
    }
    records.sort_unstable_by(|(left_end, left), (right_end, right)| {
        left_end
            .cmp(right_end)
            .then_with(|| right.offset.cmp(&left.offset))
    });
    Some((
        records.into_iter().map(|(_, record)| record).collect(),
        candidates,
    ))
}

fn same_object_frame(bytes: &[u8], left: &ObjectFrame, right: &ObjectFrame) -> bool {
    left.family == right.family
        && left.class == right.class
        && bytes
            .get(left.start..left.end)
            .zip(bytes.get(right.start..right.end))
            .is_some_and(|(left, right)| left == right)
}

fn frame_payload<'a>(bytes: &'a [u8], frame: &ObjectFrame) -> Option<&'a [u8]> {
    let header = if frame.family == 0xa8 { 11 } else { 8 };
    bytes.get(frame.start.checked_add(header)?..frame.end)
}

fn record_from_frame(bytes: &[u8], frame: &ObjectFrame) -> Option<B5Record> {
    Some(B5Record {
        offset: frame.start,
        family: frame.family,
        class: frame.class,
        object_id: frame.object_id,
        payload: frame_payload(bytes, frame)?.to_vec(),
    })
}

#[cfg(test)]
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
        let frame = ObjectFrame {
            start,
            end,
            family,
            class,
            object_id,
        };
        let Some(record) = record_from_frame(bytes, &frame) else {
            continue;
        };
        if seen
            .get(&object_id)
            .is_some_and(|(seen_class, seen_payload)| {
                *seen_class == record.class && *seen_payload == record.payload
            })
        {
            continue;
        }
        seen.insert(object_id, (record.class, record.payload.clone()));
        records.push((end, record));
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
pub(in crate::families) fn framed_ranges(bytes: &[u8]) -> Vec<std::ops::Range<usize>> {
    object_stream_frames(bytes)
        .into_iter()
        .map(|frame| frame.start..frame.end)
        .collect()
}

pub(in crate::families) fn object_stream_frames(bytes: &[u8]) -> Vec<ObjectFrame> {
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

/// Return maximal contiguous top-level A8/B5 object-frame runs.
fn object_stream_run_ranges(bytes: &[u8]) -> Vec<Range<usize>> {
    let external_grids = crate::families::a5a8::records::a8_external_grid_ranges(bytes);
    let mut ranges = Vec::new();
    let mut position = 0usize;
    while position + 8 <= bytes.len() {
        let Some((end, _, _, _)) = object_frame(bytes, position) else {
            position += 1;
            continue;
        };
        let start = position;
        position = end;
        loop {
            if let Some((end, _, _, _)) = object_frame(bytes, position) {
                position = end;
                continue;
            }
            let allocation_end = position
                .checked_add(15)
                .filter(|&end| end <= bytes.len())
                .filter(|&end| {
                    let rows =
                        crate::wire::records::scan_vertex_record_ranges(&bytes[position..end]);
                    matches!(rows.as_slice(), [range] if range.start == 0 && range.end == 15)
                })
                .or_else(|| {
                    external_grids
                        .iter()
                        .find(|range| range.start == position)
                        .map(|range| range.end)
                });
            let Some(end) = allocation_end else {
                break;
            };
            position = end;
        }
        ranges.push(start..position);
    }
    ranges
}

/// Return runs that declare at least one face or loop topology root.
fn topology_root_run_ranges(bytes: &[u8]) -> Vec<Range<usize>> {
    object_stream_run_ranges(bytes)
        .into_iter()
        .filter(|range| {
            let run = &bytes[range.clone()];
            let frames = object_stream_frames(run);
            frames.iter().copied().any(is_topology_root_frame)
        })
        .collect()
}

fn is_topology_root_frame(frame: ObjectFrame) -> bool {
    (frame.family == 0xb5 && frame.class == 0x5f)
        || ((frame.family == 0xb5 || frame.family == 0xa8) && frame.class == 0x62)
}

/// Partition one logical stream into independently resolved object populations.
pub(in crate::families) fn object_stream_populations(stream: &[u8]) -> Vec<Vec<u8>> {
    let runs = object_stream_run_ranges(stream);
    let topology_runs = topology_root_run_ranges(stream);
    let mut owned_populations = HashMap::new();
    let mut claimed_isolated_ids = HashSet::new();
    for range in &topology_runs {
        let root_ids = object_stream_frames(&stream[range.clone()])
            .into_iter()
            .map(|frame| frame.object_id)
            .collect::<HashSet<_>>();
        let population = owned_object_stream_population(stream, range.clone());
        claimed_isolated_ids.extend(
            object_stream_frames(&population)
                .into_iter()
                .map(|frame| frame.object_id)
                .filter(|object_id| !root_ids.contains(object_id)),
        );
        owned_populations.insert(range.start, population);
    }
    runs.into_iter()
        .filter_map(|range| {
            if let Some(population) = owned_populations.remove(&range.start) {
                return Some(population);
            }
            let claimed =
                object_frame(stream, range.start).is_some_and(|(end, family, class, object_id)| {
                    end == range.end
                        && is_referenced_geometry_class(family, class)
                        && claimed_isolated_ids.contains(&object_id)
                });
            (!claimed).then(|| stream[range].to_vec())
        })
        .collect()
}

/// Unique object population selected across reconstructed logical streams.
pub(in crate::families) enum ObjectStreamSelection {
    /// Work budget ran out before a population could be chosen.
    Exhausted {
        /// Number of object runs observed before exhaustion.
        run_count: usize,
    },
    /// No unique topology-root (or unique unrooted) run.
    Unselected {
        /// Number of object runs in the reconstructed streams.
        run_count: usize,
        /// Records scanned across every run for census.
        census_records: Vec<B5Record>,
    },
    /// One topology-root population, or the unique unrooted run.
    Selected {
        /// Bytes of the selected run, with isolated referenced geometry appended.
        source: Vec<u8>,
        /// Frames of the selected population, rebased to `source`.
        frames: Vec<ObjectFrame>,
        /// Records of the selected population, rebased to `source`.
        records: Vec<B5Record>,
        /// Records scanned across every run for census.
        census_records: Vec<B5Record>,
        /// Number of object runs in the reconstructed streams.
        run_count: usize,
    },
}

#[cfg(test)]
impl ObjectStreamSelection {
    pub(in crate::families) fn run_count(&self) -> usize {
        match self {
            Self::Exhausted { run_count }
            | Self::Unselected { run_count, .. }
            | Self::Selected { run_count, .. } => *run_count,
        }
    }

    pub(in crate::families) fn selected(&self) -> bool {
        matches!(self, Self::Selected { .. })
    }

    pub(in crate::families) fn exhausted(&self) -> bool {
        matches!(self, Self::Exhausted { .. })
    }

    pub(in crate::families) fn source(&self) -> &[u8] {
        match self {
            Self::Selected { source, .. } => source,
            Self::Exhausted { .. } | Self::Unselected { .. } => &[],
        }
    }

    pub(in crate::families) fn records(&self) -> &[B5Record] {
        match self {
            Self::Selected { records, .. } => records,
            Self::Exhausted { .. } | Self::Unselected { .. } => &[],
        }
    }

    fn census_records(&self) -> &[B5Record] {
        match self {
            Self::Selected { census_records, .. } | Self::Unselected { census_records, .. } => {
                census_records
            }
            Self::Exhausted { .. } => &[],
        }
    }
}

struct IndexedObjectRun {
    stream_index: usize,
    range: Range<usize>,
    frame_range: Range<usize>,
    topology: bool,
}

/// Select one topology-root population, or one unrooted run when it is the
/// only object run in the reconstructed logical streams.
pub(in crate::families) fn select_object_stream_population(
    streams: &[Vec<u8>],
    budget: Option<&WorkBudget<'_>>,
) -> ObjectStreamSelection {
    let stream_ranges = streams
        .iter()
        .map(|stream| object_stream_run_ranges(stream))
        .collect::<Vec<_>>();
    let run_count = stream_ranges.iter().map(Vec::len).sum();
    let exhausted = || ObjectStreamSelection::Exhausted { run_count };
    let mut stream_frames = Vec::with_capacity(streams.len());
    let mut runs = Vec::new();
    for (stream_index, (stream, ranges)) in streams.iter().zip(stream_ranges).enumerate() {
        let frames = object_stream_frames(stream);
        if budget.is_some_and(|budget| !budget.charge_by(frames.len())) {
            return exhausted();
        }
        let mut frame_cursor = 0;
        for range in ranges {
            while frame_cursor < frames.len() && frames[frame_cursor].start < range.start {
                frame_cursor += 1;
            }
            let frame_start = frame_cursor;
            while frame_cursor < frames.len() && frames[frame_cursor].end <= range.end {
                frame_cursor += 1;
            }
            let frame_range = frame_start..frame_cursor;
            let topology = frames[frame_range.clone()]
                .iter()
                .copied()
                .any(is_topology_root_frame);
            runs.push(IndexedObjectRun {
                stream_index,
                range,
                frame_range,
                topology,
            });
        }
        stream_frames.push(frames);
    }
    let topology_runs = runs
        .iter()
        .enumerate()
        .filter(|(_, run)| run.topology)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let selected_run = match topology_runs.as_slice() {
        [index] => Some((*index, true)),
        [] if runs.len() == 1 => Some((0, false)),
        _ => None,
    };
    let Some((selected_index, topology)) = selected_run else {
        let mut census_records = Vec::new();
        for run in &runs {
            let frames = &stream_frames[run.stream_index][run.frame_range.clone()];
            let records = records_from_frames_budgeted(&streams[run.stream_index], frames, budget);
            if budget.is_some_and(WorkBudget::exhausted) {
                return exhausted();
            }
            census_records.extend(records);
        }
        return ObjectStreamSelection::Unselected {
            run_count,
            census_records,
        };
    };
    let selected = &runs[selected_index];
    let selected_stream = &streams[selected.stream_index];
    let selected_frames = &stream_frames[selected.stream_index][selected.frame_range.clone()];
    let Some(selected_records) =
        records_from_indexed_frames_budgeted(selected_stream, selected_frames, budget)
    else {
        return exhausted();
    };
    let mut census_records = selected_records.clone();
    for (index, run) in runs.iter().enumerate() {
        if index == selected_index {
            continue;
        }
        let frames = &stream_frames[run.stream_index][run.frame_range.clone()];
        let records = records_from_frames_budgeted(&streams[run.stream_index], frames, budget);
        if budget.is_some_and(WorkBudget::exhausted) {
            return exhausted();
        }
        census_records.extend(records);
    }
    let mut source = selected_stream[selected.range.clone()].to_vec();
    let mut frames = selected_frames
        .iter()
        .copied()
        .map(|mut frame| {
            frame.start -= selected.range.start;
            frame.end -= selected.range.start;
            frame
        })
        .collect::<Vec<_>>();
    let mut records = selected_records
        .iter()
        .cloned()
        .map(|mut record| {
            record.offset -= selected.range.start;
            record
        })
        .collect::<Vec<_>>();
    if topology {
        let referenced = topology_surface_references(&selected_records);
        let owned_ids = selected_records
            .iter()
            .map(|record| record.object_id)
            .collect::<HashSet<_>>();
        let mut isolated = HashMap::<u32, Option<usize>>::new();
        for (index, run) in runs.iter().enumerate() {
            if index == selected_index || run.stream_index != selected.stream_index {
                continue;
            }
            let stream = &streams[run.stream_index];
            let Some((end, family, class, object_id)) = object_frame(stream, run.range.start)
            else {
                continue;
            };
            if end != run.range.end
                || owned_ids.contains(&object_id)
                || !referenced.contains(&object_id)
                || !is_referenced_geometry_class(family, class)
            {
                continue;
            }
            isolated
                .entry(object_id)
                .and_modify(|stored| {
                    if stored.is_some_and(|stored| {
                        let stored = &runs[stored];
                        streams[stored.stream_index][stored.range.clone()]
                            != stream[run.range.clone()]
                    }) {
                        *stored = None;
                    }
                })
                .or_insert(Some(index));
        }
        let mut isolated = isolated.into_values().flatten().collect::<Vec<_>>();
        isolated.sort_unstable_by_key(|index| {
            let run = &runs[*index];
            (run.stream_index, run.range.start)
        });
        for index in isolated {
            let run = &runs[index];
            let stream = &streams[run.stream_index];
            let run_frames = &stream_frames[run.stream_index][run.frame_range.clone()];
            let destination = source.len();
            source.extend_from_slice(&stream[run.range.clone()]);
            frames.extend(run_frames.iter().copied().map(|mut frame| {
                frame.start = destination + frame.start - run.range.start;
                frame.end = destination + frame.end - run.range.start;
                frame
            }));
        }
        if source.len() != selected.range.len() {
            records = records_from_frames(&source, &frames);
        }
    }
    ObjectStreamSelection::Selected {
        source,
        frames,
        records,
        census_records,
        run_count,
    }
}

/// Build one topology population from its owning run and uniquely referenced
/// isolated geometry frames in the same logical stream.
fn owned_object_stream_population(stream: &[u8], topology_run: Range<usize>) -> Vec<u8> {
    let run = &stream[topology_run.clone()];
    let run_frames = object_stream_frames(run);
    let run_records = records_from_frames(run, &run_frames);
    let referenced = topology_surface_references(&run_records);
    let owned_ids = run_records
        .iter()
        .map(|record| record.object_id)
        .collect::<HashSet<_>>();
    let mut isolated = HashMap::<u32, Option<(usize, u8, u8, Vec<u8>)>>::new();
    for range in object_stream_run_ranges(stream) {
        if range == topology_run {
            continue;
        }
        let Some((end, frame_family, frame_class, object_id)) = object_frame(stream, range.start)
        else {
            continue;
        };
        if end != range.end
            || owned_ids.contains(&object_id)
            || !referenced.contains(&object_id)
            || !is_referenced_geometry_class(frame_family, frame_class)
        {
            continue;
        }
        let bytes = stream[range.clone()].to_vec();
        isolated
            .entry(object_id)
            .and_modify(|stored| {
                if stored.as_ref().is_some_and(|(_, family, class, stored)| {
                    *family != frame_family || *class != frame_class || *stored != bytes
                }) {
                    *stored = None;
                }
            })
            .or_insert(Some((range.start, frame_family, frame_class, bytes)));
    }
    let mut isolated = isolated.into_values().flatten().collect::<Vec<_>>();
    isolated.sort_by_key(|(offset, _, _, _)| *offset);

    let mut population = run.to_vec();
    for (_, _, _, frame) in isolated {
        population.extend(frame);
    }
    population
}

fn record_references(record: &B5Record) -> Vec<u32> {
    let mut position = 0;
    let Some(count) = counted_cardinality(&record.payload, &mut position) else {
        return Vec::new();
    };
    (0..count)
        .map_while(|_| wire::tokens::object_ref(&record.payload, &mut position, true))
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
            View::u32_le_at(bytes, start + 4)?,
        ),
        0xa8 => (
            11usize,
            usize::try_from(View::u32_le_at(bytes, start + 3)?).ok()?,
            View::u32_le_at(bytes, start + 7)?,
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
    surface_aliases: &BTreeMap<u32, u32>,
) -> Option<B5Face> {
    let references = &record.references;
    let surface = *references.first()?;
    if !surfaces.contains_key(&surface) {
        return None;
    }
    let canonical_surface = canonical_surface_id(surface_aliases, surface)?;
    let mut loop_ids = Vec::new();
    for &reference in &references[1..] {
        if loops.contains_key(&reference) {
            loop_ids.push(reference);
        } else {
            let repeats_carrier = surfaces.contains_key(&reference)
                && canonical_surface_id(surface_aliases, reference) == Some(canonical_surface);
            if !repeats_carrier {
                // A distinct surface reference is a multi-surface variant. Its
                // composition is not represented by the neutral Face type, so
                // keep the typed record but withhold the face from topology.
                return None;
            }
            // A face may repeat its carrier through an alias identity. This is
            // the same carrier incidence, not a multi-surface face.
        }
    }
    if loop_ids.is_empty() {
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
            .map(|_| wire::tokens::object_ref(&record.payload, &mut position, true))
            .collect::<Option<Vec<_>>>()?;
        let &[terminal_control] = record.payload.get(position..)? else {
            return None;
        };
        let terminal_control = B5FramingControl::from_byte(terminal_control)?;
        Some(B5FaceRecord {
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
fn typed_face_records(bytes: &[u8]) -> BTreeMap<u32, B5FaceRecord> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_face_records_from_records(&records)
}

pub(in crate::families) fn typed_face_records_from_records(
    records: &[B5Record],
) -> BTreeMap<u32, B5FaceRecord> {
    records
        .iter()
        .filter_map(|record| parse_face_record(record).map(|face| (record.object_id, face)))
        .collect()
}

/// Read every structurally complete loop record independently of target
/// resolution.
#[cfg(test)]
fn typed_loop_records(bytes: &[u8]) -> BTreeMap<u32, B5Loop> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_loop_records_from_records(&records)
}

pub(in crate::families) fn typed_loop_records_from_records(
    records: &[B5Record],
) -> BTreeMap<u32, B5Loop> {
    records
        .iter()
        .filter_map(|record| parse_loop_record(record).map(|loop_| (record.object_id, loop_)))
        .collect()
}

/// Read every structurally complete physical-edge record independently of
/// topology resolution.
#[cfg(test)]
fn typed_edge_records(bytes: &[u8]) -> BTreeMap<u32, B5Edge> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_edge_records_from_records(&records)
}

pub(in crate::families) fn typed_edge_records_from_records(
    records: &[B5Record],
) -> BTreeMap<u32, B5Edge> {
    records
        .iter()
        .filter_map(|record| parse_edge(record).map(|edge| (record.object_id, edge)))
        .collect()
}

/// Read every structurally complete vertex-incidence link independently of
/// topology resolution.
#[cfg(test)]
fn typed_vertex_incidence_links(bytes: &[u8]) -> BTreeMap<u32, B5VertexIncidenceLink> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_vertex_incidence_links_from_records(&records)
}

pub(in crate::families) fn typed_vertex_incidence_links_from_records(
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
fn typed_class_21_pcurves(bytes: &[u8]) -> BTreeMap<u32, B5Pcurve> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_class_21_pcurves_from_records(&records)
}

pub(in crate::families) fn typed_class_21_pcurves_from_records(
    records: &[B5Record],
) -> BTreeMap<u32, B5Pcurve> {
    records
        .iter()
        .filter_map(|record| parse_pcurve(record).map(|pcurve| (record.object_id, pcurve)))
        .collect()
}

/// Read every structurally complete parameter incidence independently of
/// curve, edge, and topology resolution.
#[cfg(test)]
fn typed_parameter_incidences(bytes: &[u8]) -> BTreeMap<u32, B5ParameterIncidence> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_parameter_incidences_from_records(&records)
}

pub(in crate::families) fn typed_parameter_incidences_from_records(
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
fn typed_vertex_incidence_rosters(bytes: &[u8]) -> BTreeMap<u32, Vec<u32>> {
    let frames = object_stream_frames(bytes);
    let records = records_from_frames(bytes, &frames);
    typed_vertex_incidence_rosters_from_records(&records)
}

pub(in crate::families) fn typed_vertex_incidence_rosters_from_records(
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
fn face_surface_references(bytes: &[u8]) -> Vec<(u32, u32)> {
    let frames = object_stream_frames(bytes);
    face_surface_references_from_frames(bytes, &frames)
}

pub(in crate::families) fn face_surface_references_from_frames(
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
        let Some(surface) = wire::tokens::object_ref(payload, &mut position, true) else {
            continue;
        };
        references.push((frame.object_id, surface));
    }
    references
}

/// Return positive face-to-edge ownership from structurally complete face and
/// loop records, without requiring their referenced surfaces or p-curves to
/// resolve into a transferable graph.
pub(in crate::families) fn edge_face_references_from_frames(
    bytes: &[u8],
    frames: &[ObjectFrame],
) -> HashMap<u32, HashSet<u32>> {
    let records = records_from_frames(bytes, frames);
    let edge_ids = records
        .iter()
        .filter(|record| record.family == 0xb5 && record.class == 0x5e)
        .map(|record| record.object_id)
        .collect::<HashSet<_>>();
    let loops = typed_loop_records_from_records(&records);
    let mut owners = HashMap::<u32, HashSet<u32>>::new();
    for (face, record) in typed_face_records_from_records(&records) {
        for loop_id in record.references.iter().skip(1) {
            let Some(loop_record) = loops.get(loop_id) else {
                continue;
            };
            for member in &loop_record.members {
                if edge_ids.contains(&member.edge) {
                    owners.entry(member.edge).or_default().insert(face);
                }
            }
        }
    }
    owners
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
    for member in &record.members {
        if (parsed_pcurves
            .get(&member.pcurve)
            .is_none_or(|pcurve| pcurve.surface != surface)
            && opaque_pcurves
                .get(&member.pcurve)
                .is_none_or(|pcurve| pcurve.surface != surface)
            && implicit_pcurves.get(&member.pcurve) != Some(&surface))
            || by_id.get(&member.edge)?.class != 0x5e
        {
            return None;
        }
    }
    Some(record.clone())
}

fn parse_loop_record(record: &B5Record) -> Option<B5Loop> {
    let (references, metadata, edge_controls) = loop_references_and_metadata(record)?;
    let surface = *references.last()?;
    let pairs = &references[..references.len() - 1];
    if pairs.len() / 2 != edge_controls.len() {
        return None;
    }
    let members = pairs
        .chunks_exact(2)
        .zip(edge_controls)
        .map(|(pair, controls)| B5LoopMember {
            pcurve: pair[0],
            edge: pair[1],
            controls,
        })
        .collect();
    Some(B5Loop {
        object_id: record.object_id,
        members,
        metadata,
        surface,
    })
}

fn loop_references(record: &B5Record) -> Option<Vec<u32>> {
    loop_references_and_metadata(record).map(|(references, _, _)| references)
}

fn loop_references_and_metadata(
    record: &B5Record,
) -> Option<(Vec<u32>, B5LoopMetadata, Vec<[i16; 3]>)> {
    (record.class == 0x62).then_some(())?;
    let mut position = 0;
    let count = counted_cardinality(&record.payload, &mut position)?;
    if count < 3 || count % 2 == 0 {
        return None;
    }
    let references = (0..count)
        .map(|_| wire::tokens::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?;
    let edge_count = (count - 1) / 2;
    if counted_cardinality(&record.payload, &mut position)? != edge_count {
        return None;
    }
    let (metadata, edge_controls) = loop_metadata(record.payload.get(position..)?, edge_count)?;
    Some((references, metadata, edge_controls))
}

fn loop_metadata(bytes: &[u8], edge_count: usize) -> Option<(B5LoopMetadata, Vec<[i16; 3]>)> {
    let controls_len = edge_count.checked_mul(3)?.checked_mul(2)?;
    let controls_end = 3usize.checked_add(controls_len)?;
    let framing_controls = [
        B5FramingControl::from_byte(*bytes.first()?)?,
        B5FramingControl::from_byte(*bytes.get(1)?)?,
    ];
    if bytes.get(2) != Some(&0x03) || controls_end > bytes.len() {
        return None;
    }
    let edge_controls = bytes[3..controls_end]
        .chunks_exact(6)
        .map(|controls| {
            let mut view = View::over_retained(controls);
            let controls = [view.i16_le()?, view.i16_le()?, view.i16_le()?];
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
            let mut view = View::over_retained(extended);
            view.seek(1)?;
            let scalars = [
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
            ];
            view.seek(38)?;
            let floats = [
                view.f32_le()?,
                view.f32_le()?,
                view.f32_le()?,
                view.f32_le()?,
                view.f32_le()?,
                view.f32_le()?,
            ];
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
    Some((
        B5LoopMetadata {
            framing_controls,
            extension,
        },
        edge_controls,
    ))
}

fn counted_cardinality(bytes: &[u8], position: &mut usize) -> Option<usize> {
    let lead = *bytes.get(*position)?;
    if lead >= 0x80 {
        *position += 1;
        Some(usize::from(lead - 0x80))
    } else {
        usize::try_from(wire::tokens::object_ref(bytes, position, true)?).ok()
    }
}

fn uncounted_references(bytes: &[u8]) -> Option<Vec<u32>> {
    let mut position = 0;
    let mut references = Vec::new();
    while position < bytes.len() {
        references.push(wire::tokens::object_ref(bytes, &mut position, true)?);
    }
    Some(references)
}

#[cfg(test)]
mod tests;
