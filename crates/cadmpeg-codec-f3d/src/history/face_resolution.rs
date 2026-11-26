// SPDX-License-Identifier: Apache-2.0
//! Draft, pattern, split, thread, and legacy face resolution.

use super::body_closures::stable_ref;
use super::face_boundaries::face_boundary_contexts_for_slots;
use super::topology_cache::TopologyQueryCache;
use super::{
    EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12, EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9,
};
use crate::history_records::{AsmHistoricalTopology, AsmHistory};
use std::collections::{BTreeSet, HashSet};

/// Resolve a Draft face whose persistent selector lane is ambiguous in the
/// active B-rep by following the surface carrier through the exact Draft
/// transition.
///
/// Draft changes the selected face's actual surface geometry. The same
/// transition may update tangent or context faces, so the transition is
/// admissible only when exactly one face from the operand's alternate lane (or,
/// when no alternate lane exists, its exact candidate lane) changes surface
/// geometry. A missing, duplicate, or unchanged surface is not a selection
/// proof. Carrier record identities are not compared: a history update may
/// assign a new carrier record without changing the surface.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum DraftSurfaceGeometry {
    Plane {
        origin: cadmpeg_ir::math::Point3,
        normal: cadmpeg_ir::math::Vector3,
    },
    Cylinder {
        origin: cadmpeg_ir::math::Point3,
        axis: cadmpeg_ir::math::Vector3,
        radius: f64,
    },
    Axis {
        origin: cadmpeg_ir::math::Point3,
        direction: cadmpeg_ir::math::Vector3,
        radius: Option<f64>,
    },
}

pub(super) fn draft_surface_geometry(
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
) -> Option<DraftSurfaceGeometry> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face)
        .map(|binding| binding.carrier);
    let carrier = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    if let Some(plane) = topology
        .surface_planes
        .iter()
        .find(|surface| surface.surface == carrier)
    {
        return Some(DraftSurfaceGeometry::Plane {
            origin: plane.origin,
            normal: plane.normal,
        });
    }
    if let Some(cylinder) = topology
        .surface_cylinders
        .iter()
        .find(|surface| surface.surface == carrier)
    {
        return Some(DraftSurfaceGeometry::Cylinder {
            origin: cylinder.origin,
            axis: cylinder.axis,
            radius: cylinder.radius,
        });
    }
    topology
        .surface_axes
        .iter()
        .find(|surface| surface.surface == carrier)
        .map(|axis| DraftSurfaceGeometry::Axis {
            origin: axis.origin,
            direction: axis.direction,
            radius: topology
                .surface_radii
                .iter()
                .find(|radius| radius.surface == carrier)
                .map(|radius| radius.radius),
        })
}

pub(super) fn resolve_draft_face_by_surface_transition(
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
) -> Option<i64> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !matches!(
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
            Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
        )
        || operand.recipe_nodes.is_empty()
    {
        return None;
    }
    let mut candidate_slots = HashSet::new();
    for face in operand
        .candidate_faces
        .iter()
        .filter_map(|face| stable_ref(face.as_str()))
    {
        candidate_slots.insert(face);
    }
    if candidate_slots.is_empty() {
        return None;
    }
    let mut alternate_slots = BTreeSet::new();
    for face in operand
        .recipe_references
        .iter()
        .flat_map(|reference| &reference.alternate_selector_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| candidate_slots.contains(face))
    {
        alternate_slots.insert(face);
    }
    let mut exact_slots = BTreeSet::new();
    for face in operand
        .recipe_references
        .iter()
        .flat_map(|reference| &reference.candidate_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| candidate_slots.contains(face))
    {
        exact_slots.insert(face);
    }
    let has_alternates = !alternate_slots.is_empty();
    let candidates = if has_alternates {
        alternate_slots
    } else {
        exact_slots
    };
    if candidates.is_empty() {
        return None;
    }
    if !has_alternates {
        let mut faces = candidates.iter();
        let face = faces.next().copied()?;
        if faces.next().is_some() {
            return None;
        }
        return (preceding.faces.contains(&face) && result.faces.contains(&face)).then_some(face);
    }
    let mut changed = None;
    for face in candidates
        .into_iter()
        .filter(|face| preceding.faces.contains(face) && result.faces.contains(face))
        .filter(|face| {
            draft_surface_geometry(preceding, *face)
                .zip(draft_surface_geometry(result, *face))
                .is_some_and(|(before, after)| before != after)
        })
    {
        if changed.replace(face).is_some() {
            return None;
        }
    }
    changed
}

