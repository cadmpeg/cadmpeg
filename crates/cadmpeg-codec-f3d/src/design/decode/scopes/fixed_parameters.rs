// SPDX-License-Identifier: Apache-2.0
//! Exact fixed extrude, fillet and chamfer parameter scopes.

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::find_frame;
use super::shared_frames::marked_record_reference;
use super::shared_frames::FixedScalarFrame;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::reference_runs::reference_position;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::fixed_parameters::DesignFixedChamferDistance;
use crate::records::feature::fixed_parameters::DesignFixedChamferParameters;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeDistance;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeParameters;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeScalar;
use crate::records::feature::fixed_parameters::DesignFixedFilletGroup;
use crate::records::feature::fixed_parameters::DesignFixedFilletParameters;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{NonZeroReal, PositiveReal};

/// Unwrap an `Option`, ending a fallible parse with `Ok(None)` when it is empty.
macro_rules! try_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

/// Whether native record `id` lies in the stream scope `stream`. An ID
/// without a stream scope matches only an absent scope.
fn same_stream(
    ctx: &DecodeContext<'_>,
    id: &str,
    stream: Option<&str>,
) -> Result<bool, CodecError> {
    match stream {
        Some(stream) => in_stream(ctx, id, stream),
        None => Ok(record_stream(ctx, id)?.is_none()),
    }
}

pub(super) fn exact_fixed_extrude_parameters(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignFixedExtrudeParameters>, CodecError> {
    // Only an Extrude-family payload carries an Extrude prologue.
    if scope
        .extrude_prologue()
        .and_then(DesignExtrudePrologue::extent)
        != Some(DesignExtrudeExtent::OneSidedDistance)
    {
        return Ok(None);
    }
    let mut fixed_lanes = [None; 2];
    let mut fixed_count = 0;
    let mut embedded_distance = None;
    // The search stops at the first reference that makes the lanes ambiguous:
    // a third owned scalar or a second embedded distance.
    let ambiguous = reference_position(
        ctx,
        scope.reference_members(),
        |&record_index| {
            if let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, record_index)?
                .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
            {
                let Some(slot) = fixed_lanes.get_mut(fixed_count) else {
                    return Ok(true);
                };
                *slot = Some((record_index, scalar));
                fixed_count += 1;
            }
            if let Some(scalar) = exact_embedded_extrude_distance(
                ctx,
                bytes,
                records,
                record_index,
                scope.record_index,
            )? {
                if embedded_distance.replace((record_index, scalar)).is_some() {
                    return Ok(true);
                }
            }
            Ok(false)
        },
        "scan F3D fixed Extrude scope references",
    )?;
    if ambiguous.is_some() {
        return Ok(None);
    }
    let mut along_distance = embedded_distance.map(|(record_index, lane)| {
        DesignFixedExtrudeDistance::DistanceConstruction(DesignFixedExtrudeScalar {
            value: lane.value,
            record_index,
            value_offset: lane.value_offset,
        })
    });
    let mut taper_angle = None;
    let mut seen_fixed_ordinals = [false; 2];
    let stream = record_stream(ctx, &scope.id)?;
    for (record_index, lane) in fixed_lanes.into_iter().flatten() {
        let ordinal = usize::from(lane.ordinal);
        if ordinal >= seen_fixed_ordinals.len() || seen_fixed_ordinals[ordinal] {
            return Ok(None);
        }
        seen_fixed_ordinals[ordinal] = true;
        let owner = ctx.find_by(
            parameter_owners,
            |owner| {
                Ok(owner.scope_record_index() == scope.record_index
                    && owner.record_index() == record_index
                    && same_stream(ctx, owner.id(), stream)?)
            },
            "find F3D fixed Extrude parameter owner",
        )?;
        let source_kind = match owner {
            Some(owner) => ctx
                .find_by(
                    parameters,
                    |parameter| {
                        Ok(parameter.record_index == owner.parameter_record_index()
                            && same_stream(ctx, &parameter.id, stream)?)
                    },
                    "find F3D fixed Extrude owner parameter",
                )?
                .map(DesignParameter::source_kind),
            None => None,
        };
        match source_kind {
            Some("AlongDistance") if along_distance.is_none() => {
                let Some(value) = NonZeroReal::new(lane.value.get()) else {
                    return Ok(None);
                };
                along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(
                    DesignFixedExtrudeScalar {
                        value,
                        record_index,
                        value_offset: lane.value_offset,
                    },
                ));
            }
            Some("TaperAngle") if taper_angle.is_none() => {
                taper_angle = Some(DesignFixedExtrudeScalar {
                    value: lane.value,
                    record_index,
                    value_offset: lane.value_offset,
                });
            }
            Some("AlongDistance") if along_distance.is_some() && lane.value.get() == 0.0 => {}
            Some(_) => return Ok(None),
            None => match lane.ordinal {
                0 if along_distance.is_none() && lane.value.get() != 0.0 => {
                    let Some(value) = NonZeroReal::new(lane.value.get()) else {
                        return Ok(None);
                    };
                    along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(
                        DesignFixedExtrudeScalar {
                            value,
                            record_index,
                            value_offset: lane.value_offset,
                        },
                    ));
                }
                0 if along_distance.is_some() && lane.value.get() == 0.0 => {}
                1 if taper_angle.is_none() => {
                    taper_angle = Some(DesignFixedExtrudeScalar {
                        value: lane.value,
                        record_index,
                        value_offset: lane.value_offset,
                    });
                }
                _ => return Ok(None),
            },
        }
    }
    if along_distance.is_none() && taper_angle.is_none() {
        return Ok(None);
    }
    Ok(Some(DesignFixedExtrudeParameters {
        along_distance,
        taper_angle,
    }))
}

