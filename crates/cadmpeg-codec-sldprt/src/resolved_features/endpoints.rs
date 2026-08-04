//! Curve endpoint index decoders.

use super::curves::compact_bounded_curve_tangent;
use super::dimensions::compact_legacy_radial_circle_index;
use super::markers::{
    alternate_current_curve_body, compact_legacy_marker_body,
    current_reverse_incidence_endpoint_offsets, finite_coordinate_pair, marker_is_geometry_locus,
    marker_native_code, marker_object_index, packed_legacy_marker_body, sketch_marker_prefix_at,
};
use super::relation_loci::same_dimension_length;
use super::scalars::operand_kind;
use super::selections::operand_accepts_marker;
use super::transforms::quantize;
use super::typed_relations::{legacy_marker104_arc_endpoints, marker_curve_endpoint_markers};
use super::{CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use crate::records::{
    FeatureInputLane, FeatureInputOperandKind, FeatureInputScalarRole, SketchInputEntity,
    SketchInputKind,
};
use cadmpeg_ir::features::{Angle, Length};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::SketchGeometry;
use std::collections::{HashMap, HashSet};

// Curve endpoint-index decoders, one per record layout, tried in precedence
// order. The first layout that accepts the bytes at `offset` yields the pair;
// order is load-bearing because a record can satisfy more than one layout's
// guards and the earliest entry must win.
type CurveEndpointDecoder = fn(&[u8], usize) -> Option<[u32; 2]>;

const CURVE_ENDPOINT_INDEX_DECODERS: &[CurveEndpointDecoder] = &[
    legacy_long_profile_line_endpoint_indices,
    linked_profile_curve_endpoint_indices,
    legacy_direct_compact_selected_axis_endpoint_indices,
    legacy_compact_roster_selected_axis_endpoint_indices,
    extended_tagged_indexed_curve_endpoint_indices,
    current_direct_92_profile_line_endpoint_indices,
    wide_indexed_curve_endpoint_indices,
    compact_indexed_curve_endpoint_indices,
    direct_indexed_curve_endpoint_indices,
    extended_compact_84_construction_line_endpoint_indices,
    extended_compact_96_selected_axis_endpoint_indices,
    extended_compact_indexed_curve_endpoint_indices,
    legacy_compact_104_profile_line_endpoint_indices,
    compact_legacy_curve_endpoint_indices,
    compact_legacy_short_role_two_curve_endpoint_indices,
    compact_legacy_short_role_one_curve_endpoint_indices,
    alternate_current_indexed_curve_endpoint_indices,
    current_compact_104_indexed_line_endpoint_indices,
    extended_profile_roster_construction_line_endpoint_indices,
    legacy_referenced_wide_arc_endpoint_indices,
    legacy_state_five_curve_endpoint_indices,
    legacy_coordinate_roster_selected_axis_endpoint_indices,
    legacy_profile_roster_selected_axis_endpoint_indices,
    standard_legacy_compact_selected_axis_endpoint_indices,
    compact_legacy_selected_axis_endpoint_indices,
    alternate_current_selected_axis_endpoint_indices,
    legacy_code_five_or_six_selected_axis_endpoint_indices,
    compact_curve_endpoint_indices,
    extended_horizontal_axis_endpoint_indices,
    current_vertical_axis_endpoint_indices,
    extended_wide_horizontal_relation_endpoint_indices,
];

fn resolved_curve_endpoint_indices(payload: &[u8], offset: usize) -> Option<[u32; 2]> {
    CURVE_ENDPOINT_INDEX_DECODERS
        .iter()
        .find_map(|decode| decode(payload, offset))
}

pub(super) fn extended_direct_object_line_endpoint_ids(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if !matches!(
        payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()),
        Some(prefix)
            if prefix == LEGACY_EXTENDED_SKETCH_MARKER || prefix == LEGACY_SKETCH_MARKER
    ) || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || !matches!(marker_native_code(payload, offset), Some(0..=2))
        || !matches!(
            payload.get(offset + 23..offset + 27),
            Some([0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
        )
        || payload.get(offset + 27..offset + 31) != Some(&[0x01, 0x00, 0x01, 0x00])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00])
        || payload.get(offset + 39..offset + 48) != Some(&[0; 9])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 76) != Some(&[0; 4])
        || payload
            .get(offset + 76..offset + 84)
            .is_none_or(|trailer| trailer == [0xff; 8])
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    let endpoint = |relative| {
        Some(u32::from(u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        )))
    };
    let endpoints = [endpoint(56)?, endpoint(58)?];
    (endpoints[0] != endpoints[1] && endpoints.iter().all(|id| *id != u32::from(u16::MAX)))
        .then_some(endpoints)
}

struct LinkedProfileCurveRecord {
    inline: [f64; 2],
    references: [u32; 2],
    state: u16,
    reference_count: u16,
    tail_flag: u32,
    identity: u32,
}

fn linked_profile_curve_record(payload: &[u8], offset: usize) -> Option<LinkedProfileCurveRecord> {
    if !matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix) if prefix == SKETCH_MARKER || prefix == LEGACY_EXTENDED_SKETCH_MARKER
    ) || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || payload.get(offset + 94..offset + 100) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 100..offset + 136) != Some(&[0; 36])
        || payload.get(offset + 140..offset + 142) != Some(&[0; 2])
        || !sketch_marker_prefix_at(payload, offset.checked_add(146)?)
    {
        return None;
    }
    let endpoint = |relative| {
        let cell = payload.get(offset + relative..offset + relative + 8)?;
        let kind = operand_kind(cell[..2].try_into().ok()?)?;
        if !operand_accepts_marker(kind, SketchInputKind::Point) || cell[4..8] != [0xff; 4] {
            return None;
        }
        Some(u32::from(u16::from_le_bytes(cell[2..4].try_into().ok()?)))
    };
    let references = [endpoint(78)?, endpoint(86)?];
    if references[0] == references[1] {
        return None;
    }
    Some(LinkedProfileCurveRecord {
        inline: finite_coordinate_pair(payload, offset + 58)?,
        references,
        state: u16::from_le_bytes(payload.get(offset + 74..offset + 76)?.try_into().ok()?),
        reference_count: u16::from_le_bytes(
            payload.get(offset + 76..offset + 78)?.try_into().ok()?,
        ),
        tail_flag: u32::from_le_bytes(payload.get(offset + 136..offset + 140)?.try_into().ok()?),
        identity: u32::from_le_bytes(payload.get(offset + 142..offset + 146)?.try_into().ok()?),
    })
}

pub(super) fn linked_profile_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    let record = linked_profile_curve_record(payload, offset)?;
    (record.state == 0
        && record.reference_count == 3
        && record.tail_flag == 0
        && !matches!(record.identity, 0 | u32::MAX))
    .then_some(record.references)
}

pub(super) fn legacy_long_profile_line_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || marker_native_code(payload, offset) != Some(0)
        || payload.get(offset + 19..offset + 23) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 25..offset + 27) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 27..offset + 33) != Some(&[0, 0, 0, 0, 4, 0])
        || payload.get(offset + 33..offset + 42) != Some(&[0; 9])
        || payload.get(offset + 46..offset + 50) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 50..offset + 58) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 58..offset + 62) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 62..offset + 64) != Some(&7u16.to_le_bytes())
        || payload.get(offset + 64..offset + 80)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 80..offset + 120) != Some(&[0; 40])
        || payload.get(offset + 120..offset + 124) != Some(&16u32.to_le_bytes())
    {
        return None;
    }
    let endpoint = |relative| {
        let id = u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        );
        (id != 0 && id != u16::MAX).then_some(u32::from(id))
    };
    let endpoints = [endpoint(42)?, endpoint(44)?];
    (endpoints[0] != endpoints[1]).then_some(endpoints)
}

pub(super) fn extended_tagged_indexed_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(0)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 76) != Some(&1i32.to_le_bytes())
        || payload.get(offset + 78..offset + 94)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 94..offset + 96) != Some(&[0; 2])
        || !extended_tagged_indexed_curve_record_ends(payload, offset)
    {
        return None;
    }
    let endpoint = |relative| {
        let id = u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        );
        (id != 0 && id != u16::MAX).then_some(u32::from(id))
    };
    let endpoints = [endpoint(58)?, endpoint(76)?];
    (endpoints[0] != endpoints[1]).then_some(endpoints)
}

fn extended_tagged_indexed_curve_record_ends(payload: &[u8], offset: usize) -> bool {
    if compact_indexed_curve_record_end(payload, offset)
        == Some(CompactIndexedCurveRecordEnd::Marker104)
    {
        return true;
    }
    let u32_at = |relative| {
        payload
            .get(offset + relative..offset + relative + 4)?
            .try_into()
            .ok()
            .map(u32::from_le_bytes)
    };
    let counts = [166, 170, 174, 178].map(u32_at);
    payload.get(offset + 94..offset + 150) == Some(&[0; 56])
        && payload.get(offset + 150..offset + 152) == Some(&[0x08, 0x80])
        && payload.get(offset + 152..offset + 162) == Some(&[0; 10])
        && payload.get(offset + 162..offset + 166) == Some(&[0x01, 0x00, 0x01, 0x00])
        && matches!(
            counts,
            [Some(first), Some(second), Some(third), Some(fourth)]
                if first > second && second > third && third > fourth && fourth != 0
        )
        && payload.get(offset + 182..offset + 230)
            == Some(&[
                1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0,
                1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0,
            ])
        && payload.get(offset + 230..offset + 258)
            == Some(&[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xfe, 0xff, 0x00, 0xff, 0xff, 0x00, 0x00,
                0x80, 0xbf, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
            ])
        && payload.get(offset + 258..offset + 282) == Some(&[0; 24])
        && u32_at(282).is_some_and(|identity| identity != 0 && identity != u32::MAX)
        && payload.get(offset + 286..offset + 338) == Some(&[0; 52])
        && u32_at(338) == Some(3)
        && u32_at(342) == Some(1)
        && payload.get(offset + 346..offset + 353) == Some(&[0; 7])
        && payload
            .get(offset + 353..offset + 357)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && payload.get(offset + 357..offset + 359) == Some(&5u16.to_le_bytes())
        && class_declaration_at(payload, offset.saturating_add(359))
}

pub(super) fn roster_curve_endpoint_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Vec<&'a SketchInputEntity> {
    let Some(offset) = usize::try_from(curve.offset).ok() else {
        return Vec::new();
    };
    let selected_construction = marker_is_selected_construction_line(payload, offset);
    let boundary_relation =
        extended_wide_horizontal_relation_endpoint_indices(payload, offset).is_some();
    if curve.coordinates_m.is_some()
        || (!matches!(
            curve.kind,
            SketchInputKind::LineOrCircle | SketchInputKind::Arc
        ) && !selected_construction
            && !boundary_relation)
    {
        return Vec::new();
    }
    if let Some((endpoints, _)) = current_wide_arc_direct_markers(payload, curve, markers) {
        return endpoints.to_vec();
    }
    if curve.kind == SketchInputKind::Arc
        && extended_marker104_arc_uses_point_roster(payload, offset)
    {
        let endpoints = coordinate_roster_curve_endpoint_markers(payload, curve, markers);
        if endpoints.len() == 2 {
            return endpoints;
        }
    }
    if curve.kind == SketchInputKind::LineOrCircle
        && (extended_marker84_line_uses_point_roster(payload, offset)
            || extended_compact_84_profile_line_uses_point_roster(payload, offset)
            || legacy_compact_84_profile_line_uses_point_roster(payload, offset))
    {
        let endpoints = coordinate_roster_curve_endpoint_markers(payload, curve, markers);
        if endpoints.len() == 2 {
            return endpoints;
        }
    }
    if let Some(offsets) = current_reverse_incidence_endpoint_offsets(payload, curve, markers) {
        let endpoints = offsets
            .into_iter()
            .filter_map(|offset| {
                markers.iter().copied().find(|marker| {
                    marker.offset == offset
                        && marker.coordinates_m.is_some()
                        && matches!(
                            marker.kind,
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        )
                })
            })
            .collect::<Vec<_>>();
        if endpoints.len() == 2 {
            return endpoints;
        }
    }
    if packed_legacy_curve_endpoint_indices(payload, offset).is_some()
        || packed_compact_legacy_curve_endpoint_indices(payload, offset).is_some()
        || extended_compact_legacy_curve_record(payload, offset)
    {
        let endpoints = coordinate_roster_curve_endpoint_markers(payload, curve, markers);
        if endpoints.len() == 2 {
            return endpoints;
        }
    }
    if let Some(indices) = extended_wide_construction_line_roster_indices(payload, offset) {
        let mut owned = markers
            .iter()
            .copied()
            .filter(|marker| marker.feature_ref == curve.feature_ref)
            .collect::<Vec<_>>();
        owned.sort_unstable_by_key(|marker| marker.offset);
        let endpoints = indices
            .into_iter()
            .filter_map(|index| owned.get(index).copied())
            .filter(|marker| marker.coordinates_m.is_some())
            .collect::<Vec<_>>();
        if endpoints.len() == 2 && endpoints[0].id != endpoints[1].id {
            return endpoints;
        }
    }
    if let Some(indices) = resolved_curve_endpoint_indices(payload, offset) {
        let indexed = indices
            .into_iter()
            .filter_map(|index| {
                let mut candidates = markers.iter().copied().filter(|marker| {
                    marker.feature_ref == curve.feature_ref
                        && marker.object_index == Some(index)
                        && marker.coordinates_m.is_some()
                        && (selected_construction
                            || matches!(
                                marker.kind,
                                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                            ))
                });
                let candidate = candidates.next()?;
                candidates.next().is_none().then_some(candidate)
            })
            .collect::<Vec<_>>();
        if coordinate_roster_curve_layout(payload, offset) {
            let roster = coordinate_roster_curve_endpoint_markers(payload, curve, markers);
            let legacy = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
                == Some(LEGACY_SKETCH_MARKER);
            let roster_preferred = (legacy
                && compact_legacy_short_role_two_curve_endpoint_indices(payload, offset).is_none())
                || current_compact_104_indexed_line_endpoint_indices(payload, offset).is_some();
            if roster_preferred || indexed.len() != 2 {
                if roster.len() == 2 {
                    return roster;
                }
                if indexed.len() == 2 {
                    return indexed;
                }
                if roster_preferred {
                    return legacy_compact_direct_endpoint_markers(payload, offset, curve, markers);
                }
            }
        }
        if indexed.len() != 2 {
            if let Some(direct) = wide_direct_line_endpoint_markers(payload, curve, markers) {
                return direct.to_vec();
            }
            let direct = extended_compact_endpoint_markers(payload, curve, markers);
            if direct.len() == 2 {
                return direct;
            }
            let terminal =
                extended_terminal_84_construction_line_endpoint_markers(payload, curve, markers);
            if terminal.len() == 2 {
                return terminal;
            }
        }
        return indexed;
    }
    let direct = extended_compact_endpoint_markers(payload, curve, markers);
    if direct.len() == 2 {
        return direct;
    }
    let terminal = extended_terminal_84_construction_line_endpoint_markers(payload, curve, markers);
    if terminal.len() == 2 {
        return terminal;
    }
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
    {
        let mut owned = markers
            .iter()
            .copied()
            .filter(|marker| marker.feature_ref == curve.feature_ref)
            .collect::<Vec<_>>();
        owned.sort_unstable_by_key(|marker| marker.offset);
        let endpoints = [56, 58]
            .into_iter()
            .map(|relative| {
                let index = usize::from(u16::from_le_bytes(
                    payload
                        .get(offset + relative..offset + relative + 2)?
                        .try_into()
                        .ok()?,
                ));
                let endpoint = *owned.get(index)?;
                (endpoint.coordinates_m.is_some()
                    && matches!(
                        endpoint.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    ))
                .then_some(endpoint)
            })
            .collect::<Option<Vec<_>>>();
        return endpoints.unwrap_or_default();
    }
    let endpoint_offsets = if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
        == Some(LEGACY_SKETCH_MARKER)
        && matches!(marker_profile_curve_role(payload, offset), Some(1 | 2))
    {
        [64, 66]
    } else {
        return Vec::new();
    };
    endpoint_offsets
        .into_iter()
        .filter_map(|relative| {
            let index = u16::from_le_bytes(
                payload
                    .get(offset + relative..offset + relative + 2)?
                    .try_into()
                    .ok()?,
            )
            .checked_add(1)?;
            let mut candidates = markers.iter().copied().filter(|marker| {
                marker.feature_ref == curve.feature_ref
                    && marker.object_index == Some(u32::from(index))
                    && marker.coordinates_m.is_some()
                    && (selected_construction
                        || matches!(
                            marker.kind,
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        ))
            });
            let candidate = candidates.next()?;
            candidates.next().is_none().then_some(candidate)
        })
        .collect()
}

