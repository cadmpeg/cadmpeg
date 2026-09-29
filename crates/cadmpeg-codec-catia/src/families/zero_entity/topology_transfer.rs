//! Lower complete zero-entity endpoint relations into neutral B-rep topology.

use cadmpeg_core::decode::u64_from_index;

use std::collections::HashMap;

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::curve_point;
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

use super::records::{ZeroEntityLoopClass, ZeroEntityOwnershipRoot, ZeroEntitySupportRun};
use super::topology::{
    endpoint_locus_candidates_with_budget, zero_entity_endpoint_pair_candidates_with_budget,
};
use cadmpeg_ir::geometry::SolvedCurveGeometry;

const MODEL_POINT_TOLERANCE: PositiveReal = match PositiveReal::new(2e-3) {
    Some(tolerance) => tolerance,
    None => panic!("zero-entity point tolerance must be positive and finite"),
};

/// Counts one complete geometry-derived zero-entity topology transfer.
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

/// Transfer a closed, geometry-resolved zero-entity B-rep subset.
///
/// The source allocation registries remain native. This route uses only the
/// settled geometric relations: a unique radial pair is one physical edge,
/// and a complete endpoint clique is one physical vertex. It therefore
/// refuses the whole candidate when any support occurrence or endpoint lacks
/// a unique relation. The caller keeps the existing wire transfer as the
/// atomic fallback.
/// The solved zero-entity carriers one closed-topology transfer reads.
///
/// The support runs, the surface ids their carrier positions were emitted
/// under, the curve ids their supports were emitted under, and the ownership
/// root that orders them are one solved pool: a run resolves through all
/// four or through none of them.
#[derive(Clone, Copy)]
pub(super) struct ZeroEntityClosedTopology<'a> {
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
    annotations: &mut AnnotationBuilder,
    solved: ZeroEntityClosedTopology<'_>,
    topology_budget: &WorkBudget<'_>,
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
                admitted!(($value)
                    .try_clone_for_decode(admission.context(), "catia_zero_topology_identity_copy"))
            };
        }
        let ZeroEntityClosedTopology {
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
        if let Err(error) = admission.context().reserve_vec(
            &mut face_ids,
            support_runs.len(),
            "catia_zero_topology_face_ids",
        ) {
            return Some(Err(error));
        }
        let mut face_id_by_ordinal = HashMap::<u32, FaceId>::new();

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
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-face"),
                face.record_ordinal,
                FaceId::mint,
                "catia_zero_topology_face_id"
            ));
            let inserted = match admission.context().insert_hash_map(
                &mut face_id_by_ordinal,
                face.record_ordinal,
                copied_id!(face_id, FaceId),
                "catia_zero_topology_face_ordinals",
            ) {
                Ok(inserted) => inserted,
                Err(error) => return Some(Err(error)),
            };
            if inserted.is_some() {
                return None;
            }
            face_ids.push(face_id);

            let mut supports_by_ordinal = HashMap::new();
            if let Err(error) = admission.context().reserve_map(
                &mut supports_by_ordinal,
                run.supports.len(),
                "catia_zero_topology_support_ordinals",
            ) {
                return Some(Err(error));
            }
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
                            let geometry = match super::records::zero_entity_neutral_pcurve(
                                admission.context(),
                                surface_geometry,
                                pcurve,
                                &format_args!(
                                    "zero-entity support record #{support_record_ordinal}"
                                ),
                                refusal,
                            ) {
                                Ok(Some(geometry)) => geometry,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                            let parameter_range = pcurve_parameter_range(&geometry)?;
                            Some(OccurrencePcurve {
                                id: admitted!(crate::resource::compose_u32_id(
                                    admission.context(),
                                    &cadmpeg_ir::identity_namespace!(
                                        "catia",
                                        "zero-entity",
                                        "topology-pcurve"
                                    ),
                                    support_record_ordinal,
                                    PcurveId::mint,
                                    "catia_zero_topology_pcurve_id"
                                )),
                                geometry,
                                parameter_range,
                            })
                        }
                        None => None,
                    };
                    let occurrence_index = occurrences.len();
                    let inserted = match admission.context().insert_hash_map(
                        &mut occurrence_by_support,
                        support_record_ordinal,
                        occurrence_index,
                        "catia_zero_topology_occurrence_ordinals",
                    ) {
                        Ok(inserted) => inserted,
                        Err(error) => return Some(Err(error)),
                    };
                    if inserted.is_some() {
                        return None;
                    }
                    let occurrence = Occurrence {
                        support_record_ordinal,
                        raw_endpoints,
                        oriented_endpoints,
                        model_parameters: support
                            .model_parameters
                            .map(|parameters| parameters.map(FiniteReal::get)),
                        curve,
                        oriented_curve: None,
                        pcurve,
                    };
                    if let Err(error) = admission.context().push_vec(
                        &mut occurrences,
                        occurrence,
                        "catia_zero_topology_occurrences",
                    ) {
                        return Some(Err(error));
                    }
                }
            }
        }

        for occurrence in &mut occurrences {
            let curve_geometry = admitted!(ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == occurrence.curve)
                .map(|curve| super::decode::copy_zero_curve(admission.context(), &curve.geometry))
                .transpose())?;
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
                Some(range) => match canonical_model_curve_range(
                    admission.context(),
                    &curve_geometry,
                    range,
                    refusal,
                    "zero-entity edge curve source parameter range",
                ) {
                    Ok(range) => range,
                    Err(error) => return Some(Err(error)),
                },
                None => None,
            };
            let raw_indices =
                endpoint_indices(occurrence.oriented_endpoints, occurrence.raw_endpoints)?;
            let direct_orientation = if matches!(
                &curve_geometry,
                cadmpeg_ir::geometry::CurveGeometry::Procedural { .. }
            ) {
                source_range.map(|range| (range, false))
            } else {
                match source_range {
                    Some(range) => match curve_orientation(
                        &curve_geometry,
                        range,
                        occurrence.raw_endpoints.map(FinitePoint3::get),
                    ) {
                        Ok(orientation) => orientation.map(|reversed| (range, reversed)),
                        Err(limit) => return Some(Err(limit.into())),
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
                            admission.context(),
                            &curve_geometry,
                            parameter_range,
                            refusal,
                            "zero-entity edge curve reversed onto its coedge",
                        );
                        let reversed_geometry = match reversed_geometry {
                            Ok(geometry) => geometry,
                            Err(error) => return Some(Err(error)),
                        };
                        let curve = ir
                            .model
                            .curves
                            .iter_mut()
                            .find(|curve| curve.id == occurrence.curve)?;
                        match reversed_geometry {
                            Some((geometry, parameter_range)) => {
                                let canonical_range = canonical_model_curve_range(
                                    admission.context(),
                                    &geometry,
                                    parameter_range,
                                    refusal,
                                    "zero-entity edge curve reversed parameter range",
                                );
                                let canonical_range = match canonical_range {
                                    Ok(range) => range,
                                    Err(error) => return Some(Err(error)),
                                };
                                if let Some(parameter_range) = canonical_range {
                                    curve.geometry = geometry;
                                    admitted!(crate::resource::derived_annotation(
                                        admission.context(),
                                        annotations,
                                        &occurrence.curve,
                                        "geometry",
                                        "catia_annotation_field"
                                    ));
                                    (copied_id!(occurrence.curve, CurveId), parameter_range)
                                } else {
                                    curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
                                        SolvedCurveGeometry::Unknown { record: None },
                                    );
                                    admitted!(crate::resource::derived_annotation(
                                        admission.context(),
                                        annotations,
                                        &occurrence.curve,
                                        "geometry",
                                        "catia_annotation_field"
                                    ));
                                    (copied_id!(occurrence.curve, CurveId), parameter_range)
                                }
                            }
                            None => {
                                curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
                                    SolvedCurveGeometry::Unknown { record: None },
                                );
                                admitted!(crate::resource::derived_annotation(
                                    admission.context(),
                                    annotations,
                                    &occurrence.curve,
                                    "geometry",
                                    "catia_annotation_field"
                                ));
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
                    admitted!(crate::resource::derived_annotation(
                        admission.context(),
                        annotations,
                        &occurrence.curve,
                        "geometry",
                        "catia_annotation_field"
                    ));
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

        let edge_candidates = match zero_entity_endpoint_pair_candidates_with_budget(
            admission.context(),
            support_runs,
            topology_budget,
        ) {
            Ok(Some(candidates)) => candidates,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if edge_candidates.len().checked_mul(2)? != occurrences.len() {
            return None;
        }
        let mut edge_for_support = HashMap::<u32, usize>::new();
        for (edge_index, candidate) in edge_candidates.iter().enumerate() {
            for support_record_ordinal in candidate.support_record_ordinals {
                if !occurrence_by_support.contains_key(&support_record_ordinal) {
                    return None;
                }
                let inserted = match admission.context().insert_hash_map(
                    &mut edge_for_support,
                    support_record_ordinal,
                    edge_index,
                    "catia_zero_topology_edge_for_support",
                ) {
                    Ok(inserted) => inserted,
                    Err(error) => return Some(Err(error)),
                };
                if inserted.is_some() {
                    return None;
                }
            }
        }
        if edge_for_support.len() != occurrences.len() {
            return None;
        }

        let endpoint_loci = match endpoint_locus_candidates_with_budget(
            admission.context(),
            &edge_candidates,
            topology_budget,
        ) {
            Ok(Some(candidates)) => candidates,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let mut vertex_for_endpoint = HashMap::<(usize, usize), usize>::new();
        for (vertex_index, locus) in endpoint_loci.iter().enumerate() {
            for &(edge_index, endpoint_index) in &locus.incident_endpoint_pair_endpoints {
                let edge_index = edge_index.ordinal();
                let endpoint_index = usize::from(u8::from(endpoint_index));
                if edge_index >= edge_candidates.len() {
                    return None;
                }
                let inserted = match admission.context().insert_hash_map(
                    &mut vertex_for_endpoint,
                    (edge_index, endpoint_index),
                    vertex_index,
                    "catia_zero_topology_vertex_for_endpoint",
                ) {
                    Ok(inserted) => inserted,
                    Err(error) => return Some(Err(error)),
                };
                if inserted.is_some() {
                    return None;
                }
            }
        }
        if vertex_for_endpoint.len() != edge_candidates.len().checked_mul(2)? {
            return None;
        }

        let first_face = support_runs.first()?.face.as_ref()?;
        let body_id = if let Some(root) = ownership_root {
            admitted!(crate::resource::compose_u32_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-body"),
                admitted!(root.body_record_ordinal()),
                BodyId::mint,
                "catia_zero_topology_body_id"
            ))
        } else {
            admitted!(BodyId::mint(admitted!(admission.context().format_retained(
                format_args!(
                    "catia:zero-entity:topology-body#inferred-{}-{}",
                    first_face.record_ordinal,
                    support_runs.len()
                ),
                "catia_zero_topology_body_id"
            )))
            .map_err(cadmpeg_core::CodecError::malformed))
        };
        let region_id = if let Some(root) = ownership_root {
            admitted!(crate::resource::compose_u32_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-region"),
                admitted!(root.body_record_ordinal()),
                RegionId::mint,
                "catia_zero_topology_region_id"
            ))
        } else {
            admitted!(
                RegionId::mint(admitted!(admission.context().format_retained(
                    format_args!(
                        "catia:zero-entity:topology-region#inferred-{}-{}",
                        first_face.record_ordinal,
                        support_runs.len()
                    ),
                    "catia_zero_topology_region_id"
                )))
                .map_err(cadmpeg_core::CodecError::malformed)
            )
        };
        let shell_id = if let Some(root) = ownership_root {
            admitted!(crate::resource::compose_u32_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-shell"),
                admitted!(root.shell_record_ordinal()),
                ShellId::mint,
                "catia_zero_topology_shell_id"
            ))
        } else {
            admitted!(ShellId::mint(admitted!(admission.context().format_retained(
                format_args!(
                    "catia:zero-entity:topology-shell#inferred-{}-{}",
                    first_face.record_ordinal,
                    support_runs.len()
                ),
                "catia_zero_topology_shell_id"
            )))
            .map_err(cadmpeg_core::CodecError::malformed))
        };

        let mut point_ids = Vec::new();
        let mut vertex_ids = Vec::new();
        for index in 0..endpoint_loci.len() {
            let point_id = admitted!(crate::resource::compose_index_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-point"),
                index,
                PointId::mint,
                "catia_zero_topology_point_id"
            ));
            let vertex_id = admitted!(crate::resource::compose_index_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-vertex"),
                index,
                VertexId::mint,
                "catia_zero_topology_vertex_id"
            ));
            if let Err(error) = admission.context().push_vec(
                &mut point_ids,
                point_id,
                "catia_zero_topology_point_ids",
            ) {
                return Some(Err(error));
            }
            if let Err(error) = admission.context().push_vec(
                &mut vertex_ids,
                vertex_id,
                "catia_zero_topology_vertex_ids",
            ) {
                return Some(Err(error));
            }
        }

        for (index, locus) in endpoint_loci.iter().enumerate() {
            admitted!(annotate(
                admission.context(),
                annotations,
                &point_ids[index],
                "zero_entity_a9_03",
                u64_from_index(ownership_root.map_or(first_face.pos, |root| root.face_roster_pos)),
                "endpoint_locus_point",
                Exactness::Inferred
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                &point_ids[index],
                "position",
                "catia_annotation_field"
            ));
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            if let Err(error) = admission.context().push_vec(
                &mut ir.model.points,
                Point::new(
                    copied_id!(point_ids[index], PointId),
                    locus.representative_point,
                    None,
                ),
                "catia_zero_topology_points",
            ) {
                return Some(Err(error));
            }
            admitted!(annotate(
                admission.context(),
                annotations,
                &vertex_ids[index],
                "zero_entity_a9_03",
                u64_from_index(ownership_root.map_or(first_face.pos, |root| root.face_roster_pos)),
                "endpoint_locus_vertex",
                Exactness::Inferred
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                &vertex_ids[index],
                "point",
                "catia_annotation_field"
            ));
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            let vertex = Vertex {
                id: copied_id!(vertex_ids[index], VertexId),
                point: copied_id!(point_ids[index], PointId),
                tolerance: Some(MODEL_POINT_TOLERANCE),
            };
            if let Err(error) = admission.context().push_vec(
                &mut ir.model.vertices,
                vertex,
                "catia_zero_topology_vertices",
            ) {
                return Some(Err(error));
            }
        }

        for occurrence in &occurrences {
            let Some(pcurve) = &occurrence.pcurve else {
                continue;
            };
            admitted!(annotate(
                admission.context(),
                annotations,
                &pcurve.id,
                "zero_entity_a9_03",
                u64::from(occurrence.support_record_ordinal),
                "topology_pcurve",
                Exactness::Derived
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                &pcurve.id,
                "geometry",
                "catia_annotation_field"
            ));
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            let pcurve = Pcurve {
                id: copied_id!(pcurve.id, PcurveId),
                geometry: admitted!(pcurve
                    .geometry
                    .try_clone_for_decode(admission.context(), "catia_zero_topology_pcurve_copy")),
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                    None,
                    Some(cadmpeg_ir::units::FiniteVector::new(
                        pcurve.parameter_range,
                    )?),
                    None,
                ),
            };
            if let Err(error) = admission.context().push_vec(
                &mut ir.model.pcurves,
                pcurve,
                "catia_zero_topology_pcurves",
            ) {
                return Some(Err(error));
            }
        }

        let mut occurrence_vertex_pairs = Vec::new();
        for occurrence in &occurrences {
            let edge_index = *edge_for_support.get(&occurrence.support_record_ordinal)?;
            let candidate = &edge_candidates[edge_index];
            let oriented_indices =
                endpoint_indices(candidate.model_endpoints, occurrence.oriented_endpoints)?;
            let raw_indices =
                endpoint_indices(occurrence.oriented_endpoints, occurrence.raw_endpoints)?;
            let pair = (
                [
                    copied_id!(
                        vertex_ids[vertex_for_endpoint[&(edge_index, oriented_indices[0])]],
                        VertexId
                    ),
                    copied_id!(
                        vertex_ids[vertex_for_endpoint[&(edge_index, oriented_indices[1])]],
                        VertexId
                    ),
                ],
                [
                    copied_id!(
                        vertex_ids
                            [vertex_for_endpoint[&(edge_index, oriented_indices[raw_indices[0]])]],
                        VertexId
                    ),
                    copied_id!(
                        vertex_ids
                            [vertex_for_endpoint[&(edge_index, oriented_indices[raw_indices[1]])]],
                        VertexId
                    ),
                ],
                raw_indices == [0, 1],
            );
            if let Err(error) = admission.context().push_vec(
                &mut occurrence_vertex_pairs,
                pair,
                "catia_zero_topology_occurrence_vertices",
            ) {
                return Some(Err(error));
            }
        }

        let mut edge_ids = Vec::new();
        if let Err(error) = admission.context().reserve_vec(
            &mut edge_ids,
            edge_candidates.len(),
            "catia_zero_topology_edge_ids",
        ) {
            return Some(Err(error));
        }
        let mut coedges_by_support = HashMap::<u32, CoedgeId>::new();
        // Each candidate pushes exactly one edge id and the loop has no `continue`,
        // so `edge_ids[i]` is the edge of `edge_candidates[i]` by construction.
        for candidate in &edge_candidates {
            let first_occurrence =
                &occurrences[*occurrence_by_support.get(&candidate.support_record_ordinals[0])?];
            let edge_id = admitted!(EdgeId::mint(admitted!(admission.context().format_retained(
                format_args!(
                    "catia:zero-entity:topology-edge#{}-{}",
                    candidate.support_record_ordinals[0], candidate.support_record_ordinals[1]
                ),
                "catia_zero_topology_edge_id"
            )))
            .map_err(cadmpeg_core::CodecError::malformed));
            let (oriented_curve, parameter_range) = first_occurrence.oriented_curve.as_ref()?;
            let param_range = Some(*parameter_range);
            let oriented_vertices = &occurrence_vertex_pairs
                [*occurrence_by_support.get(&candidate.support_record_ordinals[0])?]
            .0;
            admitted!(annotate(
                admission.context(),
                annotations,
                &edge_id,
                "zero_entity_a9_03",
                u64::from(first_occurrence.support_record_ordinal),
                "topology_physical_edge_candidate",
                Exactness::Inferred
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                &edge_id,
                "curve",
                "catia_annotation_field"
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                &edge_id,
                "start",
                "catia_annotation_field"
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                &edge_id,
                "end",
                "catia_annotation_field"
            ));
            if param_range.is_some() {
                admitted!(crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    &edge_id,
                    "param_range",
                    "catia_annotation_field"
                ));
            }
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            let edge = Edge {
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
            if let Err(error) =
                admission
                    .context()
                    .push_vec(&mut ir.model.edges, edge, "catia_zero_topology_edges")
            {
                return Some(Err(error));
            }
            edge_ids.push(edge_id);
        }

        for (run_index, run) in support_runs.iter().enumerate() {
            let face = run.face.as_ref()?;
            let face_id = &face_ids[run_index];
            let mut loop_ids = Vec::new();
            for loop_record in face.loops.iter().flatten() {
                let id = admitted!(crate::resource::compose_u32_id(
                    admission.context(),
                    &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-loop"),
                    loop_record.record_ordinal,
                    LoopId::mint,
                    "catia_zero_topology_loop_id"
                ));
                if let Err(error) =
                    admission
                        .context()
                        .push_vec(&mut loop_ids, id, "catia_zero_topology_loop_ids")
                {
                    return Some(Err(error));
                }
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
                admission.context(),
                annotations,
                face_id,
                "zero_entity_a9_03",
                u64::from(face.record_ordinal),
                "topology_face",
                Exactness::Inferred
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                face_id,
                "shell",
                "catia_annotation_field"
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                face_id,
                "surface",
                "catia_annotation_field"
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                face_id,
                "sense",
                "catia_annotation_field"
            ));
            admitted!(crate::resource::derived_annotation(
                admission.context(),
                annotations,
                face_id,
                "loops",
                "catia_annotation_field"
            ));
            if let Err(error) = admission.charge() {
                return Some(Err(error));
            }
            let face_record = Face {
                id: copied_id!(face_id, FaceId),
                shell: copied_id!(shell_id, ShellId),
                surface: copied_id!(surface_ids_by_position[&run.carrier_pos], SurfaceId),
                sense: outer_sense,
                loops: match loop_ids.split_first() {
                    // The source states the outer boundary first.
                    Some((outer, inner)) => {
                        let inner = admitted!(admission.context().try_collect_vec(
                            inner.iter().map(|id| id.try_clone_for_decode(
                                admission.context(),
                                "catia_zero_topology_inner_loop_id"
                            )),
                            "catia_zero_topology_inner_loops"
                        ));
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
            if let Err(error) = admission.context().push_vec(
                &mut ir.model.faces,
                face_record,
                "catia_zero_topology_faces",
            ) {
                return Some(Err(error));
            }

            for (loop_index, loop_record) in face.loops.iter().flatten().enumerate() {
                let loop_id = &loop_ids[loop_index];
                let mut coedge_ids = Vec::new();
                let mut vertex_uses = Vec::new();
                for (member_index, support_record_ordinal) in
                    loop_record.support_record_ordinals.iter().enumerate()
                {
                    let id = admitted!(crate::resource::compose_u32_id(
                        admission.context(),
                        &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-coedge"),
                        *support_record_ordinal,
                        CoedgeId::mint,
                        "catia_zero_topology_coedge_id"
                    ));
                    if let Err(error) = admission.context().push_vec(
                        &mut coedge_ids,
                        id,
                        "catia_zero_topology_coedge_ids",
                    ) {
                        return Some(Err(error));
                    }
                    let occurrence_index = *occurrence_by_support.get(support_record_ordinal)?;
                    let vertex_use = AnchoredVertexUse {
                        vertex: copied_id!(
                            occurrence_vertex_pairs[occurrence_index].0[1],
                            VertexId
                        ),
                        after: copied_id!(coedge_ids[member_index], CoedgeId),
                        pcurves: Vec::new(),
                    };
                    if let Err(error) = admission.context().push_vec(
                        &mut vertex_uses,
                        vertex_use,
                        "catia_zero_topology_vertex_uses",
                    ) {
                        return Some(Err(error));
                    }
                }
                admitted!(annotate(
                    admission.context(),
                    annotations,
                    loop_id,
                    "zero_entity_a9_03",
                    u64::from(loop_record.record_ordinal),
                    "topology_loop",
                    Exactness::Inferred
                ));
                admitted!(crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    loop_id,
                    "face",
                    "catia_annotation_field"
                ));
                admitted!(crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    loop_id,
                    "coedges",
                    "catia_annotation_field"
                ));
                admitted!(crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    loop_id,
                    "vertex_uses",
                    "catia_annotation_field"
                ));
                let ring = cadmpeg_ir::topology::LoopRing::new(
                    admitted!(admission.context().try_collect_vec(
                        coedge_ids.iter().map(|id| id.try_clone_for_decode(
                            admission.context(),
                            "catia_zero_topology_ring_coedge_id"
                        )),
                        "catia_zero_topology_ring_coedges"
                    )),
                    vertex_uses,
                )
                .ok()?;
                if let Err(error) = admission.charge() {
                    return Some(Err(error));
                }
                let loop_entity = Loop {
                    id: copied_id!(loop_id, LoopId),
                    face: copied_id!(face_id, FaceId),
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
                };
                if let Err(error) = admission.context().push_vec(
                    &mut ir.model.loops,
                    loop_entity,
                    "catia_zero_topology_loops",
                ) {
                    return Some(Err(error));
                }

                for (member_index, support_record_ordinal) in loop_record
                    .support_record_ordinals
                    .iter()
                    .copied()
                    .enumerate()
                {
                    let occurrence_index = *occurrence_by_support.get(&support_record_ordinal)?;
                    let occurrence = &occurrences[occurrence_index];
                    let edge_index = *edge_for_support.get(&support_record_ordinal)?;
                    let oriented_vertices = &occurrence_vertex_pairs[occurrence_index].0;
                    let edge = &edge_candidates[edge_index];
                    let first_occurrence = &occurrences
                        [*occurrence_by_support.get(&edge.support_record_ordinals[0])?];
                    let first_oriented_vertices = &occurrence_vertex_pairs
                        [*occurrence_by_support.get(&edge.support_record_ordinals[0])?]
                    .0;
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
                        let range = if occurrence_vertex_pairs[occurrence_index].2 {
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
                        if let Err(error) = admission.context().push_vec(
                            &mut pcurves,
                            pcurve_use,
                            "catia_zero_topology_coedge_pcurves",
                        ) {
                            return Some(Err(error));
                        }
                    }
                    let coedge_id = copied_id!(coedge_ids[member_index], CoedgeId);
                    admitted!(annotate(
                        admission.context(),
                        annotations,
                        &coedge_id,
                        "zero_entity_a9_03",
                        u64::from(occurrence.support_record_ordinal),
                        "topology_coedge",
                        Exactness::Inferred
                    ));
                    admitted!(crate::resource::derived_annotation(
                        admission.context(),
                        annotations,
                        &coedge_id,
                        "owner_loop",
                        "catia_annotation_field"
                    ));
                    admitted!(crate::resource::derived_annotation(
                        admission.context(),
                        annotations,
                        &coedge_id,
                        "edge",
                        "catia_annotation_field"
                    ));
                    admitted!(crate::resource::derived_annotation(
                        admission.context(),
                        annotations,
                        &coedge_id,
                        "radial_next",
                        "catia_annotation_field"
                    ));
                    admitted!(crate::resource::derived_annotation(
                        admission.context(),
                        annotations,
                        &coedge_id,
                        "sense",
                        "catia_annotation_field"
                    ));
                    admitted!(crate::resource::derived_annotation(
                        admission.context(),
                        annotations,
                        &coedge_id,
                        "pcurves",
                        "catia_annotation_field"
                    ));
                    if use_curve.is_some() {
                        admitted!(crate::resource::derived_annotation(
                            admission.context(),
                            annotations,
                            &coedge_id,
                            "use_curve",
                            "catia_annotation_field"
                        ));
                        admitted!(crate::resource::derived_annotation(
                            admission.context(),
                            annotations,
                            &coedge_id,
                            "use_curve_parameter_range",
                            "catia_annotation_field"
                        ));
                    }
                    if let Err(error) = admission.charge() {
                        return Some(Err(error));
                    }
                    let coedge = Coedge {
                        id: copied_id!(coedge_id, CoedgeId),
                        owner_loop: copied_id!(loop_id, LoopId),
                        edge: copied_id!(edge_ids[edge_index], EdgeId),
                        radial_next: copied_id!(coedge_id, CoedgeId),
                        sense,
                        pcurves,
                        use_curve,
                    };
                    if let Err(error) = admission.context().push_vec(
                        &mut ir.model.coedges,
                        coedge,
                        "catia_zero_topology_coedges",
                    ) {
                        return Some(Err(error));
                    }
                    if let Err(error) = admission.context().insert_hash_map(
                        &mut coedges_by_support,
                        support_record_ordinal,
                        coedge_id,
                        "catia_zero_topology_coedges_by_support",
                    ) {
                        return Some(Err(error));
                    }
                }
            }
        }

        for candidate in &edge_candidates {
            let first = coedges_by_support.get(&candidate.support_record_ordinals[0])?;
            let second = coedges_by_support.get(&candidate.support_record_ordinals[1])?;
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
            admission.context(),
            annotations,
            &body_id,
            "zero_entity_a9_03",
            u64_from_index(ownership_root.map_or(first_face.pos, |root| root.body_pos)),
            "topology_body",
            Exactness::Derived
        ));
        admitted!(crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &body_id,
            "kind",
            "catia_annotation_field"
        ));
        admitted!(crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &body_id,
            "regions",
            "catia_annotation_field"
        ));
        if let Err(error) = admission.charge() {
            return Some(Err(error));
        }
        let mut regions = Vec::new();
        if let Err(error) = admission.context().push_vec(
            &mut regions,
            copied_id!(region_id, RegionId),
            "catia_zero_topology_body_regions",
        ) {
            return Some(Err(error));
        }
        let body = Body {
            id: copied_id!(body_id, BodyId),
            kind: BodyKind::Solid,
            regions,
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        if let Err(error) =
            admission
                .context()
                .push_vec(&mut ir.model.bodies, body, "catia_zero_topology_bodies")
        {
            return Some(Err(error));
        }
        admitted!(annotate(
            admission.context(),
            annotations,
            &region_id,
            "zero_entity_a9_03",
            u64_from_index(ownership_root.map_or(first_face.pos, |root| root.shell_pos)),
            "topology_region",
            Exactness::Derived
        ));
        admitted!(crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &region_id,
            "body",
            "catia_annotation_field"
        ));
        admitted!(crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &region_id,
            "shells",
            "catia_annotation_field"
        ));
        if let Err(error) = admission.charge() {
            return Some(Err(error));
        }
        let mut shells = Vec::new();
        if let Err(error) = admission.context().push_vec(
            &mut shells,
            copied_id!(shell_id, ShellId),
            "catia_zero_topology_region_shells",
        ) {
            return Some(Err(error));
        }
        let region = Region {
            id: copied_id!(region_id, RegionId),
            body: body_id,
            shells,
        };
        if let Err(error) = admission.context().push_vec(
            &mut ir.model.regions,
            region,
            "catia_zero_topology_regions",
        ) {
            return Some(Err(error));
        }
        admitted!(annotate(
            admission.context(),
            annotations,
            &shell_id,
            "zero_entity_a9_03",
            u64_from_index(ownership_root.map_or(first_face.pos, |root| root.shell_pos)),
            "topology_shell",
            Exactness::Derived
        ));
        admitted!(crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &shell_id,
            "region",
            "catia_annotation_field"
        ));
        admitted!(crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &shell_id,
            "faces",
            "catia_annotation_field"
        ));
        if let Err(error) = admission.charge() {
            return Some(Err(error));
        }
        let shell = Shell::new(shell_id, region_id, face_ids, Vec::new(), Vec::new()).ok()?;
        if let Err(error) =
            admission
                .context()
                .push_vec(&mut ir.model.shells, shell, "catia_zero_topology_shells")
        {
            return Some(Err(error));
        }

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
            edges: edge_candidates.len(),
            vertices: endpoint_loci.len(),
            points: endpoint_loci.len(),
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
        *nurbs.knots().get(nurbs.control_points().len())?,
    ];
    (range[0] < range[1]).then_some(range)
}

