// SPDX-License-Identifier: Apache-2.0
//! Parse exact legacy As-built assembly alignment frames.

use crate::bytes::lp_ascii_filtered;
use crate::design::decode::operands::{parse_entity_selection_prefix, parse_face_operand};
use crate::layout::assembly_as_built_421_frame_297 as as_built_421_frame_297;
use crate::layout::assembly_as_built_421_frame_327 as as_built_421_frame_327;
use crate::layout::assembly_as_built_421_frame_376 as as_built_421_frame_376;
use crate::layout::assembly_as_built_421_frame_448 as as_built_421_frame_448;
use crate::layout::assembly_as_built_421_scope as as_built_421;
use crate::records::{
    ConstructionRecipe, DesignAssemblyLegacyOperand, DesignAssemblyLegacyOperands,
    DesignAssemblyLegacySelection, DesignAssemblyLimits,
    DesignAssemblySolvedFrame, DesignParameterOwner, DesignParameterScope, DesignRecordHeader,
    DesignWorkPointRule,
};
use cadmpeg_core::decode::View;
use std::collections::HashMap;

use super::scopes::{
    exact_hole_construction, exact_indexed_header_at, exact_point_data_construction,
    marked_record_reference, rigid_transform_at,
};
use super::sketch::IndexedRecordOffsets;

pub(crate) struct LegacyAsBuilt421Alignment {
    pub(crate) angle: f64,
    pub(crate) offset: [f64; 3],
    pub(crate) owners: Vec<crate::records::Located<u32>>,
    pub(crate) limits: DesignAssemblyLimits,
}

pub(crate) fn exact_legacy_as_built_421_alignment(
    bytes: &[u8],
    scope: &DesignParameterScope,
    lanes: &[&DesignParameterOwner],
) -> Option<LegacyAsBuilt421Alignment> {
    let generation = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    )?;
    let crate::records::ReferenceRun::Located(references) = &scope.reference_members else { return None; };
    if scope.kind() != crate::records::DesignFeatureKind::AsBuilt
        || lanes.len() != 6
        || references.len() != 11
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if usize::try_from(scope.paired_byte_offset).ok()? != start.checked_add(as_built_421::LEN)?
        || scope.reference_count_offset
            != u64::try_from(start.checked_add(as_built_421::REFERENCE_COUNT)?).ok()?
        || View::u32_le_at(bytes, start.checked_add(as_built_421::REFERENCE_COUNT)?)?
            != as_built_421::REFERENCE_COUNT_VALUE
        || View::u32_le_at(bytes, start.checked_add(as_built_421::KIND_LENGTH)?)?
            != as_built_421::KIND_LENGTH_VALUE
        || scope.feature_ordinal_offset
            != u64::try_from(start.checked_add(as_built_421::FEATURE_ORDINAL)?).ok()?
    {
        return None;
    }
    for (ordinal, reference) in references.iter().enumerate() {
        let reference_at = start
            .checked_add(as_built_421::REFERENCE_ENTRIES.checked_add(ordinal.checked_mul(11)?)?)?;
        if marked_record_reference(bytes, reference_at)? != reference.value
            || bytes.get(reference_at.checked_add(5)?..reference_at.checked_add(11)?)? != [0; 6]
            || reference.offset != u64::try_from(reference_at.checked_add(1)?).ok()?
        {
            return None;
        }
    }
    if bytes.get(
        start.checked_add(as_built_421::KIND_LENGTH)?..start.checked_add(as_built_421::KIND)?,
    )? != as_built_421::KIND_LENGTH_VALUE.to_le_bytes()
        || bytes.get(
            start.checked_add(as_built_421::REFERENCE_TRAILER)?
                ..start.checked_add(as_built_421::KIND_LENGTH)?,
        )? != as_built_421::REFERENCE_TRAILER_VALUE
    {
        return None;
    }
    if lanes
        .iter()
        .any(|owner| owner.class_tag != generation.owner_class_tag() || owner.frame_length != 103)
    {
        return None;
    }
    let [offset_x, offset_y, offset_z, angle, limit_first, limit_second] = lanes else {
        return None;
    };
    let alignment_owner_record_indices = [
        offset_x.record_index,
        offset_y.record_index,
        offset_z.record_index,
        angle.record_index,
    ];
    let source_limit_owner_record_indices = [limit_first.record_index, limit_second.record_index];
    if !scope.reference_members.values().skip(4).take(4).eq(alignment_owner_record_indices.iter())
        || !scope.reference_members.values().skip(9).take(2).eq(source_limit_owner_record_indices.iter())
    {
        return None;
    }
    let (minimum_owner, maximum_owner) = if generation.reverse_limit_order() {
        (limit_second, limit_first)
    } else {
        (limit_first, limit_second)
    };
    let kind = generation.limit_kind();
    let minimum = minimum_owner.evaluated_value;
    let maximum = maximum_owner.evaluated_value;
    let limit_owner_record_indices = [minimum_owner.record_index, maximum_owner.record_index];
    let limit_value_offsets = [
        minimum_owner.evaluated_value_offset,
        maximum_owner.evaluated_value_offset,
    ];
    if !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
        return None;
    }
    Some(LegacyAsBuilt421Alignment {
        angle: angle.evaluated_value,
        offset: [
            offset_x.evaluated_value,
            offset_y.evaluated_value,
            offset_z.evaluated_value,
        ],
        owners: [angle, offset_x, offset_y, offset_z].into_iter()
            .map(|owner| crate::records::Located { value: owner.record_index, offset: owner.evaluated_value_offset }).collect(),
        limits: DesignAssemblyLimits {
            kind,
            minimum,
            maximum,
            owner_record_indices: limit_owner_record_indices,
            value_offsets: limit_value_offsets,
        },
    })
}

