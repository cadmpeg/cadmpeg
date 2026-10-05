// SPDX-License-Identifier: Apache-2.0
//! Decode exact carrier-owned assembly operand paths.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::layout::assembly_class_307_264_joint_origin_scope as class_307_joint_origin;

use super::legacy_operand_paths::ASSEMBLY_MARKED_REFERENCE_LEN;
use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::find_frame;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::design::decode::byte_fields::bytes_at;
use crate::design::decode::reference_runs::reference_position;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::{
    fixed_guid_ascii, fixed_guid_end, fixed_relaxed_guid_text, fixed_utf16_ascii_eq,
    retain_class_tag,
};
use crate::layout::assembly_class_363_264_frame_360_child as class_363_child;
use crate::layout::assembly_class_363_264_frame_360_leading as class_363_leading;
use crate::layout::assembly_class_363_264_frame_363_carrier as class_363_carrier;
use crate::layout::assembly_class_363_264_frame_386_terminal as class_363_terminal;
use crate::layout::assembly_class_363_264_frame_388_identity as class_363_identity;
use crate::layout::assembly_class_363_264_frame_388_identity_extended as class_363_identity_extended;
use crate::layout::assembly_class_363_264_frame_388_identity_reduced_490 as class_363_identity_reduced_490;
use crate::layout::assembly_class_363_264_frame_388_identity_reduced_501 as class_363_identity_reduced_501;
use crate::layout::assembly_class_363_264_frame_388_identity_short as class_363_identity_short;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::assembly::DesignAssemblyOperandQualifier;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;

pub(super) fn exact_variable_reference_operand_qualifiers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frames: &[DesignAssemblyOperandFrame; 2],
) -> Result<Option<[DesignAssemblyOperandQualifier; 2]>, CodecError> {
    let mut qualifiers = [None, None];
    for (qualifier, frame) in qualifiers.iter_mut().zip(frames) {
        *qualifier = match exact_class_363_operand_path(ctx, bytes, records, scope, frame)? {
            Some(path) => Some(DesignAssemblyOperandQualifier::OccurrencePath { path }),
            None => exact_class_307_joint_origin(ctx, bytes, records, frame)?,
        };
    }
    let [Some(first), Some(second)] = qualifiers else {
        return Ok(None);
    };
    Ok(Some([first, second]))
}

