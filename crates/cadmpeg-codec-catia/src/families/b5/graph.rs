// SPDX-License-Identifier: Apache-2.0
//! Object-id topology in the CATIA `b5 03` short-frame family.

use cadmpeg_core::convert::{f64_from_index, truncate_f64_to_i64, truncate_f64_to_usize};
use cadmpeg_core::decode::u64_from_index;

type VertexComponentOutput = Result<(BTreeMap<u32, usize>, bool), CodecError>;
type CirclePcurveFields = Option<(u32, [f64; 2], f64, [f64; 2], [f64; 2])>;
type Class1aPcurveFields = Option<(u32, [f64; 2], [f64; 2], [f64; 2], f64, [f64; 2], [f64; 2])>;
type LoopReferencesOutput = Result<Option<(Vec<u32>, B5LoopMetadata, Vec<[i16; 3]>)>, CodecError>;
type LoopMetadataOutput = Result<Option<(B5LoopMetadata, Vec<[i16; 3]>)>, CodecError>;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::NonZeroUsize;
use std::ops::Range;

use cadmpeg_core::decode::{cost::DecodeCost, DecodeContext, ScopedReservation, View, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::nurbs_pcurve_uv;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    analytic::ConeSurface,
    nurbs::{knots_strictly_increasing, NurbsSurface},
    ProceduralSurfaceDefinition,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{
    Angle, FiniteReal, NonNegativeLength, PositiveAngle, PositiveLength, PositiveReal,
};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::{FiniteVector, OrthonormalFrame3, UnitVector3};

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
    pub(in crate::families) point: FinitePoint3,
}

impl B5Graph {
    /// Follow surface aliases to their canonical terminal identity.
    pub(super) fn canonical_surface_id(
        &self,
        ctx: &DecodeContext<'_>,
        object_id: u32,
    ) -> Result<Option<u32>, CodecError> {
        canonical_surface_id(ctx, &self.surface_aliases, object_id)
    }

    /// Return native edge identities proven to belong to the closed B-rep.
    ///
    /// A structurally parseable class-`5e` allocation is a physical edge only
    /// when a resolved face loop references it.  An incomplete graph cannot
    /// prove that the retained loop set is exhaustive, so callers must use
    /// their unresolved-association fallback in that case.
    pub(in crate::families) fn referenced_edge_vertex_references(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<BTreeMap<u32, [u32; 2]>>, CodecError> {
        if !self.complete {
            return Ok(None);
        }
        let mut referenced = BTreeMap::new();
        for (_, loop_) in ctx.admit_iter(&self.loops, "catia_b5_referenced_edge_loop_scan")? {
            for member in ctx.admit_iter(&loop_.members, "catia_b5_referenced_edge_member_scan")? {
                if let Some(edge) =
                    ctx.get_btree_map(&self.edges, &member.edge, "catia_b5_referenced_edge_lookup")?
                {
                    ctx.insert_btree_map(
                        &mut referenced,
                        member.edge,
                        edge.vertices,
                        "catia_b5_referenced_edge_vertices",
                    )?;
                }
            }
        }
        Ok(Some(referenced))
    }
}

/// Return the ordered start/end stations for one edge's occurrence of a
/// pcurve when both native endpoint incidences name that pcurve consistently.
pub(super) fn edge_pcurve_parameters(
    ctx: &DecodeContext<'_>,
    graph: &B5Graph,
    edge: u32,
    pcurve: u32,
) -> Result<Option<[FiniteReal; 2]>, CodecError> {
    edge_pcurve_parameter_values(
        ctx,
        &graph.edge_parameter_incidences,
        &graph.parameter_incidences,
        edge,
        pcurve,
    )
}

fn edge_pcurve_parameter_values(
    ctx: &DecodeContext<'_>,
    edge_parameter_incidences: &BTreeMap<u32, [u32; 2]>,
    parameter_incidences: &BTreeMap<u32, B5ParameterIncidence>,
    edge: u32,
    pcurve: u32,
) -> Result<Option<[FiniteReal; 2]>, CodecError> {
    const OPERATION: &str = "catia_b5_edge_parameter_incidence_lookup";
    let Some(incidences) = ctx.get_btree_map(edge_parameter_incidences, &edge, OPERATION)? else {
        return Ok(None);
    };
    let mut endpoints = [None, None];
    for (endpoint, incidence_id) in endpoints.iter_mut().zip(incidences) {
        let Some(incidence) = ctx.get_btree_map(parameter_incidences, incidence_id, OPERATION)?
        else {
            return Ok(None);
        };
        // Every lane naming the pcurve must state the same parameter.
        let mut parameter = None;
        let consistent = ctx.all_by(
            &incidence.lanes,
            |lane| {
                if lane.curve != pcurve {
                    return Ok(true);
                }
                Ok(*parameter.get_or_insert(lane.parameter) == lane.parameter)
            },
            "catia_b5_edge_parameter_lane_scan",
        )?;
        if !consistent || parameter.is_none() {
            return Ok(None);
        }
        *endpoint = parameter;
    }
    let [Some(start), Some(end)] = endpoints else {
        return Ok(None);
    };
    Ok(Some([start, end]))
}

/// Follow one surface identity through a direct alias map.
pub(in crate::families) fn canonical_surface_id(
    ctx: &DecodeContext<'_>,
    aliases: &BTreeMap<u32, u32>,
    mut object_id: u32,
) -> Result<Option<u32>, CodecError> {
    // A chain with more steps than aliases revisits one, so it is a cycle.
    // Each step charges its own lookup, so a chain pays only for its length.
    for _ in 0..=aliases.len() {
        match ctx.get_btree_map(aliases, &object_id, "catia_b5_surface_alias_step")? {
            Some(&target) => object_id = target,
            None => return Ok(Some(object_id)),
        }
    }
    Ok(None)
}

/// A profile curve swept by a `b5 03 2d` surface of revolution.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) enum B5Profile {
    /// `b5 03 0e`: a line through `point` along `direction`.
    Line {
        /// A point on the line.
        point: FinitePoint3,
        /// Unit direction of the line.
        direction: ExactUnitVector3,
        /// Complete native line parameter interval.
        parameter_range: IncreasingParameterInterval,
    },
    /// `b5 03 0f`: an arc with a positive radius.
    Arc {
        /// Arc center.
        center: FinitePoint3,
        /// Unit vector from `center` toward the zero-angle point.
        direction_x: ExactUnitVector3,
        /// Unit vector orthogonal to `direction_x` completing the arc
        /// plane's basis.
        direction_y: ExactUnitVector3,
        /// Positive arc radius.
        radius: PositiveLength,
        /// Complete native arc-length parameter interval.
        parameter_range: IncreasingParameterInterval,
    },
}

impl B5Profile {
    pub(super) fn parameter_range(&self) -> IncreasingParameterInterval {
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
        direction_v: ExactUnitVector3,
        /// Active native U interval.
        u_range: IncreasingParameterInterval,
        /// Active native V interval.
        v_range: IncreasingParameterInterval,
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
        u_range: IncreasingParameterInterval,
        /// Active native axial interval.
        v_range: IncreasingParameterInterval,
        /// Divisor mapping native U to azimuth.
        angular_scale: FiniteReal,
        /// Origin of the full-turn native U chart.
        chart_origin: FiniteReal,
    },
    /// `b5 03 29`: a circular cone in its native arc-length/slant chart.
    Cone {
        /// Cone apex.
        apex: FinitePoint3,
        /// The stored cone-axis unit direction and the stored first transverse
        /// unit direction, the zero-angle ray.
        frame: OrthonormalFrame3,
        /// The stored second transverse unit direction.
        direction_y: UnitVector3,
        /// Cone half-angle in radians, strictly between zero and a quarter
        /// turn.
        half_angle: PositiveAngle,
        /// Reference radius of the conical surface, independent of the active chart ranges.
        reference_radius: FiniteReal,
        /// Active azimuth interval.
        angular_range: IncreasingParameterInterval,
        /// Native slant-coordinate range.
        slant_range: IncreasingParameterInterval,
        /// Positive divisor mapping native U to azimuth.
        angular_scale: PositiveReal,
        /// Full-turn azimuth chart domain.
        angular_domain: IncreasingParameterInterval,
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
        direction_y: UnitVector3,
        /// Positive sphere radius.
        radius: PositiveLength,
        /// Active azimuth interval, in radians.
        azimuth_range: IncreasingParameterInterval,
        /// Active latitude interval, in radians.
        latitude_range: IncreasingParameterInterval,
        /// Positive radius of the enclosing support-bound construction.
        construction_radius: PositiveLength,
        /// Origin of the native periodic V coordinate, in length units.
        chart_origin: FiniteReal,
    },
    /// `b5 03 2b`: a torus in its two arc-length angular coordinates.
    Torus {
        /// Torus center.
        center: FinitePoint3,
        /// The torus axis and the zero-major-angle direction.
        frame: OrthonormalFrame3,
        /// Quarter-turn major-angle direction.
        direction_y: ExactUnitVector3,
        /// Positive major radius.
        major_radius: PositiveLength,
        /// Positive minor radius.
        minor_radius: PositiveLength,
        /// Active major-angle interval.
        major_angular_range: IncreasingParameterInterval,
        /// Full-turn major-angle chart domain.
        major_angular_domain: IncreasingParameterInterval,
        /// Active minor-angle interval.
        minor_angular_range: IncreasingParameterInterval,
        /// Full-turn minor-angle chart domain.
        minor_angular_domain: IncreasingParameterInterval,
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
        profile_range: IncreasingParameterInterval,
        /// Active native arc-length interval of the revolution.
        angular_range: IncreasingParameterInterval,
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
        definition: Box<ProceduralSurfaceDefinition>,
    },
}

impl DecodeCost for B5Surface {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        // Equality reads the inline fields and each owned child.
        let overflow = || ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX);
        let children = match self {
            // The header's knot lanes are decoded from the payload: each
            // distinct knot occupies eight payload bytes and keeps an
            // eight-byte value and a four-byte multiplicity, so the header owns
            // fewer bytes than twice the payload.
            Self::UnresolvedNurbs { payload, .. } => {
                payload.len().checked_mul(3).ok_or_else(overflow)?
            }
            Self::Unknown { payload, .. } => payload.len(),
            Self::Nurbs(surface) => {
                let knots = surface
                    .u_knots()
                    .as_slice()
                    .len()
                    .checked_add(surface.v_knots().as_slice().len())
                    .and_then(|count| count.checked_mul(std::mem::size_of::<f64>()));
                let poles = surface
                    .u_count()
                    .checked_mul(surface.v_count())
                    .and_then(|count| {
                        count.checked_mul(
                            std::mem::size_of::<FinitePoint3>() + std::mem::size_of::<f64>(),
                        )
                    });
                knots
                    .zip(poles)
                    .and_then(|(knots, poles)| knots.checked_add(poles))
                    .ok_or_else(overflow)?
            }
            // Decode builds jet definitions only; another definition is
            // compared by its inline representation.
            Self::RollingBall { definition, .. } => match definition.as_ref() {
                ProceduralSurfaceDefinition::RollingBallJet(jet) => {
                    std::mem::size_of_val(jet.stations())
                }
                _ => std::mem::size_of::<ProceduralSurfaceDefinition>(),
            },
            Self::Plane { .. }
            | Self::Cylinder { .. }
            | Self::Cone { .. }
            | Self::Sphere { .. }
            | Self::Torus { .. }
            | Self::Revolution { .. } => 0,
        };
        children
            .checked_add(std::mem::size_of::<Self>())
            .map(u64_from_index)
            .ok_or_else(overflow)
    }
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
        parameter_range: IncreasingParameterInterval,
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
        parameter_range: IncreasingParameterInterval,
    },
    /// Fixed-direction offset carried by a `b5 03 14` record.
    Offset {
        /// Persistent offset-curve object id.
        object_id: u32,
        /// Complete source curve construction.
        source: Box<B5ExtrusionDirectrix>,
        /// Increasing interval on the source curve.
        source_parameter_range: IncreasingParameterInterval,
        /// Signed nonzero offset distance.
        distance: FiniteReal,
        /// Unit direction defining the positive offset side.
        direction: UnitVector3,
        /// Increasing result-curve parameter range.
        parameter_range: IncreasingParameterInterval,
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

    fn parameter_range(&self) -> IncreasingParameterInterval {
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

    fn reorigin_parameter_range(&mut self, range: IncreasingParameterInterval) -> bool {
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

    pub(super) fn supports(&self) -> &[(u32, u32, [FiniteReal; 2])] {
        match self {
            Self::Intersection { supports, .. } => supports,
            Self::SurfaceCurve { support, .. } => std::slice::from_ref(support),
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
        construction_radius: PositiveLength,
    },
    /// Class `3b`: six contiguous controls followed by two positive scalars.
    ScalarPair {
        /// Six contiguous control bytes.
        controls: [u8; 6],
        /// Two positive construction scalars.
        scalars: [PositiveReal; 2],
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
    pub(super) control_points: Vec<FiniteVector<2>>,
    /// Positive per-pole rational weights. `None` denotes a polynomial
    /// pcurve.
    pub(super) weights: Option<Vec<PositiveReal>>,
    /// Explicit occurrence parameter interval when the pcurve record stores one.
    pub(super) parameter_range: Option<[FiniteReal; 2]>,
    /// Coordinate convention for evaluating the stored knot vector.
    pub(super) parameterization: B5PcurveParameterization,
    /// Positive scalar stored in the exact class-`21` suffix. When a class-`21`
    /// pcurve is present, it is the length of the zero-based occurrence interval.
    pub(in crate::families) class_21_suffix_scalar: Option<PositiveReal>,
    /// The curve's two clamped-end poles lifted through `surface` into
    /// world-frame 3D points, or `None` before [`parse`] resolves them or
    /// when the lift fails (unresolved surface, degenerate revolution
    /// scale, or NURBS evaluation failure).
    pub(super) lifted_endpoints: Option<[FinitePoint3; 2]>,
}

impl DecodeCost for B5Pcurve {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        // Equality reads the inline fields and each owned lane.
        let lanes = std::mem::size_of_val(self.distinct_knots.as_slice())
            .checked_add(std::mem::size_of_val(self.multiplicities.as_slice()))
            .and_then(|bytes| {
                bytes.checked_add(std::mem::size_of_val(self.control_points.as_slice()))
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    self.weights
                        .as_deref()
                        .map_or(0, std::mem::size_of_val::<[PositiveReal]>),
                )
            })
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>()))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        Ok(u64_from_index(lanes))
    }
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
/// The payload borrows the framed stream bytes; a parser copies only the
/// bytes it keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::families) struct B5Record<'a> {
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
    /// Raw record payload after the record header.
    pub(in crate::families) payload: &'a [u8],
}

impl B5Record<'_> {
    /// Exclusive end of the record frame in the source stream.
    fn frame_end(&self) -> usize {
        let header = if self.family == 0xa8 { 11 } else { 8 };
        self.offset + header + self.payload.len()
    }
}

/// Test-owned record bytes that tests edit before borrowing them as a
/// [`B5Record`].
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::families) struct B5RecordBuf {
    pub(in crate::families) offset: usize,
    pub(in crate::families) family: u8,
    pub(in crate::families) class: u8,
    pub(in crate::families) object_id: u32,
    pub(in crate::families) payload: Vec<u8>,
}

#[cfg(test)]
impl B5RecordBuf {
    /// Borrows the owned bytes as a framed record.
    pub(in crate::families) fn record(&self) -> B5Record<'_> {
        B5Record {
            offset: self.offset,
            family: self.family,
            class: self.class,
            object_id: self.object_id,
            payload: &self.payload,
        }
    }
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

impl DecodeCost for B5Face {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (
            &self.object_id,
            &self.surface,
            &self.loops,
            &self.terminal_control,
        )
            .decode_cost(ctx, operation)
    }
}

/// Count the face incidences for each object-stream loop.
pub(super) fn face_loop_owner_counts(
    ctx: &DecodeContext<'_>,
    faces: &[B5Face],
) -> Result<BTreeMap<u32, usize>, CodecError> {
    const OPERATION: &str = "catia_b5_face_loop_owners";
    let mut owners = BTreeMap::new();
    for face in ctx.admit_iter(faces, "catia_b5_face_loop_owner_scan")? {
        for &loop_id in ctx.admit_iter(&face.loops, "catia_b5_face_loop_owner_loop_scan")? {
            if let Some(count) = ctx.get_mut_btree_map(&mut owners, &loop_id, OPERATION)? {
                *count += 1;
            } else {
                ctx.insert_btree_map(&mut owners, loop_id, 1, OPERATION)?;
            }
        }
    }
    Ok(owners)
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
    scalars: [FiniteReal; 4],
    /// Exact admitted odd extension control.
    control: u8,
    /// Six finite binary32 fields in serialized order.
    floats: [f32; 6],
}

impl B5Loop {
    pub(super) fn edge_senses(&self, ctx: &DecodeContext<'_>) -> Result<Vec<bool>, CodecError> {
        ctx.collect_vec(
            self.members.iter().map(|member| member.controls[0] == -1),
            "catia_b5_loop_edge_senses",
        )
    }
}

/// Resolve the dominant object-stream topology graph through inline object ids.
pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<B5Graph>, CodecError> {
    // A second resolved run makes the graph ambiguous, so the walk stops there.
    let mut first = None;
    let mut ambiguous = false;
    visit_topology_runs(ctx, bytes, refusal, |_, graph| {
        if first.is_some() {
            ambiguous = true;
            return Ok(false);
        }
        first = Some(graph);
        Ok(true)
    })?;
    Ok(first.filter(|_| !ambiguous))
}

/// Resolve each contiguous object-stream run independently.
#[cfg(test)]
fn topology_runs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<(Range<usize>, B5Graph)>, CodecError> {
    let mut graphs = Vec::new();
    visit_topology_runs(ctx, bytes, refusal, |range, graph| {
        ctx.push_vec(&mut graphs, (range, graph), "catia B5 topology runs")?;
        Ok(true)
    })?;
    Ok(graphs)
}

/// Resolve the topology-root runs, or every run when none declares a root,
/// and pass each resolved graph to `visit` until it answers `false`. Each
/// population is scratch: it is dropped once its graph is resolved.
fn visit_topology_runs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
    mut visit: impl FnMut(Range<usize>, B5Graph) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let (frames, runs) = index_object_runs(ctx, bytes, 0)?;
    let rooted = ctx.any_by(
        &runs,
        |run| Ok(run.topology),
        "catia_b5_topology_root_run_scan",
    )?;
    for (index, run) in ctx
        .admit_iter(&runs, "catia_b5_topology_candidate_scan")?
        .enumerate()
    {
        if rooted && !run.topology {
            continue;
        }
        let mut scratch = ctx.reserve_scoped(0, "catia_b5_topology_population_scratch")?;
        let (population, _) = scratch
            .with_storage(|| owned_object_stream_population(ctx, bytes, &frames, &runs, index))?;
        let Some(graph) = parse_flat(ctx, &population, refusal)? else {
            continue;
        };
        if !visit(run.range.clone(), graph)? {
            break;
        }
    }
    Ok(())
}

fn parse_flat(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<B5Graph>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_parse_frame_scratch")?;
    let frames = scratch.with_storage(|| collect_object_stream_frames(ctx, bytes))?;
    let records = scratch.with_storage(|| records_from_frames(ctx, bytes, &frames))?;
    parse_from_records(ctx, bytes, &records, &frames, true, refusal)
}

pub(in crate::families) fn parse_from_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frames: &[ObjectFrame],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<B5Graph>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_parse_record_scratch")?;
    let records = scratch.with_storage(|| records_from_frames(ctx, bytes, frames))?;
    parse_from_records(ctx, bytes, &records, frames, true, refusal)
}

pub(in crate::families) fn parse_from_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[B5Record],
    frames: &[ObjectFrame],
    require_topology: bool,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<B5Graph>, CodecError> {
    parse_from_records_budgeted(ctx, bytes, records, frames, require_topology, None, refusal)
}

