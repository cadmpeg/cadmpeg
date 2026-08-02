// SPDX-License-Identifier: Apache-2.0
//! Decode fixed local component-occurrence carriers.

use cadmpeg_codec_core::le::u32_at;
use cadmpeg_codec_core::CodecError;

use crate::bytes::{is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded};
use crate::container::{role, ContainerScan};
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
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
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
    occurrences.sort_by_key(|occurrence| occurrence.id.clone());
    occurrences.dedup_by(|left, right| left.id == right.id);
    Ok(occurrences)
}

/// Decode one fixed class-256 component-occurrence carrier.
pub(crate) fn exact_component_occurrence(
    bytes: &[u8],
    start: usize,
    stream: &str,
) -> Option<DesignComponentOccurrence> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if class_tag != "256" || after_tag != start.checked_add(7)? {
        return None;
    }
    let record_index = u32_at(bytes, after_tag)?;
    let end = next_indexed_record_offset(bytes, start.checked_add(1)?)?;
    let frame_length = end.checked_sub(start)?;
    if !matches!(frame_length, BASE_FRAME_LENGTH | PLACED_FRAME_LENGTH)
        || bytes.get(start + 11..start + 19)? != [0; 8]
        || bytes.get(start + 19) != Some(&1)
        || u32_at(bytes, start + 20)? != 1
        || bytes.get(start + 24) != Some(&1)
        || bytes.get(start + 196) != Some(&0)
        || bytes.get(start + 197) != Some(&1)
    {
        return None;
    }
    let component_record_index =
        u64::from_le_bytes(bytes.get(start + 25..start + 33)?.try_into().ok()?);
    if u64::from_le_bytes(bytes.get(start + 198..start + 206)?.try_into().ok()?)
        != component_record_index
    {
        return None;
    }
    let occurrence_ordinal = u32_at(bytes, start + 40)?;
    if occurrence_ordinal == 0 {
        return None;
    }
    let (component_guid, after_component) = lp_utf16_bounded(bytes, start + 44, 36..=36)?;
    let (occurrence_guid, after_occurrence) = lp_utf16_bounded(bytes, start + 120, 36..=36)?;
    if after_component != start + 120
        || after_occurrence != start + 196
        || !is_guid_relaxed(&component_guid)
        || !is_guid_relaxed(&occurrence_guid)
    {
        return None;
    }
    let (transform, transform_offset) = match frame_length {
        BASE_FRAME_LENGTH => {
            if occurrence_ordinal != 1
                || bytes.get(start + 206..start + 218)? != [0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
                || bytes.get(start + 218) != Some(&1)
                || bytes.get(start + 227..start + 229)? != [0; 2]
            {
                return None;
            }
            (None, None)
        }
        PLACED_FRAME_LENGTH => {
            if occurrence_ordinal < 2
                || bytes.get(start + 206..start + 209)? != [0; 3]
                || bytes.get(start + 337..start + 346)? != [0; 9]
                || bytes.get(start + 346) != Some(&1)
                || bytes.get(start + 355..start + 357)? != [0; 2]
            {
                return None;
            }
            let transform = super::scopes::rigid_transform_at(bytes, start + 209)?;
            (
                Some(transform),
                Some(u64::try_from(start.checked_add(209)?).ok()?),
            )
        }
        _ => return None,
    };
    Some(DesignComponentOccurrence {
        id: format!("{stream}:design-component-occurrence#{start}"),
        record_index,
        byte_offset: u64::try_from(start).ok()?,
        component_record_index,
        component_guid,
        component_guid_offset: u64::try_from(start + 48).ok()?,
        occurrence_guid,
        occurrence_guid_offset: u64::try_from(start + 124).ok()?,
        occurrence_ordinal,
        transform,
        transform_offset,
    })
}

#[cfg(test)]
mod tests {
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
        assert_eq!(seed.occurrence_ordinal, 1);
        assert_eq!(seed.transform, None);

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
        assert_eq!(generated.occurrence_ordinal, 2);
        assert_eq!(generated.transform, Some(transform));
        assert_eq!(generated.transform_offset, Some(209));
    }
}
