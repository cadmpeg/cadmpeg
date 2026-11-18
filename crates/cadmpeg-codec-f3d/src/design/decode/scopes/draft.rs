// SPDX-License-Identifier: Apache-2.0
//! Exact draft operation scopes.

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::layout::draft_operation_prefix as prefix;
use crate::layout::draft_parting_group_suffix as suffix;
use crate::layout::draft_parting_prefix as parting_prefix;
use crate::records::feature::direct_face::DesignDraftFlags;
use crate::records::feature::direct_face::DesignDraftOperation;
use crate::records::feature::direct_face::DesignDraftParting;
use crate::records::feature::direct_face::DesignDraftTwoSided;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::FiniteReal;

const MODE_SINGLE_ANGLE: u32 = 1;
const MARKED_REFERENCE_LEN: usize = 11;

pub(super) fn exact_draft_operation_with_owners(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignDraftOperation> {
    // The frame is variable-length and carries six or more references, so no
    // frame length or reference count identifies the record. The ordered
    // reference table is in record-index order, so the two scalar lanes hold no
    // fixed position in it either: they sort before the operand groups in one
    // document and after them in another. The lanes are identified by their own
    // properties instead. They are the only scope-owned fixed scalars among the
    // references, and their local ordinals order them.
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Draft)
        || scope.reference_members().len() < 6
    {
        return None;
    }
    let scope_stream = native_stream(&scope.id);
    let mut lanes = scope
        .reference_members()
        .values()
        .filter_map(|record_index| {
            if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index) {
                return (scalar.owner_record_index == Some(scope.record_index)).then_some((
                    *record_index,
                    u32::from(scalar.ordinal),
                    scalar.value,
                    scalar.value_offset,
                ));
            }
            let mut owners = parameter_owners.iter().filter(|owner| {
                owner.record_index() == *record_index
                    && owner.scope_record_index() == scope.record_index
                    && scope_stream.is_none_or(|stream| native_stream(owner.id()) == Some(stream))
            });
            let owner = owners.next()?;
            if owners.next().is_some() {
                return None;
            }
            Some((
                *record_index,
                owner.local_ordinal(),
                owner.evaluated_value(),
                owner.evaluated_value_offset(),
            ))
        });
    let (Some(mut first), Some(mut second), None) = (lanes.next(), lanes.next(), lanes.next())
    else {
        return None;
    };
    if first.1 > second.1 {
        std::mem::swap(&mut first, &mut second);
    }
    let (angle_record_index, angle_ordinal, angle, angle_offset) = first;
    let (opposite_angle_record_index, opposite_ordinal, opposite, opposite_offset) = second;
    if angle_ordinal != 0 || opposite_ordinal != 1 {
        return None;
    }
    let flags = draft_flags(
        bytes,
        scope,
        angle_record_index,
        opposite_angle_record_index,
    )
    .ok()?;
    let opposite_angle = match flags.and_then(|flags| flags.two_sided) {
        _ if flags.is_some_and(|flags| flags.parting.is_some()) => Some(opposite),
        None if opposite.get() == 0.0 => None,
        Some(DesignDraftTwoSided::TwoAngles) => Some(opposite),
        Some(DesignDraftTwoSided::Symmetric) if opposite == angle => Some(opposite),
        _ => return None,
    };
    Some(DesignDraftOperation {
        angle: cadmpeg_ir::scalar::Angle::from_assigned_real(angle),
        angle_record_index,
        angle_offset,
        opposite_angle_record_index,
        opposite_angle_offset: opposite_offset,
        opposite_angle: opposite_angle.map(cadmpeg_ir::scalar::Angle::from_assigned_real),
        flags,
    })
}