pub(in crate::families) fn parse_from_records_budgeted(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[B5Record<'_>],
    frames: &[ObjectFrame],
    require_topology: bool,
    budget: Option<&WorkBudget<'_>>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<B5Graph>, CodecError> {
    if records.is_empty() {
        return Ok(None);
    }
    let mut index_storage = ctx.reserve_scoped(0, "catia_b5_record_index")?;
    let by_id = index_storage.with_storage(|| {
        ctx.collect_hash_map(
            ctx.admit_iter(records, "catia_b5_record_index_scan")?
                .map(|record| (record.object_id, record)),
            "catia_b5_record_index",
        )
    })?;
    if by_id.len() != records.len() {
        return Ok(None);
    }
    let class21_candidates = a8_class21_pcurves_from_frames(ctx, bytes, frames)?;
    let mut jet_storage = ctx.reserve_scoped(0, "catia_b5_object_pcurve_jets")?;
    let jets = jet_storage
        .with_storage(|| crate::families::a5a8::records::object_stream_pcurves(ctx, bytes))?;
    let mut object_stream_pcurve_candidates = Vec::new();
    for jet in ctx.admit_iter(&jets, "catia_b5_object_pcurve_jet_scan")? {
        if let Some(candidate) = object_stream_pcurve_candidate(ctx, jet)? {
            ctx.push_vec(
                &mut object_stream_pcurve_candidates,
                candidate,
                "catia B5 object pcurve candidates",
            )?;
        }
    }
    drop(jets);
    drop(jet_storage);
    parse_from_records_with_class21(
        ctx,
        bytes,
        (records, frames),
        require_topology,
        budget,
        refusal,
        PreparedB5Graph {
            class21_candidates,
            object_stream_pcurve_candidates,
            by_id: &by_id,
        },
    )
}

struct PreparedB5Graph<'a, 'b> {
    class21_candidates: Vec<B5Pcurve>,
    object_stream_pcurve_candidates: Vec<B5Pcurve>,
    by_id: &'a HashMap<u32, &'b B5Record<'b>>,
}

/// Charged lookup of one framed record by object id.
fn record_by_id<'r, 'a>(
    ctx: &DecodeContext<'_>,
    by_id: &'r HashMap<u32, &'r B5Record<'a>>,
    object_id: u32,
    operation: &'static str,
) -> Result<Option<&'r B5Record<'a>>, CodecError> {
    Ok(ctx.get_hash_map(by_id, &object_id, operation)?.copied())
}

/// Whether two identities hold equal surfaces, both absent counting as equal.
fn same_surface_entries(
    ctx: &DecodeContext<'_>,
    surfaces: &BTreeMap<u32, B5Surface>,
    left: u32,
    right: u32,
    operation: &'static str,
) -> Result<bool, CodecError> {
    match (
        ctx.get_btree_map(surfaces, &left, operation)?,
        ctx.get_btree_map(surfaces, &right, operation)?,
    ) {
        (Some(left), Some(right)) => ctx.equal(left, right, operation),
        (None, None) => Ok(true),
        _ => Ok(false),
    }
}

fn parse_from_records_with_class21(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    (records, frames): (&[B5Record<'_>], &[ObjectFrame]),
    require_topology: bool,
    budget: Option<&WorkBudget<'_>>,
    refusal: &mut crate::nurbs::LaneRefusals,
    prepared: PreparedB5Graph<'_, '_>,
) -> Result<Option<B5Graph>, CodecError> {
    const LOOKUP: &str = "catia_b5_graph_lookup";
    let PreparedB5Graph {
        class21_candidates,
        object_stream_pcurve_candidates: object_stream_candidates,
        by_id,
    } = prepared;
    // Indexes and conflict sets that the graph does not keep.
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_graph_scratch")?;
    let mut object_stream_pcurve_candidates = BTreeMap::new();
    let mut conflicting_object_stream_pcurves = HashSet::new();
    let mut object_stream_pcurve_classes = HashMap::<u32, Option<u8>>::new();
    for (class, candidates) in [(0x20, object_stream_candidates), (0x21, class21_candidates)] {
        for candidate in ctx.admit_iter(candidates, "catia_b5_object_pcurve_candidate_scan")? {
            scratch.with_storage(|| {
                ctx.entry_hash_map(
                    &mut object_stream_pcurve_classes,
                    candidate.object_id,
                    "catia_b5_object_pcurve_classes",
                )?
                .and_modify(|stored| {
                    if *stored != Some(class) {
                        *stored = None;
                    }
                })
                .or_insert(Some(class));
                Ok::<_, CodecError>(())
            })?;
            merge_pcurve_candidate(
                ctx,
                &mut object_stream_pcurve_candidates,
                (&mut conflicting_object_stream_pcurves, &mut scratch),
                candidate,
            )?;
        }
    }
    let a8_headers = scratch.with_storage(|| {
        let mut headers = BTreeMap::new();
        for frame in ctx.admit_iter(frames, "catia_b5_a8_surface_header_frame_scan")? {
            if let Some(header) =
                crate::families::a5a8::records::a8_surface_header_from_object_frame(
                    ctx,
                    bytes,
                    frame.start,
                    frame.end,
                    frame.object_id,
                )?
            {
                ctx.insert_btree_map(
                    &mut headers,
                    header.object_id,
                    header,
                    "catia_b5_a8_surface_headers",
                )?;
            }
        }
        Ok::<_, CodecError>(headers)
    })?;
    let mut surfaces = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_class21_candidate_record_scan")? {
        if let Some(surface) = surface_node(ctx, record, &a8_headers)? {
            ctx.insert_btree_map(
                &mut surfaces,
                record.object_id,
                surface,
                "catia_b5_graph_surfaces",
            )?;
        }
    }
    let mut conflicting_surfaces = HashSet::new();
    let topology_surfaces = scratch.with_storage(|| topology_surface_references(ctx, records))?;
    for &surface_id in ctx.admit_iter(&topology_surfaces, "catia_b5_topology_surface_scan")? {
        if ctx.contains_key_btree_map(&surfaces, &surface_id, LOOKUP)? {
            continue;
        }
        let Some(record) = record_by_id(ctx, by_id, surface_id, LOOKUP)?
            .filter(|record| record.family == 0xb5 && is_opaque_surface_class(record.class))
        else {
            continue;
        };
        let payload = ctx.copy_slice(record.payload, "catia_b5_opaque_surface_payload")?;
        ctx.insert_btree_map(
            &mut surfaces,
            surface_id,
            B5Surface::Unknown {
                family: record.family,
                class: record.class,
                payload,
            },
            "catia_b5_opaque_surfaces",
        )?;
    }
    let mut external_grids = crate::families::a5a8::records::A8ExternalGridSites::default();
    for frame in ctx.admit_iter(frames, "catia_b5_class21_candidate_frame_scan")? {
        let Some(surface) = crate::families::a5a8::records::resolved_a8_surface_from_object_frame(
            ctx,
            bytes,
            frame.start,
            frame.end,
            frame.object_id,
            &mut external_grids,
            refusal,
        )?
        else {
            continue;
        };
        if let Some(object_id) = surface.object_id() {
            merge_surface_candidate(
                ctx,
                &mut surfaces,
                (&mut conflicting_surfaces, &mut scratch),
                object_id,
                B5Surface::Nurbs(surface.geometry),
            )?;
        }
    }
    let mut jet_storage = ctx.reserve_scoped(0, "catia_b5_rolling_ball_jets")?;
    let jets = jet_storage
        .with_storage(|| crate::families::a5a8::records::a8_freeform_curves(ctx, bytes))?;
    for jet in ctx.admit_iter(&jets, "catia_b5_rolling_ball_jet_scan")? {
        if let Some(definition) =
            crate::families::a5a8::records::rolling_ball_jet_definition(ctx, jet)?
        {
            ctx.charge_retained(
                u64_from_index(std::mem::size_of::<ProceduralSurfaceDefinition>()),
                "catia_b5_rolling_ball_definition_box",
            )?;
            merge_surface_candidate(
                ctx,
                &mut surfaces,
                (&mut conflicting_surfaces, &mut scratch),
                jet.object_id,
                B5Surface::RollingBall {
                    carrier_object_id: jet.object_id,
                    definition: Box::new(definition),
                },
            )?;
        }
    }
    drop(jets);
    drop(jet_storage);
    let object_stream_pcurves = scratch.with_storage(|| {
        let mut pcurves = BTreeMap::new();
        for (&object_id, pcurve) in ctx.admit_iter(
            &object_stream_pcurve_candidates,
            "catia_b5_object_stream_pcurve_materialization_scan",
        )? {
            let Some((class, parameter_range)) = ctx
                .get_hash_map(&object_stream_pcurve_classes, &object_id, LOOKUP)?
                .copied()
                .flatten()
                .zip(pcurve.parameter_range)
            else {
                continue;
            };
            ctx.insert_btree_map(
                &mut pcurves,
                object_id,
                B5ObjectStreamPcurve {
                    class,
                    surface: pcurve.surface,
                    parameter_range,
                    class_21_suffix_scalar: pcurve.class_21_suffix_scalar,
                    distinct_knots: &pcurve.distinct_knots,
                },
                "catia_b5_object_pcurve_index",
            )?;
        }
        Ok::<_, CodecError>(pcurves)
    })?;
    let offset_constructions = scratch.with_storage(|| {
        ctx.collect_vec(
            ctx.admit_iter(records, "catia_b5_offset_construction_scan")?
                .filter_map(parse_offset_surface_fields),
            "catia_b5_offset_constructions",
        )
    })?;
    let extrusion_offsets =
        scratch.with_storage(|| extrusion_offsets_by_carrier(ctx, &offset_constructions))?;
    let mut extrusion_surfaces = BTreeMap::<u32, B5ExtrusionSurface>::new();
    let mut pending_extrusions = scratch.with_storage(|| {
        ctx.collect_vec(
            ctx.admit_iter(records, "catia_b5_extrusion_candidate_scan")?
                .filter(|record| record.family == 0xb5 && record.class == 0x2c),
            "catia_b5_extrusion_candidates",
        )
    })?;
    // Each pass resolves the extrusions whose directrix context is complete;
    // a pass that resolves none ends the fixpoint.
    while !pending_extrusions.is_empty() {
        if budget.is_some_and(|budget| !budget.charge_by(pending_extrusions.len())) {
            return Ok(None);
        }
        let mut changed = false;
        for record in ctx.admit_iter(&pending_extrusions, "catia_b5_extrusion_fixpoint_scan")? {
            let Some(extrusion) = parse_extrusion_surface_with_context(
                ctx,
                record,
                by_id,
                &object_stream_pcurves,
                &extrusion_offsets,
                &extrusion_surfaces,
            )?
            else {
                continue;
            };
            ctx.insert_btree_map(
                &mut extrusion_surfaces,
                record.object_id,
                extrusion,
                "catia_b5_extrusion_surfaces",
            )?;
            changed = true;
        }
        if !changed {
            break;
        }
        ctx.retain_vec(
            &mut pending_extrusions,
            |record| {
                Ok(!ctx.contains_key_btree_map(
                    &extrusion_surfaces,
                    &record.object_id,
                    "catia_b5_resolved_extrusion_filter",
                )?)
            },
            "catia_b5_resolved_extrusion_filter",
        )?;
    }
    let extrusion_pcurves = scratch.with_storage(|| {
        let mut pcurves = HashSet::new();
        for (_, extrusion) in ctx.admit_iter(
            &extrusion_surfaces,
            "catia_b5_extrusion_surface_support_scan",
        )? {
            // A directrix has at most two supports.
            for (_, pcurve, _) in extrusion.directrix.supports() {
                ctx.insert_hash_set(&mut pcurves, *pcurve, "catia_b5_extrusion_pcurve_ids")?;
            }
        }
        Ok::<_, CodecError>(pcurves)
    })?;
    let mut offset_surfaces = BTreeMap::new();
    let mut supported_surfaces = BTreeMap::new();
    let (alias_records, supported_constructions) = scratch.with_storage(|| {
        let mut aliases = Vec::new();
        let mut supported = Vec::new();
        for record in ctx.admit_iter(records, "catia_b5_surface_fixpoint_candidate_scan")? {
            if surface_alias_target(record).is_some() {
                ctx.push_vec(&mut aliases, record, "catia_b5_surface_alias_records")?;
            } else if let Some(construction) = parse_supported_surface(record) {
                ctx.push_vec(
                    &mut supported,
                    construction,
                    "catia_b5_supported_surface_candidates",
                )?;
            }
        }
        Ok::<_, CodecError>((aliases, supported))
    })?;
    let mut fixpoint = !offset_constructions.is_empty()
        || !alias_records.is_empty()
        || !supported_constructions.is_empty();
    while fixpoint {
        if budget.is_some_and(|budget| {
            !budget.charge_by(
                offset_constructions.len() + alias_records.len() + supported_constructions.len(),
            )
        }) {
            return Ok(None);
        }
        let mut changed = resolve_surface_aliases(
            ctx,
            &alias_records,
            by_id,
            &mut surfaces,
            (&mut conflicting_surfaces, &mut scratch),
        )?;
        for offset in ctx.admit_iter(
            &offset_constructions,
            "catia_b5_offset_surface_fixpoint_scan",
        )? {
            if !offset_surface_agrees(ctx, offset, &surfaces, &extrusion_surfaces, by_id)? {
                continue;
            }
            // A resolved carrier is already its own entry; an unresolved one
            // enters as its opaque record.
            if !ctx.contains_key_btree_map(&surfaces, &offset.carrier_surface, LOOKUP)? {
                let Some(record) = record_by_id(ctx, by_id, offset.carrier_surface, LOOKUP)? else {
                    continue;
                };
                let carrier = B5Surface::Unknown {
                    family: record.family,
                    class: record.class,
                    payload: ctx.copy_slice(record.payload, "catia_b5_offset_carrier_payload")?,
                };
                if !merge_surface_candidate(
                    ctx,
                    &mut surfaces,
                    (&mut conflicting_surfaces, &mut scratch),
                    offset.carrier_surface,
                    carrier,
                )? {
                    continue;
                }
            }
            let Some(surface_changed) = merge_surface_from(
                ctx,
                &mut surfaces,
                (&mut conflicting_surfaces, &mut scratch),
                offset.object_id,
                offset.carrier_surface,
            )?
            else {
                continue;
            };
            let metadata_changed =
                ctx.get_btree_map(&offset_surfaces, &offset.object_id, LOOKUP)? != Some(offset);
            ctx.insert_btree_map(
                &mut offset_surfaces,
                offset.object_id,
                offset.clone(),
                "catia_b5_offset_surfaces",
            )?;
            changed |= surface_changed || metadata_changed;
        }
        for construction in ctx.admit_iter(
            &supported_constructions,
            "catia_b5_supported_surface_fixpoint_scan",
        )? {
            let Some(carrier) =
                ctx.get_btree_map(&surfaces, &construction.carrier_surface, LOOKUP)?
            else {
                continue;
            };
            let parameters_match_carrier =
                supported_surface_parameters_match_carrier(&construction.parameters, carrier);
            let Some(surface_changed) = merge_surface_from(
                ctx,
                &mut surfaces,
                (&mut conflicting_surfaces, &mut scratch),
                construction.object_id,
                construction.carrier_surface,
            )?
            else {
                continue;
            };
            if parameters_match_carrier
                && supported_surface_pcurves_match(
                    ctx,
                    construction,
                    by_id,
                    &object_stream_pcurve_candidates,
                )?
                && ctx.all_by(
                    &construction.support_surfaces,
                    |surface| ctx.contains_key_btree_map(&surfaces, surface, LOOKUP),
                    "catia_b5_supported_surface_support_scan",
                )?
            {
                let metadata_changed =
                    ctx.get_btree_map(&supported_surfaces, &construction.object_id, LOOKUP)?
                        != Some(construction);
                ctx.insert_btree_map(
                    &mut supported_surfaces,
                    construction.object_id,
                    construction.clone(),
                    "catia_b5_supported_surfaces",
                )?;
                changed |= surface_changed || metadata_changed;
            }
        }
        changed |= resolve_surface_aliases(
            ctx,
            &alias_records,
            by_id,
            &mut surfaces,
            (&mut conflicting_surfaces, &mut scratch),
        )?;
        fixpoint = changed;
    }
    ctx.retain_btree_map(
        &mut offset_surfaces,
        |object_id, offset| {
            same_surface_entries(
                ctx,
                &surfaces,
                *object_id,
                offset.carrier_surface,
                "catia_b5_offset_surface_carrier_check",
            )
        },
        "catia_b5_offset_surface_carrier_check",
    )?;
    ctx.retain_btree_map(
        &mut supported_surfaces,
        |object_id, construction| {
            same_surface_entries(
                ctx,
                &surfaces,
                *object_id,
                construction.carrier_surface,
                "catia_b5_supported_surface_carrier_check",
            )
        },
        "catia_b5_supported_surface_carrier_check",
    )?;
    let mut profiles = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_graph_profile_record_scan")? {
        if let Some(profile) = parse_profile(record) {
            ctx.insert_btree_map(
                &mut profiles,
                record.object_id,
                profile,
                "catia_b5_graph_profiles",
            )?;
        }
    }
    let mut pcurves = BTreeMap::new();
    let mut resolved_class_1a = BTreeSet::new();
    for record in ctx.admit_iter(records, "catia_b5_graph_pcurve_record_scan")? {
        let pcurve = match record.class {
            0x18 => parse_line_pcurve(ctx, record)?,
            0x19 => parse_circle_pcurve(ctx, record)?,
            0x1a => parse_class_1a_pcurve(ctx, record)?,
            0x21 => parse_pcurve(ctx, record)?,
            _ => None,
        };
        if let Some(pcurve) = pcurve {
            if record.class == 0x1a {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut resolved_class_1a,
                        record.object_id,
                        "catia_b5_resolved_class_1a_pcurves",
                    )
                })?;
            }
            ctx.insert_btree_map(
                &mut pcurves,
                record.object_id,
                pcurve,
                "catia_b5_graph_pcurves",
            )?;
        }
    }
    let mut conflicting_pcurves = HashSet::new();
    let mut circle_candidates = BTreeMap::<u32, Vec<B5Pcurve>>::new();
    for pcurve in circle_pcurves_from_frames(ctx, bytes, frames)? {
        if ctx.contains_key_btree_map(&surfaces, &pcurve.surface, LOOKUP)? {
            scratch.with_storage(|| {
                ctx.push_btree_group(
                    &mut circle_candidates,
                    pcurve.object_id,
                    pcurve,
                    "catia_b5_circle_candidate_groups",
                    "catia_b5_circle_candidates",
                )
            })?;
        }
    }
    for (object_id, mut candidates) in
        ctx.admit_iter(circle_candidates, "catia_b5_circle_candidate_group_scan")?
    {
        let Some((candidate, others)) = candidates.split_first() else {
            continue;
        };
        if ctx.all_by(
            others,
            |other| ctx.equal(other, candidate, "catia_b5_circle_candidate_equality_scan"),
            "catia_b5_circle_candidate_equality_scan",
        )? {
            let candidate = candidates.swap_remove(0);
            merge_pcurve_candidate(
                ctx,
                &mut pcurves,
                (&mut conflicting_pcurves, &mut scratch),
                candidate,
            )?;
        } else {
            ctx.remove_btree_map(
                &mut pcurves,
                &object_id,
                "catia_b5_conflicting_circle_pcurves",
            )?;
            scratch.with_storage(|| {
                ctx.insert_hash_set(
                    &mut conflicting_pcurves,
                    object_id,
                    "catia_b5_conflicting_circle_pcurves",
                )
            })?;
        }
    }
    drop(object_stream_pcurves);
    for (_, candidate) in ctx.admit_iter(
        object_stream_pcurve_candidates,
        "catia_b5_object_stream_pcurve_merge_scan",
    )? {
        let directrix_reference =
            ctx.contains_hash_set(&extrusion_pcurves, &candidate.object_id, LOOKUP)?;
        if !directrix_reference
            && record_by_id(ctx, by_id, candidate.object_id, LOOKUP)?
                .is_none_or(|record| record.class != 0x20)
        {
            continue;
        }
        merge_pcurve_candidate(
            ctx,
            &mut pcurves,
            (&mut conflicting_pcurves, &mut scratch),
            candidate,
        )?;
    }
    let mut opaque_pcurves = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_opaque_pcurve_record_scan")? {
        let resolved = record.class == 0x1a
            && ctx.contains_btree_set(&resolved_class_1a, &record.object_id, LOOKUP)?;
        if let Some(pcurve) = parse_opaque_pcurve(ctx, record, resolved)? {
            ctx.insert_btree_map(
                &mut opaque_pcurves,
                record.object_id,
                pcurve,
                "catia_b5_opaque_pcurves",
            )?;
        }
    }
    for (_, pcurve) in ctx.admit_iter(&mut opaque_pcurves, "catia_b5_opaque_pcurve_chart_scan")? {
        let Some(record) = record_by_id(ctx, by_id, pcurve.object_id, LOOKUP)? else {
            continue;
        };
        pcurve.sphere_great_circle = ctx
            .get_btree_map(&surfaces, &pcurve.surface, LOOKUP)?
            .and_then(|surface| parse_sphere_great_circle_pcurve(record, surface));
    }
    let mut parameter_incidences = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_parameter_incidence_record_scan")? {
        if let Some(incidence) = parameter_incidence(ctx, record)? {
            ctx.insert_btree_map(
                &mut parameter_incidences,
                record.object_id,
                incidence,
                "catia_b5_graph_parameter_incidences",
            )?;
        }
    }
    let mut edges = BTreeMap::new();
    for record in ctx
        .admit_iter(records, "catia_b5_edge_record_scan")?
        .filter(|record| record.class == 0x5e)
    {
        if let Some(edge) = parse_edge(record) {
            ctx.insert_btree_map(&mut edges, record.object_id, edge, "catia_b5_graph_edges")?;
        }
    }
    let mut edge_parameter_incidences = BTreeMap::new();
    for (&object_id, edge) in ctx.admit_iter(&edges, "catia_b5_edge_parameter_index_scan")? {
        let [start, end] = edge.parameter_incidences;
        if ctx.contains_key_btree_map(&parameter_incidences, &start, LOOKUP)?
            && ctx.contains_key_btree_map(&parameter_incidences, &end, LOOKUP)?
        {
            ctx.insert_btree_map(
                &mut edge_parameter_incidences,
                object_id,
                edge.parameter_incidences,
                "catia_b5_edge_parameter_index",
            )?;
        }
    }
    let mut parsed_loops = Vec::new();
    for record in ctx
        .admit_iter(records, "catia_b5_loop_record_scan")?
        .filter(|record| record.class == 0x62)
    {
        if let Some(parsed) = parse_loop_record(ctx, record)? {
            ctx.push_vec(&mut parsed_loops, parsed, "catia_b5_parsed_loops")?;
        }
    }
    let implicit_pcurves = implicit_pcurve_bindings(
        ctx,
        &parsed_loops,
        by_id,
        &edges,
        &parameter_incidences,
        (&pcurves, &opaque_pcurves, &surfaces),
    )?;
    for (_, pcurve) in ctx.admit_iter(&mut pcurves, "catia_b5_pcurve_endpoint_lift_scan")? {
        pcurve.lifted_endpoints = if let (Some(surface), Some(first), Some(last)) = (
            ctx.get_btree_map(&surfaces, &pcurve.surface, LOOKUP)?,
            pcurve.control_points.first(),
            pcurve.control_points.last(),
        ) {
            lift_pcurve_endpoints(ctx, surface, &profiles, [first.get(), last.get()])?
        } else {
            None
        };
    }
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    let source_face_count = ctx
        .admit_iter(records, "catia_b5_source_face_count_scan")?
        .filter(|record| record.class == 0x5f)
        .count();
    let mut loops = BTreeMap::new();
    for parsed in ctx.admit_iter(parsed_loops, "catia_b5_parsed_loop_scan")? {
        let object_id = parsed.object_id;
        let Some(loop_) = parse_loop(
            ctx,
            parsed,
            by_id,
            &pcurves,
            &opaque_pcurves,
            &implicit_pcurves,
            &surfaces,
        )?
        else {
            continue;
        };
        ctx.insert_btree_map(&mut loops, object_id, loop_, "catia_b5_graph_loops")?;
    }
    let mut face_records = BTreeMap::new();
    for record in ctx
        .admit_iter(records, "catia_b5_face_record_scan")?
        .filter(|record| record.class == 0x5f)
    {
        if let Some(face) = parse_face_record(ctx, record)? {
            ctx.insert_btree_map(
                &mut face_records,
                record.object_id,
                face,
                "catia_b5_graph_face_records",
            )?;
        }
    }
    let mut surface_aliases = BTreeMap::new();
    for record in ctx.admit_iter(&alias_records, "catia_b5_surface_alias_record_scan")? {
        let Some(target) = surface_alias_target(record) else {
            continue;
        };
        if ctx.contains_key_btree_map(&surfaces, &record.object_id, LOOKUP)? {
            ctx.insert_btree_map(
                &mut surface_aliases,
                record.object_id,
                target,
                "catia_b5_surface_aliases",
            )?;
        }
    }
    let mut faces = Vec::new();
    for record in ctx.admit_iter(records, "catia_b5_face_record_resolution_scan")? {
        let Some(face_record) = ctx.get_btree_map(&face_records, &record.object_id, LOOKUP)? else {
            continue;
        };
        if let Some(face) = parse_face(ctx, face_record, &loops, &surfaces, &surface_aliases)? {
            ctx.push_vec(&mut faces, face, "catia_b5_graph_faces")?;
        }
    }
    if require_topology && (faces.is_empty() || loops.is_empty()) {
        return Ok(None);
    }
    let vertex_points = crate::families::consolidated::records::object_stream_vertices(ctx, bytes)?;
    let geometric_edge_vertices = bind_edge_vertices(ctx, &loops, &geometry, &vertex_points)?;
    let mut vertex_incidence_links = BTreeMap::new();
    for record in ctx
        .admit_iter(records, "catia_b5_vertex_incidence_record_scan")?
        .filter(|record| record.class == 0x5d)
    {
        if let Some(link) = parse_vertex_incidence_link(record) {
            ctx.insert_btree_map(
                &mut vertex_incidence_links,
                record.object_id,
                link,
                "catia_b5_vertex_incidence_links",
            )?;
        }
    }
    let mut native_edge_vertices = BTreeMap::new();
    for (&object_id, edge) in ctx.admit_iter(&edges, "catia_b5_native_edge_vertex_scan")? {
        ctx.insert_btree_map(
            &mut native_edge_vertices,
            object_id,
            edge.vertices,
            "catia_b5_native_edge_vertices",
        )?;
    }
    let native_vertex_coordinates = incidence_vertex_coordinates(
        ctx,
        &native_edge_vertices,
        &vertex_incidence_links,
        by_id,
        &geometry,
    )?;
    let bound_vertices = bind_native_vertices(
        ctx,
        &loops,
        &geometry,
        &native_edge_vertices,
        &geometric_edge_vertices,
        &native_vertex_coordinates,
        &vertex_points,
    )?;
    let edge_vertices = bound_vertices.edges;
    let logical_vertices = bound_vertices.vertices;
    let vertex_tolerances = bound_vertices.tolerances;
    let referenced_loops = retain_referenced_loops(ctx, &faces, &mut loops)?;
    let mut complete = faces.len() == source_face_count;
    if complete {
        let owner_counts = face_loop_owner_counts(ctx, &faces)?;
        complete = ctx.all_by(
            &owner_counts,
            |(_, count)| Ok(*count == 1),
            "catia_b5_face_loop_owner_check",
        )?;
    }
    if complete {
        complete = ctx.all_by(
            &referenced_loops,
            |loop_id| {
                let Some(loop_) =
                    ctx.get_btree_map(&loops, loop_id, "catia_b5_referenced_loop_check")?
                else {
                    return Ok(false);
                };
                loop_resolves(
                    ctx,
                    loop_,
                    &pcurves,
                    &opaque_pcurves,
                    &implicit_pcurves,
                    &edge_vertices,
                )
            },
            "catia_b5_referenced_loop_check",
        )?;
    }
    let Some(vertices) = B5Vertices::try_new(ctx, vertex_points, logical_vertices, edge_vertices)?
    else {
        return Ok(None);
    };
    Ok(Some(B5Graph {
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
        vertices,
        edge_parameter_incidences,
        vertex_tolerances,
        profiles,
    }))
}

