// SPDX-License-Identifier: Apache-2.0
//! Exact sheet-metal base-flange, edge-flange and hem operation frames.

use super::shared_frames::marked_record_reference;
use crate::ids::native_stream;
use crate::layout::edge_flange_class286_two_sided_per_edge_fixed_operation as edge_flange_286_per_edge;
use crate::layout::edge_flange_class325_334_two_sided_per_edge_fixed_operation as edge_flange_325_per_edge;
use crate::layout::edge_flange_class364_per_edge_width_fixed_operation as edge_flange_364_width;
use crate::layout::edge_flange_fixed_operation_section as edge_flange;
use crate::layout::edge_flange_legacy_single_edge_fixed_operation as edge_flange_legacy;
use crate::layout::edge_flange_multi_edge_fixed_operation as edge_flange_multi;
use crate::layout::edge_flange_to_object_fixed_operation_section as flange_to_object;
use crate::layout::hem_gap_length_fixed_operation_section as hem_gap;
use crate::layout::hem_rolled_fixed_operation_section as hem_rolled;
use crate::layout::hem_teardrop_fixed_operation_section as hem_teardrop;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::sheet_metal::DesignBaseFlangeOperation;
use crate::records::feature::sheet_metal::DesignBendPosition;
use crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent;
use crate::records::feature::sheet_metal::DesignEdgeFlangeOperation;
use crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource;
use crate::records::feature::sheet_metal::DesignEdgeWidthMode;
use crate::records::feature::sheet_metal::DesignHemOperation;
use crate::records::feature::sheet_metal::DesignHemParameterOwners;
use crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn exact_base_flange_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
) -> Option<DesignBaseFlangeOperation> {
    let [profile_group_record_index, profile_record_index, thickness_record_index, settings_record_index] =
        references
    else {
        return None;
    };
    if paired_at.checked_sub(start)? != 416
        || View::u32_le_at(bytes, start + 73)? != 1
        || bytes.get(start + 81) != Some(&1)
        || View::u32_le_at(bytes, start + 82)? != *settings_record_index
        || bytes.get(start + 86..start + 92)? != [0; 6]
        || View::u32_le_at(bytes, start + 92)? != 1
        || bytes.get(start + 112) != Some(&1)
        || View::u32_le_at(bytes, start + 113)? != *thickness_record_index
        || bytes.get(start + 117..start + 123)? != [0; 6]
        || View::u32_le_at(bytes, start + 141)? != 1
        || bytes.get(start + 145) != Some(&1)
        || View::u32_le_at(bytes, start + 146)? != *profile_group_record_index
        || bytes.get(start + 150..start + 156)? != [0; 6]
    {
        return None;
    }
    let thickness = cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, start + 123)?)?;
    Some(DesignBaseFlangeOperation {
        thickness,
        thickness_offset: u64::try_from(start + 123).ok()?,
        profile_group_record_index: *profile_group_record_index,
        profile_record_index: *profile_record_index,
        thickness_record_index: *thickness_record_index,
        settings_record_index: *settings_record_index,
    })
}

/// Optional four-byte scope-header member widths that shift the fixed operation
/// section of a sheet-metal edge treatment.
///
/// The member is not announced by another field, so the true offset of the fixed
/// section is settled by reference agreement instead: exactly one candidate
/// makes every marked slot name a record the ordered reference table lists.
const SHEET_METAL_HEADER_SHIFTS: [usize; 2] = [0, 4];

/// Largest width-distance parameter-owner count a sheet-metal edge-width mode adds.
///
/// The full-edge mode adds none, the symmetric mode one, and the two-sided mode
/// two. A higher count belongs to a frame form this reader does not account for.
const MAX_EDGE_WIDTH_DISTANCE_OWNERS: usize = 2;

pub(super) fn exact_edge_flange_operation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    references: &[u32],
) -> Result<Option<DesignEdgeFlangeOperation>, CodecError> {
    // The legacy form is keyed by both class tags. The current form recovers
    // its optional header shift by agreement, so a frame that reads under more
    // than one candidate is refused as ambiguous.
    let mut resolved = None;
    let classed_candidates = match (class_tag, paired_class_tag) {
        ("325", "258") | ("334", "257") => [
            legacy_edge_flange_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                references,
                LEGACY_SINGLE_EDGE_FLANGE_LAYOUT,
            )?,
            legacy_edge_flange_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                references,
                LEGACY_MULTI_EDGE_FLANGE_LAYOUT,
            )?,
            legacy_edge_flange_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS325_TWO_SIDED_PER_EDGE_LAYOUT,
            )?,
            None,
        ],
        ("364", "261") => [
            None,
            legacy_edge_flange_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS364_PER_EDGE_WIDTH_LAYOUT,
            )?,
            legacy_edge_flange_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                references,
                LEGACY_MULTI_EDGE_FLANGE_LAYOUT,
            )?,
            None,
        ],
        ("286", "258") => [
            legacy_edge_flange_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS286_TWO_SIDED_PER_EDGE_LAYOUT,
            )?,
            legacy_edge_flange_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS286_SINGLE_EDGE_FLANGE_LAYOUT,
            )?,
            None,
            None,
        ],
        _ => [None, None, None, None],
    };
    for candidate in classed_candidates.into_iter().flatten() {
        if resolved.is_some() {
            return Ok(None);
        }
        resolved = Some(candidate);
    }
    for header_shift in SHEET_METAL_HEADER_SHIFTS {
        for candidate in [
            edge_flange_operation_at(ctx, bytes, start, paired_at, references, header_shift)?,
            edge_flange_to_object_operation_at(ctx, bytes, start, paired_at, references, header_shift)?,
        ]
        .into_iter()
        .flatten()
        {
            if resolved.is_some() {
                return Ok(None);
            }
            resolved = Some(candidate);
        }
    }
    Ok(resolved)
}

