// SPDX-License-Identifier: Apache-2.0
//! Exact rectangular and circular pattern constructions and their axes.

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::bytes::lp_ascii_filtered;
use crate::bytes::lp_utf16_bounded;
use crate::bytes::{f64s_at, finite_reals_at};
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::records::feature::patterns;
use crate::records::feature::patterns::DesignCircularPatternConstruction;
use crate::records::feature::patterns::DesignRectangularPatternConstruction;
use crate::records::feature::patterns::DesignRectangularPatternInstances;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::FiniteReal;

const EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8: f64 = 1.0e-8;

const EPS_SCOPES_SAME_TRANSFORM_BASIS_E10: f64 = 1.0e-10;

const EPS_SCOPES_EXACT_CIRCULAR_PATTERN_AXIS_E12: f64 = 1.0e-12;

pub(super) fn exact_rectangular_pattern_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignRectangularPatternConstruction> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::RectangularPattern) {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut lanes = parameter_owners
        .iter()
        .filter(|owner| {
            native_stream(owner.id()) == Some(stream)
                && owner.scope_record_index() == scope.record_index
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(|owner| owner.local_ordinal());
    let [u_count, v_count, u_extent, v_extent] = lanes.as_slice() else {
        return None;
    };
    if [u_count, v_count, u_extent, v_extent]
        .iter()
        .enumerate()
        .any(|(ordinal, owner)| owner.local_ordinal() != ordinal as u32)
    {
        return None;
    }
    let exact_count = |value: f64| {
        (value > 0.0 && value <= f64::from(u32::MAX) && value.fract() == 0.0)
            .then_some(value as u32)
    };
    let u_count_value = exact_count(u_count.evaluated_value().get())?;
    let v_count_value = exact_count(v_count.evaluated_value().get())?;
    let mut construction = DesignRectangularPatternConstruction::try_from(
        patterns::DesignRectangularPatternConstructionWire {
            u_count: u_count_value,
            v_count: v_count_value,
            u_extent: u_extent.evaluated_value().get(),
            v_extent: v_extent.evaluated_value().get(),
            owner_record_indices: [
                u_count.record_index(),
                v_count.record_index(),
                u_extent.record_index(),
                v_extent.record_index(),
            ],
            value_offsets: [
                u_count.evaluated_value_offset(),
                v_count.evaluated_value_offset(),
                u_extent.evaluated_value_offset(),
                v_extent.evaluated_value_offset(),
            ],
            instances: None,
        },
    )
    .ok()?;
    construction.instances =
        exact_rectangular_pattern_instances(bytes, records, scope, &construction);
    Some(construction)
}

fn exact_rectangular_pattern_instances(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    construction: &DesignRectangularPatternConstruction,
) -> Option<DesignRectangularPatternInstances> {
    let active = [
        (construction.u_count(), construction.u_extent()),
        (construction.v_count(), construction.v_extent()),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 1)
    .collect::<Vec<_>>();
    let [(count, extent)] = active.as_slice() else {
        return None;
    };
    let count = usize::try_from(*count).ok()?;
    if count > 4_096
        || scope.reference_members().len() != count.checked_add(6)?
        || !scope
            .reference_members()
            .values()
            .skip(1)
            .take(4)
            .eq(construction.owner_record_indices.iter())
    {
        return None;
    }
    let mut record_indices = Vec::with_capacity(count);
    record_indices.push(*scope.reference_members().values().next()?);
    record_indices.extend(
        scope
            .reference_members()
            .values_in(6..count.checked_add(5)?)?
            .copied(),
    );
    let reference_starts = scope
        .reference_members()
        .values()
        .map(|record_index| {
            records
                .first_at_or_after(0, *record_index)
                .map(|offset| (*record_index, offset))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut candidates = Vec::with_capacity(count);
    let mut scanned_bytes = 0_usize;
    for record_index in &record_indices {
        let start = reference_starts
            .iter()
            .find_map(|(candidate, offset)| (candidate == record_index).then_some(*offset))?;
        let end = reference_starts
            .iter()
            .filter_map(|(_, offset)| (*offset > start).then_some(*offset))
            .min()?;
        let span = end.checked_sub(start)?;
        scanned_bytes = scanned_bytes.checked_add(span)?;
        if span > 1_048_576 || scanned_bytes > 16_777_216 {
            return None;
        }
        candidates.push(exact_rigid_transform_candidates(bytes, start, end)?);
    }
    let first_candidates = candidates.first()?;
    let final_candidates = candidates.last()?;
    let mut runs = Vec::new();
    for first in first_candidates {
        for final_candidate in final_candidates {
            if !same_transform_basis(&first.0, &final_candidate.0) {
                continue;
            }
            let delta = translation_delta(&first.0, &final_candidate.0);
            let distance = cadmpeg_ir::math::Vector3::from(delta).norm();
            if (distance - extent.abs()).abs() > EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8 {
                continue;
            }
            let mut run = vec![*first];
            let mut unique = true;
            for (ordinal, record_candidates) in candidates[1..count - 1].iter().enumerate() {
                let fraction = (ordinal + 1) as f64 / (count - 1) as f64;
                let matches = record_candidates
                    .iter()
                    .filter(|candidate| {
                        same_transform_basis(&first.0, &candidate.0)
                            && translation_delta(&first.0, &candidate.0)
                                .iter()
                                .zip(delta)
                                .all(|(value, total)| {
                                    (*value - total * fraction).abs()
                                        <= EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8
                                })
                    })
                    .collect::<Vec<_>>();
                let [candidate] = matches.as_slice() else {
                    unique = false;
                    break;
                };
                run.push(**candidate);
            }
            if unique {
                run.push(*final_candidate);
                runs.push(run);
            }
        }
    }
    runs.sort_by(|a, b| {
        a.iter()
            .map(|(_, offset)| *offset)
            .cmp(b.iter().map(|(_, offset)| *offset))
    });
    runs.dedup_by(|left, right| left == right);
    let [run] = runs.as_slice() else {
        return None;
    };
    Some(DesignRectangularPatternInstances::Bodies(
        record_indices
            .into_iter()
            .zip(run)
            .map(
                |(record_index, (value, offset))| patterns::DesignPatternInstance {
                    record_index,
                    transform: crate::records::identity::Located {
                        value: *value,
                        offset: *offset,
                    },
                },
            )
            .collect(),
    ))
}

type TransformCandidate = (crate::records::sketch_placement::SketchPlacementMatrix, u64);

fn exact_rigid_transform_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Option<Vec<TransformCandidate>> {
    /// The single byte image of `1.0_f64`, the last lane of a rigid
    /// transform's fixed `0 0 0 1` bottom row.
    const ONE_F64_LE: [u8; 8] = [0, 0, 0, 0, 0, 0, 0xF0, 0x3F];
    let last_exclusive = end.checked_sub(127)?;
    if start >= last_exclusive || end > bytes.len() {
        // An exhaustive scan over this range finds nothing or aborts on its
        // first out-of-bounds sixteen-lane read.
        return None;
    }
    let mut candidates = Vec::new();
    // A valid transform carries exactly `1.0` at lane fifteen and a zero of
    // either sign at lanes twelve to fourteen. Locate the fixed `1.0` image
    // (it cannot overlap itself, so every occurrence surfaces) and read the
    // full sixteen-lane frame only at surviving offsets.
    for hit in memchr::memmem::find_iter(&bytes[start + 120..end], &ONE_F64_LE) {
        let offset = start + hit;
        let zero_lanes_valid = (0..3).all(|lane| {
            let at = offset + 96 + lane * 8;
            bytes[at..at + 7].iter().all(|byte| *byte == 0)
                && (bytes[at + 7] == 0 || bytes[at + 7] == 0x80)
        });
        if !zero_lanes_valid {
            continue;
        }
        let values = f64s_at(bytes, offset, 16)?;
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        if let Ok(transform) =
            crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
        {
            candidates.push((transform, u64::try_from(offset).ok()?));
        }
    }
    (!candidates.is_empty()).then_some(candidates)
}

fn same_transform_basis(
    left: &crate::records::sketch_placement::SketchPlacementMatrix,
    right: &crate::records::sketch_placement::SketchPlacementMatrix,
) -> bool {
    (0..3).all(|row| {
        (0..3).all(|column| {
            (left[row][column] - right[row][column]).abs() <= EPS_SCOPES_SAME_TRANSFORM_BASIS_E10
        })
    })
}

fn translation_delta(
    left: &crate::records::sketch_placement::SketchPlacementMatrix,
    right: &crate::records::sketch_placement::SketchPlacementMatrix,
) -> [f64; 3] {
    [
        right[0][3] - left[0][3],
        right[1][3] - left[1][3],
        right[2][3] - left[2][3],
    ]
}

pub(super) fn exact_circular_pattern_construction_with_owners(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
) -> Option<DesignCircularPatternConstruction> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::CircularPattern) {
        return None;
    }
    let mut axis_candidates = Vec::new();
    for (record_index, selection_record_index) in scope
        .reference_members()
        .values()
        .zip(scope.reference_members().values().skip(1))
    {
        for (start, paired_at) in records.frames(*record_index) {
            if let Some((origin, direction)) = exact_circular_pattern_axis(
                bytes,
                start,
                paired_at,
                *record_index,
                *selection_record_index,
                scope.record_index,
            ) {
                axis_candidates.push(CircularPatternAxisCandidate {
                    axis: patterns::DesignCircularPatternAxis::Inline {
                        origin,
                        origin_offset: (start + 25) as u64,
                        direction,
                        direction_offset: (start + 49) as u64,
                    },
                    axis_record_index: *record_index,
                    selection_record_index: *selection_record_index,
                });
            }
        }
    }
    for record_index in scope.reference_members().values() {
        for (start, paired_at) in records.frames(*record_index) {
            if let Some((axis, selection_record_index)) = exact_legacy_circular_pattern_axis(
                bytes,
                records,
                start,
                paired_at,
                *record_index,
                scope,
            ) {
                axis_candidates.push(CircularPatternAxisCandidate {
                    axis,
                    axis_record_index: *record_index,
                    selection_record_index,
                });
            }
        }
    }
    let CircularPatternAxisCandidate {
        axis,
        axis_record_index,
        selection_record_index,
    } = select_circular_pattern_axis(&axis_candidates)?;
    let owner_count_candidates = parameter_owners.iter().filter_map(|owner| {
        if native_stream(owner.id()) != native_stream(&scope.id)
            || owner.scope_record_index() != scope.record_index
            || owner.local_ordinal() != 0
            || owner.evaluated_value().get() <= 0.0
            || owner.evaluated_value().get() > f64::from(u32::MAX)
            || owner.evaluated_value().get().fract() != 0.0
        {
            return None;
        }
        Some((
            owner.evaluated_value().get() as u32,
            owner.record_index(),
            owner.evaluated_value_offset(),
        ))
    });
    let mut count_candidates = owner_count_candidates.collect::<Vec<_>>();
    if count_candidates.is_empty() {
        count_candidates.extend(
            scope
                .reference_members()
                .values()
                .filter_map(|record_index| {
                    exact_fixed_pattern_count(bytes, records, *record_index, scope.record_index)
                        .map(|(count, count_offset)| (count, *record_index, count_offset))
                }),
        );
    }
    count_candidates.sort_unstable();
    count_candidates.dedup();
    let [(count, count_record_index, count_offset)] = count_candidates.as_slice() else {
        return None;
    };
    let owner_angle_candidates = parameter_owners.iter().filter_map(|owner| {
        (native_stream(owner.id()) == native_stream(&scope.id)
            && owner.scope_record_index() == scope.record_index
            && owner.local_ordinal() == 1)
            .then_some(())?;
        Some((
            cadmpeg_ir::scalar::PositiveAngle::new(owner.evaluated_value().get())?,
            owner.record_index(),
            owner.evaluated_value_offset(),
        ))
    });
    let mut angle_candidates = owner_angle_candidates.collect::<Vec<_>>();
    if angle_candidates.is_empty() {
        angle_candidates.extend(
            scope
                .reference_members()
                .values()
                .filter_map(|record_index| {
                    let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
                    (scalar.owner_record_index == Some(scope.record_index) && scalar.ordinal == 1)
                        .then_some(())?;
                    Some((
                        cadmpeg_ir::scalar::PositiveAngle::new(scalar.value.get())?,
                        *record_index,
                        scalar.value_offset,
                    ))
                }),
        );
    }
    angle_candidates.sort_by(|left, right| {
        left.0
            .get()
            .total_cmp(&right.0.get())
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    angle_candidates.dedup();
    let [(angle, angle_record_index, angle_offset)] = angle_candidates.as_slice() else {
        return None;
    };
    Some(DesignCircularPatternConstruction {
        count: *count,
        count_record_index: *count_record_index,
        count_offset: *count_offset,
        angle: *angle,
        angle_record_index: *angle_record_index,
        angle_offset: *angle_offset,
        axis: axis.clone(),
        axis_record_index: *axis_record_index,
        selection_record_index: *selection_record_index,
    })
}

/// Circular pattern axis with its carrier and selection record indices.
struct CircularPatternAxisCandidate {
    axis: patterns::DesignCircularPatternAxis,
    axis_record_index: u32,
    selection_record_index: u32,
}

/// Select one circular-pattern axis, preferring the explicit solved carrier.
fn select_circular_pattern_axis(
    candidates: &[CircularPatternAxisCandidate],
) -> Option<&CircularPatternAxisCandidate> {
    let inline = candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.axis,
                patterns::DesignCircularPatternAxis::Inline { .. }
            )
        })
        .collect::<Vec<_>>();
    match inline.as_slice() {
        [candidate] => Some(*candidate),
        [] => match candidates {
            [candidate] => Some(candidate),
            _ => None,
        },
        _ => None,
    }
}

fn exact_legacy_circular_pattern_axis(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    start: usize,
    paired_at: usize,
    record_index: u32,
    scope: &DesignParameterScope,
) -> Option<(patterns::DesignCircularPatternAxis, u32)> {
    use patterns::DesignCircularPatternAxis;

    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != start + 7
        || View::u32_le_at(bytes, after_tag) != Some(record_index)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
    {
        return None;
    }
    let (identity_offsets, selection_at, second_count_at, second_identity_at, scope_at, tail_at) =
        match (
            paired_at.checked_sub(start),
            View::u32_le_at(bytes, start + 21),
        ) {
            (Some(129), Some(1)) => (
                vec![start + 26, start + 56],
                start + 40,
                start + 51,
                start + 55,
                start + 66,
                start + 77,
            ),
            (Some(118), Some(0)) => (
                vec![start + 45],
                start + 29,
                start + 40,
                start + 44,
                start + 55,
                start + 66,
            ),
            _ => return None,
        };
    let first_identity_at = identity_offsets.first().copied()?.checked_sub(1)?;
    if (identity_offsets.len() == 2
        && (marked_record_reference(bytes, first_identity_at).is_none()
            || bytes.get(first_identity_at + 5..first_identity_at + 11) != Some(&[0; 6])
            || View::u32_le_at(bytes, start + 36) != Some(1)))
        || View::u32_le_at(bytes, second_count_at) != Some(1)
        || marked_record_reference(bytes, second_identity_at).is_none()
        || bytes.get(second_identity_at + 5..second_identity_at + 11) != Some(&[0; 6])
        || marked_record_reference(bytes, scope_at) != Some(scope.record_index)
        || bytes.get(scope_at + 5..scope_at + 11) != Some(&[0; 6])
    {
        return None;
    }
    let selection_record_index = marked_record_reference(bytes, selection_at)?;
    if !scope
        .reference_members()
        .values()
        .any(|value| value == &selection_record_index)
        || bytes.get(selection_at + 5..selection_at + 11) != Some(&[0; 6])
    {
        return None;
    }
    let opaque_index = View::u32_le_at(bytes, tail_at)?;
    if opaque_index == 0
        || !View::f64_le_at(bytes, tail_at + 4)?.is_finite()
        || View::u32_le_at(bytes, tail_at + 12) != Some(opaque_index)
        || marked_record_reference(bytes, tail_at + 16) != record_index.checked_add(2)
        || bytes.get(tail_at + 21..tail_at + 27) != Some(&[0; 6])
        || bytes.get(tail_at + 27..tail_at + 29) != Some(&[0; 2])
        || marked_record_reference(bytes, tail_at + 29) != record_index.checked_add(1)
        || bytes.get(tail_at + 34..tail_at + 40) != Some(&[0; 6])
        || bytes.get(tail_at + 40) != Some(&0)
        || marked_record_reference(bytes, tail_at + 41) != Some(scope.record_index)
        || bytes.get(tail_at + 46..tail_at + 52) != Some(&[0; 6])
    {
        return None;
    }
    let (paired_class_tag, paired_after_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    if paired_class_tag.len() != 3
        || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || paired_after_tag != paired_at + 7
        || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
    {
        return None;
    }
    let wrappers = identity_offsets
        .iter()
        .map(|offset| {
            let record_index = View::u32_le_at(bytes, *offset)?;
            let (identity, identity_offset) =
                exact_pattern_identity_wrapper(bytes, records, record_index)?;
            Some((
                identity,
                patterns::DesignPatternAxisWrapper {
                    record_index,
                    identity_offset,
                },
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let persistent_identity = wrappers.first()?.0;
    if wrappers
        .iter()
        .any(|(identity, _)| *identity != persistent_identity)
    {
        return None;
    }
    Some((
        DesignCircularPatternAxis::HistoricalEdge {
            wrappers: wrappers.into_iter().map(|(_, wrapper)| wrapper).collect(),
            persistent_identity,
            resolved: None,
        },
        selection_record_index,
    ))
}

fn exact_pattern_identity_wrapper(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<(u64, u64)> {
    let [start] = records.offsets(record_index) else {
        return None;
    };
    let start = *start;
    let (_, after_tag) = lp_ascii_filtered(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if after_tag != start + 7
        || View::u32_le_at(bytes, after_tag) != Some(record_index)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        || View::u64_le_at(bytes, start + 21)? == 0
    {
        return None;
    }
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 29, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !crate::bytes::is_guid_relaxed(&asset_id)
        || !crate::bytes::is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id) != Some(2)
        || bytes.get(after_context_id + 4..after_context_id + 8) != Some(&[0; 4])
        || marked_record_reference(bytes, after_context_id + 8) != record_index.checked_add(1)
        || bytes.get(after_context_id + 13..after_context_id + 19) != Some(&[0; 6])
    {
        return None;
    }
    let nested_one_at = next_indexed_record_offset(bytes, after_context_id + 19)?;
    let (_, nested_one_tag) = lp_ascii_filtered(bytes, nested_one_at, 3..=3, u8::is_ascii_digit)?;
    if View::u32_le_at(bytes, nested_one_tag) != record_index.checked_add(1)
        || bytes.get(nested_one_at + 11..nested_one_at + 21) != Some(&[0; 10])
        || marked_record_reference(bytes, nested_one_at + 21) != record_index.checked_add(2)
        || bytes.get(nested_one_at + 26..nested_one_at + 32) != Some(&[0; 6])
    {
        return None;
    }
    let identity_at = next_indexed_record_offset(bytes, nested_one_at + 32)?;
    let (_, identity_tag) = lp_ascii_filtered(bytes, identity_at, 3..=3, u8::is_ascii_digit)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + 29)?;
    let (_, next_tag) = lp_ascii_filtered(bytes, next_at, 3..=3, u8::is_ascii_digit)?;
    if View::u32_le_at(bytes, identity_tag) != record_index.checked_add(2)
        || bytes.get(identity_at + 11..identity_at + 21) != Some(&[0; 10])
        || identity_at.checked_add(29) != Some(next_at)
        || View::u32_le_at(bytes, next_tag) != record_index.checked_add(3)
    {
        return None;
    }
    Some((
        View::u64_le_at(bytes, identity_at + 21)?,
        u64::try_from(identity_at + 21).ok()?,
    ))
}

fn exact_circular_pattern_axis(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    record_index: u32,
    selection_record_index: u32,
    scope_record_index: u32,
) -> Option<([FiniteReal; 3], [FiniteReal; 3])> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != start + 7
        || View::u32_le_at(bytes, after_tag) != Some(record_index)
        || paired_at.checked_sub(start) != Some(195)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        || View::u32_le_at(bytes, start + 21) != Some(8)
        || bytes.get(start + 73..start + 89) != Some(&[0; 16])
        || View::u32_le_at(bytes, start + 89)? == 0
        || View::u32_le_at(bytes, start + 93) != Some(1)
        || marked_record_reference(bytes, start + 97) != Some(selection_record_index)
        || bytes.get(start + 102..start + 108) != Some(&[0; 6])
        || bytes.get(start + 108..start + 110) != Some(&[0; 2])
        || View::u32_le_at(bytes, start + 110) != Some(1)
        || marked_record_reference(bytes, start + 114).is_none()
        || bytes.get(start + 119..start + 125) != Some(&[0; 6])
        || View::u64_le_at(bytes, start + 125) != Some(0x0000_0004_0000_0000)
        || bytes.get(start + 133..start + 143) != Some(&[0; 10])
    {
        return None;
    }
    let opaque_index = View::u32_le_at(bytes, start + 143)?;
    if opaque_index == 0
        || !View::f64_le_at(bytes, start + 147)?.is_finite()
        || View::u32_le_at(bytes, start + 155) != Some(opaque_index)
        || marked_record_reference(bytes, start + 159) != record_index.checked_add(2)
        || bytes.get(start + 164..start + 172) != Some(&[0; 8])
        || marked_record_reference(bytes, start + 172) != record_index.checked_add(1)
        || bytes.get(start + 177..start + 184) != Some(&[0; 7])
        || marked_record_reference(bytes, start + 184) != Some(scope_record_index)
        || bytes.get(start + 189..start + 195) != Some(&[0; 6])
    {
        return None;
    }
    let (paired_class_tag, paired_after_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    if paired_class_tag.len() != 3
        || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || paired_after_tag != paired_at + 7
        || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
    {
        return None;
    }
    let origin = finite_reals_at(bytes, start + 25)?;
    let displacement: [FiniteReal; 3] = finite_reals_at(bytes, start + 49)?;
    let displacement_length = displacement[0]
        .get()
        .hypot(displacement[1].get())
        .hypot(displacement[2].get());
    if !displacement_length.is_finite() || displacement_length <= f64::EPSILON {
        return None;
    }
    if (displacement_length - 1.0).abs() <= EPS_SCOPES_EXACT_CIRCULAR_PATTERN_AXIS_E12 {
        return Some((origin, displacement));
    }
    let mut direction = [FiniteReal::ZERO; 3];
    for (slot, component) in direction.iter_mut().zip(displacement) {
        *slot = FiniteReal::new(component.get() / displacement_length)?;
    }
    Some((origin, direction))
}

fn exact_fixed_pattern_count(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<(u32, u64)> {
    let candidates = records
        .frames(record_index)
        .filter_map(|(start, paired_at)| {
            let (class_tag, after_tag) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
            if class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || after_tag != start + 7
                || View::u32_le_at(bytes, after_tag) != Some(record_index)
                || paired_at.checked_sub(start) != Some(99)
                || bytes.get(start + 11..start + 19) != Some(&[0; 8])
                || bytes.get(start + 19) != Some(&1)
                || View::u32_le_at(bytes, start + 20) != Some(1)
                || marked_record_reference(bytes, start + 24) != Some(scope_record_index)
                || bytes.get(start + 29..start + 40) != Some(&[0; 11])
                || marked_record_reference(bytes, start + 44) != record_index.checked_add(2)
                || bytes.get(start + 49..start + 55) != Some(&[0; 6])
                || View::u32_le_at(bytes, start + 55)? == 0
                || bytes.get(start + 59..start + 63) != Some(&[0; 4])
                || marked_record_reference(bytes, start + 63) != Some(scope_record_index)
                || bytes.get(start + 68..start + 76) != Some(&[0; 8])
                || marked_record_reference(bytes, start + 76) != record_index.checked_add(1)
                || bytes.get(start + 81..start + 88) != Some(&[0; 7])
                || marked_record_reference(bytes, start + 88) != Some(scope_record_index)
                || bytes.get(start + 93..start + 99) != Some(&[0; 6])
            {
                return None;
            }
            let count = View::u32_le_at(bytes, start + 40)?;
            (count > 0).then_some((count, (start + 40) as u64))
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

#[cfg(test)]
mod tests;