/// Adds a pcurve under its identity. A second, different candidate for the
/// same identity removes it and records the identity as conflicting.
fn merge_pcurve_candidate(
    ctx: &DecodeContext<'_>,
    pcurves: &mut BTreeMap<u32, B5Pcurve>,
    (conflicts, conflict_storage): (&mut HashSet<u32>, &mut ScopedReservation<'_>),
    candidate: B5Pcurve,
) -> Result<(), CodecError> {
    const OPERATION: &str = "catia_b5_pcurve_candidates";
    let object_id = candidate.object_id;
    if ctx.contains_hash_set(conflicts, &object_id, OPERATION)? {
        return Ok(());
    }
    match ctx.get_btree_map(pcurves, &object_id, OPERATION)? {
        None => {
            ctx.insert_btree_map(pcurves, object_id, candidate, OPERATION)?;
        }
        Some(existing) => {
            if !ctx.equal(existing, &candidate, OPERATION)? {
                ctx.remove_btree_map(pcurves, &object_id, OPERATION)?;
                conflict_storage.with_storage(|| {
                    ctx.insert_hash_set(conflicts, object_id, "catia_b5_conflicting_pcurves")
                })?;
            }
        }
    }
    Ok(())
}

/// Adds a surface under its identity. A resolved candidate replaces an
/// unresolved entry, an unresolved candidate never replaces one, and two
/// different resolved surfaces make the identity conflicting. Returns whether
/// the identity still holds a surface.
fn merge_surface_candidate(
    ctx: &DecodeContext<'_>,
    surfaces: &mut BTreeMap<u32, B5Surface>,
    (conflicts, conflict_storage): (&mut HashSet<u32>, &mut ScopedReservation<'_>),
    object_id: u32,
    candidate: B5Surface,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_b5_surface_candidates";
    if ctx.contains_hash_set(conflicts, &object_id, OPERATION)? {
        return Ok(false);
    }
    let replace = match ctx.get_btree_map(surfaces, &object_id, OPERATION)? {
        None => true,
        Some(existing) if unresolved_surface_candidate(existing) => true,
        Some(existing) => {
            if !unresolved_surface_candidate(&candidate)
                && !ctx.equal(existing, &candidate, OPERATION)?
            {
                ctx.remove_btree_map(surfaces, &object_id, OPERATION)?;
                conflict_storage.with_storage(|| {
                    ctx.insert_hash_set(conflicts, object_id, "catia_b5_conflicting_surfaces")
                })?;
                return Ok(false);
            }
            false
        }
    };
    if replace {
        ctx.insert_btree_map(surfaces, object_id, candidate, OPERATION)?;
    }
    Ok(true)
}

/// Merges the surface held by `source_id` into `object_id`, copying it only
/// when it is stored. Returns `None` when the identity conflicts or the source
/// holds no surface, and otherwise whether the stored surface changed.
fn merge_surface_from(
    ctx: &DecodeContext<'_>,
    surfaces: &mut BTreeMap<u32, B5Surface>,
    (conflicts, conflict_storage): (&mut HashSet<u32>, &mut ScopedReservation<'_>),
    object_id: u32,
    source_id: u32,
) -> Result<Option<bool>, CodecError> {
    const OPERATION: &str = "catia_b5_surface_candidates";
    if ctx.contains_hash_set(conflicts, &object_id, OPERATION)? {
        return Ok(None);
    }
    let Some(candidate) = ctx.get_btree_map(surfaces, &source_id, OPERATION)? else {
        return Ok(None);
    };
    let (replace, changed) = match ctx.get_btree_map(surfaces, &object_id, OPERATION)? {
        None => (true, true),
        Some(existing) => {
            let equal = ctx.equal(existing, candidate, OPERATION)?;
            if unresolved_surface_candidate(existing) {
                (!equal, !equal)
            } else if unresolved_surface_candidate(candidate) || equal {
                (false, !equal)
            } else {
                ctx.remove_btree_map(surfaces, &object_id, OPERATION)?;
                conflict_storage.with_storage(|| {
                    ctx.insert_hash_set(conflicts, object_id, "catia_b5_conflicting_surfaces")
                })?;
                return Ok(None);
            }
        }
    };
    if replace {
        let copy = copy_surface(ctx, candidate)?;
        ctx.insert_btree_map(surfaces, object_id, copy, OPERATION)?;
    }
    Ok(Some(changed))
}

fn unresolved_surface_candidate(surface: &B5Surface) -> bool {
    matches!(
        surface,
        B5Surface::Unknown { .. } | B5Surface::UnresolvedNurbs { .. }
    )
}

fn copy_surface(ctx: &DecodeContext<'_>, surface: &B5Surface) -> Result<B5Surface, CodecError> {
    Ok(match surface {
        B5Surface::UnresolvedNurbs { header, payload } => B5Surface::UnresolvedNurbs {
            header: header.copy_charged(ctx)?,
            payload: ctx.copy_slice(payload, "catia_b5_copied_unresolved_surface_payload")?,
        },
        B5Surface::Unknown {
            family,
            class,
            payload,
        } => B5Surface::Unknown {
            family: *family,
            class: *class,
            payload: ctx.copy_slice(payload, "catia_b5_copied_unknown_surface_payload")?,
        },
        B5Surface::Nurbs(nurbs) => {
            B5Surface::Nurbs(nurbs.try_clone_for_decode(ctx, "catia_b5_copied_nurbs_surface")?)
        }
        B5Surface::RollingBall {
            carrier_object_id,
            definition,
        } => {
            let ProceduralSurfaceDefinition::RollingBallJet(jet) = definition.as_ref() else {
                return Ok(surface.clone());
            };
            let stations =
                ctx.copy_slice(jet.stations(), "catia_b5_copied_rolling_ball_stations")?;
            let jet = cadmpeg_ir::geometry::RollingBallJetStations::from_parts(
                jet.degree(),
                stations,
                ctx,
            )?
            .map_err(CodecError::malformed)?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    ProceduralSurfaceDefinition,
                >()),
                "catia_b5_copied_rolling_ball_definition",
            )?;
            B5Surface::RollingBall {
                carrier_object_id: *carrier_object_id,
                definition: Box::new(ProceduralSurfaceDefinition::RollingBallJet(jet)),
            }
        }
        other => other.clone(),
    })
}

/// Merges each alias identity with the resolved surface its chain ends at.
/// Returns whether any alias entry changed.
fn resolve_surface_aliases(
    ctx: &DecodeContext<'_>,
    alias_records: &[&B5Record<'_>],
    by_id: &HashMap<u32, &B5Record<'_>>,
    surfaces: &mut BTreeMap<u32, B5Surface>,
    (conflicts, conflict_storage): (&mut HashSet<u32>, &mut ScopedReservation<'_>),
) -> Result<bool, CodecError> {
    let mut changed = false;
    for record in ctx.admit_iter(alias_records, "catia_b5_surface_alias_record_scan")? {
        let Some(terminal) =
            surface_alias_terminal(ctx, record.object_id, by_id, surfaces, alias_records.len())?
        else {
            continue;
        };
        changed |= merge_surface_from(
            ctx,
            surfaces,
            (&mut *conflicts, &mut *conflict_storage),
            record.object_id,
            terminal,
        )?
        .unwrap_or(false);
    }
    Ok(changed)
}

/// Follows alias records from `object_id` to the identity that ends the
/// chain, when that identity holds a resolved surface. Every step but the last
/// passes an alias record, so a chain longer than `alias_count` repeats one
/// and is refused. Each step charges its own record lookup.
fn surface_alias_terminal(
    ctx: &DecodeContext<'_>,
    mut object_id: u32,
    by_id: &HashMap<u32, &B5Record<'_>>,
    surfaces: &BTreeMap<u32, B5Surface>,
    alias_count: usize,
) -> Result<Option<u32>, CodecError> {
    const OPERATION: &str = "catia_b5_surface_alias_step";
    for _ in 0..=alias_count {
        let target = record_by_id(ctx, by_id, object_id, OPERATION)?.and_then(surface_alias_target);
        let Some(target) = target else {
            let resolved = ctx
                .get_btree_map(surfaces, &object_id, OPERATION)?
                .is_some_and(|surface| !unresolved_surface_candidate(surface));
            return Ok(resolved.then_some(object_id));
        };
        object_id = target;
    }
    Ok(None)
}

fn object_stream_pcurve_candidate(
    ctx: &DecodeContext<'_>,
    jet: &crate::families::a5a8::records::A8Pcurve,
) -> Result<Option<B5Pcurve>, CodecError> {
    let Some((_, control_points)) = jet.bspline(ctx)? else {
        return Ok(None);
    };
    Ok(Some(B5Pcurve {
        object_id: jet.object_id,
        surface: jet.support_id,
        degree: crate::families::a5a8::records::A8Pcurve::DEGREE,
        distinct_knots: jet.knots(ctx)?,
        multiplicities: ctx.alloc_filled(
            jet.sites.len(),
            crate::families::a5a8::records::A8Pcurve::DEGREE + 1,
            "catia_B5_object_pcurve_multiplicities",
        )?,
        control_points,
        weights: None,
        parameter_range: Some(jet.range),
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    }))
}

#[cfg(test)]
fn a8_class21_pcurves(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<B5Pcurve>, CodecError> {
    let frames = collect_object_stream_frames(ctx, bytes)?;
    a8_class21_pcurves_from_frames(ctx, bytes, &frames)
}

fn a8_class21_pcurves_from_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frames: &[ObjectFrame],
) -> Result<Vec<B5Pcurve>, CodecError> {
    let mut pcurves = Vec::new();
    for frame in ctx.admit_iter(frames, "catia_b5_class21_frame_scan")? {
        if frame.family != 0xa8 || frame.class != 0x21 {
            continue;
        }
        if let Some(pcurve) =
            parse_a8_class21_pcurve(ctx, frame.object_id, &bytes[frame.start + 11..frame.end])?
        {
            ctx.push_vec(&mut pcurves, pcurve, "catia B5 class-21 pcurves")?;
        }
    }
    Ok(pcurves)
}

fn parse_a8_class21_pcurve(
    ctx: &DecodeContext<'_>,
    object_id: u32,
    payload: &[u8],
) -> Result<Option<B5Pcurve>, CodecError> {
    (|| -> Option<Result<B5Pcurve, CodecError>> {
        (payload.first() == Some(&0x81)).then_some(())?;
        let mut position = 1;
        let surface = wire::tokens::object_ref(payload, &mut position, true)?;
        (payload.get(position) == Some(&0x01)).then_some(())?;
        position += 1;
        let degree = wire::tokens::compact_uint(payload, &mut position)?;
        (degree == 5 && payload.get(position..position + 2) == Some(&[0x01, 0x01])).then_some(())?;
        position += 2;
        let knot_count =
            usize::try_from(wire::tokens::compact_uint(payload, &mut position)?).ok()?;
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
        let Some(scalar_width) = NonZeroUsize::new(8) else {
            return None;
        };
        let read_values = |position: &mut usize, values: &mut Vec<FiniteReal>| {
            let Some(end) = (*position).checked_add(scalar_bytes) else {
                return Some(Err(ctx.refuse_codec_limit(
                    "catia_b5_a8_class21_scalar_lane_bytes",
                    u64::MAX,
                    u64::MAX,
                )));
            };
            let lane = payload.get(*position..end)?;
            let mut chunks = match ctx.admit_iter(lane, "catia_b5_a8_class21_scalar_lane_bytes") {
                Ok(admitted) => admitted.chunks(scalar_width),
                Err(error) => return Some(Err(error.into())),
            };
            for chunk in &mut chunks {
                if let Err(error) =
                    ctx.push_vec(values, f64_le(chunk, 0)?, "catia B5 pcurve distinct knots")
                {
                    return Some(Err(error));
                }
            }
            *position = end;
            Some(Ok(()))
        };
        let mut distinct_knots = Vec::new();
        if let Err(error) = ctx.reserve_capacity(
            &mut distinct_knots,
            knot_count,
            "catia B5 pcurve distinct knots",
        ) {
            return Some(Err(error));
        }
        match read_values(&mut position, &mut distinct_knots) {
            Some(Ok(())) => {}
            Some(Err(error)) => return Some(Err(error)),
            None => return None,
        }
        // The knot and jet lanes are scratch for the B-spline fit.
        let mut scratch = match ctx.reserve_scoped(0, "catia_b5_a8_class21_jet_lanes") {
            Ok(scratch) => scratch,
            Err(error) => return Some(Err(error)),
        };
        let knot_values = match scratch.with_storage(|| {
            ctx.collect_vec(
                distinct_knots.iter().copied().map(FiniteReal::get),
                "catia B5 pcurve knot values",
            )
        }) {
            Ok(values) => values,
            Err(error) => return Some(Err(error)),
        };
        match knots_strictly_increasing(&knot_values, |count| {
            ctx.charge_work(count, "IR strict knot order")
        }) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(error) => return Some(Err(error)),
        }
        let mut multiplicities_valid = true;
        let multiplicity_indices =
            match ctx.admit_iter(&(0..knot_count), "catia_b5_a8_class21_multiplicity_scan") {
                Ok(indices) => indices,
                Err(error) => return Some(Err(error.into())),
            };
        for index in multiplicity_indices {
            let multiplicity = wire::tokens::compact_uint(payload, &mut position)?;
            multiplicities_valid &= multiplicity
                == if index == 0 || index + 1 == knot_count {
                    degree + 1
                } else {
                    3
                };
        }
        multiplicities_valid.then_some(())?;
        let read_lane = |position: &mut usize, values: &mut Vec<f64>| {
            let Some(end) = (*position).checked_add(scalar_bytes) else {
                return Some(Err(ctx.refuse_codec_limit(
                    "catia_b5_a8_class21_scalar_lane_bytes",
                    u64::MAX,
                    u64::MAX,
                )));
            };
            let lane = payload.get(*position..end)?;
            let mut chunks = match ctx.admit_iter(lane, "catia_b5_a8_class21_scalar_lane_bytes") {
                Ok(admitted) => admitted.chunks(scalar_width),
                Err(error) => return Some(Err(error.into())),
            };
            for chunk in &mut chunks {
                values.push(f64_le(chunk, 0)?.get());
            }
            *position = end;
            Some(Ok(()))
        };
        let mut u = Vec::new();
        if let Err(error) =
            ctx.reserve_scoped_vec(&mut scratch, &mut u, knot_count, "catia B5 pcurve u jet")
        {
            return Some(Err(error));
        }
        match read_lane(&mut position, &mut u) {
            Some(Ok(())) => {}
            Some(Err(error)) => return Some(Err(error)),
            None => return None,
        }
        let mut v = Vec::new();
        if let Err(error) =
            ctx.reserve_scoped_vec(&mut scratch, &mut v, knot_count, "catia B5 pcurve v jet")
        {
            return Some(Err(error));
        }
        match read_lane(&mut position, &mut v) {
            Some(Ok(())) => {}
            Some(Err(error)) => return Some(Err(error)),
            None => return None,
        }
        let mut du = Vec::new();
        if let Err(error) =
            ctx.reserve_scoped_vec(&mut scratch, &mut du, knot_count, "catia B5 pcurve du jet")
        {
            return Some(Err(error));
        }
        match read_lane(&mut position, &mut du) {
            Some(Ok(())) => {}
            Some(Err(error)) => return Some(Err(error)),
            None => return None,
        }
        let mut dv = Vec::new();
        if let Err(error) =
            ctx.reserve_scoped_vec(&mut scratch, &mut dv, knot_count, "catia B5 pcurve dv jet")
        {
            return Some(Err(error));
        }
        match read_lane(&mut position, &mut dv) {
            Some(Ok(())) => {}
            Some(Err(error)) => return Some(Err(error)),
            None => return None,
        }
        let mut ddu = Vec::new();
        if let Err(error) = ctx.reserve_scoped_vec(
            &mut scratch,
            &mut ddu,
            knot_count,
            "catia B5 pcurve ddu jet",
        ) {
            return Some(Err(error));
        }
        match read_lane(&mut position, &mut ddu) {
            Some(Ok(())) => {}
            Some(Err(error)) => return Some(Err(error)),
            None => return None,
        }
        let mut ddv = Vec::new();
        if let Err(error) = ctx.reserve_scoped_vec(
            &mut scratch,
            &mut ddv,
            knot_count,
            "catia B5 pcurve ddv jet",
        ) {
            return Some(Err(error));
        }
        match read_lane(&mut position, &mut ddv) {
            Some(Ok(())) => {}
            Some(Err(error)) => return Some(Err(error)),
            None => return None,
        }
        let mut points = Vec::new();
        if let Err(error) = ctx.reserve_scoped_vec(
            &mut scratch,
            &mut points,
            knot_count,
            "catia B5 pcurve point jets",
        ) {
            return Some(Err(error));
        }
        points.extend(u.into_iter().zip(v).map(|(u, v)| [u, v]));
        let mut first = Vec::new();
        if let Err(error) = ctx.reserve_scoped_vec(
            &mut scratch,
            &mut first,
            knot_count,
            "catia B5 pcurve first jets",
        ) {
            return Some(Err(error));
        }
        first.extend(du.into_iter().zip(dv).map(|(u, v)| [u, v]));
        let mut second = Vec::new();
        if let Err(error) = ctx.reserve_scoped_vec(
            &mut scratch,
            &mut second,
            knot_count,
            "catia B5 pcurve second jets",
        ) {
            return Some(Err(error));
        }
        second.extend(ddu.into_iter().zip(ddv).map(|(u, v)| [u, v]));
        let (_, control_points) = match crate::nurbs::quintic_jet_bspline(
            ctx,
            degree,
            &knot_values,
            &points,
            &first,
            &second,
            cadmpeg_ir::units::FiniteVector::new,
        ) {
            Ok(Some(curve)) => curve,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let tail = payload.get(position..)?;
        let tail_control = tail.get(..2);
        let extension_control = tail.get(34..36);
        (matches!(tail.len(), 36 | 38)
            && (tail_control == Some(&[0x05, 0x05]) || tail_control == Some(&[0x05, 0x11]))
            && f64_le(tail, 2)?.get() == 0.0
            && f64_le(tail, 18)?.get() == 1.0
            && f64_le(tail, 26)?.get() == 0.0
            && (tail.len() == 36
                || extension_control == Some(&[0x01, 0x11])
                || extension_control == Some(&[0x01, 0x19]))
            && tail.get(tail.len() - 2..) == Some(&[0x00, 0x07]))
        .then_some(())?;
        let parameter_range = [*distinct_knots.first()?, *distinct_knots.last()?];
        let multiplicities =
            match ctx.alloc_filled(knot_count, degree + 1, "catia B5 pcurve multiplicities") {
                Ok(multiplicities) => multiplicities,
                Err(error) => return Some(Err(error)),
            };
        Some(Ok(B5Pcurve {
            object_id,
            surface,
            degree,
            distinct_knots,
            multiplicities,
            control_points,
            weights: None,
            parameter_range: Some(parameter_range),
            parameterization: B5PcurveParameterization::Native,
            class_21_suffix_scalar: Some(PositiveReal::new(f64_le(tail, 10)?.get())?),
            lifted_endpoints: None,
        }))
    })()
    .transpose()
}

/// Return native start/end vertex identities for every framed `b5 03 5e`
/// edge, keyed by the edge object id.
pub(in crate::families) fn edge_vertex_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<BTreeMap<u32, [u32; 2]>, CodecError> {
    const AMBIGUOUS: &str = "catia_b5_ambiguous_edge_vertices";
    let mut edges = BTreeMap::new();
    let mut scratch = ctx.reserve_scoped(0, AMBIGUOUS)?;
    let mut ambiguous = HashSet::new();
    for frame in object_stream_frames(ctx, bytes)? {
        let frame = frame?;
        if frame.family != 0xb5 || frame.class != 0x5e {
            continue;
        }
        let Some(edge) = record_from_frame(bytes, &frame).and_then(|record| parse_edge(&record))
        else {
            continue;
        };
        let vertices = edge.vertices;
        if ctx
            .insert_btree_map(
                &mut edges,
                frame.object_id,
                vertices,
                "catia_b5_edge_vertex_references",
            )?
            .is_some_and(|existing| existing != vertices)
        {
            scratch
                .with_storage(|| ctx.insert_hash_set(&mut ambiguous, frame.object_id, AMBIGUOUS))?;
        }
    }
    if !ambiguous.is_empty() {
        ctx.retain_btree_map(
            &mut edges,
            |object_id, _| Ok(!ctx.contains_hash_set(&ambiguous, object_id, AMBIGUOUS)?),
            AMBIGUOUS,
        )?;
    }
    Ok(edges)
}

/// Return the ordered pcurve pair owned by each requested native edge's
/// class-`23` curve-support wrapper.
#[cfg(test)]
fn edge_support_pcurve_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_ids: &HashSet<u32>,
) -> Result<BTreeMap<u32, [u32; 2]>, CodecError> {
    let frames = collect_object_stream_frames(ctx, bytes)?;
    edge_support_pcurve_references_from_frames(ctx, bytes, edge_ids, &frames)
}

pub(in crate::families) fn edge_support_pcurve_references_from_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_ids: &HashSet<u32>,
    frames: &[ObjectFrame],
) -> Result<BTreeMap<u32, [u32; 2]>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_edge_support_index")?;
    let (edge_wrappers, wrappers) = scratch.with_storage(|| {
        let mut edge_wrappers = BTreeMap::<u32, Option<u32>>::new();
        let mut wrappers = HashMap::<u32, Option<[u32; 2]>>::new();
        for frame in ctx.admit_iter(frames, "catia_b5_edge_support_frame_scan")? {
            let requested_edge = frame.class == 0x5e
                && ctx.contains_hash_set(
                    edge_ids,
                    &frame.object_id,
                    "catia_b5_edge_support_edge_ids",
                )?;
            let wrapper_frame = frame.family == 0xb5 && frame.class == 0x23;
            if !(requested_edge || wrapper_frame) {
                continue;
            }
            let Some(record) = record_from_frame(bytes, frame) else {
                continue;
            };
            if requested_edge {
                let Some(wrapper) = parse_edge(&record).map(|edge| edge.support) else {
                    continue;
                };
                ctx.entry_btree_map(
                    &mut edge_wrappers,
                    frame.object_id,
                    "catia_b5_edge_support_wrappers",
                )?
                .and_modify(|stored| {
                    if stored.is_some_and(|stored| stored != wrapper) {
                        *stored = None;
                    }
                })
                .or_insert(Some(wrapper));
            } else {
                // A wrapper holds exactly two references, so three fixed
                // token reads decide it.
                let mut references = record_references(&record);
                let (Some(first), Some(second), None) =
                    (references.next(), references.next(), references.next())
                else {
                    continue;
                };
                let references = [first, second];
                ctx.entry_hash_map(
                    &mut wrappers,
                    frame.object_id,
                    "catia_b5_pcurve_support_wrappers",
                )?
                .and_modify(|stored| {
                    if stored.is_some_and(|stored| stored != references) {
                        *stored = None;
                    }
                })
                .or_insert(Some(references));
            }
        }
        Ok::<_, CodecError>((edge_wrappers, wrappers))
    })?;
    let mut resolved = BTreeMap::new();
    for (&edge, &wrapper) in ctx.admit_iter(&edge_wrappers, "catia_b5_edge_support_wrapper_scan")? {
        let Some(wrapper) = wrapper else {
            continue;
        };
        if let Some(references) = ctx
            .get_hash_map(&wrappers, &wrapper, "catia_b5_edge_support_wrapper_lookup")?
            .and_then(Option::as_ref)
        {
            ctx.insert_btree_map(
                &mut resolved,
                edge,
                *references,
                "catia_b5_edge_support_pcurves",
            )?;
        }
    }
    Ok(resolved)
}

/// Index each identity's framed record; an identity framed twice with
/// different bytes keeps no record.
fn index_unique_records<'a>(
    ctx: &DecodeContext<'_>,
    records: &mut HashMap<u32, Option<B5Record<'a>>>,
    record: B5Record<'a>,
    operation: &'static str,
) -> Result<(), CodecError> {
    match ctx.get_mut_hash_map(records, &record.object_id, operation)? {
        Some(stored) => {
            let differs = match stored {
                Some(stored) => {
                    stored.family != record.family
                        || stored.class != record.class
                        || !ctx.equal_bytes(stored.payload, record.payload, operation)?
                }
                None => false,
            };
            if differs {
                *stored = None;
            }
        }
        None => {
            ctx.insert_hash_map(records, record.object_id, Some(record), operation)?;
        }
    }
    Ok(())
}

pub(in crate::families) fn targeted_surfaces_from_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    object_ids: &BTreeSet<u32>,
    frames: &[ObjectFrame],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<BTreeMap<u32, B5Surface>, CodecError> {
    // The candidate indexes are scratch; each target copies what it resolves.
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_targeted_surface_index")?;
    let resolved = scratch.with_storage(|| {
        let mut resolved = HashMap::<u32, Option<B5Surface>>::new();
        let mut external_grids = crate::families::a5a8::records::A8ExternalGridSites::default();
        for frame in ctx.admit_iter(frames, "catia_b5_targeted_surface_frame_scan")? {
            let Some(surface) =
                crate::families::a5a8::records::resolved_a8_surface_from_object_frame(
                    ctx,
                    bytes,
                    frame.start,
                    frame.end,
                    frame.object_id,
                    &mut external_grids,
                    refusal,
                )?
            else {
                continue;
            };
            let Some(object_id) = surface.object_id() else {
                continue;
            };
            merge_targeted_surface(
                ctx,
                &mut resolved,
                object_id,
                B5Surface::Nurbs(surface.geometry),
            )?;
        }
        Ok::<_, CodecError>(resolved)
    })?;
    let (headers, records, rolling) = scratch.with_storage(|| {
        let mut headers = BTreeMap::new();
        for frame in ctx.admit_iter(frames, "catia_b5_targeted_surface_header_scan")? {
            if let Some(header) =
                crate::families::a5a8::records::a8_surface_header_from_object_frame(
                    ctx,
                    bytes,
                    frame.start,
                    frame.end,
                    frame.object_id,
                )?
            {
                ctx.insert_btree_map(
                    &mut headers,
                    header.object_id,
                    header,
                    "catia_b5_targeted_a8_headers",
                )?;
            }
        }
        let mut records = HashMap::<u32, Option<B5Record<'_>>>::new();
        for frame in ctx.admit_iter(frames, "catia_b5_targeted_surface_record_scan")? {
            if !is_surface_class(frame.class) {
                continue;
            }
            if let Some(record) = record_from_frame(bytes, frame) {
                index_unique_records(
                    ctx,
                    &mut records,
                    record,
                    "catia_b5_targeted_surface_records",
                )?;
            }
        }
        let mut rolling = HashMap::<u32, Option<B5Surface>>::new();
        let jets = crate::families::a5a8::records::a8_freeform_curves(ctx, bytes)?;
        for jet in ctx.admit_iter(&jets, "catia_b5_targeted_rolling_ball_jet_scan")? {
            let Some(definition) =
                crate::families::a5a8::records::rolling_ball_jet_definition(ctx, jet)?
            else {
                continue;
            };
            ctx.charge_retained(
                u64_from_index(std::mem::size_of::<ProceduralSurfaceDefinition>()),
                "catia_b5_rolling_ball_definition_box",
            )?;
            merge_targeted_surface(
                ctx,
                &mut rolling,
                jet.object_id,
                B5Surface::RollingBall {
                    carrier_object_id: jet.object_id,
                    definition: Box::new(definition),
                },
            )?;
        }
        Ok::<_, CodecError>((headers, records, rolling))
    })?;
    let mut surfaces = BTreeMap::new();
    for &object_id in ctx.admit_iter(object_ids, "catia_b5_targeted_surface_id_scan")? {
        if let Some(surface) =
            resolve_targeted_surface(ctx, object_id, &records, &headers, &resolved, &rolling)?
        {
            ctx.insert_btree_map(
                &mut surfaces,
                object_id,
                surface,
                "catia_b5_targeted_surfaces",
            )?;
        }
    }
    Ok(surfaces)
}

/// Resolve the unique length-closed geometry construction frames independently
/// of the dominant topology run.
#[cfg(test)]
fn targeted_geometry_graph(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<B5Graph>, CodecError> {
    let frames = collect_object_stream_frames(ctx, bytes)?;
    targeted_geometry_graph_from_frames(ctx, bytes, &frames, &mut crate::nurbs::LaneRefusals::new())
}

pub(in crate::families) fn targeted_geometry_graph_from_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frames: &[ObjectFrame],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<B5Graph>, CodecError> {
    const OPERATION: &str = "catia_b5_targeted_geometry_candidates";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let records = scratch.with_storage(|| {
        let mut candidates = HashMap::<u32, Option<B5Record<'_>>>::new();
        let mut order = BTreeMap::new();
        for frame in ctx.admit_iter(frames, "catia_b5_targeted_geometry_frame_scan")? {
            if !is_targeted_geometry_class(frame.family, frame.class) {
                continue;
            }
            let Some(record) = record_from_frame(bytes, frame) else {
                continue;
            };
            if !ctx.contains_key_hash_map(&candidates, &record.object_id, OPERATION)? {
                ctx.insert_btree_map(&mut order, record.offset, record.object_id, OPERATION)?;
            }
            index_unique_records(ctx, &mut candidates, record, OPERATION)?;
        }
        let mut records = Vec::new();
        for (_, object_id) in ctx.admit_iter(&order, "catia_b5_targeted_geometry_records")? {
            if let Some(record) = ctx
                .get_hash_map(&candidates, object_id, OPERATION)?
                .copied()
                .flatten()
            {
                ctx.push_vec(&mut records, record, "catia_b5_targeted_geometry_records")?;
            }
        }
        Ok::<_, CodecError>(records)
    })?;
    parse_from_records(ctx, bytes, &records, frames, false, refusal)
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
    ctx: &DecodeContext<'_>,
    candidates: &mut HashMap<u32, Option<B5Surface>>,
    object_id: u32,
    surface: B5Surface,
) -> Result<(), CodecError> {
    const OPERATION: &str = "catia_b5_targeted_surface_candidates";
    match ctx.get_mut_hash_map(candidates, &object_id, OPERATION)? {
        Some(stored) => {
            let differs = match stored.as_ref() {
                Some(existing) => !ctx.equal(existing, &surface, OPERATION)?,
                None => false,
            };
            if differs {
                *stored = None;
            }
        }
        None => {
            ctx.insert_hash_map(candidates, object_id, Some(surface), OPERATION)?;
        }
    }
    Ok(())
}

fn resolve_targeted_surface(
    ctx: &DecodeContext<'_>,
    object_id: u32,
    records: &HashMap<u32, Option<B5Record<'_>>>,
    headers: &BTreeMap<u32, crate::families::a5a8::records::A8SurfaceHeader>,
    resolved: &HashMap<u32, Option<B5Surface>>,
    rolling: &HashMap<u32, Option<B5Surface>>,
) -> Result<Option<B5Surface>, CodecError> {
    resolve_targeted_surface_inner(
        ctx,
        object_id,
        records,
        headers,
        resolved,
        rolling,
        (
            BTreeSet::new(),
            ctx.reserve_scoped(0, "catia_b5_targeted_surface_visited")?,
        ),
    )
}

fn resolve_targeted_surface_inner(
    ctx: &DecodeContext<'_>,
    mut object_id: u32,
    records: &HashMap<u32, Option<B5Record<'_>>>,
    headers: &BTreeMap<u32, crate::families::a5a8::records::A8SurfaceHeader>,
    resolved: &HashMap<u32, Option<B5Surface>>,
    rolling: &HashMap<u32, Option<B5Surface>>,
    (mut visited, mut visited_storage): (BTreeSet<u32>, ScopedReservation<'_>),
) -> Result<Option<B5Surface>, CodecError> {
    const OPERATION: &str = "catia_b5_targeted_surface_lookup";
    let _depth = ctx.enter_nested("catia_b5_targeted_surface_resolution")?;
    loop {
        if !visited_storage.with_storage(|| {
            ctx.insert_btree_set(&mut visited, object_id, "catia_b5_targeted_surface_visited")
        })? {
            return Ok(None);
        }
        let record = match ctx.get_hash_map(records, &object_id, OPERATION)? {
            // An identity framed twice with different bytes is ambiguous.
            Some(None) => return Ok(None),
            Some(Some(record)) => Some(record),
            None => None,
        };
        let rolling_surface = ctx
            .get_hash_map(rolling, &object_id, OPERATION)?
            .and_then(Option::as_ref);
        let resolved_surface = ctx
            .get_hash_map(resolved, &object_id, OPERATION)?
            .and_then(Option::as_ref);
        match (rolling_surface, resolved_surface) {
            (Some(left), Some(right)) if !ctx.equal(left, right, OPERATION)? => return Ok(None),
            (Some(surface), _) | (_, Some(surface)) => return copy_surface(ctx, surface).map(Some),
            (None, None) => {}
        }
        let Some(record) = record else {
            return Ok(None);
        };
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
                ctx, record, records, headers, resolved, rolling, &visited,
            );
        }
        return surface_node(ctx, record, headers);
    }
}

/// Copies the visited set into scratch storage that the recursive call owns
/// and releases when it returns.
fn copy_visited<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    visited: &BTreeSet<u32>,
) -> Result<(BTreeSet<u32>, ScopedReservation<'ctx>), CodecError> {
    const OPERATION: &str = "catia_b5_targeted_visited_copy";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let copy = storage.with_storage(|| {
        ctx.collect_btree_set(ctx.admit_iter(visited, OPERATION)?.copied(), OPERATION)
    })?;
    Ok((copy, storage))
}

fn resolve_targeted_analytic_offset(
    ctx: &DecodeContext<'_>,
    record: &B5Record<'_>,
    records: &HashMap<u32, Option<B5Record<'_>>>,
    headers: &BTreeMap<u32, crate::families::a5a8::records::A8SurfaceHeader>,
    resolved: &HashMap<u32, Option<B5Surface>>,
    rolling: &HashMap<u32, Option<B5Surface>>,
    visited: &BTreeSet<u32>,
) -> Result<Option<B5Surface>, CodecError> {
    const OPERATION: &str = "catia_b5_targeted_offset_surfaces";
    if record.payload.first() != Some(&0x82) {
        return Ok(None);
    }
    let mut position = 1;
    let Some(carrier_id) = wire::tokens::object_ref(record.payload, &mut position, true) else {
        return Ok(None);
    };
    let Some(source_id) = wire::tokens::object_ref(record.payload, &mut position, true) else {
        return Ok(None);
    };
    let Some(carrier) = resolve_targeted_surface_inner(
        ctx,
        carrier_id,
        records,
        headers,
        resolved,
        rolling,
        copy_visited(ctx, visited)?,
    )?
    else {
        return Ok(None);
    };
    let rolling_carrier = matches!(carrier, B5Surface::RollingBall { .. });
    // The two-entry context holds the resolved surfaces themselves; the
    // carrier is taken back out once the construction agrees with it.
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut surfaces = BTreeMap::new();
    storage.with_storage(|| ctx.insert_btree_map(&mut surfaces, carrier_id, carrier, OPERATION))?;
    if !rolling_carrier {
        let Some(source) = resolve_targeted_surface_inner(
            ctx,
            source_id,
            records,
            headers,
            resolved,
            rolling,
            copy_visited(ctx, visited)?,
        )?
        else {
            return Ok(None);
        };
        storage
            .with_storage(|| ctx.insert_btree_map(&mut surfaces, source_id, source, OPERATION))?;
    }
    if parse_offset_surface(ctx, record, &surfaces, &BTreeMap::new(), &HashMap::new())?.is_none() {
        return Ok(None);
    }
    ctx.remove_btree_map(&mut surfaces, &carrier_id, OPERATION)
}

