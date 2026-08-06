// SPDX-License-Identifier: Apache-2.0
//! Resolve face-selection operands and extrude start planes.

use crate::design::dimensions::{planar_point, sketch_normal_sign};
use crate::design::edge_resolve::feature_input_topology_id;
use crate::design::feature_project::design_angle_unit;
use crate::ids::{self, native_stream, neutral_feature_id};
use crate::records::{
    DesignBodyRecipeOperand, DesignConstructionOperandGroup, DesignExtrudeFaceRole,
    DesignFaceOperand, DesignParameter, DesignParameterScope, DesignSketchPlacement,
    SketchCurveGeometry, SketchCurveIdentity, SketchPoint,
};
use cadmpeg_core::le::f64_at;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::HashSet;

pub(crate) fn resolved_face_group(
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let stream = native_stream(&group.id)?;
    let mut faces = Vec::with_capacity(group.members.len());
    for record_index in &group.members {
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let operand_faces = resolved_face_operand(operand)?;
        for face in operand_faces {
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
    }
    (!faces.is_empty()).then(|| cadmpeg_ir::features::FaceSelection::Resolved {
        faces,
        native: group.id.clone(),
    })
}

/// Resolve the active faces carried by an Extrude target-shape body recipe.
///
/// The native target is a whole-body recipe rather than a face recipe. Its
/// persistent Design references still identify the active boundary faces that
/// define the target shape, so retain that exact set alongside the native
/// group identity.
pub(crate) fn resolved_body_recipe_shape(
    group: &DesignConstructionOperandGroup,
    operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let stream = native_stream(&group.id)?;
    let mut faces = Vec::new();
    for (ordinal, record_index) in group.members.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some() || operand.references.is_empty() {
            return None;
        }
        for reference in &operand.references {
            for face in &reference.candidate_faces {
                if !faces.contains(face) {
                    faces.push(face.clone());
                }
            }
        }
    }
    (!faces.is_empty()).then(|| cadmpeg_ir::features::FaceSelection::Resolved {
        faces,
        native: group.id.clone(),
    })
}

pub(crate) fn resolved_profile_face_group(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::ProfileRef;

    let selection = resolved_historical_face_group(scope, group, operands)?;
    let cadmpeg_ir::features::FaceSelection::Historical {
        state,
        faces,
        native,
    } = selection
    else {
        return None;
    };
    Some(ProfileRef::HistoricalFaces {
        state,
        faces,
        native: vec![native],
    })
}

pub(crate) fn resolved_historical_face_group(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    use cadmpeg_ir::features::FaceSelection;

    let previous_state_id = scope.previous_history_state_id?;
    let stream = native_stream(&group.id)?;
    let mut faces = Vec::with_capacity(group.members.len());
    for (ordinal, record_index) in group.members.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index == Some(group.record_index)
                && operand.group_member_ordinal == Some(ordinal)
                && operand.record_index == *record_index
        });
        let operand = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        if operand.resolved_face_slots.is_empty() {
            return None;
        }
        for face in &operand.resolved_face_slots {
            if !faces.contains(face) {
                faces.push(*face);
            }
        }
    }
    (!faces.is_empty()).then(|| {
        let feature = neutral_feature_id(scope);
        let feature_key = feature
            .0
            .split_once('#')
            .map_or(feature.0.as_str(), |(_, key)| key);
        FaceSelection::Historical {
            state: feature_input_topology_id(&feature, previous_state_id),
            faces: faces
                .into_iter()
                .map(|face| {
                    ids::history_input_face_id(
                        &ids::history_input_prefix(feature_key, previous_state_id),
                        face,
                    )
                })
                .collect(),
            native: group.id.clone(),
        }
    })
}

