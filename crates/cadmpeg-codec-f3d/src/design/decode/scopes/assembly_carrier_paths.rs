// SPDX-License-Identifier: Apache-2.0
//! Decode exact carrier-owned assembly operand paths.

use cadmpeg_core::decode::View;

use crate::layout::assembly_class_307_264_joint_origin_scope as class_307_joint_origin;

use super::{
    class_363_carrier, class_363_child, class_363_identity, class_363_identity_extended,
    class_363_identity_reduced_490, class_363_identity_reduced_501, class_363_identity_short,
    class_363_leading, class_363_terminal, exact_indexed_header_at,
    exact_same_segment_record_reference, is_guid_relaxed, lp_utf16_bounded,
    marked_record_reference, rigid_transform_at, DesignAssemblyOperandFrame,
    DesignAssemblyOperandPath, DesignAssemblyOperandPathLink, DesignAssemblyOperandQualifier,
    DesignParameterScope, IndexedRecordOffsets, ASSEMBLY_MARKED_REFERENCE_LEN,
};

pub(super) fn exact_variable_reference_operand_qualifiers(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frames: &[DesignAssemblyOperandFrame; 2],
) -> Option<[DesignAssemblyOperandQualifier; 2]> {
    frames
        .iter()
        .map(|frame| {
            exact_class_363_operand_path(bytes, records, scope, frame)
                .map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })
                .or_else(|| exact_class_307_joint_origin(bytes, records, frame))
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn exact_class_363_operand_path(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
) -> Option<DesignAssemblyOperandPath> {
    let (carrier_at, carrier_paired_at) = exact_class_264_record_frame(
        bytes,
        records,
        frame.reference_record_index,
        "363",
        class_363_carrier::LEN,
    )?;
    let leading_record_index = marked_record_reference(
        bytes,
        carrier_at.checked_add(class_363_carrier::LEADING_REFERENCE)?,
    )?;
    let terminal_record_index = marked_record_reference(
        bytes,
        carrier_at.checked_add(class_363_carrier::TERMINAL_REFERENCE)?,
    )?;
    let (leading_at, leading_scope_reference) =
        exact_class_363_node_frame(bytes, records, scope, leading_record_index)?;
    let (terminal_at, terminal_paired_at) = exact_class_264_record_frame(
        bytes,
        records,
        terminal_record_index,
        "386",
        class_363_terminal::LEN,
    )?;
    let leading_identity_record_index = marked_record_reference(
        bytes,
        leading_at.checked_add(class_363_leading::IDENTITY_REFERENCE)?,
    )?;
    let terminal_identity_record_index = marked_record_reference(
        bytes,
        terminal_at.checked_add(class_363_terminal::IDENTITY_REFERENCE)?,
    )?;
    let (leading_identity_at, leading_identity_scope_reference) =
        exact_class_363_identity_frame(bytes, records, leading_identity_record_index)?;
    let (terminal_identity_at, terminal_identity_scope_reference) =
        exact_class_363_identity_frame(bytes, records, terminal_identity_record_index)?;
    let scope_backlinks = [
        (leading_at, leading_scope_reference),
        (terminal_at, class_363_terminal::SCOPE_REFERENCE),
        (leading_identity_at, leading_identity_scope_reference),
        (terminal_identity_at, terminal_identity_scope_reference),
        (carrier_at, class_363_carrier::SCOPE_REFERENCE),
    ];
    if carrier_paired_at != carrier_at.checked_add(class_363_carrier::LEN)?
        || terminal_paired_at != terminal_at.checked_add(class_363_terminal::LEN)?
        || leading_record_index == terminal_record_index
        || leading_identity_record_index == terminal_identity_record_index
        || rigid_transform_at(bytes, carrier_at.checked_add(class_363_carrier::TRANSFORM)?)?
            != frame.transform
        || marked_record_reference(
            bytes,
            carrier_at.checked_add(class_363_carrier::REPEATED_LEADING_REFERENCE)?,
        ) != Some(leading_record_index)
        || marked_record_reference(
            bytes,
            carrier_at.checked_add(class_363_carrier::REPEATED_TERMINAL_REFERENCE)?,
        ) != Some(terminal_record_index)
        || scope_backlinks.iter().any(|(start, relative_offset)| {
            marked_record_reference(bytes, start.saturating_add(*relative_offset))
                != Some(scope.record_index)
        })
    {
        return None;
    }
    for ordinal in 0..4 {
        let owner_record_index = marked_record_reference(
            bytes,
            carrier_at.checked_add(
                class_363_carrier::PLACEMENT_OWNER_REFERENCES
                    .checked_add(ordinal * ASSEMBLY_MARKED_REFERENCE_LEN)?,
            )?,
        )?;
        if !scope.reference_members.values().any(|value| value == &owner_record_index) {
            return None;
        }
    }
    let (occurrence_guid, identity_guid, occurrence_guid_offset, identity_guid_offset) =
        exact_class_363_identity_guids(bytes, leading_identity_at)?;
    let (terminal_occurrence_guid, terminal_identity_guid, _, _) =
        exact_class_363_identity_guids(bytes, terminal_identity_at)?;
    if occurrence_guid != terminal_occurrence_guid || identity_guid != terminal_identity_guid {
        return None;
    }
    let (scope_record_index, locator_scope_reference_offset) = exact_same_segment_record_reference(
        bytes,
        carrier_at.checked_add(class_363_carrier::SCOPE_REFERENCE)?,
    )?;
    if scope_record_index != scope.record_index {
        return None;
    }
    let (_, wrapper_reference_offset) = exact_same_segment_record_reference(
        bytes,
        leading_at.checked_add(class_363_leading::IDENTITY_REFERENCE)?,
    )?;
    Some(DesignAssemblyOperandPath {
        link: DesignAssemblyOperandPathLink {
            locator_reference_offset: frame.reference_offset,
            locator_record_index: frame.reference_record_index,
            locator_class_tag: "363".into(),
            locator_byte_offset: u64::try_from(carrier_at).ok()?,
            locator_scope_reference_offset,
            wrapper_record_index: leading_identity_record_index,
            wrapper_reference_offset,
            wrapper_class_tag: "388".into(),
            wrapper_byte_offset: u64::try_from(leading_identity_at).ok()?,
            path_reference_offset: occurrence_guid_offset,
        },
        record_index: terminal_record_index,
        class_tag: "386".into(),
        byte_offset: u64::try_from(terminal_at).ok()?,
        occurrence_guids: vec![occurrence_guid],
        occurrence_guid_offsets: vec![occurrence_guid_offset],
        identity_guids: vec![identity_guid],
        identity_guid_offsets: vec![identity_guid_offset],
    })
}

fn exact_class_307_joint_origin(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    frame: &DesignAssemblyOperandFrame,
) -> Option<DesignAssemblyOperandQualifier> {
    let (start, paired_at) = exact_class_264_record_frame(
        bytes,
        records,
        frame.reference_record_index,
        "307",
        class_307_joint_origin::LEN,
    )?;
    let (identity_guid, identity_end) = lp_utf16_bounded(
        bytes,
        start.checked_add(class_307_joint_origin::IDENTITY_GUID)?,
        36..=36,
    )?;
    let (kind, kind_end) = lp_utf16_bounded(
        bytes,
        start.checked_add(class_307_joint_origin::KIND_CODE_UNIT_COUNT)?,
        11..=11,
    )?;
    if paired_at != start.checked_add(class_307_joint_origin::LEN)?
        || marked_record_reference(
            bytes,
            start.checked_add(class_307_joint_origin::FIRST_REFERENCE)?,
        )
        .is_none()
        || marked_record_reference(
            bytes,
            start.checked_add(class_307_joint_origin::SECOND_REFERENCE)?,
        )
        .is_none()
        || !is_guid_relaxed(&identity_guid)
        || identity_end
            != start
                .checked_add(class_307_joint_origin::REFERENCE_COUNT)?
                .checked_sub(3)?
        || kind != "JointOrigin"
        || kind_end != start.checked_add(class_307_joint_origin::FEATURE_ORDINAL)?
        || View::u32_le_at(
            bytes,
            start.checked_add(class_307_joint_origin::REFERENCE_COUNT)?,
        )? != class_307_joint_origin::REFERENCE_COUNT_VALUE
        || bytes.get(
            start.checked_add(class_307_joint_origin::REFERENCE_TRAILER)?
                ..start
                    .checked_add(class_307_joint_origin::REFERENCE_TRAILER)?
                    .checked_add(class_307_joint_origin::REFERENCE_TRAILER_VALUE.len())?,
        )? != class_307_joint_origin::REFERENCE_TRAILER_VALUE
    {
        return None;
    }
    for ordinal in 0..class_307_joint_origin::REFERENCE_COUNT_VALUE as usize {
        marked_record_reference(
            bytes,
            start
                .checked_add(class_307_joint_origin::REFERENCE_ENTRIES)?
                .checked_add(ordinal.checked_mul(ASSEMBLY_MARKED_REFERENCE_LEN)?)?,
        )?;
    }
    Some(DesignAssemblyOperandQualifier::JointOrigin {
        scope_record_index: frame.reference_record_index,
        class_tag: "307".into(),
        byte_offset: u64::try_from(start).ok()?,
        paired_class_tag: "264".into(),
        paired_byte_offset: u64::try_from(paired_at).ok()?,
    })
}

fn exact_class_363_identity_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<(usize, usize)> {
    for (frame_length, scope_reference) in [
        (
            class_363_identity_reduced_490::LEN,
            class_363_identity_reduced_490::SCOPE_REFERENCE,
        ),
        (
            class_363_identity_reduced_501::LEN,
            class_363_identity_reduced_501::SCOPE_REFERENCE,
        ),
    ] {
        if let Some((start, _paired_at)) =
            exact_class_264_record_frame(bytes, records, record_index, "388", frame_length)
        {
            return Some((start, scope_reference));
        }
    }
    if let Some((start, _paired_at)) = exact_class_264_record_frame(
        bytes,
        records,
        record_index,
        "388",
        class_363_identity_short::LEN,
    ) {
        return Some((start, class_363_identity_short::SCOPE_REFERENCE));
    }
    if let Some((start, _paired_at)) =
        exact_class_264_record_frame(bytes, records, record_index, "388", class_363_identity::LEN)
    {
        return Some((start, class_363_identity::SCOPE_REFERENCE));
    }
    let (start, _paired_at) = exact_class_264_record_frame(
        bytes,
        records,
        record_index,
        "388",
        class_363_identity_extended::LEN,
    )?;
    Some((start, class_363_identity_extended::SCOPE_REFERENCE))
}

fn exact_class_363_node_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    record_index: u32,
) -> Option<(usize, usize)> {
    if let Some((start, _paired_at)) =
        exact_class_264_record_frame(bytes, records, record_index, "360", class_363_leading::LEN)
    {
        return Some((start, class_363_leading::SCOPE_REFERENCE));
    }
    let (start, _paired_at) =
        exact_class_264_record_frame(bytes, records, record_index, "360", class_363_child::LEN)?;
    let leading_record_index = marked_record_reference(
        bytes,
        start.checked_add(class_363_child::LEADING_REFERENCE)?,
    )?;
    let _ = exact_class_264_record_frame(
        bytes,
        records,
        leading_record_index,
        "360",
        class_363_leading::LEN,
    )?;
    if !scope.reference_members.values().any(|value| value == &leading_record_index) {
        return None;
    }
    Some((start, class_363_child::SCOPE_REFERENCE))
}