#[derive(Clone, Copy)]
struct LegacyEdgeFlangeLayout {
    frame_length: usize,
    bend_position_offset: usize,
    edge_count_offset: usize,
    edge_columns: &'static [(usize, usize)],
    settings_offset: usize,
    height_datum_offset: usize,
    angle_owner_offset: usize,
    height_owner_offset: usize,
    bend_radius_offset: usize,
    result_count_offset: usize,
    result_reference_start: usize,
    result_trailer_start: usize,
    result_separator_offset: usize,
    aggregate_group_offset: usize,
    auxiliary_reference_count: usize,
    width_mode: DesignEdgeWidthMode,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource,
    result_trailers: &'static [u32],
}

impl LegacyEdgeFlangeLayout {
    fn width_owner_count(self) -> usize {
        match self.width_mode {
            DesignEdgeWidthMode::FullEdge => 0,
            DesignEdgeWidthMode::Symmetric => 1,
            DesignEdgeWidthMode::TwoSides => 2,
            DesignEdgeWidthMode::SymmetricPerEdge => self.edge_columns.len(),
            DesignEdgeWidthMode::TwoSidesPerEdge => 2 * self.edge_columns.len(),
        }
    }

    fn reference_count(self) -> usize {
        4 + 4 * self.edge_columns.len() + self.width_owner_count() + self.auxiliary_reference_count
    }
}

const LEGACY_SINGLE_EDGE_FLANGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 494,
    bend_position_offset: edge_flange_legacy::BEND_POSITION,
    edge_count_offset: edge_flange_legacy::EDGE_COUNT,
    edge_columns: &[(
        edge_flange_legacy::EDGE_WRAPPER_REFERENCE,
        edge_flange_legacy::EDGE_GROUP_REFERENCE,
    )],
    settings_offset: edge_flange_legacy::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_legacy::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_legacy::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_legacy::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_legacy::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_legacy::RESULT_COUNT,
    result_reference_start: edge_flange_legacy::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_legacy::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_legacy::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_legacy::AGGREGATE_GROUP_REFERENCE,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::FullEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[1, 0],
};

const LEGACY_MULTI_EDGE_FLANGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 591,
    bend_position_offset: edge_flange_multi::BEND_POSITION,
    edge_count_offset: edge_flange_multi::EDGE_COUNT,
    edge_columns: &[
        (
            edge_flange_multi::EDGE_WRAPPER_ONE_REFERENCE,
            edge_flange_multi::EDGE_GROUP_ONE_REFERENCE,
        ),
        (
            edge_flange_multi::EDGE_WRAPPER_TWO_REFERENCE,
            edge_flange_multi::EDGE_GROUP_TWO_REFERENCE,
        ),
    ],
    settings_offset: edge_flange_multi::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_multi::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_multi::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_multi::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_multi::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_multi::RESULT_COUNT,
    result_reference_start: edge_flange_multi::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_multi::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_multi::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_multi::AGGREGATE_GROUP_REFERENCE,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::FullEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[1, 1, 0],
};

const LEGACY_CLASS325_TWO_SIDED_PER_EDGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 669,
    bend_position_offset: edge_flange_325_per_edge::BEND_POSITION,
    edge_count_offset: edge_flange_325_per_edge::EDGE_COUNT,
    edge_columns: &[
        (
            edge_flange_325_per_edge::EDGE_WRAPPER_ONE_REFERENCE,
            edge_flange_325_per_edge::EDGE_GROUP_ONE_REFERENCE,
        ),
        (
            edge_flange_325_per_edge::EDGE_WRAPPER_TWO_REFERENCE,
            edge_flange_325_per_edge::EDGE_GROUP_TWO_REFERENCE,
        ),
    ],
    settings_offset: edge_flange_325_per_edge::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_325_per_edge::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_325_per_edge::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_325_per_edge::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_325_per_edge::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_325_per_edge::RESULT_COUNT,
    result_reference_start: edge_flange_325_per_edge::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_325_per_edge::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_325_per_edge::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_325_per_edge::AGGREGATE_GROUP_REFERENCE,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::TwoSidesPerEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[1, 1, 1, 1, 0],
};

const LEGACY_CLASS364_PER_EDGE_WIDTH_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 643,
    bend_position_offset: edge_flange_364_width::BEND_POSITION,
    edge_count_offset: edge_flange_364_width::EDGE_COUNT,
    edge_columns: &[
        (
            edge_flange_364_width::EDGE_WRAPPER_ONE_REFERENCE,
            edge_flange_364_width::EDGE_GROUP_ONE_REFERENCE,
        ),
        (
            edge_flange_364_width::EDGE_WRAPPER_TWO_REFERENCE,
            edge_flange_364_width::EDGE_GROUP_TWO_REFERENCE,
        ),
    ],
    settings_offset: edge_flange_364_width::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_364_width::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_364_width::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_364_width::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_364_width::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_364_width::RESULT_COUNT,
    result_reference_start: edge_flange_364_width::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_364_width::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_364_width::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_364_width::AGGREGATE_GROUP_REFERENCE,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::SymmetricPerEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[1, 1, 1, 1, 0],
};

const LEGACY_CLASS286_TWO_SIDED_PER_EDGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 801,
    bend_position_offset: edge_flange_286_per_edge::BEND_POSITION,
    edge_count_offset: edge_flange_286_per_edge::EDGE_COUNT,
    edge_columns: &[
        (
            edge_flange_286_per_edge::EDGE_WRAPPER_ONE_REFERENCE,
            edge_flange_286_per_edge::EDGE_GROUP_ONE_REFERENCE,
        ),
        (
            edge_flange_286_per_edge::EDGE_WRAPPER_TWO_REFERENCE,
            edge_flange_286_per_edge::EDGE_GROUP_TWO_REFERENCE,
        ),
    ],
    settings_offset: edge_flange_286_per_edge::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_286_per_edge::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_286_per_edge::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_286_per_edge::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_286_per_edge::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_286_per_edge::RESULT_COUNT,
    result_reference_start: edge_flange_286_per_edge::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_286_per_edge::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_286_per_edge::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_286_per_edge::AGGREGATE_GROUP_REFERENCE,
    auxiliary_reference_count: 12,
    width_mode: DesignEdgeWidthMode::TwoSidesPerEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeOffset,
    result_trailers: &[1, 1, 1, 1, 0],
};