fn curve_orientation(
    geometry: &cadmpeg_ir::geometry::CurveGeometry,
    parameter_range: [f64; 2],
    endpoints: [Point3; 2],
) -> Result<Option<bool>, cadmpeg_core::decode::ResourceLimit> {
    let Some(start) =
        cadmpeg_ir::eval::finite_or_refusal(curve_point(geometry, parameter_range[0]))?
    else {
        return Ok(None);
    };
    let Some(end) = cadmpeg_ir::eval::finite_or_refusal(curve_point(geometry, parameter_range[1]))?
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

fn endpoint_indices(reference: [FinitePoint3; 2], target: [FinitePoint3; 2]) -> Option<[usize; 2]> {
    let [reference, target] = [reference, target].map(|pair| pair.map(FinitePoint3::get));
    let direct = reference[0].distance(target[0]) <= MODEL_POINT_TOLERANCE.get()
        && reference[1].distance(target[1]) <= MODEL_POINT_TOLERANCE.get();
    let reversed = reference[0].distance(target[1]) <= MODEL_POINT_TOLERANCE.get()
        && reference[1].distance(target[0]) <= MODEL_POINT_TOLERANCE.get();
    match (direct, reversed) {
        (true, false) => Some([0, 1]),
        (false, true) => Some([1, 0]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget,
    };
    use cadmpeg_core::CodecError;
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
            pos: ordinal as usize,
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
                    start,
                    end,
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
                pos: face_ordinal as usize,
                record_ordinal: face_ordinal,
                tag: [0x5f, 0x0c],
                allocations: vec![10, 3],
                loops: Some(vec![super::super::records::ZeroEntityLoop {
                    pos: face_ordinal as usize + 1,
                    record_ordinal: face_ordinal + 100,
                    tag: [0x62, 0x14],
                    members: crate::families::zero_entity::records::ZeroEntityLoopMembers::try_new(
                        7,
                        1,
                        std::num::NonZeroUsize::new(3).expect("nonzero loop member count"),
                    )
                    .expect("admitted loop member run"),
                    typed_references: vec![1, 2, 3],
                    support_record_ordinals,

                    loop_class: ZeroEntityLoopClass::Outer41,
                    forward_senses: vec![true, true, true],
                    oriented_model_endpoints: order
                        .into_iter()
                        .map(|(start, end)| [start, end].map(finite))
                        .collect(),
                }]),
                terminal_control:
                    crate::families::zero_entity::records::ZeroEntityFaceControl::Control05,
            }),
            supports,
        }
    }

    #[test]
    fn closed_topology_face_ids_refuse_collection_limit_before_absent_result() {
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
                    support_runs: &support_runs,
                    surface_ids_by_position: &surface_ids,
                    support_curve_ids: &curve_ids,
                    ownership_root: None,
                },
                &WorkBudget::new(100),
                &mut crate::nurbs::LaneRefusals::new(),
            )
        };

        let service_arena = DecodeArena::new();
        let service_policy = DecodePolicy::service();
        let (service_ctx, _) =
            DecodeContext::from_root_bytes(&[0], &service_arena, &service_policy)
                .expect("fixture fits the input limit");
        assert!(run_with(&service_ctx)
            .expect("service resource budget")
            .is_none());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (limited_ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let Err(CodecError::ResourceLimit(error)) = run_with(&limited_ctx) else {
            panic!("face ids must refuse the collection limit");
        };
        assert_eq!(error.dimension, ResourceDimension::CollectionItems);
        assert_eq!(error.operation, "catia_zero_topology_face_ids");
    }

    #[test]
    fn closed_topology_face_identity_refuses_retained_limit_before_absent_result() {
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
                    support_runs: &support_runs,
                    surface_ids_by_position: &surface_ids,
                    support_curve_ids: &curve_ids,
                    ownership_root: None,
                },
                &WorkBudget::new(100),
                &mut crate::nurbs::LaneRefusals::new(),
            )
        };
        let service = crate::test_support::with_service_context(run_with);
        assert!(service.expect("service budget").is_none());
        let limited = crate::test_support::with_retained_limit(0, run_with);
        assert!(matches!(limited, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_zero_topology_face_id"));
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
        let mut no_root_ir = ir.clone();
        let mut no_root_annotations = AnnotationBuilder::new();
        let topology_budget =
            WorkBudget::new(super::super::topology::MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS);
        let no_root_counts = crate::test_support::with_service_context(|ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            transfer_closed_face_topology(
                &mut admission,
                &mut no_root_ir,
                &mut no_root_annotations,
                ZeroEntityClosedTopology {
                    support_runs: &runs,
                    surface_ids_by_position: &HashMap::from([(
                        100,
                        SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
                    )]),
                    support_curve_ids: &curve_ids,
                    ownership_root: None,
                },
                &topology_budget,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service entity admission")
        })
        .expect("complete topology without native ownership root");
        assert_eq!(no_root_counts.faces, 2);
        assert_eq!(no_root_ir.model.bodies[0].kind, BodyKind::Solid);
        assert!(
            crate::assemble::neutral_model_is_admissible(&mut no_root_ir, &[])
                .expect("resource allocation did not fail")
        );
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
                    support_runs: &runs,
                    surface_ids_by_position: &HashMap::from([(
                        100,
                        SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
                    )]),
                    support_curve_ids: &curve_ids,
                    ownership_root: Some(&root),
                },
                &topology_budget,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service entity admission")
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
        assert!(crate::assemble::neutral_model_is_admissible(&mut ir, &[])
            .expect("resource allocation did not fail"));
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
                    id: curve_ids[&support.record_ordinal].clone(),
                    geometry,
                    source_object: None,
                });
            }
        }
        let mut annotations = AnnotationBuilder::new();
        let budget = WorkBudget::new(super::super::topology::MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS);
        let counts = crate::test_support::with_service_context(|ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            transfer_closed_face_topology(
                &mut admission,
                &mut ir,
                &mut annotations,
                ZeroEntityClosedTopology {
                    support_runs: &runs,
                    surface_ids_by_position: &HashMap::from([(
                        100,
                        SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
                    )]),
                    support_curve_ids: &curve_ids,
                    ownership_root: None,
                },
                &budget,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service entity admission")
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
        assert!(crate::assemble::neutral_model_is_admissible(&mut ir, &[])
            .expect("resource allocation did not fail"));
    }
}