fn parse_edge(record: &B5Record) -> Option<B5Edge> {
    (record.class == 0x5e && record.payload.first() == Some(&0x85)).then_some(())?;
    let mut position = 1;
    let references = [
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
    ];
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
    ctx: &DecodeContext<'_>,
    native_edges: &BTreeMap<u32, [u32; 2]>,
    vertex_incidence_links: &BTreeMap<u32, B5VertexIncidenceLink>,
    by_id: &HashMap<u32, &B5Record<'_>>,
    geometry: &B5PcurveContext<'_>,
) -> Result<BTreeMap<u32, FinitePoint3>, CodecError> {
    const OPERATION: &str = "catia_b5_incidence_vertex_lookup";
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_incidence_vertex_scratch")?;
    let mut seen = HashSet::new();
    let mut coordinates_by_vertex = BTreeMap::new();
    for (_, edge_vertices) in ctx.admit_iter(native_edges, "catia_b5_incidence_edge_vertex_scan")? {
        for &vertex in edge_vertices {
            if !scratch.with_storage(|| {
                ctx.insert_hash_set(&mut seen, vertex, "catia_b5_incidence_vertex_seen")
            })? {
                continue;
            }
            let Some(link) = ctx.get_btree_map(vertex_incidence_links, &vertex, OPERATION)? else {
                continue;
            };
            let Some(roster_record) = record_by_id(ctx, by_id, link.incidence, OPERATION)? else {
                continue;
            };
            let Some(incidence_records) =
                scratch.with_storage(|| counted_references(ctx, roster_record, 0x05))?
            else {
                continue;
            };
            let mut first = None;
            let mut valid = true;
            let tolerance_squared = POINT_TOLERANCE * POINT_TOLERANCE;
            'incidences: for &incidence_record in
                ctx.admit_iter(&incidence_records, "catia_b5_incidence_record_scan")?
            {
                let Some(incidence) =
                    ctx.get_btree_map(geometry.parameter_incidences, &incidence_record, OPERATION)?
                else {
                    valid = false;
                    break;
                };
                if incidence.lanes.is_empty() {
                    valid = false;
                    break;
                }
                for lane in ctx.admit_iter(&incidence.lanes, "catia_b5_incidence_lane_scan")? {
                    let Some(point) =
                        lift_parameter_incidence(ctx, lane.curve, lane.parameter, geometry)?
                    else {
                        valid = false;
                        break 'incidences;
                    };
                    if let Some(reference) = first {
                        if distance_squared(coordinates(point), coordinates(reference))
                            > tolerance_squared
                        {
                            valid = false;
                            break 'incidences;
                        }
                    } else {
                        first = Some(point);
                    }
                }
            }
            if let (true, Some(point)) = (valid, first) {
                ctx.insert_btree_map(
                    &mut coordinates_by_vertex,
                    vertex,
                    point,
                    "catia_b5_incidence_vertex_coordinates",
                )?;
            }
        }
    }
    Ok(coordinates_by_vertex)
}

/// Evaluate one class-`06` curve/parameter pair at its native parameter
/// domain. A vertex roster is valid only when every pair evaluates, so one
/// unsupported, malformed, or out-of-domain member withholds the identity
/// coordinate instead of allowing an earlier member to win.
fn lift_parameter_incidence(
    ctx: &DecodeContext<'_>,
    pcurve_id: u32,
    parameter: FiniteReal,
    geometry: &B5PcurveContext<'_>,
) -> Result<Option<FinitePoint3>, CodecError> {
    const OPERATION: &str = "catia_b5_incidence_curve_lookup";
    if let Some(pcurve) = ctx.get_btree_map(geometry.pcurves, &pcurve_id, OPERATION)? {
        let Some(domain) = pcurve_parameter_domain(ctx, pcurve)? else {
            return Ok(None);
        };
        if parameter < domain[0] || parameter > domain[1] {
            return Ok(None);
        }
        let Some(uv) = evaluate_pcurve(ctx, pcurve, parameter.get())? else {
            return Ok(None);
        };
        let Some(surface) = ctx.get_btree_map(geometry.surfaces, &pcurve.surface, OPERATION)?
        else {
            return Ok(None);
        };
        return Ok(
            lift_pcurve_endpoints(ctx, surface, geometry.profiles, [uv, uv])?
                .map(|[point, _]| point),
        );
    }
    let Some(opaque) = ctx.get_btree_map(geometry.opaque_pcurves, &pcurve_id, OPERATION)? else {
        return Ok(None);
    };
    let Some(pcurve) = opaque.sphere_great_circle.as_ref() else {
        return Ok(None);
    };
    let Some(surface) = ctx.get_btree_map(geometry.surfaces, &opaque.surface, OPERATION)? else {
        return Ok(None);
    };
    Ok(sphere_great_circle_point(pcurve, surface, parameter))
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

fn counted_references(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
    class: u8,
) -> Result<Option<Vec<u32>>, CodecError> {
    if record.class != class {
        return Ok(None);
    }
    let Some((references, position)) = wire::tokens::counted_refs(ctx, &record.payload, true)?
    else {
        return Ok(None);
    };
    Ok((position == record.payload.len()).then_some(references))
}

fn parameter_incidence(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
) -> Result<Option<B5ParameterIncidence>, CodecError> {
    if record.class != 0x06 {
        return Ok(None);
    }
    let Some(count) = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x80))
    else {
        return Ok(None);
    };
    let count = usize::from(count);
    let mut position = 1;
    let Some(references) = ctx.collect_options(
        (0..count).map(|_| wire::tokens::object_ref(&record.payload, &mut position, true)),
        "catia_b5_parameter_incidence_references",
    )?
    else {
        return Ok(None);
    };
    let Some(expected) = u8::try_from(count)
        .ok()
        .and_then(|count| 0x80u8.checked_add(count))
    else {
        return Ok(None);
    };
    if record.payload.get(position) != Some(&expected) {
        return Ok(None);
    }
    position += 1;
    let mut lanes = Vec::new();
    for curve in references {
        let Some(parameter) = f64_le(&record.payload, position) else {
            return Ok(None);
        };
        let Some(next) = position.checked_add(8) else {
            return Ok(None);
        };
        position = next;
        let Some(control) = wire::tokens::compact_uint(&record.payload, &mut position) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut lanes,
            B5IncidenceLane {
                curve,
                parameter,
                control,
            },
            "catia_b5_parameter_incidence_lanes",
        )?;
    }
    Ok(
        (position == record.payload.len()).then_some(B5ParameterIncidence {
            object_id: record.object_id,
            lanes,
        }),
    )
}

/// Bind each loop pcurve occurrence that has no geometry record to the loop's
/// surface, when the occurrence's edge names that pcurve through its
/// curve-support wrapper or through both endpoint incidences. A pcurve bound
/// to two different surfaces is ambiguous and stays unbound.
fn implicit_pcurve_bindings(
    ctx: &DecodeContext<'_>,
    loops: &[B5Loop],
    by_id: &HashMap<u32, &B5Record<'_>>,
    edges: &BTreeMap<u32, B5Edge>,
    parameter_incidences: &BTreeMap<u32, B5ParameterIncidence>,
    geometry: (
        &BTreeMap<u32, B5Pcurve>,
        &BTreeMap<u32, B5OpaquePcurve>,
        &BTreeMap<u32, B5Surface>,
    ),
) -> Result<BTreeMap<u32, u32>, CodecError> {
    const OPERATION: &str = "catia_b5_implicit_pcurve_lookup";
    const AMBIGUOUS: &str = "catia_b5_ambiguous_implicit_pcurves";
    let (pcurves, opaque_pcurves, surfaces) = geometry;
    let mut scratch = ctx.reserve_scoped(0, AMBIGUOUS)?;
    let mut bindings = BTreeMap::new();
    let mut ambiguous = HashSet::new();
    for loop_ in ctx.admit_iter(loops, "catia_b5_implicit_pcurve_loop_scan")? {
        let surface = loop_.surface;
        if !ctx.contains_key_btree_map(surfaces, &surface, OPERATION)? {
            continue;
        }
        for member in ctx.admit_iter(&loop_.members, "catia_b5_implicit_pcurve_member_scan")? {
            let pcurve = member.pcurve;
            if ctx.contains_key_btree_map(pcurves, &pcurve, OPERATION)?
                || ctx.contains_key_btree_map(opaque_pcurves, &pcurve, OPERATION)?
                || ctx.contains_key_hash_map(by_id, &pcurve, OPERATION)?
            {
                continue;
            }
            let Some(edge) = ctx.get_btree_map(edges, &member.edge, OPERATION)? else {
                continue;
            };
            let endpoint_incidence_contains = |reference_id| -> Result<bool, CodecError> {
                let Some(incidence) =
                    ctx.get_btree_map(parameter_incidences, &reference_id, OPERATION)?
                else {
                    return Ok(false);
                };
                ctx.any_by(
                    &incidence.lanes,
                    |lane| Ok(lane.curve == pcurve),
                    "catia_b5_implicit_pcurve_lane_scan",
                )
            };
            let curve_wrapper_contains = match record_by_id(ctx, by_id, edge.support, OPERATION)? {
                Some(wrapper) => {
                    matches!(wrapper.class, 0x23..=0x25)
                        && ctx.any_by(
                            record_references(wrapper),
                            |reference| Ok(reference == pcurve),
                            "catia_b5_curve_wrapper_pcurve_search",
                        )?
                }
                None => false,
            };
            if !(curve_wrapper_contains
                || endpoint_incidence_contains(edge.parameter_incidences[0])?
                    && endpoint_incidence_contains(edge.parameter_incidences[1])?)
            {
                continue;
            }
            if ctx
                .insert_btree_map(
                    &mut bindings,
                    pcurve,
                    surface,
                    "catia_b5_implicit_pcurve_bindings",
                )?
                .is_some_and(|existing| existing != surface)
            {
                scratch.with_storage(|| ctx.insert_hash_set(&mut ambiguous, pcurve, AMBIGUOUS))?;
            }
        }
    }
    if !ambiguous.is_empty() {
        ctx.retain_btree_map(
            &mut bindings,
            |pcurve, _| Ok(!ctx.contains_hash_set(&ambiguous, pcurve, AMBIGUOUS)?),
            AMBIGUOUS,
        )?;
    }
    Ok(bindings)
}

pub(super) fn evaluate_pcurve(
    ctx: &DecodeContext<'_>,
    pcurve: &B5Pcurve,
    parameter: f64,
) -> Result<Option<[f64; 2]>, CodecError> {
    let Some(knots) = pcurve_nurbs_knots(ctx, pcurve)? else {
        return Ok(None);
    };
    let knots = ctx.collect_vec(
        knots.into_iter().map(FiniteReal::get),
        "catia_b5_evaluation_knots",
    )?;
    let control_points = ctx.collect_vec(
        pcurve
            .control_points
            .iter()
            .map(|point| Point2::new(point[0], point[1])),
        "catia_b5_evaluation_points",
    )?;
    let weights = pcurve
        .weights
        .as_ref()
        .map(|weights| {
            ctx.collect_vec(
                weights.iter().copied().map(PositiveReal::get),
                "catia_b5_evaluation_weights",
            )
        })
        .transpose()?;
    let point = cadmpeg_ir::eval::finite_or_refusal(nurbs_pcurve_uv(
        ctx,
        pcurve.degree,
        &knots,
        &control_points,
        weights.as_deref(),
        parameter,
    ))?
    .map(Point2::from);
    Ok(point.map(|point| [point.u, point.v]))
}

fn pcurve_knots(
    ctx: &DecodeContext<'_>,
    pcurve: &B5Pcurve,
) -> Result<Option<Vec<FiniteReal>>, CodecError> {
    let Some(count) = ctx
        .admit_iter(&pcurve.distinct_knots, "catia_b5_pcurve_knot_count")?
        .zip(ctx.admit_iter(&pcurve.multiplicities, "catia_b5_pcurve_multiplicity_count")?)
        .try_fold(0usize, |count, (_, multiplicity)| {
            usize::try_from(*multiplicity)
                .ok()
                .and_then(|multiplicity| count.checked_add(multiplicity))
        })
    else {
        return Ok(None);
    };
    let mut knots = Vec::new();
    ctx.reserve_vec(&mut knots, count, "catia_b5_expanded_pcurve_knots")?;
    for (&knot, &multiplicity) in ctx
        .admit_iter(&pcurve.distinct_knots, "catia_b5_pcurve_knot_emit")?
        .zip(ctx.admit_iter(&pcurve.multiplicities, "catia_b5_pcurve_multiplicity_emit")?)
    {
        let Some(multiplicity) = usize::try_from(multiplicity).ok() else {
            return Ok(None);
        };
        knots.extend(std::iter::repeat_with(|| knot).take(multiplicity));
    }
    Ok(Some(knots))
}

/// Return the knot vector in the pcurve's occurrence coordinate system. A
/// translated knot is admitted finite, since the translation can overflow.
pub(super) fn pcurve_nurbs_knots(
    ctx: &DecodeContext<'_>,
    pcurve: &B5Pcurve,
) -> Result<Option<Vec<FiniteReal>>, CodecError> {
    let Some(mut knots) = pcurve_knots(ctx, pcurve)? else {
        return Ok(None);
    };
    match pcurve.parameterization {
        B5PcurveParameterization::Native => {}
        B5PcurveParameterization::Translated { native_origin } => {
            for knot in ctx.admit_iter(&mut knots, "catia_b5_translated_pcurve_knots")? {
                let Some(value) = FiniteReal::new(knot.get() - native_origin.get()) else {
                    return Ok(None);
                };
                *knot = value;
            }
        }
    }
    Ok(Some(knots))
}

pub(super) fn pcurve_parameter_domain(
    ctx: &DecodeContext<'_>,
    pcurve: &B5Pcurve,
) -> Result<Option<[FiniteReal; 2]>, CodecError> {
    let Some(degree) = usize::try_from(pcurve.degree).ok() else {
        return Ok(None);
    };
    let total = ctx
        .admit_iter(&pcurve.distinct_knots, "catia_b5_pcurve_domain_knot_count")?
        .zip(ctx.admit_iter(
            &pcurve.multiplicities,
            "catia_b5_pcurve_domain_multiplicity_count",
        )?)
        .try_fold(0usize, |count, (_, multiplicity)| {
            count.checked_add(usize::try_from(*multiplicity).ok()?)
        });
    let Some(total) = total else {
        return Ok(None);
    };
    let Some(last) = degree
        .checked_add(1)
        .and_then(|width| total.checked_sub(width))
    else {
        return Ok(None);
    };
    let mut first_knot = None;
    let mut last_knot = None;
    let mut index = 0usize;
    for (&knot, &multiplicity) in ctx
        .admit_iter(&pcurve.distinct_knots, "catia_b5_pcurve_domain_knot_bounds")?
        .zip(ctx.admit_iter(
            &pcurve.multiplicities,
            "catia_b5_pcurve_domain_multiplicity_bounds",
        )?)
    {
        let Some(next) = usize::try_from(multiplicity)
            .ok()
            .and_then(|multiplicity| index.checked_add(multiplicity))
        else {
            return Ok(None);
        };
        if index < next {
            let knot = match pcurve.parameterization {
                B5PcurveParameterization::Native => knot,
                B5PcurveParameterization::Translated { native_origin } => {
                    let Some(knot) = FiniteReal::new(knot.get() - native_origin.get()) else {
                        return Ok(None);
                    };
                    knot
                }
            };
            if index <= degree && degree < next {
                first_knot = Some(knot);
            }
            if index <= last && last < next {
                last_knot = Some(knot);
            }
        }
        index = next;
    }
    let (Some(first_knot), Some(last_knot)) = (first_knot, last_knot) else {
        return Ok(None);
    };
    let spline_domain = [first_knot, last_knot];
    if spline_domain[0] >= spline_domain[1] {
        return Ok(None);
    }
    Ok(match pcurve.parameter_range {
        Some(range) => bounded_occurrence_range(range, spline_domain),
        None => Some(spline_domain),
    })
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
    ctx: &DecodeContext<'_>,
    loops: &BTreeMap<u32, B5Loop>,
    geometry: &B5PcurveContext<'_>,
    native_edges: &BTreeMap<u32, [u32; 2]>,
    geometric_edges: &BTreeMap<u32, [usize; 2]>,
    native_coordinates: &BTreeMap<u32, FinitePoint3>,
    points: &[FinitePoint3],
) -> Result<BoundNativeVertices, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_vertex_binding_scratch")?;
    let (logical_coordinates, logical_vertex_indices) = scratch.with_storage(|| {
        let mut constraints = Vec::new();
        for (edge, vertices) in
            ctx.admit_iter(native_edges, "catia_b5_vertex_constraint_edge_scan")?
        {
            if let Some(points) = ctx.get_btree_map(
                geometric_edges,
                edge,
                "catia_b5_vertex_constraint_edge_points",
            )? {
                ctx.push_vec(
                    &mut constraints,
                    (*vertices, *points),
                    "catia_b5_vertex_constraints",
                )?;
            }
        }
        let mut adjacency = HashMap::<u32, Vec<usize>>::new();
        for (index, (vertices, _)) in ctx
            .admit_iter(&constraints, "catia_b5_vertex_constraint_adjacency_scan")?
            .enumerate()
        {
            for &vertex in vertices {
                ctx.push_hash_group(
                    &mut adjacency,
                    vertex,
                    index,
                    "catia_b5_vertex_adjacency_groups",
                    "catia_b5_vertex_adjacency_links",
                )?;
            }
        }
        // Logical coordinates are keyed and visited in object-id order, which
        // fixes each logical vertex rank.
        let mut logical_coordinates =
            propagate_vertex_points(ctx, &constraints, &adjacency, points)?;
        for (&vertex, &point) in
            ctx.admit_iter(native_coordinates, "catia_b5_native_vertex_coordinate_scan")?
        {
            ctx.insert_btree_map(
                &mut logical_coordinates,
                vertex,
                point,
                "catia_b5_logical_coordinates",
            )?;
        }
        // Native vertex identity fixes topology even when incident lifted endpoints
        // are separated. Keep the first deterministic finite lift as the logical
        // coordinate; the pass below records every separation in vertex tolerance.
        for (_, loop_) in ctx.admit_iter(loops, "catia_b5_native_vertex_loop_scan")? {
            for member in ctx.admit_iter(&loop_.members, "catia_b5_geometric_edge_member_scan")? {
                let Some(vertices) =
                    ctx.get_btree_map(native_edges, &member.edge, "catia_b5_native_member_edge")?
                else {
                    continue;
                };
                let Some(lifted) = pcurve_endpoints(ctx, member.pcurve, member.edge, geometry)?
                else {
                    continue;
                };
                for lane in 0..2 {
                    ctx.entry_btree_map(
                        &mut logical_coordinates,
                        vertices[lane],
                        "catia_b5_logical_coordinates",
                    )?
                    .or_insert(lifted[lane]);
                }
            }
        }
        let mut logical_vertex_indices = HashMap::new();
        for (rank, (&object_id, _)) in ctx
            .admit_iter(&logical_coordinates, "catia_b5_ranked_logical_vertex_scan")?
            .enumerate()
        {
            ctx.insert_hash_map(
                &mut logical_vertex_indices,
                object_id,
                B5VertexRef::Logical(rank),
                "catia_b5_logical_vertex_indices",
            )?;
        }
        Ok::<_, CodecError>((logical_coordinates, logical_vertex_indices))
    })?;
    let logical_vertices = ctx.collect_vec(
        ctx.admit_iter(&logical_coordinates, "catia_b5_logical_vertex_emit_scan")?
            .map(|(&object_id, &point)| B5LogicalVertex { object_id, point }),
        "catia_b5_logical_vertices",
    )?;
    let mut edge_vertices = BTreeMap::new();
    for (&edge, vertices) in
        ctx.admit_iter(geometric_edges, "catia_b5_geometric_edge_binding_scan")?
    {
        ctx.insert_btree_map(
            &mut edge_vertices,
            edge,
            vertices.map(B5VertexRef::Raw),
            "catia_b5_bound_edge_vertices",
        )?;
    }
    for (&edge, vertices) in ctx.admit_iter(native_edges, "catia_b5_native_edge_binding_scan")? {
        const OPERATION: &str = "catia_b5_native_edge_logical_vertices";
        if let (Some(&start), Some(&end)) = (
            ctx.get_hash_map(&logical_vertex_indices, &vertices[0], OPERATION)?,
            ctx.get_hash_map(&logical_vertex_indices, &vertices[1], OPERATION)?,
        ) {
            ctx.insert_btree_map(
                &mut edge_vertices,
                edge,
                [start, end],
                "catia_b5_bound_edge_vertices",
            )?;
        }
    }
    let mut tolerances = BTreeMap::<usize, PositiveReal>::new();
    for (_, loop_) in ctx.admit_iter(loops, "catia_b5_vertex_tolerance_loop_scan")? {
        for member in ctx.admit_iter(&loop_.members, "catia_b5_vertex_tolerance_member_scan")? {
            let Some(lifted) = pcurve_endpoints(ctx, member.pcurve, member.edge, geometry)? else {
                continue;
            };
            let lifted = lifted.map(coordinates);
            let Some(&loci) = ctx.get_btree_map(
                &edge_vertices,
                &member.edge,
                "catia_b5_vertex_tolerance_edge",
            )?
            else {
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
                ctx.entry_btree_map(
                    &mut tolerances,
                    locus.combined_index(points.len()),
                    "catia_b5_vertex_tolerances",
                )?
                .and_modify(|tolerance| {
                    if candidate > *tolerance {
                        *tolerance = candidate;
                    }
                })
                .or_insert(candidate);
            }
        }
    }
    Ok(BoundNativeVertices {
        edges: edge_vertices,
        vertices: logical_vertices,
        tolerances,
    })
}

/// Assigns lifted points to native vertices one connected constraint component
/// at a time. A component whose constraints put one vertex at two separated
/// points assigns nothing.
fn propagate_vertex_points(
    ctx: &DecodeContext<'_>,
    constraints: &[([u32; 2], [usize; 2])],
    adjacency: &HashMap<u32, Vec<usize>>,
    points: &[FinitePoint3],
) -> Result<BTreeMap<u32, FinitePoint3>, CodecError> {
    let mut mapping = BTreeMap::new();
    let mut visited = ctx.alloc_filled(
        constraints.len(),
        false,
        "catia_b5_component_visited_constraints",
    )?;
    for seed in ctx.admit_iter(0..constraints.len(), "catia_b5_vertex_component_seed_scan")? {
        if visited[seed] {
            continue;
        }
        let (component, consistent) =
            propagate_vertex_component(ctx, seed, constraints, adjacency, points, &mut visited)?;
        if consistent {
            for (vertex, locus) in ctx.admit_iter(component, "catia_b5_vertex_component_scan")? {
                ctx.insert_btree_map(
                    &mut mapping,
                    vertex,
                    points[locus],
                    "catia_b5_propagated_vertex_points",
                )?;
            }
        }
    }
    Ok(mapping)
}

/// Walks the constraints connected to `seed`, marking each in `visited`, and
/// returns the vertex loci and whether they agree within tolerance.
fn propagate_vertex_component(
    ctx: &DecodeContext<'_>,
    seed: usize,
    constraints: &[([u32; 2], [usize; 2])],
    adjacency: &HashMap<u32, Vec<usize>>,
    points: &[FinitePoint3],
    visited: &mut [bool],
) -> VertexComponentOutput {
    let mut mapping = BTreeMap::new();
    let mut pending = std::collections::VecDeque::new();
    let mut consistent = true;
    let mut assign = |mapping: &mut BTreeMap<u32, usize>,
                      pending: &mut std::collections::VecDeque<u32>,
                      vertex,
                      locus|
     -> Result<(), CodecError> {
        if let Some(&previous) =
            ctx.get_btree_map(mapping, &vertex, "catia_b5_component_vertex_points")?
        {
            let residual =
                distance_squared(coordinates(points[previous]), coordinates(points[locus]));
            if !residual.is_finite() || residual > POINT_TOLERANCE * POINT_TOLERANCE {
                consistent = false;
            }
        } else {
            ctx.insert_btree_map(mapping, vertex, locus, "catia_b5_component_vertex_points")?;
            ctx.push_back(pending, vertex, "catia_b5_component_pending_vertices")?;
        }
        Ok(())
    };
    let (seed_vertices, seed_loci) = constraints[seed];
    for lane in 0..2 {
        assign(
            &mut mapping,
            &mut pending,
            seed_vertices[lane],
            seed_loci[lane],
        )?;
    }
    while let Some(vertex) = pending.pop_front() {
        let Some(indices) =
            ctx.get_hash_map(adjacency, &vertex, "catia_b5_vertex_adjacency_lookup")?
        else {
            continue;
        };
        for &index in ctx.admit_iter(indices, "catia_b5_vertex_adjacency_scan")? {
            if std::mem::replace(&mut visited[index], true) {
                continue;
            }
            let (vertices, loci) = constraints[index];
            for lane in 0..2 {
                assign(&mut mapping, &mut pending, vertices[lane], loci[lane])?;
            }
        }
    }
    Ok((mapping, consistent))
}

fn vertex_coordinate(
    points: &[FinitePoint3],
    logical_vertices: &[B5LogicalVertex],
    vertex: B5VertexRef,
) -> [f64; 3] {
    coordinates(match vertex {
        B5VertexRef::Raw(index) => points[index],
        B5VertexRef::Logical(index) => logical_vertices[index].point,
    })
}

/// Keeps the loops that some face references and returns the referenced loop
/// ids, including ids with no loop.
pub(super) fn retain_referenced_loops(
    ctx: &DecodeContext<'_>,
    faces: &[B5Face],
    loops: &mut BTreeMap<u32, B5Loop>,
) -> Result<BTreeSet<u32>, CodecError> {
    let mut referenced = BTreeSet::new();
    for face in ctx.admit_iter(faces, "catia_b5_referenced_face_scan")? {
        for &loop_id in ctx.admit_iter(&face.loops, "catia_b5_referenced_face_loop_scan")? {
            ctx.insert_btree_set(&mut referenced, loop_id, "catia_b5_referenced_loops")?;
        }
    }
    ctx.retain_btree_map(
        loops,
        |loop_id, _| {
            ctx.contains_btree_set(&referenced, loop_id, "catia_b5_referenced_loop_filter")
        },
        "catia_b5_referenced_loop_filter",
    )?;
    Ok(referenced)
}

/// Tests that every member of a loop has a pcurve on the loop's surface and a
/// bound edge, and that the bound edges chain into a closed cycle.
pub(super) fn loop_resolves(
    ctx: &DecodeContext<'_>,
    loop_: &B5Loop,
    pcurves: &BTreeMap<u32, B5Pcurve>,
    opaque_pcurves: &BTreeMap<u32, B5OpaquePcurve>,
    implicit_pcurves: &BTreeMap<u32, u32>,
    edge_vertices: &BTreeMap<u32, [B5VertexRef; 2]>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_b5_loop_member_resolution";
    let members_resolve = ctx.all_by(
        &loop_.members,
        |member| {
            let pcurve_matches = ctx
                .get_btree_map(pcurves, &member.pcurve, OPERATION)?
                .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                || ctx
                    .get_btree_map(opaque_pcurves, &member.pcurve, OPERATION)?
                    .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                || ctx.get_btree_map(implicit_pcurves, &member.pcurve, OPERATION)?
                    == Some(&loop_.surface);
            Ok(pcurve_matches
                && ctx.contains_key_btree_map(edge_vertices, &member.edge, OPERATION)?)
        },
        OPERATION,
    )?;
    Ok(members_resolve && loop_chain_closes(ctx, loop_, edge_vertices)?)
}

pub(super) fn loop_chain_closes(
    ctx: &DecodeContext<'_>,
    loop_: &B5Loop,
    edge_vertices: &BTreeMap<u32, [B5VertexRef; 2]>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_b5_loop_chain_closure";
    let Some((first_member, members)) = loop_.members.split_first() else {
        return Ok(false);
    };
    let Some(first) = ctx.get_btree_map(edge_vertices, &first_member.edge, OPERATION)? else {
        return Ok(false);
    };
    let first_reversed = usize::from(first_member.controls[0] == -1);
    let initial = first[first_reversed];
    let mut current = first[1 - first_reversed];
    let chained = ctx.all_by(
        members,
        |member| {
            let Some(endpoints) = ctx.get_btree_map(edge_vertices, &member.edge, OPERATION)? else {
                return Ok(false);
            };
            let reversed = usize::from(member.controls[0] == -1);
            if endpoints[reversed] != current {
                return Ok(false);
            }
            current = endpoints[1 - reversed];
            Ok(true)
        },
        OPERATION,
    )?;
    Ok(chained && current == initial)
}