fn resolved_face_operand(operand: &DesignFaceOperand) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    if !operand.resolved_face_slots.is_empty() {
        return Some(
            operand
                .resolved_face_slots
                .iter()
                .map(|slot| cadmpeg_ir::ids::FaceId(ids::brep_entity_id(slot)))
                .collect(),
        );
    }
    let candidates = face_operand_candidates(operand);
    if !operand.alternate_selector_candidate_faces.is_empty() {
        return Some(candidates.to_vec());
    }
    if operand.recipe_kind == crate::records::ConstructionRecipeKind::Face {
        let mut referenced = Vec::new();
        for reference in &operand.recipe_references {
            for face in &reference.candidate_faces {
                if !referenced.contains(face) {
                    referenced.push(face.clone());
                }
            }
        }
        if !referenced.is_empty() {
            return Some(referenced);
        }
    }
    if !operand.unreferenced_candidate_faces.is_empty()
        && complete_counted_face_recipe(operand).is_some()
    {
        return Some(candidates.to_vec());
    }
    let [face] = candidates else { return None };
    Some(vec![face.clone()])
}

fn complete_counted_face_recipe(operand: &DesignFaceOperand) -> Option<usize> {
    if operand.recipe_kind != crate::records::ConstructionRecipeKind::BoundedFace {
        return None;
    }
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return None;
    };
    (operand.recipe_nodes.len() == header_value
        && operand
            .recipe_nodes
            .iter()
            .all(|node| node.recipe_structure.is_some()))
    .then_some(header_value)
}

pub(crate) fn resolve_face_operand_history_candidates(operand: &DesignFaceOperand) -> Option<i64> {
    let direct = match operand.preceding_candidate_faces.as_slice() {
        [face] => face,
        _ => {
            let [face] = operand.changed_candidate_faces.as_slice() else {
                return resolve_face_operand_support_candidate(operand);
            };
            face
        }
    };
    if !historical_face_operand_candidates(operand).contains(direct) {
        return None;
    }
    direct.0.rsplit_once('#')?.1.parse().ok()
}

pub(crate) fn resolve_bounded_face_history_candidates(
    operand: &DesignFaceOperand,
) -> Option<Vec<i64>> {
    if operand.recipe_kind != crate::records::ConstructionRecipeKind::BoundedFace {
        return None;
    }
    if let Some(candidate) = convergent_effective_face_support(operand) {
        return Some(candidate);
    }
    let header_value = complete_counted_face_recipe(operand)?;
    bounded_face_candidate_by_boundary_cardinality(
        header_value,
        &operand.historical_support_contexts,
    )
}

pub(crate) fn resolve_stable_bounded_face_history_set(
    operand: &DesignFaceOperand,
) -> Option<Vec<i64>> {
    complete_counted_face_recipe(operand)?;
    let mut active_faces = Vec::with_capacity(operand.preceding_candidate_faces.len());
    for face in &operand.preceding_candidate_faces {
        let slot = face.0.rsplit_once('#')?.1.parse::<i64>().ok()?;
        if active_faces.contains(&slot) {
            return None;
        }
        active_faces.push(slot);
    }
    stable_face_support_set(&active_faces, &operand.historical_support_contexts)
}

fn stable_face_support_set(
    active_faces: &[i64],
    contexts: &[crate::records::DesignHistoricalFaceSupportContext],
) -> Option<Vec<i64>> {
    if active_faces.is_empty()
        || contexts.len() != active_faces.len()
        || active_faces.iter().collect::<HashSet<_>>().len() != active_faces.len()
    {
        return None;
    }
    let mut covered = HashSet::with_capacity(active_faces.len());
    for context in contexts {
        if !active_faces.contains(&context.active_face_slot)
            || !covered.insert(context.active_face_slot)
            || context.preceding_face_slots != [context.active_face_slot]
            || !context.changed_preceding_face_slots.is_empty()
        {
            return None;
        }
    }
    Some(active_faces.to_vec())
}