fn exact_class_363_operand_path(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    const CLASS_TAG_OPERATION: &str = "copy F3D carrier operand path class tag";
    let Some((carrier_at, carrier_paired_at)) = exact_class_264_record_frame(
        ctx,
        bytes,
        records,
        frame.reference_record_index,
        *b"363",
        class_363_carrier::LEN,
    )?
    else {
        return Ok(None);
    };
    let Some((leading_record_index, terminal_record_index)) = carrier_at
        .checked_add(class_363_carrier::LEADING_REFERENCE)
        .and_then(|at| {
            Some((
                marked_record_reference(bytes, at)?,
                marked_record_reference(
                    bytes,
                    carrier_at.checked_add(class_363_carrier::TERMINAL_REFERENCE)?,
                )?,
            ))
        })
    else {
        return Ok(None);
    };
    let Some(leading) =
        exact_class_363_node_frame(ctx, bytes, records, scope, leading_record_index)?
    else {
        return Ok(None);
    };
    let Some((terminal_at, terminal_paired_at)) = exact_class_264_record_frame(
        ctx,
        bytes,
        records,
        terminal_record_index,
        *b"386",
        class_363_terminal::LEN,
    )?
    else {
        return Ok(None);
    };
    let Some((leading_identity_record_index, terminal_identity_record_index)) = leading
        .start
        .checked_add(class_363_leading::IDENTITY_REFERENCE)
        .and_then(|at| {
            Some((
                marked_record_reference(bytes, at)?,
                marked_record_reference(
                    bytes,
                    terminal_at.checked_add(class_363_terminal::IDENTITY_REFERENCE)?,
                )?,
            ))
        })
    else {
        return Ok(None);
    };
    let Some(leading_identity) =
        exact_class_363_identity_frame(ctx, bytes, records, leading_identity_record_index)?
    else {
        return Ok(None);
    };
    let Some(terminal_identity) =
        exact_class_363_identity_frame(ctx, bytes, records, terminal_identity_record_index)?
    else {
        return Ok(None);
    };
    let scope_backlinks = [
        leading,
        CarrierFrame {
            start: terminal_at,
            scope_reference: class_363_terminal::SCOPE_REFERENCE,
        },
        leading_identity,
        terminal_identity,
        CarrierFrame {
            start: carrier_at,
            scope_reference: class_363_carrier::SCOPE_REFERENCE,
        },
    ];
    let carrier_matches = (|| {
        Some(
            carrier_paired_at == carrier_at.checked_add(class_363_carrier::LEN)?
                && terminal_paired_at == terminal_at.checked_add(class_363_terminal::LEN)?
                && leading_record_index != terminal_record_index
                && leading_identity_record_index != terminal_identity_record_index
                && rigid_transform_at(
                    bytes,
                    carrier_at.checked_add(class_363_carrier::TRANSFORM)?,
                )? == frame.transform
                && marked_record_reference(
                    bytes,
                    carrier_at.checked_add(class_363_carrier::REPEATED_LEADING_REFERENCE)?,
                ) == Some(leading_record_index)
                && marked_record_reference(
                    bytes,
                    carrier_at.checked_add(class_363_carrier::REPEATED_TERMINAL_REFERENCE)?,
                ) == Some(terminal_record_index)
                && scope_backlinks.iter().all(|frame| {
                    frame
                        .start
                        .checked_add(frame.scope_reference)
                        .and_then(|at| marked_record_reference(bytes, at))
                        == Some(scope.record_index)
                }),
        )
    })();
    if carrier_matches != Some(true) {
        return Ok(None);
    }
    for ordinal in 0..4 {
        let Some(owner_record_index) = (ordinal * ASSEMBLY_MARKED_REFERENCE_LEN)
            .checked_add(class_363_carrier::PLACEMENT_OWNER_REFERENCES)
            .and_then(|relative| carrier_at.checked_add(relative))
            .and_then(|at| marked_record_reference(bytes, at))
        else {
            return Ok(None);
        };
        if reference_position(
            ctx,
            scope.reference_members(),
            |value| Ok(*value == owner_record_index),
            "find F3D carrier placement owner",
        )?
        .is_none()
        {
            return Ok(None);
        }
    }
    // The terminal identity repeats the leading one; only the leading GUIDs
    // are kept.
    let Some([terminal_occurrence_guid, terminal_identity_guid]) =
        class_363_identity_guid_codes(ctx, bytes, terminal_identity.start)?
    else {
        return Ok(None);
    };
    let Some((occurrence_guid, identity_guid)) =
        exact_class_363_identity_guids(ctx, bytes, leading_identity.start)?
    else {
        return Ok(None);
    };
    if guid_code_units(&occurrence_guid.value) != Some(&terminal_occurrence_guid)
        || guid_code_units(&identity_guid.value) != Some(&terminal_identity_guid)
    {
        return Ok(None);
    }
    let Some((scope_record_index, locator_scope_reference_offset)) = carrier_at
        .checked_add(class_363_carrier::SCOPE_REFERENCE)
        .and_then(|at| exact_same_segment_record_reference(bytes, at))
    else {
        return Ok(None);
    };
    if scope_record_index != scope.record_index {
        return Ok(None);
    }
    let Some((_, wrapper_reference_offset)) = leading
        .start
        .checked_add(class_363_leading::IDENTITY_REFERENCE)
        .and_then(|at| exact_same_segment_record_reference(bytes, at))
    else {
        return Ok(None);
    };
    let link = DesignAssemblyOperandPathLink {
        locator_reference_offset: frame.reference_offset,
        locator_record_index: frame.reference_record_index,
        locator_class_tag: retain_class_tag(ctx, b"363", CLASS_TAG_OPERATION)?,
        locator_byte_offset: u64_from_index(carrier_at),
        locator_scope_reference_offset,
        wrapper_record_index: leading_identity_record_index,
        wrapper_reference_offset,
        wrapper_class_tag: retain_class_tag(ctx, b"388", CLASS_TAG_OPERATION)?,
        wrapper_byte_offset: u64_from_index(leading_identity.start),
        path_reference_offset: occurrence_guid.offset,
    };
    let class_tag = retain_class_tag(ctx, b"386", CLASS_TAG_OPERATION)?;
    let mut occurrence_guids = Vec::new();
    ctx.push_vec(
        &mut occurrence_guids,
        occurrence_guid,
        "f3d carrier path occurrences",
    )?;
    let mut identity_guids = Vec::new();
    ctx.push_vec(
        &mut identity_guids,
        identity_guid,
        "collect F3D carrier path identity GUIDs",
    )?;
    Ok(DesignAssemblyOperandPath::try_new(
        link,
        terminal_record_index,
        class_tag,
        u64_from_index(terminal_at),
        occurrence_guids,
        identity_guids,
    )
    .ok())
}

