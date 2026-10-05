// SPDX-License-Identifier: Apache-2.0
//! Exact rectangular and circular pattern constructions and their axes.

use cadmpeg_core::convert::{f64_from_index, truncate_f64_to_u32};
use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::find_frame;
use super::shared_frames::marked_record_reference;
use crate::bytes::{f64s_at, finite_reals_at};
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::operands::reference_at;
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::reference_runs::{admit_reference_values, reference_position};
use crate::design::decode::sketch::{
    indexed_record_header_at, next_indexed_record_header, IndexedRecordOffsets,
};
use crate::design::decode::text::relaxed_guid_end;
use crate::records::feature::patterns;
use crate::records::feature::patterns::DesignCircularPatternConstruction;
use crate::records::feature::patterns::DesignRectangularPatternConstruction;
use crate::records::feature::patterns::DesignRectangularPatternInstances;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::identity::ReferenceRun;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

const EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8: f64 = 1.0e-8;

const EPS_SCOPES_SAME_TRANSFORM_BASIS_E10: f64 = 1.0e-10;

/// Whether record `id` has the stream scope `stream`; an absent scope
/// matches only an absent one.
fn has_stream(ctx: &DecodeContext<'_>, id: &str, stream: Option<&str>) -> Result<bool, CodecError> {
    match stream {
        Some(stream) => in_stream(ctx, id, stream),
        None => Ok(record_stream(ctx, id)?.is_none()),
    }
}

/// Keep the first candidate; `false` when a later one differs from it.
fn agrees<T: PartialEq>(agreed: &mut Option<T>, candidate: T) -> bool {
    match agreed {
        Some(value) => *value == candidate,
        None => {
            *agreed = Some(candidate);
            true
        }
    }
}

pub(super) fn exact_rectangular_pattern_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignRectangularPatternConstruction>, CodecError> {
    if !matches!(
        scope.payload(),
        DesignScopePayload::RPattern(_) | DesignScopePayload::RectangularPattern(_)
    ) {
        return Ok(None);
    }
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(None);
    };
    let Some([u_count, v_count, u_extent, v_extent]) =
        rectangular_pattern_lanes(ctx, scope, stream, parameter_owners)?
    else {
        return Ok(None);
    };
    let exact_count = |value: f64| {
        (value > 0.0 && value <= f64::from(u32::MAX) && value.fract() == 0.0)
            .then(|| truncate_f64_to_u32(value))
            .flatten()
    };
    let (Some(u_count_value), Some(v_count_value)) =
        (exact_count(u_count.value), exact_count(v_count.value))
    else {
        return Ok(None);
    };
    let Ok(mut construction) = DesignRectangularPatternConstruction::try_from(
        patterns::DesignRectangularPatternConstructionWire {
            u_count: u_count_value,
            v_count: v_count_value,
            u_extent: u_extent.value,
            v_extent: v_extent.value,
            owner_record_indices: [
                u_count.record_index,
                v_count.record_index,
                u_extent.record_index,
                v_extent.record_index,
            ],
            value_offsets: [
                u_count.value_offset,
                v_count.value_offset,
                u_extent.value_offset,
                v_extent.value_offset,
            ],
            instances: None,
        },
    ) else {
        return Ok(None);
    };
    let instances = exact_rectangular_pattern_instances(ctx, bytes, records, scope, &construction)?;
    construction
        .try_set_instances(instances)
        .map_err(CodecError::malformed)?;
    Ok(Some(construction))
}

/// The record index, evaluated value and value offset of one pattern owner.
#[derive(Clone, Copy)]
struct PatternLane {
    record_index: u32,
    value: f64,
    value_offset: u64,
}

