// SPDX-License-Identifier: Apache-2.0
//! Parse exact legacy As-built assembly alignment frames.

use crate::design::decode::byte_fields::bytes_at;
use crate::design::decode::operands::{parse_entity_selection_prefix, parse_face_operand};
use crate::design::decode::text::retain_class_tag;
use crate::layout::assembly_as_built_421_frame_297 as as_built_421_frame_297;
use crate::layout::assembly_as_built_421_frame_327 as as_built_421_frame_327;
use crate::layout::assembly_as_built_421_frame_376 as as_built_421_frame_376;
use crate::layout::assembly_as_built_421_frame_448 as as_built_421_frame_448;
use crate::layout::assembly_as_built_421_scope as as_built_421;
use crate::records::{
    decal::DesignRecordHeader,
    feature::{
        assembly::{
            DesignAssemblyLegacyOperand, DesignAssemblyLegacyOperands,
            DesignAssemblyLegacySelection, DesignAssemblyLimits, DesignAssemblyLimitsWire,
            DesignAssemblySolvedFrame,
        },
        scope::{DesignParameterScope, DesignScopePayload},
    },
    parameters::DesignParameterOwner,
    recipes::ConstructionRecipe,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

use super::scopes::hole::exact_hole_construction;
use super::scopes::point_data::exact_point_data_construction;
use super::scopes::shared_frames::exact_indexed_header_at;
use super::scopes::shared_frames::marked_record_reference;
use super::scopes::shared_frames::rigid_transform_at;
use super::sketch::IndexedRecordOffsets;

pub(in crate::design::decode) struct LegacyAsBuilt421Alignment {
    pub(super) angle: f64,
    pub(super) offset: [f64; 3],
    pub(super) owners: Vec<crate::records::identity::Located<u32>>,
    pub(super) limits: DesignAssemblyLimits,
}

/// Whether `scope` is an `As-built` scope. The payload names the kind without
/// copying its native name.
fn is_as_built(scope: &DesignParameterScope) -> bool {
    matches!(scope.payload(), DesignScopePayload::AsBuilt(_))
}

/// Whether the fixed 421-byte `As-built` scope frame at `start` stores the
/// eleven `references` at their stated offsets, the reference trailer and the
/// kind length. The test reads a constant number of bytes.
fn as_built_421_scope_frame(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
    references: &[crate::records::identity::Located<u32>; 11],
) -> Option<()> {
    let paired_start = usize::try_from(scope.paired_byte_offset()).ok()?;
    if paired_start != start.checked_add(as_built_421::LEN)? {
        return None;
    }
    let reference_count_at = start.checked_add(as_built_421::REFERENCE_COUNT)?;
    let feature_ordinal_at = start.checked_add(as_built_421::FEATURE_ORDINAL)?;
    if scope.reference_count_offset() != u64::try_from(reference_count_at).ok()?
        || View::u32_le_at(bytes, reference_count_at)? != as_built_421::REFERENCE_COUNT_VALUE
        || bytes.get(
            start.checked_add(as_built_421::KIND_LENGTH)?..start.checked_add(as_built_421::KIND)?,
        ) != Some(&as_built_421::KIND_LENGTH_VALUE.to_le_bytes()[..])
        || scope.feature_ordinal_offset() != u64::try_from(feature_ordinal_at).ok()?
        || bytes_at::<4>(bytes, start.checked_add(as_built_421::REFERENCE_TRAILER)?)
            != Some(&as_built_421::REFERENCE_TRAILER_VALUE)
    {
        return None;
    }
    let mut reference_at = start.checked_add(as_built_421::REFERENCE_ENTRIES)?;
    for reference in references {
        if marked_record_reference(bytes, reference_at)? != reference.value
            || reference.offset != u64::try_from(reference_at.checked_add(1)?).ok()?
        {
            return None;
        }
        reference_at = reference_at.checked_add(11)?;
    }
    Some(())
}

pub(super) fn exact_legacy_as_built_421_alignment(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    lanes: &[&DesignParameterOwner],
) -> Result<Option<LegacyAsBuilt421Alignment>, cadmpeg_core::CodecError> {
    let Some(generation) = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    ) else {
        return Ok(None);
    };
    let Some(references) = scope
        .reference_members()
        .located_rows()
        .and_then(|rows| <&[_; 11]>::try_from(rows).ok())
    else {
        return Ok(None);
    };
    let Ok(lanes) = <&[&DesignParameterOwner; 6]>::try_from(lanes) else {
        return Ok(None);
    };
    if !is_as_built(scope) {
        return Ok(None);
    }
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    if as_built_421_scope_frame(bytes, scope, start, references).is_none() {
        return Ok(None);
    }
    for owner in lanes {
        if owner.frame_length() != 103
            || !ctx.equal_bytes(
                owner.class_tag().as_bytes(),
                generation.owner_class_tag().as_bytes(),
                "match F3D legacy AsBuilt alignment owner class",
            )?
        {
            return Ok(None);
        }
    }
    let [offset_x, offset_y, offset_z, angle, limit_first, limit_second] = lanes;
    let [_, _, _, _, x_reference, y_reference, z_reference, angle_reference, _, first_limit_reference, second_limit_reference] =
        references;
    if [x_reference, y_reference, z_reference, angle_reference].map(|reference| reference.value)
        != [offset_x, offset_y, offset_z, angle].map(|owner| owner.record_index())
        || [first_limit_reference, second_limit_reference].map(|reference| reference.value)
            != [limit_first, limit_second].map(|owner| owner.record_index())
    {
        return Ok(None);
    }
    let (minimum_owner, maximum_owner) = if generation.reverse_limit_order() {
        (limit_second, limit_first)
    } else {
        (limit_first, limit_second)
    };
    let Ok(limits) = DesignAssemblyLimits::try_from(DesignAssemblyLimitsWire {
        kind: generation.limit_kind(),
        minimum: minimum_owner.evaluated_value().get(),
        maximum: maximum_owner.evaluated_value().get(),
        owner_record_indices: [minimum_owner.record_index(), maximum_owner.record_index()],
        value_offsets: [
            minimum_owner.evaluated_value_offset(),
            maximum_owner.evaluated_value_offset(),
        ],
    }) else {
        return Ok(None);
    };
    let owners = ctx.collect_vec(
        [angle, offset_x, offset_y, offset_z]
            .into_iter()
            .map(|owner| crate::records::identity::Located {
                value: owner.record_index(),
                offset: owner.evaluated_value_offset(),
            }),
        "f3d legacy AsBuilt alignment owners",
    )?;
    Ok(Some(LegacyAsBuilt421Alignment {
        angle: angle.evaluated_value().get(),
        offset: [
            offset_x.evaluated_value().get(),
            offset_y.evaluated_value().get(),
            offset_z.evaluated_value().get(),
        ],
        owners,
        limits,
    }))
}

