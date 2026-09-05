// SPDX-License-Identifier: Apache-2.0
//! Decode fixed local component-occurrence carriers.

use cadmpeg_core::container::ContainerRole;

use cadmpeg_core::CodecError;
use cadmpeg_core::decode::View;

use crate::bytes::{is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded};
use crate::container::ContainerScan;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::ids;
use crate::records::DesignComponentOccurrence;

const BASE_FRAME_LENGTH: usize = 229;
const PLACED_FRAME_LENGTH: usize = 357;

/// Decode exact local component-occurrence records from every Design bulk stream.
pub fn decode_component_occurrences(
    scan: &ContainerScan,
) -> Result<Vec<DesignComponentOccurrence>, CodecError> {
    let mut occurrences = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let scope = ids::native_scope(&entry.name);
        let mut at = 0;
        while let Some(start) = next_indexed_record_offset(bytes, at) {
            if let Some(occurrence) = exact_component_occurrence(bytes, start, &scope) {
                occurrences.push(occurrence);
            }
            at = start.saturating_add(1);
        }
    }
    occurrences.sort_by(|a, b| a.id.cmp(&b.id));
    occurrences.dedup_by(|left, right| left.id == right.id);
    Ok(occurrences)
}

/// Decode one fixed component-occurrence carrier. The class tag is a per-file
/// dynamic value, so the fixed frame identifies the carrier.
pub(crate) fn exact_component_occurrence(
    bytes: &[u8],
    start: usize,
    stream: &str,
) -> Option<DesignComponentOccurrence> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if after_tag != start.checked_add(7)? {
        return None;
    }
    let record_index = View::u32_le_at(bytes, after_tag)?;
    let end = next_indexed_record_offset(bytes, start.checked_add(1)?)?;
    let frame_length = end.checked_sub(start)?;
    if !matches!(frame_length, BASE_FRAME_LENGTH | PLACED_FRAME_LENGTH)
        || bytes.get(start + 11..start + 19)? != [0; 8]
        || bytes.get(start + 19) != Some(&1)
        || View::u32_le_at(bytes, start + 20)? != 1
        || bytes.get(start + 24) != Some(&1)
        || bytes.get(start + 196) != Some(&0)
        || bytes.get(start + 197) != Some(&1)
    {
        return None;
    }
    let component_record_index = View::u64_le_at(bytes, start + 25)?;
    if View::u64_le_at(bytes, start + 198)? != component_record_index {
        return None;
    }
    let occurrence_ordinal = std::num::NonZeroU32::new(View::u32_le_at(bytes, start + 40)?)?;
    let (component_guid, after_component) = lp_utf16_bounded(bytes, start + 44, 36..=36)?;
    let (occurrence_guid, after_occurrence) = lp_utf16_bounded(bytes, start + 120, 36..=36)?;
    if after_component != start + 120
        || after_occurrence != start + 196
        || !is_guid_relaxed(&component_guid)
        || !is_guid_relaxed(&occurrence_guid)
    {
        return None;
    }
    let placement = match frame_length {
        BASE_FRAME_LENGTH => {
            if occurrence_ordinal.get() != 1
                || bytes.get(start + 206..start + 218)? != [0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
                || bytes.get(start + 218) != Some(&1)
                || bytes.get(start + 227..start + 229)? != [0; 2]
            {
                return None;
            }
            crate::records::DesignComponentOccurrencePlacement::Base
        }
        PLACED_FRAME_LENGTH => {
            if (class_tag == "256" && occurrence_ordinal.get() < 2)
                || bytes.get(start + 206..start + 209)? != [0; 3]
                || bytes.get(start + 337..start + 346)? != [0; 9]
                || bytes.get(start + 346) != Some(&1)
                || bytes.get(start + 355..start + 357)? != [0; 2]
            {
                return None;
            }
            let transform = super::scopes::rigid_transform_at(bytes, start + 209)?;
            crate::records::DesignComponentOccurrencePlacement::Explicit {
                ordinal: occurrence_ordinal,
                transform: crate::records::Located { value: transform, offset: u64::try_from(start.checked_add(209)?).ok()? },
            }
        }
        _ => return None,
    };
    Some(DesignComponentOccurrence {
        id: format!("{stream}:design-component-occurrence#{start}"),
        class_tag,
        record_index,
        byte_offset: u64::try_from(start).ok()?,
        component_record_index,
        component_guid,
        component_guid_offset: u64::try_from(start + 48).ok()?,
        occurrence_guid,
        occurrence_guid_offset: u64::try_from(start + 124).ok()?,
        placement,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::trivially_copy_pass_by_ref)]

    use super::exact_component_occurrence;

    const COMPONENT: &str = "a989beb9-467b-4afa-9e90-a9329a2ca258";
    const OCCURRENCE: &str = "f2371d14-7339-4f5c-82a1-50ec8fca5597";

    fn header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn guid(bytes: &mut [u8], at: usize, value: &str) {
        bytes[at..at + 4].copy_from_slice(&36_u32.to_le_bytes());
        for (ordinal, unit) in value.encode_utf16().enumerate() {
            bytes[at + 4 + ordinal * 2..at + 6 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    fn common(frame_length: usize, ordinal: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        header(&mut bytes, b"256", 20);
        bytes.resize(frame_length, 0);
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[24] = 1;
        bytes[25..33].copy_from_slice(&10_u64.to_le_bytes());
        bytes[40..44].copy_from_slice(&ordinal.to_le_bytes());
        guid(&mut bytes, 44, COMPONENT);
        guid(&mut bytes, 120, OCCURRENCE);
        bytes[197] = 1;
        bytes[198..206].copy_from_slice(&10_u64.to_le_bytes());
        bytes
    }

    #[test]
    fn fixed_component_occurrence_frames_distinguish_seed_and_generated_placements() {
        let mut seed = common(229, 1);
        seed[208] = 1;
        seed[218] = 1;
        header(&mut seed, b"333", 21);
        let seed = exact_component_occurrence(&seed, 0, "f3d:Design/BulkStream.dat")
            .expect("seed occurrence");
        assert_eq!(seed.component_guid, COMPONENT);
        assert_eq!(seed.occurrence_guid, OCCURRENCE);
        assert_eq!(seed.occurrence_ordinal(), 1);
        assert_eq!(seed.transform(), None);

        let mut generated = common(357, 2);
        let transform: [[f64; 4]; 4] = [
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 1.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            generated[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        generated[346] = 1;
        header(&mut generated, b"325", 21);
        let generated = exact_component_occurrence(&generated, 0, "f3d:Design/BulkStream.dat")
            .expect("generated occurrence");
        assert_eq!(generated.occurrence_ordinal(), 2);
        assert_eq!(generated.transform().map(|frame| frame.value), Some(transform));
        assert_eq!(generated.transform().map(|frame| frame.offset), Some(209));

        let mut legacy = common(229, 1);
        legacy[4..7].copy_from_slice(b"327");
        legacy[208] = 1;
        legacy[218] = 1;
        header(&mut legacy, b"333", 21);
        let legacy = exact_component_occurrence(&legacy, 0, "f3d:Design/BulkStream.dat")
            .expect("legacy occurrence");
        assert_eq!(legacy.component_guid, COMPONENT);
        assert_eq!(legacy.occurrence_guid, OCCURRENCE);

        let mut legacy_placed = common(357, 1);
        legacy_placed[4..7].copy_from_slice(b"327");
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            legacy_placed[209 + ordinal * 8..217 + ordinal * 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        legacy_placed[346] = 1;
        header(&mut legacy_placed, b"325", 21);
        let legacy_placed =
            exact_component_occurrence(&legacy_placed, 0, "f3d:Design/BulkStream.dat")
                .expect("legacy placed occurrence");
        assert_eq!(legacy_placed.occurrence_ordinal(), 1);
        assert_eq!(legacy_placed.transform().map(|frame| frame.value), Some(transform));

        // The carrier class tag is a per-file dynamic value, so the fixed frame
        // alone identifies the carrier and a third tag reads the same members.
        let mut dynamic_tag = common(357, 1);
        dynamic_tag[4..7].copy_from_slice(b"336");
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            dynamic_tag[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        dynamic_tag[346] = 1;
        header(&mut dynamic_tag, b"325", 21);
        let dynamic_tag = exact_component_occurrence(&dynamic_tag, 0, "f3d:Design/BulkStream.dat")
            .expect("dynamic-tag placed occurrence");
        assert_eq!(dynamic_tag.class_tag, "336");
        assert_eq!(dynamic_tag.occurrence_ordinal(), 1);
        assert_eq!(dynamic_tag.transform().map(|frame| frame.value), Some(transform));

        // A class-256 carrier still cannot use a placed frame for ordinal one.
        let mut placed_seed = common(357, 1);
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            placed_seed[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        placed_seed[346] = 1;
        header(&mut placed_seed, b"325", 21);
        assert!(exact_component_occurrence(&placed_seed, 0, "f3d:Design/BulkStream.dat").is_none());
    }
}