/// The scope's parameter owners in `stream`, one for each local ordinal from
/// zero to three. The search stops at the first owner whose ordinal repeats or
/// falls outside that range.
fn rectangular_pattern_lanes(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    stream: &str,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<[PatternLane; 4]>, CodecError> {
    let mut lanes = [None; 4];
    let rejected = ctx.any_by(
        parameter_owners,
        |owner| {
            if owner.scope_record_index() != scope.record_index
                || !in_stream(ctx, owner.id(), stream)?
            {
                return Ok(false);
            }
            let Some(slot) = usize::try_from(owner.local_ordinal())
                .ok()
                .and_then(|ordinal| lanes.get_mut(ordinal))
            else {
                return Ok(true);
            };
            let lane = PatternLane {
                record_index: owner.record_index(),
                value: owner.evaluated_value().get(),
                value_offset: owner.evaluated_value_offset(),
            };
            Ok(slot.replace(lane).is_some())
        },
        "scan F3D rectangular pattern owners",
    )?;
    let [Some(u_count), Some(v_count), Some(u_extent), Some(v_extent)] = lanes else {
        return Ok(None);
    };
    Ok((!rejected).then_some([u_count, v_count, u_extent, v_extent]))
}

/// The instance record of the `ordinal`th pattern instance: the scope's first
/// member, then the members after the four owner lanes and the fifth member.
fn pattern_record_index(members: &ReferenceRun<u32>, ordinal: usize) -> Option<u32> {
    match ordinal {
        0 => reference_at(members, 0),
        _ => reference_at(members, ordinal.checked_add(5)?),
    }
}

fn exact_rectangular_pattern_instances(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    construction: &DesignRectangularPatternConstruction,
) -> Result<Option<DesignRectangularPatternInstances>, CodecError> {
    let mut active = [
        (construction.u_count(), construction.u_extent()),
        (construction.v_count(), construction.v_extent()),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 1);
    let (Some((count, extent)), None) = (active.next(), active.next()) else {
        return Ok(None);
    };
    let Ok(count) = usize::try_from(count) else {
        return Ok(None);
    };
    if count > 4_096 {
        return Err(ctx.refuse_codec_limit(
            "F3D rectangular pattern search count",
            4_096,
            u64_from_index(count),
        ));
    }
    let members = scope.reference_members();
    if Some(members.len()) != count.checked_add(6)
        || !members
            .values()
            .skip(1)
            .take(4)
            .eq(construction.owner_record_indices.iter())
    {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "f3d rectangular pattern search")?;
    let Some(run) = storage
        .with_storage(|| rectangular_pattern_run(ctx, bytes, records, members, count, extent))?
    else {
        return Ok(None);
    };
    let mut instances = ctx.vector_storage(count, "f3d rectangular pattern instances")?;
    for (ordinal, (value, offset)) in ctx
        .admit_iter(&run, "scan F3D rectangular pattern instance transforms")?
        .enumerate()
    {
        let Some(record_index) = pattern_record_index(members, ordinal) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut instances,
            patterns::DesignPatternInstance {
                record_index,
                transform: crate::records::identity::Located {
                    value: *value,
                    offset: *offset,
                },
            },
            "f3d rectangular pattern instances",
        )?;
    }
    Ok(Some(DesignRectangularPatternInstances::Bodies(instances)))
}

type TransformCandidate = (crate::records::sketch_placement::SketchPlacementMatrix, u64);