pub(super) fn resolve_pattern_face_by_surface_radius(
    candidates: &[cadmpeg_ir::ids::FaceId],
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<i64> {
    let mut candidate_faces = HashSet::new();
    for face in candidates
        .iter()
        .filter_map(|face| stable_ref(face.as_str()))
    {
        candidate_faces.insert(face);
    }
    if candidate_faces.is_empty() {
        return None;
    }
    let mut result_radius = None;
    let mut bound_candidates = HashSet::new();
    for binding in result
        .face_surfaces
        .iter()
        .filter(|binding| candidate_faces.contains(&binding.entity))
    {
        if !bound_candidates.insert(binding.entity) {
            return None;
        }
        let mut radii = result
            .surface_radii
            .iter()
            .filter(|radius| radius.surface == binding.carrier);
        let radius = radii.next().map(|radius| radius.radius)?;
        if radii.next().is_some() || !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        match result_radius {
            None => result_radius = Some(radius.to_bits()),
            Some(expected) if expected == radius.to_bits() => {}
            Some(_) => return None,
        }
    }
    if bound_candidates != candidate_faces {
        return None;
    }
    let result_radius = result_radius?;
    let mut matches = preceding
        .face_surfaces
        .iter()
        .filter(|binding| changed_faces.contains(&binding.entity))
        .filter_map(|binding| {
            let mut radii = preceding
                .surface_radii
                .iter()
                .filter(|radius| radius.surface == binding.carrier);
            let radius = radii.next()?;
            (radii.next().is_none() && radius.radius.to_bits() == result_radius)
                .then_some(binding.entity)
        });
    let face = matches.next()?;
    matches.next().is_none().then_some(face)
}

pub(super) fn resolve_split_tool_face(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<i64> {
    if operand.group_record_index().is_some()
        || operand.group_member_ordinal().is_some()
        || operand.scope_reference_ordinal != 1
        || operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face
        || operand.recipe_program != [0, -1]
    {
        return None;
    }
    let [reference] = operand.recipe_references.as_slice() else {
        return None;
    };
    let candidates = super::selection::faces_in_topology(&reference.candidate_faces, topology);
    let [face] = candidates.as_slice() else {
        return None;
    };
    stable_ref(face.as_str())
}

pub(super) fn effective_faces(
    reference: &crate::records::dimensions::DesignRecipeReference,
) -> &[cadmpeg_ir::ids::FaceId] {
    if reference.candidate_faces.is_empty() {
        &reference.alternate_selector_faces
    } else {
        &reference.candidate_faces
    }
}

pub(super) fn resolve_thread_face_by_transition(
    scope: &crate::records::feature::scope::DesignParameterScope,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &AsmHistory,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<i64> {
    let construction = scope.thread_construction()?;
    let source = historical_brep_source(&history.id)?;
    let mut source_candidates = candidates
        .iter()
        .filter(|face| active_brep_face_matches_source(face, source));
    source_candidates.next()?;
    if source_candidates.next().is_some() {
        return None;
    }
    let minimum_radius = construction.diameters.minor() * 5.0;
    let maximum_radius = construction.diameters.major() * 5.0;
    if !minimum_radius.is_finite() || !maximum_radius.is_finite() {
        return None;
    }
    let tolerance = EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9 * (1.0 + maximum_radius.abs());
    let mut matching_faces = topology
        .faces
        .iter()
        .copied()
        .filter(|face| changed_faces.contains(face))
        .filter_map(|face| {
            let mut bindings = topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == face);
            let binding = bindings.next()?;
            if bindings.next().is_some() {
                return None;
            }
            let mut cylinders = topology
                .surface_cylinders
                .iter()
                .filter(|cylinder| cylinder.surface == binding.carrier);
            let cylinder = cylinders.next()?;
            (cylinders.next().is_none()
                && cylinder.radius + tolerance >= minimum_radius
                && cylinder.radius <= maximum_radius + tolerance)
                .then_some(face)
        });
    let face = matching_faces.next()?;
    matching_faces
        .all(|candidate| candidate == face)
        .then_some(face)
}

pub(super) fn grouped_reference_face_candidate(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<cadmpeg_ir::ids::FaceId> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(
            &operand.recipe_prefix_bytes,
        )
    {
        return None;
    }
    let mut candidates = operand
        .recipe_references
        .iter()
        .map(|reference| reference.design_reference)
        .filter(|reference| {
            topology.faces.contains(reference) && changed_faces.contains(reference)
        });
    let face = candidates.next()?;
    candidates
        .all(|candidate| candidate == face)
        .then(|| crate::ids::brep_face_id(face))
}

pub(super) fn relation_members(
    relations: &[crate::history_records::AsmHistoricalRelation],
    owner: i64,
) -> Option<&[i64]> {
    let mut matches = relations
        .iter()
        .filter(|relation| relation.owner_ref == owner);
    let members = matches.next()?.member_refs.as_slice();
    matches.next().is_none().then_some(members)
}

pub(super) fn resolve_bounded_face_recipe_target<'a>(
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &'a crate::history_records::AsmHistoricalTopology,
    result: &'a crate::history_records::AsmHistoricalTopology,
    inserted_bodies: &[i64],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return Ok(None);
    };
    if operand.recipe_nodes.len() != header_value
        || !operand
            .recipe_nodes
            .iter()
            .all(crate::records::topology::face::DesignFaceRecipeNode::has_two_side_clauses)
    {
        return Ok(None);
    }
    let Some(ordinals) =
        crate::records::topology::face::prefix_operand_ordinals(&operand.recipe_references)
    else {
        return Ok(None);
    };
    let first_clause_len = ordinals.iter().take_while(|ordinal| **ordinal == 0).count();
    if first_clause_len == 0 {
        return Ok(None);
    }
    let first_clause = &operand.recipe_references[..first_clause_len];
    let topology_faces = super::selection::FamilySlots::new(&preceding.faces);
    let mut target_candidates = BTreeSet::new();
    for face in first_clause
        .first()
        .into_iter()
        .flat_map(effective_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| topology_faces.contains(*face))
    {
        target_candidates.insert(face);
    }
    for reference in first_clause.iter().skip(1) {
        let mut candidates = HashSet::new();
        for face in effective_faces(reference)
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| topology_faces.contains(*face))
        {
            candidates.insert(face);
        }
        target_candidates.retain(|face| candidates.contains(face));
    }
    let mut construction_faces = Vec::new();
    'bodies: for body in inserted_bodies {
        let Some(regions) = relation_members(&result.body_regions, *body) else {
            continue;
        };
        let mut unique = None;
        let mut ambiguous = false;
        for region in regions {
            let Some(shells) = relation_members(&result.region_shells, *region) else {
                continue 'bodies;
            };
            for shell in shells {
                let Some(faces) = relation_members(&result.shell_faces, *shell) else {
                    continue 'bodies;
                };
                for face in faces {
                    if unique.is_some_and(|prior| prior != *face) {
                        ambiguous = true;
                    }
                    unique = Some(*face);
                }
            }
        }
        if !ambiguous {
            if let Some(face) = unique {
                construction_faces.push(face);
            }
        }
    }
    if construction_faces.is_empty() {
        return Ok(None);
    }
    let mut face_loop_positions = |face,
                                   topology|
     -> Result<
        Option<(usize, Vec<cadmpeg_ir::math::Point3>)>,
        cadmpeg_core::CodecError,
    > {
        let contexts = face_boundary_contexts_for_slots(&[face], topology, topology_queries);
        let [context] = contexts.as_slice() else {
            return Ok(None);
        };
        let [loop_] = context.loops.as_slice() else {
            return Ok(None);
        };
        let crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Positions(
            rows,
        ) = &loop_.boundary
        else {
            return Ok(None);
        };
        if rows.is_empty() {
            return Ok(None);
        }
        let positions = rows.iter().map(|row| row.position).collect::<Vec<_>>();
        Ok(Some((rows.len(), positions)))
    };
    let mut matches = Vec::new();
    for candidate in target_candidates {
        let Some((edge_count, candidate_points)) = face_loop_positions(candidate, preceding)?
        else {
            continue;
        };
        if edge_count != header_value {
            continue;
        }
        let mut matched = false;
        for face in &construction_faces {
            if let Some((construction_edge_count, construction_points)) =
                face_loop_positions(*face, result)?
            {
                if construction_edge_count >= edge_count
                    && cyclic_point_subsequence(&candidate_points, &construction_points)
                {
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            matches.push(candidate);
        }
    }
    matches.sort_unstable_by(Ord::cmp);
    matches.dedup();
    let [face] = matches.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*face))
}

