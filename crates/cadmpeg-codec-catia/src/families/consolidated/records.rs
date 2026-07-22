//! Consolidated record framing and edge-resolution vocabulary.
//!
//! Inventories length-closed A/B-family records, groups consolidated edge runs
//! and their native incidence graph, and resolves edge-block side carriers
//! against typed analytic and NURBS charts.

use cadmpeg_ir::eval::nurbs_surface_partials;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::le::{u16_at as u16_le, u32_at as u32_le};
use cadmpeg_ir::math::{Point3, Vector3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::HashMap;
use std::ops::Range;

use crate::families::a5a8::records::{a5_pcurves, a5_surfaces, A8Surface};
use crate::families::b2::records::{
    b2_circles, b2_class25_descriptors, b2_cone_point, b2_cones, b2_cylinder_point, b2_cylinders,
    b2_edge_nodes, b2_edge_parameters, b2_embedded_cylinders, b2_pcurves, b2_use_metadata,
    point_distance, B2Circle, B2Class25Descriptor, B2Cone, B2Cylinder, B2EdgeNode,
    B2EdgeParameters, B2EmbeddedCylinder, B2UseMetadata,
};
use crate::families::standard::records::scan_vertex_records;
use crate::wire::bytes::{
    allocation_ref, compact_int, f64_le, finite_f64_lane, persistent_ref, read_f64_array,
};

/// Degree-5 UV jet stored in an A- or B-family class-`0x20` consolidated record.
#[derive(Debug, Clone)]
pub struct ConsolidatedPcurve {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced support-surface identifier.
    pub support_id: u32,
    /// Parametric curve degree.
    pub degree: u32,
    /// Number of leading extrapolation sites encoded by the array marker.
    pub extrapolation_sites: u32,
    /// Global parameters at the stored sites.
    pub knots: Vec<f64>,
    /// UV positions at the stored sites.
    pub points: Vec<[f64; 2]>,
    /// UV first derivatives at the stored sites.
    pub first_derivatives: Vec<[f64; 2]>,
    /// UV second derivatives at the stored sites.
    pub second_derivatives: Vec<[f64; 2]>,
    /// Native parameter range.
    pub range: [f64; 2],
    /// Bytes following the native range inside the framed record.
    pub tail: Vec<u8>,
}

/// Serialized consolidated edge block formed by two pcurves and one range packet.
#[derive(Debug, Clone)]
pub struct ConsolidatedEdgeBlock {
    /// The two face-side UV definitions in serialization order.
    pub pcurves: [ConsolidatedPcurve; 2],
    /// Shared parameter range and tolerance packet.
    pub parameters: B2EdgeParameters,
    /// Both pcurves and the edge packet store the same native range and site count.
    pub co_parametric: bool,
}

/// Complete consolidated edge run serialized as two side pcurves, their shared
/// parameter packet, two oriented uses, and one native edge node.
#[derive(Debug, Clone)]
pub struct ConsolidatedTopologyEdgeRun {
    /// Co-parametric side definitions and shared range packet.
    pub edge: ConsolidatedEdgeBlock,
    /// The two serialized edge uses, in side order.
    #[cfg(test)]
    pub uses: [B2UseMetadata; 2],
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub node: B2EdgeNode,
    /// Whether the two counted use-reference vectors form the allocation chain
    /// ending at the node's curve reference.
    pub identity_chain_consistent: bool,
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
    /// Whether the use references and endpoint selectors close one allocation chain.
    pub identity_chain_consistent: bool,
}

/// Exact class-`0x18` frame attached to an analytic circle carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidatedAnalyticCircleDescriptor {
    /// Record byte offset.
    pub pos: usize,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Complete class-specific payload.
    pub payload: Vec<u8>,
}

/// Complete class-`0x25` edge run with its adjacent class-`0x18` descriptor.
#[derive(Debug, Clone)]
pub struct ConsolidatedClass25EdgeRun {
    /// Typed class-`0x18` descriptor.
    pub descriptor: B2Class25Descriptor,
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub node: B2EdgeNode,
    /// Whether the use references and endpoint selectors close one allocation chain.
    pub identity_chain_consistent: bool,
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
    /// Whether the use references and endpoint selectors close one allocation chain.
    pub identity_chain_consistent: bool,
}

