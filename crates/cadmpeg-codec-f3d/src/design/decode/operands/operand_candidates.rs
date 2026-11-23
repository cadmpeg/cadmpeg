// SPDX-License-Identifier: Apache-2.0
//! Face and edge operand candidate binding.

use super::face_sources::{indexed_operand_recipes, referenced_operand_faces};
use crate::design::decode::dimension_frames::bind_recipe_reference_candidates;
use crate::design::decode::sketch::{indexed_record_header_at, next_indexed_record_offset};
use crate::layout::{
    indexed_design_record_header as indexed_header, sketch_profile_text_selection as text_selection,
};
use crate::records::recipes::ConstructionRecipe;
use crate::records::sketch_links::PersistentSubentityTag;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::face::DesignFaceOperand;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_core::decode::View;

/// Join each face recipe's persistent Design reference to active solved faces.
pub(crate) fn bind_face_operand_candidates(
    operands: &mut [DesignFaceOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    let recipes = indexed_operand_recipes(recipes);
    for operand in operands {
        operand.alternate_selector_candidate_faces.clear();
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0)
        else {
            continue;
        };
        operand.candidate_faces.clear();
        for tag in tags.iter().filter(|tag| {
            tag.design_references.contains(&design_reference)
                && crate::ids::same_native_occurrence(&tag.id, &operand.id)
        }) {
            if let AttributeTarget::Face(face) = &tag.target {
                operand.candidate_faces.push(face.clone());
            }
        }
        operand
            .candidate_faces
            .sort_by(|left, right| left.as_str().cmp(right.as_str()));
        operand.candidate_faces.dedup();
        let referenced = referenced_operand_faces(&operand.recipe_references, design_reference);
        operand.unreferenced_candidate_faces.clear();
        for face in operand
            .candidate_faces
            .iter()
            .filter(|face| !referenced.contains(face))
        {
            operand.unreferenced_candidate_faces.push(face.clone());
        }
        operand.alternate_selector_candidate_faces.extend(
            operand
                .recipe_references
                .iter()
                .filter(|reference| reference.design_reference == design_reference)
                .flat_map(|reference| &reference.alternate_selector_faces)
                .cloned(),
        );
        operand
            .alternate_selector_candidate_faces
            .sort_by(|left, right| left.as_str().cmp(right.as_str()));
        operand.alternate_selector_candidate_faces.dedup();
    }
}

/// Join each edge recipe's persistent Design reference to active solved faces.
pub(crate) fn bind_edge_operand_candidates(
    operands: &mut [DesignEdgeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    let recipes = indexed_operand_recipes(recipes);
    for operand in operands {
        operand.candidate_faces.clear();
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0)
        else {
            continue;
        };
        for tag in tags.iter().filter(|tag| {
            tag.design_references.contains(&design_reference)
                && crate::ids::same_native_occurrence(&tag.id, &operand.id)
        }) {
            if let cadmpeg_ir::attributes::AttributeTarget::Face(face) = &tag.target {
                operand.candidate_faces.push(face.clone());
            }
        }
        operand
            .candidate_faces
            .sort_by(|a, b| a.as_str().cmp(b.as_str()));
        operand.candidate_faces.dedup();
    }
}

pub(crate) fn edge_operand_candidate_faces(
    design_reference: i64,
    tags: &[PersistentSubentityTag],
    owner_id: Option<&str>,
) -> Vec<cadmpeg_ir::ids::FaceId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut faces = tags
        .iter()
        .filter(|tag| {
            tag.design_references.contains(&design_reference)
                && owner_id
                    .is_none_or(|owner_id| crate::ids::same_native_occurrence(&tag.id, owner_id))
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Face(id) => Some(id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    faces
}

/// The Sketch text record index named by the text selection `N+3` of the
/// sketch-profile operand with record index `N`.
pub(super) fn sketch_profile_text_target(
    bytes: &[u8],
    profile_record_index: u32,
    paired_byte_offset: u64,
) -> Option<u32> {
    let paired_at = usize::try_from(paired_byte_offset).ok()?;
    let next_header = |position: usize, expected: u32| {
        let at = next_indexed_record_offset(bytes, position)?;
        (indexed_record_header_at(bytes, at)?.record_index == expected).then_some(at)
    };
    let nested_one_at = next_header(
        paired_at.checked_add(indexed_header::LEN)?,
        profile_record_index.checked_add(1)?,
    )?;
    let nested_two_at = next_header(
        nested_one_at.checked_add(indexed_header::LEN)?,
        profile_record_index.checked_add(2)?,
    )?;
    let selection_at = next_header(
        nested_two_at.checked_add(indexed_header::LEN)?,
        profile_record_index.checked_add(3)?,
    )?;
    let zeros_at = selection_at.checked_add(text_selection::ZERO_RUN_10)?;
    let text_at = selection_at.checked_add(text_selection::TEXT_RECORD_INDEX)?;
    let end = selection_at.checked_add(text_selection::LEN)?;
    if bytes.get(zeros_at..text_at)?.iter().any(|&byte| byte != 0) {
        return None;
    }
    let text_record = View::u64_le_at(bytes, text_at)?;
    match next_indexed_record_offset(bytes, zeros_at) {
        Some(next) if next == end => {}
        None if bytes.len() == end => {}
        _ => return None,
    }
    u32::try_from(text_record).ok()
}

/// The text picks of an Extrude whose every sketch-profile operand carries a
/// text selection, in operand order.
pub(super) fn sketch_profile_text_picks(
    bytes: &[u8],
    profiles: &[DesignSketchProfileOperand],
) -> Option<Vec<crate::design::sketch_text_extrude::SketchTextPick>> {
    let picks = profiles
        .iter()
        .map(|profile| {
            Some(crate::design::sketch_text_extrude::SketchTextPick {
                sketch_entity: profile.entity_id.suffix(),
                text_record: sketch_profile_text_target(
                    bytes,
                    profile.record_index,
                    profile.paired_byte_offset(),
                )?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (!picks.is_empty()).then_some(picks)
}