pub(super) fn cyclic_point_subsequence(
    candidate: &[cadmpeg_ir::math::Point3],
    construction: &[cadmpeg_ir::math::Point3],
) -> bool {
    let coincident = |left: &cadmpeg_ir::math::Point3, right: &cadmpeg_ir::math::Point3| {
        let dx = left.x - right.x;
        let dy = left.y - right.y;
        let dz = left.z - right.z;
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)) <= EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12
    };
    let matches_orientation = |candidate: &[cadmpeg_ir::math::Point3]| {
        construction.iter().enumerate().any(|(start, point)| {
            if !coincident(&candidate[0], point) {
                return false;
            }
            let mut cursor = start;
            candidate.iter().skip(1).all(|target| {
                let limit = start + construction.len();
                while cursor < limit {
                    cursor += 1;
                    if coincident(target, &construction[cursor % construction.len()]) {
                        return true;
                    }
                }
                false
            })
        })
    };
    if candidate.is_empty() || candidate.len() > construction.len() {
        return false;
    }
    if matches_orientation(candidate) {
        return true;
    }
    let reversed_matches = construction.iter().enumerate().any(|(start, point)| {
        if !coincident(&candidate[candidate.len() - 1], point) {
            return false;
        }
        let mut cursor = start;
        candidate.iter().rev().skip(1).all(|target| {
            let limit = start + construction.len();
            while cursor < limit {
                cursor += 1;
                if coincident(target, &construction[cursor % construction.len()]) {
                    return true;
                }
            }
            false
        })
    });
    reversed_matches
}

