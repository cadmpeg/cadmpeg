// SPDX-License-Identifier: Apache-2.0
//! Lower complete zero-entity endpoint relations into neutral B-rep topology.

use cadmpeg_core::decode::{u64_from_index, WorkBudget};

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::topology::{
    AnchoredVertexUse, Body, BodyKind, Coedge, Edge, Face, Loop, PcurveUse, Point, Region, Sense,
    Shell, Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::assemble::annotate;
use crate::families::FamilyEntityAdmission;
use crate::nurbs::canonical_model_curve_range;

use super::allocation::AllocationTopology;
use super::records::{ZeroEntityLoopClass, ZeroEntityOwnershipRoot, ZeroEntitySupportRun};
use cadmpeg_ir::geometry::SolvedCurveGeometry;

const MODEL_POINT_TOLERANCE: PositiveReal = match PositiveReal::new(2e-3) {
    Some(tolerance) => tolerance,
    None => panic!("zero-entity point tolerance must be positive and finite"),
};

/// Minimum allowance for validating source-bound endpoint contributors.
const MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS: usize = 1_000_000;
/// Comparison allowance per native support occurrence.
const MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS_PER_SUPPORT: usize = 256;

fn push_vertex_point(
    contributors: &mut Vec<FinitePoint3>,
    point: FinitePoint3,
    budget: &WorkBudget,
) -> Option<bool> {
    for existing in contributors.iter() {
        if !budget.charge() {
            return None;
        }
        if existing.get().distance(point.get()) > MODEL_POINT_TOLERANCE.get() {
            return Some(false);
        }
    }
    contributors.push(point);
    Some(true)
}

/// Counts one complete source-bound zero-entity topology transfer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::families::zero_entity) struct ZeroEntityTopologyCounts {
    pub(super) bodies: usize,
    pub(super) faces: usize,
    pub(super) loops: usize,
    pub(super) coedges: usize,
    pub(super) edges: usize,
    pub(super) vertices: usize,
    pub(super) points: usize,
    pub(super) pcurves: usize,
}

#[derive(Debug, Clone)]
struct Occurrence {
    support_record_ordinal: u32,
    support_pos: usize,
    face_record_ordinal: u32,
    forward: bool,
    raw_endpoints: [FinitePoint3; 2],
    oriented_endpoints: [FinitePoint3; 2],
    model_parameters: Option<[f64; 2]>,
    curve: CurveId,
    oriented_curve: Option<(CurveId, [f64; 2])>,
    pcurve: Option<OccurrencePcurve>,
}

#[derive(Debug, Clone)]
struct OccurrencePcurve {
    id: PcurveId,
    geometry: PcurveGeometry,
    parameter_range: [f64; 2],
}

/// The complete source allocation graph and solved carriers for one transfer.
/// Geometry validates source bindings before topology is emitted. The caller
/// retains the wire transfer when the complete graph cannot be admitted.
#[derive(Clone, Copy)]
pub(super) struct ZeroEntityClosedTopology<'a> {
    /// Complete physical-edge, side-use, and persistent vertex identities.
    pub(super) allocation_topology: &'a AllocationTopology,
    /// Support runs in record order.
    pub(super) support_runs: &'a [ZeroEntitySupportRun],
    /// Surface id emitted for each carrier position.
    pub(super) surface_ids_by_position: &'a HashMap<usize, SurfaceId>,
    /// Curve id emitted for each support.
    pub(super) support_curve_ids: &'a HashMap<u32, CurveId>,
    /// Ownership root ordering the face slots, when the source states one.
    pub(super) ownership_root: Option<&'a ZeroEntityOwnershipRoot>,
}