fn extended_terminal_84_construction_line_endpoint_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Vec<&'a SketchInputEntity> {
    let Some(offset) = usize::try_from(curve.offset).ok() else {
        return Vec::new();
    };
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 31)
            != Some(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&[0; 4])
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 80)
            != Some(&[0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00])
        || payload.get(offset + 80..offset + 84) != Some(&[0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.saturating_add(84))
    {
        return Vec::new();
    }
    let indices = [56, 58].map(|relative| {
        payload
            .get(offset + relative..offset + relative + 2)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
            .and_then(|index| usize::from(index).checked_sub(1))
    });
    let [Some(first), Some(second)] = indices else {
        return Vec::new();
    };
    if first == second {
        return Vec::new();
    }
    let mut owned = markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref == curve.feature_ref)
        .collect::<Vec<_>>();
    owned.sort_unstable_by_key(|marker| marker.offset);
    let endpoints = [first, second].map(|index| {
        owned.get(index).copied().filter(|marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
        })
    });
    match endpoints {
        [Some(first), Some(second)] if first.id != second.id => vec![first, second],
        _ => Vec::new(),
    }
}

pub(super) fn extended_wide_construction_line_roster_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[usize; 2]> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&[0; 4])
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload
            .get(offset + 88..offset + 92)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    match (
        payload.get(offset + 80..offset + 84),
        payload.get(offset + 84..offset + 88),
    ) {
        (Some([0x00, 0x00, 0x00, 0x00]), Some(identity))
            if identity != [0; 4] && identity != [0xff; 4] => {}
        (Some([0x00, 0x00, 0x01, 0x00]), Some([0x00, 0x00, 0x00, 0x00])) => {}
        _ => return None,
    }
    let indices = [64, 66].map(|relative| {
        Some(usize::from(u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        )))
    });
    let [Some(first), Some(second)] = indices else {
        return None;
    };
    let indices = [first, second];
    (indices[0] != indices[1]).then_some(indices)
}

pub(super) fn extended_compact_endpoint_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Vec<&'a SketchInputEntity> {
    let Some(offset) = usize::try_from(curve.offset).ok() else {
        return Vec::new();
    };
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || !matches!(marker_native_code(payload, offset), Some(0..=2))
        || !matches!(
            (
                marker_profile_curve_role(payload, offset),
                payload.get(offset + 31..offset + 39)
            ),
            (
                Some(1),
                Some([0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
            ) | (
                Some(2),
                Some([0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
            )
        )
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || !(matches!(
            compact_indexed_curve_record_end(payload, offset),
            Some(
                CompactIndexedCurveRecordEnd::Marker84
                    | CompactIndexedCurveRecordEnd::Marker96
                    | CompactIndexedCurveRecordEnd::Marker104
                    | CompactIndexedCurveRecordEnd::Terminal102
                    | CompactIndexedCurveRecordEnd::Terminal116
                    | CompactIndexedCurveRecordEnd::Continuation120
            )
        ) || payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
            && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
            && payload.get(offset + 72..offset + 102) == Some(&[0; 30])
            && !sketch_marker_prefix_at(payload, offset.saturating_add(102)))
    {
        return Vec::new();
    }
    let ids = [56, 58].map(|relative| {
        payload
            .get(offset + relative..offset + relative + 2)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
            .map(u32::from)
    });
    let [Some(first), Some(second)] = ids else {
        return Vec::new();
    };
    if first == second {
        return Vec::new();
    }
    let endpoint_by_object = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && if id == 0 {
                    marker.object_index.is_none()
                } else {
                    marker.object_index == Some(id)
                }
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    match (endpoint_by_object(first), endpoint_by_object(second)) {
        (Some(first), Some(second)) if first.id != second.id => vec![first, second],
        _ => {
            if compact_indexed_curve_record_end(payload, offset)
                == Some(CompactIndexedCurveRecordEnd::Terminal116)
            {
                let mut owned = markers
                    .iter()
                    .copied()
                    .filter(|marker| {
                        marker.feature_ref == curve.feature_ref
                            && marker.coordinates_m.is_some()
                            && matches!(
                                marker.kind,
                                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                            )
                    })
                    .collect::<Vec<_>>();
                owned.sort_unstable_by_key(|marker| marker.offset);
                let endpoint = |id: u32| {
                    let index = usize::try_from(id.checked_sub(1)?).ok()?;
                    owned.get(index).copied()
                };
                if let (Some(first), Some(second)) = (endpoint(first), endpoint(second)) {
                    if first.id != second.id {
                        return vec![first, second];
                    }
                }
            }
            let endpoint_by_roster_index = |id| {
                let mut candidates = markers.iter().copied().filter(|marker| {
                    marker.feature_ref == curve.feature_ref
                        && marker.ordinal == id
                        && marker.coordinates_m.is_some()
                        && matches!(
                            marker.kind,
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        )
                });
                let candidate = candidates.next()?;
                candidates.next().is_none().then_some(candidate)
            };
            match (
                endpoint_by_roster_index(first),
                endpoint_by_roster_index(second),
            ) {
                (Some(first), Some(second)) if first.id != second.id => vec![first, second],
                _ => Vec::new(),
            }
        }
    }
}

pub(super) fn legacy_compact_direct_endpoint_markers<'a>(
    payload: &[u8],
    offset: usize,
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Vec<&'a SketchInputEntity> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || compact_indexed_curve_endpoint_indices(payload, offset).is_none()
        || !sketch_marker_prefix_at(payload, offset.saturating_add(84))
    {
        return Vec::new();
    }
    let endpoint = |relative: usize| {
        payload
            .get(offset + relative..offset + relative + 2)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
            .map(u32::from)
    };
    let Some(indices) = endpoint(56).zip(endpoint(58)) else {
        return Vec::new();
    };
    if indices.0 == 0 || indices.0 == indices.1 {
        return Vec::new();
    }
    let endpoints = [indices.0, indices.1]
        .into_iter()
        .filter_map(|index| {
            let mut candidates = markers.iter().copied().filter(|marker| {
                marker.feature_ref == curve.feature_ref
                    && marker.object_index == Some(index)
                    && marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
            });
            let candidate = candidates.next()?;
            candidates.next().is_none().then_some(candidate)
        })
        .collect::<Vec<_>>();
    if endpoints.len() == 2 {
        endpoints
    } else {
        Vec::new()
    }
}

pub(super) fn current_wide_arc_direct_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<([&'a SketchInputEntity; 2], [f64; 2])> {
    let offset = usize::try_from(curve.offset).ok()?;
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || wide_indexed_curve_endpoint_indices(payload, offset).is_none()
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    let endpoint = |relative: usize| {
        payload
            .get(offset + relative..offset + relative + 2)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
            .map(u32::from)
    };
    let raw = [endpoint(64)?, endpoint(66)?];
    if raw[0] == 0 || raw[1] == 0 || raw[0] == raw[1] {
        return None;
    }
    let resolve = |indices: [u32; 2]| {
        let endpoints = indices.map(|index| {
            let mut matches = markers.iter().copied().filter(|marker| {
                marker.feature_ref == curve.feature_ref
                    && marker.object_index == Some(index)
                    && marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
            });
            let endpoint = matches.next()?;
            matches.next().is_none().then_some(endpoint)
        });
        Some([endpoints[0]?, endpoints[1]?])
    };
    let has_unique_center = |endpoints: [&SketchInputEntity; 2]| {
        let [start_u, start_v] = endpoints[0].coordinates_m?;
        let [end_u, end_v] = endpoints[1].coordinates_m?;
        let candidates = markers
            .iter()
            .filter(|marker| {
                marker.feature_ref == curve.feature_ref && marker.kind == SketchInputKind::Arc
            })
            .filter_map(|marker| marker.coordinates_m)
            .map(|[u, v]| Point2::new(u, v))
            .collect::<Vec<_>>();
        unique_arc_center_marker(
            Point2::new(start_u, start_v),
            Point2::new(end_u, end_v),
            &candidates,
            1.0e-9,
        )
    };
    let direct = resolve(raw)?;
    let center = has_unique_center(direct)?;
    Some((direct, [center.u, center.v]))
}

pub(super) fn wide_direct_line_endpoint_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if !matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix) if prefix == SKETCH_MARKER || prefix == LEGACY_EXTENDED_SKETCH_MARKER
    ) || marker_native_code(payload, offset) != Some(1)
        || wide_indexed_curve_endpoint_indices(payload, offset).is_none()
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    let endpoint_id = |relative| {
        Some(u32::from(u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        )))
    };
    let endpoint_ids = [endpoint_id(64)?, endpoint_id(66)?];
    if endpoint_ids[0] == endpoint_ids[1] {
        return None;
    }
    let resolve = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.object_index == (id != 0).then_some(id)
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let endpoints = endpoint_ids.map(resolve);
    let [Some(first), Some(second)] = endpoints else {
        return None;
    };
    (first.id != second.id).then_some([first, second])
}

pub(super) fn compact_curve_endpoint_indices(payload: &[u8], offset: usize) -> Option<[u32; 2]> {
    if !matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix)
            if prefix == LEGACY_SKETCH_MARKER
                || prefix == LEGACY_EXTENDED_SKETCH_MARKER
                || prefix == SKETCH_MARKER
    ) || marker_native_code(payload, offset) != Some(0)
        || !matches!(
            payload.get(offset + 23..offset + 27),
            Some(locus) if locus == [0x04, 0x00, 0x02, 0x00] || locus == [0x05, 0x00, 0x01, 0x00]
        )
        || !matches!(marker_profile_curve_role(payload, offset), Some(1 | 2))
        || !matches!(
            payload.get(offset + 35..offset + 39),
            Some([0x00, 0x00, 0x0c | 0x0d, 0x00])
        )
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 56)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn coordinate_roster_curve_endpoint_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Vec<&'a SketchInputEntity> {
    coordinate_roster_curve_endpoint_markers_at(payload, curve, markers, None)
}

pub(super) fn coordinate_roster_curve_endpoint_markers_at<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
    explicit_endpoint_offset: Option<usize>,
) -> Vec<&'a SketchInputEntity> {
    let Some(offset) = usize::try_from(curve.offset).ok() else {
        return Vec::new();
    };
    let current_complete_roster =
        current_referenced_compact_curve_uses_marker_roster(payload, offset);
    let complete_entity_roster = current_complete_roster
        || extended_marker84_line_uses_point_roster(payload, offset)
            && marker_profile_curve_role(payload, offset) == Some(2)
            && payload.get(offset + 72..offset + 76) == Some(&[0x00, 0x00, 0x01, 0x00]);
    let Some(endpoint_offset) =
        explicit_endpoint_offset.or_else(|| coordinate_roster_endpoint_offset(payload, offset))
    else {
        return Vec::new();
    };
    let one_based = (extended_marker84_line_uses_point_roster(payload, offset)
        && payload.get(offset + 27..offset + 31) == Some(&[0x01, 0x00, 0x01, 0x00])
        && payload.get(offset + 72..offset + 76) == Some(&[0x00, 0x00, 0x02, 0x00]))
        || current_identity_linked_wide_curve_uses_one_based_roster(payload, offset)
        || current_complete_roster;
    let resolve = |complete_entity_roster: bool, one_based: bool| {
        let mut coordinates = markers
            .iter()
            .copied()
            .filter(|marker| {
                marker.feature_ref == curve.feature_ref
                    && (complete_entity_roster
                        || marker.coordinates_m.is_some()
                            && matches!(
                                marker.kind,
                                SketchInputKind::Point
                                    | SketchInputKind::ConstrainedPoint
                                    | SketchInputKind::LineOrCircle
                                    | SketchInputKind::Arc
                            ))
            })
            .collect::<Vec<_>>();
        coordinates.sort_unstable_by_key(|marker| marker.offset);
        let endpoint = |relative: usize| {
            let index = usize::from(u16::from_le_bytes(
                payload
                    .get(offset + relative..offset + relative + 2)?
                    .try_into()
                    .ok()?,
            ));
            let index = if one_based {
                index.checked_sub(1)?
            } else {
                index
            };
            coordinates.get(index).copied().filter(|marker| {
                marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind,
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                    )
            })
        };
        let (Some(first), Some(second)) =
            (endpoint(endpoint_offset), endpoint(endpoint_offset + 2))
        else {
            return None;
        };
        (first.id != second.id).then_some(vec![first, second])
    };
    resolve(complete_entity_roster, one_based)
        .or_else(|| {
            (current_complete_roster
                && matches!(marker_native_code(payload, offset), Some(1 | 2))
                && compact_indexed_curve_record_end(payload, offset)
                    == Some(CompactIndexedCurveRecordEnd::Marker84))
            .then(|| resolve(false, false))
            .flatten()
        })
        .unwrap_or_default()
}

pub(super) fn output_curve_endpoint_markers<'a>(
    payload: &[u8],
    curve: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    markers: &[&'a SketchInputEntity],
) -> Vec<&'a SketchInputEntity> {
    if usize::try_from(curve.offset)
        .ok()
        .is_some_and(|offset| current_compact_roster_selected_axis(payload, offset))
    {
        let roster = coordinate_roster_curve_endpoint_markers_at(payload, curve, markers, Some(56));
        if roster.len() == 2 {
            return roster;
        }
    }
    marker_curve_endpoint_markers(payload, curve, markers_by_id, markers)
}

pub(super) fn current_identity_linked_wide_curve_uses_one_based_roster(
    payload: &[u8],
    offset: usize,
) -> bool {
    payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && wide_indexed_curve_endpoint_indices(payload, offset).is_some()
        && sketch_marker_prefix_at(payload, offset.saturating_add(92))
        && payload
            .get(offset + 88..offset + 92)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && current_direct_92_profile_line_endpoint_indices(payload, offset).is_none()
}

pub(super) fn current_referenced_compact_curve_uses_marker_roster(
    payload: &[u8],
    offset: usize,
) -> bool {
    let distinct_identities = |first: usize, second: usize| {
        let identities =
            [first, second].map(|relative| payload.get(offset + relative..offset + relative + 4));
        matches!(identities, [Some(first), Some(second)] if first != [0; 4] && first != [0xff; 4] && second != [0; 4] && second != [0xff; 4] && first != second)
    };
    let referenced_ending = match compact_indexed_curve_record_end(payload, offset) {
        Some(CompactIndexedCurveRecordEnd::Marker84) => {
            payload.get(offset + 72..offset + 76) == Some(&[0; 4]) && distinct_identities(76, 80)
        }
        Some(CompactIndexedCurveRecordEnd::Marker96) => distinct_identities(88, 92),
        Some(CompactIndexedCurveRecordEnd::Marker104) => {
            payload
                .get(offset + 76..offset + 78)
                .is_some_and(|state| state != [0; 2] && state != [0xff; 2])
                && distinct_identities(96, 100)
        }
        _ => false,
    };
    payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && matches!(marker_native_code(payload, offset), Some(1 | 2))
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && compact_indexed_curve_endpoint_indices(payload, offset)
            .is_some_and(|endpoints| endpoints[0] != endpoints[1])
        && referenced_ending
}