pub(crate) fn exact_legacy_as_built_421_solved_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignAssemblySolvedFrame> {
    let generation = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    )?;
    let crate::records::ReferenceRun::Located(references) = &scope.reference_members else { return None; };
    let [_, _, _, _, _, _, _, _, frame_reference, _, _] = references.as_slice() else { return None; };
    if scope.kind() != crate::records::DesignFeatureKind::AsBuilt
    {
        return None;
    }
    let frame_record_index = frame_reference.value;
    let expected_class_tag = generation.frame_class_tag();
    let mut frame_candidates =
        records
            .offsets(frame_record_index)
            .iter()
            .copied()
            .filter(|frame_start| {
                exact_indexed_header_at(bytes, *frame_start, frame_record_index).as_deref()
                    == Some(expected_class_tag)
            });
    let frame_start = frame_candidates.next()?;
    if frame_candidates.next().is_some() {
        return None;
    }
    let frame_length = generation.frame_length();
    let matrix_prefix = generation.matrix_prefix();
    let transform_offset = generation.matrix_offset();
    let matrix_prefix_value = match generation {
        crate::design::assembly::LegacyAsBuilt421Generation::Class364 => as_built_421_frame_376::MATRIX_PREFIX_VALUE,
        crate::design::assembly::LegacyAsBuilt421Generation::Class420 => as_built_421_frame_327::MATRIX_PREFIX_VALUE,
        crate::design::assembly::LegacyAsBuilt421Generation::Class417 => as_built_421_frame_448::MATRIX_PREFIX_VALUE,
        crate::design::assembly::LegacyAsBuilt421Generation::Class457 => as_built_421_frame_297::MATRIX_PREFIX_VALUE,
    };
    if exact_indexed_header_at(
        bytes,
        frame_start.checked_add(frame_length)?,
        frame_record_index,
    )
    .as_deref()
        != Some(generation.frame_paired_class_tag())
    {
        return None;
    }
    if bytes
        .get(frame_start.checked_add(matrix_prefix)?..frame_start.checked_add(transform_offset)?)?
        != matrix_prefix_value
    {
        return None;
    }
    let transform_at = frame_start.checked_add(transform_offset)?;
    Some(DesignAssemblySolvedFrame {
        reference_record_index: frame_record_index,
        reference_offset: frame_reference.offset,
        record_byte_offset: u64::try_from(frame_start).ok()?,
        class_tag: expected_class_tag.into(),
        transform: rigid_transform_at(bytes, transform_at)?,
        transform_offset: u64::try_from(transform_at).ok()?,
    })
}

/// Maximum component error accepted when a legacy hole direction is compared
/// with the solved connector frame's third basis column.
const EPS_LEGACY_AS_BUILT_DIRECTION: f64 = 1.0e-10;

