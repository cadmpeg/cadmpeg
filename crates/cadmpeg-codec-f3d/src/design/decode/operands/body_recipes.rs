// SPDX-License-Identifier: Apache-2.0
//! Body recipe operands and frames.

use crate::bytes::{is_guid_relaxed, lp_utf16_bounded};
use crate::container::ContainerScan;
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::design::decode::text::design_record_id;
use crate::ids::native_stream;
use crate::layout::{
    body_recipe_family_name_frame as body_recipe_end,
    indexed_design_record_header as indexed_header,
};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::sketch_links::PersistentSubentityTag;
use crate::records::topology::body_recipe::{
    DesignBodyRecipeOperand, DesignBodyRecipeReference, DesignOperandOwner,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignExtrudeOperandRole;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

pub(crate) fn body_recipe_serialization_form_losses(
    operands: &[DesignBodyRecipeOperand],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    operands.iter().filter_map(|operand| operand.serialization_form().filter(|form| *form > 1).map(|form| {
        crate::loss::F3dLossCode::BodyRecipeSerializationFormAssumed.note(format!(
            "body recipe operand {} stores serialization form {form}; its reference runs decode with the layout of forms 0 and 1", operand.id
        ))
    })).collect()
}

/// Decode whole-body construction operands that contain one persistent body recipe.
pub(crate) fn decode_body_recipe_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignBodyRecipeOperand>, CodecError> {
    let mut headers_by_identity = HashMap::<_, Option<&DesignRecordHeader>>::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        if let Some(existing) = headers_by_identity.get_mut(&key) {
            *existing = None;
        } else {
            headers_by_identity.insert(key, Some(header));
        }
    }
    let mut body_recipes_by_stream = HashMap::<_, Vec<&ConstructionRecipe>>::new();
    for recipe in recipes
        .iter()
        .filter(|recipe| recipe.kind == ConstructionRecipeKind::Body)
    {
        let Some(stream) = native_stream(&recipe.id) else {
            continue;
        };
        body_recipes_by_stream
            .entry(stream)
            .or_default()
            .push(recipe);
    }
    for stream_recipes in body_recipes_by_stream.values_mut() {
        stream_recipes.sort_by_key(|recipe| recipe.byte_offset);
    }
    let cut_scopes: HashMap<_, _> = scopes
        .iter()
        .filter(|scope| scope.is_legacy_class_330_blind_cut())
        .map(|scope| ((native_stream(&scope.id), scope.record_index), scope))
        .collect();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        if scopes.iter().any(|scope| {
            scope.record_index == group.scope_record_index
                && native_stream(&scope.id) == Some(stream)
                && scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
        }) {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = scan.design_record_offsets(&entry.name)?;
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(Some(header)) = headers_by_identity.get(&(stream, record_index)) else {
                continue;
            };
            let mut frame = BodyRecipeFrame::Standard;
            if group.extrude_role() == Some(DesignExtrudeOperandRole::Bodies)
                && group.scope_reference_ordinal == 0
                && group.members().len() == 1
                && header.class_tag.as_str() == "311"
            {
                if let Some(scope) = cut_scopes.get(&(Some(stream), group.scope_record_index)) {
                    let Some(closing_index) =
                        scope.legacy_class_330_body_recipe_close(group.record_index, record_index)
                    else {
                        continue;
                    };
                    let Some(legacy) =
                        BodyRecipeFrame::legacy_cut(bytes, &records, header, closing_index)
                    else {
                        continue;
                    };
                    frame = legacy;
                }
            }
            let Some(recipe) = unique_body_recipe_with_index(
                bytes,
                &records,
                header,
                frame,
                body_recipes_by_stream
                    .get(stream)
                    .map_or(&[], Vec::as_slice),
            ) else {
                continue;
            };
            if let Some(operand) = parse_body_recipe_operand_with_index(
                bytes, &records, group, ordinal, header, recipe, frame,
            ) {
                push_body_recipe_operand(&mut out, operand, &entry.name, header.byte_offset);
            }
        }
    }
    for scope in scopes.iter().filter(|scope| {
        scope.combine_operation().is_some()
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = scan.design_record_offsets(&entry.name)?;
        let operation = scope.combine_operation();
        let record_indexes = operation
            .into_iter()
            .flat_map(|operation| {
                std::iter::once(&operation.target_record_index)
                    .chain(operation.tools.iter().map(|tool| &tool.record_index))
            })
            .chain(
                scope
                    .reference_members()
                    .values()
                    .filter(|_| operation.is_none()),
            );
        for record_index in record_indexes {
            let mut ordinals = scope
                .reference_members()
                .values()
                .enumerate()
                .filter(|(_, member)| *member == record_index)
                .filter_map(|(ordinal, _)| u32::try_from(ordinal).ok());
            let Some(scope_reference_ordinal) = ordinals.next() else {
                continue;
            };
            if ordinals.next().is_some()
                || scope
                    .combine_operation()
                    .is_some_and(|_| scope_reference_ordinal.is_multiple_of(2))
            {
                continue;
            }
            let Some(Some(header)) = headers_by_identity.get(&(stream, *record_index)) else {
                continue;
            };
            let Some(recipe) = unique_body_recipe_with_index(
                bytes,
                &records,
                header,
                BodyRecipeFrame::Standard,
                body_recipes_by_stream
                    .get(stream)
                    .map_or(&[], Vec::as_slice),
            ) else {
                continue;
            };
            let owner = DesignOperandOwner::ScopeReference {
                scope_reference_ordinal,
            };
            if let Some(operand) = parse_body_recipe_operand_frame_with_index(
                bytes,
                &records,
                scope.record_index,
                owner,
                header,
                recipe,
                BodyRecipeFrame::Standard,
            ) {
                push_body_recipe_operand(&mut out, operand, &entry.name, header.byte_offset);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    let mut owner_counts = HashMap::<String, u32>::new();
    for operand in &out {
        if let Some(count) = owner_counts.get_mut(&operand.id) {
            // Only a count of exactly one is kept, so a count that cannot
            // grow further already means more than one.
            if let Some(next) = count.checked_add(1) {
                *count = next;
            }
        } else {
            owner_counts.insert(operand.id.clone(), 1_u32);
        }
    }
    out.retain(|operand| owner_counts.get(&operand.id) == Some(&1));
    Ok(out)
}

pub(super) fn push_body_recipe_operand(
    out: &mut Vec<DesignBodyRecipeOperand>,
    mut operand: DesignBodyRecipeOperand,
    stream: &str,
    offset: u64,
) {
    operand.id = design_record_id(stream, ":design-body-recipe-operand#", offset);

    out.push(operand);
}

#[derive(Clone, Copy)]
pub(super) enum BodyRecipeFrame {
    Standard,
    LegacyCut {
        prologue_end: usize,
        next_index: u32,
        next_at: usize,
    },
}

impl BodyRecipeFrame {
    pub(super) fn legacy_cut(
        bytes: &[u8],
        records: &IndexedRecordOffsets,
        header: &DesignRecordHeader,
        next_index: u32,
    ) -> Option<Self> {
        if header.class_tag.as_str() != "311" || header.record_index.checked_add(3)? != next_index {
            return None;
        }
        let start = usize::try_from(header.byte_offset).ok()?;
        let [primary, paired] = records.offsets(header.record_index) else {
            return None;
        };
        let [first] = records.offsets(header.record_index.checked_add(1)?) else {
            return None;
        };
        let [nested] = records.offsets(header.record_index.checked_add(2)?) else {
            return None;
        };
        let next_at = records.first_at_or_after(*nested, next_index)?;
        let expected = [
            (*paired, b"262"),
            (*first, b"328"),
            (*nested, b"301"),
            (next_at, b"291"),
        ];
        if *primary != start
            || [start, *paired, *first, *nested, next_at]
                .windows(2)
                .any(|pair| {
                    pair[0]
                        .checked_add(indexed_header::LEN)
                        .is_none_or(|end| end > pair[1])
                })
            || expected.into_iter().any(|(offset, class)| {
                offset
                    .checked_add(indexed_header::CLASS_TAG)
                    .and_then(|tag| bytes.get(tag..tag + class.len()))
                    != Some(class.as_slice())
            })
        {
            return None;
        }
        Some(Self::LegacyCut {
            prologue_end: nested.checked_add(indexed_header::LEN)?,
            next_index,
            next_at,
        })
    }
}

/// Select the sole body recipe in the structural interval after the `N+3`
/// header and before the enclosing `N+4` header. The bounded Design stream,
/// rather than an arbitrary byte distance, limits the interval. A stream with
/// no `N+4` header after the prologue admits the first body recipe that
/// `body_recipe_operand_end_with_index` closes.
#[cfg(test)]
pub(super) fn unique_body_recipe<'a>(
    bytes: &[u8],
    header: &DesignRecordHeader,
    recipes: &'a [&'a ConstructionRecipe],
) -> Option<&'a ConstructionRecipe> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    unique_body_recipe_with_index(bytes, &records, header, BodyRecipeFrame::Standard, recipes)
}

