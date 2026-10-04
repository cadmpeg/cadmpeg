// SPDX-License-Identifier: Apache-2.0
//! Exact sheet-metal base-flange, edge-flange and hem operation frames.
//!
//! Every form read here lists at most `MAX_SHEET_METAL_REFERENCES` ordered
//! references and names its roles at fixed offsets, so the frame readers do
//! fixed work and need no decode context. Binding a `Hem` to its parameter
//! kinds searches the decoded parameter owners and is charged.

use super::shared_frames::marked_record_reference;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::record_streams::{in_stream, record_stream};
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
use crate::records::feature::scope::{
    DesignParameterScope, DesignScopePayload, DesignScopePayloadMut,
};
use crate::records::feature::sheet_metal::DesignBaseFlangeOperation;
use crate::records::feature::sheet_metal::DesignBendPosition;
use crate::records::feature::sheet_metal::DesignEdgeFlangeEdge;
use crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent;
use crate::records::feature::sheet_metal::DesignEdgeFlangeOperation;
use crate::records::feature::sheet_metal::DesignEdgeFlangeSelection;
use crate::records::feature::sheet_metal::DesignEdgeFlangeShape;
use crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource;
use crate::records::feature::sheet_metal::DesignEdgeWidthMode;
use crate::records::feature::sheet_metal::DesignHemOperation;
use crate::records::feature::sheet_metal::DesignHemParameterOwners;
use crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum;
use crate::records::identity::ReferenceRun;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

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
        || !zeros_at::<6>(bytes, start + 86)
        || View::u32_le_at(bytes, start + 92)? != 1
        || bytes.get(start + 112) != Some(&1)
        || View::u32_le_at(bytes, start + 113)? != *thickness_record_index
        || !zeros_at::<6>(bytes, start + 117)
        || View::u32_le_at(bytes, start + 141)? != 1
        || bytes.get(start + 145) != Some(&1)
        || View::u32_le_at(bytes, start + 146)? != *profile_group_record_index
        || !zeros_at::<6>(bytes, start + 150)
    {
        return None;
    }
    let thickness = PositiveReal::new(View::f64_le_at(bytes, start + 123)?)?;
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

/// Largest ordered reference table of a sheet-metal form read here: the
/// class-286 two-sided per-edge flange lists twenty-eight references.
const MAX_SHEET_METAL_REFERENCES: usize = 28;

/// The ordered reference-table entries no fixed slot has claimed yet, in table
/// order. The table holds at most `MAX_SHEET_METAL_REFERENCES` entries, so
/// building the pool and each claim are fixed work.
struct UnclaimedReferences([Option<u32>; MAX_SHEET_METAL_REFERENCES]);

impl UnclaimedReferences {
    /// The pool of `references`; a table longer than any accepted form has none.
    fn new(references: &[u32]) -> Option<Self> {
        if references.len() > MAX_SHEET_METAL_REFERENCES {
            return None;
        }
        let mut entries = [None; MAX_SHEET_METAL_REFERENCES];
        for (entry, reference) in entries.iter_mut().zip(references) {
            *entry = Some(*reference);
        }
        Some(Self(entries))
    }

    /// The pool of a run of exactly `N` references.
    fn from_run<const N: usize>(references: &ReferenceRun<u32>) -> Option<Self> {
        Self::new(&references.values_array::<N>()?.map(|value| *value))
    }

    /// Remove the first unclaimed entry equal to `record_index`.
    fn claim(&mut self, record_index: u32) -> Option<u32> {
        let entry = self
            .0
            .iter_mut()
            .find(|entry| **entry == Some(record_index))?;
        *entry = None;
        Some(record_index)
    }

    /// Claim the record that the marked reference slot at `at` names.
    fn claim_slot(&mut self, bytes: &[u8], at: usize) -> Option<u32> {
        self.claim(marked_record_reference(bytes, at)?)
    }

    /// The unclaimed entries in table order.
    fn remaining(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().flatten().copied()
    }
}

