// SPDX-License-Identifier: Apache-2.0
//! Face operand frames and group retention.

use super::face_operands::RegisteredFaceOperandFrame;
use super::face_recipe_program_kind;
use super::recipe_structure::{face_recipe_next_boundary, face_recipe_nodes};
use super::sketch_profiles::parse_sketch_profile_region_selection;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::dimension_frames::{
    contiguous_i32_program, decode_recipe_references, recipe_record_prefix,
};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::design_record_id;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::native_stream;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

#[derive(Clone, Copy)]
pub(crate) struct FaceOperandFrame<'a> {
    pub(crate) scope: &'a DesignParameterScope,
    pub(crate) scope_reference_ordinal: u32,
    pub(crate) group_ownership: Option<(u32, u32)>,
    pub(crate) next_byte_offset: Option<u64>,
    pub(crate) registered_frame: Option<&'a RegisteredFaceOperandFrame>,
    pub(crate) header: &'a DesignRecordHeader,
}

pub(crate) fn parse_face_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    input: FaceOperandFrame<'_>,
    recipes: &[ConstructionRecipe],
) -> Option<Result<DesignFaceOperand, CodecError>> {
    let FaceOperandFrame {
        scope,
        scope_reference_ordinal,
        group_ownership,
        next_byte_offset,
        registered_frame,
        header,
    } = input;
    let stream = native_stream(&scope.id)?;
    let (offsets, next_record_index) = if let Some(frame) = registered_frame {
        if frame.header.record_index != header.record_index
            || frame.header.byte_offset != header.byte_offset
        {
            return None;
        }
        (frame.offsets, frame.next_record_index)
    } else {
        let start = usize::try_from(header.byte_offset).ok()?;
        let mut offsets = [0usize; 5];
        let mut position = start.checked_add(11)?;
        for (delta, slot) in offsets[..4].iter_mut().enumerate() {
            let record_index = header
                .record_index
                .checked_add(u32::try_from(delta).ok()?)?;
            let offset = records.first_at_or_after(position, record_index)?;
            *slot = offset;
            position = offset.checked_add(11)?;
        }
        if parse_sketch_profile_region_selection(bytes, header.record_index, offsets[0]).is_some() {
            return None;
        }
        let recipe_start = u64::try_from(offsets[3]).ok()?;
        let first_recipe = recipes
            .iter()
            .filter(|recipe| {
                native_stream(&recipe.id) == Some(stream)
                    && recipe.byte_offset > recipe_start
                    && next_byte_offset.is_none_or(|end| recipe.byte_offset < end)
            })
            .min_by_key(|recipe| recipe.byte_offset)?;
        let family_name_len = match first_recipe.kind {
            ConstructionRecipeKind::Face => b"face_recipe_data".len(),
            ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
            _ => return None,
        };
        let program_start = usize::try_from(first_recipe.byte_offset)
            .ok()?
            .checked_add(family_name_len)?;
        let (immediate_next, next_record_index) =
            face_recipe_next_boundary(bytes, program_start, header.record_index, next_byte_offset)?;
        offsets[4] = immediate_next;
        (offsets, next_record_index)
    };
    let recipe_start = u64::try_from(offsets[3]).ok()?;
    let mut indexed = [("", 0u32); 5];
    for (offset, slot) in offsets.iter().zip(&mut indexed) {
        if *offset == bytes.len() {
            continue;
        }
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, *offset, 0..=2000, u8::is_ascii_graphic)?;
        *slot = (class_tag, View::u32_le_at(bytes, after_tag)?);
    }
    let recipe_record_index = header.record_index.checked_add(3)?;
    if indexed[0].1 != header.record_index
        || indexed[1].1 != header.record_index.checked_add(1)?
        || indexed[2].1 != header.record_index.checked_add(2)?
        || indexed[3].1 != recipe_record_index
        || indexed[4].1 != next_record_index
    {
        return None;
    }
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let mut matches = recipes.iter().filter(|recipe| {
        recipe.byte_offset > recipe_start
            && recipe.byte_offset < next_byte_offset
            && (registered_frame.is_some()
                || matches!(
                    recipe.kind,
                    ConstructionRecipeKind::Face | ConstructionRecipeKind::BoundedFace
                ))
            && native_stream(&recipe.id) == Some(stream)
    });
    let recipe = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let family_name_len = match recipe.kind {
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        _ => return None,
    };
    if registered_frame.is_some() {
        let family = match recipe.kind {
            ConstructionRecipeKind::Face => b"face_recipe_data".as_slice(),
            ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".as_slice(),
            _ => return None,
        };
        let at = usize::try_from(recipe.byte_offset).ok()?;
        let end = at.checked_add(family.len())?;
        if end >= offsets[4] || bytes.get(at..end)? != family {
            return None;
        }
    }
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        family_name_len,
    )?;
    let recipe_references =
        decode_recipe_references(recipe_prefix_bytes, u64::try_from(recipe_prefix_at).ok()?);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(family_name_len)?;
    let recipe_program_end = usize::try_from(next_byte_offset).ok()?;
    if recipe_program_end.checked_sub(recipe_program_at)? > 64 * 1024 {
        return None;
    }
    let recipe_program = contiguous_i32_program(bytes, recipe_program_at, recipe_program_end)?;
    let recipe_prefix_bytes = recipe_prefix_bytes.to_vec();
    let program_kind = face_recipe_program_kind(&recipe_program)?;
    let recipe_program_offset = u64::try_from(recipe_program_at).ok()?;
    let recipe_nodes =
        match face_recipe_nodes(ctx, &recipe_program, recipe_program_at, program_kind) {
            Ok(Some(nodes)) => nodes,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
    let recipe_id = recipe.id.clone();
    let id = design_record_id(
        stream
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .unwrap_or(stream),
        ":design-face-operand#",
        header.byte_offset,
    );
    DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
        id,
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        group: group_ownership.map(|(group_record_index, group_member_ordinal)| {
            crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index,
                group_member_ordinal,
            }
        }),
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: crate::design::decode::text::class_tag_from_view(indexed[0].0).ok()?,
        recipe_record_index,
        recipe_record_byte_offset: recipe_start,
        recipe_id,
        recipe_prefix_offset: u64::try_from(recipe_prefix_at).ok()?,
        recipe_prefix_bytes,
        recipe_references,
        recipe_kind: recipe.kind,
        recipe_program_offset,
        recipe_program,
        recipe_nodes,
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index,
        next_byte_offset,
    })
    .and_then(|operand| {
        operand.with_registered_recipe_frame(registered_frame.and_then(|frame| frame.recipe_frame))
    })
    .ok()
    .map(Ok)
}

pub(in crate::design) fn has_typed_edge_treatment_group(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> bool {
    matches!(
        design_feature_family(kind),
        Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
    )
}

/// Apply the selection-identity completeness rule to a parsed group candidate.
///
/// A localized edge-treatment reference table can contain selections that do
/// not use counted groups. Such a reference is a group only when its parsed
/// candidate also resolves through one of the selection-identity grammars.
pub(crate) fn construction_operand_group_is_retained(
    scope: Option<&crate::records::feature::scope::DesignParameterScope>,
    has_selection_identity: bool,
) -> bool {
    !scope.is_some_and(crate::design::is_localized_edge_treatment) || has_selection_identity
}
