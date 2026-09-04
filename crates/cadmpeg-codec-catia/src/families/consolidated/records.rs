//! Consolidated record framing and edge-resolution vocabulary.
//!
//! Inventories length-closed A/B-family records, groups consolidated edge runs
//! and their native incidence graph, and resolves edge-block side carriers
//! against typed analytic and NURBS charts.

use cadmpeg_core::decode::View;
use cadmpeg_ir::eval::nurbs_surface_partials;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use crate::families::a5a8::records::{
    a5_pcurves_from_records, a5_surfaces_from_records, FreeformSurface,
};
use crate::families::b2::records::{
    b2_adjacent_face_counted_owners_from_records, b2_circles_from_records,
    b2_class25_descriptors_from_records, b2_closed_owner_boundary_edges, b2_cone_point,
    b2_cones_from_records, b2_cylinder_point, b2_cylinders_from_records,
    b2_edge_nodes_from_records, b2_edge_parameters_from_records,
    b2_embedded_cylinders_from_records, b2_face_nodes_5f_from_records,
    b2_owner_identity_targets_from_records, b2_owner_packets_from_records, b2_pcurves_from_records,
    b2_plane_carriers_from_records, b2_plane_geometry, b2_sphere_geometry, b2_spheres_from_records,
    b2_tori_from_records, b2_torus_geometry, b2_use_metadata_from_records, point_distance,
    B2Circle, B2Class25Descriptor, B2Cone, B2Cylinder, B2EdgeNode, B2EdgeParameters,
    B2EmbeddedCylinder, B2FaceNode5f, B2PlaneCarrier, B2Sphere, B2Torus, B2UseMetadata,
};
use crate::wire::bytes::{
    allocation_ref, compact_int, finite_f64_lane, persistent_ref, read_f64_array,
    AllocationReferenceEncoding,
};
use crate::wire::records::{
    consolidated_records, records_are_contiguous, scan_vertex_record_ranges, ConsolidatedFamily,
    ConsolidatedPcurve, ConsolidatedRawFrame, ConsolidatedRecord,
};

const EPS_TRANSVERSE_RESIDUAL: f64 = 1.0e-6;
const EPS_SAMPLE_AGREEMENT: f64 = 1.0e-6;
const EPS_ENDPOINT_RANGE: f64 = 1.0e-6;
const EPS_CIRCLE_ENDPOINT: f64 = 1.0e-9;

/// Serialized consolidated edge block formed by two pcurves and one range packet.
#[derive(Debug, Clone)]
pub struct ConsolidatedEdgeBlock {
    /// The two face-side UV definitions in serialization order.
    pub pcurves: [ConsolidatedPcurve; 2],
    /// Shared parameter range and tolerance packet.
    pub parameters: B2EdgeParameters,
}

/// Complete consolidated edge run serialized as two side pcurves, their shared
/// parameter packet, two oriented uses, and one native edge node.
#[derive(Debug, Clone)]
pub struct ConsolidatedTopologyEdgeRun {
    /// Co-parametric side definitions and shared range packet.
    pub edge: ConsolidatedEdgeBlock,
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub node: B2EdgeNode,
}

/// Complete analytic-circle edge run serialized as a class-`0x18` descriptor,
/// circle carrier, scalar definition, two oriented uses, and one edge node.
#[derive(Debug, Clone)]
pub struct ConsolidatedAnalyticCircleEdgeRun {
    /// Class-`0x18` descriptor immediately preceding the circle carrier.
    pub descriptor: ConsolidatedAnalyticCircleDescriptor,
    /// Arc-length circle carrier.
    pub circle: B2Circle,
    /// Eight-scalar class-`0x23` edge definition.
    #[cfg(test)]
    pub definition: ConsolidatedEdgeDefinition,
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub node: B2EdgeNode,
}

/// Exact class-`0x18` frame attached to an analytic circle carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidatedAnalyticCircleDescriptor {
    /// Framed record.
    pub frame: ConsolidatedRawFrame,
}

/// Complete class-`0x25` edge run with its adjacent class-`0x18` descriptor.
#[derive(Debug, Clone)]
pub struct ConsolidatedClass25EdgeRun {
    /// Typed class-`0x18` descriptor.
    pub descriptor: B2Class25Descriptor,
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub node: B2EdgeNode,
}

/// Two adjacent oriented uses and their terminal native edge node.
#[derive(Debug, Clone)]
pub struct ConsolidatedEdgeUseRun {
    /// Immediately preceding edge-definition frame in classes `0x23..=0x25`.
    pub definition: Option<ConsolidatedEdgeDefinition>,
    /// The two serialized edge uses, in side order.
    pub uses: [B2UseMetadata; 2],
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub node: B2EdgeNode,
}

/// Compact edge node selected by its zero-based ordinal in one face-owner allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsolidatedOwnedEdgeNode {
    /// Byte offset of the owning class-`0x62` packet.
    pub owner_pos: usize,
    /// Zero-based frame ordinal after the owner packet.
    pub allocation_ordinal: u32,
    /// Selected compact edge node.
    pub node: B2EdgeNode,
}

/// Compact edge endpoints resolved through structural allocation references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsolidatedCompactEdgeEndpoints {
    /// Edge node whose endpoint references closed under the local allocation grammar.
    pub node: B2EdgeNode,
    /// Byte offsets of the resolved endpoint records, in edge order.
    pub endpoint_records: [usize; 2],
}

/// One fixed-nine owner boundary closed by four resolved class-`0x5e` edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConsolidatedOwnerBoundaryCycle {
    /// Bounded record source containing the owner and its local targets.
    pub source_index: usize,
    /// Class-`0x62` owner-record offset.
    pub owner_pos: usize,
    /// Source-scoped class-`0x5f` face node associated with this boundary
    /// allocation, when the cycle prelude closes its checked identity.
    pub face_node: Option<B2FaceNode5f>,
    /// Four edge targets in fixed-nine slot order.
    pub edges: [crate::families::b2::records::B2OwnerBoundaryEdge; 4],
}

/// Framed edge definition structurally owned by an adjacent oriented-use run.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidatedEdgeDefinition {
    /// Framed record.
    pub frame: ConsolidatedRawFrame,
    /// Edge-definition class in `0x23..=0x25`.
    pub class: u8,
}

impl ConsolidatedEdgeDefinition {
    pub fn data(&self) -> Option<ConsolidatedEdgeDefinitionData> {
        consolidated_edge_definition_data(self.class, &self.frame.payload)
    }
}

/// Closed payload grammar of a consolidated edge-definition frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ConsolidatedEdgeDefinitionData {
    /// Compact class-`0x24` payload `81 <operand> 0f 87`.
    Compact24 {
        /// Width-coded operand.
        operand: u32,
    },
    /// Three operand references followed by eight or nine scalar lanes.
    Scalar {
        /// Two compact operands followed by one persistent operand.
        operands: [u32; 3],
        /// Complete finite scalar lane.
        values: Vec<f64>,
    },
    /// Class-`0x25` three-operand form with one uninterrupted scalar lane.
    Scalar25 {
        /// Two mixed-width allocation operands followed by one persistent operand.
        operands: [u32; 3],
        /// Explicit third-operand lead (`0x0a` or `0x0b`), or `None` for compact encoding.
        persistent_lead: Option<u8>,
        /// Complete finite scalar lane.
        values: Vec<f64>,
    },
    /// Class-`0x25` three-operand form with a tagged scalar-lane boundary.
    SegmentedScalar25 {
        /// Two mixed-width allocation operands followed by one persistent operand.
        operands: [u32; 3],
        /// Explicit third-operand lead (`0x0a` or `0x0b`), or `None` for compact encoding.
        persistent_lead: Option<u8>,
        /// Five finite scalars preceding the segment marker.
        leading: [f64; 5],
        /// Scalar-lane boundary marker (`0x82`, `0x83`, `0x89`, or `0x8b`).
        marker: u8,
        /// Complete finite scalar lane following the marker.
        trailing: Vec<f64>,
    },
}