/// The only frame of `record_index` that holds an embedded distance owned by
/// the scope. Each visited header is charged; the search stops at a second
/// such frame.
fn exact_embedded_extrude_distance(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Result<Option<FixedScalarFrame<PositiveReal>>, CodecError> {
    let mut found = None;
    let second = find_frame(
        ctx,
        records,
        record_index,
        |start, paired| {
            let Some(frame) = embedded_extrude_distance_at(
                ctx,
                bytes,
                start,
                paired,
                record_index,
                scope_record_index,
            )?
            else {
                return Ok(false);
            };
            Ok(found.replace(frame).is_some())
        },
        "find F3D embedded Extrude distance frame",
    )?;
    Ok(if second.is_some() { None } else { found })
}

/// The embedded positive distance of the 100-byte frame from `start` to
/// `paired`, when its references tie it to the scope and its two auxiliary
/// records.
fn embedded_extrude_distance_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired: usize,
    record_index: u32,
    scope_record_index: u32,
) -> Result<Option<FixedScalarFrame<PositiveReal>>, CodecError> {
    try_some!((try_some!(paired.checked_sub(start)) == 100).then_some(()));
    let (_, after_tag) = try_some!(lp_ascii_filtered_view(
        ctx,
        bytes,
        start,
        3..=3,
        u8::is_ascii_digit
    )?);
    let first_auxiliary = try_some!(record_index.checked_add(1));
    let second_auxiliary = try_some!(record_index.checked_add(2));
    if after_tag != start + 7
        || !zeros_at::<10>(bytes, start + 11)
        || try_some!(marked_record_reference(bytes, start + 21)) != scope_record_index
        || !zeros_at::<6>(bytes, start + 26)
        || try_some!(View::u32_le_at(bytes, start + 32)) != 1
        || marked_record_reference(bytes, start + 36).is_none()
        || !zeros_at::<6>(bytes, start + 41)
        || try_some!(View::u32_le_at(bytes, start + 47)) != 210
        || try_some!(View::u32_le_at(bytes, start + 59)) != 210
        || try_some!(marked_record_reference(bytes, start + 63)) != second_auxiliary
        || !zeros_at::<6>(bytes, start + 68)
        || bytes_at::<3>(bytes, start + 74) != Some(&[1, 0, 0])
        || try_some!(marked_record_reference(bytes, start + 77)) != first_auxiliary
        || !zeros_at::<7>(bytes, start + 82)
        || try_some!(marked_record_reference(bytes, start + 89)) != scope_record_index
        || !zeros_at::<6>(bytes, start + 94)
    {
        return Ok(None);
    }
    let value = try_some!(PositiveReal::new(try_some!(View::f64_le_at(
        bytes,
        start + 51
    ))));
    Ok(Some(FixedScalarFrame {
        owner_record_index: Some(scope_record_index),
        ordinal: 0,
        value,
        value_offset: try_some!(u64::try_from(start + 51).ok()),
    }))
}