/// The one run of rigid transforms, one in each instance record's span, that
/// steps evenly from the first instance to the last across `extent`. A span
/// runs from its record's first header to the nearest following first header
/// of any scope member.
fn rectangular_pattern_run(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    members: &ReferenceRun<u32>,
    count: usize,
    extent: f64,
) -> Result<Option<Vec<TransformCandidate>>, CodecError> {
    let mut starts =
        ctx.vector_storage(members.len(), "f3d rectangular pattern reference starts")?;
    // The scan stops at the first member without a header.
    let unindexed = reference_position(
        ctx,
        members,
        |record_index| {
            let Some(start) = records.first_offset(*record_index) else {
                return Ok(true);
            };
            ctx.push_vec(
                &mut starts,
                start,
                "f3d rectangular pattern reference starts",
            )?;
            Ok(false)
        },
        "scan F3D rectangular pattern scope references",
    )?;
    if unindexed.is_some() {
        return Ok(None);
    }
    ctx.sort_unstable_by(
        &mut starts,
        |start| start,
        Ord::cmp,
        "sort F3D rectangular pattern reference starts",
    )?;

    let mut candidates = ctx.vector_storage(count, "f3d rectangular pattern candidate groups")?;
    let mut scanned_bytes = 0_usize;
    for ordinal in 0..count {
        ctx.charge_work(1, "scan F3D rectangular pattern record indexes")?;
        let Some(start) =
            pattern_record_index(members, ordinal).and_then(|index| records.first_offset(index))
        else {
            return Ok(None);
        };
        let next = ctx.partition_point(
            &starts,
            |offset| Ok(*offset <= start),
            "find F3D rectangular pattern record end",
        )?;
        let Some(&end) = starts.get(next) else {
            return Ok(None);
        };
        let span = end - start;
        let Some(total) = scanned_bytes.checked_add(span) else {
            return Err(ctx.refuse_codec_limit(
                "F3D rectangular pattern aggregate span",
                16_777_216,
                u64::MAX,
            ));
        };
        scanned_bytes = total;
        if span > 1_048_576 {
            return Err(ctx.refuse_codec_limit(
                "F3D rectangular pattern record span",
                1_048_576,
                u64_from_index(span),
            ));
        }
        if scanned_bytes > 16_777_216 {
            return Err(ctx.refuse_codec_limit(
                "F3D rectangular pattern aggregate span",
                16_777_216,
                u64_from_index(scanned_bytes),
            ));
        }
        let Some(record_candidates) = exact_rigid_transform_candidates(ctx, bytes, start, end)?
        else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut candidates,
            record_candidates,
            "f3d rectangular pattern candidate groups",
        )?;
    }
    let (Some(first_candidates), Some(final_candidates), Some(intermediates)) = (
        candidates.first(),
        candidates.last(),
        candidates.get(1..count - 1),
    ) else {
        return Ok(None);
    };
    // Distinct endpoint pairs give distinct runs, so the run is unique when
    // exactly one pair has evenly spaced intermediates. Each pair is admitted
    // as the search reaches it; the search stops at a second run.
    let mut selected = None;
    for first in first_candidates {
        ctx.charge_work(1, "scan F3D rectangular pattern first candidates")?;
        for last in final_candidates {
            ctx.charge_work(1, "scan F3D rectangular pattern final candidates")?;
            if !same_transform_basis(&first.0, &last.0) {
                continue;
            }
            let delta = translation_delta(&first.0, &last.0);
            let distance = cadmpeg_ir::math::Vector3::from(delta).norm();
            if (distance - extent.abs()).abs() > EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8
                || !rectangular_pattern_steps(ctx, first, delta, intermediates, |_| Ok(()))?
            {
                continue;
            }
            if selected.replace((first, last, delta)).is_some() {
                return Ok(None);
            }
        }
    }
    let Some((first, last, delta)) = selected else {
        return Ok(None);
    };
    let mut run = ctx.vector_storage(count, "f3d rectangular pattern candidate run")?;
    ctx.push_vec(&mut run, *first, "f3d rectangular pattern candidate run")?;
    let complete = rectangular_pattern_steps(ctx, first, delta, intermediates, |candidate| {
        ctx.push_vec(
            &mut run,
            *candidate,
            "f3d rectangular pattern candidate run",
        )
    })?;
    if !complete {
        return Ok(None);
    }
    ctx.push_vec(&mut run, *last, "f3d rectangular pattern candidate run")?;
    Ok(Some(run))
}

/// Whether every intermediate record holds exactly one transform with the
/// first instance's basis, translated by its even fraction of `delta`. Each
/// matched transform is passed to `step` in order. The check stops at the
/// first record without such a transform.
fn rectangular_pattern_steps(
    ctx: &DecodeContext<'_>,
    first: &TransformCandidate,
    delta: [f64; 3],
    intermediates: &[Vec<TransformCandidate>],
    mut step: impl FnMut(&TransformCandidate) -> Result<(), CodecError>,
) -> Result<bool, CodecError> {
    let Some(intervals) = f64_from_index(intermediates.len() + 1) else {
        return Ok(false);
    };
    let mut position = 0.0;
    ctx.all_by(
        intermediates,
        |record_candidates| {
            position += 1.0;
            let fraction = position / intervals;
            let mut matched = None;
            for candidate in record_candidates {
                ctx.charge_work(1, "scan F3D rectangular pattern intermediate candidates")?;
                let evenly_spaced = same_transform_basis(&first.0, &candidate.0)
                    && translation_delta(&first.0, &candidate.0)
                        .iter()
                        .zip(delta)
                        .all(|(value, total)| {
                            (*value - total * fraction).abs()
                                <= EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8
                        });
                if evenly_spaced && matched.replace(candidate).is_some() {
                    return Ok(false);
                }
            }
            let Some(candidate) = matched else {
                return Ok(false);
            };
            step(candidate)?;
            Ok(true)
        },
        "scan F3D rectangular pattern intermediate records",
    )
}