pub(super) fn exact_edge_flange_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    references: &[u32],
) -> Option<DesignEdgeFlangeOperation> {
    // The legacy form is keyed by both class tags. The current form recovers
    // its optional header shift by agreement, so a frame that reads under more
    // than one candidate is refused as ambiguous.
    let classed_candidates = match (class_tag, paired_class_tag) {
        ("325", "258") | ("334", "257") => [
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_SINGLE_EDGE_FLANGE_LAYOUT,
            ),
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_MULTI_EDGE_FLANGE_LAYOUT,
            ),
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS325_TWO_SIDED_PER_EDGE_LAYOUT,
            ),
        ],
        ("364", "261") => [
            None,
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS364_PER_EDGE_WIDTH_LAYOUT,
            ),
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_MULTI_EDGE_FLANGE_LAYOUT,
            ),
        ],
        ("286", "258") => [
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS286_TWO_SIDED_PER_EDGE_LAYOUT,
            ),
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS286_SINGLE_EDGE_FLANGE_LAYOUT,
            ),
            None,
        ],
        _ => [None, None, None],
    };
    let current_candidates = SHEET_METAL_HEADER_SHIFTS.map(|header_shift| {
        [
            edge_flange_operation_at(bytes, start, paired_at, references, header_shift),
            edge_flange_to_object_operation_at(bytes, start, paired_at, references, header_shift),
        ]
    });
    let mut resolved = None;
    for candidate in classed_candidates
        .into_iter()
        .chain(current_candidates.into_iter().flatten())
        .flatten()
    {
        if resolved.replace(candidate).is_some() {
            return None;
        }
    }
    resolved
}

/// One exact classed `EdgeFlange` form with `EDGES` selected edges and
/// `RESULTS` result references.
#[derive(Clone, Copy)]
struct LegacyEdgeFlangeLayout<const EDGES: usize, const RESULTS: usize> {
    frame_length: usize,
    bend_position_offset: usize,
    edge_count_offset: usize,
    edge_columns: [(usize, usize); EDGES],
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
    result_trailers: [u32; RESULTS],
}

impl<const EDGES: usize, const RESULTS: usize> LegacyEdgeFlangeLayout<EDGES, RESULTS> {
    const fn width_owner_count(self) -> usize {
        match self.width_mode {
            DesignEdgeWidthMode::FullEdge => 0,
            DesignEdgeWidthMode::Symmetric => 1,
            DesignEdgeWidthMode::TwoSides => 2,
            DesignEdgeWidthMode::SymmetricPerEdge => EDGES,
            DesignEdgeWidthMode::TwoSidesPerEdge => 2 * EDGES,
        }
    }

    const fn reference_count(self) -> usize {
        4 + 4 * EDGES + self.width_owner_count() + self.auxiliary_reference_count
    }
}