fn draft_flags(
    bytes: &[u8],
    scope: &DesignParameterScope,
    angle_record_index: u32,
    opposite_angle_record_index: u32,
) -> Result<Option<DesignDraftFlags>, ()> {
    let start = usize::try_from(scope.byte_offset()).map_err(|_| ())?;
    let length = usize::try_from(scope.frame_length()).map_err(|_| ())?;
    let end = start.checked_add(length).ok_or(())?.min(bytes.len());
    let frame = bytes.get(start..end).ok_or(())?;
    let Some(form @ 1..=3) = View::u32_le_at(frame, prefix::ANCHOR_KIND) else {
        return Ok(None);
    };
    let names_lanes = frame.get(prefix::ZERO_RUN_10..prefix::FORM) == Some(&[0; 10])
        && View::u32_le_at(frame, prefix::FORM) == Some(prefix::FORM_VALUE)
        && marked_record_reference(frame, prefix::ANGLE_REFERENCE) == Some(angle_record_index)
        && marked_record_reference(frame, prefix::OPPOSITE_ANGLE_REFERENCE)
            == Some(opposite_angle_record_index);
    if !names_lanes {
        return Ok(None);
    }
    let flag = |offset| match frame.get(offset) {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    };
    let mode = View::u32_le_at(frame, prefix::MODE).ok_or(())?;
    if form != 1 {
        return Ok(
            parting_groups(frame, scope, form, mode).and_then(|parting| {
                Some(DesignDraftFlags {
                    flipped: flag(prefix::FLIPPED)?,
                    tangent_chain: flag(prefix::TANGENT_CHAIN)?,
                    two_sided: None,
                    parting: Some(parting),
                })
            }),
        );
    }
    let flipped = flag(prefix::FLIPPED).ok_or(())?;
    let tangent_chain = flag(prefix::TANGENT_CHAIN).ok_or(())?;
    let two_sided = match mode {
        MODE_SINGLE_ANGLE => None,
        mode => Some(DesignDraftTwoSided::try_from(mode).map_err(|_| ())?),
    };
    Ok(Some(DesignDraftFlags {
        flipped,
        tangent_chain,
        two_sided,
        parting: None,
    }))
}

fn parting_groups(
    frame: &[u8],
    scope: &DesignParameterScope,
    form: u32,
    mode: u32,
) -> Option<DesignDraftParting> {
    let count_at = usize::try_from(
        scope
            .reference_count_offset()
            .checked_sub(scope.byte_offset())?,
    )
    .ok()?;
    let bytes = frame.get(..count_at)?;
    let prefix_scalars = [
        FiniteReal::new(View::f64_le_at(bytes, parting_prefix::FIRST_SCALAR)?)?,
        FiniteReal::new(View::f64_le_at(bytes, parting_prefix::SECOND_SCALAR)?)?,
    ];
    let boundary_flag = match *bytes.get(parting_prefix::BOUNDARY_FLAG)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let reference = |offset| {
        let value = marked_record_reference(bytes, offset)?;
        (value != 0
            && scope
                .reference_members()
                .values()
                .any(|member| *member == value))
        .then_some(Located {
            value,
            offset: scope.byte_offset().checked_add(u64_from_index(offset))?,
        })
    };
    let fixed_edges_at = parting_prefix::FIXED_EDGES_PRESENT;
    let (fixed_edges, group_at) = match *bytes.get(fixed_edges_at)? {
        0 => (None, parting_prefix::LEN),
        1 => (
            Some(reference(fixed_edges_at)?),
            fixed_edges_at.checked_add(MARKED_REFERENCE_LEN)?,
        ),
        _ => return None,
    };
    let pull = reference(group_at.checked_add(suffix::PULL_REFERENCE)?)?;
    let tool = reference(group_at.checked_add(suffix::TOOL_REFERENCE)?)?;
    let faces = reference(group_at.checked_add(suffix::FACES_REFERENCE)?)?;
    if View::u32_le_at(bytes, group_at.checked_add(suffix::CONSTANT_ONE)?) != Some(1)
        || pull.value == tool.value
        || pull.value == faces.value
        || tool.value == faces.value
        || fixed_edges
            .is_some_and(|fixed| [pull.value, tool.value, faces.value].contains(&fixed.value))
    {
        return None;
    }
    Some(DesignDraftParting {
        form,
        mode,
        prefix_scalars,
        boundary_flag,
        fixed_edges,
        pull,
        tool,
        faces,
    })
}

pub(super) fn contains_consecutive_guid_pair(bytes: &[u8]) -> bool {
    let relaxed_guid_end = |at: usize| {
        let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        if !(36..=38).contains(&count) {
            return None;
        }
        let start = at.checked_add(4)?;
        let end = start.checked_add(count.checked_mul(2)?)?;
        let units = bytes.get(start..end)?;
        units
            .chunks_exact(2)
            .all(|unit| {
                unit[1] == 0 && (unit[0].is_ascii_alphanumeric() || matches!(unit[0], b'-' | b'_'))
            })
            .then_some(end)
    };
    (0..bytes.len()).any(|at| {
        relaxed_guid_end(at).is_some_and(|after_first| relaxed_guid_end(after_first).is_some())
    })
}