/// The 36 ASCII code units of a relaxed GUID read by `fixed_relaxed_guid_text`.
fn guid_code_units(guid: &DesignRelaxedGuidText) -> Option<&[u8; 36]> {
    guid.as_str().as_bytes().first_chunk::<36>()
}

fn exact_class_307_joint_origin(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    frame: &DesignAssemblyOperandFrame,
) -> Result<Option<DesignAssemblyOperandQualifier>, CodecError> {
    const CLASS_TAG_OPERATION: &str = "copy F3D joint-origin operand class tag";
    let Some((start, paired_at)) = exact_class_264_record_frame(
        ctx,
        bytes,
        records,
        frame.reference_record_index,
        *b"307",
        class_307_joint_origin::LEN,
    )?
    else {
        return Ok(None);
    };
    if !class_307_joint_origin_fields(bytes, start, paired_at) {
        return Ok(None);
    }
    let Some(identity_end) =
        fixed_guid_end(ctx, bytes, start + class_307_joint_origin::IDENTITY_GUID)?
    else {
        return Ok(None);
    };
    let Some(kind_end) = fixed_utf16_ascii_eq(
        ctx,
        bytes,
        start + class_307_joint_origin::KIND_CODE_UNIT_COUNT,
        "JointOrigin",
    )?
    else {
        return Ok(None);
    };
    if identity_end != start + class_307_joint_origin::REFERENCE_COUNT - 3
        || kind_end != start + class_307_joint_origin::FEATURE_ORDINAL
    {
        return Ok(None);
    }
    Ok(Some(DesignAssemblyOperandQualifier::JointOrigin {
        scope_record_index: frame.reference_record_index,
        class_tag: retain_class_tag(ctx, b"307", CLASS_TAG_OPERATION)?,
        byte_offset: u64_from_index(start),
        paired_class_tag: retain_class_tag(ctx, b"264", CLASS_TAG_OPERATION)?,
        paired_byte_offset: u64_from_index(paired_at),
    }))
}