fn parse_profile(record: &B5Record) -> Option<B5Profile> {
    (record.family == 0xb5).then_some(())?;
    match record.class {
        0x0e => {
            (record.payload.len() == 73 && record.payload.first() == Some(&0x80)).then_some(())?;
            let direction = ExactUnitVector3::new(
                read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get),
            )?;
            let parameter_range = IncreasingParameterInterval::new([
                f64_le(&record.payload, 57)?.get(),
                f64_le(&record.payload, 65)?.get(),
            ])?;
            (f64_le(&record.payload, 49)?.get() == 1.0).then_some(B5Profile::Line {
                point: f64_point(&record.payload, 1)?,
                direction,
                parameter_range,
            })
        }
        0x0f => {
            (record.payload.len() == 113 && record.payload.first() == Some(&0x80)).then_some(())?;
            let [direction_x, direction_y] = unit_and_orthogonal_directions(
                read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get),
                read_f64_array::<3>(&record.payload, 49)?.map(FiniteReal::get),
            )?;
            let radius = PositiveLength::new(f64_le(&record.payload, 73)?.get())?;
            let parameter_range = [
                f64_le(&record.payload, 81)?.get(),
                f64_le(&record.payload, 89)?.get(),
            ];
            let chart_origin = f64_le(&record.payload, 105)?.get();
            let scaled = |value: f64| value / radius.get();
            (periodic_angular_range_is_valid(
                parameter_range.map(scaled),
                [
                    scaled(chart_origin),
                    scaled(chart_origin) + std::f64::consts::TAU,
                ],
            ) && f64_le(&record.payload, 97)?.get() == 1.0)
                .then_some(())?;
            Some(B5Profile::Arc {
                center: f64_point(&record.payload, 1)?,
                direction_x,
                direction_y,
                radius,
                // The angular check divides both endpoints by the positive
                // radius and finds them strictly increasing, so the stored
                // endpoints are strictly increasing too.
                parameter_range: IncreasingParameterInterval::new(parameter_range)?,
            })
        }
        _ => None,
    }
}

fn bind_edge_vertices(
    ctx: &DecodeContext<'_>,
    loops: &BTreeMap<u32, B5Loop>,
    geometry: &B5PcurveContext<'_>,
    points: &[FinitePoint3],
) -> Result<BTreeMap<u32, [usize; 2]>, CodecError> {
    const OPERATION: &str = "catia_b5_geometric_edge_vertices";
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_edge_binding_scratch")?;
    let point_index = scratch.with_storage(|| point_index(ctx, points))?;
    let mut edges: BTreeMap<u32, [usize; 2]> = BTreeMap::new();
    let mut conflicts = HashSet::new();
    for (_, loop_) in ctx.admit_iter(loops, "catia_b5_edge_binding_loop_scan")? {
        for member in ctx.admit_iter(&loop_.members, "catia_b5_edge_binding_member_scan")? {
            if ctx.contains_hash_set(&conflicts, &member.edge, OPERATION)? {
                continue;
            }
            let Some(endpoints) = pcurve_endpoints(ctx, member.pcurve, member.edge, geometry)?
            else {
                continue;
            };
            let (Some(start), Some(end)) = (
                canonical_point(ctx, points, &point_index, coordinates(endpoints[0]))?,
                canonical_point(ctx, points, &point_index, coordinates(endpoints[1]))?,
            ) else {
                continue;
            };
            let indices = [start, end];
            match ctx.get_btree_map(&edges, &member.edge, OPERATION)? {
                // Either traversal direction binds the same vertex pair.
                Some(&[first, second]) => {
                    if [first.min(second), first.max(second)] != [start.min(end), start.max(end)] {
                        ctx.remove_btree_map(&mut edges, &member.edge, OPERATION)?;
                        scratch.with_storage(|| {
                            ctx.insert_hash_set(
                                &mut conflicts,
                                member.edge,
                                "catia_b5_conflicting_edge_vertices",
                            )
                        })?;
                    }
                }
                None => {
                    ctx.insert_btree_map(&mut edges, member.edge, indices, OPERATION)?;
                }
            }
        }
    }
    Ok(edges)
}

fn pcurve_endpoints(
    ctx: &DecodeContext<'_>,
    pcurve_id: u32,
    edge_id: u32,
    geometry: &B5PcurveContext<'_>,
) -> Result<Option<[FinitePoint3; 2]>, CodecError> {
    const OPERATION: &str = "catia_b5_pcurve_endpoint_lookup";
    if let Some(pcurve) = ctx.get_btree_map(geometry.pcurves, &pcurve_id, OPERATION)? {
        let domain = pcurve_parameter_domain(ctx, pcurve)?;
        let parameters = edge_pcurve_parameter_values(
            ctx,
            geometry.edge_parameter_incidences,
            geometry.parameter_incidences,
            edge_id,
            pcurve_id,
        )?
        .and_then(|parameters| bounded_occurrence_range(parameters, domain?))
        // A missing or invalid edge incidence selects the complete pcurve
        // knot domain. This is the native object-stream fallback; it is not
        // permission to use an arbitrary control-polygon endpoint.
        .or(domain);
        let Some(parameters) = parameters else {
            return Ok(pcurve.lifted_endpoints);
        };
        let Some(first) = evaluate_pcurve(ctx, pcurve, parameters[0].get())? else {
            return Ok(None);
        };
        let Some(second) = evaluate_pcurve(ctx, pcurve, parameters[1].get())? else {
            return Ok(None);
        };
        let uv = [first, second];
        let Some(surface) = ctx.get_btree_map(geometry.surfaces, &pcurve.surface, OPERATION)?
        else {
            return Ok(pcurve.lifted_endpoints);
        };
        return Ok(
            lift_pcurve_endpoints(ctx, surface, geometry.profiles, uv)?.or(pcurve.lifted_endpoints)
        );
    }
    let Some(opaque) = ctx.get_btree_map(geometry.opaque_pcurves, &pcurve_id, OPERATION)? else {
        return Ok(None);
    };
    let Some(pcurve) = opaque.sphere_great_circle.as_ref() else {
        return Ok(None);
    };
    let Some(surface) = ctx.get_btree_map(geometry.surfaces, &opaque.surface, OPERATION)? else {
        return Ok(None);
    };
    let parameters = edge_pcurve_parameter_values(
        ctx,
        geometry.edge_parameter_incidences,
        geometry.parameter_incidences,
        edge_id,
        pcurve_id,
    )?;
    let [start, end] = parameters
        .and_then(|parameters| {
            bounded_occurrence_range(parameters, pcurve.u_bounds.finite_endpoints())
        })
        .unwrap_or(pcurve.u_bounds.finite_endpoints());
    Ok(sphere_great_circle_point(pcurve, surface, start)
        .zip(sphere_great_circle_point(pcurve, surface, end))
        .map(|(start, end)| [start, end]))
}

/// CATIA's object-stream on-carrier incidence tolerance, in millimetres.
const POINT_TOLERANCE: f64 = 1e-3;

fn point_cell(point: [f64; 3]) -> Option<[i64; 3]> {
    let [x, y, z] =
        point.map(|coordinate| truncate_f64_to_i64((coordinate / POINT_TOLERANCE).floor()));
    Some([x?, y?, z?])
}

fn point_index(
    ctx: &DecodeContext<'_>,
    points: &[FinitePoint3],
) -> Result<HashMap<[i64; 3], Vec<usize>>, CodecError> {
    let mut index = HashMap::<[i64; 3], Vec<usize>>::new();
    for (point_index, point) in ctx
        .admit_iter(points, "catia_b5_point_index_scan")?
        .enumerate()
    {
        let cell = point_cell(coordinates(*point))
            .ok_or_else(|| CodecError::malformed("B5 point exceeds spatial index range"))?;
        ctx.push_hash_group(
            &mut index,
            cell,
            point_index,
            "catia_b5_point_index_cells",
            "catia_b5_point_index_members",
        )?;
    }
    Ok(index)
}

fn canonical_point(
    ctx: &DecodeContext<'_>,
    points: &[FinitePoint3],
    index: &HashMap<[i64; 3], Vec<usize>>,
    endpoint: [f64; 3],
) -> Result<Option<usize>, CodecError> {
    let Some(cell) = point_cell(endpoint) else {
        return Ok(None);
    };
    let mut best = None;
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let (Some(x), Some(y), Some(z)) = (
                    cell[0].checked_add(dx),
                    cell[1].checked_add(dy),
                    cell[2].checked_add(dz),
                ) else {
                    continue;
                };
                let neighbor = [x, y, z];
                let Some(indices) =
                    ctx.get_hash_map(index, &neighbor, "catia_b5_point_cell_lookup")?
                else {
                    continue;
                };
                for &point_index in ctx.admit_iter(indices, "catia_b5_point_cell_scan")? {
                    if distance_squared(coordinates(points[point_index]), endpoint)
                        <= POINT_TOLERANCE * POINT_TOLERANCE
                    {
                        best = Some(
                            best.map_or(point_index, |previous: usize| previous.min(point_index)),
                        );
                    }
                }
            }
        }
    }
    Ok(best)
}

fn distance_squared(left: [f64; 3], right: [f64; 3]) -> f64 {
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2)
}

/// Admit two directions whose squared lengths are one and whose dot product
/// is zero, each within `1e-12`.
fn unit_and_orthogonal_directions(
    first: [f64; 3],
    second: [f64; 3],
) -> Option<[ExactUnitVector3; 2]> {
    let dot = first
        .iter()
        .zip(second)
        .map(|(left, right)| left * right)
        .sum::<f64>();
    let directions = [
        ExactUnitVector3::new(first)?,
        ExactUnitVector3::new(second)?,
    ];
    (dot.abs() <= EPS_B5_GRAPH_EXACT_GEOMETRY).then_some(directions)
}

/// Admit a `b5 03 2b` or `2d` frame: the two transverse directions are unit
/// length, perpendicular, and `direction_x × direction_y` lies within `1e-12`
/// of the axis. The frame holds the stored axis and `direction_x`; the
/// admitted `direction_y` is returned beside it.
fn right_handed_frame(
    axis: [f64; 3],
    direction_x: [f64; 3],
    direction_y: [f64; 3],
) -> Option<(OrthonormalFrame3, ExactUnitVector3)> {
    let direction_y = ExactUnitVector3::new(direction_y)?;
    let frame = OrthonormalFrame3::right_handed_euclidean_1e12(
        UnitVector3::new(Vector3::from(axis))?,
        ExactUnitVector3::new(direction_x)?.into(),
        direction_y.into(),
    )?;
    Some((frame, direction_y))
}