pub(super) fn transfer_closed_face_topology(
    admission: &mut FamilyEntityAdmission<'_, '_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    solved: ZeroEntityClosedTopology<'_>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<ZeroEntityTopologyCounts>, cadmpeg_core::CodecError> {
    (|| -> Option<Result<ZeroEntityTopologyCounts, cadmpeg_core::CodecError>> {
        macro_rules! admitted {
            ($value:expr) => {
                match $value {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                }
            };
        }
        macro_rules! copied_id {
            ($value:expr, $kind:ident) => {
                ($value).clone()
            };
        }
        let ZeroEntityClosedTopology {
            allocation_topology,
            support_runs,
            surface_ids_by_position,
            support_curve_ids,
            ownership_root,
        } = solved;
        if support_runs.is_empty() || support_runs.iter().any(|run| run.face.is_none()) {
            return None;
        }
        if let Some(ownership_root) = ownership_root {
            if ownership_root.face_slots.len() != support_runs.len()
                || !ownership_root
                    .face_slots
                    .iter()
                    .copied()
                    .eq((1..=u32::try_from(support_runs.len()).ok()?).rev())
            {
                return None;
            }
        }

        let mut occurrences = Vec::new();
        let mut occurrence_by_support = HashMap::<u32, usize>::new();
        let mut face_ids = Vec::new();

        let mut face_ordinals = HashSet::new();

        for run in support_runs {
            let face = run.face.as_ref()?;
            let surface_id = surface_ids_by_position.get(&run.carrier_pos)?;
            let surface_geometry = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *surface_id)
                .map(|surface| &surface.geometry)?;
            let face_id = admitted!(crate::resource::compose_u32_id(
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-face"),
                face.record_ordinal,
                FaceId::mint
            ));
            let inserted = face_ordinals.insert(face.record_ordinal);
            if !inserted {
                return None;
            }
            face_ids.push(face_id);

            let mut supports_by_ordinal = HashMap::new();
            for support in &run.supports {
                supports_by_ordinal.insert(support.record_ordinal, support);
            }
            for loop_record in face.loops.iter().flatten() {
                if loop_record.support_record_ordinals.len() != loop_record.forward_senses.len()
                    || loop_record.support_record_ordinals.len()
                        != loop_record.oriented_model_endpoints.len()
                    || loop_record.support_record_ordinals.is_empty()
                {
                    return None;
                }
                for (member_index, support_record_ordinal) in loop_record
                    .support_record_ordinals
                    .iter()
                    .copied()
                    .enumerate()
                {
                    let support = *supports_by_ordinal.get(&support_record_ordinal)?;
                    let curve =
                        copied_id!(support_curve_ids.get(&support_record_ordinal)?, CurveId);
                    if !ir
                        .model
                        .curves
                        .iter()
                        .any(|candidate| candidate.id == curve)
                    {
                        return None;
                    }
                    let raw_endpoints = support.model_endpoints?;
                    let oriented_endpoints = loop_record.oriented_model_endpoints[member_index];
                    let pcurve = match support.pcurve.as_ref() {
                        Some(pcurve) => {
                            let geometry = super::records::zero_entity_neutral_pcurve(
                                surface_geometry,
                                pcurve,
                                &format_args!(
                                    "zero-entity support record #{support_record_ordinal}"
                                ),
                                refusal,
                            )?;
                            let parameter_range = pcurve_parameter_range(&geometry)?;
                            Some(OccurrencePcurve {
                                id: admitted!(crate::resource::compose_u32_id(
                                    &cadmpeg_ir::identity_namespace!(
                                        "catia",
                                        "zero-entity",
                                        "topology-pcurve"
                                    ),
                                    support_record_ordinal,
                                    PcurveId::mint
                                )),
                                geometry,
                                parameter_range,
                            })
                        }
                        None => None,
                    };
                    let occurrence_index = occurrences.len();
                    let inserted =
                        occurrence_by_support.insert(support_record_ordinal, occurrence_index);
                    if inserted.is_some() {
                        return None;
                    }
                    let occurrence = Occurrence {
                        support_record_ordinal,
                        support_pos: support.pos,
                        face_record_ordinal: face.record_ordinal,
                        forward: loop_record.forward_senses[member_index],
                        raw_endpoints,
                        oriented_endpoints,
                        model_parameters: support
                            .model_parameters
                            .map(|parameters| parameters.map(FiniteReal::get)),
                        curve,
                        oriented_curve: None,
                        pcurve,
                    };
                    occurrences.push(occurrence);
                }
            }
        }

        for occurrence in &mut occurrences {
            let curve_geometry = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == occurrence.curve)
                .map(|curve| curve.geometry.clone())?;
            let source_range = occurrence
                .model_parameters
                .and_then(increasing_range)
                .or_else(|| {
                    occurrence
                        .pcurve
                        .as_ref()
                        .map(|pcurve| pcurve.parameter_range)
                });
            let source_range = match source_range {
                Some(range) => canonical_model_curve_range(
                    &curve_geometry,
                    range,
                    refusal,
                    "zero-entity edge curve source parameter range",
                ),
                None => None,
            };
            let raw_indices = if occurrence.forward { [0, 1] } else { [1, 0] };
            for (oriented, raw) in occurrence
                .oriented_endpoints
                .into_iter()
                .zip(raw_indices.map(|i| occurrence.raw_endpoints[i]))
            {
                if oriented.get().distance(raw.get()) > MODEL_POINT_TOLERANCE.get() {
                    return None;
                }
            }
            let direct_orientation = if matches!(
                &curve_geometry,
                cadmpeg_ir::geometry::CurveGeometry::Procedural { .. }
            ) {
                source_range.map(|range| (range, false))
            } else {
                match source_range {
                    Some(range) => match curve_orientation(
                        admission.context(),
                        &curve_geometry,
                        range,
                        occurrence.raw_endpoints.map(FinitePoint3::get),
                    ) {
                        Ok(orientation) => orientation.map(|reversed| (range, reversed)),
                        Err(error) => return Some(Err(error)),
                    },
                    None => None,
                }
            };
            let raw_is_oriented = raw_indices == [0, 1];
            let (oriented_curve, oriented_curve_parameter_range) =
                if let Some((parameter_range, reversed)) = direct_orientation {
                    let needs_reverse = reversed == raw_is_oriented;
                    if needs_reverse
                        && !matches!(
                            &curve_geometry,
                            cadmpeg_ir::geometry::CurveGeometry::Procedural { .. }
                        )
                    {
                        let reversed_geometry = crate::nurbs::reverse_curve_geometry(
                            &curve_geometry,
                            parameter_range,
                            refusal,
                            "zero-entity edge curve reversed onto its coedge",
                        );
                        let curve = ir
                            .model
                            .curves
                            .iter_mut()
                            .find(|curve| curve.id == occurrence.curve)?;
                        match reversed_geometry {
                            Some((geometry, parameter_range)) => {
                                let canonical_range = canonical_model_curve_range(
                                    &geometry,
                                    parameter_range,
                                    refusal,
                                    "zero-entity edge curve reversed parameter range",
                                );
                                if let Some(parameter_range) = canonical_range {
                                    curve.geometry = geometry;
                                    admitted!(annotations
                                        .derived(&occurrence.curve, "geometry")
                                        .map(|_| ())
                                        .map_err(cadmpeg_core::CodecError::from));
                                    (copied_id!(occurrence.curve, CurveId), parameter_range)
                                } else {
                                    curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
                                        SolvedCurveGeometry::Unknown { record: None },
                                    );
                                    admitted!(annotations
                                        .derived(&occurrence.curve, "geometry")
                                        .map(|_| ())
                                        .map_err(cadmpeg_core::CodecError::from));
                                    (copied_id!(occurrence.curve, CurveId), parameter_range)
                                }
                            }
                            None => {
                                curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
                                    SolvedCurveGeometry::Unknown { record: None },
                                );
                                admitted!(annotations
                                    .derived(&occurrence.curve, "geometry")
                                    .map(|_| ())
                                    .map_err(cadmpeg_core::CodecError::from));
                                (copied_id!(occurrence.curve, CurveId), parameter_range)
                            }
                        }
                    } else {
                        (copied_id!(occurrence.curve, CurveId), parameter_range)
                    }
                } else {
                    let parameter_range = source_range.or_else(|| {
                        occurrence
                            .pcurve
                            .as_ref()
                            .map(|pcurve| pcurve.parameter_range)
                    })?;
                    let curve = ir
                        .model
                        .curves
                        .iter_mut()
                        .find(|curve| curve.id == occurrence.curve)?;
                    if !matches!(
                        &curve.geometry,
                        cadmpeg_ir::geometry::CurveGeometry::Procedural { .. }
                    ) {
                        curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
                            SolvedCurveGeometry::Unknown { record: None },
                        );
                    }
                    admitted!(annotations
                        .derived(&occurrence.curve, "geometry")
                        .map(|_| ())
                        .map_err(cadmpeg_core::CodecError::from));
                    (copied_id!(occurrence.curve, CurveId), parameter_range)
                };
            occurrence.oriented_curve = Some((oriented_curve, oriented_curve_parameter_range));
        }

        let support_count = support_runs
            .iter()
            .map(|run| run.supports.len())
            .sum::<usize>();
        if support_count != occurrences.len() {
            return None;
        }

        let geometry_limit = support_count
            .checked_mul(MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS_PER_SUPPORT)
            .unwrap_or(support_count)
            .max(MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS);
        let geometry_budget = WorkBudget::new(geometry_limit);
        let allocation_edges = &allocation_topology.edges;
        if allocation_edges.len().checked_mul(2)? != occurrences.len() {
            return None;
        }
        let mut edge_for_support = HashMap::<u32, usize>::new();
        let mut vertex_points = HashMap::<u32, Vec<FinitePoint3>>::new();
        for (edge_index, edge) in allocation_edges.iter().enumerate() {
            let vertices = edge.uses.map(|side| side.vertex_record_ordinal);
            if vertices[0] == vertices[1] {
                return None;
            }
            for side in edge.uses {
                let occurrence =
                    &occurrences[*occurrence_by_support.get(&side.support_record_ordinal)?];
                if occurrence.face_record_ordinal != side.face_record_ordinal {
                    return None;
                }
                if edge_for_support
                    .insert(side.support_record_ordinal, edge_index)
                    .is_some()
                {
                    return None;
                }
                for (vertex, point) in vertices.into_iter().zip(occurrence.raw_endpoints) {
                    vertex_points.entry(vertex).or_default();
                    let contributors = vertex_points.get_mut(&vertex)?;
                    match push_vertex_point(contributors, point, &geometry_budget) {
                        Some(true) => {}
                        Some(false) => return None,
                        None => {
                            refusal.push_zero_entity_candidates_bounded(geometry_limit);
                            return None;
                        }
                    }
                }
            }
        }
        if edge_for_support.len() != occurrences.len()
            || vertex_points.len() != allocation_topology.vertices.len()
        {
            return None;
        }
        let mut vertex_index_by_ordinal = HashMap::new();
        let mut vertex_positions = Vec::new();
        for (index, vertex) in allocation_topology.vertices.iter().enumerate() {
            if vertex_index_by_ordinal
                .insert(vertex.record_ordinal, index)
                .is_some()
            {
                return None;
            }
            let point = *vertex_points.get(&vertex.record_ordinal)?.first()?;
            vertex_positions.push(point);
        }

        let first_face = support_runs.first()?.face.as_ref()?;
        let body_id = if let Some(root) = ownership_root {
            admitted!(crate::resource::compose_u32_id(
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-body"),
                admitted!(root.body_record_ordinal()),
                BodyId::mint
            ))
        } else {
            admitted!(BodyId::mint(format!(
                "catia:zero-entity:topology-body#inferred-{}-{}",
                first_face.record_ordinal,
                support_runs.len()
            ))
            .map_err(cadmpeg_core::CodecError::malformed))
        };
        let region_id = if let Some(root) = ownership_root {
            admitted!(crate::resource::compose_u32_id(
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-region"),
                admitted!(root.body_record_ordinal()),
                RegionId::mint
            ))
        } else {
            admitted!(RegionId::mint(format!(
                "catia:zero-entity:topology-region#inferred-{}-{}",
                first_face.record_ordinal,
                support_runs.len()
            ))
            .map_err(cadmpeg_core::CodecError::malformed))
        };
        let shell_id = if let Some(root) = ownership_root {
            admitted!(crate::resource::compose_u32_id(
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-shell"),
                admitted!(root.shell_record_ordinal()),
                ShellId::mint
            ))
        } else {
            admitted!(ShellId::mint(format!(
                "catia:zero-entity:topology-shell#inferred-{}-{}",
                first_face.record_ordinal,
                support_runs.len()
            ))
            .map_err(cadmpeg_core::CodecError::malformed))
        };

        let mut point_ids = Vec::new();
        let mut vertex_ids = Vec::new();
        for vertex in &allocation_topology.vertices {
            let point_id = admitted!(crate::resource::compose_u32_id(
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-point"),
                vertex.record_ordinal,
                PointId::mint
            ));
            let vertex_id = admitted!(crate::resource::compose_u32_id(
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-vertex"),
                vertex.record_ordinal,
                VertexId::mint
            ));
            point_ids.push(point_id);
            vertex_ids.push(vertex_id);
        }

        for (index, locus) in vertex_positions.iter().enumerate() {
            let vertex_pos = allocation_topology.vertices[index].pos;
            admitted!(annotate(
                annotations,
                &point_ids[index],
                "zero_entity_a9_03",
                u64_from_index(vertex_pos),
                "allocation_vertex_point",
                Exactness::Derived
            ));
            admitted!(annotations
                .derived(&point_ids[index], "position")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            ir.model.points.push(Point::new(
                copied_id!(point_ids[index], PointId),
                *locus,
                None,
            ));
            admitted!(annotate(
                annotations,
                &vertex_ids[index],
                "zero_entity_a9_03",
                u64_from_index(vertex_pos),
                "allocation_vertex",
                Exactness::Derived
            ));
            admitted!(annotations
                .derived(&vertex_ids[index], "point")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            let vertex = Vertex {
                native_ref: None,
                id: copied_id!(vertex_ids[index], VertexId),
                point: copied_id!(point_ids[index], PointId),
                tolerance: Some(MODEL_POINT_TOLERANCE),
            };
            ir.model.vertices.push(vertex);
        }

        for occurrence in &occurrences {
            let Some(pcurve) = &occurrence.pcurve else {
                continue;
            };
            admitted!(annotate(
                annotations,
                &pcurve.id,
                "zero_entity_a9_03",
                u64_from_index(occurrence.support_pos),
                "topology_pcurve",
                Exactness::Derived
            ));
            admitted!(annotations
                .derived(&pcurve.id, "geometry")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            let pcurve = Pcurve {
                native_ref: None,
                id: copied_id!(pcurve.id, PcurveId),
                geometry: pcurve.geometry.clone(),
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                    None,
                    Some(cadmpeg_ir::units::FiniteVector::new(
                        pcurve.parameter_range,
                    )?),
                    None,
                ),
            };
            ir.model.pcurves.push(pcurve);
        }

        let mut occurrence_vertex_pairs = Vec::new();
        for occurrence in &occurrences {
            let edge_index = *edge_for_support.get(&occurrence.support_record_ordinal)?;
            let edge = &allocation_edges[edge_index];
            let raw_indices = [
                *vertex_index_by_ordinal.get(&edge.uses[0].vertex_record_ordinal)?,
                *vertex_index_by_ordinal.get(&edge.uses[1].vertex_record_ordinal)?,
            ];
            let oriented_indices = if occurrence.forward {
                raw_indices
            } else {
                [raw_indices[1], raw_indices[0]]
            };
            let pair = [
                copied_id!(vertex_ids[oriented_indices[0]], VertexId),
                copied_id!(vertex_ids[oriented_indices[1]], VertexId),
            ];
            occurrence_vertex_pairs.push(pair);
        }

        let mut edge_ids = Vec::new();

        let mut coedges_by_support = HashMap::<u32, CoedgeId>::new();
        // Each allocation edge pushes exactly one edge id and the loop has no `continue`,
        // so `edge_ids[i]` is the edge of `allocation_edges[i]` by construction.
        for candidate in allocation_edges {
            let first_occurrence = &occurrences
                [*occurrence_by_support.get(&candidate.uses[0].support_record_ordinal)?];
            let edge_id = admitted!(crate::resource::compose_u32_id(
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-edge"),
                candidate.record_ordinal,
                EdgeId::mint
            ));
            let (oriented_curve, parameter_range) = first_occurrence.oriented_curve.as_ref()?;
            let param_range = Some(*parameter_range);
            let oriented_vertices = &occurrence_vertex_pairs
                [*occurrence_by_support.get(&candidate.uses[0].support_record_ordinal)?];
            admitted!(annotate(
                annotations,
                &edge_id,
                "zero_entity_a9_03",
                u64_from_index(candidate.pos),
                "topology_physical_edge",
                Exactness::Derived
            ));
            admitted!(annotations
                .derived(&edge_id, "curve")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            admitted!(annotations
                .derived(&edge_id, "start")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            admitted!(annotations
                .derived(&edge_id, "end")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            if param_range.is_some() {
                admitted!(annotations
                    .derived(&edge_id, "param_range")
                    .map(|_| ())
                    .map_err(cadmpeg_core::CodecError::from));
            }
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            let edge = Edge {
                native_ref: None,
                id: copied_id!(edge_id, EdgeId),
                carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                    Some(copied_id!(oriented_curve, CurveId)),
                    param_range,
                )
                .ok()?,
                start: copied_id!(oriented_vertices[0], VertexId),
                end: copied_id!(oriented_vertices[1], VertexId),
                tolerance: Some(MODEL_POINT_TOLERANCE),
            };
            ir.model.edges.push(edge);
            edge_ids.push(edge_id);
        }

        for (run_index, run) in support_runs.iter().enumerate() {
            let face = run.face.as_ref()?;
            let face_id = &face_ids[run_index];
            let mut loop_ids = Vec::new();
            for loop_record in face.loops.iter().flatten() {
                let id = admitted!(crate::resource::compose_u32_id(
                    &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-loop"),
                    loop_record.record_ordinal,
                    LoopId::mint
                ));
                loop_ids.push(id);
            }
            let outer_sense = match face
                .loops
                .as_ref()
                .and_then(|loops| loops.first())?
                .loop_class
            {
                ZeroEntityLoopClass::Outer41 => Sense::Forward,
                ZeroEntityLoopClass::ReversedC1 => Sense::Reversed,
                ZeroEntityLoopClass::Bound50 => return None,
            };
            admitted!(annotate(
                annotations,
                face_id,
                "zero_entity_a9_03",
                u64_from_index(face.pos),
                "topology_face",
                Exactness::Derived
            ));
            admitted!(annotations
                .derived(face_id, "shell")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            admitted!(annotations
                .derived(face_id, "surface")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            admitted!(annotations
                .derived(face_id, "sense")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            admitted!(annotations
                .derived(face_id, "loops")
                .map(|_| ())
                .map_err(cadmpeg_core::CodecError::from));
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            let face_record = Face {
                native_ref: None,
                id: copied_id!(face_id, FaceId),
                shell: copied_id!(shell_id, ShellId),
                surface: copied_id!(surface_ids_by_position[&run.carrier_pos], SurfaceId),
                sense: outer_sense,
                loops: match loop_ids.split_first() {
                    // The source states the outer boundary first.
                    Some((outer, inner)) => {
                        let inner = inner.to_vec();
                        cadmpeg_ir::topology::FaceLoops::classified(
                            copied_id!(outer, LoopId),
                            inner,
                        )
                    }
                    None => cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
                },
                name: None,
                color: None,
                tolerance: None,
            };
            ir.model.faces.push(face_record);

            for (loop_index, loop_record) in face.loops.iter().flatten().enumerate() {
                let loop_id = &loop_ids[loop_index];
                let mut coedge_ids = Vec::new();
                let mut vertex_uses = Vec::new();
                for (member_index, support_record_ordinal) in
                    loop_record.support_record_ordinals.iter().enumerate()
                {
                    let id = admitted!(crate::resource::compose_u32_id(
                        &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-coedge"),
                        allocation_edges[*edge_for_support.get(support_record_ordinal)?]
                            .uses
                            .iter()
                            .find(|side| side.support_record_ordinal == *support_record_ordinal)?
                            .use_record_ordinal,
                        CoedgeId::mint
                    ));
                    coedge_ids.push(id);
                    let occurrence_index = *occurrence_by_support.get(support_record_ordinal)?;
                    let vertex_use = AnchoredVertexUse {
                        vertex: copied_id!(occurrence_vertex_pairs[occurrence_index][1], VertexId),
                        after: copied_id!(coedge_ids[member_index], CoedgeId),
                        pcurves: Vec::new(),
                    };
                    vertex_uses.push(vertex_use);
                }
                admitted!(annotate(
                    annotations,
                    loop_id,
                    "zero_entity_a9_03",
                    u64_from_index(loop_record.pos),
                    "topology_loop",
                    Exactness::Derived
                ));
                admitted!(annotations
                    .derived(loop_id, "face")
                    .map(|_| ())
                    .map_err(cadmpeg_core::CodecError::from));
                admitted!(annotations
                    .derived(loop_id, "coedges")
                    .map(|_| ())
                    .map_err(cadmpeg_core::CodecError::from));
                admitted!(annotations
                    .derived(loop_id, "vertex_uses")
                    .map(|_| ())
                    .map_err(cadmpeg_core::CodecError::from));
                let ring =
                    cadmpeg_ir::topology::LoopRing::new(coedge_ids.clone(), vertex_uses).ok()?;
                if let Err(error) = admission.charge() {
                    return Some(Err(error));
                }
                let loop_entity = Loop {
                    native_ref: None,
                    id: copied_id!(loop_id, LoopId),
                    face: copied_id!(face_id, FaceId),
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
                };
                ir.model.loops.push(loop_entity);

                for (member_index, support_record_ordinal) in loop_record
                    .support_record_ordinals
                    .iter()
                    .copied()
                    .enumerate()
                {
                    let occurrence_index = *occurrence_by_support.get(&support_record_ordinal)?;
                    let occurrence = &occurrences[occurrence_index];
                    let edge_index = *edge_for_support.get(&support_record_ordinal)?;
                    let oriented_vertices = &occurrence_vertex_pairs[occurrence_index];
                    let edge = &allocation_edges[edge_index];
                    let first_occurrence = &occurrences
                        [*occurrence_by_support.get(&edge.uses[0].support_record_ordinal)?];
                    let first_oriented_vertices = &occurrence_vertex_pairs
                        [*occurrence_by_support.get(&edge.uses[0].support_record_ordinal)?];
                    let sense = if oriented_vertices == first_oriented_vertices {
                        Sense::Forward
                    } else if oriented_vertices[0] == first_oriented_vertices[1]
                        && oriented_vertices[1] == first_oriented_vertices[0]
                    {
                        Sense::Reversed
                    } else {
                        return None;
                    };
                    let (curve, parameter_range) = occurrence.oriented_curve.as_ref()?;
                    let (first_curve, _) = first_occurrence.oriented_curve.as_ref()?;
                    let use_curve = if curve == first_curve {
                        None
                    } else {
                        Some(cadmpeg_ir::topology::CoedgeUseCurve {
                            curve: copied_id!(curve, CurveId),
                            parameter_range: cadmpeg_ir::topology::ParameterInterval::new(
                                *parameter_range,
                            )
                            .ok()?,
                        })
                    };
                    let pcurve_use = if let Some(pcurve) = occurrence.pcurve.as_ref() {
                        let range = if occurrence.forward {
                            pcurve.parameter_range
                        } else {
                            [pcurve.parameter_range[1], pcurve.parameter_range[0]]
                        };
                        Some(PcurveUse {
                            pcurve: copied_id!(pcurve.id, PcurveId),
                            isoparametric: None,
                            parameter_range: Some(
                                cadmpeg_ir::geometry::DirectedParameterRange::new(range).ok()?,
                            ),
                        })
                    } else {
                        None
                    };
                    let mut pcurves = Vec::new();
                    if let Some(pcurve_use) = pcurve_use {
                        pcurves.push(pcurve_use);
                    }
                    let coedge_id = copied_id!(coedge_ids[member_index], CoedgeId);
                    admitted!(annotate(
                        annotations,
                        &coedge_id,
                        "zero_entity_a9_03",
                        u64_from_index(
                            edge.uses
                                .iter()
                                .find(|side| side.support_record_ordinal == support_record_ordinal)?
                                .use_pos
                        ),
                        "topology_coedge",
                        Exactness::Derived
                    ));
                    admitted!(annotations
                        .derived(&coedge_id, "owner_loop")
                        .map(|_| ())
                        .map_err(cadmpeg_core::CodecError::from));
                    admitted!(annotations
                        .derived(&coedge_id, "edge")
                        .map(|_| ())
                        .map_err(cadmpeg_core::CodecError::from));
                    admitted!(annotations
                        .derived(&coedge_id, "radial_next")
                        .map(|_| ())
                        .map_err(cadmpeg_core::CodecError::from));
                    admitted!(annotations
                        .derived(&coedge_id, "sense")
                        .map(|_| ())
                        .map_err(cadmpeg_core::CodecError::from));
                    admitted!(annotations
                        .derived(&coedge_id, "pcurves")
                        .map(|_| ())
                        .map_err(cadmpeg_core::CodecError::from));
                    if use_curve.is_some() {
                        admitted!(annotations
                            .derived(&coedge_id, "use_curve")
                            .map(|_| ())
                            .map_err(cadmpeg_core::CodecError::from));
                        admitted!(annotations
                            .derived(&coedge_id, "use_curve_parameter_range")
                            .map(|_| ())
                            .map_err(cadmpeg_core::CodecError::from));
                    }
                    if let Err(error) = admission.charge() {
                        return Some(Err(error));
                    }
                    let coedge = Coedge {
                        native_ref: None,
                        id: copied_id!(coedge_id, CoedgeId),
                        owner_loop: copied_id!(loop_id, LoopId),
                        edge: copied_id!(edge_ids[edge_index], EdgeId),
                        radial_next: copied_id!(coedge_id, CoedgeId),
                        sense,
                        pcurves,
                        use_curve,
                    };
                    ir.model.coedges.push(coedge);
                    coedges_by_support.insert(support_record_ordinal, coedge_id);
                }
            }
        }

        for candidate in allocation_edges {
            let first = coedges_by_support.get(&candidate.uses[0].support_record_ordinal)?;
            let second = coedges_by_support.get(&candidate.uses[1].support_record_ordinal)?;
            let second_copy = copied_id!(second, CoedgeId);
            ir.model
                .coedges
                .iter_mut()
                .find(|coedge| coedge.id == *first)?
                .radial_next = second_copy;
            let first_copy = copied_id!(first, CoedgeId);
            ir.model
                .coedges
                .iter_mut()
                .find(|coedge| coedge.id == *second)?
                .radial_next = first_copy;
        }

        admitted!(annotate(
            annotations,
            &body_id,
            "zero_entity_a9_03",
            u64_from_index(ownership_root.map_or(first_face.pos, |root| root.body_pos)),
            "topology_body",
            Exactness::Derived
        ));
        admitted!(annotations
            .derived(&body_id, "kind")
            .map(|_| ())
            .map_err(cadmpeg_core::CodecError::from));
        admitted!(annotations
            .derived(&body_id, "regions")
            .map(|_| ())
            .map_err(cadmpeg_core::CodecError::from));
        if let Err(error) = admission.charge() {
            return Some(Err(error));
        }
        let regions = vec![copied_id!(region_id, RegionId)];
        let body = Body {
            native_ref: None,
            id: copied_id!(body_id, BodyId),
            kind: BodyKind::Solid,
            regions,
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        ir.model.bodies.push(body);
        admitted!(annotate(
            annotations,
            &region_id,
            "zero_entity_a9_03",
            u64_from_index(ownership_root.map_or(first_face.pos, |root| root.shell_pos)),
            "topology_region",
            Exactness::Derived
        ));
        admitted!(annotations
            .derived(&region_id, "body")
            .map(|_| ())
            .map_err(cadmpeg_core::CodecError::from));
        admitted!(annotations
            .derived(&region_id, "shells")
            .map(|_| ())
            .map_err(cadmpeg_core::CodecError::from));
        if let Err(error) = admission.charge() {
            return Some(Err(error));
        }
        let shells = vec![copied_id!(shell_id, ShellId)];
        let region = Region {
            native_ref: None,
            id: copied_id!(region_id, RegionId),
            body: body_id,
            shells,
        };
        ir.model.regions.push(region);
        admitted!(annotate(
            annotations,
            &shell_id,
            "zero_entity_a9_03",
            u64_from_index(ownership_root.map_or(first_face.pos, |root| root.shell_pos)),
            "topology_shell",
            Exactness::Derived
        ));
        admitted!(annotations
            .derived(&shell_id, "region")
            .map(|_| ())
            .map_err(cadmpeg_core::CodecError::from));
        admitted!(annotations
            .derived(&shell_id, "faces")
            .map(|_| ())
            .map_err(cadmpeg_core::CodecError::from));
        if let Err(error) = admission.charge() {
            return Some(Err(error));
        }
        let shell = Shell::new(shell_id, region_id, face_ids, Vec::new(), Vec::new()).ok()?;
        ir.model.shells.push(shell);

        Some(Ok(ZeroEntityTopologyCounts {
            bodies: 1,
            faces: support_runs.len(),
            loops: support_runs
                .iter()
                .map(|run| {
                    run.face
                        .as_ref()
                        .map_or(0, |face| face.loops.as_ref().map_or(0, Vec::len))
                })
                .sum(),
            coedges: occurrences.len(),
            edges: allocation_edges.len(),
            vertices: vertex_positions.len(),
            points: vertex_positions.len(),
            pcurves: occurrences
                .iter()
                .filter(|occurrence| occurrence.pcurve.is_some())
                .count(),
        }))
    })()
    .transpose()
}

fn pcurve_parameter_range(pcurve: &PcurveGeometry) -> Option<[f64; 2]> {
    let PcurveGeometry::Nurbs { nurbs } = pcurve else {
        return None;
    };
    let degree = usize::try_from(nurbs.degree()).ok()?;
    let range = [
        *nurbs.knots().get(degree)?,
        *nurbs.knots().get(nurbs.pole_rows().count())?,
    ];
    (range[0] < range[1]).then_some(range)
}

fn curve_orientation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &cadmpeg_ir::geometry::CurveGeometry,
    parameter_range: [f64; 2],
    endpoints: [Point3; 2],
) -> Result<Option<bool>, cadmpeg_core::CodecError> {
    let Some(start) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, geometry, parameter_range[0]),
        )?)?
    else {
        return Ok(None);
    };
    let Some(end) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
        cadmpeg_ir::eval::decode::curve_point(ctx, geometry, parameter_range[1]),
    )?)?
    else {
        return Ok(None);
    };
    let evaluated = [start, end];
    let direct = evaluated[0].distance(endpoints[0]) <= MODEL_POINT_TOLERANCE.get()
        && evaluated[1].distance(endpoints[1]) <= MODEL_POINT_TOLERANCE.get();
    let reversed = evaluated[0].distance(endpoints[1]) <= MODEL_POINT_TOLERANCE.get()
        && evaluated[1].distance(endpoints[0]) <= MODEL_POINT_TOLERANCE.get();
    Ok(match (direct, reversed) {
        (true, false) => Some(false),
        (false, true) => Some(true),
        _ => None,
    })
}