/// Decode the two ordered construction/face-selection pairs of a 421-byte
/// `As-built` scope and derive their local frames from the stored solved frame.
pub(crate) fn exact_legacy_as_built_421_operands(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
    recipes: &[ConstructionRecipe],
    solved_frame: &DesignAssemblySolvedFrame,
) -> Option<DesignAssemblyLegacyOperands> {
    let generation = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    )?;
    let crate::records::ReferenceRun::Located(references) = &scope.reference_members else { return None; };
    let [point_reference, first_selection_reference, hole_reference, second_selection_reference, _, _, _, _, frame_reference, _, _] = references.as_slice() else { return None; };
    if scope.kind() != crate::records::DesignFeatureKind::AsBuilt
        || solved_frame.reference_record_index != frame_reference.value
    {
        return None;
    }
    let point_record_index = point_reference.value;
    let first_selection_record_index = first_selection_reference.value;
    let hole_record_index = hole_reference.value;
    let second_selection_record_index = second_selection_reference.value;
    let point = exact_point_data_construction(bytes, records, &[point_record_index], stream_types)?;
    let mut hole_scope = scope.clone();
    hole_scope.payload = crate::records::DesignFeatureKind::Hole.into();
    let hole = exact_hole_construction(bytes, records, &hole_scope, stream_types)?;
    if hole.point_record_index != hole_record_index
        || !hole.input_records.iter().map(|reference| reference.value).eq([second_selection_record_index])
        || point_rule_input_indices(&point.rule).as_slice() != [first_selection_record_index]
    {
        return None;
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
        .any(|(actual, expected)| (actual - expected).abs() > EPS_LEGACY_AS_BUILT_DIRECTION)
    {
        return None;
    }
    let selection_class_tag = match generation {
        crate::design::assembly::LegacyAsBuilt421Generation::Class364 => "307",
        crate::design::assembly::LegacyAsBuilt421Generation::Class420 => "273",
        crate::design::assembly::LegacyAsBuilt421Generation::Class417 => "332",
        crate::design::assembly::LegacyAsBuilt421Generation::Class457 => "264",
    };
    let first_selection = exact_legacy_as_built_face_selection(
        bytes,
        records,
        scope,
        1,
        first_selection_record_index,
        selection_class_tag,
        recipes,
    )?;
    let second_selection = exact_legacy_as_built_face_selection(
        bytes,
        records,
        scope,
        3,
        second_selection_record_index,
        selection_class_tag,
        recipes,
    )?;
    let point_class_tag = indexed_class_at(bytes, point.point_record_byte_offset)?;
    let hole_class_tag = indexed_class_at(bytes, hole.point_record_byte_offset)?;
    Some(DesignAssemblyLegacyOperands {
        point: DesignAssemblyLegacyOperand {
            construction_class_tag: point_class_tag,
            construction: Box::new(point),
            selection: first_selection,
            reference_offset: point_reference.offset,
        },
        hole: DesignAssemblyLegacyOperand {
            construction_class_tag: hole_class_tag,
            construction: Box::new(hole),
            selection: second_selection,
            reference_offset: hole_reference.offset,
        },
    })
}

fn point_rule_input_indices(rule: &DesignWorkPointRule) -> Vec<u32> {
    rule.inputs().iter().map(|input| input.record_index).collect()
}

fn indexed_class_at(bytes: &[u8], byte_offset: u64) -> Option<String> {
    let start = usize::try_from(byte_offset).ok()?;
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    (after_tag == start.checked_add(7)?
        && class_tag.len() == 3
        && class_tag.bytes().all(|byte| byte.is_ascii_digit()))
    .then_some(class_tag)
}

fn exact_legacy_as_built_face_selection(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    record_index: u32,
    expected_class_tag: &str,
    recipes: &[ConstructionRecipe],
) -> Option<DesignAssemblyLegacySelection> {
    let scope_start = usize::try_from(scope.byte_offset).ok()?;
    let next_byte_offset = scope
        .reference_members.values().nth(
            usize::try_from(scope_reference_ordinal)
                .ok()?
                .checked_add(1)?,
        )
        .and_then(|record_index| {
            records
                .offsets(*record_index)
                .iter()
                .copied()
                .find(|offset| *offset > scope_start)
        })
        .and_then(|offset| u64::try_from(offset).ok());
    let candidates = records
        .offsets(record_index)
        .iter()
        .copied()
        .filter_map(|byte_offset| {
            let class_tag = indexed_class_at(bytes, u64::try_from(byte_offset).ok()?)?;
            if class_tag != expected_class_tag {
                return None;
            }
            let header = DesignRecordHeader {
                id: scope.id.clone(),
                record_index,
                class_tag: class_tag.clone(),
                byte_offset: u64::try_from(byte_offset).ok()?,
            };
            let operand = parse_face_operand(
                bytes,
                records,
                scope,
                scope_reference_ordinal,
                None,
                next_byte_offset,
                &header,
                recipes,
            )?;
            let prefix = parse_entity_selection_prefix(bytes, byte_offset, record_index)?;
            Some(DesignAssemblyLegacySelection {
                record_index,
                byte_offset: u64::try_from(byte_offset).ok()?,
                class_tag,
                asset_id: prefix.asset_id,
                asset_id_offset: prefix.asset_id_offset,
                context_id: prefix.context_id,
                context_id_offset: prefix.context_id_offset,
                recipe_record_index: operand.recipe_record_index,
                recipe_record_byte_offset: operand.recipe_record_byte_offset,
                recipe_id: operand.recipe_id,
                recipe_kind: operand.recipe_kind,
                recipe_references: operand.recipe_references,
                next_byte_offset: operand.next_byte_offset,
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}