fn convergent_effective_face_support(operand: &DesignFaceOperand) -> Option<Vec<i64>> {
    let mut active_faces = face_operand_candidates(operand)
        .iter()
        .filter_map(|face| face.0.rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<Vec<_>>();
    active_faces.sort_unstable();
    active_faces.dedup();
    convergent_face_support(&active_faces, &operand.historical_support_contexts)
}

fn convergent_face_support(
    active_faces: &[i64],
    support_contexts: &[crate::records::DesignHistoricalFaceSupportContext],
) -> Option<Vec<i64>> {
    if active_faces.is_empty() {
        return None;
    }
    let mut contexts = support_contexts.iter();
    let first = contexts.next()?;
    let mut support = first.preceding_face_slots.clone();
    support.sort_unstable();
    support.dedup();
    if support.is_empty() {
        return None;
    }
    let mut covered = vec![first.active_face_slot];
    for context in contexts {
        let mut candidate = context.preceding_face_slots.clone();
        candidate.sort_unstable();
        candidate.dedup();
        if candidate != support {
            return None;
        }
        covered.push(context.active_face_slot);
    }
    covered.sort_unstable();
    covered.dedup();
    (covered == active_faces).then_some(support)
}

fn bounded_face_candidate_by_boundary_cardinality(
    header_value: usize,
    contexts: &[crate::records::DesignHistoricalFaceSupportContext],
) -> Option<Vec<i64>> {
    let mut candidates = contexts
        .iter()
        .filter_map(|context| {
            let mut faces = context.preceding_face_slots.clone();
            faces.sort_unstable();
            faces.dedup();
            if faces.is_empty()
                || faces.len() != context.preceding_face_boundaries.len()
                || !faces.iter().all(|face| {
                    context
                        .preceding_face_boundaries
                        .iter()
                        .any(|boundary| boundary.face_slot == *face)
                })
            {
                return None;
            }
            let boundary_edge_count =
                context
                    .preceding_face_boundaries
                    .iter()
                    .try_fold(0usize, |total, face| {
                        face.loops.iter().try_fold(total, |total, loop_| {
                            (!loop_.edge_slots.is_empty()
                                && loop_.edge_slots.len() == loop_.coedge_slots.len())
                            .then(|| total.checked_add(loop_.edge_slots.len()))
                            .flatten()
                        })
                    })?;
            (boundary_edge_count == header_value).then_some(faces)
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn resolve_face_operand_support_candidate(operand: &DesignFaceOperand) -> Option<i64> {
    let reference = operand.recipe_references.first()?;
    let active_faces = if reference.candidate_faces.is_empty() {
        &reference.alternate_selector_faces
    } else {
        &reference.candidate_faces
    };
    let active_slots = active_faces
        .iter()
        .filter_map(|face| face.0.rsplit_once('#')?.1.parse::<i64>().ok())
        .collect::<HashSet<_>>();
    if active_slots.is_empty() {
        return None;
    }
    let mut candidates = operand
        .historical_support_contexts
        .iter()
        .filter(|context| active_slots.contains(&context.active_face_slot))
        .flat_map(|context| {
            if context.changed_preceding_face_slots.is_empty() {
                context.preceding_face_slots.iter()
            } else {
                context.changed_preceding_face_slots.iter()
            }
        })
        .copied()
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(crate) fn face_operand_candidates(operand: &DesignFaceOperand) -> &[cadmpeg_ir::ids::FaceId] {
    if !operand.alternate_selector_candidate_faces.is_empty() {
        &operand.alternate_selector_candidate_faces
    } else if operand.unreferenced_candidate_faces.is_empty() {
        &operand.candidate_faces
    } else {
        &operand.unreferenced_candidate_faces
    }
}

/// Return the active face identities that can participate in historical
/// resolution. A single-face recipe names its selected face in the recipe
/// reference even when the broader persistent-tag set also contains faces
/// excluded from the operand's unreferenced candidate lane.
pub(crate) fn historical_face_operand_candidates(
    operand: &DesignFaceOperand,
) -> Vec<cadmpeg_ir::ids::FaceId> {
    if operand.recipe_kind == crate::records::ConstructionRecipeKind::Face {
        let mut referenced = operand
            .recipe_references
            .iter()
            .flat_map(|reference| {
                reference
                    .candidate_faces
                    .iter()
                    .chain(&reference.alternate_selector_faces)
            })
            .cloned()
            .collect::<Vec<_>>();
        referenced.sort_by(|left, right| left.0.cmp(&right.0));
        referenced.dedup();
        if !referenced.is_empty() {
            return referenced;
        }
    }
    face_operand_candidates(operand).to_vec()
}

/// Resolve selected-face Extrude starts from exact sketch-plane coincidence.
pub(crate) struct ExtrudeStartPlaneResolution<'a> {
    pub faces: &'a [cadmpeg_ir::topology::Face],
    pub surfaces: &'a [cadmpeg_ir::geometry::Surface],
    pub groups: &'a [DesignConstructionOperandGroup],
    pub operands: &'a mut [DesignFaceOperand],
    pub linear_tolerance: f64,
    pub angular_tolerance: f64,
}

pub(crate) fn bind_extrude_start_planes(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: &mut ExtrudeStartPlaneResolution<'_>,
) {
    use cadmpeg_ir::features::{ExtrudeStart, FaceSelection, FeatureDefinition, ProfileRef};

    for feature in features {
        let FeatureDefinition::Extrude { profile, start, .. } = &mut feature.definition else {
            continue;
        };
        let sketch_id = match profile {
            ProfileRef::Sketch(sketch)
            | ProfileRef::SketchProfiles { sketch, .. }
            | ProfileRef::SketchRegions { sketch, .. }
            | ProfileRef::SketchEntities { sketch, .. }
            | ProfileRef::SketchSelection { sketch, .. } => sketch,
            ProfileRef::Native(_)
            | ProfileRef::Unresolved(_)
            | ProfileRef::Feature(_)
            | ProfileRef::Generated { .. }
            | ProfileRef::SpatialSketchProfiles { .. }
            | ProfileRef::SpatialSketchSelection { .. }
            | ProfileRef::HistoricalFaces { .. }
            | ProfileRef::Faces(_) => continue,
        };
        let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id) else {
            continue;
        };
        let ExtrudeStart::FromFace {
            face: FaceSelection::Native(native),
            offset,
        } = start
        else {
            continue;
        };
        let retained_offset = *offset;
        let mut matching_groups = resolution.groups.iter().filter(|group| group.id == *native);
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some()
            || group.extrude_face_role != Some(DesignExtrudeFaceRole::Start)
        {
            continue;
        }
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut candidates = Vec::new();
        for record_index in &group.members {
            let mut matching_operands = resolution.operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index == *record_index
            });
            let Some(operand) = matching_operands.next() else {
                candidates.clear();
                break;
            };
            if matching_operands.next().is_some() {
                candidates.clear();
                break;
            }
            candidates.extend(face_operand_candidates(operand).iter().cloned());
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0));
        candidates.dedup();
        let coincident = candidates
            .into_iter()
            .filter(|candidate| {
                face_coincident_with_sketch(
                    candidate,
                    sketch,
                    resolution.faces,
                    resolution.surfaces,
                    resolution.linear_tolerance,
                    resolution.angular_tolerance,
                )
            })
            .collect::<Vec<_>>();
        if let [face] = coincident.as_slice() {
            retain_face_operand_resolution(group, resolution.operands, face);
            *start = ExtrudeStart::FromFace {
                face: FaceSelection::Resolved {
                    faces: vec![face.clone()],
                    native: native.clone(),
                },
                offset: retained_offset,
            };
        }
    }
}

