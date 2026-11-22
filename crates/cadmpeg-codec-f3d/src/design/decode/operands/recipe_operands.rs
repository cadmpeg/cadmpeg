// SPDX-License-Identifier: Apache-2.0
//! Vertex, recipe, and edge operand parse.

use super::edge_operands::edge_recipe_terminal_delta;
use super::entity_selections::parse_entity_selection_frame;
use super::recipe_structure::{
    edge_recipe_local_topology_references, edge_recipe_structure, surface_patch_recipe_structure,
};
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::dimension_frames::{
    contiguous_i32_program, decode_recipe_references, recipe_record_prefix,
};
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::design::decode::text::design_record_id;
use crate::ids::native_stream;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignVertexRecipe;
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::edge_identity::DesignEdgeOperand;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) struct ParsedRecipeOperand {
    pub(super) paired_byte_offset: u64,
    pub(super) paired_class_tag: crate::records::references::DesignClassTag,
    pub(super) recipe_record_index: u32,
    pub(super) recipe_record_byte_offset: u64,
    pub(super) recipe_id: String,
    pub(super) recipe_prefix_offset: u64,
    pub(super) recipe_prefix_bytes: Vec<u8>,
    pub(super) recipe_references: Vec<crate::records::dimensions::DesignRecipeReference>,
    pub(super) recipe_program_offset: u64,
    pub(super) recipe_program: Vec<i32>,
    pub(super) next_record_index: u32,
    pub(super) next_byte_offset: u64,
}

#[derive(Clone, Copy)]
pub(super) enum RecipeOperandTerminator {
    RecordDelta(u32),
    NextIndexedAfterRecipe { limit: u64 },
}

/// Parse one exact persistent vertex-recipe envelope.
pub(super) fn parse_vertex_recipe(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<DesignVertexRecipe> {
    let parsed = parse_recipe_operand(
        bytes,
        records,
        stream,
        header,
        recipes,
        (
            ConstructionRecipeKind::Vertex,
            RecipeOperandTerminator::RecordDelta(5),
        ),
    )?;
    DesignVertexRecipe::try_new(
        crate::records::feature::work_geometry::DesignVertexRecipeDraft {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            paired_byte_offset: parsed.paired_byte_offset,
            paired_class_tag: parsed.paired_class_tag,
            recipe_record_index: parsed.recipe_record_index,
            recipe_record_byte_offset: parsed.recipe_record_byte_offset,
            recipe_id: parsed.recipe_id,
            recipe_prefix_offset: parsed.recipe_prefix_offset,
            recipe_prefix_bytes: parsed.recipe_prefix_bytes,
            recipe_references: parsed.recipe_references,
            recipe_program_offset: parsed.recipe_program_offset,
            recipe_program: parsed.recipe_program,
            resolution: None,
            next_record_index: parsed.next_record_index,
            next_byte_offset: parsed.next_byte_offset,
        },
    )
    .ok()
}

/// Parse the indexed-record envelope shared by topology recipe operands.
pub(super) fn parse_recipe_operand(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
    (recipe_kind, terminator): (ConstructionRecipeKind, RecipeOperandTerminator),
) -> Option<ParsedRecipeOperand> {
    let family_name = crate::design::RECIPES
        .iter()
        .find_map(|(name, kind)| (*kind == recipe_kind).then_some(*name))?;
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
    offsets[4] = match terminator {
        RecipeOperandTerminator::RecordDelta(delta) => {
            records.first_at_or_after(position, header.record_index.checked_add(delta)?)?
        }
        RecipeOperandTerminator::NextIndexedAfterRecipe { limit } => {
            let recipe_record_byte_offset = u64::try_from(offsets[3]).ok()?;
            let recipe = recipes
                .iter()
                .filter(|recipe| {
                    recipe.byte_offset > recipe_record_byte_offset
                        && recipe.byte_offset < limit
                        && recipe.kind == recipe_kind
                        && native_stream(&recipe.id) == Some(stream)
                })
                .min_by_key(|recipe| recipe.byte_offset)?;
            if recipe_kind == ConstructionRecipeKind::Edge
                && parse_entity_selection_frame(
                    bytes,
                    header.record_index,
                    header.byte_offset,
                    header.class_tag.as_str(),
                )
                .is_some_and(|selection| recipe.byte_offset >= selection.next_byte_offset)
            {
                return None;
            }
            let recipe_program_at = usize::try_from(recipe.byte_offset)
                .ok()?
                .checked_add(family_name.len())?;
            let next = next_indexed_record_offset(bytes, recipe_program_at)?;
            (u64::try_from(next).ok()? <= limit).then_some(next)?
        }
    };
    let mut indexed = [("", 0u32); 5];
    for (offset, slot) in offsets.iter().zip(&mut indexed) {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, *offset, 0..=2000, u8::is_ascii_graphic)?;
        *slot = (class_tag, View::u32_le_at(bytes, after_tag)?);
    }
    let recipe_record_index = header.record_index.checked_add(3)?;
    let expected_prefix = [
        header.record_index,
        header.record_index.checked_add(1)?,
        header.record_index.checked_add(2)?,
        recipe_record_index,
    ];
    if !indexed
        .iter()
        .take(4)
        .zip(expected_prefix)
        .all(|((_, actual), expected)| *actual == expected)
    {
        return None;
    }
    let next_record_index = indexed[4].1;
    if let RecipeOperandTerminator::RecordDelta(delta) = terminator {
        if next_record_index != header.record_index.checked_add(delta)? {
            return None;
        }
    }
    let recipe_record_byte_offset = u64::try_from(offsets[3]).ok()?;
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let mut matches = recipes.iter().filter(|recipe| {
        recipe.byte_offset > recipe_record_byte_offset
            && recipe.byte_offset < next_byte_offset
            && recipe.kind == recipe_kind
            && native_stream(&recipe.id) == Some(stream)
    });
    let recipe = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        family_name.len(),
    )?;
    let recipe_prefix_offset = u64::try_from(recipe_prefix_at).ok()?;
    let recipe_references = decode_recipe_references(recipe_prefix_bytes, recipe_prefix_offset);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(family_name.len())?;
    let recipe_program_end = usize::try_from(next_byte_offset).ok()?;
    if recipe_program_end.checked_sub(recipe_program_at)? > 64 * 1024 {
        return None;
    }
    let recipe_program = contiguous_i32_program(bytes, recipe_program_at, recipe_program_end)?;
    let recipe_prefix_bytes = recipe_prefix_bytes.to_vec();
    let recipe_id = recipe.id.clone();
    Some(ParsedRecipeOperand {
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: crate::design::decode::text::class_tag_from_view(indexed[0].0).ok()?,
        recipe_record_index,
        recipe_record_byte_offset,
        recipe_id,
        recipe_prefix_offset,
        recipe_prefix_bytes,
        recipe_references,
        recipe_program_offset: u64::try_from(recipe_program_at).ok()?,
        recipe_program,
        next_record_index,
        next_byte_offset,
    })
}