pub(super) fn inferred_point_coordinates_by_index(
    lane: &FeatureInputLane,
    feature: &str,
) -> HashMap<u32, [f64; 2]> {
    const POINT_REFERENCE_TAG: u16 = 0x820f;

    let mut candidates = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter_map(|marker| marker.coordinates_m)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left[0]
            .total_cmp(&right[0])
            .then_with(|| left[1].total_cmp(&right[1]))
    });
    candidates.dedup_by(|left, right| {
        same_dimension_length(left[0], right[0]) && same_dimension_length(left[1], right[1])
    });

    let mut constraints = Vec::new();
    for scalar in lane.scalars.iter().filter(|scalar| {
        scalar.feature_ref.as_deref() == Some(feature)
            && scalar.role == FeatureInputScalarRole::Driving
            && scalar.value.is_finite()
            && scalar.value >= 0.0
            && scalar.operands.len() == 2
            && scalar
                .operands
                .iter()
                .all(|operand| operand.kind == FeatureInputOperandKind::Native(POINT_REFERENCE_TAG))
    }) {
        let [first, second] = scalar.operands.as_slice() else {
            unreachable!("scalar operand cardinality was filtered above");
        };
        let endpoints = [
            u32::from(first.entity_index),
            u32::from(second.entity_index),
        ];
        constraints.push((endpoints, scalar.value));
    }

    let mut indices = HashSet::new();
    for (endpoints, _) in &constraints {
        indices.extend(endpoints);
    }
    let mut domains = indices
        .into_iter()
        .map(|index| (index, candidates.clone()))
        .collect::<HashMap<_, _>>();
    loop {
        let previous = domains.clone();
        for (index, domain) in &mut domains {
            domain.retain(|candidate| {
                constraints.iter().all(|(endpoints, distance)| {
                    let other = match endpoints {
                        [left, other] if left == index => other,
                        [other, right] if right == index => other,
                        _ => return true,
                    };
                    if other == index {
                        same_dimension_length(*distance, 0.0)
                    } else {
                        previous.get(other).is_some_and(|other_domain| {
                            other_domain.iter().any(|point| {
                                same_dimension_length(
                                    (candidate[0] - point[0]).hypot(candidate[1] - point[1]),
                                    *distance,
                                )
                            })
                        })
                    }
                })
            });
        }
        if domains == previous {
            break;
        }
    }
    domains
        .iter()
        .filter_map(|(&index, domain)| {
            let [point] = domain.as_slice() else {
                return None;
            };
            point_distance_component_has_solution(index, &domains, &constraints)
                .then_some((index, *point))
        })
        .collect()
}

fn point_distance_component_has_solution(
    seed: u32,
    domains: &HashMap<u32, Vec<[f64; 2]>>,
    constraints: &[([u32; 2], f64)],
) -> bool {
    let mut component = HashSet::from([seed]);
    let mut pending = vec![seed];
    while let Some(index) = pending.pop() {
        for (endpoints, _) in constraints
            .iter()
            .filter(|(endpoints, _)| endpoints.contains(&index))
        {
            for endpoint in endpoints {
                if component.insert(*endpoint) {
                    pending.push(*endpoint);
                }
            }
        }
    }
    let mut unassigned = component.into_iter().collect::<Vec<_>>();
    unassigned.sort_unstable_by_key(|index| {
        std::cmp::Reverse(domains.get(index).map_or(usize::MAX, Vec::len))
    });

    point_distance_assignment_exists(&mut unassigned, &mut HashMap::new(), domains, constraints)
}

fn point_distance_assignment_exists(
    unassigned: &mut Vec<u32>,
    assigned: &mut HashMap<u32, [f64; 2]>,
    domains: &HashMap<u32, Vec<[f64; 2]>>,
    constraints: &[([u32; 2], f64)],
) -> bool {
    let Some(index) = unassigned.pop() else {
        return true;
    };
    let solved = domains.get(&index).is_some_and(|domain| {
        domain.iter().copied().any(|candidate| {
            let compatible = constraints.iter().all(|(endpoints, distance)| {
                let other = match endpoints {
                    [left, other] if *left == index => *other,
                    [other, right] if *right == index => *other,
                    _ => return true,
                };
                if other == index {
                    same_dimension_length(*distance, 0.0)
                } else {
                    assigned.get(&other).is_none_or(|point| {
                        same_dimension_length(
                            (candidate[0] - point[0]).hypot(candidate[1] - point[1]),
                            *distance,
                        )
                    })
                }
            });
            if !compatible {
                return false;
            }
            assigned.insert(index, candidate);
            let solved =
                point_distance_assignment_exists(unassigned, assigned, domains, constraints);
            assigned.remove(&index);
            solved
        })
    });
    unassigned.push(index);
    solved
}

pub(super) fn implicit_coordinate_roster_curve_endpoints(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
    inferred: &HashMap<u32, [f64; 2]>,
) -> Option<[[f64; 2]; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if !coordinate_roster_curve_layout(payload, offset) {
        return None;
    }
    let mut coordinates = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
        })
        .collect::<Vec<_>>();
    coordinates.sort_unstable_by_key(|marker| marker.offset);
    let endpoint_offset = coordinate_roster_endpoint_offset(payload, offset)?;
    let endpoint_index = |relative| {
        Some(u32::from(u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        )))
    };
    let indices = [
        endpoint_index(endpoint_offset)?,
        endpoint_index(endpoint_offset + 2)?,
    ];
    let mut used_inferred = false;
    let endpoints = indices.map(|index| {
        if let Some(point) = coordinates
            .get(usize::try_from(index).ok()?)
            .and_then(|marker| marker.coordinates_m)
        {
            Some(point)
        } else {
            used_inferred = true;
            inferred.get(&index).copied()
        }
    });
    used_inferred.then_some([endpoints[0]?, endpoints[1]?])
}

pub(super) fn implicit_profile_chain_closure_endpoints(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<[[f64; 2]; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    let indices = packed_compact_legacy_curve_endpoint_indices(payload, offset)?;
    if marker_profile_curve_role(payload, offset) != Some(1) {
        return None;
    }
    let coordinate_count = markers
        .iter()
        .filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
        })
        .count();
    if indices.iter().any(|index| {
        usize::try_from(*index)
            .ok()
            .is_some_and(|index| index < coordinate_count)
    }) {
        return None;
    }
    let unresolved = markers
        .iter()
        .copied()
        .filter(|candidate| {
            candidate.feature_ref == curve.feature_ref
                && usize::try_from(candidate.offset)
                    .ok()
                    .is_some_and(|offset| {
                        packed_compact_legacy_curve_endpoint_indices(payload, offset).is_some()
                            && marker_profile_curve_role(payload, offset) == Some(1)
                    })
        })
        .filter(|candidate| {
            coordinate_roster_curve_endpoint_markers(payload, candidate, markers).len() != 2
        })
        .collect::<Vec<_>>();
    if !matches!(unresolved.as_slice(), [candidate] if candidate.id == curve.id) {
        return None;
    }
    let markers_by_id = markers
        .iter()
        .copied()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let mut degrees = HashMap::<&str, (usize, [f64; 2], u64)>::new();
    let mut edge_count = 0usize;
    for sibling in markers.iter().copied().filter(|sibling| {
        sibling.feature_ref == curve.feature_ref
            && sibling.id != curve.id
            && matches!(
                sibling.kind,
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
            && usize::try_from(sibling.offset)
                .ok()
                .is_some_and(|offset| marker_profile_curve_role(payload, offset) == Some(1))
    }) {
        let endpoints = marker_curve_endpoint_markers(payload, sibling, &markers_by_id, markers);
        let [first, second] = endpoints.as_slice() else {
            continue;
        };
        let [Some(first_coordinates), Some(second_coordinates)] =
            [first.coordinates_m, second.coordinates_m]
        else {
            continue;
        };
        let coordinates = [first_coordinates, second_coordinates];
        if first.id == second.id || coordinates[0] == coordinates[1] {
            continue;
        }
        for (endpoint, coordinates) in [(*first, coordinates[0]), (*second, coordinates[1])] {
            let entry =
                degrees
                    .entry(endpoint.id.as_str())
                    .or_insert((0, coordinates, endpoint.offset));
            if entry.1 != coordinates {
                return None;
            }
            entry.0 += 1;
        }
        edge_count += 1;
    }
    if edge_count < 2
        || edge_count.checked_add(1) != Some(degrees.len())
        || degrees
            .values()
            .any(|(degree, _, _)| !matches!(degree, 1 | 2))
    {
        return None;
    }
    let mut endpoints = degrees
        .values()
        .filter_map(|(degree, coordinates, offset)| {
            (*degree == 1).then_some((*offset, *coordinates))
        })
        .collect::<Vec<_>>();
    endpoints.sort_unstable_by_key(|(offset, _)| *offset);
    let [(_, first), (_, second)] = endpoints.as_slice() else {
        return None;
    };
    (first != second).then_some([*first, *second])
}

pub(super) fn extended_declared_inline_line_endpoints(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<[[f64; 2]; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    let declaration = payload.get(offset + 96..offset + 106)?;
    let declaration_id = u16::from_le_bytes(declaration[..2].try_into().ok()?);
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || payload.get(offset + 74..offset + 78) != Some(&[0x00, 0x00, 0x02, 0x00])
        || payload.get(offset + 78..offset + 84) != Some(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00])
        || payload.get(offset + 84..offset + 96) != Some(b"sgLineHandle")
        || matches!(declaration_id, 0 | u16::MAX)
        || declaration[2..6] != [0xff; 4]
        || declaration[6..10] != [0; 4]
        || payload.get(offset + 110..offset + 114) != Some(&[0xff; 4])
        || payload.get(offset + 114..offset + 118) != Some(&[0; 4])
        || payload.get(offset + 118..offset + 124) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 124..offset + 166) != Some(&[0; 42])
        || payload
            .get(offset + 166..offset + 170)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(170)?)
    {
        return None;
    }
    let cell = payload.get(offset + 106..offset + 114)?;
    let kind = operand_kind(cell[..2].try_into().ok()?)?;
    if !operand_accepts_marker(kind, SketchInputKind::Point) {
        return None;
    }
    let index = u32::from(u16::from_le_bytes(cell[2..4].try_into().ok()?));
    let mut candidates = markers.iter().copied().filter(|marker| {
        marker.feature_ref == curve.feature_ref
            && marker.object_index == Some(index)
            && marker.coordinates_m.is_some()
            && matches!(
                marker.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
    });
    let external = match (candidates.next(), candidates.next()) {
        (Some(external), None) => external.coordinates_m?,
        _ => return None,
    };
    Some([external, finite_coordinate_pair(payload, offset + 58)?])
}

pub(super) fn extended_linked_inline_line_endpoints(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<[[f64; 2]; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
    {
        return None;
    }
    let record = linked_profile_curve_record(payload, offset)?;
    if !matches!(record.state, 0 | 1)
        || !matches!(record.reference_count, 2 | 3)
        || !matches!(record.tail_flag, 0 | 1)
    {
        return None;
    }
    let curve_index = curve.object_index?;
    let zero_based = record
        .references
        .iter()
        .position(|reference| reference.checked_add(1) == Some(curve_index));
    let exact = record
        .references
        .iter()
        .position(|reference| *reference == curve_index);
    let external_index = match (zero_based, exact) {
        (Some(position), None) => record.references[1 - position].checked_add(1)?,
        (None, Some(position)) => record.references[1 - position],
        _ => return None,
    };
    let mut candidates = markers.iter().copied().filter(|marker| {
        marker.feature_ref == curve.feature_ref
            && marker.object_index == Some(external_index)
            && marker.coordinates_m.is_some()
            && matches!(
                marker.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
    });
    let external = match (candidates.next(), candidates.next()) {
        (Some(external), None) => external.coordinates_m?,
        _ => return None,
    };
    Some([external, record.inline])
}

pub(super) fn extended_identity_inline_line_endpoints(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<[[f64; 2]; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if !extended_identity_inline_line_record(payload, offset) {
        return None;
    }
    let identity = u32::from_le_bytes(payload.get(offset + 130..offset + 134)?.try_into().ok()?);
    let mut candidates = markers.iter().copied().filter(|marker| {
        marker.feature_ref == curve.feature_ref
            && marker.id != curve.id
            && marker.object_index == Some(identity)
            && marker.coordinates_m.is_some()
            && matches!(
                marker.kind,
                SketchInputKind::Point
                    | SketchInputKind::ConstrainedPoint
                    | SketchInputKind::LineOrCircle
                    | SketchInputKind::Arc
            )
    });
    let endpoint = candidates.next()?;
    candidates.next().is_none().then_some([
        finite_coordinate_pair(payload, offset + 58)?,
        endpoint.coordinates_m?,
    ])
}

pub(super) fn extended_identity_inline_line_record(payload: &[u8], offset: usize) -> bool {
    let counted_state = payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 74..offset + 84)
            == Some(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00])
        && payload.get(offset + 88..offset + 130) == Some(&[0; 42]);
    let direct_state = payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 74..offset + 84)
            == Some(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
        && payload.get(offset + 88..offset + 126) == Some(&[0; 38])
        && payload
            .get(offset + 126..offset + 130)
            .and_then(|identity| identity.try_into().ok())
            .map(u32::from_le_bytes)
            .is_some_and(|identity| !matches!(identity, 0 | u32::MAX));
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || !matches!(marker_native_code(payload, offset), Some(1 | 2))
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || !(counted_state || direct_state)
        || payload.get(offset + 84..offset + 88) != Some(&(-2i32).to_le_bytes())
        || !offset
            .checked_add(134)
            .is_some_and(|next| sketch_marker_prefix_at(payload, next))
    {
        return false;
    }
    payload
        .get(offset + 130..offset + 134)
        .and_then(|identity| identity.try_into().ok())
        .map(u32::from_le_bytes)
        .is_some_and(|identity| !matches!(identity, 0 | u32::MAX))
}

pub(super) fn coordinate_roster_arc_center(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
    resolved_endpoints: [&SketchInputEntity; 2],
) -> Option<[f64; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    let current_wide = payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && wide_indexed_curve_endpoint_indices(payload, offset).is_some()
        && sketch_marker_prefix_at(payload, offset.saturating_add(92));
    let extended_wide = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && wide_indexed_curve_endpoint_indices(payload, offset).is_some()
        && sketch_marker_prefix_at(payload, offset.saturating_add(92));
    let extended_compact_104 = extended_compact_104_indexed_arc(payload, offset);
    if !current_wide
        && !extended_wide
        && !extended_compact_104
        && legacy_referenced_wide_arc_endpoint_indices(payload, offset).is_none()
    {
        return None;
    }
    if let Some((endpoints, center)) = current_wide_arc_direct_markers(payload, curve, markers) {
        let matches = (resolved_endpoints[0].id == endpoints[0].id
            && resolved_endpoints[1].id == endpoints[1].id)
            || (resolved_endpoints[0].id == endpoints[1].id
                && resolved_endpoints[1].id == endpoints[0].id);
        if matches {
            return Some(center);
        }
    }
    let endpoint_offset = if extended_compact_104 { 56 } else { 64 };
    let first_index = usize::from(u16::from_le_bytes(
        payload
            .get(offset + endpoint_offset..offset + endpoint_offset + 2)?
            .try_into()
            .ok()?,
    ));
    let second_index = usize::from(u16::from_le_bytes(
        payload
            .get(offset + endpoint_offset + 2..offset + endpoint_offset + 4)?
            .try_into()
            .ok()?,
    ));
    let center_index = first_index.min(second_index).checked_sub(1)?;
    let first = resolved_endpoints[0].coordinates_m?;
    let second = resolved_endpoints[1].coordinates_m?;
    let roster = |include_relations: bool| {
        let mut coordinates = markers
            .iter()
            .copied()
            .filter(|marker| {
                marker.feature_ref == curve.feature_ref
                    && marker.coordinates_m.is_some()
                    && (include_relations
                        || matches!(
                            marker.kind,
                            SketchInputKind::Point
                                | SketchInputKind::ConstrainedPoint
                                | SketchInputKind::LineOrCircle
                                | SketchInputKind::Arc
                        ))
            })
            .collect::<Vec<_>>();
        coordinates.sort_unstable_by_key(|marker| marker.offset);
        Some([
            *coordinates.get(first_index)?,
            *coordinates.get(second_index)?,
        ])
    };
    let endpoint_pair_matches = |roster_endpoints: [&SketchInputEntity; 2]| {
        (resolved_endpoints[0].id == roster_endpoints[0].id
            && resolved_endpoints[1].id == roster_endpoints[1].id)
            || (resolved_endpoints[0].id == roster_endpoints[1].id
                && resolved_endpoints[1].id == roster_endpoints[0].id)
    };
    if ![roster(false), roster(true)]
        .into_iter()
        .flatten()
        .any(endpoint_pair_matches)
    {
        return None;
    }
    let mut complete_roster = markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref == curve.feature_ref && marker.coordinates_m.is_some())
        .collect::<Vec<_>>();
    complete_roster.sort_unstable_by_key(|marker| marker.offset);
    let equidistant = |center: [f64; 2]| {
        let first_radius = (first[0] - center[0]).hypot(first[1] - center[1]);
        let second_radius = (second[0] - center[0]).hypot(second[1] - center[1]);
        first_radius > 0.0 && same_dimension_length(first_radius, second_radius)
    };
    if let Some(center) = complete_roster
        .get(center_index)
        .and_then(|marker| marker.coordinates_m)
        .filter(|center| equidistant(*center))
    {
        return Some(center);
    }
    let mut centers = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.object_index == u32::try_from(center_index).ok()
        })
        .filter_map(|marker| marker.coordinates_m)
        .filter(|center| equidistant(*center))
        .collect::<Vec<_>>();
    centers.sort_by(|left, right| {
        left[0]
            .total_cmp(&right[0])
            .then_with(|| left[1].total_cmp(&right[1]))
    });
    centers.dedup();
    let [center] = centers.as_slice() else {
        return None;
    };
    Some(*center)
}