pub(crate) fn retain_face_operand_resolution(
    group: &DesignConstructionOperandGroup,
    operands: &mut [DesignFaceOperand],
    face: &cadmpeg_ir::ids::FaceId,
) -> bool {
    let Some(stream) = native_stream(&group.id) else {
        return false;
    };
    let Some(slot) = face
        .0
        .rsplit_once('#')
        .and_then(|(_, slot)| slot.parse::<i64>().ok())
    else {
        return false;
    };
    let mut matches = operands.iter_mut().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == group.scope_record_index
            && group.members.contains(&operand.record_index)
            && face_operand_candidates(operand).contains(face)
            && (operand.resolved_face_slots.is_empty() || operand.resolved_face_slots == [slot])
    });
    let Some(operand) = matches.next() else {
        return false;
    };
    if matches.next().is_some() {
        return false;
    }
    operand.resolved_face_slots = vec![slot];
    true
}

pub(crate) fn face_coincident_with_sketch(
    candidate: &cadmpeg_ir::ids::FaceId,
    sketch: &cadmpeg_ir::sketches::Sketch,
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> bool {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let Some(face) = faces.iter().find(|face| face.id == *candidate) else {
        return false;
    };
    let Some(surface) = surfaces.iter().find(|surface| surface.id == face.surface) else {
        return false;
    };
    let SurfaceGeometry::Plane { origin, normal, .. } = &surface.geometry else {
        return false;
    };
    let Some((sketch_origin, sketch_normal, _)) = sketch.resolved_placement() else {
        return false;
    };
    parallel_vectors(*normal, sketch_normal, angular_tolerance)
        && point_plane_distance(*origin, sketch_origin, sketch_normal) <= linear_tolerance
}

fn parallel_vectors(left: Vector3, right: Vector3, tolerance: f64) -> bool {
    let cross = Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    );
    let left_length = (left.x * left.x + left.y * left.y + left.z * left.z).sqrt();
    let right_length = (right.x * right.x + right.y * right.y + right.z * right.z).sqrt();
    let cross_length = (cross.x * cross.x + cross.y * cross.y + cross.z * cross.z).sqrt();
    left_length > 0.0
        && right_length > 0.0
        && cross_length <= tolerance * left_length * right_length
}