pub(super) fn active_brep_face_matches_source(
    face: &cadmpeg_ir::ids::FaceId,
    source: &str,
) -> bool {
    face.as_str().starts_with("f3d:brep:entity#") || entity_in_brep_source(face.as_str(), source)
}

pub(super) fn entity_in_brep_source(id: &str, source: &str) -> bool {
    id.strip_prefix("f3d:brep/")
        .and_then(|tail| tail.strip_prefix(source))
        .is_some_and(|tail| tail.starts_with('/'))
}

#[derive(Debug, PartialEq)]
pub(super) enum LegacyFaceResolution {
    Historical(i64),
    Active(cadmpeg_ir::ids::FaceId),
}

pub(super) fn select_legacy_extrude_face_candidate(
    mut candidates: Vec<cadmpeg_ir::ids::FaceId>,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    history_source: Option<&str>,
) -> Option<LegacyFaceResolution> {
    for changed_only in [true, false] {
        let mut faces = candidates.iter().filter(|face| {
            stable_ref(face.as_str()).is_some_and(|slot| {
                topology.faces.contains(&slot) && (!changed_only || changed_faces.contains(&slot))
            })
        });
        let face = faces.next();
        if faces.next().is_none() {
            let Some(face) = face else {
                continue;
            };
            if let Some(slot) = stable_ref(face.as_str()) {
                return Some(LegacyFaceResolution::Historical(slot));
            }
        }
    }
    if let Some(source) = history_source {
        let unique_index = {
            let mut matches = candidates
                .iter()
                .enumerate()
                .filter(|(_, face)| entity_in_brep_source(face.as_str(), source));
            let first = matches.next().map(|(index, _)| index);
            first.filter(|_| matches.next().is_none())
        };
        if let Some(index) = unique_index {
            return Some(LegacyFaceResolution::Active(candidates.swap_remove(index)));
        }
    }
    if candidates.len() == 1 {
        candidates.pop().map(LegacyFaceResolution::Active)
    } else {
        None
    }
}