fn exact_class_264_record_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: &str,
    frame_length: usize,
) -> Option<(usize, usize)> {
    let mut candidates = records.frames(record_index).filter(|(start, paired_at)| {
        *paired_at == start.saturating_add(frame_length)
            && exact_indexed_header_at(bytes, *start, record_index).as_deref() == Some(class_tag)
            && exact_indexed_header_at(bytes, *paired_at, record_index).as_deref() == Some("264")
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn exact_class_363_identity_guids(
    bytes: &[u8],
    start: usize,
) -> Option<(String, String, u64, u64)> {
    let occurrence_at = start.checked_add(class_363_identity::OCCURRENCE_GUID)?;
    let identity_at = start.checked_add(class_363_identity::COMPONENT_IDENTITY_GUID)?;
    let (occurrence_guid, occurrence_end) = lp_utf16_bounded(bytes, occurrence_at, 36..=36)?;
    let (identity_guid, identity_end) = lp_utf16_bounded(bytes, identity_at, 36..=36)?;
    if !is_guid_relaxed(&occurrence_guid)
        || !is_guid_relaxed(&identity_guid)
        || occurrence_end != identity_at
        || identity_end != identity_at.checked_add(76)?
    {
        return None;
    }
    Some((
        occurrence_guid,
        identity_guid,
        u64::try_from(occurrence_at.checked_add(4)?).ok()?,
        u64::try_from(identity_at.checked_add(4)?).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_header(bytes: &mut [u8], at: usize, class_tag: [u8; 3], record_index: u32) {
        bytes[at..at + 4].copy_from_slice(&3_u32.to_le_bytes());
        bytes[at + 4..at + 7].copy_from_slice(&class_tag);
        bytes[at + 7..at + 11].copy_from_slice(&record_index.to_le_bytes());
    }

    fn write_reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    fn write_lp_utf16(bytes: &mut [u8], at: usize, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        bytes[at..at + 4].copy_from_slice(&(units.len() as u32).to_le_bytes());
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
        write_reference(&mut bytes, class_307_joint_origin::FIRST_REFERENCE, 31);
        write_reference(&mut bytes, class_307_joint_origin::SECOND_REFERENCE, 32);
        write_lp_utf16(
            &mut bytes,
            class_307_joint_origin::IDENTITY_GUID,
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        );
        bytes[class_307_joint_origin::REFERENCE_COUNT..class_307_joint_origin::REFERENCE_COUNT + 4]
            .copy_from_slice(&class_307_joint_origin::REFERENCE_COUNT_VALUE.to_le_bytes());
        for ordinal in 0..class_307_joint_origin::REFERENCE_COUNT_VALUE as usize {
            write_reference(
                &mut bytes,
                class_307_joint_origin::REFERENCE_ENTRIES + ordinal * ASSEMBLY_MARKED_REFERENCE_LEN,
                40 + ordinal as u32,
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
        let records = IndexedRecordOffsets::build(&bytes);
        let frame = DesignAssemblyOperandFrame {
            reference_record_index: record_index,
            reference_offset: 9,
            transform: super::super::identity_matrix(),
            transform_offset: 20,
        };

        assert_eq!(
            exact_class_264_record_frame(
                &bytes,
                &records,
                record_index,
                "307",
                class_307_joint_origin::LEN,
            ),
            Some((0, class_307_joint_origin::LEN))
        );
        assert_eq!(
            lp_utf16_bounded(
                &bytes,
                class_307_joint_origin::KIND_CODE_UNIT_COUNT,
                11..=11,
            ),
            Some((
                "JointOrigin".into(),
                class_307_joint_origin::FEATURE_ORDINAL
            ))
        );
        assert!(matches!(
            exact_class_307_joint_origin(&bytes, &records, &frame),
            Some(DesignAssemblyOperandQualifier::JointOrigin {
                scope_record_index: 17,
                byte_offset: 0,
                paired_byte_offset: 366,
                ..
            })
        ));

        bytes[class_307_joint_origin::REFERENCE_TRAILER] = 0;
        let records = IndexedRecordOffsets::build(&bytes);
        assert_eq!(exact_class_307_joint_origin(&bytes, &records, &frame), None);
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
            let records = IndexedRecordOffsets::build(&bytes);
            assert_eq!(
                exact_class_363_identity_frame(&bytes, &records, record_index),
                Some((0, scope_reference))
            );
            let (occurrence, identity, _, _) =
                exact_class_363_identity_guids(&bytes, 0).expect("identity GUID prefix");
            assert_eq!(occurrence, guid);
            assert_eq!(identity, guid);
        }
    }
}