pub(super) fn exact_legacy_as_built_421_solved_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignAssemblySolvedFrame>, CodecError> {
    let Some(generation) = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    ) else {
        return Ok(None);
    };
    let Some([_, _, _, _, _, _, _, _, frame_reference, _, _]) =
        scope.reference_members().located_rows()
    else {
        return Ok(None);
    };
    if !is_as_built(scope) {
        return Ok(None);
    }
    let frame_record_index = frame_reference.value;
    let expected_class_tag = generation.frame_class_tag();
    // The frame is the only header of its record index carrying the
    // generation's frame class.
    let offsets = records.offsets(frame_record_index);
    let is_frame = |frame_start: &usize| {
        Ok(
            exact_indexed_header_at(bytes, *frame_start, frame_record_index)
                .is_some_and(|tag| tag.as_slice() == expected_class_tag.as_bytes()),
        )
    };
    let operation = "find F3D As-built frame record";
    let Some(first) = ctx.position_by(offsets, is_frame, operation)? else {
        return Ok(None);
    };
    let (Some(&frame_start), Some(later)) = (offsets.get(first), offsets.get(first + 1..)) else {
        return Ok(None);
    };
    if ctx.any_by(later, is_frame, operation)? {
        return Ok(None);
    }
    let matrix_prefix_value = match generation {
        crate::design::assembly::LegacyAsBuilt421Generation::Class364 => {
            as_built_421_frame_376::MATRIX_PREFIX_VALUE
        }
        crate::design::assembly::LegacyAsBuilt421Generation::Class420 => {
            as_built_421_frame_327::MATRIX_PREFIX_VALUE
        }
        crate::design::assembly::LegacyAsBuilt421Generation::Class417 => {
            as_built_421_frame_448::MATRIX_PREFIX_VALUE
        }
        crate::design::assembly::LegacyAsBuilt421Generation::Class457 => {
            as_built_421_frame_297::MATRIX_PREFIX_VALUE
        }
    };
    let Some(paired_class_tag) = frame_start
        .checked_add(generation.frame_length())
        .and_then(|paired_at| exact_indexed_header_at(bytes, paired_at, frame_record_index))
    else {
        return Ok(None);
    };
    let Some(transform_at) = frame_start.checked_add(generation.matrix_offset()) else {
        return Ok(None);
    };
    if paired_class_tag.as_slice() != generation.frame_paired_class_tag().as_bytes()
        || frame_start
            .checked_add(generation.matrix_prefix())
            .and_then(|prefix_at| bytes_at::<4>(bytes, prefix_at))
            != Some(&matrix_prefix_value)
    {
        return Ok(None);
    }
    let (Some(transform), Ok(record_byte_offset), Ok(transform_offset)) = (
        rigid_transform_at(bytes, transform_at),
        u64::try_from(frame_start),
        u64::try_from(transform_at),
    ) else {
        return Ok(None);
    };
    let Some(frame_class_tag) = expected_class_tag.as_bytes().first_chunk::<3>() else {
        return Ok(None);
    };
    let class_tag = retain_class_tag(ctx, *frame_class_tag, "copy F3D As-built frame class tag")?;
    Ok(Some(DesignAssemblySolvedFrame {
        reference_record_index: frame_record_index,
        reference_offset: frame_reference.offset,
        record_byte_offset,
        class_tag,
        transform,
        transform_offset,
    }))
}