/// Decode a complete class-specific edge-definition payload without inferring
/// geometric meanings for its operand or scalar lanes.
#[must_use]
pub fn consolidated_edge_definition_data(
    class: u8,
    payload: &[u8],
) -> Option<ConsolidatedEdgeDefinitionData> {
    if class == 0x24 && payload.first() == Some(&0x81) {
        let mut at = 1;
        let operand = compact_int(payload, &mut at)?;
        return (payload.get(at..) == Some(&[0x0f, 0x87][..]))
            .then_some(ConsolidatedEdgeDefinitionData::Compact24 { operand });
    }
    if class == 0x25 && payload.first() == Some(&0x82) {
        let mut at = 1;
        let first = allocation_ref(payload, &mut at)?;
        let second = allocation_ref(payload, &mut at)?;
        let (third, persistent_lead) = class25_persistent_ref(payload, &mut at)?;
        let operands = [first, second, third];
        let scalar_bytes = payload.get(at..)?;
        if matches!(scalar_bytes.len(), 56 | 64 | 72 | 80) {
            let values = finite_f64_lane(scalar_bytes)?;
            return Some(ConsolidatedEdgeDefinitionData::Scalar25 {
                operands,
                persistent_lead,
                values,
            });
        }
        let leading = read_f64_array::<5>(scalar_bytes, 0)?;
        let marker = *scalar_bytes.get(40)?;
        let trailing = finite_f64_lane(scalar_bytes.get(41..)?)?;
        if leading.iter().all(|value| value.is_finite())
            && matches!(
                (marker, trailing.len()),
                (0x82, 5..=7) | (0x83, 8..=9) | (0x89, 20) | (0x8b, 24)
            )
        {
            return Some(ConsolidatedEdgeDefinitionData::SegmentedScalar25 {
                operands,
                persistent_lead,
                leading,
                marker,
                trailing,
            });
        }
        return None;
    }
    if !matches!(class, 0x23 | 0x24) || payload.first() != Some(&0x82) {
        return None;
    }
    let mut at = 1;
    let operands = [
        compact_int(payload, &mut at)?,
        compact_int(payload, &mut at)?,
        persistent_ref(payload, &mut at)?,
    ];
    let scalar_bytes = payload.get(at..)?;
    if !matches!((class, scalar_bytes.len()), (0x23, 64 | 72) | (0x24, 64)) {
        return None;
    }
    let values = finite_f64_lane(scalar_bytes)?;
    if values[2] != *values.last()? {
        return None;
    }
    if values.len() == 9
        && !(values[0] == values[3]
            && values[0] == values[6]
            && values[1] == values[4]
            && values[1] == values[7]
            && values[2] == values[5]
            && values[5] == 1.0)
    {
        return None;
    }
    Some(ConsolidatedEdgeDefinitionData::Scalar { operands, values })
}

fn class25_persistent_ref(bytes: &[u8], at: &mut usize) -> Option<(u32, Option<u8>)> {
    match *bytes.get(*at)? {
        lead @ (0x0a | 0x0b) => {
            let value = u32::from(View::u16_le_at(bytes, *at + 1)?);
            *at += 3;
            Some((value, Some(lead)))
        }
        _ => Some((compact_int(bytes, at)?, None)),
    }
}

/// Native endpoint-incidence graph of complete consolidated edge runs.
#[derive(Debug, Clone)]
#[cfg(test)]
pub struct ConsolidatedNativeEdgeGraph {
    /// Persistent native vertex identities in first-incidence order.
    pub vertex_identities: Vec<u32>,
    /// Edge runs in serialization order, with endpoints indexing
    /// `vertex_identities`.
    pub edges: Vec<ConsolidatedNativeGraphEdge>,
    /// Connected edge components, expressed as edge ordinals.
    pub components: Vec<Vec<usize>>,
}

/// One edge in a consolidated native endpoint-incidence graph.
#[derive(Debug, Clone)]
#[cfg(test)]
pub struct ConsolidatedNativeGraphEdge {
    /// Compact endpoint indices into [`ConsolidatedNativeEdgeGraph::vertex_identities`].
    pub vertices: [usize; 2],
}