/// Framed edge definition structurally owned by an adjacent oriented-use run.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidatedEdgeDefinition {
    /// Record byte offset.
    pub pos: usize,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Edge-definition class in `0x23..=0x25`.
    pub class: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Complete class-specific payload.
    pub payload: Vec<u8>,
    /// Structurally decoded class-specific payload, when its complete grammar closes.
    pub data: Option<ConsolidatedEdgeDefinitionData>,
}

/// Closed payload grammar of a consolidated edge-definition frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
            && values[5] == 1.0)
    {
        return None;
    }
    Some(ConsolidatedEdgeDefinitionData::Scalar { operands, values })
}

fn class25_persistent_ref(bytes: &[u8], at: &mut usize) -> Option<(u32, Option<u8>)> {
    match *bytes.get(*at)? {
        lead @ (0x0a | 0x0b) => {
            let value = u32::from(u16_le(bytes, *at + 1)?);
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
    /// Complete serialized edge run.
    pub run: ConsolidatedTopologyEdgeRun,
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

/// Group ordered pairs of same-family class-`0x20` pcurves followed by one
/// B-family class-`0x23` range packet.
#[must_use]
pub fn consolidated_edge_blocks(data: &[u8]) -> Vec<ConsolidatedEdgeBlock> {
    let pcurves = a5_pcurves(data)
        .into_iter()
        .chain(b2_pcurves(data))
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let parameters = b2_edge_parameters(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    consolidated_records(data)
        .windows(3)
        .filter_map(|window| {
            let [first_record, second_record, parameter_record] = window else {
                return None;
            };
            if first_record.class == 0x20
                && second_record.class == 0x20
                && first_record.family == second_record.family
                && parameter_record.family == ConsolidatedFamily::B
                && parameter_record.class == 0x23
            {
                let first = pcurves.get(&first_record.range.start)?;
                let second = pcurves.get(&second_record.range.start)?;
                let parameters = parameters.get(&parameter_record.range.start)?;
                let co_parametric = first.points.len() == second.points.len()
                    && first.range == second.range
                    && first.range == parameters.range;
                Some(ConsolidatedEdgeBlock {
                    pcurves: [first.clone(), second.clone()],
                    parameters: parameters.clone(),
                    co_parametric,
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
pub fn consolidated_topology_edge_runs(data: &[u8]) -> Vec<ConsolidatedTopologyEdgeRun> {
    let edges = consolidated_edge_blocks(data)
        .into_iter()
        .map(|edge| (edge.pcurves[0].pos, edge))
        .collect::<BTreeMap<_, _>>();
    let use_runs = consolidated_edge_use_runs(data)
        .into_iter()
        .map(|value| (value.uses[0].pos, value))
        .collect::<BTreeMap<_, _>>();
    consolidated_records(data)
        .windows(6)
        .filter_map(|window| {
            let [pcurve0, pcurve1, parameters, use0, use1, node] = window else {
                return None;
            };
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
                    #[cfg(test)]
                    uses: use_run.uses.clone(),
                    node: use_run.node,
                    identity_chain_consistent: use_run.identity_chain_consistent,
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
pub fn consolidated_analytic_circle_edge_runs(
    data: &[u8],
) -> Vec<ConsolidatedAnalyticCircleEdgeRun> {
    let circles = b2_circles(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let use_runs = consolidated_edge_use_runs(data)
        .into_iter()
        .map(|value| (value.uses[0].pos, value))
        .collect::<BTreeMap<_, _>>();
    consolidated_records(data)
        .windows(6)
        .filter_map(|window| {
            let [parameter, circle, definition, use0, use1, node] = window else {
                return None;
            };
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
            match definition.data.as_ref()? {
                ConsolidatedEdgeDefinitionData::Scalar { values, .. } if values.len() == 8 => {}
                _ => return None,
            }
            Some(ConsolidatedAnalyticCircleEdgeRun {
                descriptor: ConsolidatedAnalyticCircleDescriptor {
                    pos: parameter.range.start,
                    width: parameter.width,
                    flag: parameter.flag,
                    header_token: parameter.header_token,
                    payload: data[parameter.payload.clone()].to_vec(),
                },
                circle: circles.get(&circle.range.start)?.clone(),
                #[cfg(test)]
                definition,
                node: use_run.node,
                identity_chain_consistent: use_run.identity_chain_consistent,
            })
        })
        .collect()
}

/// Decode adjacent `18,25,06,06,5e` edge runs whose descriptor and definition
/// both close under their typed grammars.
#[must_use]
pub fn consolidated_class25_edge_runs(data: &[u8]) -> Vec<ConsolidatedClass25EdgeRun> {
    let descriptors = b2_class25_descriptors(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let use_runs = consolidated_edge_use_runs(data)
        .into_iter()
        .map(|value| (value.uses[0].pos, value))
        .collect::<BTreeMap<_, _>>();
    consolidated_records(data)
        .windows(5)
        .filter_map(|window| {
            let [descriptor, definition, use0, use1, node] = window else {
                return None;
            };
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
                definition.data.as_ref(),
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
                identity_chain_consistent: use_run.identity_chain_consistent,
            })
        })
        .collect()
}

/// Decode every adjacent `06,06,5e` edge-use run independently of pcurve
/// availability. Records separated by another framed record do not form a run.
#[must_use]
pub fn consolidated_edge_use_runs(data: &[u8]) -> Vec<ConsolidatedEdgeUseRun> {
    let uses = b2_use_metadata(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let nodes = b2_edge_nodes(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let records = consolidated_records(data);
    records
        .windows(3)
        .enumerate()
        .filter_map(|(index, window)| {
            let [use0, use1, node] = window else {
                return None;
            };
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
                    uses[0].references.as_deref() == Some(&[first, second])
                        && uses[1].references.as_deref() == Some(&[second, node.curve_ref])
                })
                && [node.start_parameter_ref, node.end_parameter_ref] == [2, 1];
            let definition = index
                .checked_sub(1)
                .and_then(|preceding| records.get(preceding))
                .filter(|record| {
                    record.family == ConsolidatedFamily::B && matches!(record.class, 0x23..=0x25)
                })
                .map(|record| ConsolidatedEdgeDefinition {
                    pos: record.range.start,
                    width: record.width,
                    flag: record.flag,
                    class: record.class,
                    header_token: record.header_token,
                    payload: data[record.payload.clone()].to_vec(),
                    data: consolidated_edge_definition_data(
                        record.class,
                        &data[record.payload.clone()],
                    ),
                });
            Some(ConsolidatedEdgeUseRun {
                definition,
                uses,
                node,
                identity_chain_consistent,
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
        if !run.identity_chain_consistent {
            return None;
        }
        let vertices = [run.node.start_vertex_ref, run.node.end_vertex_ref].map(|identity| {
            *vertex_indices.entry(identity).or_insert_with(|| {
                let index = vertex_identities.len();
                vertex_identities.push(identity);
                index
            })
        });
        edges.push(ConsolidatedNativeGraphEdge { run, vertices });
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
/// A carrier wins only when it is the sole chart whose two lifted pcurve endpoints
/// coincide with serialized `05 08 01` vertices at single-precision tolerance.
#[must_use]
pub fn resolve_consolidated_edge_blocks(data: &[u8]) -> Vec<ResolvedConsolidatedEdgeBlock> {
    let points = object_stream_vertices(data);
    let standalone = b2_cylinders(data);
    let embedded = b2_embedded_cylinders(data);
    let circles = b2_circles(data);
    let cones = b2_cones(data);
    let surfaces = a5_surfaces(data);
    consolidated_edge_blocks(data)
        .into_iter()
        .map(|block| {
            let mut supports = std::array::from_fn(|side| {
                let pcurve = &block.pcurves[side];
                let mut winners = Vec::new();
                for cylinder in &standalone {
                    if cylinder.geometry.is_some()
                        && pcurve_endpoints_match_vertices(pcurve, cylinder, &points)
                    {
                        winners.push(ConsolidatedSupportBinding::Cylinder { pos: cylinder.pos });
                    }
                }
                for value in &embedded {
                    if pcurve_endpoints_match_vertices(pcurve, &value.cylinder, &points) {
                        winners.push(ConsolidatedSupportBinding::EmbeddedCylinder {
                            pos: value.pos,
                            wrapper_pos: value.wrapper_pos,
                        });
                    }
                }
                if winners.is_empty() {
                    let mut circle_winners: Vec<_> = circles
                        .iter()
                        .filter(|circle| pcurve_matches_circle(pcurve, circle))
                        .map(|circle| ConsolidatedSupportBinding::Circle { pos: circle.pos })
                        .collect();
                    if circle_winners.len() == 1 {
                        winners.append(&mut circle_winners);
                    }
                }
                if winners.is_empty() {
                    let mut cone_winners: Vec<_> = cones
                        .iter()
                        .filter(|cone| pcurve_endpoints_match_cone(pcurve, cone, &points))
                        .map(|cone| ConsolidatedSupportBinding::Cone { pos: cone.pos })
                        .collect();
                    if cone_winners.len() == 1 {
                        winners.append(&mut cone_winners);
                    }
                }
                (winners.len() == 1).then(|| winners.remove(0))
            });
            for anchor_side in [0, 1] {
                let partner = 1 - anchor_side;
                if supports[partner].is_some() {
                    continue;
                }
                let Some(anchor_points) = supports[anchor_side].as_ref().and_then(|binding| {
                    support_points(
                        binding,
                        &block.pcurves[anchor_side],
                        &standalone,
                        &embedded,
                        &cones,
                        &surfaces,
                    )
                }) else {
                    continue;
                };
                let winners: Vec<_> = surfaces
                    .iter()
                    .filter_map(|surface| {
                        nurbs_carrier_offset(
                            &surface.geometry,
                            &block.pcurves[partner].points,
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
            let shared_loci =
                resolved_support_loci(&block, &supports, &standalone, &embedded, &cones, &surfaces);
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

fn resolved_support_loci(
    block: &ConsolidatedEdgeBlock,
    supports: &[Option<ConsolidatedSupportBinding>; 2],
    cylinders: &[B2Cylinder],
    embedded: &[B2EmbeddedCylinder],
    cones: &[B2Cone],
    surfaces: &[A8Surface],
) -> Option<Vec<Point3>> {
    let candidates = supports
        .iter()
        .zip(&block.pcurves)
        .filter_map(|(binding, pcurve)| {
            let points = support_points(
                binding.as_ref()?,
                pcurve,
                cylinders,
                embedded,
                cones,
                surfaces,
            )?;
            (!points.is_empty()).then_some(points)
        })
        .collect::<Vec<_>>();
    let first = candidates.first()?;
    candidates
        .iter()
        .all(|candidate| {
            candidate.len() == first.len()
                && first
                    .iter()
                    .zip(candidate)
                    .all(|(&left, &right)| point_distance(left, right) <= 2e-3)
        })
        .then(|| first.clone())
}

fn support_points(
    binding: &ConsolidatedSupportBinding,
    pcurve: &ConsolidatedPcurve,
    cylinders: &[B2Cylinder],
    embedded: &[B2EmbeddedCylinder],
    cones: &[B2Cone],
    surfaces: &[A8Surface],
) -> Option<Vec<Point3>> {
    match binding {
        ConsolidatedSupportBinding::Cylinder { pos } => {
            let carrier = cylinders.iter().find(|value| value.pos == *pos)?;
            pcurve
                .points
                .iter()
                .map(|uv| b2_cylinder_point(carrier, *uv))
                .collect()
        }
        ConsolidatedSupportBinding::EmbeddedCylinder { pos, .. } => {
            let carrier = &embedded.iter().find(|value| value.pos == *pos)?.cylinder;
            pcurve
                .points
                .iter()
                .map(|uv| b2_cylinder_point(carrier, *uv))
                .collect()
        }
        ConsolidatedSupportBinding::Cone { pos } => {
            let carrier = cones.iter().find(|value| value.pos == *pos)?;
            pcurve
                .points
                .iter()
                .map(|uv| b2_cone_point(carrier, *uv))
                .collect()
        }
        ConsolidatedSupportBinding::NurbsCarrier { pos, offset } => {
            let SurfaceGeometry::Nurbs(surface) = &surfaces
                .iter()
                .find(|surface| surface.pos == *pos)?
                .geometry
            else {
                return None;
            };
            pcurve
                .points
                .iter()
                .map(|&[u, v]| {
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
        let residual_length = (residual.x.powi(2) + residual.y.powi(2) + residual.z.powi(2)).sqrt();
        if residual_length < 1e-6 {
            offsets.push(0.0);
            continue;
        }
        let normal = partials.du.cross(partials.dv).unit()?;
        let distance = residual.x * normal.x + residual.y * normal.y + residual.z * normal.z;
        let perpendicular_squared = residual_length.powi(2) - distance.powi(2);
        if perpendicular_squared > 1e-12 {
            return None;
        }
        offsets.push(distance);
    }
    let first = offsets[0];
    if !first.is_finite() || offsets.iter().any(|value| (value - first).abs() > 1e-6) {
        return None;
    }
    Some(if first.abs() < 1e-6 { 0.0 } else { first })
}

fn pcurve_matches_circle(pcurve: &ConsolidatedPcurve, circle: &B2Circle) -> bool {
    let (Some(first), Some(last)) = (pcurve.points.first(), pcurve.points.last()) else {
        return false;
    };
    (first[1] - last[1]).abs() <= 1e-6
        && (first[0].min(last[0]) - circle.range[0]).abs() < 1e-9
        && (first[0].max(last[0]) - circle.range[1]).abs() < 1e-9
}

fn pcurve_endpoints_match_cone(
    pcurve: &ConsolidatedPcurve,
    cone: &B2Cone,
    vertices: &[Point3],
) -> bool {
    let (Some(first), Some(last)) = (pcurve.points.first(), pcurve.points.last()) else {
        return false;
    };
    [*first, *last].into_iter().all(|uv| {
        b2_cone_point(cone, uv).is_some_and(|point| {
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
        .points
        .first()
        .and_then(|uv| b2_cylinder_point(cylinder, *uv))
    else {
        return false;
    };
    let Some(last) = pcurve
        .points
        .last()
        .and_then(|uv| b2_cylinder_point(cylinder, *uv))
    else {
        return false;
    };
    [first, last].iter().all(|point| {
        vertices
            .iter()
            .any(|vertex| point_distance(*point, *vertex) < 2e-3)
    })
}

pub(crate) fn parse_consolidated_pcurve(
    data: &[u8],
    pos: usize,
    payload: usize,
    end: usize,
) -> Option<ConsolidatedPcurve> {
    let mut at = payload;
    let support_id = compact_int(data, &mut at)?;
    let degree = compact_int(data, &mut at)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    if degree != 5 || !(2..=4096).contains(&count) {
        return None;
    }
    let extrapolation_sites = match *data.get(at)? {
        0x0c => {
            at += 1;
            0
        }
        0x08 => {
            let encoded = *data.get(at + 1)?;
            if encoded % 4 != 1 {
                return None;
            }
            at += 2;
            u32::from((encoded - 1) / 4)
        }
        _ => return None,
    };
    let read = |at: &mut usize| -> Option<Vec<f64>> {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(f64_le(data, *at)?);
            *at += 8;
        }
        Some(values)
    };
    let knots = read(&mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
        return None;
    }
    at += 1;
    data.get(..at)?;
    let u = read(&mut at)?;
    let v = read(&mut at)?;
    let du = read(&mut at)?;
    let dv = read(&mut at)?;
    if data.get(at) != Some(&0x05) {
        return None;
    }
    at += 1;
    let ddu = read(&mut at)?;
    let ddv = read(&mut at)?;
    let range = [f64_le(data, at)?, f64_le(data, at + 8)?];
    at += 16;
    if at > end
        || !matches!(&data[at..end], [0x07] | [0x07, 0x00])
        || knots.windows(2).any(|v| v[0] >= v[1])
        || range[0] >= range[1]
        || knots
            .iter()
            .chain(&u)
            .chain(&v)
            .chain(&du)
            .chain(&dv)
            .chain(&ddu)
            .chain(&ddv)
            .chain(&range)
            .any(|x| !x.is_finite())
    {
        return None;
    }
    Some(ConsolidatedPcurve {
        pos,
        support_id,
        degree,
        extrapolation_sites,
        knots,
        points: u.into_iter().zip(v).map(|p| [p.0, p.1]).collect(),
        first_derivatives: du.into_iter().zip(dv).map(|p| [p.0, p.1]).collect(),
        second_derivatives: ddu.into_iter().zip(ddv).map(|p| [p.0, p.1]).collect(),
        range,
        tail: data[at..end].to_vec(),
    })
}

#[derive(Clone, Copy)]
pub(crate) struct ConsolidatedFrame {
    pub(crate) pos: usize,
    pub(crate) payload: usize,
    pub(crate) end: usize,
    pub(crate) header_token: u32,
}

/// Width-coded consolidated record family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsolidatedFamily {
    /// U32-length A family (`a5/a6/a7`).
    A,
    /// U8-length B family (`b2/b3/b4`).
    B,
}

/// One length-closed record in a consolidated A/B cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidatedRecord {
    /// Record family.
    pub family: ConsolidatedFamily,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent flag byte (`0x03`, `0x13`, or `0x83`).
    pub flag: u8,
    /// Record class byte.
    pub class: u8,
    /// Little-endian width-coded header token.
    pub header_token: u32,
    /// Complete record byte range.
    pub range: Range<usize>,
    /// Payload byte range.
    pub payload: Range<usize>,
}

/// Inventory length-closed consolidated A/B records while suppressing candidates
/// nested inside the payload of an already accepted frame.
#[must_use]
pub fn consolidated_records(data: &[u8]) -> Vec<ConsolidatedRecord> {
    let flags = [0x03, 0x13, 0x83];
    let mut candidates = Vec::new();
    for pos in 0..data.len().saturating_sub(4) {
        let (family, width, token_at, length) = if let Some(width) = data[pos]
            .checked_sub(0xa4)
            .filter(|width| (1..=3).contains(width))
        {
            let Some(length) = u32_le(data, pos + 3).and_then(|v| usize::try_from(v).ok()) else {
                continue;
            };
            (ConsolidatedFamily::A, width, pos + 7, length)
        } else if let Some(width) = data[pos]
            .checked_sub(0xb1)
            .filter(|width| (1..=3).contains(width))
        {
            (
                ConsolidatedFamily::B,
                width,
                pos + 4,
                usize::from(data[pos + 3]),
            )
        } else {
            continue;
        };
        let Some(&flag) = data.get(pos + 1) else {
            continue;
        };
        let Some(&class) = data.get(pos + 2) else {
            continue;
        };
        if !flags.contains(&flag) {
            continue;
        }
        let width_usize = usize::from(width);
        let Some(payload_start) = token_at.checked_add(width_usize) else {
            continue;
        };
        let Some(end) = payload_start.checked_add(length) else {
            continue;
        };
        if end > data.len() {
            continue;
        }
        let header_token = data[token_at..payload_start]
            .iter()
            .enumerate()
            .fold(0u32, |value, (shift, byte)| {
                value | (u32::from(*byte) << (8 * shift))
            });
        candidates.push(ConsolidatedRecord {
            family,
            width,
            flag,
            class,
            header_token,
            range: pos..end,
            payload: payload_start..end,
        });
    }
    let mut records: Vec<ConsolidatedRecord> = Vec::new();
    let mut active_payload: Option<Range<usize>> = None;
    for candidate in candidates {
        if active_payload
            .as_ref()
            .is_some_and(|payload| payload.contains(&candidate.range.start))
        {
            continue;
        }
        active_payload = Some(candidate.payload.clone());
        records.push(candidate);
    }
    records
}

/// Read `05 08 01` coordinate rows outside every length-closed consolidated
/// A/B or B5/A8 record. Marker-like bytes inside record payloads are not
/// vertices.
#[must_use]
pub(crate) fn object_stream_vertices(data: &[u8]) -> Vec<Point3> {
    let mut ranges = consolidated_records(data)
        .into_iter()
        .map(|record| record.range)
        .chain(crate::families::b5::graph::framed_ranges(data))
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return Vec::new();
    }
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    let mut vertices = Vec::new();
    let mut region_start = 0usize;
    for range in ranges {
        if range.end <= region_start {
            continue;
        }
        if range.start > region_start {
            vertices.extend(scan_vertex_records(&data[region_start..range.start]));
        }
        region_start = region_start.max(range.end);
    }
    vertices.extend(scan_vertex_records(&data[region_start..]));
    vertices
}

pub(crate) fn a_family_frames(data: &[u8], class: u8) -> Vec<ConsolidatedFrame> {
    consolidated_records(data)
        .into_iter()
        .filter(|record| record.family == ConsolidatedFamily::A && record.class == class)
        .map(|record| ConsolidatedFrame {
            pos: record.range.start,
            payload: record.payload.start,
            end: record.range.end,
            header_token: record.header_token,
        })
        .collect()
}

pub(crate) fn b_family_frames(data: &[u8], class: u8) -> Vec<ConsolidatedFrame> {
    consolidated_records(data)
        .into_iter()
        .filter(|record| record.family == ConsolidatedFamily::B && record.class == class)
        .map(|record| ConsolidatedFrame {
            pos: record.range.start,
            payload: record.payload.start,
            end: record.range.end,
            header_token: record.header_token,
        })
        .collect()
}