const LEGACY_CLASS286_SINGLE_EDGE_FLANGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 483,
    bend_position_offset: 80,
    edge_count_offset: 84,
    edge_columns: &[(88, 196)],
    settings_offset: 99,
    height_datum_offset: 110,
    angle_owner_offset: 114,
    height_owner_offset: 125,
    bend_radius_offset: 142,
    result_count_offset: 150,
    result_reference_start: 154,
    result_trailer_start: 165,
    result_separator_offset: 169,
    aggregate_group_offset: 173,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::FullEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[0],
};

/// Read one exact classed `EdgeFlange` form.
fn legacy_edge_flange_operation_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    layout: LegacyEdgeFlangeLayout,
) -> Result<Option<DesignEdgeFlangeOperation>, CodecError> {
    let claim = |index: u32, pool: &mut Vec<u32>| -> Result<Option<u32>, CodecError> {
        let mut entries = ctx
            .admit_iter(pool, "claim F3D legacy edge flange reference")
            .map_err(CodecError::from)?;
        let Some(at) = entries.position(|entry| *entry == index) else {
            return Ok(None);
        };
        pool.remove(at);
        Ok(Some(index))
    };
    let parsed = (|| -> Option<Result<DesignEdgeFlangeOperation, CodecError>> {
    let edge_count = layout.edge_columns.len();
    if references.len() != layout.reference_count()
        || paired_at.checked_sub(start)? != layout.frame_length
        || View::u32_le_at(bytes, start.checked_add(layout.edge_count_offset)?)?
            != u32::try_from(edge_count).ok()?
    {
        return None;
    }
    let mut unclaimed = match ctx.copy_slice(references, "f3d legacy edge-flange reference copy") {
        Ok(values) => values,
        Err(error) => return Some(Err(error)),
    };
    let mut wrapper_columns = match ctx.admit_iter(
        layout.edge_columns,
        "scan F3D legacy edge flange wrapper columns",
    ) {
        Ok(columns) => columns,
        Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
    };
    let edge_wrapper_record_indices = match wrapper_columns.try_fold(
        Vec::new(),
        |mut record_indices, (offset, _)| -> std::ops::ControlFlow<Option<CodecError>, Vec<u32>> {
            let Some(at) = start.checked_add(*offset) else {
                return std::ops::ControlFlow::Break(None);
            };
            let Some(record_index) = marked_record_reference(bytes, at) else {
                return std::ops::ControlFlow::Break(None);
            };
            let record_index = match claim(record_index, &mut unclaimed) {
                Ok(Some(record_index)) => record_index,
                Ok(None) => return std::ops::ControlFlow::Break(None),
                Err(error) => return std::ops::ControlFlow::Break(Some(error)),
            };
            record_indices.push(record_index);
            std::ops::ControlFlow::Continue(record_indices)
        },
    ) {
        std::ops::ControlFlow::Continue(record_indices) => record_indices,
        std::ops::ControlFlow::Break(None) => return None,
        std::ops::ControlFlow::Break(Some(error)) => return Some(Err(error)),
    };
    let settings_slot = marked_record_reference(bytes, start.checked_add(layout.settings_offset)?)?;
    let settings_record_index = match claim(settings_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let height_datum = DesignSheetMetalHeightDatum::from_code(View::u32_le_at(
        bytes,
        start.checked_add(layout.height_datum_offset)?,
    )?);
    let angle_slot = marked_record_reference(bytes, start.checked_add(layout.angle_owner_offset)?)?;
    let angle_owner_record_index = match claim(angle_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let height_slot = marked_record_reference(bytes, start.checked_add(layout.height_owner_offset)?)?;
    let height_owner_record_index = match claim(height_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let bend_radius_offset = start.checked_add(layout.bend_radius_offset)?;
    let bend_radius =
        cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    if View::u32_le_at(bytes, start.checked_add(layout.result_count_offset)?)?
        != u32::try_from(layout.result_trailers.len()).ok()?
        || View::u32_le_at(bytes, start.checked_add(layout.result_separator_offset)?)? != 1
    {
        return None;
    }
    // Each static layout lists at most five result references.
    let mut result_record_indices = [None; 5];
    let trailers = match ctx.admit_iter(
        layout.result_trailers,
        "validate F3D legacy edge flange result trailers",
    ) {
        Ok(trailers) => trailers,
        Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
    };
    for (ordinal, expected_trailer) in trailers.enumerate() {
        let result_offset = layout
            .result_reference_start
            .checked_add(ordinal.checked_mul(15)?)?;
        let result_record_index =
            marked_record_reference(bytes, start.checked_add(result_offset)?)?;
        if result_record_indices.contains(&Some(result_record_index))
            || View::u32_le_at(
                bytes,
                start.checked_add(
                    layout
                        .result_trailer_start
                        .checked_add(ordinal.checked_mul(15)?)?,
                )?,
            )? != *expected_trailer
        {
            return None;
        }
        *result_record_indices.get_mut(ordinal)? = Some(result_record_index);
    }
    let aggregate_slot =
        marked_record_reference(bytes, start.checked_add(layout.aggregate_group_offset)?)?;
    let aggregate_group_record_index = match claim(aggregate_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let mut group_columns = match ctx.admit_iter(
        layout.edge_columns,
        "scan F3D legacy edge flange group columns",
    ) {
        Ok(columns) => columns,
        Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
    };
    let edge_group_record_indices = match group_columns.try_fold(
        Vec::new(),
        |mut record_indices, (_, offset)| -> std::ops::ControlFlow<Option<CodecError>, Vec<u32>> {
            let Some(at) = start.checked_add(*offset) else {
                return std::ops::ControlFlow::Break(None);
            };
            let Some(record_index) = marked_record_reference(bytes, at) else {
                return std::ops::ControlFlow::Break(None);
            };
            let record_index = match claim(record_index, &mut unclaimed) {
                Ok(Some(record_index)) => record_index,
                Ok(None) => return std::ops::ControlFlow::Break(None),
                Err(error) => return std::ops::ControlFlow::Break(Some(error)),
            };
            record_indices.push(record_index);
            std::ops::ControlFlow::Continue(record_indices)
        },
    ) {
        std::ops::ControlFlow::Continue(record_indices) => record_indices,
        std::ops::ControlFlow::Break(None) => return None,
        std::ops::ControlFlow::Break(Some(error)) => return Some(Err(error)),
    };
    let mut operand_groups = match ctx.admit_iter(
        &edge_group_record_indices,
        "scan F3D legacy edge flange operand groups",
    ) {
        Ok(groups) => groups,
        Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
    };
    let edge_operand_record_indices = match operand_groups.try_fold(
        Vec::new(),
        |mut record_indices, group_record_index| -> std::ops::ControlFlow<Option<CodecError>, Vec<u32>> {
            let Some(operand_record_index) = group_record_index.checked_add(3) else {
                return std::ops::ControlFlow::Break(None);
            };
            let record_index = match claim(operand_record_index, &mut unclaimed) {
                Ok(Some(record_index)) => record_index,
                Ok(None) => return std::ops::ControlFlow::Break(None),
                Err(error) => return std::ops::ControlFlow::Break(Some(error)),
            };
            record_indices.push(record_index);
            std::ops::ControlFlow::Continue(record_indices)
        },
    ) {
        std::ops::ControlFlow::Continue(record_indices) => record_indices,
        std::ops::ControlFlow::Break(None) => return None,
        std::ops::ControlFlow::Break(Some(error)) => return Some(Err(error)),
    };
    let aggregate_operand_start = layout.width_owner_count() + layout.auxiliary_reference_count;
    let aggregate_operand_record_indices = unclaimed.split_off(aggregate_operand_start);
    let width_distance_owner_record_indices = unclaimed
        .drain(..layout.width_owner_count())
        .collect::<Vec<_>>();
    let auxiliary_reference_record_indices = unclaimed;
    let width_distance_owner_record_indices_by_edge =
        if layout.width_mode == DesignEdgeWidthMode::TwoSidesPerEdge {
            width_distance_owner_record_indices
                .chunks_exact(2)
                .map(|pair| [pair[0], pair[1]])
                .collect()
        } else {
            Vec::new()
        };
    let edges = crate::records::feature::sheet_metal::DesignEdgeFlangeEdge::from_columns(
        edge_wrapper_record_indices,
        edge_group_record_indices,
        &edge_operand_record_indices,
        aggregate_operand_record_indices,
    )
    .ok()?;
    Some(Ok(DesignEdgeFlangeOperation {
        height_owner_record_index,
        angle_owner_record_index,
        auxiliary_reference_record_indices,
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        height_datum,
        bend_position: DesignBendPosition::from_code(View::u32_le_at(
            bytes,
            start.checked_add(layout.bend_position_offset)?,
        )?),
        selection: crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
            crate::records::feature::sheet_metal::DesignEdgeFlangeShape::from_wire(
                edges,
                Some(layout.width_mode),
                width_distance_owner_record_indices,
                width_distance_owner_record_indices_by_edge,
                layout.width_parameter_source,
                DesignEdgeFlangeHeightExtent::Distance,
            )
            .ok()?,
            aggregate_group_record_index,
        )
        .ok()?,
    }))
    })();
    parsed.transpose()
}

/// Read the `EdgeFlange` fixed operation section for one candidate header shift
/// and refuse the candidate unless every slot agrees.
fn edge_flange_operation_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Result<Option<DesignEdgeFlangeOperation>, CodecError> {
    let parsed = (|| -> Option<Result<DesignEdgeFlangeOperation, CodecError>> {
        // The ordered reference table is in record-index order, so no role has a
        // fixed table position. Every role is instead named by a marked slot in the
        // fixed operation section, and the operand of a group is the record three
        // after it. The table entries no role claims are the width-distance
        // parameter owners the edge-width mode adds.
        //
        // Only the single-edge form is accounted for. A frame selecting more edges
        // names one edge group and one aggregate group in the same two slots, so
        // neither the further groups nor the order of their operands against the
        // aggregate operands is established, and such a frame is refused.
        if !(8..=8 + MAX_EDGE_WIDTH_DISTANCE_OWNERS).contains(&references.len()) {
            return None;
        }
        let common = start.checked_add(85)?.checked_add(header_shift)?;
        let bend_position = DesignBendPosition::from_code(View::u32_le_at(bytes, common)?);
        if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
            return None;
        }
        // Every reference the fixed section names is removed from this pool, so the
        // entries that remain at the end are exactly the unclaimed ones.
        // The three width modes share one fixed ten-slot allocation.
        let mut unclaimed = match ctx.vector_storage(
            8 + MAX_EDGE_WIDTH_DISTANCE_OWNERS,
            "collect F3D unclaimed flange references",
        ) {
            Ok(values) => values,
            Err(error) => return Some(Err(error)),
        };
        if let Err(error) = ctx.extend_from_slice(
            &mut unclaimed,
            references,
            "collect F3D unclaimed flange references",
        ) {
            return Some(Err(error));
        }
        let claim = |index: u32, pool: &mut Vec<u32>| -> Result<Option<u32>, CodecError> {
            let mut entries = ctx
                .admit_iter(pool, "claim F3D edge flange reference")
                .map_err(CodecError::from)?;
            let Some(at) = entries.position(|entry| *entry == index) else {
                return Ok(None);
            };
            pool.remove(at);
            Ok(Some(index))
        };

        let mut cursor = common.checked_add(edge_flange::EDGE_WRAPPER_REFERENCE)?;
        let edge_wrapper_slot = marked_record_reference(bytes, cursor)?;
        let edge_wrapper_record_index = match claim(edge_wrapper_slot, &mut unclaimed) {
            Ok(Some(record_index)) => record_index,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        cursor = common.checked_add(edge_flange::SETTINGS_REFERENCE)?;
        let settings_slot = marked_record_reference(bytes, cursor)?;
        let settings_record_index = match claim(settings_slot, &mut unclaimed) {
            Ok(Some(record_index)) => record_index,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        cursor = common.checked_add(edge_flange::HEIGHT_DATUM)?;
        let height_datum = DesignSheetMetalHeightDatum::from_code(View::u32_le_at(bytes, cursor)?);
        cursor = common.checked_add(edge_flange::ANGLE_OWNER_REFERENCE)?;
        let angle_owner_slot = marked_record_reference(bytes, cursor)?;
        let angle_owner_record_index = match claim(angle_owner_slot, &mut unclaimed) {
            Ok(Some(record_index)) => record_index,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        cursor = common.checked_add(edge_flange::HEIGHT_OWNER_REFERENCE)?;
        let height_owner_slot = marked_record_reference(bytes, cursor)?;
        let height_owner_record_index = match claim(height_owner_slot, &mut unclaimed) {
            Ok(Some(record_index)) => record_index,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let bend_radius_offset = common.checked_add(edge_flange::INSIDE_BEND_RADIUS)?;
        let bend_radius =
            cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
        let result_count =
            usize::try_from(View::u32_le_at(bytes, bend_radius_offset.checked_add(14)?)?).ok()?;
        // The aggregate-group and role-`0x08` group slots close the section after the
        // result-record run, so they also confirm the recovered result count.
        let aggregate_slot = bend_radius_offset
            .checked_add(22)?
            .checked_add(result_count.checked_mul(15)?)?;
        let aggregate_slot_record_index = marked_record_reference(bytes, aggregate_slot)?;
        let aggregate_group_record_index = match claim(aggregate_slot_record_index, &mut unclaimed) {
            Ok(Some(record_index)) => record_index,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let first_edge_group = marked_record_reference(bytes, aggregate_slot.checked_add(27)?)?;

        // A group's recipe-backed operand is the record three after the group.
        let aggregate_operand_index = aggregate_group_record_index.checked_add(3)?;
        let aggregate_operand_record_index = match claim(aggregate_operand_index, &mut unclaimed) {
            Ok(Some(record_index)) => record_index,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let edge_group_record_index = match claim(first_edge_group, &mut unclaimed) {
            Ok(Some(record_index)) => record_index,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let edge_operand_index = first_edge_group.checked_add(3)?;
        match claim(edge_operand_index, &mut unclaimed) {
            Ok(Some(_)) => {}
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        }

        if unclaimed.len() > MAX_EDGE_WIDTH_DISTANCE_OWNERS {
            return None;
        }
        let width_count = unclaimed.len();
        let width_distance_owner_record_indices = unclaimed;

        let expected_length = 493usize
            .checked_add(result_count.checked_mul(15)?)?
            .checked_add(width_count.checked_mul(11)?)?
            .checked_add(header_shift)?;
        if paired_at.checked_sub(start)? != expected_length {
            return None;
        }
        Some(Ok(DesignEdgeFlangeOperation {
            height_owner_record_index,
            angle_owner_record_index,
            auxiliary_reference_record_indices: Vec::new(),
            settings_record_index,
            bend_radius,
            bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
            height_datum,
            bend_position,
            selection: crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::from_wire(
                    vec![crate::records::feature::sheet_metal::DesignEdgeFlangeEdge {
                        wrapper: edge_wrapper_record_index,
                        group_record_index: edge_group_record_index.try_into().ok()?,
                        aggregate_operand_record_index,
                    }],
                    None,
                    width_distance_owner_record_indices,
                    Vec::new(),
                    DesignEdgeFlangeWidthParameterSource::EdgeWidth,
                    DesignEdgeFlangeHeightExtent::Distance,
                )
                .ok()?,
                aggregate_group_record_index,
            )
            .ok()?,
        }))
    })();
    parsed.transpose()
}

/// Read the single-edge `EdgeFlange` form whose height is measured from a
/// selected construction entity.
fn edge_flange_to_object_operation_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Result<Option<DesignEdgeFlangeOperation>, CodecError> {
    let claim = |index: u32, pool: &mut Vec<u32>| -> Result<Option<u32>, CodecError> {
        let mut entries = ctx
            .admit_iter(pool, "claim F3D edge flange object reference")
            .map_err(CodecError::from)?;
        let Some(at) = entries.position(|entry| *entry == index) else {
            return Ok(None);
        };
        pool.remove(at);
        Ok(Some(index))
    };
    let parsed = (|| -> Option<Result<DesignEdgeFlangeOperation, CodecError>> {
    // This form has one target group and one target entity-selection operand in
    // addition to the distance form's roles. The two marked references between
    // the target group and the aggregate group are fixed-frame references, not
    // entries in the scope's ordered reference table, and are retained as
    // native references for rewrite.
    if references.len() != 11 {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    let bend_position = DesignBendPosition::from_code(View::u32_le_at(bytes, common)?);
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }
    let mut unclaimed = match ctx.copy_slice(references, "f3d ToObject edge-flange reference copy") {
        Ok(values) => values,
        Err(error) => return Some(Err(error)),
    };
    let mut cursor = common.checked_add(edge_flange::EDGE_WRAPPER_REFERENCE)?;
    let edge_wrapper_slot = marked_record_reference(bytes, cursor)?;
    let edge_wrapper_record_index = match claim(edge_wrapper_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    cursor = common.checked_add(edge_flange::SETTINGS_REFERENCE)?;
    let settings_slot = marked_record_reference(bytes, cursor)?;
    let settings_record_index = match claim(settings_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    cursor = common.checked_add(edge_flange::HEIGHT_DATUM)?;
    let height_datum = DesignSheetMetalHeightDatum::from_code(View::u32_le_at(bytes, cursor)?);
    cursor = common.checked_add(edge_flange::ANGLE_OWNER_REFERENCE)?;
    let angle_owner_slot = marked_record_reference(bytes, cursor)?;
    let angle_owner_record_index = match claim(angle_owner_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    cursor = common.checked_add(edge_flange::HEIGHT_OWNER_REFERENCE)?;
    let height_owner_slot = marked_record_reference(bytes, cursor)?;
    let height_owner_record_index = match claim(height_owner_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let bend_radius_offset = common.checked_add(edge_flange::INSIDE_BEND_RADIUS)?;
    let bend_radius =
        cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    let result_count = View::u32_le_at(bytes, bend_radius_offset.checked_add(14)?)?;
    if result_count != 1
        || bytes.get(bend_radius_offset.checked_add(18)?..bend_radius_offset.checked_add(22)?)?
            != [0; 4]
    {
        return None;
    }
    if bytes.get(common.checked_add(89)?..common.checked_add(94)?)? != [0; 5] {
        return None;
    }
    let target_slot = marked_record_reference(
        bytes,
        common.checked_add(flange_to_object::TARGET_GROUP_REFERENCE)?,
    )?;
    let target_group_record_index = match claim(target_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    if View::u32_le_at(
        bytes,
        common.checked_add(flange_to_object::TARGET_REFERENCE_COUNT)?,
    )? != 2
    {
        return None;
    }
    let reference_record_indices = [
        marked_record_reference(
            bytes,
            common.checked_add(flange_to_object::INSERTED_REFERENCE_ONE)?,
        )?,
        marked_record_reference(
            bytes,
            common.checked_add(flange_to_object::INSERTED_REFERENCE_TWO)?,
        )?,
    ];
    if reference_record_indices[0] == reference_record_indices[1]
        || reference_record_indices
            .iter()
            .any(|record_index| references.contains(record_index))
        || View::u32_le_at(
            bytes,
            common.checked_add(flange_to_object::INSERTED_REFERENCE_COUNT)?,
        )? != 1
        || bytes.get(
            common.checked_add(135)?
                ..common.checked_add(flange_to_object::AGGREGATE_REFERENCE_COUNT)?,
        )? != [0; 4]
        || View::u32_le_at(
            bytes,
            common.checked_add(flange_to_object::AGGREGATE_REFERENCE_COUNT)?,
        )? != 1
        || bytes.get(
            common.checked_add(154)?..common.checked_add(flange_to_object::EDGE_REFERENCE_COUNT)?,
        )? != [0; 12]
        || View::u32_le_at(
            bytes,
            common.checked_add(flange_to_object::EDGE_REFERENCE_COUNT)?,
        )? != 1
    {
        return None;
    }
    let aggregate_slot = marked_record_reference(
        bytes,
        common.checked_add(flange_to_object::AGGREGATE_GROUP_REFERENCE)?,
    )?;
    let aggregate_group_record_index = match claim(aggregate_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let edge_slot = marked_record_reference(
        bytes,
        common.checked_add(flange_to_object::EDGE_GROUP_REFERENCE)?,
    )?;
    let edge_group_record_index = match claim(edge_slot, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let target_operand_index = target_group_record_index.checked_add(3)?;
    let target_operand_record_index = match claim(target_operand_index, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let aggregate_operand_index = aggregate_group_record_index.checked_add(3)?;
    let aggregate_operand_record_index = match claim(aggregate_operand_index, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let edge_operand_index = edge_group_record_index.checked_add(3)?;
    match claim(edge_operand_index, &mut unclaimed) {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    }
    let [offset_owner_record_index] = unclaimed.as_slice() else {
        return None;
    };
    let expected_length = 576usize.checked_add(header_shift)?;
    if paired_at.checked_sub(start)? != expected_length {
        return None;
    }
    Some(Ok(DesignEdgeFlangeOperation {
        height_owner_record_index,
        angle_owner_record_index,
        auxiliary_reference_record_indices: Vec::new(),
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        height_datum,
        bend_position,
        selection: crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
            crate::records::feature::sheet_metal::DesignEdgeFlangeShape::FullEdge {
                edges: vec![crate::records::feature::sheet_metal::DesignEdgeFlangeEdge {
                    wrapper: edge_wrapper_record_index,
                    group_record_index: edge_group_record_index.try_into().ok()?,
                    aggregate_operand_record_index,
                }],
                height: DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index,
                    target_operand_record_index,
                    offset_owner_record_index: *offset_owner_record_index,
                    reference_record_indices,
                },
            },
            aggregate_group_record_index,
        )
        .ok()?,
    }))
    })();
    parsed.transpose()
}

fn exact_hem_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &(impl ExactSizeIterator<Item = u32> + Clone),
    has_kind: &impl Fn(u32, &str) -> Result<bool, CodecError>,
) -> Result<Option<DesignHemOperation>, CodecError> {
    // The header shift and form are recovered by agreement, so all candidates
    // are evaluated and a frame that admits more than one is refused.
    let mut resolved = None;
    for header_shift in SHEET_METAL_HEADER_SHIFTS {
        let candidates = [
            hem_gap_length_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                (*references).clone(),
                header_shift,
            )?,
            hem_radius_angle_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                (*references).clone(),
                header_shift,
            )?,
            hem_gap_length_radius_operation_at(
                ctx,
                bytes,
                start,
                paired_at,
                (*references).clone(),
                header_shift,
            )?,
        ];
        for candidate in candidates.into_iter().flatten() {
            if !hem_parameter_kinds_match(&candidate, has_kind)? {
                continue;
            }
            if resolved.is_some() {
                return Ok(None);
            }
            resolved = Some(candidate);
        }
    }
    Ok(resolved)
}

fn hem_parameter_kinds_match(
    operation: &DesignHemOperation,
    has_kind: &impl Fn(u32, &str) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    match operation.parameter_owners {
        DesignHemParameterOwners::GapLength {
            gap_owner_record_index,
            length_owner_record_index,
        } => Ok(has_kind(gap_owner_record_index, "HemGap")?
            && has_kind(length_owner_record_index, "HemLength")?),
        DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index,
            angle_owner_record_index,
        } => Ok(has_kind(radius_owner_record_index, "HemRadius")?
            && has_kind(angle_owner_record_index, "HemAngle")?),
        DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index,
            length_owner_record_index,
            radius_owner_record_index,
        } => Ok(has_kind(gap_owner_record_index, "HemGap")?
            && has_kind(length_owner_record_index, "HemLength")?
            && has_kind(radius_owner_record_index, "HemRadius")?),
    }
}

pub(super) fn bind_hem_operation_from_parameters(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &mut DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) -> Result<(), CodecError> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Hem {
        return Ok(());
    }
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(());
    };
    let has_kind = |record_index: u32, expected: &str| -> Result<bool, CodecError> {
        let mut matches = parameter_owners.iter().filter_map(|owner| {
            if native_stream(owner.id()) != Some(stream)
                || owner.scope_record_index() != scope.record_index
                || owner.record_index() != record_index
            {
                return None;
            }
            let mut references = match super::parameter_scope::reference_members(
                ctx, scope.reference_members(), "check F3D Hem owner references",
            ) {
                Ok(references) => references,
                Err(error) => return Some(Err(error)),
            };
            references.any(|value| value == owner.record_index()).then_some(Ok(owner))
        }).flat_map(|owner| {
            let (error, owner) = match owner {
                Ok(owner) => (None, Some(owner)),
                Err(error) => (Some(error), None),
            };
            error.map(Err).into_iter().chain(owner.into_iter().flat_map(|owner| {
                parameters.iter().filter(move |parameter| {
                    native_stream(&parameter.id) == Some(stream)
                        && parameter.record_index == owner.parameter_record_index()
                }).map(|parameter| Ok(parameter.source_kind()))
            }))
        });
        match matches.next() {
            Some(Ok(kind)) if kind == expected => match matches.next() {
                None => Ok(true),
                Some(Ok(_)) => Ok(false),
                Some(Err(error)) => Err(error),
            },
            Some(Ok(_)) | None => Ok(false),
            Some(Err(error)) => Err(error),
        }
    };
    let Some(start) = usize::try_from(scope.byte_offset()).ok() else {
        return Ok(());
    };
    let Some(paired_at) = usize::try_from(scope.paired_byte_offset()).ok() else {
        return Ok(());
    };
    {
        let construction = exact_hem_operation(
            ctx,
            bytes,
            start,
            paired_at,
            &scope.reference_members().values().copied(),
            &has_kind,
        )?;
        if let crate::records::feature::scope::DesignScopePayloadMut::Hem(slot) =
            scope.payload_mut()
        {
            *slot = construction;
        }
    }
    Ok(())
}

/// Read the gap-and-length `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
///
/// The ordered reference table is in record-index order, so every role is taken
/// from the marked slot that names it and each group's operand is the record
/// three after that group. The rolled and teardrop forms place their owner
/// references at other offsets and are handled by their corresponding readers.
fn hem_gap_length_operation_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: impl ExactSizeIterator<Item = u32>,
    header_shift: usize,
) -> Result<Option<DesignHemOperation>, CodecError> {
    let parsed = (|| -> Option<Result<DesignHemOperation, CodecError>> {
    if references.len() != 8
        || paired_at.checked_sub(start)? != 494usize.checked_add(header_shift)?
    {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }

    let mut unclaimed: Vec<u32> = references.collect::<Vec<_>>();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Result<Option<u32>, CodecError> {
        let mut entries = ctx
            .admit_iter(pool, "claim F3D gap-length Hem reference")
            .map_err(CodecError::from)?;
        let Some(at) = entries.position(|entry| *entry == index) else {
            return Ok(None);
        };
        pool.remove(at);
        Ok(Some(index))
    };
    let slot = |offset: usize, pool: &mut Vec<u32>| -> Result<Option<u32>, CodecError> {
        let Some(at) = common.checked_add(offset) else {
            return Ok(None);
        };
        let Some(index) = marked_record_reference(bytes, at) else {
            return Ok(None);
        };
        claim(index, pool)
    };

    let edge_wrapper_record_index = match slot(hem_gap::EDGE_WRAPPER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let settings_record_index = match slot(hem_gap::SETTINGS_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    // The two owners are the form's inputs in local-ordinal order.
    let gap_owner_record_index = match slot(hem_gap::GAP_OWNER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let length_owner_record_index = match slot(hem_gap::LENGTH_OWNER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };

    let bend_radius_offset = common.checked_add(hem_gap::INSIDE_BEND_RADIUS)?;
    let bend_radius =
        cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;

    let aggregate_group_record_index = match slot(108, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let edge_group_record_index = match slot(135, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let aggregate_operand_index = aggregate_group_record_index.checked_add(3)?;
    match claim(aggregate_operand_index, &mut unclaimed) {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    }
    let edge_operand_index = edge_group_record_index.checked_add(3)?;
    match claim(edge_operand_index, &mut unclaimed) {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    }
    if !unclaimed.is_empty() {
        return None;
    }

    Some(Ok(DesignHemOperation {
        edge_wrapper_record_index,
        edge_group_record_index: edge_group_record_index.try_into().ok()?,
        aggregate_group_record_index: aggregate_group_record_index.try_into().ok()?,
        parameter_owners: DesignHemParameterOwners::GapLength {
            gap_owner_record_index,
            length_owner_record_index,
        },
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
    }))
    })();
    parsed.transpose()
}

/// Read the rolled `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
///
/// Rolled forms keep the two-owner frame length, but their owner slots are at
/// offsets `41` and `54` instead of `42` and `53`. The source parameter kinds
/// assign those slots to radius and angle; the fixed frame only proves their
/// record identities.
fn hem_radius_angle_operation_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: impl ExactSizeIterator<Item = u32>,
    header_shift: usize,
) -> Result<Option<DesignHemOperation>, CodecError> {
    let parsed = (|| -> Option<Result<DesignHemOperation, CodecError>> {
    if references.len() != 8
        || paired_at.checked_sub(start)? != 494usize.checked_add(header_shift)?
    {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }

    let mut unclaimed = references.collect::<Vec<_>>();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Result<Option<u32>, CodecError> {
        let mut entries = ctx
            .admit_iter(pool, "claim F3D rolled Hem reference")
            .map_err(CodecError::from)?;
        let Some(at) = entries.position(|entry| *entry == index) else {
            return Ok(None);
        };
        pool.remove(at);
        Ok(Some(index))
    };
    let slot = |offset: usize, pool: &mut Vec<u32>| -> Result<Option<u32>, CodecError> {
        let Some(at) = common.checked_add(offset) else {
            return Ok(None);
        };
        let Some(index) = marked_record_reference(bytes, at) else {
            return Ok(None);
        };
        claim(index, pool)
    };

    let edge_wrapper_record_index = match slot(hem_gap::EDGE_WRAPPER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let settings_record_index = match slot(hem_gap::SETTINGS_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let angle_owner_record_index = match slot(hem_rolled::ANGLE_OWNER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let radius_owner_record_index = match slot(hem_rolled::RADIUS_OWNER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let bend_radius_offset = common.checked_add(hem_rolled::INSIDE_BEND_RADIUS)?;
    let bend_radius =
        cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    let aggregate_group_record_index = match slot(108, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let edge_group_record_index = match slot(135, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let aggregate_operand_index = aggregate_group_record_index.checked_add(3)?;
    match claim(aggregate_operand_index, &mut unclaimed) {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    }
    let edge_operand_index = edge_group_record_index.checked_add(3)?;
    match claim(edge_operand_index, &mut unclaimed) {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    }
    if !unclaimed.is_empty() {
        return None;
    }

    Some(Ok(DesignHemOperation {
        edge_wrapper_record_index,
        edge_group_record_index: edge_group_record_index.try_into().ok()?,
        aggregate_group_record_index: aggregate_group_record_index.try_into().ok()?,
        parameter_owners: DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index,
            angle_owner_record_index,
        },
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
    }))
    })();
    parsed.transpose()
}

/// Read the teardrop `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
fn hem_gap_length_radius_operation_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: impl ExactSizeIterator<Item = u32>,
    header_shift: usize,
) -> Result<Option<DesignHemOperation>, CodecError> {
    let parsed = (|| -> Option<Result<DesignHemOperation, CodecError>> {
    if references.len() != 9
        || paired_at.checked_sub(start)? != 515usize.checked_add(header_shift)?
    {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }

    let mut unclaimed = references.collect::<Vec<_>>();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Result<Option<u32>, CodecError> {
        let mut entries = ctx
            .admit_iter(pool, "claim F3D teardrop Hem reference")
            .map_err(CodecError::from)?;
        let Some(at) = entries.position(|entry| *entry == index) else {
            return Ok(None);
        };
        pool.remove(at);
        Ok(Some(index))
    };
    let slot = |offset: usize, pool: &mut Vec<u32>| -> Result<Option<u32>, CodecError> {
        let Some(at) = common.checked_add(offset) else {
            return Ok(None);
        };
        let Some(index) = marked_record_reference(bytes, at) else {
            return Ok(None);
        };
        claim(index, pool)
    };

    let edge_wrapper_record_index = match slot(hem_gap::EDGE_WRAPPER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let settings_record_index = match slot(hem_gap::SETTINGS_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let gap_owner_record_index = match slot(hem_teardrop::GAP_OWNER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let length_owner_record_index = match slot(hem_teardrop::LENGTH_OWNER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let radius_owner_record_index = match slot(hem_teardrop::RADIUS_OWNER_REFERENCE, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let bend_radius_offset = common.checked_add(hem_teardrop::INSIDE_BEND_RADIUS)?;
    let bend_radius =
        cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    let aggregate_group_record_index = match slot(118, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let edge_group_record_index = match slot(145, &mut unclaimed) {
        Ok(Some(record_index)) => record_index,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let aggregate_operand_index = aggregate_group_record_index.checked_add(3)?;
    match claim(aggregate_operand_index, &mut unclaimed) {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    }
    let edge_operand_index = edge_group_record_index.checked_add(3)?;
    match claim(edge_operand_index, &mut unclaimed) {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    }
    if !unclaimed.is_empty() {
        return None;
    }

    Some(Ok(DesignHemOperation {
        edge_wrapper_record_index,
        edge_group_record_index: edge_group_record_index.try_into().ok()?,
        aggregate_group_record_index: aggregate_group_record_index.try_into().ok()?,
        parameter_owners: DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index,
            length_owner_record_index,
            radius_owner_record_index,
        },
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
    }))
    })();
    parsed.transpose()
}

#[cfg(test)]
mod tests;