/// Admit a `b5 03 27` or `28` frame: the two stored directions are unit
/// length and perpendicular. The frame holds the unit normal of
/// `first × second` and the stored `first` direction.
fn completed_frame(first: [f64; 3], second: ExactUnitVector3) -> Option<OrthonormalFrame3> {
    OrthonormalFrame3::completing_by_largest_component(
        ExactUnitVector3::new(first)?.into(),
        second.into(),
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
) -> Option<(OrthonormalFrame3, UnitVector3)> {
    let axis = UnitVector3::new(Vector3::from(axis))?;
    let direction_x = ExactUnitVector3::new(direction_x)?.into();
    let direction_y = ExactUnitVector3::new(direction_y)?.into();
    let frame = OrthonormalFrame3::right_handed_euclidean(axis, direction_x, direction_y).or_else(
        || {
            let mut frame = OrthonormalFrame3::right_handed_euclidean(
                axis.reversed(),
                direction_x,
                direction_y,
            )?;
            frame.reverse_axis();
            Some(frame)
        },
    )?;
    Some((frame, direction_y))
}

fn parse_surface(record: &B5Record) -> Option<B5Surface> {
    (record.family == 0xb5).then_some(())?;
    match record.class {
        0x27 => {
            (record.payload.len() == 121 && record.payload.first() == Some(&0x80)).then_some(())?;
            let direction_u = read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get);
            let direction_v = ExactUnitVector3::new(
                read_f64_array::<3>(&record.payload, 49)?.map(FiniteReal::get),
            )?;
            let u_range = IncreasingParameterInterval::new([
                f64_le(&record.payload, 89)?.get(),
                f64_le(&record.payload, 97)?.get(),
            ])?;
            let v_range = IncreasingParameterInterval::new([
                f64_le(&record.payload, 105)?.get(),
                f64_le(&record.payload, 113)?.get(),
            ])?;
            let frame = completed_frame(direction_u, direction_v)?;
            (f64_le(&record.payload, 73)?.get() == 1.0 && f64_le(&record.payload, 81)?.get() == 1.0)
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
            let v_range = IncreasingParameterInterval::new([
                f64_le(&record.payload, 97)?.get(),
                f64_le(&record.payload, 105)?.get(),
            ])?;
            let angular_factor = f64_le(&record.payload, 113)?.get();
            let chart_origin = f64_le(&record.payload, 129)?;
            let angular_scale = FiniteReal::new(radius / angular_factor)?;
            let chart_domain = [
                chart_origin.get(),
                chart_origin.get() + std::f64::consts::TAU * angular_scale.get(),
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
            let frame = completed_frame(stored_u, ExactUnitVector3::new(stored_v)?)?;
            (angular_factor > 0.0
                && f64_le(&record.payload, 121)?.get() == 1.0
                && u_range[0] >= chart_domain[0] - chart_tolerance
                && u_range[1] <= chart_domain[1] + chart_tolerance)
                .then_some(())?;
            let u_range = IncreasingParameterInterval::new(u_range)?;
            Some(B5Surface::Cylinder {
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
            let apex = f64_point(&record.payload, 1)?;
            let direction_x = read_f64_array::<3>(&record.payload, 25)?.map(FiniteReal::get);
            let direction_y = read_f64_array::<3>(&record.payload, 49)?.map(FiniteReal::get);
            let axis = read_f64_array::<3>(&record.payload, 73)?.map(FiniteReal::get);
            let half_angle = f64_le(&record.payload, 97)?;
            let reference_radius = f64_le(&record.payload, 105)?;
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
            let (frame, direction_y) = cone_frame(axis, direction_x, direction_y)?;
            let slant_start = NonNegativeLength::new(slant_range[0])?;
            let positive_half_angle = PositiveAngle::new(half_angle.get())?;
            (half_angle.get() < std::f64::consts::FRAC_PI_2
                && periodic_angular_range_is_valid(angular_range, angular_domain)
                && f64_le(&record.payload, 153)?.get() == 1.0
                && f64_le(&record.payload, 161)?.get() == 0.0)
                .then_some(())?;
            // The angular check admits a strictly increasing range inside a
            // strictly increasing full-turn domain.
            let angular_range = IncreasingParameterInterval::new(angular_range)?;
            let angular_domain = IncreasingParameterInterval::new(angular_domain)?;
            let slant_range = IncreasingParameterInterval::new(slant_range)?;
            let origin = add(
                coordinates(apex),
                scale(axis, slant_range.lower() * half_angle.get().cos()),
            );
            let half_angle_radians = Angle::from_assigned_real(half_angle);
            Some(B5Surface::Cone {
                apex,
                frame,
                direction_y,
                half_angle: positive_half_angle,
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
            let chart_values = read_f64_array::<6>(&record.payload, 105)?;
            let chart_origin = chart_values[5];
            let [azimuth_lo, azimuth_hi, latitude_lo, latitude_hi, construction_radius, _] =
                chart_values.map(FiniteReal::get);
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
            let chart_origin_tolerance = 2.0
                * f64::EPSILON
                * chart_origin
                    .get()
                    .abs()
                    .max(expected_chart_origin.abs())
                    .max(1.0);
            let admitted_radius = PositiveLength::new(radius)?;
            let frame =
                OrthonormalFrame3::right_handed_euclidean_1e12(axis, direction_x, direction_y)?;
            let admitted_construction_radius = PositiveLength::new(construction_radius)?;
            (sphere_angular_ranges_are_valid(azimuth_range, latitude_range)
                && expected_chart_origin.is_finite()
                && (chart_origin.get() - expected_chart_origin).abs() <= chart_origin_tolerance
                && [stored_x, stored_y, stored_axis].iter().all(|direction| {
                    ((vector_length(*direction) / radius) - 1.0).abs()
                        <= EPS_B5_GRAPH_EXACT_GEOMETRY
                }))
            .then_some(())?;
            // The sphere chart check admits strictly increasing azimuth and
            // latitude ranges.
            Some(B5Surface::Sphere {
                center,
                frame,
                direction_y,
                radius: admitted_radius,
                azimuth_range: IncreasingParameterInterval::new(azimuth_range)?,
                latitude_range: IncreasingParameterInterval::new(latitude_range)?,
                construction_radius: admitted_construction_radius,
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
            let (frame, direction_y) = right_handed_frame(axis, direction_x, direction_y)?;
            let admitted_major_radius = PositiveLength::new(major_radius)?;
            let admitted_minor_radius = PositiveLength::new(minor_radius)?;
            (periodic_angular_range_is_valid(major_angular_range, major_angular_domain)
                && periodic_angular_range_is_valid(minor_angular_range, minor_angular_domain))
            .then_some(())?;
            // Each angular check admits a strictly increasing range inside a
            // strictly increasing full-turn domain.
            Some(B5Surface::Torus {
                center: f64_point(&record.payload, 1)?,
                frame,
                direction_y,
                major_radius: admitted_major_radius,
                minor_radius: admitted_minor_radius,
                major_angular_range: IncreasingParameterInterval::new(major_angular_range)?,
                major_angular_domain: IncreasingParameterInterval::new(major_angular_domain)?,
                minor_angular_range: IncreasingParameterInterval::new(minor_angular_range)?,
                minor_angular_domain: IncreasingParameterInterval::new(minor_angular_domain)?,
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
            let (frame, _) = right_handed_frame(axis_direction, reference_x, reference_y)?;
            let angular_range = IncreasingParameterInterval::new(angular_range)?;
            let profile_range = IncreasingParameterInterval::new(profile_range)?;
            (angular_range.lower() >= 0.0
                && angular_range.upper() <= 2.0 * angular_half_turn
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
    ctx: &DecodeContext<'_>,
    record: &B5Record<'_>,
    headers: &BTreeMap<u32, crate::families::a5a8::records::A8SurfaceHeader>,
) -> Result<Option<B5Surface>, CodecError> {
    if let Some(surface) = parse_surface(record) {
        return Ok(Some(surface));
    }
    if record.family != 0xa8 || record.class != 0x34 {
        return Ok(None);
    }
    let header = ctx.get_btree_map(
        headers,
        &record.object_id,
        "catia_b5_a8_surface_header_lookup",
    )?;
    let payload = ctx.copy_slice(record.payload, "catia_b5_surface_node_payload")?;
    Ok(Some(match header {
        Some(header) => B5Surface::UnresolvedNurbs {
            header: header.copy_charged(ctx)?,
            payload,
        },
        None => B5Surface::Unknown {
            family: record.family,
            class: record.class,
            payload,
        },
    }))
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
    ctx: &DecodeContext<'_>,
    record: &B5Record<'_>,
    surfaces: &BTreeMap<u32, B5Surface>,
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
    records: &HashMap<u32, &B5Record<'_>>,
) -> Result<Option<B5OffsetSurface>, CodecError> {
    let Some(fields) = parse_offset_surface_fields(record) else {
        return Ok(None);
    };
    Ok(
        offset_surface_agrees(ctx, &fields, surfaces, extrusion_surfaces, records)?
            .then_some(fields),
    )
}

/// Tests an offset construction against its carrier: the declared carrier
/// kind must match the carrier geometry, and the carrier must sit at the
/// stated distance from the source.
fn offset_surface_agrees(
    ctx: &DecodeContext<'_>,
    fields: &B5OffsetSurface,
    surfaces: &BTreeMap<u32, B5Surface>,
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
    records: &HashMap<u32, &B5Record<'_>>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_b5_offset_surface_context";
    let &B5OffsetSurface {
        carrier_surface,
        source_surface,
        distance,
        carrier_kind,
        parameter_bounds: [u_bounds, v_bounds],
        ..
    } = fields;
    let source_extrusion = ctx.get_btree_map(extrusion_surfaces, &source_surface, OPERATION)?;
    if carrier_kind == B5OffsetCarrierKind::Extrusion {
        if let (Some(source), Some(carrier)) = (
            source_extrusion,
            ctx.get_btree_map(extrusion_surfaces, &carrier_surface, OPERATION)?,
        ) {
            return Ok(extrusion_offset_construction_agrees(
                source,
                carrier,
                distance,
                [u_bounds, v_bounds],
            ));
        }
    }
    let source = ctx.get_btree_map(surfaces, &source_surface, OPERATION)?;
    let expected_kind = match ctx.get_btree_map(surfaces, &carrier_surface, OPERATION)? {
        Some(
            carrier @ (B5Surface::Plane { .. }
            | B5Surface::Cylinder { .. }
            | B5Surface::Sphere { .. }
            | B5Surface::Torus { .. }),
        ) => {
            let Some(source) = source else {
                return Ok(false);
            };
            if !analytic_offset_magnitude_agrees(carrier, source, distance.get()) {
                return Ok(false);
            }
            match carrier {
                B5Surface::Plane { .. } => B5OffsetCarrierKind::Plane,
                B5Surface::Cylinder { .. } => B5OffsetCarrierKind::Cylinder,
                B5Surface::Sphere { .. } => B5OffsetCarrierKind::Sphere,
                _ => B5OffsetCarrierKind::Torus,
            }
        }
        Some(B5Surface::RollingBall { .. }) => B5OffsetCarrierKind::RollingBall,
        Some(_) => return Ok(false),
        None => {
            let carrier_record = ctx.get_hash_map(records, &carrier_surface, OPERATION)?;
            if let (Some(source), Some(carrier)) = (
                source_extrusion,
                carrier_record.and_then(|record| extrusion_carrier(record)),
            ) {
                return Ok(carrier.direction == source.direction
                    && carrier.u_bounds == v_bounds
                    && carrier.v_bounds == Some(u_bounds)
                    && carrier_kind == B5OffsetCarrierKind::Extrusion);
            }
            let Some(cache) = carrier_record.and_then(|record| parse_offset_cache(record)) else {
                return Ok(false);
            };
            let (Some(source), Some(cached_source)) = (
                source,
                ctx.get_btree_map(surfaces, &cache.source_surface, OPERATION)?,
            ) else {
                return Ok(false);
            };
            let [u0, u1] = u_bounds.endpoints();
            let [v0, v1] = v_bounds.endpoints();
            if distance.get().to_bits() != cache.distance.get().to_bits()
                || [u0, v0, u1, v1]
                    .into_iter()
                    .zip(cache.interleaved_bounds)
                    .any(|(left, right)| left.to_bits() != right.get().to_bits())
                || !ctx.equal(source, cached_source, OPERATION)?
            {
                return Ok(false);
            }
            B5OffsetCarrierKind::Cache
        }
    };
    Ok(carrier_kind == expected_kind)
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
                .endpoints()
                .into_iter()
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
            .endpoints()
            .into_iter()
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
    distance: FiniteReal,
    interleaved_bounds: [FiniteReal; 4],
}

/// Fields of one object-stream pcurve candidate that directrix resolution
/// reads; the knots borrow the candidate.
struct B5ObjectStreamPcurve<'a> {
    class: u8,
    surface: u32,
    parameter_range: [FiniteReal; 2],
    class_21_suffix_scalar: Option<PositiveReal>,
    distinct_knots: &'a [FiniteReal],
}

fn parse_offset_cache(record: &B5Record) -> Option<B5OffsetCache> {
    (record.family == 0xb5 && record.class == 0x31 && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let source_surface = wire::tokens::object_ref(&record.payload, &mut position, true)?;
    let [distance, u0, v0, u1, v1] = read_f64_array::<5>(&record.payload, position)?;
    position += 40;
    (position == record.payload.len() && u0 < u1 && v0 < v1).then_some(B5OffsetCache {
        source_surface,
        distance,
        interleaved_bounds: [u0, v0, u1, v1],
    })
}

#[cfg(test)]
fn parse_extrusion_surface(
    record: &B5Record<'_>,
    records: &HashMap<u32, &B5Record<'_>>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve<'_>>,
) -> Option<B5ExtrusionSurface> {
    crate::test_support::with_service_context(|ctx| {
        parse_extrusion_surface_with_context(
            ctx,
            record,
            records,
            object_stream_pcurves,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
    })
    .expect("service budget")
}

/// Index the extrusion-kind offset constructions by their carrier surface,
/// keeping construction order within each carrier.
fn extrusion_offsets_by_carrier<'o>(
    ctx: &DecodeContext<'_>,
    offsets: &'o [B5OffsetSurface],
) -> Result<BTreeMap<u32, Vec<&'o B5OffsetSurface>>, CodecError> {
    let mut index = BTreeMap::new();
    for offset in ctx.admit_iter(offsets, "catia_b5_extrusion_offset_index_scan")? {
        if offset.carrier_kind == B5OffsetCarrierKind::Extrusion {
            ctx.push_btree_group(
                &mut index,
                offset.carrier_surface,
                offset,
                "catia_b5_extrusion_offset_index",
                "catia_b5_extrusion_offset_index_entries",
            )?;
        }
    }
    Ok(index)
}

fn parse_extrusion_surface_with_context(
    ctx: &DecodeContext<'_>,
    record: &B5Record<'_>,
    records: &HashMap<u32, &B5Record<'_>>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve<'_>>,
    extrusion_offsets: &BTreeMap<u32, Vec<&B5OffsetSurface>>,
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
) -> Result<Option<B5ExtrusionSurface>, CodecError> {
    const OPERATION: &str = "catia_b5_extrusion_context_lookup";
    let Some(carrier) = extrusion_carrier(record) else {
        return Ok(None);
    };
    let Some(active_bounds) = carrier.v_bounds else {
        let Some(directrix_record) = record_by_id(ctx, records, carrier.directrix_id, OPERATION)?
        else {
            return Ok(None);
        };
        let Some(directrix) =
            parse_extrusion_directrix(ctx, directrix_record, records, object_stream_pcurves)?
        else {
            return Ok(None);
        };
        let Some(parameter_bounds) = contextual_offset_extrusion_bounds(
            ctx,
            record.object_id,
            &carrier,
            &directrix,
            extrusion_offsets,
            extrusion_surfaces,
        )?
        else {
            return Ok(None);
        };
        return Ok(Some(B5ExtrusionSurface {
            object_id: record.object_id,
            direction: carrier.direction,
            parameter_bounds,
            directrix,
        }));
    };
    let active = active_bounds.endpoints();
    let terminal_span_chart = matches!(carrier.controls, [0x05, 0x15 | 0x19]);
    let directrix = if terminal_span_chart {
        terminal_span_directrix(
            ctx,
            carrier.directrix_id,
            active_bounds,
            carrier.controls,
            object_stream_pcurves,
        )?
    } else {
        let Some(directrix_record) = record_by_id(ctx, records, carrier.directrix_id, OPERATION)?
        else {
            return Ok(None);
        };
        parse_extrusion_directrix(ctx, directrix_record, records, object_stream_pcurves)?
    };
    let Some(mut directrix) = directrix else {
        return Ok(None);
    };
    let directrix_contains_active = active.into_iter().all(|value| {
        cadmpeg_ir::math::parameter_in_domain(
            value,
            directrix.parameter_range().endpoints(),
            64.0 * f64::EPSILON,
        )
    });
    let translated_chart = carrier.controls == [0x05, 0x11];
    if translated_chart {
        if translated_directrix_span_count(
            ctx,
            &directrix,
            active,
            carrier.controls,
            object_stream_pcurves,
        )?
        .is_none()
        {
            return Ok(None);
        }
        if !directrix.reorigin_parameter_range(active_bounds) {
            return Ok(None);
        }
    } else if !directrix_contains_active {
        let source_range = directrix.parameter_range().endpoints();
        let active_span = active[1] - active[0];
        let suffix_span = match directrix.supports().first() {
            Some(support) => ctx
                .get_btree_map(object_stream_pcurves, &support.1, OPERATION)?
                .and_then(|candidate| candidate.class_21_suffix_scalar),
            None => None,
        };
        if !parameter_range_spans_agree(source_range, active)
            || !suffix_span.is_some_and(|span| parameter_spans_agree(span.get(), active_span))
        {
            return Ok(None);
        }
        if !directrix.reorigin_parameter_range(active_bounds) {
            return Ok(None);
        }
    }
    Ok(Some(B5ExtrusionSurface {
        object_id: record.object_id,
        direction: carrier.direction,
        parameter_bounds: [carrier.u_bounds, active_bounds],
        directrix,
    }))
}

fn contextual_offset_extrusion_bounds(
    ctx: &DecodeContext<'_>,
    carrier_surface: u32,
    carrier: &B5ExtrusionCarrier,
    directrix: &B5ExtrusionDirectrix,
    extrusion_offsets: &BTreeMap<u32, Vec<&B5OffsetSurface>>,
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
) -> Result<Option<[IncreasingParameterInterval; 2]>, CodecError> {
    const OPERATION: &str = "catia_b5_contextual_offset_lookup";
    let B5ExtrusionDirectrix::Offset {
        source,
        distance,
        direction,
        parameter_range,
        ..
    } = directrix
    else {
        return Ok(None);
    };
    let Some(constructions) = ctx.get_btree_map(extrusion_offsets, &carrier_surface, OPERATION)?
    else {
        return Ok(None);
    };
    let mut resolved = None;
    for construction in ctx.admit_iter(constructions, "catia_b5_contextual_offset_scan")? {
        let Some(source_extrusion) =
            ctx.get_btree_map(extrusion_surfaces, &construction.source_surface, OPERATION)?
        else {
            return Ok(None);
        };
        let bounds = [
            construction.parameter_bounds[1],
            construction.parameter_bounds[0],
        ];
        if source.object_id() != source_extrusion.directrix.object_id()
            || carrier.direction != source_extrusion.direction
            || *direction != source_extrusion.direction
            || distance.get().to_bits() != construction.distance.get().to_bits()
            || carrier.u_bounds != bounds[0]
            || *parameter_range != bounds[1]
        {
            return Ok(None);
        }
        if resolved.is_some_and(|previous| previous != bounds) {
            return Ok(None);
        }
        resolved = Some(bounds);
    }
    Ok(resolved)
}

fn terminal_span_directrix(
    ctx: &DecodeContext<'_>,
    directrix_id: u32,
    active: IncreasingParameterInterval,
    controls: [u8; 2],
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve<'_>>,
) -> Result<Option<B5ExtrusionDirectrix>, CodecError> {
    let mut first_position = 0;
    let target_span_count = wire::tokens::compact_uint(&controls[..1], &mut first_position);
    let mut second_position = 0;
    let source_span_count = wire::tokens::compact_uint(&controls[1..], &mut second_position)
        .and_then(|count| usize::try_from(count).ok());
    let (Some(1), Some(source_span_count @ (5 | 6))) = (target_span_count, source_span_count)
    else {
        return Ok(None);
    };
    let Some(pcurve) = ctx.get_btree_map(
        object_stream_pcurves,
        &directrix_id,
        "catia_b5_terminal_directrix_lookup",
    )?
    else {
        return Ok(None);
    };
    Ok((|| {
        (pcurve.class == 0x20 && pcurve.distinct_knots.len() == source_span_count + 1)
            .then_some(())?;
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
        parameter_range_spans_agree(source_range.map(FiniteReal::get), active.endpoints())
            .then_some(B5ExtrusionDirectrix::SurfaceCurve {
                object_id: directrix_id,
                support: (pcurve.surface, directrix_id, source_range),
                parameter_range: active,
            })
    })())
}

fn translated_directrix_span_count(
    ctx: &DecodeContext<'_>,
    directrix: &B5ExtrusionDirectrix,
    active: [f64; 2],
    controls: [u8; 2],
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve<'_>>,
) -> Result<Option<usize>, CodecError> {
    let mut first_position = 0;
    let Some(target_span_count) = wire::tokens::compact_uint(&controls[..1], &mut first_position)
    else {
        return Ok(None);
    };
    let mut second_position = 0;
    let Some(source_span_count) = wire::tokens::compact_uint(&controls[1..], &mut second_position)
        .and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    if target_span_count != 1 || source_span_count <= 1 {
        return Ok(None);
    }
    let [support] = match directrix.supports() {
        [support] => [support],
        _ => return Ok(None),
    };
    let Some(pcurve) = ctx.get_btree_map(
        object_stream_pcurves,
        &support.1,
        "catia_b5_translated_directrix_lookup",
    )?
    else {
        return Ok(None);
    };
    if pcurve.class != 0x21 {
        return Ok(None);
    }
    let Some(suffix_span) = pcurve.class_21_suffix_scalar.map(PositiveReal::get) else {
        return Ok(None);
    };
    let active_span = active[1] - active[0];
    if !parameter_spans_agree(suffix_span, active_span) {
        return Ok(None);
    }
    let source = directrix.parameter_range().endpoints();
    let Some(start) = ctx.position_by(
        pcurve.distinct_knots,
        |knot| Ok(knot.get().to_bits() == source[0].to_bits()),
        "catia_b5_translated_directrix_start_search",
    )?
    else {
        return Ok(None);
    };
    let Some(end) = ctx.position_by(
        pcurve.distinct_knots,
        |knot| Ok(knot.get().to_bits() == source[1].to_bits()),
        "catia_b5_translated_directrix_end_search",
    )?
    else {
        return Ok(None);
    };
    let Some(span_count) = end.checked_sub(start) else {
        return Ok(None);
    };
    if span_count != source_span_count {
        return Ok(None);
    }
    let spans_agree = ctx.all_by(
        pcurve.distinct_knots[start..=end].windows(2),
        |knots| {
            Ok(parameter_spans_agree(
                knots[1].get() - knots[0].get(),
                suffix_span,
            ))
        },
        "catia_b5_translated_directrix_knot_window_scan",
    )?;
    Ok(spans_agree.then_some(source_span_count))
}

fn parameter_spans_agree(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= 64.0 * f64::EPSILON * scale
}

fn parameter_range_spans_agree(left: [f64; 2], right: [f64; 2]) -> bool {
    let left_span = left[1] - left[0];
    let right_span = right[1] - right[0];
    if left_span.is_finite() && right_span.is_finite() {
        parameter_spans_agree(left_span, right_span)
    } else {
        parameter_spans_agree(
            left[1] * 0.5 - left[0] * 0.5,
            right[1] * 0.5 - right[0] * 0.5,
        )
    }
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
    ctx: &DecodeContext<'_>,
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Result<Option<B5ExtrusionDirectrix>, CodecError> {
    if record.family == 0xb5 && record.class == 0x24 {
        return parse_surface_curve_directrix(ctx, record, records, object_stream_pcurves);
    }
    if record.family == 0xb5 && record.class == 0x14 {
        return parse_offset_curve_directrix(ctx, record, records, object_stream_pcurves);
    }
    let parsed = (|| -> Option<_> {
        (record.family == 0xa8 && record.class == 0x25 && record.payload.first() == Some(&0x82))
            .then_some(())?;
        let mut position = 1;
        let wrapper_id = wire::tokens::object_ref(&record.payload, &mut position, true)?;
        let second_pcurve = wire::tokens::object_ref(&record.payload, &mut position, true)?;
        let tail = record.payload.len().checked_sub(25)?;
        (position < tail).then_some(())?;
        let parameter_range = IncreasingParameterInterval::new(
            read_f64_array::<2>(&record.payload, tail)?.map(FiniteReal::get),
        )?;
        let cache_fit_tolerance = PositiveReal::new(f64_le(&record.payload, tail + 16)?.get())?;
        if record.payload.get(tail + 24) != Some(&0x01) {
            return None;
        }
        let wrapper =
            match ctx.get_hash_map(records, &wrapper_id, "catia_b5_directrix_record_lookup") {
                Ok(value) => *value?,
                Err(error) => return Some(Err(error)),
            };
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
                .zip(parameter_range.endpoints())
                .any(|(left, right)| left.to_bits() != right.to_bits())
        {
            return None;
        }
        let first =
            match ctx.get_hash_map(records, &first_pcurve, "catia_b5_directrix_record_lookup") {
                Ok(value) => *value?,
                Err(error) => return Some(Err(error)),
            };
        let first_surface = pcurve_surface_reference(first)?;
        let second = match ctx.get_btree_map(
            object_stream_pcurves,
            &second_pcurve,
            "catia_b5_directrix_pcurve_lookup",
        ) {
            Ok(value) => value?,
            Err(error) => return Some(Err(error)),
        };
        Some(Ok((
            first,
            first_surface,
            first_pcurve,
            second_pcurve,
            second.surface,
            second.parameter_range,
            parameter_range,
            cache_fit_tolerance,
        )))
    })()
    .transpose()?;
    let Some((
        first,
        first_surface,
        first_pcurve,
        second_pcurve,
        second_surface,
        second_range,
        parameter_range,
        cache_fit_tolerance,
    )) = parsed
    else {
        return Ok(None);
    };
    let Some(first_range) = analytic_pcurve_range(ctx, first)? else {
        return Ok(None);
    };
    Ok(Some(B5ExtrusionDirectrix::Intersection {
        object_id: record.object_id,
        supports: [
            (first_surface, first_pcurve, first_range),
            (second_surface, second_pcurve, second_range),
        ],
        parameter_range,
        cache_fit_tolerance,
    }))
}

fn parse_surface_curve_directrix(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Result<Option<B5ExtrusionDirectrix>, CodecError> {
    let parsed = (|| -> Option<_> {
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
        let interval = IncreasingParameterInterval::new([start.get(), end.get()])?;
        if record.payload.get(position..) != Some(&[0x01])
            || zero.get().to_bits() != 0.0f64.to_bits()
        {
            return None;
        }
        Some((pcurve, start, end, interval))
    })();
    let Some((pcurve, start, end, interval)) = parsed else {
        return Ok(None);
    };
    let (surface, pcurve_range) = if let Some(candidate) = ctx.get_btree_map(
        object_stream_pcurves,
        &pcurve,
        "catia_b5_directrix_pcurve_lookup",
    )? {
        (candidate.surface, candidate.parameter_range)
    } else {
        let Some(pcurve_record) =
            ctx.get_hash_map(records, &pcurve, "catia_b5_directrix_record_lookup")?
        else {
            return Ok(None);
        };
        let Some(surface) = pcurve_surface_reference(pcurve_record) else {
            return Ok(None);
        };
        let Some(range) = analytic_pcurve_range(ctx, pcurve_record)? else {
            return Ok(None);
        };
        (surface, range)
    };
    let parameter_range = [start, end];
    Ok(parameter_range
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
            parameter_range: interval,
        }))
}

fn parse_offset_curve_directrix(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, B5ObjectStreamPcurve>,
) -> Result<Option<B5ExtrusionDirectrix>, CodecError> {
    let parsed = (|| -> Option<_> {
        (record.family == 0xb5 && record.class == 0x14 && record.payload.first() == Some(&0x81))
            .then_some(())?;
        let mut position = 1;
        let source_id = wire::tokens::object_ref(&record.payload, &mut position, true)?;
        let source_parameter_range = IncreasingParameterInterval::new(
            read_f64_array::<2>(&record.payload, position)?.map(FiniteReal::get),
        )?;
        position += 16;
        if record.payload.get(position) != Some(&0x05) {
            return None;
        }
        position += 1;
        let [distance, x, y, z, start, end] = read_f64_array::<6>(&record.payload, position)?;
        let [x, y, z, start, end] = [x, y, z, start, end].map(FiniteReal::get);
        let parameter_range = IncreasingParameterInterval::new([start, end])?;
        position += 48;
        let source_record =
            match ctx.get_hash_map(records, &source_id, "catia_b5_directrix_record_lookup") {
                Ok(value) => *value?,
                Err(error) => return Some(Err(error)),
            };
        if !((source_record.family == 0xb5 && source_record.class == 0x24)
            || (source_record.family == 0xa8 && source_record.class == 0x25))
        {
            return None;
        }
        let direction = ExactUnitVector3::new([x, y, z])?.into();
        if position != record.payload.len() || distance.get() == 0.0 {
            return None;
        }
        Some(Ok((
            source_record,
            source_parameter_range,
            distance,
            direction,
            parameter_range,
        )))
    })()
    .transpose()?;
    let Some((source_record, source_parameter_range, distance, direction, parameter_range)) =
        parsed
    else {
        return Ok(None);
    };
    let Some(source) =
        parse_extrusion_directrix(ctx, source_record, records, object_stream_pcurves)?
    else {
        return Ok(None);
    };
    // A directrix exposes at most two supports, each with two endpoints.
    if !source.supports().iter().any(|support| {
        support
            .2
            .into_iter()
            .zip(source_parameter_range.endpoints())
            .all(|(left, right)| left.get().to_bits() == right.to_bits())
    }) {
        return Ok(None);
    }
    ctx.charge_collection_items(1, "catia_b5_offset_directrix_box")?;
    ctx.charge_retained(
        u64_from_index(std::mem::size_of::<B5ExtrusionDirectrix>()),
        "catia_b5_offset_directrix_box",
    )?;
    Ok(Some(B5ExtrusionDirectrix::Offset {
        object_id: record.object_id,
        source: Box::new(source),
        source_parameter_range,
        distance,
        direction,
        parameter_range,
    }))
}

fn pcurve_surface_reference(record: &B5Record) -> Option<u32> {
    (matches!(record.class, 0x18..=0x21)).then_some(())?;
    let mut position = usize::from(record.payload.first() == Some(&0x81));
    wire::tokens::object_ref(&record.payload, &mut position, true)
}

fn analytic_pcurve_range(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
) -> Result<Option<[FiniteReal; 2]>, CodecError> {
    let pcurve = match record.class {
        0x18 => parse_line_pcurve(ctx, record)?,
        0x19 => parse_circle_pcurve(ctx, record)?,
        _ => None,
    };
    Ok(pcurve.and_then(|pcurve| {
        Some([
            *pcurve.distinct_knots.first()?,
            *pcurve.distinct_knots.last()?,
        ])
    }))
}

fn parse_supported_surface(record: &B5Record) -> Option<B5SupportedSurface> {
    (record.family == 0xb5 && record.payload.first() == Some(&0x85)).then_some(())?;
    let mut position = 1;
    let references = [
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
        wire::tokens::object_ref(&record.payload, &mut position, true)?,
    ];
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
            let construction_radius =
                PositiveLength::new(f64_le(&record.payload, position + 2)?.get())?;
            (f64_le(&record.payload, position + 12)?.get() == 0.0).then_some(())?;
            B5SupportedSurfaceParameters::Radius {
                controls,
                construction_radius,
            }
        }
        0x3b => {
            let controls = record.payload[position..position + 6].try_into().ok()?;
            let scalars = [
                PositiveReal::new(f64_le(&record.payload, position + 6)?.get())?,
                PositiveReal::new(f64_le(&record.payload, position + 14)?.get())?,
            ];
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
        ) => relative_close(construction_radius.get(), radius.get()),
        (
            B5SupportedSurfaceParameters::Radius {
                construction_radius,
                ..
            },
            B5Surface::Torus { minor_radius, .. },
        ) => relative_close(construction_radius.get(), minor_radius.get()),
        (
            B5SupportedSurfaceParameters::Radius {
                construction_radius,
                ..
            },
            B5Surface::Sphere {
                construction_radius: carrier_radius,
                ..
            },
        ) => relative_close(construction_radius.get(), carrier_radius.get()),
        (B5SupportedSurfaceParameters::Radius { .. }, B5Surface::RollingBall { .. }) => true,
        (B5SupportedSurfaceParameters::ScalarPair { .. }, B5Surface::Plane { .. }) => true,
        (
            B5SupportedSurfaceParameters::ScalarPair { scalars, .. },
            B5Surface::Cone { half_angle, .. },
        ) => relative_close(scalars[1].get(), half_angle.get()),
        _ => false,
    }
}

fn supported_surface_pcurves_match(
    ctx: &DecodeContext<'_>,
    construction: &B5SupportedSurface,
    by_id: &HashMap<u32, &B5Record<'_>>,
    object_stream_pcurves: &BTreeMap<u32, B5Pcurve>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_b5_supported_surface_pcurve_lookup";
    // A construction has exactly two support sides.
    for (pcurve_id, support_id) in construction
        .support_pcurves
        .into_iter()
        .zip(construction.support_surfaces)
    {
        let Some(pcurve) = record_by_id(ctx, by_id, pcurve_id, OPERATION)? else {
            return Ok(false);
        };
        let matches = if pcurve.family == 0xa8 && pcurve.class == 0x20 {
            ctx.get_btree_map(object_stream_pcurves, &pcurve_id, OPERATION)?
                .is_some_and(|candidate| candidate.surface == support_id)
        } else {
            let mut position = 1;
            pcurve.payload.first() == Some(&0x81)
                && wire::tokens::object_ref(pcurve.payload, &mut position, true) == Some(support_id)
        };
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

fn lift_pcurve_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &B5Surface,
    profiles: &BTreeMap<u32, B5Profile>,
    endpoints: [[f64; 2]; 2],
) -> Result<Option<[FinitePoint3; 2]>, CodecError> {
    if let B5Surface::Nurbs(surface) = surface {
        let Some(start) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::nurbs_surface_point(
                    ctx,
                    surface,
                    endpoints[0][0],
                    endpoints[0][1],
                ),
            )?)?
        else {
            return Ok(None);
        };
        let Some(end) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::nurbs_surface_point(
                    ctx,
                    surface,
                    endpoints[1][0],
                    endpoints[1][1],
                ),
            )?)?
        else {
            return Ok(None);
        };
        return Ok(Some([start, end]));
    }
    let profile = match surface {
        B5Surface::Revolution { profile_curve, .. } => ctx.get_btree_map(
            profiles,
            profile_curve,
            "catia_b5_revolution_profile_lookup",
        )?,
        _ => None,
    };
    Ok((|| {
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
                Some(endpoints.map(|[u, v]| {
                    add(
                        origin,
                        add(scale(direction_u, u), scale(direction_v.get(), v)),
                    )
                }))
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
                frame,
                direction_y,
                half_angle,
                angular_scale,
                ..
            } => Some(endpoints.map(|[u, v]| {
                let angle = u / angular_scale.get();
                let radial = add(
                    scale(components(frame.reference()), angle.cos()),
                    scale(components(direction_y), angle.sin()),
                );
                add(
                    coordinates(*apex),
                    scale(
                        add(
                            scale(components(frame.axis()), half_angle.get().cos()),
                            scale(radial, half_angle.get().sin()),
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
                        scale(direction_y.get(), major_angle.sin()),
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
                axis_origin,
                axis_direction,
                profile_range,
                angular_scale,
                ..
            } => {
                let profile = profile?;
                (profile
                    .parameter_range()
                    .endpoints()
                    .into_iter()
                    .zip(profile_range.endpoints())
                    .all(|(profile, surface)| profile.to_bits() == surface.to_bits()))
                .then_some(())?;
                Some(endpoints.map(|[u, v]| {
                    let point = match profile {
                        B5Profile::Line {
                            point, direction, ..
                        } => add(coordinates(*point), scale(direction.get(), u)),
                        B5Profile::Arc {
                            center,
                            direction_x,
                            direction_y,
                            radius,
                            ..
                        } => {
                            let angle = u / radius.get();
                            add(
                                coordinates(*center),
                                scale(
                                    add(
                                        scale(direction_x.get(), angle.cos()),
                                        scale(direction_y.get(), angle.sin()),
                                    ),
                                    radius.get(),
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
            B5Surface::Nurbs(_) => None,
        };
        let [start, end] = lifted?;
        Some([
            FinitePoint3::new(Point3::from(start))?,
            FinitePoint3::new(Point3::from(end))?,
        ])
    })())
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

fn parse_pcurve(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
) -> Result<Option<B5Pcurve>, CodecError> {
    (|| -> Option<Result<B5Pcurve, CodecError>> {
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
        // The record holds exactly two knots and at most six poles, so its
        // lanes are fixed-width reads.
        let knot_lane = record.payload.get(position..position + 16)?;
        let mut distinct_knots = Vec::new();
        for chunk in knot_lane.chunks_exact(8) {
            let knot = f64_le(chunk, 0)?;
            if let Err(error) =
                ctx.push_vec(&mut distinct_knots, knot, "catia_b5_class21_distinct_knots")
            {
                return Some(Err(error));
            }
        }
        position += 16;
        if distinct_knots[0] >= distinct_knots[1] {
            return None;
        }
        let multiplicities = match ctx.collect_fallible_options(
            (0..knot_count).map(|_| {
                Ok::<_, CodecError>(wire::tokens::compact_uint(&record.payload, &mut position))
            }),
            "catia_b5_class21_multiplicities",
        ) {
            Ok(Some(values)) => values,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let endpoint_multiplicity = degree + 1;
        if multiplicities != [endpoint_multiplicity; 2] {
            return None;
        }
        let pole_count = usize::try_from(endpoint_multiplicity).ok()?;
        let point_lane = record
            .payload
            .get(position..position.checked_add(pole_count * 16)?)?;
        let mut control_points = Vec::new();
        for chunk in point_lane.chunks_exact(16) {
            let point = FiniteVector::new([f64_le(chunk, 0)?.get(), f64_le(chunk, 8)?.get()])?;
            if let Err(error) = ctx.push_vec(
                &mut control_points,
                point,
                "catia_b5_class21_control_points",
            ) {
                return Some(Err(error));
            }
        }
        position += point_lane.len();
        let tail = record.payload.get(position..)?;
        let suffix_scalar = PositiveReal::new(f64_le(tail, 10)?.get())?;
        let native_origin = *distinct_knots.first()?;
        let native_span = distinct_knots[1].get() - native_origin.get();
        if tail.len() != 36
            || tail.get(..2) != Some(&[0x05, 0x05])
            || f64_le(tail, 2)?.get() != 0.0
            || suffix_scalar.get().to_bits() != native_span.to_bits()
            || f64_le(tail, 18)?.get() != 1.0
            || f64_le(tail, 26)?.get() != 0.0
            || tail.get(34..) != Some(&[0x00, 0x07])
        {
            return None;
        }
        Some(Ok(B5Pcurve {
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
        }))
    })()
    .transpose()
}

fn parse_circle_pcurve(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
) -> Result<Option<B5Pcurve>, CodecError> {
    let Some((surface, center, radius, range, angles)) = parse_circle_pcurve_fields(record) else {
        return Ok(None);
    };
    rational_arc_pcurve(
        ctx,
        crate::families::b5::graph::RationalArcPcurveInputs {
            record,
            surface,
            center,
            reference_x: [1.0, 0.0],
            reference_y: [0.0, 1.0],
            radius,
            parameter_range: range,
            angle_range: angles,
        },
    )
}

fn parse_circle_pcurve_fields(record: &B5Record) -> CirclePcurveFields {
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
    Some((
        surface,
        center,
        radius,
        [start, end],
        [start_angle, end_angle],
    ))
}

fn parse_class_1a_pcurve(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
) -> Result<Option<B5Pcurve>, CodecError> {
    let Some((surface, center, reference_x, reference_y, radius, range, angles)) =
        parse_class_1a_pcurve_fields(record)
    else {
        return Ok(None);
    };
    rational_arc_pcurve(
        ctx,
        crate::families::b5::graph::RationalArcPcurveInputs {
            record,
            surface,
            center,
            reference_x,
            reference_y,
            radius,
            parameter_range: range,
            angle_range: angles,
        },
    )
}

fn parse_class_1a_pcurve_fields(record: &B5Record) -> Class1aPcurveFields {
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
    Some((
        surface,
        center,
        reference_x,
        reference_y,
        diameter * 0.5,
        [start, end],
        angles,
    ))
}
#[derive(Clone, Copy)]
struct RationalArcPcurveInputs<'input0> {
    record: &'input0 B5Record<'input0>,
    surface: u32,
    center: [f64; 2],
    reference_x: [f64; 2],
    reference_y: [f64; 2],
    radius: f64,
    parameter_range: [f64; 2],
    angle_range: [f64; 2],
}

fn rational_arc_pcurve(
    ctx: &DecodeContext<'_>,
    inputs: RationalArcPcurveInputs<'_>,
) -> Result<Option<B5Pcurve>, CodecError> {
    let RationalArcPcurveInputs {
        record,
        surface,
        center,
        reference_x,
        reference_y,
        radius,
        parameter_range,
        angle_range,
    } = inputs;

    let [start, end] = parameter_range;
    let [start_angle, end_angle] = angle_range;
    let span_count = ((end_angle - start_angle).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS {
        return Ok(None);
    }
    // `ceil` answers zero only for an angular span of exactly zero: an arc that
    // sweeps no angle states no span, which this route refuses as it refuses
    // every other degeneracy.
    let Some(span_count) = truncate_f64_to_usize(span_count).and_then(std::num::NonZeroUsize::new)
    else {
        return Ok(None);
    };
    let span_count = span_count.get();
    let Some(control_count) = span_count
        .checked_mul(2)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    // Each lane is reserved at its exact final length and validated as it is
    // filled; an invalid value withholds the pcurve.
    let mut control_points = Vec::<FiniteVector<2>>::new();
    let mut weights = Vec::<PositiveReal>::new();
    let mut distinct_knots = Vec::<FiniteReal>::new();
    let mut multiplicities = Vec::new();
    ctx.reserve_vec(
        &mut control_points,
        control_count,
        "catia_b5_arc_control_points",
    )?;
    ctx.reserve_vec(&mut weights, control_count, "catia_b5_arc_weights")?;
    ctx.reserve_vec(
        &mut distinct_knots,
        span_count + 1,
        "catia_b5_arc_distinct_knots",
    )?;
    ctx.reserve_vec(
        &mut multiplicities,
        span_count + 1,
        "catia_b5_arc_multiplicities",
    )?;
    let arc_point = |angle: f64, scale: f64| {
        FiniteVector::new([
            center[0] + scale * (reference_x[0] * angle.cos() + reference_y[0] * angle.sin()),
            center[1] + scale * (reference_x[1] * angle.cos() + reference_y[1] * angle.sin()),
        ])
    };
    let (Some(start_knot), Some(end_knot), Some(unit_weight)) = (
        FiniteReal::new(start),
        FiniteReal::new(end),
        PositiveReal::new(1.0),
    ) else {
        return Ok(None);
    };
    distinct_knots.push(start_knot);
    multiplicities.push(3);
    let (Some(span_total), true) = (f64_from_index(span_count), span_count > 0) else {
        return Ok(None);
    };
    for span in ctx.admit_iter(&(0..span_count), "catia_b5_rational_arc_span_generation")? {
        let (Some(span_start), Some(span_end)) = (f64_from_index(span), f64_from_index(span + 1))
        else {
            return Ok(None);
        };
        let fraction0 = span_start / span_total;
        let fraction1 = span_end / span_total;
        let angle0 = start_angle + (end_angle - start_angle) * fraction0;
        let angle1 = start_angle + (end_angle - start_angle) * fraction1;
        let middle = (angle0 + angle1) * 0.5;
        let middle_weight = ((angle1 - angle0) * 0.5).cos();
        if middle_weight <= f64::EPSILON {
            return Ok(None);
        }
        if span == 0 {
            let Some(point) = arc_point(angle0, radius) else {
                return Ok(None);
            };
            control_points.push(point);
            weights.push(unit_weight);
        }
        let (Some(middle_point), Some(end_point), Some(middle_weight)) = (
            arc_point(middle, radius / middle_weight),
            arc_point(angle1, radius),
            PositiveReal::new(middle_weight),
        ) else {
            return Ok(None);
        };
        control_points.extend([middle_point, end_point]);
        weights.extend([middle_weight, unit_weight]);
        if span + 1 < span_count {
            let ordinary = start + (end - start) * fraction1;
            let knot = if ordinary.is_finite() {
                FiniteReal::new(ordinary)
            } else {
                cadmpeg_ir::math::interpolate(start, end, fraction1)
            };
            let Some(knot) = knot else {
                return Ok(None);
            };
            distinct_knots.push(knot);
            multiplicities.push(2);
        }
    }
    distinct_knots.push(end_knot);
    multiplicities.push(3);
    Ok(Some(B5Pcurve {
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
    }))
}

/// Keep a structurally bounded pcurve record whose chart geometry is not
/// resolved. `resolved` states whether the record already parsed as a
/// resolved pcurve, which then is not opaque.
fn parse_opaque_pcurve(
    ctx: &DecodeContext<'_>,
    record: &B5Record<'_>,
    resolved: bool,
) -> Result<Option<B5OpaquePcurve>, CodecError> {
    if resolved {
        return Ok(None);
    }
    let Some(surface) = parse_opaque_pcurve_surface(record) else {
        return Ok(None);
    };
    Ok(Some(B5OpaquePcurve {
        object_id: record.object_id,
        surface,
        class: record.class,
        payload: ctx.copy_slice(record.payload, "catia_b5_opaque_pcurve_payload")?,
        sphere_great_circle: None,
    }))
}

fn parse_opaque_pcurve_surface(record: &B5Record) -> Option<u32> {
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
    Some(surface)
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

    let surface_u_bounds = azimuth_range
        .endpoints()
        .map(|angle| chart_scale.get() * angle);
    let u_scale = surface_u_bounds
        .into_iter()
        .chain([u0, u1])
        .map(f64::abs)
        .fold(1.0, f64::max);
    let u_tolerance = EPS_B5_GRAPH_EXACT_GEOMETRY * u_scale;
    (direction.abs() == 1.0
        && zero0 == 0.0
        && zero1 == 0.0
        && chart_scale.get() == sphere_chart_scale.get()
        && reciprocal_scale == -direction / chart_scale.get()
        && u0 >= surface_u_bounds[0] - u_tolerance
        && u1 <= surface_u_bounds[1] + u_tolerance
        && v0 == chart_origin.get()
        && v1 == chart_origin.get() + std::f64::consts::TAU * chart_scale.get())
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
    let (center, direction_x, direction_y, axis, radius) = (
        coordinates(*center),
        components(frame.reference()),
        components(direction_y),
        components(frame.axis()),
        radius.get(),
    );
    let parameter = parameter.get();
    let chart_scale = pcurve.chart_scale.get();
    if parameter < pcurve.u_bounds.lower()
        || parameter > pcurve.u_bounds.upper()
        || chart_scale != construction_radius.get()
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

fn circle_pcurves_from_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frames: &[ObjectFrame],
) -> Result<Vec<B5Pcurve>, CodecError> {
    let mut pcurves = Vec::new();
    for frame in ctx.admit_iter(frames, "catia_b5_circle_pcurve_frame_scan")? {
        if frame.family != 0xb5 || frame.class != 0x19 {
            continue;
        }
        let Some(record) = record_from_frame(bytes, frame) else {
            continue;
        };
        if let Some(pcurve) = parse_circle_pcurve(ctx, &record)? {
            ctx.push_vec(&mut pcurves, pcurve, "catia_b5_circle_frame_pcurves")?;
        }
    }
    Ok(pcurves)
}

fn parse_line_pcurve(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
) -> Result<Option<B5Pcurve>, CodecError> {
    let Some((surface, start, end, [Some(start_point), Some(end_point)])) =
        parse_line_pcurve_fields(record)
    else {
        return Ok(None);
    };
    Ok(Some(B5Pcurve {
        object_id: record.object_id,
        surface,
        degree: 1,
        distinct_knots: ctx.collect_vec([start, end], "catia_b5_line_pcurve_knots")?,
        multiplicities: ctx.collect_vec([2, 2], "catia_b5_line_pcurve_multiplicities")?,
        control_points: ctx.collect_vec([start_point, end_point], "catia_b5_line_pcurve_points")?,
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    }))
}

fn parse_line_pcurve_fields(
    record: &B5Record,
) -> Option<(u32, FiniteReal, FiniteReal, [Option<FiniteVector<2>>; 2])> {
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
                [
                    FiniteVector::new([u + start.get() * du, v + start.get() * dv]),
                    FiniteVector::new([u + end.get() * du, v + end.get() * dv]),
                ],
            )
        }
        0x05 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = read_f64_array::<3>(&record.payload, position)?;
            (
                start,
                end,
                [
                    Some(FiniteVector::from([constant, start])),
                    Some(FiniteVector::from([constant, end])),
                ],
            )
        }
        0x09 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = read_f64_array::<3>(&record.payload, position)?;
            (
                start,
                end,
                [
                    Some(FiniteVector::from([start, constant])),
                    Some(FiniteVector::from([end, constant])),
                ],
            )
        }
        _ => return None,
    };
    if start >= end {
        return None;
    }
    Some((surface, start, end, control_points))
}

#[cfg(test)]
fn records(bytes: &[u8]) -> Vec<B5Record<'_>> {
    crate::test_support::with_service_context(|ctx| {
        let frames = collect_object_stream_frames(ctx, bytes)?;
        records_from_frames(ctx, bytes, &frames)
    })
    .expect("service budget")
}

fn records_from_frames<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    frames: &[ObjectFrame],
) -> Result<Vec<B5Record<'a>>, CodecError> {
    records_from_frames_budgeted(ctx, bytes, frames, None)
}

/// Borrow the topology records of a frame population and close them over
/// their referenced dependencies, with an optional session work budget.
///
/// The dependency candidates are indexed once by object identity, so each
/// discovered dependency costs one keyed lookup.
fn records_from_frames_budgeted<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    frames: &[ObjectFrame],
    budget: Option<&WorkBudget<'_>>,
) -> Result<Vec<B5Record<'a>>, CodecError> {
    if budget.is_some_and(|budget| !budget.charge_by(frames.len())) {
        return Ok(Vec::new());
    }
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_record_closure_scratch")?;
    let Some((records, candidates)) =
        topology_records_and_dependency_candidates(ctx, bytes, frames, None, &mut scratch)?
    else {
        return Ok(Vec::new());
    };
    admit_dependency_records(ctx, bytes, records, &candidates, budget, &mut scratch)
}

/// Materialize one already-indexed topology population.
///
/// The caller has already charged the frame index. This path charges each
/// topology record as it is borrowed, then charges each admitted dependency
/// in the closure fixpoint.
fn records_from_indexed_frames_budgeted<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    frames: &[ObjectFrame],
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<Vec<B5Record<'a>>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_record_closure_scratch")?;
    let Some((records, candidates)) =
        topology_records_and_dependency_candidates(ctx, bytes, frames, budget, &mut scratch)?
    else {
        return Ok(None);
    };
    let records = admit_dependency_records(ctx, bytes, records, &candidates, budget, &mut scratch)?;
    if budget.is_some_and(WorkBudget::exhausted) {
        Ok(None)
    } else {
        Ok(Some(records))
    }
}

/// Queues each reference of a record that names a dependency candidate frame.
fn queue_dependency_references(
    ctx: &DecodeContext<'_>,
    record: &B5Record<'_>,
    candidates: &DependencyCandidates,
    pending: &mut BTreeSet<u32>,
) -> Result<(), CodecError> {
    let mut references = record_references(record);
    while let Some(reference) =
        ctx.next_charged(&mut references, "catia_b5_dependency_reference_scan")?
    {
        if ctx
            .get_hash_map(
                candidates,
                &reference,
                "catia_b5_dependency_candidate_lookup",
            )?
            .is_some_and(Option::is_some)
        {
            ctx.insert_btree_set(pending, reference, "catia_b5_pending_dependency_ids")?;
        }
    }
    Ok(())
}

/// Appends every dependency frame reachable from `records` through counted
/// references, one breadth layer at a time in source order. The visited and
/// pending identity sets live in `scratch`.
fn admit_dependency_records<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    mut records: Vec<B5Record<'a>>,
    candidates: &DependencyCandidates,
    budget: Option<&WorkBudget<'_>>,
    scratch: &mut ScopedReservation<'_>,
) -> Result<Vec<B5Record<'a>>, CodecError> {
    const VISITED: &str = "catia_b5_visited_dependency_ids";
    let mut visited = HashSet::new();
    let mut pending = BTreeSet::new();
    scratch.with_storage(|| {
        for record in ctx.admit_iter(&records, "catia_b5_dependency_record_reference_scan")? {
            ctx.insert_hash_set(&mut visited, record.object_id, VISITED)?;
            queue_dependency_references(ctx, record, candidates, &mut pending)?;
        }
        Ok::<_, CodecError>(())
    })?;
    loop {
        ctx.retain_btree_set(
            &mut pending,
            |object_id| {
                Ok(!ctx.contains_hash_set(
                    &visited,
                    object_id,
                    "catia_b5_pending_dependency_filter",
                )?)
            },
            "catia_b5_pending_dependency_filter",
        )?;
        if pending.is_empty() {
            break;
        }
        if budget.is_some_and(|budget| !budget.charge_by(pending.len())) {
            break;
        }
        let mut found = scratch.with_storage(|| {
            let mut found = Vec::new();
            for object_id in ctx.admit_iter(&pending, "catia_b5_pending_dependency_scan")? {
                if let Some(record) = ctx
                    .get_hash_map(candidates, object_id, "catia_b5_pending_dependency_frame")?
                    .and_then(Option::as_ref)
                    .and_then(|frame| record_from_frame(bytes, frame))
                {
                    ctx.push_vec(&mut found, record, "catia_b5_found_dependency_records")?;
                }
            }
            Ok::<_, CodecError>(found)
        })?;
        if found.is_empty() {
            break;
        }
        ctx.sort_unstable_by(
            &mut found,
            |value| &value.offset,
            Ord::cmp,
            "catia_b5_dependency_found_sort",
        )?;
        pending.clear();
        for candidate in ctx.admit_iter(found, "catia_b5_found_dependency_scan")? {
            scratch.with_storage(|| {
                ctx.insert_hash_set(&mut visited, candidate.object_id, VISITED)?;
                queue_dependency_references(ctx, &candidate, candidates, &mut pending)
            })?;
            ctx.push_vec(
                &mut records,
                candidate,
                "catia_b5_admitted_dependency_records",
            )?;
        }
    }
    Ok(records)
}

/// Borrow the topology records of a frame population and index its exact
/// dependency candidates by object identity in one frame pass. A repeated
/// identity whose frame bytes differ leaves its candidate ambiguous. With
/// `per_record`, each topology record charges one step of that budget and an
/// exhausted budget answers `None`. The seen-record and candidate indexes
/// live in `scratch`.
fn topology_records_and_dependency_candidates<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    frames: &[ObjectFrame],
    per_record: Option<&WorkBudget<'_>>,
    scratch: &mut ScopedReservation<'_>,
) -> Result<Option<(Vec<B5Record<'a>>, DependencyCandidates)>, CodecError> {
    const CANDIDATES: &str = "catia_b5_dependency_candidates";
    const SEEN: &str = "catia_b5_seen_records";
    let mut records = Vec::new();
    let mut seen = HashMap::<u32, (u8, &[u8])>::new();
    let mut candidates = DependencyCandidates::new();
    for frame in ctx.admit_iter(frames, "catia_b5_framed_record_dependency_scan")? {
        if is_reference_dependency_class(frame.family, frame.class)
            && frame_payload(bytes, frame).is_some()
        {
            if let Some(slot) =
                ctx.get_mut_hash_map(&mut candidates, &frame.object_id, CANDIDATES)?
            {
                if let Some(existing) = slot {
                    if !same_object_frame(ctx, bytes, existing, frame)? {
                        *slot = None;
                    }
                }
            } else {
                scratch.with_storage(|| {
                    ctx.insert_hash_map(&mut candidates, frame.object_id, Some(*frame), CANDIDATES)
                })?;
            }
        }
        if !((frame.family == 0xb5 && is_topology_class(frame.class))
            || (frame.family == 0xa8 && matches!(frame.class, 0x34 | 0x62)))
        {
            continue;
        }
        if per_record.is_some_and(|budget| !budget.charge()) {
            return Ok(None);
        }
        let Some(record) = record_from_frame(bytes, frame) else {
            continue;
        };
        if let Some(&(class, payload)) = ctx.get_hash_map(&seen, &frame.object_id, SEEN)? {
            if class == record.class && ctx.equal_bytes(payload, record.payload, SEEN)? {
                continue;
            }
        }
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut seen,
                frame.object_id,
                (record.class, record.payload),
                SEEN,
            )
        })?;
        ctx.push_vec(&mut records, record, "catia_b5_framed_records")?;
    }
    // A record nested in an A8 frame ends inside its wrapper, so ordering by
    // frame end and then by descending start puts each child first.
    ctx.sort_unstable_by_key(
        &mut records,
        |record| (record.frame_end(), std::cmp::Reverse(record.offset)),
        Ord::cmp,
        "catia_b5_framed_records_sort",
    )?;
    Ok(Some((records, candidates)))
}

fn same_object_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    left: &ObjectFrame,
    right: &ObjectFrame,
) -> Result<bool, CodecError> {
    let (Some(left_bytes), Some(right_bytes)) = (
        bytes.get(left.start..left.end),
        bytes.get(right.start..right.end),
    ) else {
        return Ok(false);
    };
    Ok(left.family == right.family
        && left.class == right.class
        && ctx.equal_bytes(
            left_bytes,
            right_bytes,
            "catia_b5_dependency_frame_identity",
        )?)
}

fn frame_payload<'a>(bytes: &'a [u8], frame: &ObjectFrame) -> Option<&'a [u8]> {
    let header = if frame.family == 0xa8 { 11 } else { 8 };
    bytes.get(frame.start.checked_add(header)?..frame.end)
}