const LEGACY_SINGLE_EDGE_FLANGE_LAYOUT: LegacyEdgeFlangeLayout<1, 2> = LegacyEdgeFlangeLayout {
    frame_length: 494,
    bend_position_offset: edge_flange_legacy::BEND_POSITION,
    edge_count_offset: edge_flange_legacy::EDGE_COUNT,
    edge_columns: [(
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
    result_trailers: [1, 0],
};

const LEGACY_MULTI_EDGE_FLANGE_LAYOUT: LegacyEdgeFlangeLayout<2, 3> = LegacyEdgeFlangeLayout {
    frame_length: 591,
    bend_position_offset: edge_flange_multi::BEND_POSITION,
    edge_count_offset: edge_flange_multi::EDGE_COUNT,
    edge_columns: [
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
    result_trailers: [1, 1, 0],
};

const LEGACY_CLASS325_TWO_SIDED_PER_EDGE_LAYOUT: LegacyEdgeFlangeLayout<2, 5> =
    LegacyEdgeFlangeLayout {
        frame_length: 669,
        bend_position_offset: edge_flange_325_per_edge::BEND_POSITION,
        edge_count_offset: edge_flange_325_per_edge::EDGE_COUNT,
        edge_columns: [
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
        result_trailers: [1, 1, 1, 1, 0],
    };

const LEGACY_CLASS364_PER_EDGE_WIDTH_LAYOUT: LegacyEdgeFlangeLayout<2, 5> =
    LegacyEdgeFlangeLayout {
        frame_length: 643,
        bend_position_offset: edge_flange_364_width::BEND_POSITION,
        edge_count_offset: edge_flange_364_width::EDGE_COUNT,
        edge_columns: [
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
        result_trailers: [1, 1, 1, 1, 0],
    };

const LEGACY_CLASS286_TWO_SIDED_PER_EDGE_LAYOUT: LegacyEdgeFlangeLayout<2, 5> =
    LegacyEdgeFlangeLayout {
        frame_length: 801,
        bend_position_offset: edge_flange_286_per_edge::BEND_POSITION,
        edge_count_offset: edge_flange_286_per_edge::EDGE_COUNT,
        edge_columns: [
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
        result_trailers: [1, 1, 1, 1, 0],
    };

const LEGACY_CLASS286_SINGLE_EDGE_FLANGE_LAYOUT: LegacyEdgeFlangeLayout<1, 1> =
    LegacyEdgeFlangeLayout {
        frame_length: 483,
        bend_position_offset: 80,
        edge_count_offset: 84,
        edge_columns: [(88, 196)],
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
        result_trailers: [0],
    };

// Every classed layout fits the reference pool.
const _: () = {
    assert!(LEGACY_SINGLE_EDGE_FLANGE_LAYOUT.reference_count() <= MAX_SHEET_METAL_REFERENCES);
    assert!(LEGACY_MULTI_EDGE_FLANGE_LAYOUT.reference_count() <= MAX_SHEET_METAL_REFERENCES);
    assert!(
        LEGACY_CLASS325_TWO_SIDED_PER_EDGE_LAYOUT.reference_count() <= MAX_SHEET_METAL_REFERENCES
    );
    assert!(LEGACY_CLASS364_PER_EDGE_WIDTH_LAYOUT.reference_count() <= MAX_SHEET_METAL_REFERENCES);
    assert!(
        LEGACY_CLASS286_TWO_SIDED_PER_EDGE_LAYOUT.reference_count() <= MAX_SHEET_METAL_REFERENCES
    );
    assert!(
        LEGACY_CLASS286_SINGLE_EDGE_FLANGE_LAYOUT.reference_count() <= MAX_SHEET_METAL_REFERENCES
    );
};

/// Read one exact classed `EdgeFlange` form.
fn legacy_edge_flange_operation_at<const EDGES: usize, const RESULTS: usize>(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    layout: LegacyEdgeFlangeLayout<EDGES, RESULTS>,
) -> Option<DesignEdgeFlangeOperation> {
    if references.len() != layout.reference_count()
        || paired_at.checked_sub(start)? != layout.frame_length
        || View::u32_le_at(bytes, start.checked_add(layout.edge_count_offset)?)?
            != u32::try_from(EDGES).ok()?
    {
        return None;
    }
    let mut unclaimed = UnclaimedReferences::new(references)?;
    let mut edge_wrapper_record_indices = [0; EDGES];
    for (record_index, (offset, _)) in edge_wrapper_record_indices
        .iter_mut()
        .zip(layout.edge_columns)
    {
        *record_index = unclaimed.claim_slot(bytes, start.checked_add(offset)?)?;
    }
    let settings_record_index =
        unclaimed.claim_slot(bytes, start.checked_add(layout.settings_offset)?)?;
    let height_datum = DesignSheetMetalHeightDatum::from_code(View::u32_le_at(
        bytes,
        start.checked_add(layout.height_datum_offset)?,
    )?);
    let angle_owner_record_index =
        unclaimed.claim_slot(bytes, start.checked_add(layout.angle_owner_offset)?)?;
    let height_owner_record_index =
        unclaimed.claim_slot(bytes, start.checked_add(layout.height_owner_offset)?)?;
    let bend_radius_offset = start.checked_add(layout.bend_radius_offset)?;
    let bend_radius = PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    if View::u32_le_at(bytes, start.checked_add(layout.result_count_offset)?)?
        != u32::try_from(RESULTS).ok()?
        || View::u32_le_at(bytes, start.checked_add(layout.result_separator_offset)?)? != 1
    {
        return None;
    }
    let mut result_record_indices = [None; RESULTS];
    for (ordinal, expected_trailer) in layout.result_trailers.into_iter().enumerate() {
        let result_offset = layout
            .result_reference_start
            .checked_add(ordinal.checked_mul(15)?)?;
        let result_record_index =
            marked_record_reference(bytes, start.checked_add(result_offset)?)?;
        let trailer_offset = layout
            .result_trailer_start
            .checked_add(ordinal.checked_mul(15)?)?;
        if result_record_indices.contains(&Some(result_record_index))
            || View::u32_le_at(bytes, start.checked_add(trailer_offset)?)? != expected_trailer
        {
            return None;
        }
        *result_record_indices.get_mut(ordinal)? = Some(result_record_index);
    }
    let aggregate_group_record_index =
        unclaimed.claim_slot(bytes, start.checked_add(layout.aggregate_group_offset)?)?;
    let mut edge_group_record_indices = [0; EDGES];
    for (record_index, (_, offset)) in edge_group_record_indices
        .iter_mut()
        .zip(layout.edge_columns)
    {
        *record_index = unclaimed.claim_slot(bytes, start.checked_add(offset)?)?;
    }
    // A group's recipe-backed operand is the record three after the group.
    let mut edge_operand_record_indices = [0; EDGES];
    for (record_index, group_record_index) in edge_operand_record_indices
        .iter_mut()
        .zip(edge_group_record_indices)
    {
        *record_index = unclaimed.claim(group_record_index.checked_add(3)?)?;
    }
    // The unclaimed entries are the width owners, then the auxiliary
    // references, then one aggregate operand per edge.
    let width_owner_count = layout.width_owner_count();
    let aggregate_operand_start = width_owner_count + layout.auxiliary_reference_count;
    let width_distance_owner_record_indices: Vec<u32> =
        unclaimed.remaining().take(width_owner_count).collect();
    let auxiliary_reference_record_indices: Vec<u32> = unclaimed
        .remaining()
        .skip(width_owner_count)
        .take(layout.auxiliary_reference_count)
        .collect();
    let aggregate_operand_record_indices: Vec<u32> = unclaimed
        .remaining()
        .skip(aggregate_operand_start)
        .collect();
    let width_distance_owner_record_indices_by_edge: Vec<[u32; 2]> =
        if layout.width_mode == DesignEdgeWidthMode::TwoSidesPerEdge {
            let mut owners = unclaimed.remaining();
            [(); EDGES]
                .into_iter()
                .map(|()| Some([owners.next()?, owners.next()?]))
                .collect::<Option<_>>()?
        } else {
            Vec::new()
        };
    let edges = DesignEdgeFlangeEdge::from_columns(
        Vec::from(edge_wrapper_record_indices),
        Vec::from(edge_group_record_indices),
        &edge_operand_record_indices,
        aggregate_operand_record_indices,
    )
    .ok()?;
    Some(DesignEdgeFlangeOperation {
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
        selection: DesignEdgeFlangeSelection::try_new(
            DesignEdgeFlangeShape::from_wire(
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
    })
}

/// Read the `EdgeFlange` fixed operation section for one candidate header shift
/// and refuse the candidate unless every slot agrees.
fn edge_flange_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignEdgeFlangeOperation> {
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
    let mut unclaimed = UnclaimedReferences::new(references)?;
    let edge_wrapper_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(edge_flange::EDGE_WRAPPER_REFERENCE)?,
    )?;
    let settings_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(edge_flange::SETTINGS_REFERENCE)?)?;
    let height_datum = DesignSheetMetalHeightDatum::from_code(View::u32_le_at(
        bytes,
        common.checked_add(edge_flange::HEIGHT_DATUM)?,
    )?);
    let angle_owner_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(edge_flange::ANGLE_OWNER_REFERENCE)?,
    )?;
    let height_owner_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(edge_flange::HEIGHT_OWNER_REFERENCE)?,
    )?;
    let bend_radius_offset = common.checked_add(edge_flange::INSIDE_BEND_RADIUS)?;
    let bend_radius = PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    let result_count =
        usize::try_from(View::u32_le_at(bytes, bend_radius_offset.checked_add(14)?)?).ok()?;
    // The aggregate-group and role-`0x08` group slots close the section after the
    // result-record run, so they also confirm the recovered result count.
    let aggregate_slot = bend_radius_offset
        .checked_add(22)?
        .checked_add(result_count.checked_mul(15)?)?;
    let aggregate_group_record_index = unclaimed.claim_slot(bytes, aggregate_slot)?;
    let first_edge_group = marked_record_reference(bytes, aggregate_slot.checked_add(27)?)?;
    // A group's recipe-backed operand is the record three after the group.
    let aggregate_operand_record_index =
        unclaimed.claim(aggregate_group_record_index.checked_add(3)?)?;
    let edge_group_record_index = unclaimed.claim(first_edge_group)?;
    unclaimed.claim(first_edge_group.checked_add(3)?)?;
    // Eight roles are claimed from a table of at most ten entries, so at most
    // `MAX_EDGE_WIDTH_DISTANCE_OWNERS` width owners remain.
    let width_distance_owner_record_indices: Vec<u32> = unclaimed.remaining().collect();
    let expected_length = 493usize
        .checked_add(result_count.checked_mul(15)?)?
        .checked_add(width_distance_owner_record_indices.len().checked_mul(11)?)?
        .checked_add(header_shift)?;
    if paired_at.checked_sub(start)? != expected_length {
        return None;
    }
    Some(DesignEdgeFlangeOperation {
        height_owner_record_index,
        angle_owner_record_index,
        auxiliary_reference_record_indices: Vec::new(),
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        height_datum,
        bend_position,
        selection: DesignEdgeFlangeSelection::try_new(
            DesignEdgeFlangeShape::from_wire(
                vec![DesignEdgeFlangeEdge {
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
    })
}

/// Read the single-edge `EdgeFlange` form whose height is measured from a
/// selected construction entity.
fn edge_flange_to_object_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignEdgeFlangeOperation> {
    // This form has one target group and one target entity-selection operand in
    // addition to the distance form's roles. The two marked references between
    // the target group and the aggregate group are fixed-frame references, not
    // entries in the scope's ordered reference table, and are retained as
    // native references for rewrite.
    let references: &[u32; 11] = references.try_into().ok()?;
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    let bend_position = DesignBendPosition::from_code(View::u32_le_at(bytes, common)?);
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }
    let mut unclaimed = UnclaimedReferences::new(references)?;
    let edge_wrapper_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(edge_flange::EDGE_WRAPPER_REFERENCE)?,
    )?;
    let settings_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(edge_flange::SETTINGS_REFERENCE)?)?;
    let height_datum = DesignSheetMetalHeightDatum::from_code(View::u32_le_at(
        bytes,
        common.checked_add(edge_flange::HEIGHT_DATUM)?,
    )?);
    let angle_owner_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(edge_flange::ANGLE_OWNER_REFERENCE)?,
    )?;
    let height_owner_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(edge_flange::HEIGHT_OWNER_REFERENCE)?,
    )?;
    let bend_radius_offset = common.checked_add(edge_flange::INSIDE_BEND_RADIUS)?;
    let bend_radius = PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    if View::u32_le_at(bytes, bend_radius_offset.checked_add(14)?)? != 1
        || !zeros_at::<4>(bytes, bend_radius_offset.checked_add(18)?)
        || !zeros_at::<5>(bytes, common.checked_add(89)?)
    {
        return None;
    }
    let target_group_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(flange_to_object::TARGET_GROUP_REFERENCE)?,
    )?;
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
        || !zeros_at::<4>(bytes, common.checked_add(135)?)
        || View::u32_le_at(
            bytes,
            common.checked_add(flange_to_object::AGGREGATE_REFERENCE_COUNT)?,
        )? != 1
        || !zeros_at::<12>(bytes, common.checked_add(154)?)
        || View::u32_le_at(
            bytes,
            common.checked_add(flange_to_object::EDGE_REFERENCE_COUNT)?,
        )? != 1
    {
        return None;
    }
    let aggregate_group_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(flange_to_object::AGGREGATE_GROUP_REFERENCE)?,
    )?;
    let edge_group_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(flange_to_object::EDGE_GROUP_REFERENCE)?,
    )?;
    let target_operand_record_index = unclaimed.claim(target_group_record_index.checked_add(3)?)?;
    let aggregate_operand_record_index =
        unclaimed.claim(aggregate_group_record_index.checked_add(3)?)?;
    unclaimed.claim(edge_group_record_index.checked_add(3)?)?;
    let mut remaining = unclaimed.remaining();
    let offset_owner_record_index = remaining.next()?;
    if remaining.next().is_some() || paired_at.checked_sub(start)? != 576 + header_shift {
        return None;
    }
    Some(DesignEdgeFlangeOperation {
        height_owner_record_index,
        angle_owner_record_index,
        auxiliary_reference_record_indices: Vec::new(),
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        height_datum,
        bend_position,
        selection: DesignEdgeFlangeSelection::try_new(
            DesignEdgeFlangeShape::FullEdge {
                edges: vec![DesignEdgeFlangeEdge {
                    wrapper: edge_wrapper_record_index,
                    group_record_index: edge_group_record_index.try_into().ok()?,
                    aggregate_operand_record_index,
                }],
                height: DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index,
                    target_operand_record_index,
                    offset_owner_record_index,
                    reference_record_indices,
                },
            },
            aggregate_group_record_index,
        )
        .ok()?,
    })
}