pub(super) fn parse_edge_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    (scope_reference_ordinal, header): (u32, &DesignRecordHeader),
    recipes: &[ConstructionRecipe],
    terminal_group_limit: Option<u64>,
) -> Option<Result<DesignEdgeOperand, CodecError>> {
    let next_record_delta = edge_recipe_terminal_delta(&scope.kind());
    let stream = native_stream(&scope.id)?;
    let parsed = parse_recipe_operand(
        bytes,
        records,
        stream,
        header,
        recipes,
        (
            ConstructionRecipeKind::Edge,
            RecipeOperandTerminator::RecordDelta(next_record_delta),
        ),
    )
    .or_else(|| {
        let limit = terminal_group_limit?;
        parse_recipe_operand(
            bytes,
            records,
            stream,
            header,
            recipes,
            (
                ConstructionRecipeKind::Edge,
                RecipeOperandTerminator::NextIndexedAfterRecipe { limit },
            ),
        )
    })?;
    let recipe_structure = match edge_recipe_structure(ctx, &parsed.recipe_program) {
        Ok(structure) => structure,
        Err(error) => return Some(Err(error)),
    };
    let surface_patch_recipe_structure =
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch {
            surface_patch_recipe_structure(
                &parsed.recipe_program,
                crate::records::dimensions::reference_lanes(&parsed.recipe_references).len(),
            )
        } else {
            None
        };
    let local_topology_references = recipe_structure.as_ref().and_then(|structure| {
        edge_recipe_local_topology_references(
            structure,
            crate::records::dimensions::reference_lanes(&parsed.recipe_references).len(),
        )
    });
    let id = design_record_id(
        stream
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .unwrap_or(stream),
        ":design-edge-operand#",
        header.byte_offset,
    );
    DesignEdgeOperand::try_new(
        crate::records::topology::edge_identity::DesignEdgeOperandDraft {
            id,
            scope_record_index: scope.record_index,
            scope_reference_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            paired_byte_offset: parsed.paired_byte_offset,
            paired_class_tag: parsed.paired_class_tag,
            recipe_record_index: parsed.recipe_record_index,
            recipe_record_byte_offset: parsed.recipe_record_byte_offset,
            recipe_id: parsed.recipe_id,
            recipe_prefix_offset: parsed.recipe_prefix_offset,
            recipe_prefix_bytes: parsed.recipe_prefix_bytes.into(),
            recipe_references: parsed.recipe_references,
            recipe_program_offset: parsed.recipe_program_offset,
            recipe_program: parsed.recipe_program,
            recipe_structure,
            surface_patch_recipe_structure,
            local_topology_references,
            candidate_faces: Vec::new(),
            result_candidate_faces: Vec::new(),
            result_boundary_edge_slots: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            terminal_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            preceding_boundary_edge_slots: Vec::new(),
            terminal_boundary_edge_slots: Vec::new(),
            changed_boundary_edge_slots: Vec::new(),
            deleted_boundary_edge_slots: Vec::new(),
            transition_deleted_edge_slots: Vec::new(),
            updated_boundary_edge_slots: Vec::new(),
            treatment_radius_candidates: Vec::new(),
            changed_boundary_edge_contexts: Vec::new(),
            terminal_boundary_edge_contexts: Vec::new(),
            terminal_reference_edge_slots: Vec::new(),
            recipe_reference_contexts: Vec::new(),
            recipe_selectors: Vec::new(),
            recipe_state_id: None,
            resolved_edge_slot: None,
            resolved_axis: None,
            next_record_index: parsed.next_record_index,
            next_byte_offset: parsed.next_byte_offset,
        },
    )
    .ok()
    .map(Ok)
}