pub(super) fn legacy_marker104_arc_center(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
    endpoints: [&SketchInputEntity; 2],
) -> Option<[f64; 2]> {
    legacy_marker104_arc_endpoints(payload, curve, markers)?;
    let [first_u, first_v] = endpoints[0].coordinates_m?;
    let [second_u, second_v] = endpoints[1].coordinates_m?;
    let mut centers = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.id != endpoints[0].id
                && marker.id != endpoints[1].id
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .filter_map(|marker| marker.coordinates_m)
        .filter(|center| {
            let first_radius = (first_u - center[0]).hypot(first_v - center[1]);
            let second_radius = (second_u - center[0]).hypot(second_v - center[1]);
            first_radius > 0.0 && same_dimension_length(first_radius, second_radius)
        })
        .collect::<Vec<_>>();
    centers.sort_by(|left, right| {
        left[0]
            .total_cmp(&right[0])
            .then_with(|| left[1].total_cmp(&right[1]))
    });
    centers.dedup_by(|left, right| {
        same_dimension_length(left[0], right[0]) && same_dimension_length(left[1], right[1])
    });
    let [center] = centers.as_slice() else {
        return None;
    };
    Some(*center)
}

pub(super) fn legacy_compact_diameter_arc_center(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
    endpoints: [&SketchInputEntity; 2],
) -> Option<[f64; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x05, 0x00, 0x01, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || compact_indexed_curve_endpoint_indices(payload, offset).is_none()
    {
        return None;
    }
    let [first_u, first_v] = endpoints[0].coordinates_m?;
    let [second_u, second_v] = endpoints[1].coordinates_m?;
    if first_u == second_u && first_v == second_v {
        return None;
    }
    let midpoint = [(first_u + second_u) * 0.5, (first_v + second_v) * 0.5];
    let mut centers = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.id != endpoints[0].id
                && marker.id != endpoints[1].id
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .filter_map(|marker| marker.coordinates_m)
        .filter(|center| {
            same_dimension_length(center[0], midpoint[0])
                && same_dimension_length(center[1], midpoint[1])
        })
        .collect::<Vec<_>>();
    centers.sort_by(|left, right| {
        left[0]
            .total_cmp(&right[0])
            .then_with(|| left[1].total_cmp(&right[1]))
    });
    centers.dedup();
    let [center] = centers.as_slice() else {
        return None;
    };
    Some(*center)
}

pub(super) fn coordinate_circle_radius(
    payload: &[u8],
    circle: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<f64> {
    let offset = usize::try_from(circle.offset).ok()?;
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&0u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 64..offset + 66) != Some(&[0x1e, 0x00])
        || payload.get(offset + 82..offset + 86) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 86..offset + 92) != Some(&[0; 6])
        || payload.get(offset + 92..offset + 96) != Some(&(-2i32).to_le_bytes())
        || payload.get(offset + 96..offset + 138) != Some(&[0; 42])
        || !sketch_marker_prefix_at(payload, offset.checked_add(142)?)
    {
        return None;
    }
    let [center_u, center_v] = circle.coordinates_m?;
    let mut coordinates = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == circle.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    coordinates.sort_unstable_by_key(|marker| marker.offset);
    let insertion = coordinates.partition_point(|marker| marker.offset < circle.offset);
    let grids = [
        insertion
            .checked_sub(6)
            .and_then(|start| coordinates.get(start..insertion)),
        coordinates.get(insertion..insertion.checked_add(6)?),
    ];
    let mut radii = grids
        .into_iter()
        .flatten()
        .filter_map(|grid| {
            let mut u = grid
                .iter()
                .filter_map(|marker| marker.coordinates_m.map(|point| point[0]))
                .collect::<Vec<_>>();
            let mut v = grid
                .iter()
                .filter_map(|marker| marker.coordinates_m.map(|point| point[1]))
                .collect::<Vec<_>>();
            u.sort_by(f64::total_cmp);
            u.dedup();
            v.sort_by(f64::total_cmp);
            v.dedup();
            let (u_min, u_max, v_min, v_max) = (*u.first()?, *u.last()?, *v.first()?, *v.last()?);
            let mut points = grid
                .iter()
                .filter_map(|marker| marker.coordinates_m)
                .collect::<Vec<_>>();
            points.sort_by(|left, right| {
                left[0]
                    .total_cmp(&right[0])
                    .then_with(|| left[1].total_cmp(&right[1]))
            });
            points.dedup();
            let complete_grid = points.len() == 6 && u.len() * v.len() == 6;
            let centered = same_dimension_length(center_u - u_min, u_max - center_u)
                && same_dimension_length(center_v - v_min, v_max - center_v);
            let square = same_dimension_length(u_max - u_min, v_max - v_min);
            (complete_grid && centered && square && matches!((u.len(), v.len()), (3, 2) | (2, 3)))
                .then_some((u_max - u_min) * 0.5)
        })
        .filter(|radius| radius.is_finite() && *radius > 0.0)
        .collect::<Vec<_>>();
    radii.sort_by(f64::total_cmp);
    radii.dedup_by(|left, right| same_dimension_length(*left, *right));
    let [radius] = radii.as_slice() else {
        return None;
    };
    Some(*radius)
}

pub(super) fn legacy_coordinate_circle_radius(
    payload: &[u8],
    circle: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<f64> {
    let offset = usize::try_from(circle.offset).ok()?;
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x05, 0x00, 0x01, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&0u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 64..offset + 66) != Some(&[0x1e, 0x00])
        || payload.get(offset + 82..offset + 84) != Some(&[0; 2])
        || payload.get(offset + 84..offset + 86) != Some(&2u16.to_le_bytes())
        || payload.get(offset + 90..offset + 94) != Some(&[0xff; 4])
        || payload.get(offset + 94..offset + 98) != Some(&[0; 4])
        || payload.get(offset + 102..offset + 106) != Some(&[0xff; 4])
        || payload.get(offset + 106..offset + 110) != Some(&[0; 4])
        || payload.get(offset + 110..offset + 116) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 116..offset + 158) != Some(&[0; 42])
        || !sketch_marker_prefix_at(payload, offset.checked_add(162)?)
    {
        return None;
    }
    let first_selector =
        u16::from_le_bytes(payload.get(offset + 86..offset + 88)?.try_into().ok()?);
    let second_selector =
        u16::from_le_bytes(payload.get(offset + 98..offset + 100)?.try_into().ok()?);
    if first_selector == 0
        || first_selector != second_selector
        || payload.get(offset + 88..offset + 90) == Some(&[0; 2])
        || payload.get(offset + 100..offset + 102) == Some(&[0; 2])
    {
        return None;
    }
    let radial_index =
        u32::from_le_bytes(payload.get(offset + 158..offset + 162)?.try_into().ok()?);
    let mut radial_points = markers.iter().copied().filter(|marker| {
        marker.feature_ref == circle.feature_ref
            && marker.offset == circle.offset + 162
            && marker.object_index == Some(radial_index)
            && marker.kind == SketchInputKind::Point
            && marker.coordinates_m.is_some()
    });
    let radial = radial_points.next()?;
    if radial_points.next().is_some() {
        return None;
    }
    let center = circle.coordinates_m?;
    let radial = radial.coordinates_m?;
    let radius = (radial[0] - center[0]).hypot(radial[1] - center[1]);
    (radius.is_finite() && radius > 0.0).then_some(radius)
}

pub(super) fn coordinate_roster_full_circle(
    payload: &[u8],
    circle: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<([f64; 2], f64)> {
    let offset = usize::try_from(circle.offset).ok()?;
    let radial_index = if let Some(index) = current_long_full_circle_radial_index(payload, offset) {
        index
    } else {
        if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
            != Some(LEGACY_EXTENDED_SKETCH_MARKER)
            || marker_native_code(payload, offset) != Some(0)
            || payload.get(offset + 23..offset + 27) != Some(&[0x05, 0x00, 0x01, 0x00])
            || marker_profile_curve_role(payload, offset) != Some(1)
            || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
            || payload.get(offset + 31..offset + 39)
                != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
            || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
            || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
            || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
            || payload.get(offset + 72..offset + 76) != Some(&1i32.to_le_bytes())
            || payload.get(offset + 78..offset + 94)
                != Some(&[
                    0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe,
                    0xff, 0xff, 0xff,
                ])
            || payload.get(offset + 94..offset + 96) != Some(&[0; 2])
            || !matches!(
                compact_indexed_curve_record_end(payload, offset),
                Some(
                    CompactIndexedCurveRecordEnd::Marker104
                        | CompactIndexedCurveRecordEnd::Terminal102
                )
            )
        {
            return None;
        }
        let index = usize::from(u16::from_le_bytes(
            payload.get(offset + 56..offset + 58)?.try_into().ok()?,
        ));
        if index == 0
            || payload.get(offset + 58..offset + 60)
                != Some(&u16::try_from(index).ok()?.to_le_bytes())
        {
            return None;
        }
        index
    };
    let mut points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == circle.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|marker| marker.offset);
    let center = points.first()?.coordinates_m?;
    let radial = points.get(radial_index)?.coordinates_m?;
    let radius = (radial[0] - center[0]).hypot(radial[1] - center[1]);
    (radius.is_finite() && radius > 0.0).then_some((center, radius))
}

pub(super) fn current_long_full_circle_radial_index(
    payload: &[u8],
    offset: usize,
) -> Option<usize> {
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 84) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 84..offset + 86) != Some(&[0; 2])
        || payload.get(offset + 86..offset + 102)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 102..offset + 134) != Some(&[0; 32])
        || payload.get(offset + 134..offset + 136) != Some(&4u16.to_le_bytes())
        || payload.get(offset + 136..offset + 154)
            != Some(&[
                0xf1, 0x80, 0x00, 0x00, 0x00, 0x00, 0xf3, 0x80, 0x04, 0x80, 0xff, 0xfe, 0xff, 0x02,
                0x44, 0x00, 0x31, 0x00,
            ])
    {
        return None;
    }
    let radial_index = usize::from(u16::from_le_bytes(
        payload.get(offset + 64..offset + 66)?.try_into().ok()?,
    ));
    (radial_index != 0
        && payload.get(offset + 66..offset + 68)
            == Some(&u16::try_from(radial_index).ok()?.to_le_bytes()))
    .then_some(radial_index)
}

pub(super) fn equal_index_coordinate_roster_full_circle(
    payload: &[u8],
    circle: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<([f64; 2], f64)> {
    let offset = usize::try_from(circle.offset).ok()?;
    let prefix = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())?;
    let extended_layout = prefix == LEGACY_EXTENDED_SKETCH_MARKER
        && marker_native_code(payload, offset) == Some(0)
        && marker_is_geometry_locus(payload, offset)
        && matches!(
            payload.get(offset + 31..offset + 39),
            Some([0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x01 | 0x04, 0x00])
        );
    let legacy_layout = prefix == LEGACY_SKETCH_MARKER
        && marker_native_code(payload, offset) == Some(2)
        && marker_is_geometry_locus(payload, offset)
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    let current_profile_layout = prefix == SKETCH_MARKER
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    if circle.kind != SketchInputKind::Arc
        || !(extended_layout || legacy_layout || current_profile_layout)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || !matches!(
            payload
                .get(offset + 72..offset + 76)
                .and_then(|bytes| bytes.try_into().ok())
                .map(i32::from_le_bytes),
            Some(-1 | 1)
        )
        || payload.get(offset + 78..offset + 94)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 94..offset + 96) != Some(&[0; 2])
        || !matches!(
            compact_indexed_curve_record_end(payload, offset),
            Some(
                CompactIndexedCurveRecordEnd::Marker104 | CompactIndexedCurveRecordEnd::Terminal102
            )
        )
    {
        return None;
    }
    let center_index = u16::from_le_bytes(payload.get(offset + 56..offset + 58)?.try_into().ok()?);
    if center_index == 0
        || payload.get(offset + 58..offset + 60) != Some(&center_index.to_le_bytes())
    {
        return None;
    }
    let mut points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == circle.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|marker| marker.offset);
    let center_index = usize::from(center_index.checked_sub(1)?);
    let center = points.get(center_index)?.coordinates_m?;
    let radial = points.get(center_index.checked_add(1)?)?.coordinates_m?;
    let radius = (radial[0] - center[0]).hypot(radial[1] - center[1]);
    (radius.is_finite() && radius > 0.0).then_some((center, radius))
}