/// One body/native selection row, minting the non-blank native member at the
/// boundary where the F3D record states it.
///
/// `None` when the record states a blank native member: a row the IR carrier
/// does not hold.
pub(super) fn body_member<B>(
    body: B,
    native: String,
) -> Option<cadmpeg_ir::features::selections::BodyMember<B>> {
    Some(cadmpeg_ir::features::selections::BodyMember::new(
        body,
        cadmpeg_core::text::NonBlankString::new(native)?,
    ))
}

pub(super) fn historical_brep_source(state_id: &str) -> Option<&str> {
    state_id
        .rsplit_once("/BREP.")
        .or_else(|| state_id.rsplit_once("BREP."))?
        .1
        .split_once(":asm-")
        .map(|(source, _)| source)
}

pub(super) fn resolve_direct_face_recipe_clauses(
    references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Vec<i64> {
    let mut clauses = Vec::<(
        u64,
        u64,
        Vec<&crate::records::dimensions::DesignRecipeReference>,
    )>::new();
    for reference in references {
        let key = (reference.selector_offset, reference.token_offset);
        if let Some((_, _, references)) = clauses
            .iter_mut()
            .find(|(selector, token, _)| (*selector, *token) == key)
        {
            references.push(reference);
        } else {
            let grouped = vec![reference];
            clauses.push((key.0, key.1, grouped));
        }
    }
    let topology_faces = super::selection::FamilySlots::new(&topology.faces);
    let mut resolved = Vec::new();
    for (_, _, references) in clauses {
        let mut intersection = None::<HashSet<i64>>;
        for reference in references {
            let candidates = if reference.candidate_faces.is_empty() {
                &reference.alternate_selector_faces
            } else {
                &reference.candidate_faces
            };
            let mut eligible = HashSet::new();
            for face in candidates
                .iter()
                .filter_map(|face| stable_ref(face.as_str()))
                .filter(|face| topology_faces.contains(*face) && changed_faces.contains(face))
            {
                eligible.insert(face);
            }
            let candidates = eligible;
            if candidates.is_empty() {
                return Vec::new();
            }
            intersection = Some(match intersection {
                None => candidates,
                Some(mut intersection) => {
                    intersection.retain(|face| candidates.contains(face));
                    intersection
                }
            });
        }
        let Some(intersection) = intersection else {
            return Vec::new();
        };
        let mut candidates = intersection.into_iter();
        let Some(face) = candidates.next() else {
            return Vec::new();
        };
        if candidates.next().is_some() {
            return Vec::new();
        }
        if !resolved.contains(&face) {
            resolved.push(face);
        }
    }
    resolved
}