pub(super) fn unique_body_recipe_with_index<'a>(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    header: &DesignRecordHeader,
    frame: BodyRecipeFrame,
    recipes: &'a [&'a ConstructionRecipe],
) -> Option<&'a ConstructionRecipe> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let (prologue_end, next_at) = match frame {
        BodyRecipeFrame::Standard => {
            let end = body_recipe_prologue_end_with_index(records, start, header.record_index)?;
            (
                end,
                records.first_at_or_after(end, header.record_index.checked_add(4)?),
            )
        }
        BodyRecipeFrame::LegacyCut {
            prologue_end,
            next_at,
            ..
        } => (prologue_end, Some(next_at)),
    };
    let lower = u64::try_from(prologue_end).ok()?;
    let first = recipes.partition_point(|recipe| recipe.byte_offset < lower);
    let Some(next_at) = next_at else {
        let recipe = recipes.get(first)?;
        let recipe_at = usize::try_from(recipe.byte_offset).ok()?;
        body_recipe_operand_end_with_index(
            bytes,
            records,
            prologue_end,
            header.record_index,
            recipe_at,
        )?;
        return Some(recipe);
    };
    let upper = u64::try_from(next_at).ok()?;
    let matching = &recipes[first..recipes.partition_point(|recipe| recipe.byte_offset < upper)];
    let [recipe] = matching else {
        return None;
    };
    Some(recipe)
}