pub(super) fn compact_profile_full_circle(
    payload: &[u8],
    circle: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<([f64; 2], f64)> {
    let offset = usize::try_from(circle.offset).ok()?;
    let prefix = payload.get(offset..offset + SKETCH_MARKER.len())?;
    let kind = marker_native_code(payload, offset)?;
    let supported_kind = prefix == LEGACY_EXTENDED_SKETCH_MARKER
        && kind == 1
        && circle.kind == SketchInputKind::LineOrCircle
        || prefix == SKETCH_MARKER && kind == 2 && circle.kind == SketchInputKind::Arc;
    if !supported_kind
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 76) != Some(&1i32.to_le_bytes())
        || payload.get(offset + 78..offset + 94)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 94..offset + 96) != Some(&[0; 2])
        || !matches!(
            compact_indexed_curve_record_end(payload, offset),
            Some(
                CompactIndexedCurveRecordEnd::Marker104 | CompactIndexedCurveRecordEnd::Terminal102
            )
        )
    {
        return None;
    }
    let radial_index = u16::from_le_bytes(payload.get(offset + 56..offset + 58)?.try_into().ok()?);
    if radial_index == 0
        || payload.get(offset + 58..offset + 60) != Some(&radial_index.to_le_bytes())
    {
        return None;
    }
    let mut points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == circle.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|marker| marker.offset);
    let center = points.first()?.coordinates_m?;
    let mut radials = points
        .iter()
        .copied()
        .filter(|marker| marker.object_index == Some(u32::from(radial_index)))
        .chain(
            usize::from(radial_index)
                .checked_sub(1)
                .and_then(|index| points.get(index))
                .copied(),
        )
        .filter_map(|marker| marker.coordinates_m)
        .filter(|radial| {
            let radius = (radial[0] - center[0]).hypot(radial[1] - center[1]);
            radius.is_finite() && radius > 0.0
        })
        .collect::<Vec<_>>();
    radials.sort_by(|left, right| {
        left[0]
            .total_cmp(&right[0])
            .then_with(|| left[1].total_cmp(&right[1]))
    });
    radials.dedup_by(|left, right| {
        same_dimension_length(left[0], right[0]) && same_dimension_length(left[1], right[1])
    });
    let [radial] = radials.as_slice() else {
        return None;
    };
    let radius = (radial[0] - center[0]).hypot(radial[1] - center[1]);
    Some((center, radius))
}

pub(super) fn compact_legacy_profile_full_circle(
    payload: &[u8],
    circle: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<([f64; 2], f64)> {
    let offset = usize::try_from(circle.offset).ok()?;
    if circle.kind != SketchInputKind::LineOrCircle
        || !compact_legacy_marker_body(payload, offset)
        || marker_native_code(payload, offset) != Some(1)
        || payload.get(offset + 19..offset + 23) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 25..offset + 27) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 42) != Some(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || payload.get(offset + 46..offset + 50) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 50..offset + 58) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 58..offset + 62) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 62..offset + 64) != Some(&[0; 2])
        || payload.get(offset + 64..offset + 80).is_none_or(|cells| {
            !cells
                .chunks_exact(4)
                .all(|cell| cell == (-2i32).to_le_bytes())
        })
        || payload.get(offset + 80..offset + 82) != Some(&[0; 2])
    {
        return None;
    }
    let continued = payload
        .get(offset + 82..offset + 86)
        .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && payload
            .get(offset + 86..offset + 90)
            .is_some_and(|object| object != [0; 4] && object != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.checked_add(90)?);
    let terminal = payload.get(offset + 82..offset + 112) == Some(&[0; 30])
        && payload.get(offset + 112..offset + 114) == Some(&4u16.to_le_bytes())
        && payload.get(offset + 114..offset + 118) == Some(CLASS_MARKER)
        && payload.get(offset + 118..offset + 120) == Some(&11u16.to_le_bytes())
        && payload.get(offset + 120..offset + 131) == Some(b"sgCircleDim");
    if !continued && !terminal {
        return None;
    }
    let radial_index = usize::from(u16::from_le_bytes(
        payload.get(offset + 42..offset + 44)?.try_into().ok()?,
    ));
    if payload.get(offset + 44..offset + 46)
        != Some(&u16::try_from(radial_index).ok()?.to_le_bytes())
    {
        return None;
    }
    let mut points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == circle.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|marker| marker.offset);
    let center = points.first()?.coordinates_m?;
    let feature_start = usize::try_from(points.first()?.offset).ok()?;
    let radial_offset = (feature_start..=offset)
        .filter(|candidate| sketch_marker_prefix_at(payload, *candidate))
        .nth(radial_index)?;
    let mut radial_candidates = points
        .iter()
        .filter(|marker| usize::try_from(marker.offset).ok() == Some(radial_offset));
    let radial = radial_candidates.next()?.coordinates_m?;
    if radial_candidates.next().is_some() {
        return None;
    }
    let radius = (radial[0] - center[0]).hypot(radial[1] - center[1]);
    (radius.is_finite() && radius > 0.0).then_some((center, radius))
}

pub(super) fn legacy_profile_radial_circle(
    payload: &[u8],
    circle: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<([f64; 2], f64)> {
    let offset = usize::try_from(circle.offset).ok()?;
    let identity_end = payload
        .get(offset + 104..offset + 108)
        .zip(payload.get(offset + 108..offset + 112))
        .is_some_and(|(first, second)| {
            first == second && first != [0; 4] && first != u32::MAX.to_le_bytes()
        })
        && sketch_marker_prefix_at(payload, offset.checked_add(112)?);
    let terminal_end = payload.get(offset + 104..offset + 128) == Some(&[0; 24])
        && !sketch_marker_prefix_at(payload, offset.saturating_add(112));
    if circle.kind != SketchInputKind::LineOrCircle
        || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(1)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 84) != Some(&1i32.to_le_bytes())
        || payload.get(offset + 84..offset + 86) != Some(&[0; 2])
        || payload.get(offset + 86..offset + 102)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 102..offset + 104) != Some(&[0; 2])
        || !(identity_end || terminal_end)
    {
        return None;
    }
    let radial_index = u16::from_le_bytes(payload.get(offset + 64..offset + 66)?.try_into().ok()?);
    if radial_index == 0
        || payload.get(offset + 66..offset + 68) != Some(&radial_index.to_le_bytes())
    {
        return None;
    }
    let mut coordinates = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == circle.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
        })
        .collect::<Vec<_>>();
    coordinates.sort_unstable_by_key(|marker| marker.offset);
    let center = coordinates.first()?.coordinates_m?;
    let mut radials = [
        coordinates.get(usize::from(radial_index)).copied(),
        usize::from(radial_index)
            .checked_sub(1)
            .and_then(|index| coordinates.get(index))
            .copied(),
    ]
    .into_iter()
    .flatten()
    .filter_map(|marker| marker.coordinates_m)
    .filter(|radial| {
        let radius = (radial[0] - center[0]).hypot(radial[1] - center[1]);
        radius.is_finite() && radius > 0.0
    })
    .collect::<Vec<_>>();
    radials.sort_by(|left, right| {
        left[0]
            .total_cmp(&right[0])
            .then_with(|| left[1].total_cmp(&right[1]))
    });
    radials.dedup_by(|left, right| {
        same_dimension_length(left[0], right[0]) && same_dimension_length(left[1], right[1])
    });
    let [radial] = radials.as_slice() else {
        return None;
    };
    Some((center, (radial[0] - center[0]).hypot(radial[1] - center[1])))
}

pub(super) fn wide_coordinate_roster_full_circle(
    payload: &[u8],
    circle: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<([f64; 2], f64)> {
    let offset = usize::try_from(circle.offset).ok()?;
    let prefix = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())?;
    let supported_kind = prefix == LEGACY_SKETCH_MARKER
        && marker_native_code(payload, offset) == Some(1)
        && circle.kind == SketchInputKind::LineOrCircle
        || prefix == LEGACY_EXTENDED_SKETCH_MARKER
            && marker_native_code(payload, offset) == Some(2)
            && circle.kind == SketchInputKind::LineOrCircle;
    if !supported_kind
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || !(prefix == LEGACY_SKETCH_MARKER
            && wide_indexed_curve_record_ends_at(payload, offset, prefix)
            || prefix == LEGACY_EXTENDED_SKETCH_MARKER
                && extended_wide_repeated_circle_record(payload, offset))
    {
        return None;
    }
    let endpoints = if prefix == LEGACY_EXTENDED_SKETCH_MARKER {
        one_based_u16_endpoint_pair(payload, offset, 64)?
    } else {
        wide_indexed_curve_endpoint_indices(payload, offset)?
    };
    let [first, second] = endpoints;
    if first != second || first <= 1 {
        return None;
    }
    let radial_index = usize::try_from(first.checked_sub(1)?).ok()?;
    let terminal = prefix == LEGACY_EXTENDED_SKETCH_MARKER
        && extended_terminal_wide_repeated_circle_record(payload, offset);
    let radial_index = if terminal {
        radial_index.checked_sub(1)?
    } else {
        radial_index
    };
    let center_index = radial_index.checked_sub(1)?;
    let mut coordinates = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == circle.feature_ref
                && marker.coordinates_m.is_some()
                && (terminal
                    || matches!(
                        marker.kind,
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                    ))
        })
        .collect::<Vec<_>>();
    coordinates.sort_unstable_by_key(|marker| marker.offset);
    let center = coordinates.get(center_index)?.coordinates_m?;
    let radial = coordinates.get(radial_index)?.coordinates_m?;
    let radius = (radial[0] - center[0]).hypot(radial[1] - center[1]);
    (radius.is_finite() && radius > 0.0).then_some((center, radius))
}

fn extended_wide_repeated_circle_record(payload: &[u8], offset: usize) -> bool {
    let endpoint = |relative| {
        payload
            .get(offset + relative..offset + relative + 2)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
    };
    let endpoints = [endpoint(64), endpoint(66)];
    let identities = [104usize, 108].map(|relative| {
        payload
            .get(offset + relative..offset + relative + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
    });
    let common = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 56..offset + 64) == Some(&[0; 8])
        && matches!(endpoints, [Some(first), Some(second)] if first != 0 && first == second)
        && payload.get(offset + 68..offset + 72) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 72..offset + 80) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 80..offset + 84) == Some(&1i32.to_le_bytes())
        && payload
            .get(offset + 84..offset + 86)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
            .is_some_and(|state| state != 0)
        && payload.get(offset + 86..offset + 102)
            == Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ]);
    // The trailer identities retain the circle carrier, not two curve endpoints,
    // so a repeated identity is valid for the already-equal endpoint ordinal.
    let referenced = payload.get(offset + 102..offset + 104) == Some(&[0; 2])
        && matches!(identities, [Some(first), Some(second)] if first != 0 && first != u32::MAX && second != 0 && second != u32::MAX)
        && sketch_marker_prefix_at(payload, offset.saturating_add(112));
    let terminal = extended_terminal_wide_repeated_circle_record(payload, offset);
    common && (referenced || terminal)
}

fn extended_terminal_wide_repeated_circle_record(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 102..offset + 134) == Some(&[0; 32])
        && payload.get(offset + 134..offset + 136) == Some(&[0x04, 0x00])
        && class_declaration_at(payload, offset.saturating_add(136))
}

pub(super) fn coordinate_ellipse_axes(
    payload: &[u8],
    ellipse: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<([f64; 2], f64, f64)> {
    let offset = usize::try_from(ellipse.offset).ok()?;
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || !sketch_marker_prefix_at(payload, offset.checked_add(134)?)
    {
        return None;
    }
    let [center_u, center_v] = ellipse.coordinates_m?;
    let mut following = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == ellipse.feature_ref
                && marker.offset > ellipse.offset
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    following.sort_unstable_by_key(|marker| marker.offset);
    let corners = following.get(..4)?;
    if corners[0].offset != ellipse.offset.checked_add(134)? {
        return None;
    }
    let mut u = corners
        .iter()
        .filter_map(|marker| marker.coordinates_m.map(|point| point[0]))
        .collect::<Vec<_>>();
    let mut v = corners
        .iter()
        .filter_map(|marker| marker.coordinates_m.map(|point| point[1]))
        .collect::<Vec<_>>();
    u.sort_by(f64::total_cmp);
    u.dedup_by(|left, right| same_dimension_length(*left, *right));
    v.sort_by(f64::total_cmp);
    v.dedup_by(|left, right| same_dimension_length(*left, *right));
    let ([u_min, u_max], [v_min, v_max]) = (u.as_slice(), v.as_slice()) else {
        return None;
    };
    let products = [
        [*u_min, *v_min],
        [*u_min, *v_max],
        [*u_max, *v_min],
        [*u_max, *v_max],
    ];
    if !products.iter().all(|product| {
        corners.iter().any(|corner| {
            corner.coordinates_m.is_some_and(|point| {
                same_dimension_length(point[0], product[0])
                    && same_dimension_length(point[1], product[1])
            })
        })
    }) {
        return None;
    }
    if !same_dimension_length((*u_min + *u_max) * 0.5, center_u)
        || !same_dimension_length((*v_min + *v_max) * 0.5, center_v)
    {
        return None;
    }
    let u_radius = (*u_max - *u_min) * 0.5;
    let v_radius = (*v_max - *v_min) * 0.5;
    if u_radius <= 0.0 || v_radius <= 0.0 || same_dimension_length(u_radius, v_radius) {
        return None;
    }
    if u_radius > v_radius {
        Some(([1.0, 0.0], u_radius, v_radius))
    } else {
        Some(([0.0, 1.0], v_radius, u_radius))
    }
}

fn coordinate_roster_curve_layout(payload: &[u8], offset: usize) -> bool {
    coordinate_roster_endpoint_offset(payload, offset).is_some()
}

fn extended_marker104_arc_uses_point_roster(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && indexed_arc_uses_coordinate_center(payload, offset)
        && sketch_marker_prefix_at(payload, offset.saturating_add(104))
}

pub(super) fn extended_marker84_line_uses_point_roster(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && matches!(marker_profile_curve_role(payload, offset), Some(1 | 2))
        && matches!(
            (
                marker_profile_curve_role(payload, offset),
                payload.get(offset + 29..offset + 31)
            ),
            (Some(1), Some([0 | 1, 0])) | (Some(2), Some([0, 0]))
        )
        && payload.get(offset + 31..offset + 35) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && matches!(
            payload.get(offset + 35..offset + 39),
            Some([0, 0, 4 | 12, 0])
        )
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload
            .get(offset + 56..offset + 58)
            .is_some_and(|first| first != [0xff; 2])
        && payload
            .get(offset + 58..offset + 60)
            .is_some_and(|second| second != [0xff; 2])
        && payload.get(offset + 56..offset + 58) != payload.get(offset + 58..offset + 60)
        && matches!(
            payload.get(offset + 60..offset + 64),
            Some([0 | 1, 0, 0, 0])
        )
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && matches!(
            payload.get(offset + 72..offset + 76),
            Some([0, 0, 0..=2, 0])
        )
        && payload
            .get(offset + 76..offset + 80)
            .is_some_and(|trailer| {
                trailer == [0; 4]
                    || marker_profile_curve_role(payload, offset) == Some(2)
                        && payload.get(offset + 35..offset + 39) == Some(&[0x00, 0x00, 0x0c, 0x00])
                        && payload.get(offset + 56..offset + 64)
                            != Some(&[0x00, 0x00, 0x01, 0x00, 0, 0, 0, 0])
                        && payload.get(offset + 60..offset + 64) == Some(&[0; 4])
                        && trailer != [0xff; 4]
            })
        && payload
            .get(offset + 80..offset + 84)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.saturating_add(84))
}

pub(super) fn extended_compact_84_profile_line_uses_point_roster(
    payload: &[u8],
    offset: usize,
) -> bool {
    payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 31)
            == Some(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00])
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x08, 0x00])
        && matches!(
            payload.get(offset + 39..offset + 48),
            Some([0x40 | 0x58, 0, 0, 0, 0, 0, 0, 0, 0])
        )
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload
            .get(offset + 56..offset + 60)
            .is_some_and(|endpoints| {
                endpoints[..2] != [0xff; 2]
                    && endpoints[2..] != [0xff; 2]
                    && endpoints[..2] != endpoints[2..]
            })
        && payload.get(offset + 60..offset + 64) == Some(&[0; 4])
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && matches!(
            payload.get(offset + 72..offset + 76),
            Some([0, 0, 0 | 2, 0])
        )
        && payload.get(offset + 76..offset + 80) != Some(&[0xff; 4])
        && payload
            .get(offset + 80..offset + 84)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.saturating_add(84))
}