pub(super) fn exact_fixed_fillet_parameters(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignFixedFilletParameters>, CodecError> {
    use crate::records::feature::fixed_parameters::{
        DesignFixedFilletIntermediate, DesignFixedFilletLaw, DesignFixedFilletScalar,
    };
    if !matches!(
        scope.payload(),
        scope::DesignScopePayload::Fillet(_)
            | scope::DesignScopePayload::Conge(_)
            | scope::DesignScopePayload::Abrundung(_)
            | scope::DesignScopePayload::Arredondamento(_)
    ) {
        return Ok(None);
    }
    // Owned scalar lanes carry consecutive ordinals in reference order; the
    // search stops at the first lane out of order.
    let mut reservation = ctx.reserve_scoped(0, "f3d fixed Fillet scalar lanes")?;
    let mut lanes = Vec::new();
    let misordered = reference_position(
        ctx,
        scope.reference_members(),
        |&record_index| {
            let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, record_index)?
                .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
            else {
                return Ok(false);
            };
            if usize::from(scalar.ordinal) != lanes.len() {
                return Ok(true);
            }
            ctx.push_scoped_vec(
                &mut reservation,
                &mut lanes,
                (record_index, scalar),
                "f3d fixed Fillet scalar lanes",
            )?;
            Ok(false)
        },
        "scan F3D fixed Fillet scope references",
    )?;
    if misordered.is_some() || lanes.is_empty() {
        return Ok(None);
    }

    let scalar = |(record_index, scalar): &(u32, FixedScalarFrame)| DesignFixedFilletScalar {
        value: scalar.value,
        record_index: *record_index,
        value_offset: scalar.value_offset,
    };
    let group = |tangency_lane: Option<&(u32, FixedScalarFrame)>, law: DesignFixedFilletLaw| {
        DesignFixedFilletGroup::try_new(tangency_lane.map(scalar), law).ok()
    };
    let group_count = if lanes.len() == 1 || !lanes.len().is_multiple_of(2) {
        1
    } else {
        lanes.len() / 2
    };
    let mut groups = ctx.vector_storage(group_count, "f3d fixed Fillet groups")?;
    if lanes.len() == 1 {
        let Some(value) = group(None, DesignFixedFilletLaw::Constant(scalar(&lanes[0]))) else {
            return Ok(None);
        };
        ctx.push_vec(&mut groups, value, "f3d fixed Fillet groups")?;
    } else if lanes.len().is_multiple_of(2) {
        // Each tangency lane pairs with the constant lane after it; the
        // search stops at the first pair that forms no group.
        let invalid = ctx.position_by(
            lanes.as_chunks::<2>().0,
            |[tangency_lane, constant_lane]| {
                let Some(value) = group(
                    Some(tangency_lane),
                    DesignFixedFilletLaw::Constant(scalar(constant_lane)),
                ) else {
                    return Ok(true);
                };
                ctx.push_vec(&mut groups, value, "f3d fixed Fillet groups")?;
                Ok(false)
            },
            "group F3D fixed Fillet scalar lanes",
        )?;
        if invalid.is_some() {
            return Ok(None);
        }
    } else {
        // An odd count of three or more lanes: tangency, start and end, then
        // radius and parameter pairs.
        let pairs = lanes.get(3..).unwrap_or(&[]).as_chunks::<2>().0;
        let mut intermediate =
            ctx.vector_storage(pairs.len(), "f3d fixed Fillet intermediate rows")?;
        for [radius_lane, parameter_lane] in
            ctx.admit_iter(pairs, "collect F3D fixed Fillet intermediate lane pairs")?
        {
            ctx.push_vec(
                &mut intermediate,
                DesignFixedFilletIntermediate {
                    radius: scalar(radius_lane),
                    parameter: scalar(parameter_lane),
                },
                "f3d fixed Fillet intermediate rows",
            )?;
        }
        let Some(value) = group(
            Some(&lanes[0]),
            DesignFixedFilletLaw::Variable {
                start: scalar(&lanes[1]),
                end: scalar(&lanes[2]),
                intermediate,
            },
        ) else {
            return Ok(None);
        };
        ctx.push_vec(&mut groups, value, "f3d fixed Fillet groups")?;
    }
    Ok(Some(DesignFixedFilletParameters { groups }))
}