/// Maximum component error accepted when a legacy hole direction is compared
/// with the solved connector frame's third basis column.
const EPS_LEGACY_AS_BUILT_DIRECTION: f64 = 1.0e-10;

/// Decode the two ordered construction/face-selection pairs of a 421-byte
/// `As-built` scope and derive their local frames from the stored solved frame.
pub(super) fn exact_legacy_as_built_421_operands(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
    recipes: &[ConstructionRecipe],
    solved_frame: &DesignAssemblySolvedFrame,
) -> Result<Option<DesignAssemblyLegacyOperands>, cadmpeg_core::CodecError> {
    let Some(generation) = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    ) else {
        return Ok(None);
    };
    let Some(
        [point_reference, first_selection_reference, hole_reference, second_selection_reference, _, _, _, _, frame_reference, _, _],
    ) = scope.reference_members().located_rows()
    else {
        return Ok(None);
    };
    if !is_as_built(scope) || solved_frame.reference_record_index != frame_reference.value {
        return Ok(None);
    }
    let first_selection_record_index = first_selection_reference.value;
    let second_selection_record_index = second_selection_reference.value;
    let Some(point) = exact_point_data_construction(
        ctx,
        bytes,
        records,
        std::iter::once(point_reference.value),
        stream_types,
    )?
    else {
        return Ok(None);
    };
    let Some(hole) = exact_hole_construction(
        ctx,
        bytes,
        records,
        scope,
        stream_types,
        &crate::records::feature::scope::DesignFeatureKind::AsBuilt,
    )?
    else {
        return Ok(None);
    };
    if hole.point_record_index != hole_reference.value
        || !matches!(
            hole.input_records.as_slice(),
            [input] if input.value == second_selection_record_index
        )
        || !matches!(
            point.rule.inputs(),
            [input] if input.record_index() == first_selection_record_index
        )
    {
        return Ok(None);
    }
    let solved_direction = [
        solved_frame.transform[0][2],
        solved_frame.transform[1][2],
        solved_frame.transform[2][2],
    ];
    if hole
        .direction
        .into_iter()
        .zip(solved_direction)
        .any(|(actual, expected)| (actual.get() - expected).abs() > EPS_LEGACY_AS_BUILT_DIRECTION)
    {
        return Ok(None);
    }
    let selection_class_tag = match generation {
        crate::design::assembly::LegacyAsBuilt421Generation::Class364 => b"307",
        crate::design::assembly::LegacyAsBuilt421Generation::Class420 => b"273",
        crate::design::assembly::LegacyAsBuilt421Generation::Class417 => b"332",
        crate::design::assembly::LegacyAsBuilt421Generation::Class457 => b"264",
    };
    let Some(first_selection) = exact_legacy_as_built_face_selection(
        ctx,
        bytes,
        records,
        scope,
        1,
        (first_selection_record_index, selection_class_tag),
        recipes,
    )?
    else {
        return Ok(None);
    };
    let Some(second_selection) = exact_legacy_as_built_face_selection(
        ctx,
        bytes,
        records,
        scope,
        3,
        (second_selection_record_index, selection_class_tag),
        recipes,
    )?
    else {
        return Ok(None);
    };
    let (Some(point_class_tag), Some(hole_class_tag)) = (
        indexed_class_at(bytes, point.point_record_byte_offset),
        indexed_class_at(bytes, hole.point_record_byte_offset),
    ) else {
        return Ok(None);
    };
    let point_class_tag = retain_class_tag(ctx, *point_class_tag, "copy F3D indexed class tag")?;
    let hole_class_tag = retain_class_tag(ctx, *hole_class_tag, "copy F3D indexed class tag")?;
    Ok(Some(DesignAssemblyLegacyOperands::new(
        DesignAssemblyLegacyOperand {
            construction_class_tag: point_class_tag,
            construction: Box::new(point),
            selection: first_selection,
            reference_offset: point_reference.offset,
        },
        DesignAssemblyLegacyOperand {
            construction_class_tag: hole_class_tag,
            construction: Box::new(hole),
            selection: second_selection,
            reference_offset: hole_reference.offset,
        },
    )))
}