fn point_plane_distance(point: Point3, origin: Point3, normal: Vector3) -> f64 {
    let normal_length = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
    if normal_length == 0.0 {
        return f64::INFINITY;
    }
    ((point.x - origin.x) * normal.x
        + (point.y - origin.y) * normal.y
        + (point.z - origin.z) * normal.z)
        .abs()
        / normal_length
}

pub(crate) fn design_angle(parameter: &DesignParameter) -> Option<cadmpeg_ir::features::Angle> {
    (parameter.unit.as_deref().is_some_and(design_angle_unit)
        && parameter.evaluated_value.is_finite())
    .then_some(cadmpeg_ir::features::Angle(parameter.evaluated_value))
}

pub(crate) fn valid_chamfer_spec(spec: &cadmpeg_ir::features::ChamferSpec) -> bool {
    use cadmpeg_ir::features::ChamferSpec;

    match spec {
        ChamferSpec::Distance { distance } => distance.0 > 0.0,
        ChamferSpec::TwoDistances { first, second } => first.0 > 0.0 && second.0 > 0.0,
        ChamferSpec::DistanceAngle { distance, angle } => {
            distance.0 > 0.0 && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
        }
        ChamferSpec::Unresolved { .. } => false,
    }
}

/// Length scale from a placement's stored origin to the neutral length unit.
/// The 201/329-byte frames store the origin in the neutral unit directly; the
/// `EntityGenesis`-flavor 213/341-byte frames and the member-run head record
/// of a feature-owned sketch store it in centimetres while their sketch point
/// and curve records carry values ten times the centimetre value, so the
/// origin scales by ten to stay commensurate with the entities.
pub(crate) fn placement_origin_scale(placement: &DesignSketchPlacement) -> f64 {
    if placement.member_run_head || matches!(placement.frame_length, 213 | 341) {
        10.0
    } else {
        1.0
    }
}