pub(super) fn legacy_compact_84_profile_line_uses_point_roster(
    payload: &[u8],
    offset: usize,
) -> bool {
    payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 31..offset + 41)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x08, 0x00, 0x58, 0x00])
        && payload.get(offset + 41..offset + 48) == Some(&[0; 7])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload
            .get(offset + 56..offset + 60)
            .is_some_and(|endpoints| {
                endpoints[..2] != [0xff; 2]
                    && endpoints[2..] != [0xff; 2]
                    && endpoints[..2] != endpoints[2..]
            })
        && payload.get(offset + 60..offset + 64) == Some(&[0; 4])
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && matches!(
            payload.get(offset + 72..offset + 76),
            Some([0x00, 0x00, 0x00 | 0x02, 0x00])
        )
        && payload
            .get(offset + 76..offset + 80)
            .is_some_and(|trailer| trailer != [0xff; 4])
        && payload
            .get(offset + 80..offset + 84)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.saturating_add(84))
}

pub(super) fn legacy_terminal_profile_endpoint_offset(
    payload: &[u8],
    offset: usize,
) -> Option<usize> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(0)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
    {
        return None;
    }
    let layout = |endpoint_offset: usize, state_offset: usize, end: usize| {
        (payload.get(offset + endpoint_offset + 4..offset + endpoint_offset + 8) == Some(&[0; 4])
            && payload.get(offset + endpoint_offset + 8..offset + endpoint_offset + 16)
                == Some(&(-1.0f64).to_le_bytes())
            && payload.get(offset + state_offset..offset + state_offset + 4)
                == Some(&[0x00, 0x00, 0x02, 0x00])
            && payload
                .get(offset + state_offset + 4..offset + end)
                .is_some_and(|trailer| trailer.chunks_exact(4).all(|cell| cell != [0xff; 4]))
            && sketch_marker_prefix_at(payload, offset.saturating_add(end)))
        .then_some(endpoint_offset)
    };
    if payload.get(offset + 56..offset + 64) == Some(&[0; 8]) {
        layout(64, 80, 92)
    } else {
        layout(56, 72, 84)
    }
}

pub(super) fn legacy_unlocated_geometry_handle(payload: &[u8], offset: usize) -> bool {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(0)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
    {
        return false;
    }
    let layout = |tag_offset: usize, sentinel_offset: usize, end: usize| {
        payload.get(offset + tag_offset..offset + tag_offset + 2) == Some(&[0x12, 0x00])
            && payload.get(offset + tag_offset + 2..offset + sentinel_offset) == Some(&[0; 26])
            && payload.get(offset + sentinel_offset..offset + sentinel_offset + 4)
                == Some(&[0xfe, 0xff, 0xff, 0xff])
            && payload.get(offset + sentinel_offset + 4..offset + end - 4) == Some(&[0; 42])
            && sketch_marker_prefix_at(payload, offset.saturating_add(end))
    };
    payload.get(offset + 56..offset + 64) == Some(&[0; 8]) && layout(64, 92, 142)
        || layout(56, 84, 134)
}

pub(super) fn coordinate_roster_endpoint_offset(payload: &[u8], offset: usize) -> Option<usize> {
    let prefix = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())?;
    if prefix == SKETCH_MARKER {
        return if current_compact_104_indexed_line_endpoint_indices(payload, offset).is_some() {
            Some(64)
        } else if compact_indexed_curve_endpoint_indices(payload, offset).is_some()
            && matches!(
                compact_indexed_curve_record_end(payload, offset),
                Some(
                    CompactIndexedCurveRecordEnd::Marker84
                        | CompactIndexedCurveRecordEnd::Marker96
                        | CompactIndexedCurveRecordEnd::Marker104
                )
            )
        {
            Some(56)
        } else {
            (wide_indexed_curve_endpoint_indices(payload, offset).is_some()
                && sketch_marker_prefix_at(payload, offset.checked_add(92)?))
            .then_some(64)
        };
    }
    if prefix == LEGACY_EXTENDED_SKETCH_MARKER {
        return if extended_profile_roster_construction_line_endpoint_indices(payload, offset)
            .is_some()
        {
            Some(64)
        } else if extended_marker104_arc_uses_point_roster(payload, offset)
            || extended_marker84_line_uses_point_roster(payload, offset)
            || extended_compact_84_profile_line_uses_point_roster(payload, offset)
            || extended_compact_indexed_curve_endpoint_indices(payload, offset).is_some()
            || compact_curve_endpoint_indices(payload, offset).is_some()
        {
            Some(56)
        } else if wide_indexed_curve_endpoint_indices(payload, offset).is_some() {
            Some(64)
        } else {
            None
        };
    }
    if prefix != LEGACY_SKETCH_MARKER {
        return None;
    }
    if packed_compact_legacy_curve_endpoint_indices(payload, offset).is_some() {
        return Some(48);
    }
    if legacy_compact_84_profile_line_uses_point_roster(payload, offset) {
        return Some(56);
    }
    if extended_compact_legacy_curve_record(payload, offset) {
        return Some(42);
    }
    if compact_legacy_short_role_one_curve_endpoint_indices(payload, offset).is_some() {
        return Some(42);
    }
    if compact_legacy_short_role_two_curve_endpoint_indices(payload, offset).is_some() {
        return Some(42);
    }
    if packed_legacy_curve_endpoint_indices(payload, offset).is_some() {
        return Some(48);
    }
    if legacy_coordinate_roster_selected_axis_endpoint_indices(payload, offset).is_some() {
        return Some(64);
    }
    if legacy_compact_roster_selected_axis_endpoint_indices(payload, offset).is_some() {
        return Some(56);
    }
    if legacy_profile_roster_selected_axis_endpoint_indices(payload, offset).is_some() {
        return Some(64);
    }
    if standard_legacy_compact_selected_axis_endpoint_indices(payload, offset).is_some() {
        return Some(56);
    }
    if let Some(relative) = legacy_state_five_curve_endpoint_offset(payload, offset) {
        return Some(relative);
    }
    if legacy_referenced_wide_arc_endpoint_indices(payload, offset).is_some() {
        return Some(64);
    }
    if !matches!(
        payload.get(offset + 23..offset + 27),
        Some(locus) if locus == [0x04, 0x00, 0x02, 0x00] || locus == [0x05, 0x00, 0x01, 0x00]
    ) {
        return None;
    }
    if compact_indexed_curve_endpoint_indices(payload, offset).is_some()
        || compact_curve_endpoint_indices(payload, offset).is_some()
    {
        Some(56)
    } else if wide_indexed_curve_endpoint_indices(payload, offset).is_some() {
        Some(64)
    } else {
        None
    }
}

pub(super) fn packed_legacy_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if !packed_legacy_marker_body(payload, offset)
        || !matches!(marker_native_code(payload, offset), Some(0..=2))
        || !matches!(marker_profile_curve_role(payload, offset), Some(1 | 2))
        || payload.get(offset + 52..offset + 56) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&(-1.0f64).to_le_bytes())
    {
        return None;
    }
    let endpoint = |relative| {
        Some(u32::from(u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        )))
    };
    let endpoints = [endpoint(48)?, endpoint(50)?];
    (endpoints[0] != endpoints[1]).then_some(endpoints)
}

pub(super) fn packed_compact_legacy_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    let code = marker_native_code(payload, offset)?;
    let role = marker_profile_curve_role(payload, offset)?;
    let local_state = u16::from_le_bytes(payload.get(offset + 66..offset + 68)?.try_into().ok()?);
    let body_tag = if role == 1 { 5 } else { 12 };
    if !packed_legacy_marker_body(payload, offset)
        || !matches!((code, role), (0, 1) | (1, 2))
        || payload.get(offset + 25..offset + 29) != Some(&[0; 4])
        || payload.get(offset + 29..offset + 40) != Some(&[body_tag, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || payload.get(offset + 40..offset + 48) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 52..offset + 56) != Some(&[0; 4])
        || payload.get(offset + 56..offset + 64) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 64..offset + 66) != Some(&[0; 2])
        || !matches!((role, local_state), (1, 0..=2) | (2, 2))
        || payload.get(offset + 68..offset + 72) != Some(&[0; 4])
        || payload
            .get(offset + 72..offset + 76)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(76)?)
    {
        return None;
    }
    let endpoint = |relative| {
        Some(u32::from(u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        )))
    };
    let endpoints = [endpoint(48)?, endpoint(50)?];
    (endpoints[0] != endpoints[1]).then_some(endpoints)
}

pub(super) fn legacy_state_five_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    let relative = legacy_state_five_curve_endpoint_offset(payload, offset)?;
    one_based_u16_endpoint_pair(payload, offset, relative)
}

pub(super) fn extended_profile_roster_construction_line_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&[0; 4])
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 84) != Some(&[0; 4])
        || !payload
            .get(offset + 84..offset + 88)
            .zip(payload.get(offset + 88..offset + 92))
            .is_some_and(|(first, second)| first == second && first != [0; 4] && first != [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 64)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn extended_compact_84_construction_line_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0x00, 0x00, 0x01, 0x00, 0, 0, 0, 0])
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || !matches!(
            payload.get(offset + 72..offset + 76),
            Some([0x00, 0x00, 0x00 | 0x01, 0x00])
        )
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    let endpoint = |relative| {
        let id = u32::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 4)?
                .try_into()
                .ok()?,
        );
        (!matches!(id, 0 | u32::MAX)).then_some(id)
    };
    let endpoints = [endpoint(76)?, endpoint(80)?];
    (endpoints[0] != endpoints[1]).then_some(endpoints)
}

pub(super) fn extended_compact_96_selected_axis_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    let state = u16::from_le_bytes(payload.get(offset + 82..offset + 84)?.try_into().ok()?);
    let identity = u32::from_le_bytes(payload.get(offset + 88..offset + 92)?.try_into().ok()?);
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&[0; 4])
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&[0; 8])
        || payload.get(offset + 80..offset + 82) != Some(&[0; 2])
        || matches!(state, 0 | u16::MAX)
        || payload.get(offset + 84..offset + 88) != Some(&[0; 4])
        || identity != u32::from(state)
        || payload.get(offset + 92..offset + 96) != Some(&1u32.to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(96)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 56)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn current_compact_104_indexed_line_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 88) != Some(&[0; 8])
        || payload.get(offset + 88..offset + 92) != Some(&[0x00, 0x00, 0x01, 0x00])
        || payload.get(offset + 92..offset + 96) != Some(&[0; 4])
        || payload.get(offset + 96..offset + 100) != Some(&[0; 4])
        || payload.get(offset + 100..offset + 104) != Some(&1u32.to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(104)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 64)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn legacy_compact_104_profile_line_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    let object_index = marker_object_index(payload, offset)?;
    let retained_object_index =
        u32::from_le_bytes(payload.get(offset + 96..offset + 100)?.try_into().ok()?);
    let next_object_index =
        u32::from_le_bytes(payload.get(offset + 100..offset + 104)?.try_into().ok()?);
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(0)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 76) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 76..offset + 78) != Some(&[0; 2])
        || payload.get(offset + 78..offset + 94)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 94..offset + 96) != Some(&[0; 2])
        || retained_object_index != object_index
        || matches!(next_object_index, 0 | u32::MAX)
        || next_object_index == retained_object_index
        || !sketch_marker_prefix_at(payload, offset.checked_add(104)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 56)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn current_compact_104_profile_line(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && one_based_u16_endpoint_pair(payload, offset, 56)
            .is_some_and(|endpoints| endpoints[0] != endpoints[1])
        && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 72..offset + 76) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 76..offset + 78) == Some(&[0; 2])
        && payload.get(offset + 78..offset + 94)
            == Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        && payload.get(offset + 94..offset + 96) == Some(&[0; 2])
        && payload
            .get(offset + 96..offset + 100)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && payload.get(offset + 96..offset + 100) == payload.get(offset + 100..offset + 104)
        && offset
            .checked_add(104)
            .is_some_and(|next| sketch_marker_prefix_at(payload, next))
}

pub(super) fn current_direct_92_profile_line_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 84) != Some(&[0; 4])
        || payload.get(offset + 84..offset + 88) != Some(&1u32.to_le_bytes())
        || payload
            .get(offset + 88..offset + 92)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    let endpoint = |relative| {
        let id = u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        );
        (id != 0 && id != u16::MAX).then_some(u32::from(id))
    };
    let endpoints = [endpoint(64)?, endpoint(66)?];
    (endpoints[0] != endpoints[1]).then_some(endpoints)
}

pub(super) fn extended_terminal_profile_line(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && marker_native_code(payload, offset) == Some(0)
        && marker_is_geometry_locus(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && one_based_u16_endpoint_pair(payload, offset, 56)
            .is_some_and(|endpoints| endpoints[0] != endpoints[1])
        && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 72..offset + 142) == Some(&[0; 70])
        && payload.get(offset + 142..offset + 144) == Some(&[0x08, 0x80])
        && payload.get(offset + 144..offset + 154) == Some(&[0; 10])
        && payload.get(offset + 154..offset + 170)
            == Some(&[
                0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00,
                0x00, 0x00,
            ])
}

pub(super) fn legacy_referenced_wide_arc_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(0)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 84) != Some(&1i32.to_le_bytes())
        || payload.get(offset + 84..offset + 86) != Some(&[0; 2])
        || payload.get(offset + 86..offset + 102)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 102..offset + 104) != Some(&[0; 2])
        || !payload
            .get(offset + 104..offset + 108)
            .zip(payload.get(offset + 108..offset + 112))
            .is_some_and(|(first, second)| first == second && first != [0; 4] && first != [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(112)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 64)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

fn legacy_state_five_curve_endpoint_offset(payload: &[u8], offset: usize) -> Option<usize> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(0)
        || !matches!(
            payload.get(offset + 23..offset + 27),
            Some(locus) if locus == [0x04, 0x00, 0x02, 0x00] || locus == [0x05, 0x00, 0x01, 0x00]
        )
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
    {
        return None;
    }
    if legacy_terminal_profile_endpoint_offset(payload, offset).is_some() {
        return None;
    }
    let identity_trailer = payload.get(offset + 60..offset + 64) == Some(&[0; 4])
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 72..offset + 76) == Some(&[0; 4])
        && matches!(
            (
                payload
                    .get(offset + 76..offset + 80)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u32::from_le_bytes),
                payload
                    .get(offset + 80..offset + 84)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u32::from_le_bytes),
            ),
            (Some(first), Some(second))
                if first != 0
                    && first != u32::MAX
                    && second != 0
                    && second != u32::MAX
                    && first != second
        )
        && sketch_marker_prefix_at(payload, offset.saturating_add(84));
    if identity_trailer {
        Some(56)
    } else if payload.get(offset + 60..offset + 64) == Some(&0u32.to_le_bytes())
        && payload.get(offset + 70..offset + 72) == Some(&0u16.to_le_bytes())
        && payload.get(offset + 72..offset + 80) == Some(&(-1.0f64).to_le_bytes())
        && sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        Some(64)
    } else if payload
        .get(offset + 60..offset + 64)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .is_some_and(|state| matches!(state, 0 | 1))
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 72..offset + 74) == Some(&[0; 2])
        && matches!(payload.get(offset + 74..offset + 76), Some([0..=2, 0]))
        && payload.get(offset + 76..offset + 80) == Some(&[0; 4])
        && sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        Some(56)
    } else {
        None
    }
}

pub(super) fn legacy_undetailed_profile_line(payload: &[u8], offset: usize) -> bool {
    let packed = packed_compact_legacy_curve_endpoint_indices(payload, offset).is_some()
        && marker_profile_curve_role(payload, offset) == Some(1);
    let standard = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
        == Some(LEGACY_SKETCH_MARKER)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && (compact_indexed_curve_endpoint_indices(payload, offset).is_some()
            || legacy_state_five_curve_endpoint_indices(payload, offset).is_some()
            || legacy_terminal_profile_endpoint_offset(payload, offset).is_some());
    (packed || standard) && compact_bounded_curve_tangent(payload, offset).is_none()
}

