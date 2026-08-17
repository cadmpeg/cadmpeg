// SPDX-License-Identifier: Apache-2.0
//! Typed records from the bounded fast-load assembly structure stream.

use serde::{Deserialize, Serialize};

use cadmpeg_core::decode::View;

use crate::container::Container;
use crate::layout::fastload_structure_envelope as envelope;
use crate::native::om::ObjectUuidValue;

const ENTRY_NAME: &str = "/Root/FastLoad/Structure";
const MODEL_FRAME: &[u8] = &[4, 7, b'M', b'O', b'D', b'E', b'L', 0];

/// One reusable component prototype named by the fast-load structure roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastLoadComponentPrototype {
    /// Globally unique prototype identity.
    pub id: String,
    /// Zero-based position in the serialized prototype table.
    pub ordinal: u32,
    /// Serialized component name.
    pub name: String,
    /// Directory entry containing the roster.
    pub source_entry: String,
    /// Absolute file offset of the name tag.
    pub source_offset: u64,
}

/// One UUID identity in the fast-load component roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastLoadComponentUuid {
    /// Globally unique native UUID-record identity.
    pub id: String,
    /// Zero-based position in the serialized UUID table.
    pub ordinal: u32,
    /// Canonical lowercase UUID text.
    pub uuid: String,
    /// Directory entry containing the UUID table.
    pub source_entry: String,
    /// Absolute file offset of the UUID tag.
    pub source_offset: u64,
}

/// One ordered component use referencing a reusable fast-load prototype.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastLoadComponentOccurrence {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Zero-based position in the serialized occurrence table.
    pub ordinal: u32,
    /// Referenced [`FastLoadComponentPrototype::id`].
    pub prototype: String,
    /// One-based serialized prototype-table index.
    pub prototype_index: u8,
    /// Referenced [`FastLoadComponentUuid::id`].
    pub component_uuid: String,
    /// Absolute file offset of the UUID-table index.
    pub uuid_source_offset: u64,
    /// Directory entry containing the roster.
    pub source_entry: String,
    /// Absolute file offset of the prototype index.
    pub source_offset: u64,
}

/// Equal-cardinality component uses and OM UUID values sharing one UUID.
///
/// The two ordered lists intentionally do not assert an instance-level pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastLoadComponentObjectGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Referenced [`FastLoadComponentUuid::id`].
    pub component_uuid: String,
    /// Canonical lowercase UUID shared by every member.
    pub uuid: String,
    /// Ordered [`FastLoadComponentOccurrence::id`] values.
    pub occurrences: Vec<String>,
    /// Ordered [`ObjectUuidValue::id`] values.
    pub object_uuid_values: Vec<String>,
    /// Directory entry containing the component roster.
    pub source_entry: String,
    /// Absolute file offset of the roster UUID tag.
    pub source_offset: u64,
}

/// Join fast-load occurrences and OM UUID frames only at the UUID group level.
pub fn fast_load_component_object_groups(
    uuids: &[FastLoadComponentUuid],
    occurrences: &[FastLoadComponentOccurrence],
    object_uuid_values: &[ObjectUuidValue],
) -> Vec<FastLoadComponentObjectGroup> {
    uuids
        .iter()
        .filter_map(|uuid| {
            let uses = occurrences
                .iter()
                .filter(|occurrence| occurrence.component_uuid == uuid.id)
                .map(|occurrence| occurrence.id.clone())
                .collect::<Vec<_>>();
            let values = object_uuid_values
                .iter()
                .filter(|value| value.uuid == uuid.uuid)
                .map(|value| value.id.clone())
                .collect::<Vec<_>>();
            (!uses.is_empty() && uses.len() == values.len()).then(|| FastLoadComponentObjectGroup {
                id: format!("nx:fast-load:object-group#{}", uuid.ordinal),
                component_uuid: uuid.id.clone(),
                uuid: uuid.uuid.clone(),
                occurrences: uses,
                object_uuid_values: values,
                source_entry: uuid.source_entry.clone(),
                source_offset: uuid.source_offset,
            })
        })
        .collect()
}

struct Candidate {
    prototypes: Vec<(usize, String)>,
    occurrences_offset: usize,
    prototype_indices: Vec<u8>,
    uuids: Vec<(usize, String)>,
    uuid_indices_offset: usize,
    uuid_indices: Vec<u8>,
}