pub(crate) fn sketch_curve_is_spatial(curve: &SketchCurveIdentity) -> bool {
    match curve.geometry.as_ref() {
        Some(SketchCurveGeometry::Line { start, end, .. }) => {
            !(planar_point(start) && planar_point(end))
        }
        Some(SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            ..
        }) => {
            !(planar_point(center)
                && reference_direction.z.abs() <= 1.0e-9
                && sketch_normal_sign(normal).is_some())
        }
        Some(SketchCurveGeometry::Nurbs { control_points, .. }) => {
            control_points.iter().any(|point| !planar_point(point))
        }
        None => false,
    }
}

pub(crate) fn sketch_point_depth(point: &SketchPoint) -> Option<f64> {
    f64_at(&point.raw_bytes, point.coordinate_offset as usize + 16).map(|value| value * 10.0)
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_face_candidate_by_boundary_cardinality, convergent_face_support,
        stable_face_support_set,
    };
    use crate::records::{
        DesignHistoricalFaceBoundaryContext, DesignHistoricalFaceLoopContext,
        DesignHistoricalFaceSupportContext,
    };

    fn support(active: i64, faces: &[(i64, usize)]) -> DesignHistoricalFaceSupportContext {
        DesignHistoricalFaceSupportContext {
            active_face_slot: active,
            surface_slot: active + 1_000,
            preceding_face_slots: faces.iter().map(|(face, _)| *face).collect(),
            preceding_face_boundaries: faces
                .iter()
                .map(|(face, edge_count)| DesignHistoricalFaceBoundaryContext {
                    face_slot: *face,
                    loops: vec![DesignHistoricalFaceLoopContext {
                        loop_slot: face + 2_000,
                        coedge_slots: (0..*edge_count)
                            .map(|ordinal| i64::try_from(ordinal).expect("test ordinal"))
                            .collect(),
                        edge_slots: (0..*edge_count)
                            .map(|ordinal| i64::try_from(ordinal).expect("test ordinal") + 10_000)
                            .collect(),
                        vertex_slots: Vec::new(),
                        point_slots: Vec::new(),
                        positions: Vec::new(),
                    }],
                })
                .collect(),
            changed_preceding_face_slots: Vec::new(),
        }
    }

    #[test]
    fn bounded_face_cardinality_resolves_one_complete_predecessor_set() {
        let contexts = [
            support(10, &[(100, 12)]),
            support(11, &[(101, 4), (102, 4)]),
            support(12, &[(101, 4), (102, 4)]),
        ];
        assert_eq!(
            bounded_face_candidate_by_boundary_cardinality(8, &contexts),
            Some(vec![101, 102])
        );
        assert_eq!(
            bounded_face_candidate_by_boundary_cardinality(12, &contexts),
            Some(vec![100])
        );
    }

    #[test]
    fn bounded_face_cardinality_rejects_conflicting_complete_sets() {
        let contexts = [support(10, &[(100, 4)]), support(11, &[(101, 4)])];
        assert_eq!(
            bounded_face_candidate_by_boundary_cardinality(4, &contexts),
            None
        );
    }

    #[test]
    fn effective_faces_resolve_only_with_complete_convergent_support() {
        let contexts = [
            support(10, &[(100, 4)]),
            support(11, &[(100, 4)]),
            support(12, &[(100, 4)]),
        ];
        assert_eq!(
            convergent_face_support(&[10, 11, 12], &contexts),
            Some(vec![100])
        );
        assert_eq!(convergent_face_support(&[10, 11, 12, 13], &contexts), None);

        let conflicting = [support(10, &[(100, 4)]), support(11, &[(101, 4)])];
        assert_eq!(convergent_face_support(&[10, 11], &conflicting), None);
    }

    #[test]
    fn stable_bounded_face_set_preserves_each_proven_predecessor() {
        let active_faces = [10, 11];
        let mut contexts = vec![support(10, &[(10, 4)]), support(11, &[(11, 4)])];

        assert_eq!(
            stable_face_support_set(&active_faces, &contexts),
            Some(vec![10, 11])
        );
        contexts[1].preceding_face_slots = vec![12];
        assert_eq!(stable_face_support_set(&active_faces, &contexts), None);
    }
}