/// Offset past the four consecutively indexed records that open a body-recipe
/// operand, or `None` when the records after `start` do not carry
/// `record_index` through `record_index + 3` in order.
///
/// The prologue depends only on the operand header, so a caller weighing many
/// candidate recipes against one header resolves it once.
#[cfg(test)]
pub(super) fn body_recipe_prologue_end(
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Option<usize> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    body_recipe_prologue_end_with_index(&records, start, record_index)
}

pub(super) fn body_recipe_prologue_end_with_index(
    records: &IndexedRecordOffsets,
    start: usize,
    record_index: u32,
) -> Option<usize> {
    let mut search = start.checked_add(11)?;
    for expected in [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ] {
        let at = records.first_at_or_after(search, expected)?;
        search = at.checked_add(11)?;
    }
    Some(search)
}

/// Record index and offset of the indexed header that closes a body-recipe
/// operand whose recipe sits at `recipe_at`. The recipe must follow the
/// prologue that ends at `prologue_end`.
///
/// The header is record `record_index + 4` when the stream stores that record
/// after the prologue. Otherwise the recipe record is the only indexed record
/// after the prologue, and its successor must start at the recipe's fixed end.
#[cfg(test)]
pub(super) fn body_recipe_operand_end(
    bytes: &[u8],
    prologue_end: usize,
    record_index: u32,
    recipe_at: usize,
) -> Option<(u32, usize)> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    body_recipe_operand_end_with_index(bytes, &records, prologue_end, record_index, recipe_at)
}