/// The `Hem` form that the frame from `start` to `paired_at` admits and whose
/// parameter owners `has_kind` confirms. The header shift and form are
/// recovered by agreement, so all candidates are evaluated and a frame that
/// admits more than one is refused.
fn exact_hem_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &ReferenceRun<u32>,
    has_kind: &impl Fn(u32, &str) -> Result<bool, CodecError>,
) -> Result<Option<DesignHemOperation>, CodecError> {
    let mut resolved = None;
    for header_shift in SHEET_METAL_HEADER_SHIFTS {
        let candidates = [
            hem_gap_length_operation_at(bytes, start, paired_at, references, header_shift),
            hem_radius_angle_operation_at(bytes, start, paired_at, references, header_shift),
            hem_gap_length_radius_operation_at(bytes, start, paired_at, references, header_shift),
        ];
        for candidate in candidates.into_iter().flatten() {
            if !hem_parameter_kinds_match(&candidate, has_kind)? {
                continue;
            }
            if resolved.replace(candidate).is_some() {
                return Ok(None);
            }
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
    if !matches!(scope.payload(), DesignScopePayload::Hem(_)) {
        return Ok(());
    }
    let (Ok(start), Ok(paired_at)) = (
        usize::try_from(scope.byte_offset()),
        usize::try_from(scope.paired_byte_offset()),
    ) else {
        return Ok(());
    };
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(());
    };
    let scope_record_index = scope.record_index;
    // The owners in this scope's stream that carry `record_index` must bind
    // exactly one parameter of that stream, and its source kind is `expected`.
    // Every owner record the `Hem` readers name is claimed from the scope's
    // reference table, so it is a listed reference member.
    let has_kind = |record_index: u32, expected: &str| -> Result<bool, CodecError> {
        // Whether the first bound parameter has the expected kind; the search
        // stops at a second bound parameter.
        let mut first_kind_matches = None;
        let second = ctx.position_by(
            parameter_owners,
            |owner| {
                if owner.scope_record_index() != scope_record_index
                    || owner.record_index() != record_index
                    || !in_stream(ctx, owner.id(), stream)?
                {
                    return Ok(false);
                }
                ctx.any_by(
                    parameters,
                    |parameter| {
                        if parameter.record_index != owner.parameter_record_index()
                            || !in_stream(ctx, &parameter.id, stream)?
                        {
                            return Ok(false);
                        }
                        if first_kind_matches.is_some() {
                            return Ok(true);
                        }
                        first_kind_matches = Some(ctx.equal_bytes(
                            parameter.source_kind().as_bytes(),
                            expected.as_bytes(),
                            "match F3D Hem parameter kind",
                        )?);
                        Ok(false)
                    },
                    "find F3D Hem owner parameters",
                )
            },
            "find F3D Hem parameter owners",
        )?;
        Ok(second.is_none() && first_kind_matches == Some(true))
    };
    let construction = exact_hem_operation(
        bytes,
        start,
        paired_at,
        scope.reference_members(),
        &has_kind,
    )?;
    if let DesignScopePayloadMut::Hem(slot) = scope.payload_mut() {
        *slot = construction;
    }
    Ok(())
}

/// The fixed operation section of a `Hem` form whose frame is `frame_length`
/// bytes before the header shift and whose reference table lists `N` records:
/// the section start and the unclaimed reference pool.
fn hem_section<const N: usize>(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &ReferenceRun<u32>,
    header_shift: usize,
    frame_length: usize,
) -> Option<(usize, UnclaimedReferences)> {
    if paired_at.checked_sub(start)? != frame_length.checked_add(header_shift)? {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }
    Some((common, UnclaimedReferences::from_run::<N>(references)?))
}

/// Claim the aggregate and edge groups named at `aggregate_slot` and
/// `edge_slot` and their operands, the records three after each group. Every
/// reference must then be claimed.
fn claim_hem_groups(
    unclaimed: &mut UnclaimedReferences,
    bytes: &[u8],
    common: usize,
    aggregate_slot: usize,
    edge_slot: usize,
) -> Option<(u32, u32)> {
    let aggregate_group_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(aggregate_slot)?)?;
    let edge_group_record_index = unclaimed.claim_slot(bytes, common.checked_add(edge_slot)?)?;
    unclaimed.claim(aggregate_group_record_index.checked_add(3)?)?;
    unclaimed.claim(edge_group_record_index.checked_add(3)?)?;
    if unclaimed.remaining().next().is_some() {
        return None;
    }
    Some((aggregate_group_record_index, edge_group_record_index))
}