/// Extract the component roster only when its entry and internal frame are
/// unique and every counted lane is complete.
pub fn fast_load_component_roster(
    container: &Container<'_>,
) -> (
    Vec<FastLoadComponentPrototype>,
    Vec<FastLoadComponentUuid>,
    Vec<FastLoadComponentOccurrence>,
) {
    let mut entries = container
        .entries
        .iter()
        .filter(|entry| entry.name == ENTRY_NAME && entry.file_span.is_some());
    let Some(entry) = entries.next() else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    if entries.next().is_some() {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let Some((entry_offset, entry_size)) = entry.file_span else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let (Ok(entry_offset_usize), Ok(entry_size)) =
        (usize::try_from(entry_offset), usize::try_from(entry_size))
    else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let Some(end) = entry_offset_usize.checked_add(entry_size) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let Some(bytes) = container.data.get(entry_offset_usize..end) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let Some(payload) = framed_payload(bytes) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };

    // Every admitted roster starts with a version/count pair followed by the
    // first metadata string `MODEL`. Search for that mandatory framed value
    // before invoking the complete parser; trying the parser at every byte
    // makes a large opaque structure stream quadratic in its candidate count.
    let mut candidates = payload
        .windows(MODEL_FRAME.len())
        .enumerate()
        .filter(|(_, window)| *window == MODEL_FRAME)
        .filter_map(|(model_offset, _)| {
            let start = model_offset.checked_sub(2)?;
            parse_candidate(payload, start)
        });
    let Some(candidate) = candidates.next() else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    if candidates.next().is_some() {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let prototypes: Vec<_> = candidate
        .prototypes
        .into_iter()
        .enumerate()
        .map(|(ordinal, (offset, name))| FastLoadComponentPrototype {
            id: format!("nx:fast-load:prototype#{ordinal}"),
            ordinal: ordinal as u32,
            name,
            source_entry: entry.name.clone(),
            source_offset: entry_offset + envelope::LEN as u64 + offset as u64,
        })
        .collect();
    let uuids: Vec<_> = candidate
        .uuids
        .into_iter()
        .enumerate()
        .map(|(ordinal, (offset, uuid))| FastLoadComponentUuid {
            id: format!("nx:fast-load:uuid#{ordinal}"),
            ordinal: ordinal as u32,
            uuid,
            source_entry: entry.name.clone(),
            source_offset: entry_offset + envelope::LEN as u64 + offset as u64,
        })
        .collect();
    let occurrences = candidate
        .prototype_indices
        .into_iter()
        .enumerate()
        .map(|(ordinal, prototype_index)| FastLoadComponentOccurrence {
            id: format!("nx:fast-load:occurrence#{ordinal}"),
            ordinal: ordinal as u32,
            prototype: prototypes[usize::from(prototype_index - 1)].id.clone(),
            prototype_index,
            component_uuid: uuids[usize::from(candidate.uuid_indices[ordinal] - 1)]
                .id
                .clone(),
            uuid_source_offset: entry_offset
                + envelope::LEN as u64
                + candidate.uuid_indices_offset as u64
                + ordinal as u64,
            source_entry: entry.name.clone(),
            source_offset: entry_offset
                + envelope::LEN as u64
                + candidate.occurrences_offset as u64
                + ordinal as u64,
        })
        .collect();
    (prototypes, uuids, occurrences)
}

fn framed_payload(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.get(..envelope::PAYLOAD_LEN)? != [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0] {
        return None;
    }
    let payload_len = usize::try_from(View::u32_be_at(bytes, envelope::PAYLOAD_LEN)?).ok()?;
    if payload_len.checked_add(envelope::LEN)? != bytes.len() {
        return None;
    }
    bytes.get(envelope::LEN..)
}

fn parse_candidate(bytes: &[u8], start: usize) -> Option<Candidate> {
    let mut at = start;
    take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
    let metadata_count = decoded_count(*take(bytes, &mut at, 1)?.first()?)?;
    let mut metadata = Vec::with_capacity(metadata_count);
    for _ in 0..metadata_count {
        metadata.push(parse_string(bytes, &mut at)?.1);
    }
    (metadata.first().map(String::as_str) == Some("MODEL")).then_some(())?;
    take(bytes, &mut at, 4)?.eq(&[1, 3, 0, 0]).then_some(())?;

    take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
    let occurrence_count = decoded_count(*take(bytes, &mut at, 1)?.first()?)?;
    let markers = take(bytes, &mut at, occurrence_count)?;
    markers.iter().all(|byte| *byte == b'9').then_some(())?;
    take(bytes, &mut at, 6)?
        .eq(&[1, 2, 0xff, 0xff, 0xff, 0xff])
        .then_some(())?;

    take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
    let prototype_count = decoded_count(*take(bytes, &mut at, 1)?.first()?)?;
    let mut prototypes = Vec::with_capacity(prototype_count);
    for _ in 0..prototype_count {
        prototypes.push(parse_string(bytes, &mut at)?);
    }

    take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
    (decoded_count(*take(bytes, &mut at, 1)?.first()?)? == occurrence_count).then_some(())?;
    let occurrences_offset = at;
    let prototype_indices = take(bytes, &mut at, occurrence_count)?.to_vec();
    let prototype_count = u8::try_from(prototype_count).ok()?;
    prototype_indices
        .iter()
        .all(|index| (1..=prototype_count).contains(index))
        .then_some(())?;

    take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
    let uuid_count = decoded_count(*take(bytes, &mut at, 1)?.first()?)?;
    let mut uuids = Vec::with_capacity(uuid_count);
    for _ in 0..uuid_count {
        let uuid = parse_tagged_string(bytes, &mut at, 3)?;
        crate::om::canonical_uuid_text(&uuid.1).then_some(())?;
        uuids.push(uuid);
    }
    take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
    (decoded_count(*take(bytes, &mut at, 1)?.first()?)? == occurrence_count).then_some(())?;
    let uuid_indices_offset = at;
    let uuid_indices = take(bytes, &mut at, occurrence_count)?.to_vec();
    let uuid_count = u8::try_from(uuid_count).ok()?;
    uuid_indices
        .iter()
        .all(|index| (1..=uuid_count).contains(index))
        .then_some(())?;

    Some(Candidate {
        prototypes,
        occurrences_offset,
        prototype_indices,
        uuids,
        uuid_indices_offset,
        uuid_indices,
    })
}

fn decoded_count(encoded: u8) -> Option<usize> {
    usize::from(encoded)
        .checked_sub(1)
        .filter(|count| *count > 0)
}

fn parse_string(bytes: &[u8], at: &mut usize) -> Option<(usize, String)> {
    parse_tagged_string(bytes, at, 4)
}

fn parse_tagged_string(bytes: &[u8], at: &mut usize, tag: u8) -> Option<(usize, String)> {
    let offset = *at;
    take(bytes, at, 1)?.eq(&[tag]).then_some(())?;
    let framed_len = decoded_count(*take(bytes, at, 1)?.first()?)?;
    let framed = take(bytes, at, framed_len)?;
    let (&terminator, value) = framed.split_last()?;
    value.iter().all(|byte| *byte >= 0x20).then_some(())?;
    (terminator == 0).then_some(())?;
    Some((offset, std::str::from_utf8(value).ok()?.to_owned()))
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = at.checked_add(len)?;
    let value = bytes.get(*at..end)?;
    *at = end;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{DirEntry, Region};
    use std::borrow::Cow;

    fn string(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend([4, u8::try_from(value.len() + 2).expect("short test string")]);
        bytes.extend(value.as_bytes());
        bytes.push(0);
    }

    fn payload(names: &[&str], indices: &[u8]) -> Vec<u8> {
        let mut bytes = vec![1, 2];
        string(&mut bytes, "MODEL");
        bytes.extend([
            1,
            3,
            0,
            0,
            1,
            u8::try_from(indices.len() + 1).expect("short test occurrence list"),
        ]);
        bytes.extend(std::iter::repeat_n(b'9', indices.len()));
        bytes.extend([
            1,
            2,
            0xff,
            0xff,
            0xff,
            0xff,
            1,
            u8::try_from(names.len() + 1).expect("short test prototype list"),
        ]);
        for name in names {
            string(&mut bytes, name);
        }
        bytes.extend([
            1,
            u8::try_from(indices.len() + 1).expect("short test occurrence list"),
        ]);
        bytes.extend(indices);
        bytes.extend([1, 2]);
        bytes.extend([3, 38]);
        bytes.extend(b"01234567-89ab-cdef-0123-456789abcdef");
        bytes.push(0);
        bytes.extend([
            1,
            u8::try_from(indices.len() + 1).expect("short test occurrence list"),
        ]);
        bytes.extend(std::iter::repeat_n(1, indices.len()));
        bytes
    }

    fn container(payload: Vec<u8>) -> Container<'static> {
        let mut data = vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0];
        data.extend(
            u32::try_from(payload.len())
                .expect("short test payload")
                .to_be_bytes(),
        );
        data.extend(payload);
        let len = data.len() as u64;
        Container {
            data: Cow::Owned(data),
            version: 0,
            file_tag: 0,
            footer_offset: 0,
            header_entry_count: 1,
            footer_entry_count: 0,
            footer_fingerprint: [0; 4],
            entries: vec![DirEntry {
                name: ENTRY_NAME.into(),
                region: Region::Header,
                file_span: Some((0, len)),
            }],
            indexed_section_layouts: std::sync::OnceLock::new(),
            om_operation_label_layouts: std::sync::OnceLock::new(),
            om_section_cache: std::sync::OnceLock::new(),
        }
    }

    #[test]
    fn extracts_repeated_component_occurrences() {
        let container = container(payload(&["plate", "bolt", "nut"], &[1, 2, 2, 3]));
        let (prototypes, uuids, occurrences) = fast_load_component_roster(&container);
        assert_eq!(uuids.len(), 1);
        assert_eq!(
            prototypes
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["plate", "bolt", "nut"]
        );
        assert_eq!(
            occurrences
                .iter()
                .map(|o| o.prototype_index)
                .collect::<Vec<_>>(),
            [1, 2, 2, 3]
        );
        assert_eq!(occurrences[1].prototype, occurrences[2].prototype);
    }

    #[test]
    fn groups_equal_uuid_multiplicity_without_pairing_instances() {
        let container = container(payload(&["plate", "bolt"], &[1, 2, 2]));
        let (_, uuids, occurrences) = fast_load_component_roster(&container);
        let values = (0..3)
            .map(|ordinal| ObjectUuidValue {
                id: format!("nx:test:object-uuid#{ordinal}"),
                section_ordinal: 0,
                uuid: uuids[0].uuid.clone(),
                records: vec![format!("nx:test:record#{ordinal}")],
                source_entry: "om".into(),
                source_offset: 200 + ordinal,
            })
            .collect::<Vec<_>>();
        let groups = fast_load_component_object_groups(&uuids, &occurrences, &values);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].occurrences.len(), 3);
        assert_eq!(groups[0].object_uuid_values.len(), 3);
        assert_eq!(groups[0].object_uuid_values[1], "nx:test:object-uuid#1");

        assert!(fast_load_component_object_groups(&uuids, &occurrences, &values[..2]).is_empty());
    }

    #[test]
    fn rejects_out_of_range_prototype_index_atomically() {
        let container = container(payload(&["plate"], &[1, 2]));
        assert_eq!(
            fast_load_component_roster(&container),
            (Vec::new(), Vec::new(), Vec::new())
        );
    }

    #[test]
    fn rejects_mismatched_envelope_length_atomically() {
        let mut container = container(payload(&["plate"], &[1]));
        container.data.to_mut()[11] += 1;
        assert_eq!(
            fast_load_component_roster(&container),
            (Vec::new(), Vec::new(), Vec::new())
        );
    }

    #[test]
    fn rejects_mismatched_occurrence_counts_atomically() {
        let mut bytes = payload(&["plate", "bolt"], &[1, 2]);
        let count_frame = [1, 3];
        let offset = bytes
            .windows(count_frame.len())
            .rposition(|window| window == count_frame)
            .expect("fixture contains terminal occurrence count");
        bytes[offset + 1] = 2;
        assert_eq!(
            fast_load_component_roster(&container(bytes)),
            (Vec::new(), Vec::new(), Vec::new())
        );
    }

    #[test]
    fn rejects_unterminated_string_atomically() {
        let mut bytes = payload(&["plate"], &[1]);
        let terminator = bytes
            .windows(5)
            .position(|window| window == b"MODEL")
            .expect("fixture contains MODEL")
            + 5;
        bytes[terminator] = b'X';
        assert_eq!(
            fast_load_component_roster(&container(bytes)),
            (Vec::new(), Vec::new(), Vec::new())
        );
    }

    #[test]
    fn rejects_multiple_valid_rosters_atomically() {
        let mut first = payload(&["plate"], &[1]);
        first.extend(payload(&["bolt"], &[1]));
        let container = container(first);
        assert_eq!(
            fast_load_component_roster(&container),
            (Vec::new(), Vec::new(), Vec::new())
        );
    }
}