pub(super) fn body_recipe_operand_end_with_index(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    prologue_end: usize,
    record_index: u32,
    recipe_at: usize,
) -> Option<(u32, usize)> {
    if recipe_at < prologue_end {
        return None;
    }
    let successor = record_index.checked_add(4)?;
    if records.first_at_or_after(prologue_end, successor).is_some() {
        return Some((successor, records.first_at_or_after(recipe_at, successor)?));
    }
    let recipe_record_at = next_indexed_record_offset(bytes, prologue_end)?;
    let close_at =
        next_indexed_record_offset(bytes, recipe_record_at.checked_add(indexed_header::LEN)?)?;
    if recipe_record_at >= recipe_at || close_at != recipe_at.checked_add(body_recipe_end::LEN)? {
        return None;
    }
    Some((
        View::u32_le_at(bytes, close_at.checked_add(indexed_header::RECORD_INDEX)?)?,
        close_at,
    ))
}

#[cfg(test)]
pub(super) fn parse_body_recipe_operand(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Option<DesignBodyRecipeOperand> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    parse_body_recipe_operand_with_index(
        bytes,
        &records,
        group,
        group_member_ordinal,
        header,
        recipe,
        BodyRecipeFrame::Standard,
    )
}

pub(super) fn parse_body_recipe_operand_with_index(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
    frame: BodyRecipeFrame,
) -> Option<DesignBodyRecipeOperand> {
    parse_body_recipe_operand_frame_with_index(
        bytes,
        records,
        group.scope_record_index,
        DesignOperandOwner::Group {
            group_record_index: group.record_index,
            group_member_ordinal,
        },
        header,
        recipe,
        frame,
    )
}

pub(super) fn parse_body_recipe_operand_frame_with_index(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    owner: DesignOperandOwner,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
    frame: BodyRecipeFrame,
) -> Option<DesignBodyRecipeOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let recipe_at = usize::try_from(recipe.byte_offset).ok()?;
    let (nested_delta, next_record_index, next_at) = match frame {
        BodyRecipeFrame::Standard => {
            let prologue_end =
                body_recipe_prologue_end_with_index(records, start, header.record_index)?;
            let (next, at) = body_recipe_operand_end_with_index(
                bytes,
                records,
                prologue_end,
                header.record_index,
                recipe_at,
            )?;
            (3, next, at)
        }
        BodyRecipeFrame::LegacyCut {
            prologue_end,
            next_index,
            next_at,
        } => {
            if recipe_at < prologue_end
                || recipe_at.checked_add(body_recipe_end::LEN)? != next_at
                || recipe.design.as_ref()?.selector?.value != next_index
            {
                return None;
            }
            (2, next_index, next_at)
        }
    };
    let reference_count = usize::try_from(View::u32_le_at(bytes, start + 21)?).ok()?;
    if (matches!(frame, BodyRecipeFrame::LegacyCut { .. }) && reference_count != 0)
        || start >= recipe_at
        || recipe_at >= next_at
        || bytes.get(start + 11..start + 21)? != [0; 10]
    {
        return None;
    }
    let mut cursor = start.checked_add(25)?;
    // Each reference consumes 12 bytes; a count the remaining bytes cannot
    // supply is corrupt and must not reach the allocator.
    if reference_count > bytes.len().checked_sub(cursor)? / 12 {
        return None;
    }

    let mut references = Vec::new();
    for _ in 0..reference_count {
        references.push(DesignBodyRecipeReference {
            design_reference: View::u64_le_at(bytes, cursor)?,
            design_reference_offset: u64::try_from(cursor).ok()?,
            form: View::u32_le_at(bytes, cursor + 8)?,
            form_offset: u64::try_from(cursor + 8).ok()?,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        });
        cursor = cursor.checked_add(12)?;
    }
    if bytes.get(cursor) != Some(&1)
        || bytes.get(cursor + 9..cursor + 11)? != [0; 2]
        || View::u32_le_at(bytes, cursor + 11)? != 1
    {
        return None;
    }
    let nested_record_index = View::u64_le_at(bytes, cursor + 1)?;
    let asset_id_at = cursor.checked_add(15)?;
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, asset_id_at, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let selector_tail_at = after_context_id.checked_add(4)?;
    let selector_tail: [u8; 4] = bytes
        .get(selector_tail_at..selector_tail_at.checked_add(4)?)?
        .try_into()
        .ok()?;
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
        || nested_record_index != u64::from(header.record_index.checked_add(nested_delta)?)
    {
        return None;
    }
    let recipe_id = recipe.id.clone();
    DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: String::new(),
            scope_record_index,
            owner,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            asset_id: asset_id.try_into().ok()?,
            asset_id_offset: u64::try_from(asset_id_at + 4).ok()?,
            context_id: context_id.try_into().ok()?,
            context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
            selector_tail: Some(crate::records::identity::Located {
                value: selector_tail,
                offset: u64::try_from(selector_tail_at).ok()?,
            }),
            references,
            nested_record_index,
            nested_record_index_offset: u64::try_from(cursor + 1).ok()?,
            recipe_id,
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index,
            next_byte_offset: u64::try_from(next_at).ok()?,
        },
    )
    .ok()
}