/// The fixed fields of a class-307 joint-origin frame from `start` to its
/// paired header at `paired_at`: its two references, reference count, three
/// reference entries and trailer.
fn class_307_joint_origin_fields(bytes: &[u8], start: usize, paired_at: usize) -> bool {
    paired_at == start + class_307_joint_origin::LEN
        && marked_record_reference(bytes, start + class_307_joint_origin::FIRST_REFERENCE).is_some()
        && marked_record_reference(bytes, start + class_307_joint_origin::SECOND_REFERENCE)
            .is_some()
        && View::u32_le_at(bytes, start + class_307_joint_origin::REFERENCE_COUNT)
            == Some(class_307_joint_origin::REFERENCE_COUNT_VALUE)
        && bytes_at::<{ class_307_joint_origin::REFERENCE_TRAILER_VALUE.len() }>(
            bytes,
            start + class_307_joint_origin::REFERENCE_TRAILER,
        ) == Some(&class_307_joint_origin::REFERENCE_TRAILER_VALUE)
        && (0..3).all(|ordinal| {
            marked_record_reference(
                bytes,
                start
                    + class_307_joint_origin::REFERENCE_ENTRIES
                    + ordinal * ASSEMBLY_MARKED_REFERENCE_LEN,
            )
            .is_some()
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CarrierFrame {
    start: usize,
    scope_reference: usize,
}

fn exact_class_363_identity_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Result<Option<CarrierFrame>, CodecError> {
    for (frame_length, scope_reference) in [
        (
            class_363_identity_reduced_490::LEN,
            class_363_identity_reduced_490::SCOPE_REFERENCE,
        ),
        (
            class_363_identity_reduced_501::LEN,
            class_363_identity_reduced_501::SCOPE_REFERENCE,
        ),
        (
            class_363_identity_short::LEN,
            class_363_identity_short::SCOPE_REFERENCE,
        ),
        (class_363_identity::LEN, class_363_identity::SCOPE_REFERENCE),
        (
            class_363_identity_extended::LEN,
            class_363_identity_extended::SCOPE_REFERENCE,
        ),
    ] {
        if let Some((start, _paired_at)) =
            exact_class_264_record_frame(ctx, bytes, records, record_index, *b"388", frame_length)?
        {
            return Ok(Some(CarrierFrame {
                start,
                scope_reference,
            }));
        }
    }
    Ok(None)
}

fn exact_class_363_node_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    record_index: u32,
) -> Result<Option<CarrierFrame>, CodecError> {
    if let Some((start, _paired_at)) = exact_class_264_record_frame(
        ctx,
        bytes,
        records,
        record_index,
        *b"360",
        class_363_leading::LEN,
    )? {
        return Ok(Some(CarrierFrame {
            start,
            scope_reference: class_363_leading::SCOPE_REFERENCE,
        }));
    }
    let Some((start, _paired_at)) = exact_class_264_record_frame(
        ctx,
        bytes,
        records,
        record_index,
        *b"360",
        class_363_child::LEN,
    )?
    else {
        return Ok(None);
    };
    let Some(leading_record_index) =
        marked_record_reference(bytes, start + class_363_child::LEADING_REFERENCE)
    else {
        return Ok(None);
    };
    // The leading record's frame is checked before the child frame is accepted.
    if exact_class_264_record_frame(
        ctx,
        bytes,
        records,
        leading_record_index,
        *b"360",
        class_363_leading::LEN,
    )?
    .is_none()
        || reference_position(
            ctx,
            scope.reference_members(),
            |value| Ok(*value == leading_record_index),
            "find F3D carrier leading owner reference",
        )?
        .is_none()
    {
        return Ok(None);
    }
    Ok(Some(CarrierFrame {
        start,
        scope_reference: class_363_child::SCOPE_REFERENCE,
    }))
}

/// The only frame of `record_index` that is `frame_length` bytes long, opens
/// with `class_tag` and closes with a class-264 header. The scan stops at a
/// second such frame.
fn exact_class_264_record_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: [u8; 3],
    frame_length: usize,
) -> Result<Option<(usize, usize)>, CodecError> {
    let mut candidate = None;
    let ambiguous = find_frame(
        ctx,
        records,
        record_index,
        |start, paired_at| {
            if Some(paired_at) != start.checked_add(frame_length)
                || exact_indexed_header_at(bytes, start, record_index) != Some(&class_tag)
                || exact_indexed_header_at(bytes, paired_at, record_index) != Some(b"264")
            {
                return Ok(false);
            }
            Ok(candidate.replace((start, paired_at)).is_some())
        },
        "scan F3D indexed record frames",
    )?;
    Ok(if ambiguous.is_some() { None } else { candidate })
}

/// The occurrence and component-identity GUID fields of a class-388 identity
/// record: two adjacent counted GUIDs, the second followed by four bytes.
fn class_363_identity_guid_positions(start: usize) -> (usize, usize) {
    (
        start + class_363_identity::OCCURRENCE_GUID,
        start + class_363_identity::COMPONENT_IDENTITY_GUID,
    )
}

/// The occurrence and component-identity GUIDs of a class-388 identity record.
type IdentityGuids = (
    Located<DesignRelaxedGuidText>,
    Located<DesignRelaxedGuidText>,
);

/// The occurrence and component-identity GUIDs of the class-388 identity
/// record at `start`, with the offsets of their code units.
fn exact_class_363_identity_guids(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<IdentityGuids>, CodecError> {
    let (occurrence_at, identity_at) = class_363_identity_guid_positions(start);
    let Some((occurrence_guid, occurrence_end)) =
        fixed_relaxed_guid_text(ctx, bytes, occurrence_at)?
    else {
        return Ok(None);
    };
    if occurrence_end != identity_at {
        return Ok(None);
    }
    let Some((identity_guid, identity_end)) = fixed_relaxed_guid_text(ctx, bytes, identity_at)?
    else {
        return Ok(None);
    };
    if identity_end != identity_at + 76 {
        return Ok(None);
    }
    Ok(Some((
        Located {
            value: occurrence_guid,
            offset: u64_from_index(occurrence_at + 4),
        },
        Located {
            value: identity_guid,
            offset: u64_from_index(identity_at + 4),
        },
    )))
}

/// The ASCII code units of the two identity GUIDs of the class-388 identity
/// record at `start`, validated without a copy.
fn class_363_identity_guid_codes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<[[u8; 36]; 2]>, CodecError> {
    let (occurrence_at, identity_at) = class_363_identity_guid_positions(start);
    let Some((occurrence_guid, occurrence_end)) = fixed_guid_ascii(ctx, bytes, occurrence_at)?
    else {
        return Ok(None);
    };
    if occurrence_end != identity_at {
        return Ok(None);
    }
    let Some((identity_guid, identity_end)) = fixed_guid_ascii(ctx, bytes, identity_at)? else {
        return Ok(None);
    };
    Ok((identity_end == identity_at + 76).then_some([occurrence_guid, identity_guid]))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::index_from_u32;

    use super::super::legacy_operand_paths::ASSEMBLY_MARKED_REFERENCE_LEN;
    use super::{
        class_363_identity_guid_codes, exact_class_264_record_frame, exact_class_307_joint_origin,
        exact_class_363_identity_frame, exact_class_363_identity_guids, CarrierFrame,
    };
    use crate::bytes::lp_utf16_bounded_charged;
    use crate::layout::{
        assembly_class_307_264_joint_origin_scope as class_307_joint_origin,
        assembly_class_363_264_frame_388_identity as class_363_identity,
        assembly_class_363_264_frame_388_identity_extended as class_363_identity_extended,
        assembly_class_363_264_frame_388_identity_reduced_490 as class_363_identity_reduced_490,
        assembly_class_363_264_frame_388_identity_reduced_501 as class_363_identity_reduced_501,
        assembly_class_363_264_frame_388_identity_short as class_363_identity_short,
    };
    use crate::records::feature::assembly::{
        DesignAssemblyOperandFrame, DesignAssemblyOperandQualifier,
    };
    use crate::test_support::write_marked_reference;

    fn write_header(bytes: &mut [u8], at: usize, class_tag: [u8; 3], record_index: u32) {
        bytes[at..at + 4].copy_from_slice(&3_u32.to_le_bytes());
        bytes[at + 4..at + 7].copy_from_slice(&class_tag);
        bytes[at + 7..at + 11].copy_from_slice(&record_index.to_le_bytes());
    }

    fn write_lp_utf16(bytes: &mut [u8], at: usize, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        bytes[at..at + 4].copy_from_slice(
            &(u32::try_from(units.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for (ordinal, unit) in units.into_iter().enumerate() {
            let start = at + 4 + ordinal * 2;
            bytes[start..start + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    #[test]
    fn class_307_joint_origin_is_a_pathless_operand_qualifier() {
        let record_index = 17;
        let mut bytes = vec![0; class_307_joint_origin::LEN + 22];
        write_header(&mut bytes, 0, *b"307", record_index);
        write_header(
            &mut bytes,
            class_307_joint_origin::LEN,
            *b"264",
            record_index,
        );
        write_header(
            &mut bytes,
            class_307_joint_origin::LEN + 11,
            *b"399",
            record_index + 1,
        );
        write_marked_reference(&mut bytes, class_307_joint_origin::FIRST_REFERENCE, 31);
        write_marked_reference(&mut bytes, class_307_joint_origin::SECOND_REFERENCE, 32);
        write_lp_utf16(
            &mut bytes,
            class_307_joint_origin::IDENTITY_GUID,
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        );
        bytes[class_307_joint_origin::REFERENCE_COUNT..class_307_joint_origin::REFERENCE_COUNT + 4]
            .copy_from_slice(&class_307_joint_origin::REFERENCE_COUNT_VALUE.to_le_bytes());
        for ordinal in 0..index_from_u32(class_307_joint_origin::REFERENCE_COUNT_VALUE) {
            write_marked_reference(
                &mut bytes,
                class_307_joint_origin::REFERENCE_ENTRIES + ordinal * ASSEMBLY_MARKED_REFERENCE_LEN,
                40 + u32::try_from(ordinal).expect("fixture value fits u32"),
            );
        }
        bytes[class_307_joint_origin::REFERENCE_TRAILER
            ..class_307_joint_origin::REFERENCE_TRAILER
                + class_307_joint_origin::REFERENCE_TRAILER_VALUE.len()]
            .copy_from_slice(&class_307_joint_origin::REFERENCE_TRAILER_VALUE);
        write_lp_utf16(
            &mut bytes,
            class_307_joint_origin::KIND_CODE_UNIT_COUNT,
            "JointOrigin",
        );
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let frame = DesignAssemblyOperandFrame {
            reference_record_index: record_index,
            reference_offset: 9,
            transform: crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            transform_offset: 20,
        };

        let joint_origin_frame = crate::test_support::with_decode_context(|ctx| {
            exact_class_264_record_frame(
                ctx,
                &bytes,
                &records,
                record_index,
                *b"307",
                class_307_joint_origin::LEN,
            )
        })
        .unwrap();
        assert_eq!(joint_origin_frame, Some((0, class_307_joint_origin::LEN)));
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| lp_utf16_bounded_charged(
                ctx,
                &bytes,
                class_307_joint_origin::KIND_CODE_UNIT_COUNT,
                11..=11,
                "retain F3D UTF-16 string"
            )
            .unwrap()),
            Some((
                "JointOrigin".into(),
                class_307_joint_origin::FEATURE_ORDINAL
            ))
        );
        let qualifier = crate::test_support::with_decode_context(|ctx| {
            exact_class_307_joint_origin(ctx, &bytes, &records, &frame)
        })
        .unwrap();
        assert!(matches!(
            qualifier,
            Some(DesignAssemblyOperandQualifier::JointOrigin {
                scope_record_index: 17,
                byte_offset: 0,
                paired_byte_offset: 366,
                ..
            })
        ));

        bytes[class_307_joint_origin::REFERENCE_TRAILER] = 0;
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| {
                exact_class_307_joint_origin(ctx, &bytes, &records, &frame)
            })
            .unwrap(),
            None
        );
    }

    #[test]
    fn class_388_identity_spans_keep_one_guid_prefix() {
        let variants = [
            (
                class_363_identity_reduced_490::LEN,
                class_363_identity_reduced_490::SCOPE_REFERENCE,
            ),
            (
                class_363_identity_reduced_501::LEN,
                class_363_identity_reduced_501::SCOPE_REFERENCE,
            ),
            (
                class_363_identity_short::LEN,
                class_363_identity_short::SCOPE_REFERENCE,
            ),
            (class_363_identity::LEN, class_363_identity::SCOPE_REFERENCE),
            (
                class_363_identity_extended::LEN,
                class_363_identity_extended::SCOPE_REFERENCE,
            ),
        ];
        for (frame_length, scope_reference) in variants {
            let record_index = 17;
            let mut bytes = vec![0; frame_length + 22];
            write_header(&mut bytes, 0, *b"388", record_index);
            write_header(&mut bytes, frame_length, *b"264", record_index);
            write_header(&mut bytes, frame_length + 11, *b"399", record_index + 1);
            let guid = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
            for at in [
                class_363_identity::OCCURRENCE_GUID,
                class_363_identity::COMPONENT_IDENTITY_GUID,
            ] {
                bytes[at..at + 4].copy_from_slice(&36_u32.to_le_bytes());
                for (ordinal, unit) in guid.encode_utf16().enumerate() {
                    let start = at + 4 + ordinal * 2;
                    bytes[start..start + 2].copy_from_slice(&unit.to_le_bytes());
                }
            }
            let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
            assert_eq!(
                crate::test_support::with_decode_context(|ctx| {
                    exact_class_363_identity_frame(ctx, &bytes, &records, record_index)
                })
                .unwrap(),
                Some(CarrierFrame {
                    start: 0,
                    scope_reference
                })
            );
            let (occurrence, identity) = exact_class_363_identity_guids(
                &cadmpeg_test_support::service_decode_context(),
                &bytes,
                0,
            )
            .expect("GUID admission")
            .expect("identity GUID prefix");
            assert_eq!(occurrence.value.as_str(), guid);
            assert_eq!(identity.value.as_str(), guid);
            assert_eq!(
                class_363_identity_guid_codes(
                    &cadmpeg_test_support::service_decode_context(),
                    &bytes,
                    0,
                )
                .expect("GUID admission"),
                Some([*guid.as_bytes().first_chunk::<36>().unwrap(); 2])
            );
        }
    }
}