pub(super) fn extended_compact_104_indexed_arc(payload: &[u8], offset: usize) -> bool {
    let profile_selector = payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && payload.get(offset + 35..offset + 39) == Some(&[0x00, 0x00, 0x04, 0x00]);
    let geometry_selector = marker_is_geometry_locus(payload, offset)
        && payload.get(offset + 35..offset + 39) == Some(&[0x00, 0x00, 0x05, 0x00]);
    payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && payload.get(offset + 17..offset + 21) == Some(&0u32.to_le_bytes())
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 31..offset + 35) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && (profile_selector || geometry_selector)
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload
            .get(offset + 72..offset + 76)
            .and_then(|bytes| bytes.try_into().ok())
            .map(i32::from_le_bytes)
            .is_some_and(|selector| matches!(selector, -1 | 1))
        && payload.get(offset + 78..offset + 94)
            == Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        && payload.get(offset + 94..offset + 96) == Some(&[0; 2])
        && sketch_marker_prefix_at(payload, offset.saturating_add(104))
}

pub(super) fn indexed_arc_uses_coordinate_center(payload: &[u8], offset: usize) -> bool {
    let current_compact_84 = payload.get(offset..offset + SKETCH_MARKER.len())
        == Some(SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && compact_indexed_curve_endpoint_indices(payload, offset).is_some()
        && compact_indexed_curve_record_end(payload, offset)
            == Some(CompactIndexedCurveRecordEnd::Marker84);
    let extended_compact_84 = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && payload.get(offset + 17..offset + 21) == Some(&0u32.to_le_bytes())
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload
            .get(offset + 56..offset + 58)
            .is_some_and(|first| first != [0, 0] && first != [0xff, 0xff])
        && payload
            .get(offset + 58..offset + 60)
            .is_some_and(|second| second != [0, 0] && second != [0xff, 0xff])
        && payload.get(offset + 56..offset + 58) != payload.get(offset + 58..offset + 60)
        && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 72..offset + 76) == Some(&[0x00, 0x00, 0x01, 0x00])
        && payload.get(offset + 76..offset + 80) == Some(&[0; 4])
        && payload
            .get(offset + 80..offset + 84)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.saturating_add(84));
    let extended_compact = extended_compact_104_indexed_arc(payload, offset);
    let current_wide = payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && wide_indexed_curve_endpoint_indices(payload, offset).is_some()
        && sketch_marker_prefix_at(payload, offset.saturating_add(92));
    let extended_wide = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && wide_indexed_curve_endpoint_indices(payload, offset).is_some()
        && sketch_marker_prefix_at(payload, offset.saturating_add(92));
    current_compact_84
        || extended_compact_84
        || extended_compact
        || current_wide
        || extended_wide
        || legacy_referenced_wide_arc_endpoint_indices(payload, offset).is_some()
}

pub(super) fn current_indexed_arc_reverses_center_sweep(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && wide_indexed_curve_endpoint_indices(payload, offset).is_some()
        && payload.get(offset + 80..offset + 84) == Some(&[0x00, 0x00, 0x02, 0x00])
        && sketch_marker_prefix_at(payload, offset.saturating_add(92))
}

pub(super) fn unique_arc_center_marker(
    start: Point2,
    end: Point2,
    candidates: &[Point2],
    tolerance: f64,
) -> Option<Point2> {
    if start == end {
        return None;
    }
    let mut centers = candidates
        .iter()
        .copied()
        .filter_map(|center| {
            let radius = (start.u - center.u).hypot(start.v - center.v);
            let end_radius = (end.u - center.u).hypot(end.v - center.v);
            if radius <= tolerance
                || (radius - end_radius).abs()
                    > tolerance * radius.abs().max(end_radius.abs()).max(1.0)
            {
                return None;
            }
            let start_angle = (start.v - center.v).atan2(start.u - center.u);
            let end_angle = (end.v - center.v).atan2(end.u - center.u);
            let sweep = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
            if sweep <= tolerance || (std::f64::consts::TAU - sweep) <= tolerance {
                return None;
            }
            (sweep <= std::f64::consts::PI + tolerance)
                .then_some((quantize(center, tolerance), center))
        })
        .collect::<Vec<_>>();
    centers.sort_unstable_by_key(|(center, _)| *center);
    centers.dedup_by_key(|(center, _)| *center);
    let [(_, center)] = centers.as_slice() else {
        return None;
    };
    Some(*center)
}

pub(super) fn minor_arc_geometry(
    start: Point2,
    end: Point2,
    center: Point2,
    tolerance: f64,
) -> Option<SketchGeometry> {
    let radius = (start.u - center.u).hypot(start.v - center.v);
    let end_radius = (end.u - center.u).hypot(end.v - center.v);
    if radius <= tolerance
        || (radius - end_radius).abs() > tolerance * radius.abs().max(end_radius.abs()).max(1.0)
    {
        return None;
    }
    let start_angle = (start.v - center.v).atan2(start.u - center.u);
    let end_angle = (end.v - center.v).atan2(end.u - center.u);
    let sweep = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
    let (start_angle, end_angle) = if sweep <= std::f64::consts::PI + tolerance {
        (start_angle, end_angle)
    } else {
        (end_angle, start_angle)
    };
    Some(SketchGeometry::Arc {
        center,
        radius: Length(radius),
        start_angle: Angle(start_angle),
        end_angle: Angle(end_angle),
    })
}

pub(super) fn legacy_coordinate_roster_selected_axis_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(0)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0d, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 68..offset + 72) != Some(&0u32.to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 64)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn legacy_profile_roster_selected_axis_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    let kind_two_state_trailer = matches!(
        payload.get(offset + 80..offset + 84),
        Some([0x00, 0x00, 0x01 | 0x02, 0x00])
    ) && payload.get(offset + 84..offset + 88) == Some(&[0; 4]);
    let kind_two_identity_trailer = payload.get(offset + 80..offset + 84) == Some(&[0; 4])
        && payload
            .get(offset + 84..offset + 88)
            .zip(payload.get(offset + 88..offset + 92))
            .is_some_and(|(first, second)| {
                first == second && first != [0; 4] && first != u32::MAX.to_le_bytes()
            });
    let kind_one_identity_trailer = matches!(
        payload.get(offset + 80..offset + 84),
        Some([0x00, 0x00, 0x00 | 0x02, 0x00])
    ) && payload.get(offset + 84..offset + 88) == Some(&[0; 4])
        && payload
            .get(offset + 88..offset + 92)
            .is_some_and(|identity| identity != [0; 4] && identity != u32::MAX.to_le_bytes());
    let kind = marker_native_code(payload, offset);
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || !matches!(kind, Some(1 | 2))
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&[0; 4])
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || !(kind == Some(2) && (kind_two_state_trailer || kind_two_identity_trailer)
            || kind == Some(1) && kind_one_identity_trailer)
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 64)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn compact_legacy_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    compact_legacy_curve_endpoint_indices_for_code(payload, offset, 0)
}

pub(super) fn compact_legacy_short_role_two_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    compact_legacy_short_curve_endpoint_indices_for_role(payload, offset, 2)
}

pub(super) fn compact_legacy_short_role_one_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    compact_legacy_short_curve_endpoint_indices_for_role(payload, offset, 1)
}

fn compact_legacy_short_curve_endpoint_indices_for_role(
    payload: &[u8],
    offset: usize,
    role: u16,
) -> Option<[u32; 2]> {
    let state = u16::from_le_bytes(payload.get(offset + 25..offset + 27)?.try_into().ok()?);
    let body_tag = match (role, state) {
        (1, 0 | 1) => 0x04,
        (2, 0) => 0x0c,
        _ => return None,
    };
    if !compact_legacy_marker_body(payload, offset)
        || !matches!(marker_native_code(payload, offset), Some(0 | 1))
        || marker_profile_curve_role(payload, offset) != Some(role)
        || payload.get(offset + 31..offset + 42) != Some(&[body_tag, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || payload.get(offset + 46..offset + 50) != Some(&u32::from(state).to_le_bytes())
        || payload.get(offset + 50..offset + 58) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 62..offset + 64) != Some(&[0; 2])
        || !payload
            .get(offset + 64..offset + 68)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(68)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 42)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn compact_legacy_code_one_line_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    compact_legacy_curve_endpoint_indices_for_code(payload, offset, 1)
}

fn compact_legacy_curve_endpoint_indices_for_code(
    payload: &[u8],
    offset: usize,
    code: u32,
) -> Option<[u32; 2]> {
    let short_record = sketch_marker_prefix_at(payload, offset.checked_add(68)?);
    let terminal_record = payload.get(offset + 58..offset + 104) == Some(&[0; 46])
        && payload
            .get(offset + 104..offset + 106)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
            .is_some_and(|selector| selector != 0 && selector != u16::MAX)
        && class_declaration_at(payload, offset.saturating_add(106));
    if !compact_legacy_marker_body(payload, offset)
        || marker_native_code(payload, offset) != Some(code)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 25..offset + 27) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 42) != Some(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || payload.get(offset + 46..offset + 50) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 50..offset + 58) != Some(&(-1.0f64).to_le_bytes())
        || !(short_record
            || code == 1 && terminal_record
            || extended_compact_legacy_curve_record(payload, offset))
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 42)
}

fn extended_compact_legacy_curve_record(payload: &[u8], offset: usize) -> bool {
    let continued = payload
        .get(offset + 82..offset + 86)
        .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && offset
            .checked_add(90)
            .is_some_and(|next| sketch_marker_prefix_at(payload, next));
    let terminal = payload.get(offset + 82..offset + 136) == Some(&[0; 54])
        && payload.get(offset + 136..offset + 138) == Some(&[0x08, 0x80]);
    compact_legacy_marker_body(payload, offset)
        && marker_native_code(payload, offset) == Some(0)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 25..offset + 27) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 42) == Some(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        && payload.get(offset + 46..offset + 50) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 50..offset + 58) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 58..offset + 62) == Some(&1u32.to_le_bytes())
        && payload
            .get(offset + 64..offset + 80)
            .is_some_and(|trailer| {
                trailer
                    .chunks_exact(4)
                    .all(|cell| cell == (-2i32).to_le_bytes())
            })
        && payload.get(offset + 80..offset + 82) == Some(&0u16.to_le_bytes())
        && (continued || terminal)
}

pub(super) fn alternate_current_indexed_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    (alternate_current_curve_body(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1))
    .then(|| one_based_u16_endpoint_pair(payload, offset, 56))
    .flatten()
}

pub(super) fn one_based_u16_endpoint_pair(
    payload: &[u8],
    offset: usize,
    relative: usize,
) -> Option<[u32; 2]> {
    let endpoint = |relative: usize| {
        u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        )
        .checked_add(1)
        .map(u32::from)
    };
    Some([endpoint(relative)?, endpoint(relative + 2)?])
}

pub(super) fn compact_indexed_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    compact_indexed_curve_endpoint_indices_for_prefixes(
        payload,
        offset,
        &[SKETCH_MARKER, LEGACY_SKETCH_MARKER],
    )
}

pub(super) fn legacy_compact_profile_line(payload: &[u8], offset: usize) -> bool {
    let common = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
        == Some(LEGACY_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x05, 0x00, 0x01, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 39..offset + 48) == Some(&[0; 9])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && compact_indexed_curve_endpoint_indices(payload, offset).is_some()
        && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes());
    common
        && match compact_indexed_curve_record_end(payload, offset) {
            Some(CompactIndexedCurveRecordEnd::Marker84) => matches!(
                payload.get(offset + 72..offset + 76),
                Some([0, 0, 0 | 2, 0])
            ),
            Some(CompactIndexedCurveRecordEnd::Marker96) => {
                let state = payload
                    .get(offset + 82..offset + 84)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u16::from_le_bytes);
                payload.get(offset + 72..offset + 82) == Some(&[0; 10])
                    && state.is_some_and(|state| !matches!(state, 0 | u16::MAX))
                    && payload.get(offset + 84..offset + 88) == Some(&[0; 4])
                    && payload.get(offset + 92..offset + 96) == Some(&1u32.to_le_bytes())
            }
            _ => false,
        }
}

pub(super) fn direct_indexed_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(0)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&[0; 4])
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    let endpoint = |relative: usize| {
        let id = u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        );
        (id != 0).then_some(u32::from(id))
    };
    let endpoints = [endpoint(56)?, endpoint(58)?];
    (endpoints[0] != endpoints[1]).then_some(endpoints)
}

pub(super) fn extended_compact_indexed_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if !matches!(
        compact_indexed_curve_record_end(payload, offset),
        Some(
            CompactIndexedCurveRecordEnd::Marker84
                | CompactIndexedCurveRecordEnd::Marker96
                | CompactIndexedCurveRecordEnd::Continuation120
        )
    ) {
        return None;
    }
    compact_indexed_curve_endpoint_indices_for_prefixes(
        payload,
        offset,
        &[LEGACY_EXTENDED_SKETCH_MARKER],
    )
}

fn compact_indexed_curve_endpoint_indices_for_prefixes(
    payload: &[u8],
    offset: usize,
    prefixes: &[&[u8]],
) -> Option<[u32; 2]> {
    let code = u32::from_le_bytes(payload.get(offset + 17..offset + 21)?.try_into().ok()?);
    let prefix = payload.get(offset..offset + SKETCH_MARKER.len())?;
    let standard_selector =
        payload.get(offset + 35..offset + 39) == Some(&[0x00, 0x00, 0x04, 0x00]);
    let flagged_profile_selector = prefix == LEGACY_EXTENDED_SKETCH_MARKER
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && payload.get(offset + 35..offset + 39) == Some(&[0x00, 0x00, 0x45, 0x00]);
    if !prefixes.contains(&prefix)
        || !matches!(code, 0..=2)
        || !(marker_is_geometry_locus(payload, offset)
            || payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00]))
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 31..offset + 35) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || !(standard_selector || flagged_profile_selector)
        || f64::from_le_bytes(payload.get(offset + 48..offset + 56)?.try_into().ok()?) != 1.0
        || compact_indexed_curve_record_end(payload, offset).is_none()
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 56)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CompactIndexedCurveRecordEnd {
    Marker84,
    Marker96,
    Marker104,
    Terminal102,
    Terminal116,
    Continuation120,
    ReferenceTable126,
}