pub(super) fn exact_fixed_chamfer_parameters(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignFixedChamferParameters>, CodecError> {
    if !matches!(
        scope.payload(),
        scope::DesignScopePayload::Chamfer(_) | scope::DesignScopePayload::Chanfrein(_)
    ) {
        return Ok(None);
    }
    // A Chamfer whose scope owns parameters is not a fixed Chamfer.
    if let Some(stream) = record_stream(ctx, &scope.id)? {
        if ctx.any_by(
            parameter_owners,
            |owner| {
                Ok(owner.scope_record_index() == scope.record_index
                    && in_stream(ctx, owner.id(), stream)?)
            },
            "find F3D Chamfer parameter owners",
        )? {
            return Ok(None);
        }
    }
    let mut lanes = [None; 2];
    let mut lane_count = 0;
    // The search stops at the first owned lane out of order or past the
    // second.
    let rejected = reference_position(
        ctx,
        scope.reference_members(),
        |&record_index| {
            let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, record_index)?
                .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
            else {
                return Ok(false);
            };
            if usize::from(scalar.ordinal) != lane_count {
                return Ok(true);
            }
            let Some(slot) = lanes.get_mut(lane_count) else {
                return Ok(true);
            };
            *slot = Some((record_index, scalar));
            lane_count += 1;
            Ok(false)
        },
        "scan F3D fixed Chamfer scope references",
    )?;
    if rejected.is_some() {
        return Ok(None);
    }
    let mut distances = lanes.into_iter().flatten().map(|(record_index, scalar)| {
        Some(DesignFixedChamferDistance {
            value: PositiveReal::new(scalar.value.get())?,
            record_index,
            value_offset: scalar.value_offset,
        })
    });
    let Some(Some(first)) = distances.next() else {
        return Ok(None);
    };
    Ok(Some(match distances.next() {
        Some(Some(second)) => DesignFixedChamferParameters::TwoDistances { first, second },
        None => DesignFixedChamferParameters::EqualDistance { distance: first },
        Some(None) => return Ok(None),
    }))
}

/// The only positive local-ordinal-zero owner of the scope that one of its
/// references names, restricted to `record_index` when given. The owner scan
/// stops at a second candidate.
pub(super) fn unique_revolve_angle_owner<'a>(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
    record_index: Option<u32>,
) -> Result<Option<&'a DesignParameterOwner>, CodecError> {
    let stream = record_stream(ctx, &scope.id)?;
    let mut is_angle_owner = |owner: &DesignParameterOwner| -> Result<bool, CodecError> {
        if owner.scope_record_index() != scope.record_index
            || record_index.is_some_and(|index| owner.record_index() != index)
            || owner.local_ordinal() != 0
            || owner.evaluated_value().get() <= 0.0
            || !same_stream(ctx, owner.id(), stream)?
        {
            return Ok(false);
        }
        Ok(reference_position(
            ctx,
            scope.reference_members(),
            |&value| Ok(value == owner.record_index()),
            "find F3D revolve angle owner reference",
        )?
        .is_some())
    };
    let Some(first) = ctx.position_by(
        parameter_owners,
        &mut is_angle_owner,
        "find F3D revolve angle owner",
    )?
    else {
        return Ok(None);
    };
    let later = parameter_owners.get(first + 1..).unwrap_or(&[]);
    if ctx.any_by(later, is_angle_owner, "find F3D revolve angle owner")? {
        return Ok(None);
    }
    Ok(parameter_owners.get(first))
}