/// Join body-recipe Design references to solved persistent face tags.
pub(crate) fn bind_body_recipe_operand_candidates(
    operands: &mut [DesignBodyRecipeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
    scopes: &[DesignParameterScope],
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut recipes_by_id = HashMap::<_, Option<&ConstructionRecipe>>::new();
    for recipe in recipes {
        if let Some(existing) = recipes_by_id.get_mut(recipe.id.as_str()) {
            *existing = None;
        } else {
            recipes_by_id.insert(recipe.id.as_str(), Some(recipe));
        }
    }
    for operand in operands {
        // The recipe selector is a persistent-tag selector for Combine's
        // form-three clauses. In the class-365 body-member grammar it names
        // the enclosing N+4 record instead, so each clause joins by its own
        // Design reference and the history pass performs the body proof.
        let form_three_uses_recipe_selector = {
            let mut matching_scopes = scopes.iter().filter(|scope| {
                scope.record_index == operand.scope_record_index
                    && native_stream(&scope.id) == native_stream(&operand.id)
            });
            match (matching_scopes.next(), matching_scopes.next()) {
                (Some(scope), None) => scope.combine_operation().is_some(),
                _ => true,
            }
        };
        let tag_selector = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe)
            .and_then(|recipe| recipe.design.as_ref()?.selector)
            .map(|selector| i64::from(selector.value));
        let operand_id = operand.id.clone();
        for reference in operand.reference_bindings_mut() {
            reference.candidate_faces.clear();
            let Ok(design_reference) = i64::try_from(reference.design_reference) else {
                continue;
            };
            for tag in tags.iter().filter(|tag| {
                tag.design_references.contains(&design_reference)
                    && crate::ids::same_native_occurrence(&tag.id, &operand_id)
                    && (reference.form != 3
                        || !form_three_uses_recipe_selector
                        || tag_selector == Some(tag.selector))
            }) {
                if let AttributeTarget::Face(face) = &tag.target {
                    reference.candidate_faces.push(face.clone());
                }
            }
            reference
                .candidate_faces
                .sort_by(|left, right| left.as_str().cmp(right.as_str()));
            reference.candidate_faces.dedup();
        }
    }
}