/// Borrows one framed record from its stream bytes.
fn record_from_frame<'a>(bytes: &'a [u8], frame: &ObjectFrame) -> Option<B5Record<'a>> {
    Some(B5Record {
        offset: frame.start,
        family: frame.family,
        class: frame.class,
        object_id: frame.object_id,
        payload: frame_payload(bytes, frame)?,
    })
}

#[cfg(test)]
fn framed_records<'a>(bytes: &'a [u8], frames: &[ObjectFrame]) -> Vec<B5Record<'a>> {
    let mut records = Vec::new();
    let mut seen = HashMap::<u32, (u8, &[u8])>::new();
    for frame in frames {
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
            .is_some_and(|&(class, payload)| class == record.class && payload == record.payload)
        {
            continue;
        }
        seen.insert(frame.object_id, (record.class, record.payload));
        records.push(record);
    }
    records.sort_unstable_by_key(|record| (record.frame_end(), std::cmp::Reverse(record.offset)));
    records
}

/// Return complete byte ranges for length-closed object-stream records.
pub(in crate::families) fn framed_ranges<'a>(
    ctx: &'a DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<impl Iterator<Item = Result<Range<usize>, CodecError>> + 'a, CodecError> {
    Ok(object_stream_frames(ctx, bytes)?.map(|frame| frame.map(|frame| frame.start..frame.end)))
}

/// Scan frames without allocating a frame index.
pub(in crate::families) fn object_stream_frames<'a>(
    ctx: &'a DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<impl Iterator<Item = Result<ObjectFrame, CodecError>> + 'a, CodecError> {
    let mut child = None::<(usize, usize)>;
    let mut skip_until = 0usize;
    let mut failed = false;
    Ok(ctx
        .admit_iter(bytes, "catia_b5_object_frame_scan")?
        .enumerate()
        .filter_map(move |(position, _)| {
            if failed || position < skip_until {
                return None;
            }
            if child.is_some_and(|(_, end)| position >= end) {
                child = None;
            }
            let (inside_child, limit) = match child {
                Some((start, end)) if position >= start => (true, end),
                Some((_, end)) => (false, end),
                None => (false, bytes.len()),
            };
            if position.checked_add(8).is_none_or(|next| next > limit) {
                if inside_child {
                    child = None;
                    skip_until = limit;
                }
                return None;
            }
            let Some((end, family, class, object_id)) = object_frame(&bytes[..limit], position)
            else {
                if inside_child {
                    child = None;
                    skip_until = limit;
                }
                return None;
            };
            if inside_child {
                if family != 0xb5 {
                    child = None;
                    skip_until = limit;
                    return None;
                }
                skip_until = end;
            } else {
                match family {
                    0xa8 => {
                        match crate::families::a5a8::records::a8_nested_b5_run_start(
                            ctx, bytes, position, end,
                        ) {
                            Ok(Some(child_start)) => {
                                child = Some((child_start, end));
                                skip_until = child_start;
                            }
                            Ok(None) => skip_until = end,
                            Err(error) => {
                                failed = true;
                                return Some(Err(error));
                            }
                        }
                    }
                    0xb5 => skip_until = end,
                    _ => return None,
                }
            }
            Some(Ok(ObjectFrame {
                start: position,
                end,
                family,
                class,
                object_id,
            }))
        }))
}

pub(in crate::families) fn collect_object_stream_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<ObjectFrame>, CodecError> {
    ctx.try_collect_vec(object_stream_frames(ctx, bytes)?, "catia_b5_object_frames")
}

/// Return maximal contiguous top-level A8/B5 object-frame runs. A run chains
/// frames, 15-byte vertex allocations and external A8 grids; each scan step
/// and each chained allocation charges one work unit.
fn object_stream_run_ranges(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<Range<usize>>, CodecError> {
    const OPERATION: &str = "catia_b5_object_run_scan";
    const GRIDS: &str = "catia_b5_external_grid_index";
    let mut scratch = ctx.reserve_scoped(0, GRIDS)?;
    // The first grid at each start bounds that allocation.
    let grid_ends = scratch.with_storage(|| {
        let external_grids = crate::families::a5a8::records::a8_external_grid_ranges(ctx, bytes)?;
        let mut ends = HashMap::new();
        for grid in ctx.admit_iter(&external_grids, GRIDS)? {
            ctx.entry_hash_map(&mut ends, grid.start, GRIDS)?
                .or_insert(grid.end);
        }
        Ok::<_, CodecError>(ends)
    })?;
    let mut ranges = Vec::new();
    let mut position = 0usize;
    while position + 8 <= bytes.len() {
        ctx.charge_work(1, OPERATION)?;
        let Some((end, _, _, _)) = object_frame(bytes, position) else {
            position += 1;
            continue;
        };
        let start = position;
        position = end;
        loop {
            ctx.charge_work(1, OPERATION)?;
            if let Some((end, _, _, _)) = object_frame(bytes, position) {
                position = end;
                continue;
            }
            let coordinate_end = position.checked_add(15).filter(|&end| end <= bytes.len());
            let allocation_end = if let Some(end) = coordinate_end {
                crate::wire::records::scan_vertex_record_ranges(ctx, &bytes[position..end])?
                    .eq(std::iter::once(0..15))
                    .then_some(end)
            } else {
                None
            };
            let allocation_end = match allocation_end {
                Some(end) => Some(end),
                None => ctx
                    .get_hash_map(&grid_ends, &position, "catia_b5_external_grid_range_lookup")?
                    .copied(),
            };
            let Some(end) = allocation_end else {
                break;
            };
            position = end;
        }
        ctx.push_vec(&mut ranges, start..position, "catia_b5_object_run_ranges")?;
    }
    Ok(ranges)
}

/// Return runs that declare at least one face or loop topology root.
#[cfg(test)]
fn topology_root_run_ranges(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<Range<usize>>, CodecError> {
    let (_, runs) = index_object_runs(ctx, bytes, 0)?;
    let mut roots = Vec::new();
    for run in ctx.admit_iter(&runs, "catia_b5_topology_run_ranges_scan")? {
        if run.topology {
            ctx.push_vec(
                &mut roots,
                run.range.clone(),
                "catia_b5_topology_run_ranges",
            )?;
        }
    }
    Ok(roots)
}

fn is_topology_root_frame(frame: &ObjectFrame) -> bool {
    (frame.family == 0xb5 && frame.class == 0x5f)
        || ((frame.family == 0xb5 || frame.family == 0xa8) && frame.class == 0x62)
}

/// One contiguous object run of a logical stream, with the slice of the
/// stream's frame index that lies inside it.
struct IndexedObjectRun {
    stream_index: usize,
    range: Range<usize>,
    frame_range: Range<usize>,
    topology: bool,
}

/// Index one logical stream's frames once and place each object run on its
/// slice of that index.
fn index_object_runs(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    stream_index: usize,
) -> Result<(Vec<ObjectFrame>, Vec<IndexedObjectRun>), CodecError> {
    let ranges = object_stream_run_ranges(ctx, stream)?;
    let frames = collect_object_stream_frames(ctx, stream)?;
    let mut runs = Vec::new();
    place_object_runs(ctx, ranges, &frames, stream_index, &mut runs)?;
    Ok((frames, runs))
}

/// Append each run of one stream with the slice of the stream's frame index
/// that lies inside it. Frames and runs are both in stream order, so one
/// forward cursor visits each frame at most once across all runs.
fn place_object_runs(
    ctx: &DecodeContext<'_>,
    ranges: Vec<Range<usize>>,
    frames: &[ObjectFrame],
    stream_index: usize,
    runs: &mut Vec<IndexedObjectRun>,
) -> Result<(), CodecError> {
    const CURSOR: &str = "catia_b5_run_frame_cursor";
    ctx.reserve_vec(runs, ranges.len(), "catia_b5_indexed_runs")?;
    let mut cursor = 0;
    for range in ctx.admit_iter(ranges, "catia_b5_indexed_run_scan")? {
        let rest = &frames[cursor..];
        let frame_start = cursor
            + ctx
                .position_by(rest, |frame| Ok(frame.start >= range.start), CURSOR)?
                .unwrap_or(rest.len());
        let rest = &frames[frame_start..];
        let frame_end = frame_start
            + ctx
                .position_by(rest, |frame| Ok(frame.end > range.end), CURSOR)?
                .unwrap_or(rest.len());
        cursor = frame_end;
        let topology = ctx.any_by(
            &frames[frame_start..frame_end],
            |frame| Ok(is_topology_root_frame(frame)),
            "catia_b5_topology_root_frame_scan",
        )?;
        runs.push(IndexedObjectRun {
            stream_index,
            range,
            frame_range: frame_start..frame_end,
            topology,
        });
    }
    Ok(())
}

/// Partition one logical stream into independently resolved object populations.
pub(in crate::families) fn object_stream_populations(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<Vec<u8>>, CodecError> {
    const CLAIMED: &str = "catia_b5_claimed_isolated_ids";
    let (frames, runs) = index_object_runs(ctx, stream, 0)?;
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_population_partition_scratch")?;
    let mut topology_populations = Vec::new();
    let mut claimed_isolated_ids = HashSet::new();
    for (index, run) in ctx
        .admit_iter(&runs, "catia_b5_topology_population_scan")?
        .enumerate()
    {
        if !run.topology {
            continue;
        }
        let (population, isolated) =
            owned_object_stream_population(ctx, stream, &frames, &runs, index)?;
        scratch.with_storage(|| {
            let mut root_ids = HashSet::new();
            for frame in ctx.admit_iter(
                &frames[run.frame_range.clone()],
                "catia_b5_population_root_id_scan",
            )? {
                ctx.insert_hash_set(
                    &mut root_ids,
                    frame.object_id,
                    "catia_b5_population_root_ids",
                )?;
            }
            for &isolated_index in ctx.admit_iter(&isolated, "catia_b5_claimed_run_scan")? {
                let frame_range = runs[isolated_index].frame_range.clone();
                for frame in ctx.admit_iter(&frames[frame_range], "catia_b5_claimed_frame_scan")? {
                    if !ctx.contains_hash_set(&root_ids, &frame.object_id, CLAIMED)? {
                        ctx.insert_hash_set(&mut claimed_isolated_ids, frame.object_id, CLAIMED)?;
                    }
                }
            }
            ctx.push_vec(
                &mut topology_populations,
                population,
                "catia_b5_topology_populations",
            )
        })?;
    }
    let mut topology_populations = topology_populations.into_iter();
    let mut populations = Vec::new();
    for run in ctx.admit_iter(&runs, "catia_b5_object_population_scan")? {
        if run.topology {
            if let Some(population) = topology_populations.next() {
                ctx.push_vec(&mut populations, population, "catia_b5_object_populations")?;
            }
            continue;
        }
        let claimed = match object_frame(stream, run.range.start) {
            Some((end, family, class, object_id)) => {
                end == run.range.end
                    && is_referenced_geometry_class(family, class)
                    && ctx.contains_hash_set(&claimed_isolated_ids, &object_id, CLAIMED)?
            }
            None => false,
        };
        if !claimed {
            let population = ctx.copy_slice(
                &stream[run.range.clone()],
                "catia_b5_unclaimed_population_bytes",
            )?;
            ctx.push_vec(&mut populations, population, "catia_b5_object_populations")?;
        }
    }
    Ok(populations)
}

/// Unique object population selected across reconstructed logical streams.
pub(in crate::families) enum ObjectStreamSelection<'a> {
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
        census_records: Vec<B5Record<'a>>,
    },
    /// One topology-root population, or the unique unrooted run.
    Selected {
        /// Bytes of the selected run, with isolated referenced geometry appended.
        source: &'a [u8],
        /// Frames of the selected population, rebased to `source`.
        frames: Vec<ObjectFrame>,
        /// Records of the selected population, rebased to `source`.
        records: Vec<B5Record<'a>>,
        /// Records scanned across every run for census.
        census_records: Vec<B5Record<'a>>,
        /// Number of object runs in the reconstructed streams.
        run_count: usize,
    },
}

#[cfg(test)]
impl ObjectStreamSelection<'_> {
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

    pub(in crate::families) fn records(&self) -> &[B5Record<'_>] {
        match self {
            Self::Selected { records, .. } => records,
            Self::Exhausted { .. } | Self::Unselected { .. } => &[],
        }
    }

    fn census_records(&self) -> &[B5Record<'_>] {
        match self {
            Self::Selected { census_records, .. } | Self::Unselected { census_records, .. } => {
                census_records
            }
            Self::Exhausted { .. } => &[],
        }
    }
}