/// Read the gap-and-length `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
///
/// The ordered reference table is in record-index order, so every role is taken
/// from the marked slot that names it and each group's operand is the record
/// three after that group. The rolled and teardrop forms place their owner
/// references at other offsets and are handled by their corresponding readers.
fn hem_gap_length_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &ReferenceRun<u32>,
    header_shift: usize,
) -> Option<DesignHemOperation> {
    let (common, mut unclaimed) =
        hem_section::<8>(bytes, start, paired_at, references, header_shift, 494)?;
    let edge_wrapper_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(hem_gap::EDGE_WRAPPER_REFERENCE)?)?;
    let settings_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(hem_gap::SETTINGS_REFERENCE)?)?;
    // The two owners are the form's inputs in local-ordinal order.
    let gap_owner_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(hem_gap::GAP_OWNER_REFERENCE)?)?;
    let length_owner_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(hem_gap::LENGTH_OWNER_REFERENCE)?)?;
    let bend_radius_offset = common.checked_add(hem_gap::INSIDE_BEND_RADIUS)?;
    let bend_radius = PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    let (aggregate_group_record_index, edge_group_record_index) =
        claim_hem_groups(&mut unclaimed, bytes, common, 108, 135)?;
    Some(DesignHemOperation {
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
    })
}

/// Read the rolled `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
///
/// Rolled forms keep the two-owner frame length, but their owner slots are at
/// offsets `41` and `54` instead of `42` and `53`. The source parameter kinds
/// assign those slots to radius and angle; the fixed frame only proves their
/// record identities.
fn hem_radius_angle_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &ReferenceRun<u32>,
    header_shift: usize,
) -> Option<DesignHemOperation> {
    let (common, mut unclaimed) =
        hem_section::<8>(bytes, start, paired_at, references, header_shift, 494)?;
    let edge_wrapper_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(hem_gap::EDGE_WRAPPER_REFERENCE)?)?;
    let settings_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(hem_gap::SETTINGS_REFERENCE)?)?;
    let angle_owner_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(hem_rolled::ANGLE_OWNER_REFERENCE)?,
    )?;
    let radius_owner_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(hem_rolled::RADIUS_OWNER_REFERENCE)?,
    )?;
    let bend_radius_offset = common.checked_add(hem_rolled::INSIDE_BEND_RADIUS)?;
    let bend_radius = PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    let (aggregate_group_record_index, edge_group_record_index) =
        claim_hem_groups(&mut unclaimed, bytes, common, 108, 135)?;
    Some(DesignHemOperation {
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
    })
}

