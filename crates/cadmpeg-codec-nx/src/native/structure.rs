// SPDX-License-Identifier: Apache-2.0
//! Typed records from the bounded fast-load assembly structure stream.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use cadmpeg_core::decode::View;

use crate::container::Container;
use crate::layout::fastload_structure_envelope as envelope;
use crate::native::om::ObjectUuidValue;

const ENTRY_NAME: &str = "/Root/FastLoad/Structure";
const ROSTER_ANCHOR: &[u8] = &[1, 2, 0x42, 0, 1, 2, 4];
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
    /// Byte discriminator for the serialized occurrence lane form.
    pub occurrence_lane_form: u8,
    /// Exact marker byte in the serialized occurrence marker lane.
    pub marker: u8,
    /// Absolute file offset of the occurrence marker.
    pub marker_source_offset: u64,
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
    start: usize,
    end: usize,
    prototypes: Vec<(usize, String)>,
    occurrence_lane_form: u8,
    occurrence_markers_offset: usize,
    occurrence_markers: Vec<u8>,
    occurrences_offset: usize,
    prototype_indices: Vec<u8>,
    uuids: Vec<(usize, String)>,
    uuid_indices_offset: usize,
    uuid_indices: Vec<u8>,
}

/// Resolve candidate parses by physical span before applying the one-roster
/// rule. A valid parse nested inside a larger parse is an interpretation of
/// bytes already owned by that larger candidate, not a second roster. Two
/// disjoint candidates or partially overlapping candidates remain ambiguous.
fn select_roster_candidate(mut candidates: Vec<Candidate>) -> Option<Candidate> {
    candidates.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| right.end.cmp(&left.end))
    });
    let mut selected = Vec::new();
    for candidate in candidates {
        let Some(previous) = selected.last_mut() else {
            selected.push(candidate);
            continue;
        };
        if candidate.start >= previous.end {
            selected.push(candidate);
            continue;
        }
        if candidate.end <= previous.end {
            continue;
        }
        return None;
    }
    let [candidate] = selected.try_into().ok()?;
    Some(candidate)
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

    // Every admitted roster has one of two structural anchors before the
    // complete counted lanes. Search for both before invoking the parser;
    // trying the parser at every byte makes a large opaque structure stream
    // quadratic in its candidate count. A MODEL roster matches both anchors,
    // so deduplicate candidate starts before enforcing uniqueness.
    let mut starts = BTreeSet::new();
    for (anchor_offset, window) in payload.windows(ROSTER_ANCHOR.len()).enumerate() {
        if window == ROSTER_ANCHOR {
            if let Some(start) = anchor_offset.checked_add(4) {
                starts.insert(start);
            }
        }
    }
    for (model_offset, window) in payload.windows(MODEL_FRAME.len()).enumerate() {
        if window == MODEL_FRAME {
            if let Some(start) = model_offset.checked_sub(2) {
                starts.insert(start);
            }
        }
    }
    let candidates = starts
        .into_iter()
        .filter_map(|start| parse_candidate(payload, start))
        .collect::<Vec<_>>();
    let Some(candidate) = select_roster_candidate(candidates) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };

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
            occurrence_lane_form: candidate.occurrence_lane_form,
            marker: candidate.occurrence_markers[ordinal],
            marker_source_offset: entry_offset
                + envelope::LEN as u64
                + candidate.occurrence_markers_offset as u64
                + ordinal as u64,
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
    metadata
        .first()
        .is_some_and(|value| !value.is_empty())
        .then_some(())?;
    take(bytes, &mut at, 2)?.eq(&[1, 3]).then_some(())?;
    let occurrence_lane_form = *take(bytes, &mut at, 1)?.first()?;
    (occurrence_lane_form <= 1).then_some(())?;
    take(bytes, &mut at, 1)?.eq(&[0]).then_some(())?;

    take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
    let occurrence_count = decoded_count(*take(bytes, &mut at, 1)?.first()?)?;
    let occurrence_markers_offset = at;
    let occurrence_markers = take(bytes, &mut at, occurrence_count)?.to_vec();
    occurrence_markers
        .iter()
        .all(|byte| matches!(*byte, b'1' | b'9'))
        .then_some(())?;
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
        start,
        end: at,
        prototypes,
        occurrence_lane_form,
        occurrence_markers_offset,
        occurrence_markers,
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
        let markers = vec![b'9'; indices.len()];
        payload_with_occurrence_lane(names, indices, 0, &markers)
    }

    fn payload_with_occurrence_lane(
        names: &[&str],
        indices: &[u8],
        occurrence_lane_form: u8,
        markers: &[u8],
    ) -> Vec<u8> {
        payload_with_metadata("MODEL", names, indices, occurrence_lane_form, markers)
    }

    fn payload_with_metadata(
        metadata: &str,
        names: &[&str],
        indices: &[u8],
        occurrence_lane_form: u8,
        markers: &[u8],
    ) -> Vec<u8> {
        payload_with_metadata_values(
            &[1, 2, 0x42, 0],
            &[metadata],
            names,
            indices,
            occurrence_lane_form,
            markers,
        )
    }

    fn payload_with_metadata_values(
        preamble: &[u8],
        metadata: &[&str],
        names: &[&str],
        indices: &[u8],
        occurrence_lane_form: u8,
        markers: &[u8],
    ) -> Vec<u8> {
        assert_eq!(indices.len(), markers.len());
        let mut bytes = preamble.to_vec();
        bytes.extend([
            1,
            u8::try_from(metadata.len() + 1).expect("short test metadata list"),
        ]);
        for value in metadata {
            string(&mut bytes, value);
        }
        bytes.extend([
            1,
            3,
            occurrence_lane_form,
            0,
            1,
            u8::try_from(indices.len() + 1).expect("short test occurrence list"),
        ]);
        bytes.extend(markers);
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
            declared_version: Some(0),
            file_tag: 0,
            footer_offset: 0,
            header_entry_count: 1,
            footer_entry_count: 0,
            footer_fingerprint: [0; 4],
            physical_size: len,
            legacy_cfb: false,
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

    fn candidate_span(start: usize, end: usize) -> Candidate {
        Candidate {
            start,
            end,
            prototypes: Vec::new(),
            occurrence_lane_form: 0,
            occurrence_markers_offset: 0,
            occurrence_markers: Vec::new(),
            occurrences_offset: 0,
            prototype_indices: Vec::new(),
            uuids: Vec::new(),
            uuid_indices_offset: 0,
            uuid_indices: Vec::new(),
        }
    }

    #[test]
    fn extracts_repeated_component_occurrences() {
        let container = container(payload(&["plate", "bolt", "nut"], &[1, 2, 2, 3]));
        let (prototypes, uuids, occurrences) = fast_load_component_roster(&container);
        assert_eq!(uuids.len(), 1);
        assert_eq!(occurrences[0].occurrence_lane_form, 0);
        assert_eq!(occurrences[0].marker, b'9');
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
    fn extracts_roster_with_none_metadata() {
        let (prototypes, uuids, occurrences) = fast_load_component_roster(&container(
            payload_with_metadata("None", &["pin", "head"], &[1, 2], 0, b"99"),
        ));
        assert_eq!(
            prototypes
                .iter()
                .map(|prototype| prototype.name.as_str())
                .collect::<Vec<_>>(),
            ["pin", "head"]
        );
        assert_eq!(uuids.len(), 1);
        assert_eq!(occurrences.len(), 2);
        assert_eq!(occurrences[0].prototype_index, 1);
        assert_eq!(occurrences[1].prototype_index, 2);
    }

    #[test]
    fn extracts_roster_with_extended_model_metadata_header() {
        let (prototypes, uuids, occurrences) =
            fast_load_component_roster(&container(payload_with_metadata_values(
                &[1, 1, 2, 0x42, 0],
                &["MODEL", "None"],
                &["gear", "rod"],
                &[1, 2],
                0,
                b"99",
            )));
        assert_eq!(
            prototypes
                .iter()
                .map(|prototype| prototype.name.as_str())
                .collect::<Vec<_>>(),
            ["gear", "rod"]
        );
        assert_eq!(uuids.len(), 1);
        assert_eq!(occurrences.len(), 2);
        assert_eq!(occurrences[1].prototype_index, 2);
    }

    #[test]
    fn extracts_extended_occurrence_form_and_markers() {
        let container = container(payload_with_occurrence_lane(
            &["plate", "bolt"],
            &[1, 2, 2],
            1,
            b"919",
        ));
        let (_, _, occurrences) = fast_load_component_roster(&container);
        assert_eq!(
            occurrences
                .iter()
                .map(|occurrence| occurrence.occurrence_lane_form)
                .collect::<Vec<_>>(),
            [1, 1, 1]
        );
        assert_eq!(
            occurrences
                .iter()
                .map(|occurrence| occurrence.marker)
                .collect::<Vec<_>>(),
            [b'9', b'1', b'9']
        );
        assert_eq!(
            occurrences[1].marker_source_offset,
            occurrences[0].marker_source_offset + 1
        );
    }

    #[test]
    fn rejects_unknown_occurrence_lane_form_atomically() {
        let bytes = payload_with_occurrence_lane(&["plate"], &[1], 2, b"9");
        assert_eq!(
            fast_load_component_roster(&container(bytes)),
            (Vec::new(), Vec::new(), Vec::new())
        );
    }

    #[test]
    fn rejects_unknown_occurrence_marker_atomically() {
        let bytes = payload_with_occurrence_lane(&["plate"], &[1], 0, b"7");
        assert_eq!(
            fast_load_component_roster(&container(bytes)),
            (Vec::new(), Vec::new(), Vec::new())
        );
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

    #[test]
    fn nested_roster_candidate_is_resolved_before_uniqueness() {
        let candidate =
            select_roster_candidate(vec![candidate_span(10, 100), candidate_span(25, 40)])
                .expect("nested candidate is owned by the outer span");
        assert_eq!((candidate.start, candidate.end), (10, 100));
    }

    #[test]
    fn same_start_nested_roster_candidate_is_resolved_before_uniqueness() {
        let candidate =
            select_roster_candidate(vec![candidate_span(10, 40), candidate_span(10, 100)])
                .expect("same-start nested candidate is owned by the outer span");
        assert_eq!((candidate.start, candidate.end), (10, 100));
    }

    #[test]
    fn partially_overlapping_roster_candidates_remain_ambiguous() {
        assert!(
            select_roster_candidate(vec![candidate_span(10, 50), candidate_span(30, 70)]).is_none()
        );
    }
}