/// Every rigid transform frame between `start` and `end`.
fn exact_rigid_transform_candidates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<Vec<TransformCandidate>>, CodecError> {
    /// The single byte image of `1.0_f64`, the last lane of a rigid
    /// transform's fixed `0 0 0 1` bottom row.
    const ONE_F64_LE: [u8; 8] = [0, 0, 0, 0, 0, 0, 0xF0, 0x3F];
    let Some(last_exclusive) = end.checked_sub(127) else {
        return Ok(None);
    };
    if start >= last_exclusive || end > bytes.len() {
        // An exhaustive scan over this range finds nothing or aborts on its
        // first out-of-bounds sixteen-lane read.
        return Ok(None);
    }
    let mut candidates = Vec::new();
    // A valid transform carries exactly `1.0` at lane fifteen and a zero of
    // either sign at lanes twelve to fourteen. Locate the fixed `1.0` image
    // (it cannot overlap itself, so every occurrence surfaces) and read the
    // full sixteen-lane frame only at surviving offsets.
    for hit in ctx.find_bytes_iter(
        &bytes[start + 120..end],
        &ONE_F64_LE,
        "scan F3D rectangular pattern matrix marker bytes",
    )? {
        let offset = start + hit;
        let zero_lanes_valid = (0..3).all(|lane| {
            let at = offset + 96 + lane * 8;
            zeros_at::<7>(bytes, at) && matches!(bytes.get(at + 7), Some(0 | 0x80))
        });
        if !zero_lanes_valid {
            continue;
        }
        let Some(values) = f64s_at::<16>(bytes, offset) else {
            return Ok(None);
        };
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        if let Ok(transform) =
            crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
        {
            ctx.push_vec(
                &mut candidates,
                (transform, u64_from_index(offset)),
                "f3d rectangular pattern transform candidates",
            )?;
        }
    }
    Ok((!candidates.is_empty()).then_some(candidates))
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignCircularPatternConstruction>, CodecError> {
    if !matches!(
        scope.payload(),
        DesignScopePayload::CPattern(_)
            | DesignScopePayload::CircularPattern(_)
            | DesignScopePayload::ReseauC(_)
    ) {
        return Ok(None);
    }
    let Some(axis) = circular_pattern_axis(ctx, bytes, records, scope)? else {
        return Ok(None);
    };
    let stream = record_stream(ctx, &scope.id)?;
    let Some((count, count_record_index, count_offset)) =
        circular_pattern_count(ctx, bytes, records, scope, stream, parameter_owners)?
    else {
        return Ok(None);
    };
    let Some((angle, angle_record_index, angle_offset)) =
        circular_pattern_angle(ctx, bytes, records, scope, stream, parameter_owners)?
    else {
        return Ok(None);
    };
    let Some(count) = std::num::NonZeroU32::new(count) else {
        return Ok(None);
    };
    let CircularPatternAxisCandidate {
        form,
        axis_record_index,
        selection_record_index,
    } = axis;
    let Some(axis) = circular_pattern_axis_value(ctx, form)? else {
        return Ok(None);
    };
    Ok(Some(DesignCircularPatternConstruction {
        count,
        count_record_index,
        count_offset,
        angle,
        angle_record_index,
        angle_offset,
        axis,
        axis_record_index,
        selection_record_index,
    }))
}

/// A historical-edge axis validated in place: its one or two wrappers and the
/// persistent identity they share.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HistoricalPatternAxis {
    first: patterns::DesignPatternAxisWrapper,
    second: Option<patterns::DesignPatternAxisWrapper>,
    persistent_identity: u64,
}

/// A circular-pattern axis read from one carrier frame.
#[derive(Debug)]
enum CircularPatternAxisForm {
    /// Axis coordinates stored in the carrier.
    Inline(patterns::DesignCircularPatternAxis),
    /// Wrappers of one persistent historical edge, not yet copied.
    Historical(HistoricalPatternAxis),
}

/// Circular pattern axis with its carrier and selection record indices.
#[derive(Debug)]
struct CircularPatternAxisCandidate {
    form: CircularPatternAxisForm,
    axis_record_index: u32,
    selection_record_index: u32,
}

/// The axis selection over every candidate a scope offers: its one inline
/// carrier, or without inline carriers its one historical edge.
#[derive(Debug, Default)]
struct AxisSelection {
    inline: Option<CircularPatternAxisCandidate>,
    several_inline: bool,
    historical: Option<CircularPatternAxisCandidate>,
    several_historical: bool,
}

impl AxisSelection {
    fn offer(&mut self, candidate: CircularPatternAxisCandidate) {
        let (slot, several) = match candidate.form {
            CircularPatternAxisForm::Inline(_) => (&mut self.inline, &mut self.several_inline),
            CircularPatternAxisForm::Historical(_) => {
                (&mut self.historical, &mut self.several_historical)
            }
        };
        if slot.replace(candidate).is_some() {
            *several = true;
        }
    }