/// Read the teardrop `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
fn hem_gap_length_radius_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &ReferenceRun<u32>,
    header_shift: usize,
) -> Option<DesignHemOperation> {
    let (common, mut unclaimed) =
        hem_section::<9>(bytes, start, paired_at, references, header_shift, 515)?;
    let edge_wrapper_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(hem_gap::EDGE_WRAPPER_REFERENCE)?)?;
    let settings_record_index =
        unclaimed.claim_slot(bytes, common.checked_add(hem_gap::SETTINGS_REFERENCE)?)?;
    let gap_owner_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(hem_teardrop::GAP_OWNER_REFERENCE)?,
    )?;
    let length_owner_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(hem_teardrop::LENGTH_OWNER_REFERENCE)?,
    )?;
    let radius_owner_record_index = unclaimed.claim_slot(
        bytes,
        common.checked_add(hem_teardrop::RADIUS_OWNER_REFERENCE)?,
    )?;
    let bend_radius_offset = common.checked_add(hem_teardrop::INSIDE_BEND_RADIUS)?;
    let bend_radius = PositiveReal::new(View::f64_le_at(bytes, bend_radius_offset)?)?;
    let (aggregate_group_record_index, edge_group_record_index) =
        claim_hem_groups(&mut unclaimed, bytes, common, 118, 145)?;
    Some(DesignHemOperation {
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
    })
}

#[cfg(test)]
mod tests;