fn increasing_range(parameters: [f64; 2]) -> Option<[f64; 2]> {
    if !parameters.into_iter().all(f64::is_finite) || parameters[0] == parameters[1] {
        return None;
    }
    Some([
        parameters[0].min(parameters[1]),
        parameters[0].max(parameters[1]),
    ])
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::WorkBudget;
    use std::collections::HashMap;

    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::math::Vector3;

    use super::super::records::ZeroEntityLoopClass;
    use super::super::records::ZeroEntityOwnershipRoot;
    use super::super::records::ZeroEntitySupportOccurrence;
    use super::super::records::ZeroEntitySupportRun;
    use super::{transfer_closed_face_topology, ZeroEntityClosedTopology};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::BodyKind;
    use cadmpeg_ir::AnnotationBuilder;

    fn support(ordinal: u32, start: Point3, end: Point3) -> ZeroEntitySupportOccurrence {
        ZeroEntitySupportOccurrence {
            pos: cadmpeg_core::decode::index_from_u32(ordinal),
            record_ordinal: ordinal,
            tag: [0x21, 0x71],
            face_local_slot: ordinal,
            uv_endpoints: None,
            pcurve: None,
            model_curve: Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    start,
                    end.vector_from(start)
                        .unit()
                        .expect("non-degenerate test edge"),
                )
                .expect("valid LineCurve fixture"),
            ))),
            model_curve_construction: None,
            model_parameters: Some(crate::test_support::test_b5::finite_pair([
                0.0,
                end.distance(start),
            ])),
            model_midpoint: Some(finite(Point3::new(
                (start.x + end.x) * 0.5,
                (start.y + end.y) * 0.5,
                (start.z + end.z) * 0.5,
            ))),
            model_endpoints: Some([start, end].map(finite)),
        }
    }

    fn finite(point: Point3) -> FinitePoint3 {
        FinitePoint3::new(point).expect("finite test point")
    }

    fn run(
        face_ordinal: u32,
        support_base: u32,
        points: [Point3; 3],
        reversed: bool,
    ) -> ZeroEntitySupportRun {
        let order = if reversed {
            [
                (points[1], points[0]),
                (points[2], points[1]),
                (points[0], points[2]),
            ]
        } else {
            [
                (points[0], points[1]),
                (points[1], points[2]),
                (points[2], points[0]),
            ]
        };
        let supports = order
            .into_iter()
            .enumerate()
            .map(|(index, (start, end))| {
                support(
                    support_base + u32::try_from(index).expect("small test index"),
                    if reversed { end } else { start },
                    if reversed { start } else { end },
                )
            })
            .collect::<Vec<_>>();
        let support_record_ordinals = supports
            .iter()
            .map(|support| support.record_ordinal)
            .collect::<Vec<_>>();
        ZeroEntitySupportRun {
            carrier_pos: 100,
            carrier_record_ordinal: face_ordinal,
            face: Some(super::super::records::ZeroEntityFace {
                pos: cadmpeg_core::decode::index_from_u32(face_ordinal),
                record_ordinal: face_ordinal,
                tag: [0x5f, 0x0c],
                allocations: vec![10, 3],
                loops: Some(vec![super::super::records::ZeroEntityLoop {
                    pos: cadmpeg_core::decode::index_from_u32(face_ordinal) + 1,
                    record_ordinal: face_ordinal + 100,
                    tag: [0x62, 0x14],
                    members: crate::families::zero_entity::records::ZeroEntityLoopMembers::try_new(
                        7,
                        1,
                        std::num::NonZeroUsize::new(3).expect("nonzero loop member count"),
                    )
                    .expect("admitted loop member run"),
                    edge_stride_distances: vec![1, 2, 3],
                    support_record_ordinals,

                    loop_class: if reversed {
                        ZeroEntityLoopClass::ReversedC1
                    } else {
                        ZeroEntityLoopClass::Outer41
                    },
                    forward_senses: vec![!reversed; 3],
                    oriented_model_endpoints: order
                        .into_iter()
                        .map(|(start, end)| [start, end].map(finite))
                        .collect(),
                }]),
                terminal_control: if reversed {
                    crate::families::zero_entity::records::ZeroEntityFaceControl::Control03
                } else {
                    crate::families::zero_entity::records::ZeroEntityFaceControl::Control05
                },
            }),
            supports,
        }
    }

    fn allocation(runs: &[ZeroEntitySupportRun]) -> super::AllocationTopology {
        use super::super::allocation::{AllocationEdge, AllocationTopology, AllocationUse};
        let Some(second) = runs.get(1) else {
            return AllocationTopology {
                edges: Vec::new(),
                vertices: Vec::new(),
            };
        };
        let edges = (0..3)
            .map(|index| {
                let vertices = [
                    70 + u32::try_from(index).expect("triangle edge index"),
                    70 + u32::try_from((index + 1) % 3).expect("triangle vertex index"),
                ];
                let supports = [
                    runs[0].supports[index].record_ordinal,
                    second.supports[index].record_ordinal,
                ];
                AllocationEdge {
                    pos: 300 + index,
                    record_ordinal: 30 + u32::try_from(index).expect("triangle edge index"),
                    header_record_ordinal: 40 + u32::try_from(index).expect("triangle edge index"),
                    uses: [0, 1].map(|side| AllocationUse {
                        use_pos: 500 + 2 * index + side,
                        use_record_ordinal: 50
                            + 2 * u32::try_from(index).expect("triangle edge index")
                            + u32::try_from(side).expect("two side uses"),
                        support_record_ordinal: supports[side],
                        face_record_ordinal: runs[side]
                            .face
                            .as_ref()
                            .expect("fixture face")
                            .record_ordinal,
                        incidence_record_ordinal: vertices[side] - 1,
                        vertex_record_ordinal: vertices[side],
                    }),
                }
            })
            .collect();
        AllocationTopology {
            edges,
            vertices: [70, 71, 72]
                .map(|ordinal| super::super::allocation::AllocationVertex {
                    record_ordinal: ordinal,
                    pos: cadmpeg_core::decode::index_from_u32(ordinal) * 10,
                })
                .to_vec(),
        }
    }

    #[test]
    fn endpoint_geometry_budget_skips_without_appending_a_partial_contributor() {
        let origin = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite origin");
        let distant = FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("finite point");
        let mut points = vec![origin];
        assert_eq!(
            super::push_vertex_point(&mut points, origin, &WorkBudget::new(0)),
            None
        );
        assert_eq!(points, vec![origin]);
        assert_eq!(
            super::push_vertex_point(&mut points, origin, &WorkBudget::new(1)),
            Some(true)
        );
        assert_eq!(
            super::push_vertex_point(&mut points, distant, &WorkBudget::new(2)),
            Some(false)
        );
        assert_eq!(points, vec![origin, origin]);
    }

    #[test]
    fn complete_radial_pairs_lower_to_connected_face_topology() {
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let runs = vec![run(10, 1, points, false), run(11, 4, points, true)];
        let curve_ids = runs
            .iter()
            .flat_map(|run| run.supports.iter())
            .map(|support| {
                (
                    support.record_ordinal,
                    CurveId::mint(format!("catia:test:curve#{}", support.record_ordinal))
                        .expect("identity grammar"),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            native_ref: None,
            id: SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    points[0],
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
        for run in &runs {
            for support in &run.supports {
                let [start, end] = support
                    .model_endpoints
                    .expect("test endpoints")
                    .map(FinitePoint3::get);
                ir.model.curves.push(Curve {
                    native_ref: None,
                    parameter_range: None,
                    id: curve_ids[&support.record_ordinal].clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                            start,
                            end.vector_from(start)
                                .unit()
                                .expect("non-degenerate test edge"),
                        )
                        .expect("valid LineCurve fixture"),
                    )),
                    source_object: None,
                });
            }
        }
        let mut contradictory = allocation(&runs);
        contradictory.edges[0].uses[0].vertex_record_ordinal = 72;
        let mut refused_ir = ir.clone();
        let refused = crate::test_support::with_service_context(|ctx| {
            transfer_closed_face_topology(
                &mut crate::families::FamilyEntityAdmission::new(ctx),
                &mut refused_ir,
                &mut AnnotationBuilder::new(),
                ZeroEntityClosedTopology {
                    allocation_topology: &contradictory,
                    support_runs: &runs,
                    surface_ids_by_position: &HashMap::from([(
                        100,
                        SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
                    )]),
                    support_curve_ids: &curve_ids,
                    ownership_root: None,
                },
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service entity admission");
        assert!(
            refused.is_none(),
            "source vertex binding contradicts its coordinates"
        );
        assert!(refused_ir.model.edges.is_empty());
        assert!(refused_ir.model.vertices.is_empty());
        let mut no_root_ir = ir.clone();
        let mut no_root_annotations = AnnotationBuilder::new();
        let no_root_counts = crate::test_support::with_service_context(|ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            transfer_closed_face_topology(
                &mut admission,
                &mut no_root_ir,
                &mut no_root_annotations,
                ZeroEntityClosedTopology {
                    allocation_topology: &allocation(&runs),
                    support_runs: &runs,
                    surface_ids_by_position: &HashMap::from([(
                        100,
                        SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
                    )]),
                    support_curve_ids: &curve_ids,
                    ownership_root: None,
                },
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("transfer the closed face topology")
        })
        .expect("complete topology without native ownership root");
        assert_eq!(no_root_counts.faces, 2);
        assert_eq!(
            no_root_ir
                .model
                .edges
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            [
                "catia:zero-entity:topology-edge#30",
                "catia:zero-entity:topology-edge#31",
                "catia:zero-entity:topology-edge#32"
            ]
        );
        assert_eq!(
            no_root_ir
                .model
                .vertices
                .iter()
                .map(|v| v.id.as_str())
                .collect::<Vec<_>>(),
            [
                "catia:zero-entity:topology-vertex#70",
                "catia:zero-entity:topology-vertex#71",
                "catia:zero-entity:topology-vertex#72"
            ]
        );
        assert!(no_root_ir
            .model
            .coedges
            .iter()
            .all(|c| (50..56)
                .any(|u| c.id.as_str() == format!("catia:zero-entity:topology-coedge#{u}"))));

        assert_eq!(no_root_ir.model.bodies[0].kind, BodyKind::Solid);
        assert!(crate::assemble::neutral_model_is_admissible(
            &cadmpeg_test_support::service_decode_context(),
            &mut no_root_ir,
            &[]
        )
        .expect("check that the neutral model is admissible"));
        let mut annotations = AnnotationBuilder::new();
        let root = ZeroEntityOwnershipRoot {
            face_roster_pos: 1,
            face_roster_record_ordinal: 20,
            face_slots: vec![2, 1],
            shell_pos: 2,
            body_pos: 3,
        };
        let counts = crate::test_support::with_service_context(|ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            transfer_closed_face_topology(
                &mut admission,
                &mut ir,
                &mut annotations,
                ZeroEntityClosedTopology {
                    allocation_topology: &allocation(&runs),
                    support_runs: &runs,
                    surface_ids_by_position: &HashMap::from([(
                        100,
                        SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
                    )]),
                    support_curve_ids: &curve_ids,
                    ownership_root: Some(&root),
                },
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("transfer the closed face topology")
        })
        .expect("complete topology");
        assert_eq!(counts.faces, 2);
        assert_eq!(counts.edges, 3);
        assert_eq!(counts.vertices, 3);
        assert_eq!(counts.coedges, 6);
        assert_eq!(ir.model.bodies[0].kind, BodyKind::Solid);
        assert_eq!(ir.model.shells[0].faces().len(), 2);
        assert!(ir.model.coedges.iter().all(|coedge| {
            ir.model
                .coedges
                .iter()
                .any(|candidate| candidate.id == coedge.radial_next)
        }));
        assert!(crate::assemble::neutral_model_is_admissible(
            &cadmpeg_test_support::service_decode_context(),
            &mut ir,
            &[]
        )
        .expect("check that the neutral model is admissible"));
    }

    #[test]
    fn unsupported_curve_reversal_keeps_topology_with_unknown_carrier() {
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let mut runs = vec![run(10, 1, points, false), run(11, 4, points, true)];
        runs[1].supports[1].model_parameters = Some(crate::test_support::test_b5::finite_pair([
            0.0,
            std::f64::consts::FRAC_PI_2,
        ]));
        let curve_ids = runs
            .iter()
            .flat_map(|run| run.supports.iter())
            .map(|support| {
                (
                    support.record_ordinal,
                    CurveId::mint(format!("catia:test:curve#{}", support.record_ordinal))
                        .expect("identity grammar"),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            native_ref: None,
            id: SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    points[0],
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
        for run in &runs {
            for support in &run.supports {
                let [start, end] = support
                    .model_endpoints
                    .expect("test endpoints")
                    .map(FinitePoint3::get);
                let geometry = if support.record_ordinal == 5 {
                    CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                            points[0],
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(1.0, 0.0, 0.0),
                            1.0,
                            1.0,
                        )
                        .expect("valid EllipseCurve fixture"),
                    ))
                } else {
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                            start,
                            end.vector_from(start)
                                .unit()
                                .expect("non-degenerate test edge"),
                        )
                        .expect("valid LineCurve fixture"),
                    ))
                };
                ir.model.curves.push(Curve {
                    native_ref: None,
                    parameter_range: None,
                    id: curve_ids[&support.record_ordinal].clone(),
                    geometry,
                    source_object: None,
                });
            }
        }
        let mut annotations = AnnotationBuilder::new();
        let counts = crate::test_support::with_service_context(|ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            transfer_closed_face_topology(
                &mut admission,
                &mut ir,
                &mut annotations,
                ZeroEntityClosedTopology {
                    allocation_topology: &allocation(&runs),
                    support_runs: &runs,
                    surface_ids_by_position: &HashMap::from([(
                        100,
                        SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
                    )]),
                    support_curve_ids: &curve_ids,
                    ownership_root: None,
                },
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("transfer the closed face topology")
        })
        .expect("topology remains transferable");

        assert_eq!(counts.edges, 3);
        assert!(matches!(
            ir.model
                .curves
                .iter()
                .find(|curve| curve.id == curve_ids[&5])
                .expect("reversed carrier")
                .geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
        ));
        assert!(crate::assemble::neutral_model_is_admissible(
            &cadmpeg_test_support::service_decode_context(),
            &mut ir,
            &[]
        )
        .expect("check that the neutral model is admissible"));
    }

    #[test]
    fn face_without_support_curve_ids_transfers_no_topology() {
        use cadmpeg_core::decode::DecodeArena;
        use cadmpeg_core::decode::DecodeContext;
        use cadmpeg_core::decode::DecodePolicy;
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let support_runs = vec![run(1, 10, points, false)];
        let surface_id = SurfaceId::mint("catia:test:surface#0").expect("identity grammar");
        let surface_ids = HashMap::from([(100, surface_id.clone())]);
        let curve_ids = HashMap::new();
        let run_with = |ctx: &DecodeContext<'_>| {
            let mut ir = CadIr::empty();
            ir.model.surfaces.push(Surface {
                native_ref: None,
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        points[0],
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid plane"),
                )),
                source_object: None,
            });
            let mut annotations = AnnotationBuilder::new();
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            transfer_closed_face_topology(
                &mut admission,
                &mut ir,
                &mut annotations,
                ZeroEntityClosedTopology {
                    allocation_topology: &allocation(&support_runs),
                    support_runs: &support_runs,
                    surface_ids_by_position: &surface_ids,
                    support_curve_ids: &curve_ids,
                    ownership_root: None,
                },
                &mut crate::nurbs::LaneRefusals::new(),
            )
        };
        let service_arena = DecodeArena::new();
        let service_policy = DecodePolicy::service();
        let (service_ctx, _) =
            DecodeContext::from_root_bytes(&[0], &service_arena, &service_policy)
                .expect("fixture fits the input limit");
        assert!(run_with(&service_ctx)
            .expect("transfer the zero-entity topology")
            .is_none());
    }

    #[test]
    fn missing_support_curve_ids_leave_the_face_untransferred() {
        use cadmpeg_core::decode::DecodeContext;
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let support_runs = vec![run(1, 10, points, false)];
        let surface_id = SurfaceId::mint("catia:test:surface#0").expect("identity grammar");
        let surface_ids = HashMap::from([(100, surface_id.clone())]);
        let curve_ids = HashMap::new();
        let run_with = |ctx: &DecodeContext<'_>| {
            let mut ir = CadIr::empty();
            ir.model.surfaces.push(Surface {
                native_ref: None,
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        points[0],
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid plane"),
                )),
                source_object: None,
            });
            transfer_closed_face_topology(
                &mut crate::families::FamilyEntityAdmission::new(ctx),
                &mut ir,
                &mut AnnotationBuilder::new(),
                ZeroEntityClosedTopology {
                    allocation_topology: &allocation(&support_runs),
                    support_runs: &support_runs,
                    surface_ids_by_position: &surface_ids,
                    support_curve_ids: &curve_ids,
                    ownership_root: None,
                },
                &mut crate::nurbs::LaneRefusals::new(),
            )
        };
        let service = crate::test_support::with_service_context(run_with);
        assert!(service
            .expect("transfer the zero-entity topology")
            .is_none());
    }
}

#[cfg(test)]
mod orientation_admission_tests {
    #[test]
    fn zero_entity_curve_orientation_propagates_caller_depth_refusal() {
        use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
        use cadmpeg_ir::math::{Point3, Vector3};
        let curve = CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0).unit().expect("unit direction"),
            )
            .expect("line"),
        ));
        crate::test_support::with_depth_limit(0, |ctx| {
            let error = super::curve_orientation(
                ctx,
                &curve,
                [0.0, 1.0],
                [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            )
            .expect_err("caller depth refuses curve evaluation");
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("resource refusal required")
            };
            assert_eq!(
                limit.dimension,
                cadmpeg_core::decode::ResourceDimension::RecursionDepth
            );
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }
}
