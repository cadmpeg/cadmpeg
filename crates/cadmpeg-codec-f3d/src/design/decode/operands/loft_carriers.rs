// SPDX-License-Identifier: Apache-2.0
//! Legacy loft body carriers.

use super::construction_groups::take_record_reference;
use super::face_sources::indexed_operand_headers;
use crate::container::ContainerScan;
use crate::design::decode::sketch::indexed_record_header_at;
use crate::design::decode::text::design_record_id;
use crate::ids::native_stream;
use crate::layout::{
    legacy_loft_body_carrier as legacy_loft,
    legacy_loft_body_carrier_owner_tail as legacy_loft_tail,
    legacy_loft_body_carrier_three_flag as legacy_loft_three,
};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::entity_selection::DesignLoftLegacyBodyCarrier;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

/// Decode the fixed role-less body carrier used by the legacy Boolean-Loft
/// envelopes. The ordinary role-`0x8` body group is admitted only when this
/// exact carrier is present at scope-reference ordinal zero.
pub(crate) fn decode_loft_legacy_body_carriers(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignLoftLegacyBodyCarrier>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| {
        matches!(
                &scope.payload(),
                crate::records::feature::scope::DesignScopePayload::Loft(Some(crate::records::feature::path_features::DesignLoftConstruction { operation, .. }))
                    if *operation != crate::records::feature::extrude::DesignExtrudeOperation::NewBody
            )
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            if ordinal != 0 {
                continue;
            }
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Some(carrier) = parse_loft_legacy_body_carrier(bytes, scope, header) else {
                continue;
            };
            push_loft_legacy_body_carrier(&mut out, carrier, &entry.name, header.byte_offset);
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    out.dedup_by(|left, right| left.id == right.id);
    Ok(out)
}

pub(super) fn push_loft_legacy_body_carrier(
    out: &mut Vec<DesignLoftLegacyBodyCarrier>,
    mut carrier: DesignLoftLegacyBodyCarrier,
    stream: &str,
    offset: u64,
) {
    carrier.id = design_record_id(stream, ":design-loft-legacy-body-carrier#", offset);
    out.push(carrier);
}

/// Parse one legacy Loft body carrier.
pub(super) fn parse_loft_legacy_body_carrier(
    bytes: &[u8],
    scope: &DesignParameterScope,
    header: &DesignRecordHeader,
) -> Option<DesignLoftLegacyBodyCarrier> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let pair_matches = |length: usize| {
        start
            .checked_add(length)
            .and_then(|at| indexed_record_header_at(bytes, at))
            .is_some_and(|paired| paired.record_index == header.record_index)
    };
    let frame_length = [
        legacy_loft::LEN,
        legacy_loft_tail::LEN,
        legacy_loft_three::LEN,
    ]
    .into_iter()
    .find(|&length| pair_matches(length))?;
    let three_flags = frame_length == legacy_loft_three::LEN;
    let common = start
        + if three_flags {
            legacy_loft_three::COMMON_PREFIX
        } else if frame_length == legacy_loft_tail::LEN {
            legacy_loft_tail::COMMON_PREFIX
        } else {
            legacy_loft::INDEXED_HEADER
        };
    if indexed_record_header_at(bytes, common)
        .is_none_or(|parsed| parsed.record_index != header.record_index)
        || bytes.get(common + legacy_loft::ZERO_RUN_10..common + legacy_loft::PRESENCE)? != [0; 10]
        || bytes.get(common + legacy_loft::PRESENCE)? != &1
        || View::u32_le_at(bytes, common + legacy_loft::OWNER_SCOPE_RECORD_INDEX)?
            != scope.record_index
        || bytes.get(common + legacy_loft::ZERO_RUN_6..common + legacy_loft::MEMBER_COUNT)?
            != [0; 6]
        || View::u32_le_at(bytes, common + legacy_loft::MEMBER_COUNT)? != 1
    {
        return None;
    }
    let mut cursor = common.checked_add(legacy_loft::MEMBER_REFERENCE)?;
    let member_offset = cursor;
    let (member, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != common.checked_add(legacy_loft::OPAQUE_INDEX)? {
        return None;
    }
    let opaque_index = u8::try_from(View::u32_le_at(bytes, cursor)?)
        .ok()
        .and_then(std::num::NonZeroU8::new)?;
    let opaque_scalar = cadmpeg_ir::scalar::FiniteReal::new(View::f64_le_at(
        bytes,
        common + legacy_loft::OPAQUE_SCALAR,
    )?)?;
    let repeated_opaque_index =
        View::u32_le_at(bytes, common + legacy_loft::REPEATED_OPAQUE_INDEX)?;
    if repeated_opaque_index != u32::from(opaque_index.get()) {
        return None;
    }
    cursor = common.checked_add(legacy_loft::NEXT_NEXT_REFERENCE)?;
    let (next_next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
    if next_next_record_index != header.record_index.checked_add(2)?
        || cursor != common.checked_add(legacy_loft::FLAGS)?
    {
        return None;
    }
    let (next_offset, tail_zero, tail_reference) = if three_flags {
        if bytes.get(start + legacy_loft_three::FLAGS..start + legacy_loft_three::NEXT_REFERENCE)?
            != [1, 0, 0]
        {
            return None;
        }
        (
            start.checked_add(legacy_loft_three::NEXT_REFERENCE)?,
            Some(legacy_loft_three::TAIL_ZERO),
            Some(legacy_loft_three::TRAILING_SCOPE_REFERENCE),
        )
    } else {
        if bytes.get(common + legacy_loft::FLAGS..common + legacy_loft::NEXT_REFERENCE)? != [0, 0] {
            return None;
        }
        (
            common.checked_add(legacy_loft::NEXT_REFERENCE)?,
            (frame_length == legacy_loft_tail::LEN).then_some(legacy_loft_tail::TAIL_ZERO),
            (frame_length == legacy_loft_tail::LEN)
                .then_some(legacy_loft_tail::TRAILING_SCOPE_REFERENCE),
        )
    };
    cursor = next_offset;
    let (next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
    if next_record_index != header.record_index.checked_add(1)? {
        return None;
    }
    let trailing_scope_reference_offset =
        if let (Some(zero), Some(reference)) = (tail_zero, tail_reference) {
            if cursor != start.checked_add(zero)? || bytes.get(cursor)? != &0 {
                return None;
            }
            cursor = start.checked_add(reference)?;
            let reference_offset = cursor;
            let (record_index, _) = take_record_reference(bytes, &mut cursor)?;
            if record_index != scope.record_index {
                return None;
            }
            Some(u64::try_from(reference_offset).ok()?)
        } else {
            None
        };
    let paired_byte_offset = start.checked_add(frame_length)?;
    if cursor != paired_byte_offset {
        return None;
    }
    let paired = indexed_record_header_at(bytes, paired_byte_offset)?;
    if paired.record_index != header.record_index {
        return None;
    }
    Some(DesignLoftLegacyBodyCarrier {
        id: String::new(),
        scope_record_index: scope.record_index,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        owner_scope_record_index_offset: u64::try_from(
            common + legacy_loft::OWNER_SCOPE_RECORD_INDEX,
        )
        .ok()?,
        member,
        member_offset: u64::try_from(member_offset).ok()?,
        member_count_offset: u64::try_from(common + legacy_loft::MEMBER_COUNT).ok()?,
        opaque_index,
        opaque_index_offset: u64::try_from(common + legacy_loft::OPAQUE_INDEX).ok()?,
        opaque_scalar,
        opaque_scalar_offset: u64::try_from(common + legacy_loft::OPAQUE_SCALAR).ok()?,
        repeated_opaque_index_offset: u64::try_from(common + legacy_loft::REPEATED_OPAQUE_INDEX)
            .ok()?,
        next_next_record_index,
        next_next_reference_offset: u64::try_from(common + legacy_loft::NEXT_NEXT_REFERENCE)
            .ok()?,
        flags_offset: u64::try_from(common + legacy_loft::FLAGS).ok()?,
        next_record_index,
        next_reference_offset: u64::try_from(next_offset).ok()?,
        trailing_scope_reference_offset,
        paired_class_tag: paired.class_tag,
        paired_byte_offset: u64::try_from(paired_byte_offset).ok()?,
    })
}