pub(super) fn compact_indexed_curve_record_end(
    payload: &[u8],
    offset: usize,
) -> Option<CompactIndexedCurveRecordEnd> {
    if sketch_marker_prefix_at(payload, offset.saturating_add(84)) {
        return Some(CompactIndexedCurveRecordEnd::Marker84);
    }
    let terminal_116 = payload.get(offset + 60..offset + 64) == Some(&[0; 4])
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 72..offset + 116) == Some(&[0; 44])
        && !sketch_marker_prefix_at(payload, offset.saturating_add(116));
    if terminal_116 {
        return Some(CompactIndexedCurveRecordEnd::Terminal116);
    }
    let continuation_kind = payload
        .get(offset + 120..offset + 122)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes);
    let relation_continuation = payload
        .get(offset + 122..offset + 124)
        .is_some_and(|selector| !matches!(selector, [0, 0] | [0xff, 0xff]))
        && payload.get(offset + 124..offset + 128) == Some(&[0; 4])
        && payload
            .get(offset + 128..offset + 132)
            .is_some_and(|selectors| {
                !matches!(&selectors[..2], [0, 0] | [0xff, 0xff])
                    && !matches!(&selectors[2..], [0, 0] | [0xff, 0xff])
                    && selectors[..2] != selectors[2..]
            })
        && payload.get(offset + 132..offset + 140)
            == Some(&[0xff, 0xfe, 0xff, 0x02, 0x44, 0x00, 0x31, 0x00]);
    let continuation_120 = payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 72..offset + 120) == Some(&[0; 48])
        && continuation_kind.is_some_and(|kind| kind != 0 && kind != u16::MAX)
        && (class_declaration_at(payload, offset.saturating_add(122)) || relation_continuation);
    if continuation_120 {
        return Some(CompactIndexedCurveRecordEnd::Continuation120);
    }
    let reference_table_126 = payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 72..offset + 126) == Some(&[0; 54])
        && payload
            .get(offset + 126..offset + 128)
            .is_some_and(|count| !matches!(count, [0, 0] | [0xff, 0xff]))
        && payload.get(offset + 128..offset + 136) == Some(&[0; 8])
        && payload.get(offset + 136..offset + 140) == Some(&[0xff; 4])
        && payload.get(offset + 140..offset + 154) == Some(&[0; 14])
        && payload
            .get(offset + 154..offset + 158)
            .is_some_and(|kind| kind != [0; 4] && kind != [0xff; 4])
        && payload.get(offset + 158..offset + 170)
            == Some(&[
                0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfe, 0xff, 0x00, 0x00,
            ])
        && payload
            .get(offset + 170..offset + 172)
            .is_some_and(|selector| !matches!(selector, [0, 0] | [0xff, 0xff]))
        && payload.get(offset + 172..offset + 174) == Some(&[0; 2])
        && payload.get(offset + 174..offset + 178) == Some(&[0xff; 4])
        && payload.get(offset + 178..offset + 190) == Some(&[0; 12])
        && payload.get(offset + 190..offset + 194) == Some(&[0xff; 4])
        && payload.get(offset + 194..offset + 206) == Some(&[0; 12]);
    if reference_table_126 {
        return Some(CompactIndexedCurveRecordEnd::ReferenceTable126);
    }
    if payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
    {
        return None;
    }
    let compact_96 = payload.get(offset + 72..offset + 82) == Some(&[0; 10])
        && payload
            .get(offset + 82..offset + 84)
            .is_some_and(|state| !matches!(state, [0, 0] | [0xff, 0xff]))
        && payload.get(offset + 84..offset + 88) == Some(&[0; 4])
        && payload.get(offset + 92..offset + 96) == Some(&1u32.to_le_bytes())
        && sketch_marker_prefix_at(payload, offset.saturating_add(96));
    let selector = payload
        .get(offset + 72..offset + 76)
        .and_then(|bytes| bytes.try_into().ok())
        .map(i32::from_le_bytes);
    let reference_sentinel = payload.get(offset + 78..offset + 94)
        == Some(&[
            0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
            0xff, 0xff,
        ]);
    let compact_104 = matches!(selector, Some(-1 | 1))
        && reference_sentinel
        && payload.get(offset + 94..offset + 96) == Some(&[0; 2])
        && sketch_marker_prefix_at(payload, offset.saturating_add(104));
    let terminal_102 = matches!(selector, Some(-1 | 1))
        && reference_sentinel
        && payload.get(offset + 94..offset + 102) == Some(&[0; 8])
        && !sketch_marker_prefix_at(payload, offset.saturating_add(102));
    if compact_96 {
        Some(CompactIndexedCurveRecordEnd::Marker96)
    } else if compact_104 {
        Some(CompactIndexedCurveRecordEnd::Marker104)
    } else if terminal_102 {
        Some(CompactIndexedCurveRecordEnd::Terminal102)
    } else {
        None
    }
}

pub(super) fn wide_indexed_curve_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    let prefix = payload.get(offset..offset + SKETCH_MARKER.len())?;
    let supported_prefix = prefix == SKETCH_MARKER
        || prefix == LEGACY_SKETCH_MARKER
        || prefix == LEGACY_EXTENDED_SKETCH_MARKER;
    let supported_locus = marker_is_geometry_locus(payload, offset)
        || payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00]);
    let supported_state = payload.get(offset + 35..offset + 39) == Some(&[0x00, 0x00, 0x04, 0x00])
        || current_extended_wide_curve_body(payload, offset)
        || prefix == LEGACY_SKETCH_MARKER
            && (marker_is_geometry_locus(payload, offset)
                && payload.get(offset + 35..offset + 39) == Some(&[0x00, 0x00, 0x05, 0x00])
                || matches!(
                    payload.get(offset + 35..offset + 39),
                    Some([0x00, 0x00, 0x44 | 0x84, 0x00])
                ));
    if !supported_prefix
        || !matches!(
            u32::from_le_bytes(payload.get(offset + 17..offset + 21)?.try_into().ok()?),
            0..=2
        )
        || !supported_locus
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 31..offset + 35) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || !supported_state
        || f64::from_le_bytes(payload.get(offset + 48..offset + 56)?.try_into().ok()?) != 1.0
        || payload.get(offset + 68..offset + 72) != Some(&[0x01, 0x00, 0x00, 0x00])
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || !wide_indexed_curve_record_ends_at(payload, offset, prefix)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 64)
}

fn current_extended_wide_curve_body(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && marker_is_geometry_locus(payload, offset)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 35..offset + 39) == Some(&[0x00, 0x00, 0x44, 0x00])
        && payload.get(offset + 56..offset + 64) == Some(&[0; 8])
        && payload.get(offset + 80..offset + 84) == Some(&(-1i32).to_le_bytes())
        && payload.get(offset + 84..offset + 86) == Some(&4u16.to_le_bytes())
}

fn wide_indexed_curve_record_ends_at(payload: &[u8], offset: usize, prefix: &[u8]) -> bool {
    if sketch_marker_prefix_at(payload, offset.saturating_add(92)) {
        return true;
    }
    if prefix == LEGACY_SKETCH_MARKER && payload.get(offset + 80..offset + 128) == Some(&[0; 48]) {
        return true;
    }
    let selector = payload
        .get(offset + 80..offset + 84)
        .and_then(|bytes| bytes.try_into().ok())
        .map(i32::from_le_bytes);
    let state = payload
        .get(offset + 84..offset + 86)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes);
    let identities = [104usize, 108].map(|relative| {
        payload
            .get(offset + relative..offset + relative + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
    });
    let referenced = (prefix == LEGACY_SKETCH_MARKER
        || current_extended_wide_curve_body(payload, offset))
        && matches!(selector, Some(-1 | 1))
        && state.is_some_and(|state| state != 0)
        && payload.get(offset + 86..offset + 102)
            == Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        && payload.get(offset + 102..offset + 104) == Some(&[0; 2])
        && matches!(identities, [Some(first), Some(second)] if first != u32::MAX && second != u32::MAX && first != second)
        && sketch_marker_prefix_at(payload, offset.saturating_add(112));
    referenced || legacy_terminal_wide_indexed_curve(payload, offset)
}

fn legacy_terminal_wide_indexed_curve(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(1)
        && payload.get(offset + 80..offset + 84) == Some(&1i32.to_le_bytes())
        && payload.get(offset + 84..offset + 86) == Some(&12u16.to_le_bytes())
        && payload.get(offset + 86..offset + 102)
            == Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        && payload.get(offset + 102..offset + 136) == Some(&[0; 34])
        && payload.get(offset + 136..offset + 138) == Some(&[0x05, 0x00])
        && class_declaration_at(payload, offset.saturating_add(138))
}

fn class_declaration_at(payload: &[u8], offset: usize) -> bool {
    let Some(length) = payload
        .get(offset + 4..offset + 6)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
        .map(usize::from)
    else {
        return false;
    };
    payload.get(offset..offset + CLASS_MARKER.len()) == Some(CLASS_MARKER)
        && (1..=128).contains(&length)
        && payload
            .get(offset + 6..offset + 6 + length)
            .is_some_and(|name| {
                name.iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            })
}

pub(super) fn marker_profile_curve_role(payload: &[u8], offset: usize) -> Option<u16> {
    let relative = if compact_legacy_marker_body(payload, offset)
        || packed_legacy_marker_body(payload, offset)
    {
        23
    } else {
        27
    };
    Some(u16::from_le_bytes(
        payload
            .get(offset + relative..offset + relative + 2)?
            .try_into()
            .ok()?,
    ))
}

pub(super) fn marker_is_selected_construction_line(payload: &[u8], offset: usize) -> bool {
    if (packed_compact_legacy_curve_endpoint_indices(payload, offset).is_some()
        && marker_profile_curve_role(payload, offset) == Some(2))
        || alternate_current_selected_axis_endpoint_indices(payload, offset).is_some()
        || extended_profile_roster_construction_line_endpoint_indices(payload, offset).is_some()
        || extended_compact_96_selected_axis_endpoint_indices(payload, offset).is_some()
        || legacy_direct_compact_selected_axis_endpoint_indices(payload, offset).is_some()
        || legacy_compact_roster_selected_axis_endpoint_indices(payload, offset).is_some()
        || legacy_coordinate_roster_selected_axis_endpoint_indices(payload, offset).is_some()
        || legacy_profile_roster_selected_axis_endpoint_indices(payload, offset).is_some()
        || standard_legacy_compact_selected_axis_endpoint_indices(payload, offset).is_some()
        || compact_legacy_selected_axis_endpoint_indices(payload, offset).is_some()
        || current_vertical_axis_endpoint_indices(payload, offset).is_some()
        || legacy_code_five_or_six_selected_axis_endpoint_indices(payload, offset).is_some()
    {
        true
    } else if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
    {
        extended_horizontal_axis_endpoint_indices(payload, offset).is_some()
            || payload.get(offset + 17..offset + 21) == Some(&2u32.to_le_bytes())
                && !extended_compact_84_profile_line_uses_point_roster(payload, offset)
                && !(marker_profile_curve_role(payload, offset) == Some(1)
                    && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes()))
                && wide_indexed_curve_endpoint_indices(payload, offset).is_none()
    } else {
        false
    }
}

pub(super) fn auxiliary_profile_record(payload: &[u8], offset: usize) -> bool {
    matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix)
            if prefix == SKETCH_MARKER
                || prefix == LEGACY_SKETCH_MARKER
                || prefix == LEGACY_EXTENDED_SKETCH_MARKER
    ) && marker_profile_curve_role(payload, offset) == Some(2)
        && matches!(
            (
                marker_native_code(payload, offset),
                payload.get(offset + 23..offset + 27),
                payload.get(offset + 31..offset + 39)
            ),
            (
                _,
                Some([0x04, 0x00, 0x02, 0x00]),
                Some([0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
            ) | (
                Some(0),
                Some([0x05, 0x00, 0x01, 0x00]),
                Some([0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0d, 0x00])
            )
        )
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && !marker_is_selected_construction_line(payload, offset)
        && compact_legacy_radial_circle_index(payload, offset).is_none()
}

fn legacy_compact_selected_axis_body(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(2)
        && payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 60..offset + 64) == Some(&[0; 4])
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && offset
            .checked_add(84)
            .is_some_and(|next| sketch_marker_prefix_at(payload, next))
}

pub(super) fn legacy_direct_compact_selected_axis_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if !legacy_compact_selected_axis_body(payload, offset) {
        return None;
    }
    if payload.get(offset + 72..offset + 80)
        != Some(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00])
        || payload
            .get(offset + 80..offset + 84)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
    {
        return None;
    }
    let endpoint = |relative| {
        let id = u16::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 2)?
                .try_into()
                .ok()?,
        );
        (!matches!(id, 0 | u16::MAX)).then_some(u32::from(id))
    };
    let endpoints = [endpoint(56)?, endpoint(58)?];
    (endpoints[0] != endpoints[1]).then_some(endpoints)
}

pub(super) fn legacy_compact_roster_selected_axis_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if !legacy_compact_selected_axis_body(payload, offset)
        || payload.get(offset + 72..offset + 76) != Some(&[0; 4])
        || !payload
            .get(offset + 76..offset + 80)
            .zip(payload.get(offset + 80..offset + 84))
            .is_some_and(|(first, second)| first == second && first != [0; 4] && first != [0xff; 4])
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 56)
        .filter(|endpoints| endpoints[0] != endpoints[1])
}

pub(super) fn legacy_code_five_or_six_selected_axis_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    let trailer_matches = match marker_native_code(payload, offset) {
        Some(5) => {
            matches!(
                (
                    payload.get(offset + 84..offset + 88),
                    payload.get(offset + 88..offset + 92)
                ),
                (Some(first), Some(second))
                    if first != [0; 4] && second != [0; 4] && first != second
            ) && payload.get(offset + 80..offset + 84) == Some(&[0; 4])
        }
        Some(6) => {
            payload.get(offset + 80..offset + 84) == Some(&[0x00, 0x00, 0x02, 0x00])
                && payload.get(offset + 84..offset + 88) == Some(&[0; 4])
                && payload
                    .get(offset + 88..offset + 92)
                    .is_some_and(|identity| identity != [0; 4])
        }
        _ => false,
    };
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&0u32.to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || !trailer_matches
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 64)
}

pub(super) fn standard_legacy_compact_selected_axis_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&0u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&[0x00, 0x00, 0x02, 0x00, 0, 0, 0, 0])
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 56)
}

pub(super) fn alternate_current_selected_axis_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if !alternate_current_curve_body(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 35..offset + 39) != Some(&[0x00, 0x00, 0x0d, 0x00])
    {
        return None;
    }
    for relative in [76, 80] {
        let identity = u32::from_le_bytes(
            payload
                .get(offset + relative..offset + relative + 4)?
                .try_into()
                .ok()?,
        );
        if matches!(identity, 0 | u32::MAX) {
            return None;
        }
    }
    one_based_u16_endpoint_pair(payload, offset, 56)
}

pub(super) fn current_compact_roster_selected_axis(payload: &[u8], offset: usize) -> bool {
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(0)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 35) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || payload.get(offset + 35..offset + 39) != Some(&[0x00, 0x00, 0x0d, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&0u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || !matches!(
            payload.get(offset + 72..offset + 76),
            Some([0x00, 0x00, 0x00 | 0x02, 0x00])
        )
        || !sketch_marker_prefix_at(payload, offset.saturating_add(84))
    {
        return false;
    }
    let endpoint = |relative| {
        payload
            .get(offset + relative..offset + relative + 2)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
    };
    matches!((endpoint(56), endpoint(58)), (Some(first), Some(second)) if first != second)
}

pub(super) fn compact_legacy_selected_axis_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if !compact_legacy_marker_body(payload, offset)
        || marker_native_code(payload, offset) != Some(0)
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
        || payload.get(offset + 50..offset + 58) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 58..offset + 62) != Some(&0u32.to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(80)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 42)
}

fn current_vertical_axis_endpoint_indices(payload: &[u8], offset: usize) -> Option<[u32; 2]> {
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 17..offset + 21) != Some(&5u32.to_le_bytes())
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 29..offset + 31) != Some(&0u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || f64::from_le_bytes(payload.get(offset + 48..offset + 56)?.try_into().ok()?) != 1.0
        || payload.get(offset + 60..offset + 64) != Some(&0u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 76) != Some(&0u32.to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 56)
}

pub(super) fn extended_wide_horizontal_relation_endpoint_indices(
    payload: &[u8],
    offset: usize,
) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 17..offset + 21) != Some(&4u32.to_le_bytes())
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || f64::from_le_bytes(payload.get(offset + 48..offset + 56)?.try_into().ok()?) != 1.0
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 84) != Some(&u32::MAX.to_le_bytes())
        || payload
            .get(offset + 84..offset + 86)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
            .is_none_or(|state| state == 0)
        || payload.get(offset + 86..offset + 102)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 102..offset + 104) != Some(&0u16.to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(112)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 64)
}

fn extended_horizontal_axis_endpoint_indices(payload: &[u8], offset: usize) -> Option<[u32; 2]> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 17..offset + 21) != Some(&4u32.to_le_bytes())
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(2)
        || payload.get(offset + 29..offset + 31) != Some(&0u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || f64::from_le_bytes(payload.get(offset + 48..offset + 56)?.try_into().ok()?) != 1.0
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&0u32.to_le_bytes())
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 84) != Some(&0u32.to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    one_based_u16_endpoint_pair(payload, offset, 64)
}

#[cfg(test)]
mod endpoints_tests;