/// The three-digit class tag of the indexed header at `byte_offset`: a `u32`
/// length of three followed by three ASCII digits. The test reads seven
/// bytes.
fn indexed_class_at(bytes: &[u8], byte_offset: u64) -> Option<&[u8; 3]> {
    let start = usize::try_from(byte_offset).ok()?;
    if View::u32_le_at(bytes, start)? != 3 {
        return None;
    }
    let class_tag = bytes_at::<3>(bytes, start.checked_add(4)?)?;
    class_tag
        .iter()
        .all(u8::is_ascii_digit)
        .then_some(class_tag)
}

/// The `record_index` value of the scope reference after the one at
/// `scope_reference_ordinal`.
fn next_reference_value(scope: &DesignParameterScope, scope_reference_ordinal: u32) -> Option<u32> {
    let ordinal = usize::try_from(scope_reference_ordinal)
        .ok()?
        .checked_add(1)?;
    let members = scope.reference_members();
    match (members.unlocated_values(), members.located_rows()) {
        (Some(values), _) => values.get(ordinal).copied(),
        (None, Some(rows)) => rows.get(ordinal).map(|row| row.value),
        (None, None) => None,
    }
}

fn exact_legacy_as_built_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    (record_index, expected_class_tag): (u32, &[u8; 3]),
    recipes: &[ConstructionRecipe],
) -> Result<Option<DesignAssemblyLegacySelection>, cadmpeg_core::CodecError> {
    let Some(scope_start) = usize::try_from(scope.byte_offset()).ok() else {
        return Ok(None);
    };
    // The selection ends at the first header of the next scope reference
    // after the scope.
    let next_byte_offset = match (
        next_reference_value(scope, scope_reference_ordinal),
        scope_start.checked_add(1),
    ) {
        (Some(next_record_index), Some(after_scope)) => records
            .first_at_or_after(ctx, after_scope, next_record_index)?
            .and_then(|offset| u64::try_from(offset).ok()),
        _ => None,
    };
    let selection_at = |byte_offset: usize| {
        exact_legacy_as_built_selection_at(
            ctx,
            bytes,
            records,
            scope,
            scope_reference_ordinal,
            (record_index, expected_class_tag),
            byte_offset,
            next_byte_offset,
            recipes,
        )
    };
    // Exactly one header of the record index must carry a complete selection.
    let offsets = records.offsets(record_index);
    let operation = "find F3D legacy AsBuilt face selection";
    let mut found = None;
    let Some(first) = ctx.position_by(
        offsets,
        |byte_offset| {
            found = selection_at(*byte_offset)?;
            Ok(found.is_some())
        },
        operation,
    )?
    else {
        return Ok(None);
    };
    let later = offsets.get(first + 1..).unwrap_or(&[]);
    if ctx.any_by(
        later,
        |byte_offset| Ok(selection_at(*byte_offset)?.is_some()),
        operation,
    )? {
        return Ok(None);
    }
    Ok(found)
}