    fn selected(self) -> Option<CircularPatternAxisCandidate> {
        match self {
            Self {
                inline: Some(candidate),
                several_inline: false,
                ..
            }
            | Self {
                inline: None,
                historical: Some(candidate),
                several_historical: false,
                ..
            } => Some(candidate),
            _ => None,
        }
    }
}

/// The selected axis among the inline carriers that each reference member
/// names with the member after it as selection, and the historical edges of
/// every member.
fn circular_pattern_axis(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<CircularPatternAxisCandidate>, CodecError> {
    let mut selection = AxisSelection::default();
    let mut previous = None;
    for selection_record_index in admit_reference_values(
        ctx,
        scope.reference_members(),
        "scan F3D circular pattern axis references",
    )?
    .copied()
    {
        let Some(axis_record_index) = previous.replace(selection_record_index) else {
            continue;
        };
        for (start, paired_at) in records.frames(ctx, axis_record_index)? {
            if let Some(axis) = circular_pattern_axis_at(
                bytes,
                start,
                paired_at,
                axis_record_index,
                selection_record_index,
                scope.record_index,
            ) {
                selection.offer(CircularPatternAxisCandidate {
                    form: CircularPatternAxisForm::Inline(axis),
                    axis_record_index,
                    selection_record_index,
                });
            }
        }
    }
    for record_index in admit_reference_values(
        ctx,
        scope.reference_members(),
        "scan F3D circular pattern legacy axis references",
    )?
    .copied()
    {
        for (start, paired_at) in records.frames(ctx, record_index)? {
            if let Some((axis, selection_record_index)) = exact_legacy_circular_pattern_axis(
                ctx,
                bytes,
                records,
                start,
                paired_at,
                record_index,
                scope,
            )? {
                selection.offer(CircularPatternAxisCandidate {
                    form: CircularPatternAxisForm::Historical(axis),
                    axis_record_index: record_index,
                    selection_record_index,
                });
            }
        }
    }
    Ok(selection.selected())
}

/// The axis of a selected candidate; a historical edge's wrappers are copied
/// into retained storage.
fn circular_pattern_axis_value(
    ctx: &DecodeContext<'_>,
    form: CircularPatternAxisForm,
) -> Result<Option<patterns::DesignCircularPatternAxis>, CodecError> {
    let axis = match form {
        CircularPatternAxisForm::Inline(axis) => return Ok(Some(axis)),
        CircularPatternAxisForm::Historical(axis) => axis,
    };
    let operation = "f3d circular pattern historical axis wrappers";
    let wrappers = match axis.second {
        Some(second) => ctx.copy_slice(&[axis.first, second], operation)?,
        None => ctx.copy_slice(&[axis.first], operation)?,
    };
    Ok(wrappers.try_into().ok().map(|wrappers| {
        patterns::DesignCircularPatternAxis::HistoricalEdge {
            wrappers,
            persistent_identity: axis.persistent_identity,
            resolved: None,
        }
    }))
}

/// The pattern count every count owner of the scope agrees on, or without
/// owners the count every member's fixed count frame agrees on. Each search
/// stops at the first disagreement.
fn circular_pattern_count(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream: Option<&str>,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<(u32, u32, u64)>, CodecError> {
    let mut agreed = None;
    let conflict = ctx.any_by(
        parameter_owners,
        |owner| {
            let value = owner.evaluated_value().get();
            if owner.scope_record_index() != scope.record_index
                || owner.local_ordinal() != 0
                || value <= 0.0
                || value > f64::from(u32::MAX)
                || value.fract() != 0.0
            {
                return Ok(false);
            }
            let Some(count) = truncate_f64_to_u32(value) else {
                return Ok(false);
            };
            if !has_stream(ctx, owner.id(), stream)? {
                return Ok(false);
            }
            let candidate = (count, owner.record_index(), owner.evaluated_value_offset());
            Ok(!agrees(&mut agreed, candidate))
        },
        "scan F3D circular pattern count owners",
    )?;
    if conflict {
        return Ok(None);
    }
    if agreed.is_some() {
        return Ok(agreed);
    }
    let conflict = reference_position(
        ctx,
        scope.reference_members(),
        |record_index| {
            let Some((count, count_offset)) =
                exact_fixed_pattern_count(ctx, bytes, records, *record_index, scope.record_index)?
            else {
                return Ok(false);
            };
            Ok(!agrees(&mut agreed, (count, *record_index, count_offset)))
        },
        "scan F3D circular pattern count references",
    )?;
    Ok(agreed.filter(|_| conflict.is_none()))
}

/// The pattern angle every angle owner of the scope agrees on, or without
/// owners the angle every member's fixed scalar frame agrees on. Each search
/// stops at the first disagreement.
fn circular_pattern_angle(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream: Option<&str>,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<(cadmpeg_ir::scalar::PositiveAngle, u32, u64)>, CodecError> {
    let mut agreed = None;
    let conflict = ctx.any_by(
        parameter_owners,
        |owner| {
            if owner.scope_record_index() != scope.record_index || owner.local_ordinal() != 1 {
                return Ok(false);
            }
            let Some(angle) = cadmpeg_ir::scalar::PositiveAngle::new(owner.evaluated_value().get())
            else {
                return Ok(false);
            };
            if !has_stream(ctx, owner.id(), stream)? {
                return Ok(false);
            }
            let candidate = (angle, owner.record_index(), owner.evaluated_value_offset());
            Ok(!agrees(&mut agreed, candidate))
        },
        "scan F3D circular pattern angle owners",
    )?;
    if conflict {
        return Ok(None);
    }
    if agreed.is_some() {
        return Ok(agreed);
    }
    let conflict = reference_position(
        ctx,
        scope.reference_members(),
        |record_index| {
            let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, *record_index)? else {
                return Ok(false);
            };
            if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 1 {
                return Ok(false);
            }
            let Some(angle) = cadmpeg_ir::scalar::PositiveAngle::new(scalar.value.get()) else {
                return Ok(false);
            };
            Ok(!agrees(
                &mut agreed,
                (angle, *record_index, scalar.value_offset),
            ))
        },
        "scan F3D circular pattern angle references",
    )?;
    Ok(agreed.filter(|_| conflict.is_none()))
}

/// The historical-edge axis of the legacy carrier frame of `record_index`
/// from `start` to `paired_at`, with its selection record index.
fn exact_legacy_circular_pattern_axis(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    start: usize,
    paired_at: usize,
    record_index: u32,
    scope: &DesignParameterScope,
) -> Result<Option<(HistoricalPatternAxis, u32)>, CodecError> {
    let Some((wrapper_offsets, selection_record_index)) =
        legacy_circular_pattern_axis_layout(bytes, start, paired_at, record_index, scope)
    else {
        return Ok(None);
    };
    if reference_position(
        ctx,
        scope.reference_members(),
        |value| Ok(*value == selection_record_index),
        "find F3D circular pattern selection reference",
    )?
    .is_none()
    {
        return Ok(None);
    }
    let [Some(first_at), second_at] = wrapper_offsets else {
        return Ok(None);
    };
    let Some(first) = pattern_axis_wrapper(ctx, bytes, records, first_at)? else {
        return Ok(None);
    };
    let second = match second_at {
        Some(second_at) => match pattern_axis_wrapper(ctx, bytes, records, second_at)? {
            Some(second) if second.0 == first.0 => Some(second.1),
            _ => return Ok(None),
        },
        None => None,
    };
    Ok(Some((
        HistoricalPatternAxis {
            first: first.1,
            second,
            persistent_identity: first.0,
        },
        selection_record_index,
    )))
}

/// The persistent identity and wrapper named by the record index at `at`.
fn pattern_axis_wrapper(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    at: usize,
) -> Result<Option<(u64, patterns::DesignPatternAxisWrapper)>, CodecError> {
    let Some(record_index) = View::u32_le_at(bytes, at) else {
        return Ok(None);
    };
    Ok(
        exact_pattern_identity_wrapper(ctx, bytes, records, record_index)?.map(
            |(identity, identity_offset)| {
                (
                    identity,
                    patterns::DesignPatternAxisWrapper {
                        record_index,
                        identity_offset,
                    },
                )
            },
        ),
    )
}

/// The fixed fields of a legacy circular-pattern carrier frame: the offsets of
/// its one or two wrapper record indexes and its selection record index.
fn legacy_circular_pattern_axis_layout(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    record_index: u32,
    scope: &DesignParameterScope,
) -> Option<([Option<usize>; 2], u32)> {
    if !zeros_at::<10>(bytes, start + 11) {
        return None;
    }
    let (wrapper_offsets, selection_at, second_count_at, second_identity_at, scope_at, tail_at) =
        match (
            paired_at.checked_sub(start),
            View::u32_le_at(bytes, start + 21),
        ) {
            (Some(129), Some(1)) => (
                [Some(start + 26), Some(start + 56)],
                start + 40,
                start + 51,
                start + 55,
                start + 66,
                start + 77,
            ),
            (Some(118), Some(0)) => (
                [Some(start + 45), None],
                start + 29,
                start + 40,
                start + 44,
                start + 55,
                start + 66,
            ),
            _ => return None,
        };
    let first_identity_at = wrapper_offsets[0]? - 1;
    if (wrapper_offsets[1].is_some()
        && (marked_record_reference(bytes, first_identity_at).is_none()
            || !zeros_at::<6>(bytes, first_identity_at + 5)
            || View::u32_le_at(bytes, start + 36) != Some(1)))
        || View::u32_le_at(bytes, second_count_at) != Some(1)
        || marked_record_reference(bytes, second_identity_at).is_none()
        || !zeros_at::<6>(bytes, second_identity_at + 5)
        || marked_record_reference(bytes, scope_at) != Some(scope.record_index)
        || !zeros_at::<6>(bytes, scope_at + 5)
    {
        return None;
    }
    let selection_record_index = marked_record_reference(bytes, selection_at)?;
    let opaque_index = View::u32_le_at(bytes, tail_at)?;
    if !zeros_at::<6>(bytes, selection_at + 5)
        || opaque_index == 0
        || !View::f64_le_at(bytes, tail_at + 4)?.is_finite()
        || View::u32_le_at(bytes, tail_at + 12) != Some(opaque_index)
        || marked_record_reference(bytes, tail_at + 16) != record_index.checked_add(2)
        || !zeros_at::<6>(bytes, tail_at + 21)
        || !zeros_at::<2>(bytes, tail_at + 27)
        || marked_record_reference(bytes, tail_at + 29) != record_index.checked_add(1)
        || !zeros_at::<6>(bytes, tail_at + 34)
        || bytes.get(tail_at + 40) != Some(&0)
        || marked_record_reference(bytes, tail_at + 41) != Some(scope.record_index)
        || !zeros_at::<6>(bytes, tail_at + 46)
    {
        return None;
    }
    Some((wrapper_offsets, selection_record_index))
}

/// The persistent identity of the only frame of the wrapper `record_index`
/// and its offset. The wrapper closes with two nested records and an identity
/// record whose indexes follow its own.
fn exact_pattern_identity_wrapper(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Result<Option<(u64, u64)>, CodecError> {
    let &[start] = records.offsets(record_index) else {
        return Ok(None);
    };
    if !zeros_at::<10>(bytes, start + 11)
        || View::u64_le_at(bytes, start + 21).is_none_or(|value| value == 0)
    {
        return Ok(None);
    }
    let Some(after_asset_id) = relaxed_guid_end(bytes, start + 29) else {
        return Ok(None);
    };
    let Some(after_context_id) = relaxed_guid_end(bytes, after_asset_id) else {
        return Ok(None);
    };
    if View::u32_le_at(bytes, after_context_id) != Some(2)
        || !zeros_at::<4>(bytes, after_context_id + 4)
        || marked_record_reference(bytes, after_context_id + 8) != record_index.checked_add(1)
        || !zeros_at::<6>(bytes, after_context_id + 13)
    {
        return Ok(None);
    }
    let Some(nested_one) = next_indexed_record_header(ctx, bytes, after_context_id + 19, |_| true)?
    else {
        return Ok(None);
    };
    let nested_one_at = nested_one.offset;
    if Some(nested_one.record_index) != record_index.checked_add(1)
        || !zeros_at::<10>(bytes, nested_one_at + 11)
        || marked_record_reference(bytes, nested_one_at + 21) != record_index.checked_add(2)
        || !zeros_at::<6>(bytes, nested_one_at + 26)
    {
        return Ok(None);
    }
    let Some(identity) = next_indexed_record_header(ctx, bytes, nested_one_at + 32, |_| true)?
    else {
        return Ok(None);
    };
    let identity_at = identity.offset;
    // The identity record is 29 bytes long, so the next header must open
    // where it ends.
    if Some(identity.record_index) != record_index.checked_add(2)
        || !zeros_at::<10>(bytes, identity_at + 11)
        || indexed_record_header_at(bytes, identity_at + 29).map(|next| next.record_index)
            != record_index.checked_add(3)
    {
        return Ok(None);
    }
    Ok(View::u64_le_at(bytes, identity_at + 21)
        .map(|identity| (identity, u64_from_index(identity_at + 21))))
}

/// The inline axis of the carrier frame of `record_index` from `start` to
/// `paired_at`, when the frame names `selection_record_index` and the scope.
fn circular_pattern_axis_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    record_index: u32,
    selection_record_index: u32,
    scope_record_index: u32,
) -> Option<patterns::DesignCircularPatternAxis> {
    if paired_at.checked_sub(start) != Some(195)
        || !zeros_at::<10>(bytes, start + 11)
        || View::u32_le_at(bytes, start + 21) != Some(8)
        || !zeros_at::<16>(bytes, start + 73)
        || View::u32_le_at(bytes, start + 89)? == 0
        || View::u32_le_at(bytes, start + 93) != Some(1)
        || marked_record_reference(bytes, start + 97) != Some(selection_record_index)
        || !zeros_at::<6>(bytes, start + 102)
        || !zeros_at::<2>(bytes, start + 108)
        || View::u32_le_at(bytes, start + 110) != Some(1)
        || marked_record_reference(bytes, start + 114).is_none()
        || !zeros_at::<6>(bytes, start + 119)
        || View::u64_le_at(bytes, start + 125) != Some(0x0000_0004_0000_0000)
        || !zeros_at::<10>(bytes, start + 133)
    {
        return None;
    }
    let opaque_index = View::u32_le_at(bytes, start + 143)?;
    if opaque_index == 0
        || !View::f64_le_at(bytes, start + 147)?.is_finite()
        || View::u32_le_at(bytes, start + 155) != Some(opaque_index)
        || marked_record_reference(bytes, start + 159) != record_index.checked_add(2)
        || !zeros_at::<8>(bytes, start + 164)
        || marked_record_reference(bytes, start + 172) != record_index.checked_add(1)
        || !zeros_at::<7>(bytes, start + 177)
        || marked_record_reference(bytes, start + 184) != Some(scope_record_index)
        || !zeros_at::<6>(bytes, start + 189)
    {
        return None;
    }
    let origin = finite_reals_at(bytes, start + 25)?;
    let displacement = finite_reals_at(bytes, start + 49)?.map(FiniteReal::get);
    patterns::DesignCircularPatternAxis::inline(
        origin,
        u64_from_index(start + 25),
        displacement,
        u64_from_index(start + 49),
    )
}

/// The count of the only fixed count frame of `record_index` owned by the
/// scope, and its offset.
fn exact_fixed_pattern_count(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Result<Option<(u32, u64)>, CodecError> {
    let mut candidate = None;
    let ambiguous = find_frame(
        ctx,
        records,
        record_index,
        |start, paired_at| {
            Ok(
                fixed_pattern_count_at(bytes, start, paired_at, record_index, scope_record_index)
                    .is_some_and(|count| candidate.replace(count).is_some()),
            )
        },
        "scan F3D indexed record frames",
    )?;
    Ok(candidate.filter(|_| ambiguous.is_none()))
}

fn fixed_pattern_count_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    record_index: u32,
    scope_record_index: u32,
) -> Option<(u32, u64)> {
    if paired_at.checked_sub(start) != Some(99)
        || !zeros_at::<8>(bytes, start + 11)
        || bytes.get(start + 19) != Some(&1)
        || View::u32_le_at(bytes, start + 20) != Some(1)
        || marked_record_reference(bytes, start + 24) != Some(scope_record_index)
        || !zeros_at::<11>(bytes, start + 29)
        || marked_record_reference(bytes, start + 44) != record_index.checked_add(2)
        || !zeros_at::<6>(bytes, start + 49)
        || View::u32_le_at(bytes, start + 55)? == 0
        || !zeros_at::<4>(bytes, start + 59)
        || marked_record_reference(bytes, start + 63) != Some(scope_record_index)
        || !zeros_at::<8>(bytes, start + 68)
        || marked_record_reference(bytes, start + 76) != record_index.checked_add(1)
        || !zeros_at::<7>(bytes, start + 81)
        || marked_record_reference(bytes, start + 88) != Some(scope_record_index)
        || !zeros_at::<6>(bytes, start + 93)
    {
        return None;
    }
    let count = View::u32_le_at(bytes, start + 40)?;
    (count > 0).then_some((count, u64_from_index(start + 40)))
}

#[cfg(test)]
mod tests;