/// Select one topology-root population, or one unrooted run when it is the
/// only object run in the reconstructed logical streams. The selected source
/// borrows its run when no isolated geometry joins it; otherwise the run and
/// its isolated geometry are copied once into one arena buffer.
pub(in crate::families) fn select_object_stream_population<'a>(
    ctx: &DecodeContext<'a>,
    streams: &'a [Vec<u8>],
    budget: Option<&WorkBudget<'_>>,
) -> Result<ObjectStreamSelection<'a>, CodecError> {
    let mut stream_ranges = Vec::new();
    ctx.reserve_vec(
        &mut stream_ranges,
        streams.len(),
        "catia_b5_selected_stream_ranges",
    )?;
    for stream in ctx.admit_iter(streams, "catia_b5_stream_run_range_scan")? {
        stream_ranges.push(object_stream_run_ranges(ctx, stream)?);
    }
    let run_count = ctx.fold(
        &stream_ranges,
        0usize,
        |count, ranges| Ok(count + ranges.len()),
        "catia_b5_selected_run_count",
    )?;
    let exhausted = || ObjectStreamSelection::Exhausted { run_count };
    let mut stream_frames = Vec::new();
    ctx.reserve_vec(
        &mut stream_frames,
        streams.len(),
        "catia_b5_selected_stream_frames",
    )?;
    let mut runs = Vec::new();
    for (stream_index, (stream, ranges)) in ctx
        .admit_iter(streams, "catia_b5_selected_stream_scan")?
        .zip(ctx.admit_iter(stream_ranges, "catia_b5_selected_stream_range_scan")?)
        .enumerate()
    {
        let frames = collect_object_stream_frames(ctx, stream)?;
        if budget.is_some_and(|budget| !budget.charge_by(frames.len())) {
            return Ok(exhausted());
        }
        place_object_runs(ctx, ranges, &frames, stream_index, &mut runs)?;
        stream_frames.push(frames);
    }
    let mut topology_runs = Vec::new();
    for (index, run) in ctx
        .admit_iter(&runs, "catia_b5_topology_run_scan")?
        .enumerate()
    {
        if run.topology {
            ctx.push_vec(&mut topology_runs, index, "catia_b5_topology_run_indices")?;
        }
    }
    let selected_run = match topology_runs.as_slice() {
        [index] => Some((*index, true)),
        [] if runs.len() == 1 => Some((0, false)),
        _ => None,
    };
    let Some((selected_index, topology)) = selected_run else {
        let mut census_records = Vec::new();
        for run in ctx.admit_iter(&runs, "catia_b5_unselected_run_scan")? {
            let frames = &stream_frames[run.stream_index][run.frame_range.clone()];
            let mut records =
                records_from_frames_budgeted(ctx, &streams[run.stream_index], frames, budget)?;
            if budget.is_some_and(WorkBudget::exhausted) {
                return Ok(exhausted());
            }
            ctx.append_vec(
                &mut census_records,
                &mut records,
                "catia_b5_unselected_census_records",
            )?;
        }
        return Ok(ObjectStreamSelection::Unselected {
            run_count,
            census_records,
        });
    };
    let selected = &runs[selected_index];
    let selected_stream = &streams[selected.stream_index];
    let selected_frames = &stream_frames[selected.stream_index][selected.frame_range.clone()];
    let Some(mut records) =
        records_from_indexed_frames_budgeted(ctx, selected_stream, selected_frames, budget)?
    else {
        return Ok(exhausted());
    };
    let mut census_records = ctx.copy_slice(&records, "catia_b5_census_records")?;
    for (index, run) in ctx
        .admit_iter(&runs, "catia_b5_census_run_scan")?
        .enumerate()
    {
        if index == selected_index {
            continue;
        }
        let frames = &stream_frames[run.stream_index][run.frame_range.clone()];
        let mut run_records =
            records_from_frames_budgeted(ctx, &streams[run.stream_index], frames, budget)?;
        if budget.is_some_and(WorkBudget::exhausted) {
            return Ok(exhausted());
        }
        ctx.append_vec(
            &mut census_records,
            &mut run_records,
            "catia_b5_census_records",
        )?;
    }
    let isolated = if topology {
        isolated_geometry_runs(ctx, selected_stream, &runs, selected_index, &records)?
    } else {
        Vec::new()
    };
    let mut frames = Vec::new();
    ctx.reserve_vec(
        &mut frames,
        selected_frames.len(),
        "catia_b5_selected_frames",
    )?;
    for frame in ctx.admit_iter(selected_frames, "catia_b5_selected_frame_rebase_scan")? {
        let mut frame = *frame;
        frame.start -= selected.range.start;
        frame.end -= selected.range.start;
        frames.push(frame);
    }
    if isolated.is_empty() {
        for record in ctx.admit_iter(&mut records, "catia_b5_selected_record_rebase_scan")? {
            record.offset -= selected.range.start;
        }
        return Ok(ObjectStreamSelection::Selected {
            source: &selected_stream[selected.range.clone()],
            frames,
            records,
            census_records,
            run_count,
        });
    }
    let mut parts_storage = ctx.reserve_scoped(0, "catia_b5_selected_source_parts")?;
    let mut parts = Vec::new();
    parts_storage.with_storage(|| {
        ctx.reserve_vec(
            &mut parts,
            isolated.len() + 1,
            "catia_b5_selected_source_parts",
        )
    })?;
    parts.push(View::over_retained(
        &selected_stream[selected.range.clone()],
    ));
    let mut destination = selected.range.len();
    for &index in ctx.admit_iter(&isolated, "catia_b5_selected_isolated_order_scan")? {
        let run = &runs[index];
        let run_frames = &stream_frames[run.stream_index][run.frame_range.clone()];
        parts.push(View::over_retained(&selected_stream[run.range.clone()]));
        for frame in ctx.admit_iter(run_frames, "catia_b5_selected_isolated_frame_scan")? {
            let mut frame = *frame;
            frame.start = destination + frame.start - run.range.start;
            frame.end = destination + frame.end - run.range.start;
            ctx.push_vec(&mut frames, frame, "catia_b5_selected_isolated_frames")?;
        }
        destination += run.range.len();
    }
    let source = ctx.concat_views(&parts)?.window();
    drop(parts);
    drop(parts_storage);
    let records = records_from_frames(ctx, source, &frames)?;
    Ok(ObjectStreamSelection::Selected {
        source,
        frames,
        records,
        census_records,
        run_count,
    })
}

/// Return, in stream order, the single-frame runs of `stream` that hold
/// referenced geometry the selected population does not own. A geometry
/// identity framed with different bytes in two runs is ambiguous and joins
/// neither.
fn isolated_geometry_runs(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    runs: &[IndexedObjectRun],
    selected_index: usize,
    records: &[B5Record<'_>],
) -> Result<Vec<usize>, CodecError> {
    const OPERATION: &str = "catia_b5_isolated_geometry_candidates";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let ordered = scratch.with_storage(|| {
        let referenced = topology_surface_references(ctx, records)?;
        let mut owned_ids = HashSet::new();
        for record in ctx.admit_iter(records, "catia_b5_population_owned_id_scan")? {
            ctx.insert_hash_set(
                &mut owned_ids,
                record.object_id,
                "catia_b5_population_owned_ids",
            )?;
        }
        let mut isolated = HashMap::<u32, Option<usize>>::new();
        let mut ordered = BTreeSet::new();
        for (index, run) in ctx
            .admit_iter(runs, "catia_b5_isolated_candidate_scan")?
            .enumerate()
        {
            if index == selected_index || run.stream_index != runs[selected_index].stream_index {
                continue;
            }
            let Some((end, family, class, object_id)) = object_frame(stream, run.range.start)
            else {
                continue;
            };
            if end != run.range.end
                || !is_referenced_geometry_class(family, class)
                || !ctx.contains_btree_set(&referenced, &object_id, OPERATION)?
                || ctx.contains_hash_set(&owned_ids, &object_id, OPERATION)?
            {
                continue;
            }
            match ctx.get_mut_hash_map(&mut isolated, &object_id, OPERATION)? {
                // Equal frame bytes carry equal family and class headers.
                Some(stored) => {
                    if let Some(previous) = *stored {
                        if !ctx.equal_bytes(
                            &stream[runs[previous].range.clone()],
                            &stream[run.range.clone()],
                            OPERATION,
                        )? {
                            ctx.remove_btree_set(&mut ordered, &previous, OPERATION)?;
                            *stored = None;
                        }
                    }
                }
                None => {
                    ctx.insert_hash_map(&mut isolated, object_id, Some(index), OPERATION)?;
                    ctx.insert_btree_set(&mut ordered, index, OPERATION)?;
                }
            }
        }
        Ok::<_, CodecError>(ordered)
    })?;
    ctx.collect_vec(ordered, "catia_b5_isolated_geometry_order")
}

/// Build one topology population from its owning run and uniquely referenced
/// isolated geometry frames in the same logical stream. Returns the
/// population bytes and the stream run indices of the attached geometry.
fn owned_object_stream_population(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    frames: &[ObjectFrame],
    runs: &[IndexedObjectRun],
    topology_index: usize,
) -> Result<(Vec<u8>, Vec<usize>), CodecError> {
    let run = &runs[topology_index];
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_population_record_scratch")?;
    let isolated = scratch.with_storage(|| {
        let run_records = records_from_frames(ctx, stream, &frames[run.frame_range.clone()])?;
        isolated_geometry_runs(ctx, stream, runs, topology_index, &run_records)
    })?;
    let mut population =
        ctx.copy_slice(&stream[run.range.clone()], "catia_b5_topology_run_bytes")?;
    for &index in ctx.admit_iter(&isolated, "catia_b5_population_isolated_order_scan")? {
        ctx.extend_retained_bytes(
            &mut population,
            &stream[runs[index].range.clone()],
            "catia_b5_attached_isolated_bytes",
        )?;
    }
    Ok((population, isolated))
}

/// Yields a record's counted object references, stopping at the declared count
/// or the first token that is not a reference. Each step reads one bounded
/// token; callers charge the steps they take.
fn record_references<'a>(record: &B5Record<'a>) -> impl Iterator<Item = u32> + 'a {
    let payload = record.payload;
    let mut position = 0;
    let count = counted_cardinality(payload, &mut position).unwrap_or_default();
    (0..count).map_while(move |_| wire::tokens::object_ref(payload, &mut position, true))
}

fn topology_surface_references(
    ctx: &DecodeContext<'_>,
    records: &[B5Record],
) -> Result<BTreeSet<u32>, CodecError> {
    let mut surfaces = BTreeSet::new();
    for record in ctx.admit_iter(records, "catia_b5_topology_surface_record_scan")? {
        let reference = match record.class {
            0x5f => record_references(record).next(),
            0x62 => {
                let mut references = record_references(record);
                let mut last = None;
                while let Some(reference) =
                    ctx.next_charged(&mut references, "catia_b5_topology_loop_surface_reference")?
                {
                    last = Some(reference);
                }
                last
            }
            _ => None,
        };
        if let Some(reference) = reference {
            ctx.insert_btree_set(
                &mut surfaces,
                reference,
                "catia_b5_topology_surface_references",
            )?;
        }
    }
    Ok(surfaces)
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
    ctx: &DecodeContext<'_>,
    record: &B5FaceRecord,
    loops: &BTreeMap<u32, B5Loop>,
    surfaces: &BTreeMap<u32, B5Surface>,
    surface_aliases: &BTreeMap<u32, u32>,
) -> Result<Option<B5Face>, CodecError> {
    const OPERATION: &str = "catia_b5_face_reference_lookup";
    let references = &record.references;
    let Some((&surface, loop_references)) = references.split_first() else {
        return Ok(None);
    };
    if !ctx.contains_key_btree_map(surfaces, &surface, OPERATION)? {
        return Ok(None);
    }
    let Some(canonical_surface) = canonical_surface_id(ctx, surface_aliases, surface)? else {
        return Ok(None);
    };
    let mut loop_ids = Vec::new();
    for &reference in ctx.admit_iter(loop_references, "catia_b5_face_loop_reference_scan")? {
        if ctx.contains_key_btree_map(loops, &reference, OPERATION)? {
            ctx.push_vec(&mut loop_ids, reference, "catia_b5_face_loop_ids")?;
        } else {
            let repeats_carrier = ctx.contains_key_btree_map(surfaces, &reference, OPERATION)?
                && canonical_surface_id(ctx, surface_aliases, reference)?
                    == Some(canonical_surface);
            if !repeats_carrier {
                // A distinct surface reference is a multi-surface variant. Its
                // composition is not represented by the neutral Face type, so
                // keep the typed record but withhold the face from topology.
                return Ok(None);
            }
            // A face may repeat its carrier through an alias identity. This is
            // the same carrier incidence, not a multi-surface face.
        }
    }
    if loop_ids.is_empty() {
        return Ok(None);
    }
    Ok(Some(B5Face {
        object_id: record.object_id,
        surface,
        loops: loop_ids,
        terminal_control: record.terminal_control,
    }))
}

fn parse_face_record(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
) -> Result<Option<B5FaceRecord>, CodecError> {
    if record.class != 0x5f {
        return Ok(None);
    }
    if let Some(count) = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x80))
    {
        if count == 0 {
            return Ok(None);
        }
        let mut position = 1;
        let Some(references) = ctx.collect_options(
            (0..count).map(|_| wire::tokens::object_ref(&record.payload, &mut position, true)),
            "catia_b5_counted_face_references",
        )?
        else {
            return Ok(None);
        };
        let Some(&[terminal_control]) = record.payload.get(position..) else {
            return Ok(None);
        };
        let Some(terminal_control) = B5FramingControl::from_byte(terminal_control) else {
            return Ok(None);
        };
        Ok(Some(B5FaceRecord {
            object_id: record.object_id,
            references,
            terminal_control: Some(terminal_control),
        }))
    } else {
        let Some(references) = uncounted_references(ctx, &record.payload)? else {
            return Ok(None);
        };
        Ok((!references.is_empty()).then_some(B5FaceRecord {
            object_id: record.object_id,
            references,
            terminal_control: None,
        }))
    }
}

/// Read every structurally complete face record independently of target
/// resolution.
#[cfg(test)]
fn typed_face_records(bytes: &[u8]) -> BTreeMap<u32, B5FaceRecord> {
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, bytes))
            .expect("service frame scan budget");
    let records =
        crate::test_support::with_service_context(|ctx| records_from_frames(ctx, bytes, &frames))
            .expect("service budget");
    crate::test_support::with_service_context(|ctx| {
        typed_face_records_from_records(ctx, &records).expect("service decode")
    })
}

pub(in crate::families) fn typed_face_records_from_records(
    ctx: &DecodeContext<'_>,
    records: &[B5Record],
) -> Result<BTreeMap<u32, B5FaceRecord>, CodecError> {
    let mut faces = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_typed_face_record_scan")? {
        if let Some(face) = parse_face_record(ctx, record)? {
            ctx.insert_btree_map(
                &mut faces,
                record.object_id,
                face,
                "catia_b5_typed_face_records",
            )?;
        }
    }
    Ok(faces)
}

/// Read every structurally complete loop record independently of target
/// resolution.
#[cfg(test)]
fn typed_loop_records(bytes: &[u8]) -> BTreeMap<u32, B5Loop> {
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, bytes))
            .expect("service frame scan budget");
    let records =
        crate::test_support::with_service_context(|ctx| records_from_frames(ctx, bytes, &frames))
            .expect("service budget");
    crate::test_support::with_service_context(|ctx| {
        typed_loop_records_from_records(ctx, &records).expect("service decode")
    })
}

pub(in crate::families) fn typed_loop_records_from_records(
    ctx: &DecodeContext<'_>,
    records: &[B5Record],
) -> Result<BTreeMap<u32, B5Loop>, CodecError> {
    let mut loops = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_typed_loop_record_scan")? {
        if let Some(loop_) = parse_loop_record(ctx, record)? {
            ctx.insert_btree_map(
                &mut loops,
                record.object_id,
                loop_,
                "catia_b5_typed_loop_records",
            )?;
        }
    }
    Ok(loops)
}

/// Read every structurally complete physical-edge record independently of
/// topology resolution.
#[cfg(test)]
fn typed_edge_records(bytes: &[u8]) -> BTreeMap<u32, B5Edge> {
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, bytes))
            .expect("service frame scan budget");
    let records =
        crate::test_support::with_service_context(|ctx| records_from_frames(ctx, bytes, &frames))
            .expect("service budget");
    crate::test_support::with_service_context(|ctx| {
        typed_edge_records_from_records(ctx, &records).expect("service decode")
    })
}

pub(in crate::families) fn typed_edge_records_from_records(
    ctx: &DecodeContext<'_>,
    records: &[B5Record],
) -> Result<BTreeMap<u32, B5Edge>, CodecError> {
    let mut edges = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_typed_edge_record_scan")? {
        if let Some(edge) = parse_edge(record) {
            ctx.insert_btree_map(
                &mut edges,
                record.object_id,
                edge,
                "catia_b5_typed_edge_records",
            )?;
        }
    }
    Ok(edges)
}

/// Read every structurally complete vertex-incidence link independently of
/// topology resolution.
#[cfg(test)]
fn typed_vertex_incidence_links(bytes: &[u8]) -> BTreeMap<u32, B5VertexIncidenceLink> {
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, bytes))
            .expect("service frame scan budget");
    let records =
        crate::test_support::with_service_context(|ctx| records_from_frames(ctx, bytes, &frames))
            .expect("service budget");
    crate::test_support::with_service_context(|ctx| {
        typed_vertex_incidence_links_from_records(ctx, &records).expect("service decode")
    })
}

pub(in crate::families) fn typed_vertex_incidence_links_from_records(
    ctx: &DecodeContext<'_>,
    records: &[B5Record],
) -> Result<BTreeMap<u32, B5VertexIncidenceLink>, CodecError> {
    let mut links = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_typed_vertex_link_record_scan")? {
        if let Some(link) = parse_vertex_incidence_link(record) {
            ctx.insert_btree_map(
                &mut links,
                record.object_id,
                link,
                "catia_b5_typed_vertex_incidence_links",
            )?;
        }
    }
    Ok(links)
}

/// Read every structurally complete class-`21` pcurve independently of
/// support and topology resolution.
#[cfg(test)]
fn typed_class_21_pcurves(bytes: &[u8]) -> BTreeMap<u32, B5Pcurve> {
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, bytes))
            .expect("service frame scan budget");
    let records =
        crate::test_support::with_service_context(|ctx| records_from_frames(ctx, bytes, &frames))
            .expect("service budget");
    crate::test_support::with_service_context(|ctx| {
        typed_class_21_pcurves_from_records(ctx, &records)
    })
    .expect("service budget")
}

pub(in crate::families) fn typed_class_21_pcurves_from_records(
    ctx: &DecodeContext<'_>,
    records: &[B5Record],
) -> Result<BTreeMap<u32, B5Pcurve>, CodecError> {
    let mut pcurves = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_typed_class21_record_scan")? {
        if let Some(pcurve) = parse_pcurve(ctx, record)? {
            ctx.insert_btree_map(
                &mut pcurves,
                record.object_id,
                pcurve,
                "catia_b5_typed_class21_pcurves",
            )?;
        }
    }
    Ok(pcurves)
}

/// Read every structurally complete parameter incidence independently of
/// curve, edge, and topology resolution.
#[cfg(test)]
fn typed_parameter_incidences(bytes: &[u8]) -> BTreeMap<u32, B5ParameterIncidence> {
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, bytes))
            .expect("service frame scan budget");
    let records =
        crate::test_support::with_service_context(|ctx| records_from_frames(ctx, bytes, &frames))
            .expect("service budget");
    crate::test_support::with_service_context(|ctx| {
        typed_parameter_incidences_from_records(ctx, &records).expect("service decode")
    })
}

pub(in crate::families) fn typed_parameter_incidences_from_records(
    ctx: &DecodeContext<'_>,
    records: &[B5Record],
) -> Result<BTreeMap<u32, B5ParameterIncidence>, CodecError> {
    let mut incidences = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_typed_parameter_incidence_scan")? {
        if let Some(incidence) = parameter_incidence(ctx, record)? {
            ctx.insert_btree_map(
                &mut incidences,
                record.object_id,
                incidence,
                "catia_b5_typed_parameter_incidences",
            )?;
        }
    }
    Ok(incidences)
}

/// Read every structurally complete vertex-incidence roster independently of
/// member and topology resolution.
#[cfg(test)]
fn typed_vertex_incidence_rosters(bytes: &[u8]) -> BTreeMap<u32, Vec<u32>> {
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, bytes))
            .expect("service frame scan budget");
    let records =
        crate::test_support::with_service_context(|ctx| records_from_frames(ctx, bytes, &frames))
            .expect("service budget");
    crate::test_support::with_service_context(|ctx| {
        typed_vertex_incidence_rosters_from_records(ctx, &records).expect("service decode")
    })
}

pub(in crate::families) fn typed_vertex_incidence_rosters_from_records(
    ctx: &DecodeContext<'_>,
    records: &[B5Record],
) -> Result<BTreeMap<u32, Vec<u32>>, CodecError> {
    let mut rosters = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_b5_typed_vertex_roster_scan")? {
        if let Some(members) = counted_references(ctx, record, 0x05)? {
            ctx.insert_btree_map(
                &mut rosters,
                record.object_id,
                members,
                "catia_b5_typed_vertex_incidence_rosters",
            )?;
        }
    }
    Ok(rosters)
}

/// Read each face's leading surface reference independently of its loop grammar.
#[cfg(test)]
fn face_surface_references(bytes: &[u8]) -> Vec<(u32, u32)> {
    let frames =
        crate::test_support::with_service_context(|ctx| collect_object_stream_frames(ctx, bytes))
            .expect("service frame scan budget");
    crate::test_support::with_service_context(|ctx| {
        face_surface_references_from_frames(ctx, bytes, &frames)
    })
    .expect("service budget")
}

pub(in crate::families) fn face_surface_references_from_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frames: &[ObjectFrame],
) -> Result<Vec<(u32, u32)>, CodecError> {
    let mut references = Vec::new();
    for frame in ctx.admit_iter(frames, "catia_b5_face_surface_frame_scan")? {
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
        ctx.push_vec(
            &mut references,
            (frame.object_id, surface),
            "catia_b5_face_surface_references",
        )?;
    }
    Ok(references)
}

/// Return positive face-to-edge ownership from structurally complete face and
/// loop records, without requiring their referenced surfaces or p-curves to
/// resolve into a transferable graph.
pub(in crate::families) fn edge_face_references_from_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frames: &[ObjectFrame],
) -> Result<BTreeMap<u32, Vec<u32>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_b5_edge_face_scratch")?;
    let records = scratch.with_storage(|| records_from_frames(ctx, bytes, frames))?;
    let mut edge_ids = HashSet::new();
    for record in ctx
        .admit_iter(&records, "catia_b5_edge_face_record_scan")?
        .filter(|record| record.family == 0xb5 && record.class == 0x5e)
    {
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut edge_ids,
                record.object_id,
                "catia_b5_edge_face_edge_ids",
            )
        })?;
    }
    let loops = scratch.with_storage(|| typed_loop_records_from_records(ctx, &records))?;
    // Faces are visited in ascending id order, so each owner row is built
    // ascending and a repeated owner can only be its last entry.
    let mut owners = BTreeMap::<u32, Vec<u32>>::new();
    let typed_faces = scratch.with_storage(|| typed_face_records_from_records(ctx, &records))?;
    for (&face, record) in ctx.admit_iter(&typed_faces, "catia_b5_edge_face_typed_face_scan")? {
        for loop_id in ctx
            .admit_iter(&record.references, "catia_b5_edge_face_reference_scan")?
            .skip(1)
        {
            let Some(loop_record) =
                ctx.get_btree_map(&loops, loop_id, "catia_b5_edge_face_loop_lookup")?
            else {
                continue;
            };
            for member in
                ctx.admit_iter(&loop_record.members, "catia_b5_edge_face_loop_member_scan")?
            {
                const OPERATION: &str = "catia_b5_edge_face_owners";
                if !ctx.contains_hash_set(&edge_ids, &member.edge, OPERATION)?
                    || ctx
                        .get_btree_map(&owners, &member.edge, OPERATION)?
                        .is_some_and(|row| row.last() == Some(&face))
                {
                    continue;
                }
                ctx.push_btree_group(
                    &mut owners,
                    member.edge,
                    face,
                    OPERATION,
                    "catia_b5_edge_face_owner_entries",
                )?;
            }
        }
    }
    Ok(owners)
}

fn parse_loop(
    ctx: &DecodeContext<'_>,
    record: B5Loop,
    by_id: &HashMap<u32, &B5Record<'_>>,
    parsed_pcurves: &BTreeMap<u32, B5Pcurve>,
    opaque_pcurves: &BTreeMap<u32, B5OpaquePcurve>,
    implicit_pcurves: &BTreeMap<u32, u32>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Result<Option<B5Loop>, CodecError> {
    const OPERATION: &str = "catia_b5_parse_loop_member_lookup";
    let surface = record.surface;
    if !ctx.contains_key_btree_map(surfaces, &surface, OPERATION)? {
        return Ok(None);
    }
    let members_resolve = ctx.all_by(
        &record.members,
        |member| {
            let pcurve_on_surface = ctx
                .get_btree_map(parsed_pcurves, &member.pcurve, OPERATION)?
                .is_some_and(|pcurve| pcurve.surface == surface)
                || ctx
                    .get_btree_map(opaque_pcurves, &member.pcurve, OPERATION)?
                    .is_some_and(|pcurve| pcurve.surface == surface)
                || ctx.get_btree_map(implicit_pcurves, &member.pcurve, OPERATION)?
                    == Some(&surface);
            Ok(pcurve_on_surface
                && record_by_id(ctx, by_id, member.edge, OPERATION)?
                    .is_some_and(|edge| edge.class == 0x5e))
        },
        "catia_b5_parse_loop_member_scan",
    )?;
    Ok(members_resolve.then_some(record))
}

fn parse_loop_record(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
) -> Result<Option<B5Loop>, CodecError> {
    let Some((references, metadata, edge_controls)) = loop_references_and_metadata(ctx, record)?
    else {
        return Ok(None);
    };
    let Some(&surface) = references.last() else {
        return Ok(None);
    };
    let pairs = &references[..references.len() - 1];
    if pairs.len() / 2 != edge_controls.len() {
        return Ok(None);
    }
    let pair_width = NonZeroUsize::new(2)
        .ok_or_else(|| ctx.refuse_codec_limit("catia_b5_loop_member_pair_width", 0, 1))?;
    let members = ctx.collect_vec(
        ctx.admit_iter(pairs, "catia_b5_loop_member_pair_scan")?
            .chunks(pair_width)
            .zip(ctx.admit_iter(&edge_controls, "catia_b5_loop_edge_control_scan")?)
            .map(|(pair, controls)| B5LoopMember {
                pcurve: pair[0],
                edge: pair[1],
                controls: *controls,
            }),
        "catia_b5_loop_members",
    )?;
    Ok(Some(B5Loop {
        object_id: record.object_id,
        members,
        metadata,
        surface,
    }))
}

#[cfg(test)]
fn loop_references(record: &B5Record) -> Option<Vec<u32>> {
    crate::test_support::with_service_context(|ctx| {
        loop_references_and_metadata(ctx, record)
            .expect("service decode")
            .map(|(references, _, _)| references)
    })
}

fn loop_references_and_metadata(
    ctx: &DecodeContext<'_>,
    record: &B5Record,
) -> LoopReferencesOutput {
    if record.class != 0x62 {
        return Ok(None);
    }
    let mut position = 0;
    let Some(count) = counted_cardinality(&record.payload, &mut position) else {
        return Ok(None);
    };
    if count < 3 || count % 2 == 0 {
        return Ok(None);
    }
    let Some(references) = ctx.collect_options(
        (0..count).map(|_| wire::tokens::object_ref(&record.payload, &mut position, true)),
        "catia_b5_loop_references",
    )?
    else {
        return Ok(None);
    };
    let edge_count = (count - 1) / 2;
    if counted_cardinality(&record.payload, &mut position) != Some(edge_count) {
        return Ok(None);
    }
    let Some(bytes) = record.payload.get(position..) else {
        return Ok(None);
    };
    let Some((metadata, edge_controls)) = loop_metadata(ctx, bytes, edge_count)? else {
        return Ok(None);
    };
    Ok(Some((references, metadata, edge_controls)))
}

fn loop_metadata(ctx: &DecodeContext<'_>, bytes: &[u8], edge_count: usize) -> LoopMetadataOutput {
    let Some((controls_end, framing_controls)) = (|| {
        let controls_len = edge_count.checked_mul(3)?.checked_mul(2)?;
        let controls_end = 3usize.checked_add(controls_len)?;
        let framing_controls = [
            B5FramingControl::from_byte(*bytes.first()?)?,
            B5FramingControl::from_byte(*bytes.get(1)?)?,
        ];
        Some((controls_end, framing_controls))
    })() else {
        return Ok(None);
    };
    if bytes.get(2) != Some(&0x03) || controls_end > bytes.len() {
        return Ok(None);
    }
    let Some(edge_controls) = ctx.collect_options(
        bytes[3..controls_end].chunks_exact(6).map(|controls| {
            let mut view = View::over_retained(controls);
            let controls = [view.i16_le()?, view.i16_le()?, view.i16_le()?];
            controls
                .iter()
                .all(|control| matches!(control, -1 | 1))
                .then_some(controls)
        }),
        "catia_b5_loop_edge_controls",
    )?
    else {
        return Ok(None);
    };
    let extension = (|| -> Option<_> {
        Some(match bytes.get(controls_end..)? {
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
                    FiniteReal::new(view.f64_le()?)?,
                    FiniteReal::new(view.f64_le()?)?,
                    FiniteReal::new(view.f64_le()?)?,
                    FiniteReal::new(view.f64_le()?)?,
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
                if floats.iter().any(|value| !value.is_finite()) {
                    return None;
                }
                Some(B5LoopMetadataExtension {
                    scalars,
                    control: extended[35],
                    floats,
                })
            }
            _ => return None,
        })
    })();
    let Some(extension) = extension else {
        return Ok(None);
    };
    Ok(Some((
        B5LoopMetadata {
            framing_controls,
            extension,
        },
        edge_controls,
    )))
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

fn uncounted_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut position = 0;
    let mut references = Vec::new();
    while position < bytes.len() {
        let Some(reference) = wire::tokens::object_ref(bytes, &mut position, true) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut references,
            reference,
            "catia_b5_uncounted_face_references",
        )?;
    }
    Ok(Some(references))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod nested_frame_work_tests {
    use cadmpeg_core::CodecError;

    #[test]
    fn nested_a8_frame_scanning_refuses_caller_work_before_collection() {
        let bytes =
            crate::test_support::test_b5::a8_elided_surface_stream_with_native_vertex_chain();
        let result = crate::test_support::with_work_limit(0, |ctx| {
            crate::families::b5::graph::collect_object_stream_frames(ctx, &bytes)
        });
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_b5_object_frame_scan"));
        assert!(!crate::test_support::with_service_context(|ctx| {
            crate::families::b5::graph::collect_object_stream_frames(ctx, &bytes)
        })
        .expect("service frame scan")
        .is_empty());
    }
}