/// The legacy As-built face selection whose indexed header is at
/// `byte_offset`, when that header carries `expected_class_tag` and the face
/// operand and entity-selection prefix after it are complete.
#[allow(clippy::too_many_arguments)]
fn exact_legacy_as_built_selection_at(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    (record_index, expected_class_tag): (u32, &[u8; 3]),
    byte_offset: usize,
    next_byte_offset: Option<u64>,
    recipes: &[ConstructionRecipe],
) -> Result<Option<DesignAssemblyLegacySelection>, cadmpeg_core::CodecError> {
    let Ok(header_offset) = u64::try_from(byte_offset) else {
        return Ok(None);
    };
    if indexed_class_at(bytes, header_offset) != Some(expected_class_tag) {
        return Ok(None);
    }
    let id = ctx.copy_retained_text(&scope.id, "f3d legacy AsBuilt selection header ID")?;
    let class_tag = retain_class_tag(
        ctx,
        *expected_class_tag,
        "copy F3D As-built selection class tag",
    )?;
    let header = DesignRecordHeader {
        id,
        record_index,
        class_tag,
        byte_offset: header_offset,
    };
    let Some(operand) = parse_face_operand(
        ctx,
        bytes,
        records,
        crate::design::decode::operands::FaceOperandFrame {
            scope,
            scope_reference_ordinal,
            group_ownership: None,
            next_byte_offset,
            header: &header,
        },
        recipes,
    )?
    else {
        return Ok(None);
    };
    let Some(prefix) = parse_entity_selection_prefix(ctx, bytes, byte_offset, record_index)? else {
        return Ok(None);
    };
    let (Ok(asset_id), Ok(context_id)) = (prefix.asset_id.try_into(), prefix.context_id.try_into())
    else {
        return Ok(None);
    };
    let next_byte_offset = operand.next_byte_offset();
    Ok(Some(DesignAssemblyLegacySelection {
        record_index,
        byte_offset: header_offset,
        class_tag: header.class_tag,
        asset_id,
        asset_id_offset: prefix.asset_id_offset,
        context_id,
        context_id_offset: prefix.context_id_offset,
        recipe_record_index: operand.recipe_record_index(),
        recipe_record_byte_offset: operand.recipe_record_byte_offset(),
        recipe_id: operand.recipe_id,
        recipe_kind: operand.recipe_kind,
        recipe_references: operand.recipe_references,
        next_byte_offset,
    }))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use super::exact_legacy_as_built_face_selection;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[test]
    fn legacy_as_built_selection_header_id_refuses_retained_limit() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"307");
        bytes.extend_from_slice(&77u32.to_le_bytes());
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#0",
            crate::records::feature::scope::DesignFeatureKind::AsBuilt,
            42,
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64_from_index(scope.id.len()) - 1;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = exact_legacy_as_built_face_selection(
            &ctx,
            &bytes,
            &records,
            &scope,
            0,
            (77, b"307"),
            &[],
        );
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "f3d legacy AsBuilt selection header ID"
        ));
    }

    #[test]
    fn legacy_as_built_selection_skips_other_classes_without_copies() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"308");
        bytes.extend_from_slice(&77u32.to_le_bytes());
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#0",
            crate::records::feature::scope::DesignFeatureKind::AsBuilt,
            42,
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(exact_legacy_as_built_face_selection(
            &ctx,
            &bytes,
            &records,
            &scope,
            0,
            (77, b"307"),
            &[],
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn legacy_as_built_selection_class_tag_copy_refuses_retained_bytes() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"307");
        bytes.extend_from_slice(&77u32.to_le_bytes());
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#0",
            crate::records::feature::scope::DesignFeatureKind::AsBuilt,
            42,
        );
        let refusal = crate::test_support::resource_refusal_at(
            ResourceDimension::RetainedBytes,
            "copy F3D As-built selection class tag",
            0,
            |ctx| {
                exact_legacy_as_built_face_selection(
                    ctx,
                    &bytes,
                    &records,
                    &scope,
                    0,
                    (77, b"307"),
                    &[],
                )
                .map(|_| ())
            },
        );
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "copy F3D As-built selection class tag"
                    && failure.additional == 3
        ));
    }
}