/// Uniquely resolved carrier for one side of a consolidated edge block.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsolidatedSupportBinding {
    /// Standalone `b2 03 28` cylinder record.
    Cylinder {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// Cylinder frame embedded in a `b2 03 60` wrapper.
    EmbeddedCylinder {
        /// Embedded frame byte offset.
        pos: usize,
        /// Enclosing wrapper byte offset.
        wrapper_pos: usize,
    },
    /// `b2 03 19` circle selected by constant-V and exact arc range.
    Circle {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// `b2 03 29` cone selected by endpoint lifts.
    Cone {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// `b2 03 2a` sphere selected by endpoint lifts.
    Sphere {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// `b2 03 2b` torus selected by endpoint lifts through its scaled chart.
    Torus {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// Direction-bearing `b2/b3/b4 03 27` plane carrier selected by endpoint lifts.
    Plane {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// Consolidated `a5 03 34` NURBS carrier, optionally at a constant normal offset.
    NurbsCarrier {
        /// Carrier record byte offset.
        pos: usize,
        /// Signed normal offset from the stored carrier to the shared 3D edge.
        offset: f64,
    },
}

/// Consolidated edge block with uniquely resolved side carriers.
#[derive(Debug, Clone)]
pub struct ResolvedConsolidatedEdgeBlock {
    /// Parsed pcurve pair and shared edge packet.
    pub block: ConsolidatedEdgeBlock,
    /// Carrier binding for each pcurve side.
    pub supports: [Option<ConsolidatedSupportBinding>; 2],
    /// Shared lifted 3D definition sites when every liftable side agrees
    /// pointwise in the common edge parameterization.
    pub shared_loci: Option<Vec<Point3>>,
    /// Unordered 3D endpoint loci when at least one uniquely bound side can be
    /// lifted and every liftable side agrees.
    pub endpoint_loci: Option<[Point3; 2]>,
}

struct ConsolidatedCarriers<'a> {
    cylinders: &'a [B2Cylinder],
    embedded_cylinders: &'a [B2EmbeddedCylinder],
    cones: &'a [B2Cone],
    spheres: &'a [B2Sphere],
    tori: &'a [B2Torus],
    planes: &'a [B2PlaneCarrier],
    nurbs_surfaces: &'a [FreeformSurface],
}

/// Group ordered pairs of same-family class-`0x20` pcurves followed by one
/// B-family class-`0x23` range packet.
#[must_use]
#[cfg(test)]
pub fn consolidated_edge_blocks(data: &[u8]) -> Vec<ConsolidatedEdgeBlock> {
    let records = consolidated_records(data);
    consolidated_edge_blocks_from_records(data, &records)
}

pub(crate) fn consolidated_edge_blocks_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedEdgeBlock> {
    let pcurves = a5_pcurves_from_records(data, records)
        .into_iter()
        .chain(b2_pcurves_from_records(data, records))
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let parameters = b2_edge_parameters_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    records
        .windows(3)
        .filter_map(|window| {
            let [first_record, second_record, parameter_record] = window else {
                return None;
            };
            if !records_are_contiguous(window) {
                return None;
            }
            if first_record.class == 0x20
                && second_record.class == 0x20
                && first_record.family == second_record.family
                && parameter_record.family == ConsolidatedFamily::B
                && parameter_record.class == 0x23
            {
                let first = pcurves.get(&first_record.range.start)?;
                let second = pcurves.get(&second_record.range.start)?;
                let parameters = parameters.get(&parameter_record.range.start)?;
                let co_parametric = first.sites.len() == second.sites.len()
                    && first.range == second.range
                    && first.range == parameters.range;
                co_parametric.then(|| ConsolidatedEdgeBlock {
                    pcurves: [first.clone(), second.clone()],
                    parameters: parameters.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Decode complete six-record consolidated edge runs. Records separated by any
/// other framed record do not form a run.
#[must_use]
#[cfg(test)]
pub fn consolidated_topology_edge_runs(data: &[u8]) -> Vec<ConsolidatedTopologyEdgeRun> {
    let records = consolidated_records(data);
    consolidated_topology_edge_runs_from_records(data, &records)
}

pub(crate) fn consolidated_topology_edge_runs_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedTopologyEdgeRun> {
    let edges = consolidated_edge_blocks_from_records(data, records)
        .into_iter()
        .map(|edge| (edge.pcurves[0].pos, edge))
        .collect::<BTreeMap<_, _>>();
    let use_runs = consolidated_edge_use_runs_from_records(data, records)
        .into_iter()
        .map(|value| (value.uses[0].pos, value))
        .collect::<BTreeMap<_, _>>();
    records
        .windows(6)
        .filter_map(|window| {
            let [pcurve0, pcurve1, parameters, use0, use1, node] = window else {
                return None;
            };
            if !records_are_contiguous(window) {
                return None;
            }
            if pcurve0.class == 0x20
                && pcurve1.class == 0x20
                && pcurve0.family == pcurve1.family
                && parameters.family == ConsolidatedFamily::B
                && parameters.class == 0x23
                && use0.family == ConsolidatedFamily::B
                && use0.class == 0x06
                && use1.family == ConsolidatedFamily::B
                && use1.class == 0x06
                && node.family == ConsolidatedFamily::B
                && node.class == 0x5e
            {
                let use_run = use_runs.get(&use0.range.start)?;
                Some(ConsolidatedTopologyEdgeRun {
                    edge: edges.get(&pcurve0.range.start)?.clone(),
                    node: use_run.node,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Decode adjacent `18,19,23,06,06,5e` analytic-circle edge runs. The
/// class-`0x23` definition must close under the eight-scalar grammar.
#[must_use]
#[cfg(test)]
pub fn consolidated_analytic_circle_edge_runs(
    data: &[u8],
) -> Vec<ConsolidatedAnalyticCircleEdgeRun> {
    let records = consolidated_records(data);
    consolidated_analytic_circle_edge_runs_from_records(data, &records)
}

pub(crate) fn consolidated_analytic_circle_edge_runs_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedAnalyticCircleEdgeRun> {
    let circles = b2_circles_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let use_runs = consolidated_edge_use_runs_from_records(data, records)
        .into_iter()
        .map(|value| (value.uses[0].pos, value))
        .collect::<BTreeMap<_, _>>();
    records
        .windows(6)
        .filter_map(|window| {
            let [parameter, circle, definition, use0, use1, node] = window else {
                return None;
            };
            if !records_are_contiguous(window) {
                return None;
            }
            if parameter.family != ConsolidatedFamily::B
                || parameter.class != 0x18
                || circle.family != ConsolidatedFamily::B
                || circle.class != 0x19
                || definition.family != ConsolidatedFamily::B
                || definition.class != 0x23
                || use0.family != ConsolidatedFamily::B
                || use0.class != 0x06
                || use1.family != ConsolidatedFamily::B
                || use1.class != 0x06
                || node.family != ConsolidatedFamily::B
                || node.class != 0x5e
            {
                return None;
            }
            let use_run = use_runs.get(&use0.range.start)?;
            let definition = use_run.definition.clone()?;
            match definition.data()? {
                ConsolidatedEdgeDefinitionData::Scalar { values, .. } if values.len() == 8 => {}
                _ => return None,
            }
            Some(ConsolidatedAnalyticCircleEdgeRun {
                descriptor: ConsolidatedAnalyticCircleDescriptor {
                    frame: ConsolidatedRawFrame::from_record(
                        parameter,
                        data[parameter.payload.clone()].to_vec(),
                    )?,
                },
                circle: circles.get(&circle.range.start)?.clone(),
                #[cfg(test)]
                definition,
                node: use_run.node,
            })
        })
        .collect()
}

/// Decode adjacent `18,25,06,06,5e` edge runs whose descriptor and definition
/// both close under their typed grammars.
#[must_use]
#[cfg(test)]
pub fn consolidated_class25_edge_runs(data: &[u8]) -> Vec<ConsolidatedClass25EdgeRun> {
    let records = consolidated_records(data);
    consolidated_class25_edge_runs_from_records(data, &records)
}

pub(crate) fn consolidated_class25_edge_runs_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedClass25EdgeRun> {
    let descriptors = b2_class25_descriptors_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let use_runs = consolidated_edge_use_runs_from_records(data, records)
        .into_iter()
        .map(|value| (value.uses[0].pos, value))
        .collect::<BTreeMap<_, _>>();
    records
        .windows(5)
        .filter_map(|window| {
            let [descriptor, definition, use0, use1, node] = window else {
                return None;
            };
            if !records_are_contiguous(window) {
                return None;
            }
            if descriptor.family != ConsolidatedFamily::B
                || descriptor.class != 0x18
                || definition.family != ConsolidatedFamily::B
                || definition.class != 0x25
                || use0.family != ConsolidatedFamily::B
                || use0.class != 0x06
                || use1.family != ConsolidatedFamily::B
                || use1.class != 0x06
                || node.family != ConsolidatedFamily::B
                || node.class != 0x5e
            {
                return None;
            }
            let use_run = use_runs.get(&use0.range.start)?;
            let definition = use_run.definition.clone()?;
            if !matches!(
                definition.data(),
                Some(
                    ConsolidatedEdgeDefinitionData::Scalar25 { .. }
                        | ConsolidatedEdgeDefinitionData::SegmentedScalar25 { .. }
                )
            ) {
                return None;
            }
            Some(ConsolidatedClass25EdgeRun {
                descriptor: descriptors.get(&descriptor.range.start)?.clone(),
                node: use_run.node,
            })
        })
        .collect()
}

/// Decode every adjacent `06,06,5e` edge-use run independently of pcurve
/// availability. Records separated by another framed record do not form a run.
#[must_use]
#[cfg(test)]
pub fn consolidated_edge_use_runs(data: &[u8]) -> Vec<ConsolidatedEdgeUseRun> {
    let records = consolidated_records(data);
    consolidated_edge_use_runs_from_records(data, &records)
}

pub(crate) fn consolidated_edge_use_runs_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedEdgeUseRun> {
    let uses = b2_use_metadata_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let nodes = b2_edge_nodes_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let preceding = records
        .windows(3)
        .enumerate()
        .filter_map(|(index, window)| {
            let [use0, use1, node] = window else {
                return None;
            };
            if !records_are_contiguous(window) {
                return None;
            }
            if use0.family != ConsolidatedFamily::B
                || use0.class != 0x06
                || use1.family != ConsolidatedFamily::B
                || use1.class != 0x06
                || node.family != ConsolidatedFamily::B
                || node.class != 0x5e
            {
                return None;
            }
            let node = *nodes.get(&node.range.start)?;
            let uses = [
                uses.get(&use0.range.start)?.clone(),
                uses.get(&use1.range.start)?.clone(),
            ];
            let identity_chain_consistent = node
                .curve_ref
                .checked_sub(2)
                .zip(node.curve_ref.checked_sub(1))
                .is_some_and(|(first, second)| {
                    uses[0].references() == Some(&[first, second][..])
                        && uses[1].references() == Some(&[second, node.curve_ref][..])
                })
                && [node.start_parameter_ref, node.end_parameter_ref] == [2, 1];
            let definition = index
                .checked_sub(1)
                .and_then(|preceding| records.get(preceding))
                .filter(|record| {
                    record.source_index == use0.source_index
                        && record.source_range.end == use0.source_range.start
                        && record.physically_contiguous
                        && record.family == ConsolidatedFamily::B
                        && matches!(record.class, 0x23..=0x25)
                })
                .and_then(|record| {
                    Some(ConsolidatedEdgeDefinition {
                        frame: ConsolidatedRawFrame::from_record(
                            record,
                            data[record.payload.clone()].to_vec(),
                        )?,
                        class: record.class,
                    })
                });
            identity_chain_consistent.then(|| ConsolidatedEdgeUseRun {
                definition,
                uses,
                node,
            })
        })
        .collect::<Vec<_>>();
    let succeeding = records.windows(4).filter_map(|window| {
        let [node_record, definition_record, use0, use1] = window else {
            return None;
        };
        if !records_are_contiguous(window)
            || node_record.family != ConsolidatedFamily::B
            || node_record.class != 0x5e
            || definition_record.family != ConsolidatedFamily::B
            || !matches!(definition_record.class, 0x23..=0x25)
            || use0.family != ConsolidatedFamily::B
            || use0.class != 0x06
            || use1.family != ConsolidatedFamily::B
            || use1.class != 0x06
        {
            return None;
        }
        let node = *nodes.get(&node_record.range.start)?;
        let uses = [
            uses.get(&use0.range.start)?.clone(),
            uses.get(&use1.range.start)?.clone(),
        ];
        let definition_data = consolidated_edge_definition_data(
            definition_record.class,
            &data[definition_record.payload.clone()],
        );
        let identity_chain_consistent = match &definition_data {
            Some(ConsolidatedEdgeDefinitionData::Compact24 { operand }) => {
                operand
                    .checked_add(1)
                    .zip(operand.checked_add(2))
                    .is_some_and(|(first, second)| {
                        uses[0].references() == Some(&[node.start_parameter_ref, first][..])
                            && uses[1].references() == Some(&[node.end_parameter_ref, second][..])
                    })
                    && [node.start_parameter_ref, node.end_parameter_ref] == [1, 2]
            }
            _ => false,
        };
        if !identity_chain_consistent {
            return None;
        }
        Some(ConsolidatedEdgeUseRun {
            definition: Some(ConsolidatedEdgeDefinition {
                frame: ConsolidatedRawFrame::from_record(
                    definition_record,
                    data[definition_record.payload.clone()].to_vec(),
                )?,
                class: definition_record.class,
            }),
            uses,
            node,
        })
    });
    preceding.into_iter().chain(succeeding).collect()
}

/// Resolve compact owner references that land exactly on class-`0x5e` frames.
pub(crate) fn consolidated_owned_edge_nodes_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedOwnedEdgeNode> {
    let indices = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.range.start, index))
        .collect::<BTreeMap<_, _>>();
    let nodes = b2_edge_nodes_from_records(data, records)
        .into_iter()
        .map(|node| (node.pos, node))
        .collect::<BTreeMap<_, _>>();
    let mut owned = Vec::new();
    for relation in b2_adjacent_face_counted_owners_from_records(data, records) {
        let Some(&owner_index) = indices.get(&relation.owner.pos) else {
            continue;
        };
        for (allocation_ordinal, encoding) in relation
            .owner
            .references
            .into_iter()
            .zip(relation.owner.reference_encodings)
        {
            if encoding != AllocationReferenceEncoding::OwnedChild {
                continue;
            }
            let Some(target_index) = usize::try_from(allocation_ordinal)
                .ok()
                .and_then(|ordinal| owner_index.checked_add(1 + ordinal))
                .filter(|target| *target < records.len())
            else {
                continue;
            };
            if !records_are_contiguous(&records[owner_index..=target_index]) {
                continue;
            }
            let target = &records[target_index];
            if target.family != ConsolidatedFamily::B || target.class != 0x5e {
                continue;
            }
            let Some(&node) = nodes.get(&target.range.start) else {
                continue;
            };
            if owned
                .last()
                .is_some_and(|previous: &ConsolidatedOwnedEdgeNode| {
                    previous.owner_pos == relation.owner.pos && previous.node.pos == node.pos
                })
            {
                continue;
            }
            owned.push(ConsolidatedOwnedEdgeNode {
                owner_pos: relation.owner.pos,
                allocation_ordinal,
                node,
            });
        }
    }
    owned
}

/// Resolve compact edge endpoint references through the framed allocation walk.
pub(crate) fn consolidated_compact_edge_endpoints_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedCompactEdgeEndpoints> {
    struct EndpointResolver<'a> {
        records: &'a [ConsolidatedRecord],
        nodes: &'a HashMap<usize, B2EdgeNode>,
        allocation_locations: &'a HashMap<usize, (usize, usize)>,
        allocation_scopes: &'a [Vec<usize>],
        active: HashSet<(usize, usize)>,
        memo: HashMap<(usize, usize), Option<usize>>,
    }

    impl EndpointResolver<'_> {
        fn resolve(&mut self, record_index: usize, endpoint: usize) -> Option<usize> {
            let key = (record_index, endpoint);
            if let Some(cached) = self.memo.get(&key) {
                return *cached;
            }
            if !self.active.insert(key) {
                return None;
            }
            let result = (|| {
                let node = self.nodes.get(&record_index)?;
                let reference = [node.start_vertex_ref, node.end_vertex_ref][endpoint];
                let encoding = node.reference_encodings[endpoint + 1];
                let &(scope, allocation_ordinal) = self.allocation_locations.get(&record_index)?;
                let target = match encoding {
                    AllocationReferenceEncoding::OwnedChild => allocation_ordinal
                        .checked_add(1)?
                        .checked_add(usize::try_from(reference).ok()?)
                        .and_then(|target| self.allocation_scopes.get(scope)?.get(target))
                        .copied()?,
                    AllocationReferenceEncoding::BackwardDistance => {
                        let target =
                            allocation_ordinal.checked_sub(usize::try_from(reference).ok()?)?;
                        *self.allocation_scopes.get(scope)?.get(target)?
                    }
                    AllocationReferenceEncoding::WidthCoded => {
                        let target = record_index.checked_add(usize::try_from(reference).ok()?)?;
                        let target_record = self.records.get(target)?;
                        if target_record.source_index
                            != self.records.get(record_index)?.source_index
                            || target_record.family != ConsolidatedFamily::B
                            || target_record.class != 0x18
                        {
                            return None;
                        }
                        return Some(target);
                    }
                    AllocationReferenceEncoding::Selector2
                    | AllocationReferenceEncoding::TaggedU8
                    | AllocationReferenceEncoding::TaggedU16 => return None,
                };
                let target_record = self.records.get(target)?;
                if target_record.family != ConsolidatedFamily::B {
                    return None;
                }
                match target_record.class {
                    0x5d => Some(target),
                    0x5e => self.resolve(target, 1),
                    _ => None,
                }
            })();
            self.active.remove(&key);
            self.memo.insert(key, result);
            result
        }
    }

    let by_pos = b2_edge_nodes_from_records(data, records)
        .into_iter()
        .map(|node| (node.pos, node))
        .collect::<HashMap<_, _>>();
    let nodes = records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            by_pos
                .get(&record.range.start)
                .copied()
                .map(|node| (index, node))
        })
        .collect::<HashMap<_, _>>();
    let mut allocation_scopes = vec![Vec::new()];
    let mut allocation_locations = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        if index > 0
            && (records[index - 1].source_index != record.source_index
                || records[index - 1].source_range.end != record.source_range.start)
        {
            allocation_scopes.push(Vec::new());
        }
        if record.family == ConsolidatedFamily::B && matches!(record.class, 0x5d | 0x5e) {
            let scope = allocation_scopes.len() - 1;
            let ordinal = allocation_scopes[scope].len();
            allocation_scopes[scope].push(index);
            allocation_locations.insert(index, (scope, ordinal));
        }
    }
    let mut resolver = EndpointResolver {
        records,
        nodes: &nodes,
        allocation_locations: &allocation_locations,
        allocation_scopes: &allocation_scopes,
        active: HashSet::new(),
        memo: HashMap::new(),
    };
    let mut endpoints = nodes
        .iter()
        .filter_map(|(&record_index, &node)| {
            let vertices = [0, 1].map(|endpoint| resolver.resolve(record_index, endpoint));
            let [Some(start), Some(end)] = vertices else {
                return None;
            };
            Some(ConsolidatedCompactEdgeEndpoints {
                node,
                endpoint_records: [records[start].range.start, records[end].range.start],
            })
        })
        .collect::<Vec<_>>();
    endpoints.sort_by_key(|binding| binding.node.pos);
    endpoints
}

/// Derive owner-local four-edge boundary cycles from fixed-nine references.
/// Every returned endpoint is closed by the same bounded record source as its
/// owner. Other fixed-nine roles remain unclassified.
pub(crate) fn consolidated_owner_boundary_cycles_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedOwnerBoundaryCycle> {
    let endpoint_records = consolidated_compact_edge_endpoints_from_records(data, records)
        .into_iter()
        .map(|binding| (binding.node.pos, binding.endpoint_records))
        .collect::<HashMap<_, _>>();
    let face_nodes = b2_face_nodes_5f_from_records(data, records)
        .into_iter()
        .map(|node| (node.pos, node))
        .collect::<BTreeMap<_, _>>();
    let record_indices = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.range.start, index))
        .collect::<BTreeMap<_, _>>();
    let record_sources = records
        .iter()
        .map(|record| (record.range.start, record.source_index))
        .collect::<HashMap<_, _>>();
    let mut targets_by_owner = BTreeMap::<(usize, usize), Vec<_>>::new();
    for target in b2_owner_identity_targets_from_records(data, records) {
        targets_by_owner
            .entry((target.source_index, target.owner_pos))
            .or_default()
            .push(target);
    }
    b2_owner_packets_from_records(data, records)
        .into_iter()
        .filter_map(|packet| {
            let targets = targets_by_owner.get(&(packet.source_index, packet.pos))?;
            let edges = b2_closed_owner_boundary_edges(targets, &endpoint_records)?;
            let face_node = (|| {
                let first_edge_pos = edges.iter().map(|edge| edge.target_pos).min()?;
                let &first_edge_index = record_indices.get(&first_edge_pos)?;
                let node_index = first_edge_index.checked_sub(1)?;
                let node_record = records.get(node_index)?;
                let face_node = face_nodes.get(&node_record.range.start)?;
                if !matches!(face_node.terminal, [0x27, 0x03 | 0x05]) {
                    return None;
                }
                let &owner_index = record_indices.get(&packet.pos)?;
                if owner_index <= first_edge_index
                    || !records_are_contiguous(&records[node_index..=owner_index])
                {
                    return None;
                }
                let span = &records[first_edge_index..owner_index];
                if span.iter().any(|record| {
                    record.family != ConsolidatedFamily::B || !matches!(record.class, 0x5d | 0x5e)
                }) || span.iter().filter(|record| record.class == 0x5e).count() != 4
                {
                    return None;
                }
                if edges.iter().any(|edge| {
                    !span
                        .iter()
                        .any(|record| record.range.start == edge.target_pos)
                }) {
                    return None;
                }
                (packet.references[8].checked_add(10) == Some(face_node.target))
                    .then_some(*face_node)
            })();
            edges
                .iter()
                .all(|edge| {
                    record_sources.get(&edge.target_pos) == Some(&packet.source_index)
                        && edge.endpoint_records.iter().all(|endpoint| {
                            record_sources.get(endpoint) == Some(&packet.source_index)
                        })
                })
                .then_some(ConsolidatedOwnerBoundaryCycle {
                    source_index: packet.source_index,
                    owner_pos: packet.pos,
                    face_node,
                    edges,
                })
        })
        .collect()
}

/// Build the native endpoint-incidence graph for all complete consolidated
/// edge runs. A broken use/edge allocation chain invalidates the graph.
#[must_use]
#[cfg(test)]
pub fn consolidated_native_edge_graph(data: &[u8]) -> Option<ConsolidatedNativeEdgeGraph> {
    let runs = consolidated_topology_edge_runs(data);
    if runs.is_empty() {
        return None;
    }
    let mut vertex_indices = HashMap::new();
    let mut vertex_identities = Vec::new();
    let mut edges = Vec::with_capacity(runs.len());
    for run in runs {
        let vertices = [run.node.start_vertex_ref, run.node.end_vertex_ref].map(|identity| {
            *vertex_indices.entry(identity).or_insert_with(|| {
                let index = vertex_identities.len();
                vertex_identities.push(identity);
                index
            })
        });
        edges.push(ConsolidatedNativeGraphEdge { vertices });
    }
    let mut vertex_edges = vec![Vec::new(); vertex_identities.len()];
    for (edge, value) in edges.iter().enumerate() {
        for vertex in value.vertices {
            vertex_edges[vertex].push(edge);
        }
    }
    let mut unseen = (0..edges.len()).collect::<std::collections::BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(&first) = unseen.first() {
        let mut component = Vec::new();
        let mut stack = vec![first];
        unseen.remove(&first);
        while let Some(edge) = stack.pop() {
            component.push(edge);
            for vertex in edges[edge].vertices {
                for &neighbor in &vertex_edges[vertex] {
                    if unseen.remove(&neighbor) {
                        stack.push(neighbor);
                    }
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    Some(ConsolidatedNativeEdgeGraph {
        vertex_identities,
        edges,
        components,
    })
}

/// Resolve consolidated edge sides against typed cylinder, circle, cone, and
/// NURBS carriers.
///
/// A carrier binds only when record identity or chart geometry determines one
/// solution. Ambiguous candidates, including matches from different analytic
/// families, remain unresolved.
#[must_use]
#[cfg(test)]
pub fn resolve_consolidated_edge_blocks(data: &[u8]) -> Vec<ResolvedConsolidatedEdgeBlock> {
    let records = consolidated_records(data);
    resolve_consolidated_edge_blocks_from_records(data, &records)
}

pub(crate) fn resolve_consolidated_edge_blocks_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ResolvedConsolidatedEdgeBlock> {
    let points = object_stream_vertices_from_records(data, records);
    let embedded = b2_embedded_cylinders_from_records(data, records);
    let standalone = b2_cylinders_from_records(data, records);
    let circles = b2_circles_from_records(data, records);
    let cones = b2_cones_from_records(data, records);
    let spheres = b2_spheres_from_records(data, records);
    let tori = b2_tori_from_records(data, records);
    let planes = b2_plane_carriers_from_records(data, records);
    let surfaces = a5_surfaces_from_records(data, records);
    let carriers = ConsolidatedCarriers {
        cylinders: &standalone,
        embedded_cylinders: &embedded,
        cones: &cones,
        spheres: &spheres,
        tori: &tori,
        planes: &planes,
        nurbs_surfaces: &surfaces,
    };
    consolidated_edge_blocks_from_records(data, records)
        .into_iter()
        .map(|block| {
            let mut supports = std::array::from_fn(|side| {
                let pcurve = &block.pcurves[side];
                let mut winners = Vec::new();
                let mut ambiguous_family = false;
                let identity_circles: Vec<_> = circles
                    .iter()
                    .filter(|circle| circle.record_id == pcurve.support_id)
                    .collect();
                let identity_embedded: Vec<_> = embedded
                    .iter()
                    .filter(|value| value.object_id == pcurve.support_id)
                    .collect();
                let identity_count = identity_circles.len() + identity_embedded.len();
                if identity_count == 0 {
                    for cylinder in &standalone {
                        if pcurve_endpoints_match_vertices(pcurve, cylinder, &points) {
                            winners
                                .push(ConsolidatedSupportBinding::Cylinder { pos: cylinder.pos });
                        }
                    }
                    winners.extend(
                        embedded
                            .iter()
                            .filter(|value| {
                                pcurve_endpoints_match_vertices(pcurve, &value.cylinder, &points)
                            })
                            .map(|value| ConsolidatedSupportBinding::EmbeddedCylinder {
                                pos: value.pos,
                                wrapper_pos: value.wrapper_pos,
                            }),
                    );
                    winners.extend(
                        circles
                            .iter()
                            .filter(|circle| pcurve_matches_circle(pcurve, circle))
                            .map(|circle| ConsolidatedSupportBinding::Circle { pos: circle.pos }),
                    );
                    winners.extend(
                        cones
                            .iter()
                            .filter(|cone| pcurve_endpoints_match_cone(pcurve, cone, &points))
                            .map(|cone| ConsolidatedSupportBinding::Cone { pos: cone.pos }),
                    );
                    winners.extend(
                        spheres
                            .iter()
                            .filter(|sphere| pcurve_endpoints_match_sphere(pcurve, sphere, &points))
                            .map(|sphere| ConsolidatedSupportBinding::Sphere { pos: sphere.pos }),
                    );
                    winners.extend(
                        tori.iter()
                            .filter(|torus| pcurve_endpoints_match_torus(pcurve, torus, &points))
                            .map(|torus| ConsolidatedSupportBinding::Torus { pos: torus.pos }),
                    );
                    winners.extend(
                        planes
                            .iter()
                            .filter(|plane| pcurve_endpoints_match_plane(pcurve, plane, &points))
                            .map(|plane| ConsolidatedSupportBinding::Plane { pos: plane.pos }),
                    );
                } else if identity_count > 1 {
                    ambiguous_family = true;
                } else if let [circle] = identity_circles.as_slice() {
                    if pcurve_matches_circle(pcurve, circle) {
                        winners.push(ConsolidatedSupportBinding::Circle { pos: circle.pos });
                    } else {
                        ambiguous_family = true;
                    }
                } else if let [value] = identity_embedded.as_slice() {
                    if pcurve_endpoints_match_vertices(pcurve, &value.cylinder, &points) {
                        winners.push(ConsolidatedSupportBinding::EmbeddedCylinder {
                            pos: value.pos,
                            wrapper_pos: value.wrapper_pos,
                        });
                    } else {
                        ambiguous_family = true;
                    }
                } else {
                    ambiguous_family = true;
                }
                if !ambiguous_family && winners.len() == 1 {
                    winners.pop()
                } else {
                    None
                }
            });
            for anchor_side in [0, 1] {
                let partner = 1 - anchor_side;
                if supports[partner].is_some() {
                    continue;
                }
                let Some(anchor_points) = supports[anchor_side].as_ref().and_then(|binding| {
                    support_points(binding, &block.pcurves[anchor_side], &carriers)
                }) else {
                    continue;
                };
                let partner_points = block.pcurves[partner].points();
                let winners: Vec<_> = surfaces
                    .iter()
                    .filter_map(|surface| {
                        nurbs_carrier_offset(
                            &SurfaceGeometry::Nurbs(surface.geometry.clone()),
                            &partner_points,
                            &anchor_points,
                        )
                        .map(|offset| {
                            ConsolidatedSupportBinding::NurbsCarrier {
                                pos: surface.pos,
                                offset,
                            }
                        })
                    })
                    .collect();
                if let [winner] = winners.as_slice() {
                    supports[partner] = Some(winner.clone());
                }
            }
            if supports.iter().all(Option::is_none) {
                let candidates = block.pcurves.each_ref().map(|pcurve| {
                    surfaces
                        .iter()
                        .filter_map(|surface| {
                            let binding = ConsolidatedSupportBinding::NurbsCarrier {
                                pos: surface.pos,
                                offset: 0.0,
                            };
                            let points = support_points(&binding, pcurve, &carriers)?;
                            Some((binding, points))
                        })
                        .collect::<Vec<_>>()
                });
                let mut winner = None;
                'pairs: for (first_binding, first_points) in &candidates[0] {
                    for (second_binding, second_points) in &candidates[1] {
                        if !point_sequences_agree(first_points, second_points) {
                            continue;
                        }
                        if winner.is_some() {
                            winner = None;
                            break 'pairs;
                        }
                        winner = Some([first_binding.clone(), second_binding.clone()]);
                    }
                }
                if let Some(winner) = winner {
                    supports = winner.map(Some);
                }
            }
            let shared_loci = resolved_support_loci(&block, &supports, &carriers);
            let endpoint_loci = shared_loci
                .as_ref()
                .and_then(|points| Some([*points.first()?, *points.last()?]));
            ResolvedConsolidatedEdgeBlock {
                block,
                supports,
                shared_loci,
                endpoint_loci,
            }
        })
        .collect()
}

fn point_sequences_agree(first: &[Point3], second: &[Point3]) -> bool {
    !first.is_empty()
        && first.len() == second.len()
        && first
            .iter()
            .zip(second)
            .all(|(&left, &right)| point_distance(left, right) <= 2e-3)
}

fn resolved_support_loci(
    block: &ConsolidatedEdgeBlock,
    supports: &[Option<ConsolidatedSupportBinding>; 2],
    carriers: &ConsolidatedCarriers<'_>,
) -> Option<Vec<Point3>> {
    let candidates = supports
        .iter()
        .zip(&block.pcurves)
        .filter_map(|(binding, pcurve)| {
            let points = support_points(binding.as_ref()?, pcurve, carriers)?;
            (!points.is_empty()).then_some(points)
        })
        .collect::<Vec<_>>();
    let first = candidates.first()?;
    candidates
        .iter()
        .all(|candidate| point_sequences_agree(first, candidate))
        .then(|| first.clone())
}

fn support_points(
    binding: &ConsolidatedSupportBinding,
    pcurve: &ConsolidatedPcurve,
    carriers: &ConsolidatedCarriers<'_>,
) -> Option<Vec<Point3>> {
    match binding {
        ConsolidatedSupportBinding::Cylinder { pos } => {
            let carrier = carriers.cylinders.iter().find(|value| value.pos == *pos)?;
            pcurve
                .sites
                .iter()
                .map(|site| b2_cylinder_point(carrier, site.point))
                .collect()
        }
        ConsolidatedSupportBinding::EmbeddedCylinder { pos, .. } => {
            let carrier = &carriers
                .embedded_cylinders
                .iter()
                .find(|value| value.pos == *pos)?
                .cylinder;
            pcurve
                .sites
                .iter()
                .map(|site| b2_cylinder_point(carrier, site.point))
                .collect()
        }
        ConsolidatedSupportBinding::Cone { pos } => {
            let carrier = carriers.cones.iter().find(|value| value.pos == *pos)?;
            pcurve
                .sites
                .iter()
                .map(|site| b2_cone_point(carrier, site.point))
                .collect()
        }
        ConsolidatedSupportBinding::Sphere { pos } => {
            let carrier = carriers.spheres.iter().find(|value| value.pos == *pos)?;
            pcurve
                .sites
                .iter()
                .map(|site| {
                    let [u, v] = site.point;
                    cadmpeg_ir::eval::surface_point(&b2_sphere_geometry(carrier), u, v)
                })
                .collect()
        }
        ConsolidatedSupportBinding::Torus { pos } => {
            let carrier = carriers.tori.iter().find(|value| value.pos == *pos)?;
            pcurve
                .sites
                .iter()
                .map(|site| b2_torus_point(carrier, site.point))
                .collect()
        }
        ConsolidatedSupportBinding::Plane { pos } => {
            let carrier = carriers.planes.iter().find(|value| value.pos == *pos)?;
            let geometry = b2_plane_geometry(carrier)?;
            pcurve
                .sites
                .iter()
                .map(|site| {
                    let [u, v] = site.point;
                    cadmpeg_ir::eval::surface_point(&geometry, u, v)
                })
                .collect()
        }
        ConsolidatedSupportBinding::NurbsCarrier { pos, offset } => {
            let surface = &carriers
                .nurbs_surfaces
                .iter()
                .find(|surface| surface.pos == *pos)?
                .geometry;
            pcurve
                .sites
                .iter()
                .map(|site| {
                    let [u, v] = site.point;
                    let partials = nurbs_surface_partials(surface, u, v)?;
                    let normal = partials.du.cross(partials.dv).unit()?;
                    Some(Point3::new(
                        partials.point.x + offset * normal.x,
                        partials.point.y + offset * normal.y,
                        partials.point.z + offset * normal.z,
                    ))
                })
                .collect()
        }
        ConsolidatedSupportBinding::Circle { .. } => None,
    }
}

fn b2_torus_point(torus: &B2Torus, [u, v]: [f64; 2]) -> Option<Point3> {
    cadmpeg_ir::eval::surface_point(
        &b2_torus_geometry(torus),
        u / torus.major_scale,
        v / torus.minor_scale,
    )
}

fn nurbs_carrier_offset(
    geometry: &SurfaceGeometry,
    parameters: &[[f64; 2]],
    anchors: &[Point3],
) -> Option<f64> {
    let SurfaceGeometry::Nurbs(surface) = geometry else {
        return None;
    };
    if parameters.len() != anchors.len() || parameters.is_empty() {
        return None;
    }
    let mut offsets = Vec::with_capacity(parameters.len());
    for (&[u, v], &anchor) in parameters.iter().zip(anchors) {
        let partials = nurbs_surface_partials(surface, u, v)?;
        let point = partials.point;
        let residual = Vector3::new(anchor.x - point.x, anchor.y - point.y, anchor.z - point.z);
        if residual == Vector3::new(0.0, 0.0, 0.0) {
            offsets.push(0.0);
            continue;
        }
        let residual_length = residual.x.hypot(residual.y).hypot(residual.z);
        let normal = partials.du.cross(partials.dv).unit()?;
        let distance = residual.x * normal.x + residual.y * normal.y + residual.z * normal.z;
        let transverse = Vector3::new(
            residual.x - normal.x * distance,
            residual.y - normal.y * distance,
            residual.z - normal.z * distance,
        );
        let transverse_length = transverse.x.hypot(transverse.y).hypot(transverse.z);
        if transverse_length > EPS_TRANSVERSE_RESIDUAL * residual_length {
            return None;
        }
        offsets.push(distance);
    }
    let first = offsets[0];
    if !first.is_finite()
        || offsets.iter().any(|value| {
            (value - first).abs() > EPS_SAMPLE_AGREEMENT * value.abs().max(first.abs())
        })
    {
        return None;
    }
    Some(first)
}

fn pcurve_matches_circle(pcurve: &ConsolidatedPcurve, circle: &B2Circle) -> bool {
    let (Some(first), Some(last)) = (
        pcurve.sites.first().map(|site| site.point),
        pcurve.sites.last().map(|site| site.point),
    ) else {
        return false;
    };
    let span = circle.range[1] - circle.range[0];
    span.is_finite()
        && span > 0.0
        && (first[1] - last[1]).abs() <= EPS_ENDPOINT_RANGE * span
        && (first[0].min(last[0]) - circle.range[0]).abs() <= EPS_CIRCLE_ENDPOINT * span
        && (first[0].max(last[0]) - circle.range[1]).abs() <= EPS_CIRCLE_ENDPOINT * span
}

fn pcurve_endpoints_match_cone(
    pcurve: &ConsolidatedPcurve,
    cone: &B2Cone,
    vertices: &[Point3],
) -> bool {
    let (Some(first), Some(last)) = (
        pcurve.sites.first().map(|site| site.point),
        pcurve.sites.last().map(|site| site.point),
    ) else {
        return false;
    };
    [first, last].into_iter().all(|uv| {
        b2_cone_point(cone, uv).is_some_and(|point| {
            vertices
                .iter()
                .any(|vertex| point_distance(point, *vertex) < 2e-3)
        })
    })
}

fn pcurve_endpoints_match_torus(
    pcurve: &ConsolidatedPcurve,
    torus: &B2Torus,
    vertices: &[Point3],
) -> bool {
    let (Some(first), Some(last)) = (
        pcurve.sites.first().map(|site| site.point),
        pcurve.sites.last().map(|site| site.point),
    ) else {
        return false;
    };
    [first, last].into_iter().all(|uv| {
        b2_torus_point(torus, uv).is_some_and(|point| {
            vertices
                .iter()
                .any(|vertex| point_distance(point, *vertex) < 2e-3)
        })
    })
}

fn pcurve_endpoints_match_sphere(
    pcurve: &ConsolidatedPcurve,
    sphere: &B2Sphere,
    vertices: &[Point3],
) -> bool {
    let (Some(first), Some(last)) = (
        pcurve.sites.first().map(|site| site.point),
        pcurve.sites.last().map(|site| site.point),
    ) else {
        return false;
    };
    [first, last].into_iter().all(|[u, v]| {
        cadmpeg_ir::eval::surface_point(&b2_sphere_geometry(sphere), u, v).is_some_and(|point| {
            vertices
                .iter()
                .any(|vertex| point_distance(point, *vertex) < 2e-3)
        })
    })
}

fn pcurve_endpoints_match_plane(
    pcurve: &ConsolidatedPcurve,
    plane: &B2PlaneCarrier,
    vertices: &[Point3],
) -> bool {
    let Some(geometry) = b2_plane_geometry(plane) else {
        return false;
    };
    let (Some(first), Some(last)) = (
        pcurve.sites.first().map(|site| site.point),
        pcurve.sites.last().map(|site| site.point),
    ) else {
        return false;
    };
    [first, last].into_iter().all(|[u, v]| {
        cadmpeg_ir::eval::surface_point(&geometry, u, v).is_some_and(|point| {
            vertices
                .iter()
                .any(|vertex| point_distance(point, *vertex) < 2e-3)
        })
    })
}

fn pcurve_endpoints_match_vertices(
    pcurve: &ConsolidatedPcurve,
    cylinder: &B2Cylinder,
    vertices: &[Point3],
) -> bool {
    let Some(first) = pcurve
        .sites
        .first()
        .and_then(|site| b2_cylinder_point(cylinder, site.point))
    else {
        return false;
    };
    let Some(last) = pcurve
        .sites
        .last()
        .and_then(|site| b2_cylinder_point(cylinder, site.point))
    else {
        return false;
    };
    [first, last].iter().all(|point| {
        vertices
            .iter()
            .any(|vertex| point_distance(*point, *vertex) < 2e-3)
    })
}

/// Read `05 08 01` coordinate rows outside every length-closed consolidated
/// A/B or B5/A8 record. Marker-like bytes inside record payloads are not
/// vertices.
#[must_use]
pub(crate) fn object_stream_vertices(data: &[u8]) -> Vec<Point3> {
    let records = consolidated_records(data);
    object_stream_vertices_from_records(data, &records)
}

pub(crate) fn object_stream_vertices_from_records(
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
) -> Vec<Point3> {
    object_stream_vertex_row_ranges_from_records(data, records)
        .into_iter()
        .flat_map(|range| crate::wire::records::scan_vertex_records(&data[range]))
        .collect()
}

fn object_stream_vertex_row_ranges_from_records(
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
) -> Vec<Range<usize>> {
    let mut ranges = records
        .iter()
        .map(|record| record.range.clone())
        .chain(crate::families::b5::graph::framed_ranges(data))
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return Vec::new();
    }
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    let mut rows = Vec::new();
    let mut region_start = 0usize;
    for range in ranges {
        if range.end <= region_start {
            continue;
        }
        if range.start > region_start {
            rows.extend(
                scan_vertex_record_ranges(&data[region_start..range.start])
                    .into_iter()
                    .map(|row| row.start + region_start..row.end + region_start),
            );
        }
        region_start = region_start.max(range.end);
    }
    rows.extend(
        scan_vertex_record_ranges(&data[region_start..])
            .into_iter()
            .map(|row| row.start + region_start..row.end + region_start),
    );
    rows
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::geometry::{NurbsSurface, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;

    use crate::families::b2::records::B2Circle;
    use crate::wire::records::ConsolidatedPcurve;

    use super::{nurbs_carrier_offset, pcurve_matches_circle};

    #[test]
    fn nurbs_carrier_offset_preserves_tiny_nonzero_distance() {
        let surface = SurfaceGeometry::Nurbs(
            NurbsSurface::new(
                1,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
                2,
                2,
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                ],
                None,
                false,
                false,
                false,
            )
            .expect("valid unit-square surface"),
        );
        let tiny = 1e-200;
        let offset = nurbs_carrier_offset(
            &surface,
            &[[0.25, 0.25], [0.75, 0.75]],
            &[Point3::new(0.25, 0.25, tiny), Point3::new(0.75, 0.75, tiny)],
        )
        .expect("constant normal offset");
        assert_eq!(offset, tiny);

        assert_eq!(
            nurbs_carrier_offset(
                &surface,
                &[[0.25, 0.25], [0.75, 0.75]],
                &[
                    Point3::new(0.25, 0.25, tiny),
                    Point3::new(0.75, 0.75, 2.0 * tiny),
                ],
            ),
            None
        );
        assert_eq!(
            nurbs_carrier_offset(&surface, &[[0.0, 0.0]], &[Point3::new(tiny, 0.0, tiny)],),
            None
        );
    }

    #[test]
    fn circle_binding_is_relative_to_the_arc_length_span() {
        let span = 1e-200_f64;
        let circle = B2Circle {
            pos: 0,
            layout: crate::native::CatiaCircleLayout::Identity6Bit,
            record_id: 1,
            frame_token: 0,
            center_pair: [0.0; 2],
            radius: span,
            range: [0.0, span],
            full_circle: false,
            chart_shift: 0.0,
        };
        let pcurve = |points: Vec<[f64; 2]>| ConsolidatedPcurve {
            pos: 0,
            support_id: 1,
            extrapolation_sites: 0,
            sites: points
                .into_iter()
                .enumerate()
                .map(
                    |(index, point)| crate::wire::records::ConsolidatedPcurveSite {
                        knot: if index == 0 { 0.0 } else { span },
                        point,
                        first_derivatives: [0.0, 0.0],
                        second_derivatives: [0.0, 0.0],
                    },
                )
                .collect(),
            range: [0.0, span],
            tail: Vec::new(),
        };

        assert!(pcurve_matches_circle(
            &pcurve(vec![[0.0, span], [span, span]]),
            &circle
        ));
        assert!(!pcurve_matches_circle(
            &pcurve(vec![[0.0, span], [span, 2.0 * span]]),
            &circle
        ));
        assert!(!pcurve_matches_circle(
            &pcurve(vec![[0.0, span], [2.0 * span, span]]),
            &circle
        ));
    }
}
