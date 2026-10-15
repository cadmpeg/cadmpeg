// SPDX-License-Identifier: Apache-2.0
//! Typed records from the bounded fast-load assembly structure stream.

use serde::{Deserialize, Serialize};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::container::Container;
use crate::layout::fastload_structure_envelope as envelope;
use crate::native::om::object_uuid::ObjectUuidValue;

pub(crate) mod occurrences;
mod uuid_group_members;
use occurrences::FastLoadOccurrences;
use uuid_group_members::UuidGroupMembers;

const ENTRY_NAME: &str = "/Root/FastLoad/Structure";
const ROSTER_ANCHOR: &[u8] = &[1, 2, 0x42, 0, 1, 2, 4];
const MODEL_FRAME: &[u8] = &[4, 7, b'M', b'O', b'D', b'E', b'L', 0];

/// One reusable component prototype named by the fast-load structure roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FastLoadComponentPrototype {
    /// Globally unique prototype identity.
    pub(super) id: String,
    /// Zero-based position in the serialized prototype table.
    ordinal: u32,
    /// Serialized component name.
    name: String,
    /// Directory entry containing the roster.
    source_entry: String,
    /// Absolute file offset of the name tag.
    pub(super) source_offset: u64,
}

/// One UUID identity in the fast-load component roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FastLoadComponentUuid {
    /// Globally unique native UUID-record identity.
    pub(super) id: String,
    /// Zero-based position in the serialized UUID table.
    ordinal: u32,
    /// Canonical lowercase UUID text.
    uuid: crate::canonical_uuid::CanonicalUuid<String>,
    /// Directory entry containing the UUID table.
    source_entry: String,
    /// Absolute file offset of the UUID tag.
    pub(super) source_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
enum OccurrenceLaneForm {
    Base,
    Extended,
}

impl TryFrom<u8> for OccurrenceLaneForm {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Base),
            1 => Ok(Self::Extended),
            _ => Err("occurrence_lane_form: expected 0 or 1"),
        }
    }
}

impl From<OccurrenceLaneForm> for u8 {
    fn from(value: OccurrenceLaneForm) -> Self {
        match value {
            OccurrenceLaneForm::Base => 0,
            OccurrenceLaneForm::Extended => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
enum OccurrenceMarker {
    One,
    Nine,
}

impl TryFrom<u8> for OccurrenceMarker {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            49 => Ok(Self::One),
            57 => Ok(Self::Nine),
            _ => Err("marker: expected 49 or 57"),
        }
    }
}

impl From<OccurrenceMarker> for u8 {
    fn from(value: OccurrenceMarker) -> Self {
        match value {
            OccurrenceMarker::One => 49,
            OccurrenceMarker::Nine => 57,
        }
    }
}

/// One-based slot in a count-minus-one roster table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
struct RosterIndex(u8);

impl TryFrom<u8> for RosterIndex {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if (1..=254).contains(&value) {
            Ok(Self(value))
        } else {
            Err("prototype_index/uuid_index: expected 1 through 254")
        }
    }
}

impl From<RosterIndex> for u8 {
    fn from(value: RosterIndex) -> Self {
        value.0
    }
}

impl RosterIndex {
    fn ordinal(self) -> usize {
        usize::from(self.0 - 1)
    }
}

/// One ordered component use referencing a reusable fast-load prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FastLoadComponentOccurrence {
    /// Globally unique occurrence identity.
    pub(super) id: String,
    /// Zero-based position in the serialized occurrence table.
    ordinal: u32,
    /// Exact marker byte in the serialized occurrence marker lane.
    marker: OccurrenceMarker,
    /// Absolute file offset of the occurrence marker.
    marker_source_offset: u64,
    /// One-based serialized prototype-table index.
    prototype_index: RosterIndex,
    /// Referenced [`FastLoadComponentUuid::id`].
    component_uuid: String,
    /// Absolute file offset of the UUID-table index.
    uuid_source_offset: u64,
    /// Directory entry containing the roster.
    source_entry: String,
    /// Absolute file offset of the prototype index.
    pub(super) source_offset: u64,
}

impl FastLoadComponentOccurrence {
    /// Referenced fast-load prototype identity.
    fn prototype(&self) -> String {
        format!("nx:fast-load:prototype#{}", self.prototype_index.ordinal())
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::native) struct FastLoadComponentOccurrenceWire {
    id: String,
    ordinal: u32,
    occurrence_lane_form: OccurrenceLaneForm,
    marker: OccurrenceMarker,
    marker_source_offset: u64,
    prototype: String,
    prototype_index: RosterIndex,
    component_uuid: String,
    uuid_source_offset: u64,
    source_entry: String,
    source_offset: u64,
}
impl TryFrom<FastLoadComponentOccurrenceWire> for FastLoadComponentOccurrence {
    type Error = &'static str;
    fn try_from(wire: FastLoadComponentOccurrenceWire) -> Result<Self, Self::Error> {
        let occurrence = Self {
            id: wire.id,
            ordinal: wire.ordinal,
            marker: wire.marker,
            marker_source_offset: wire.marker_source_offset,
            prototype_index: wire.prototype_index,
            component_uuid: wire.component_uuid,
            uuid_source_offset: wire.uuid_source_offset,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
        };
        if wire.prototype != occurrence.prototype() {
            return Err("FastLoadComponentOccurrence.prototype disagrees with prototype_index");
        }
        Ok(occurrence)
    }
}
impl From<(&FastLoadComponentOccurrence, OccurrenceLaneForm)> for FastLoadComponentOccurrenceWire {
    fn from(
        (value, occurrence_lane_form): (&FastLoadComponentOccurrence, OccurrenceLaneForm),
    ) -> Self {
        Self {
            id: value.id.clone(),
            ordinal: value.ordinal,
            occurrence_lane_form,
            marker: value.marker,
            marker_source_offset: value.marker_source_offset,
            prototype: value.prototype(),
            prototype_index: value.prototype_index,
            component_uuid: value.component_uuid.clone(),
            uuid_source_offset: value.uuid_source_offset,
            source_entry: value.source_entry.clone(),
            source_offset: value.source_offset,
        }
    }
}

/// Equal-cardinality component uses and OM UUID values sharing one UUID.
///
/// The two ordered lists intentionally do not assert an instance-level pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FastLoadComponentObjectGroup {
    /// Globally unique group identity.
    pub(super) id: String,
    /// Referenced [`FastLoadComponentUuid::id`].
    component_uuid: String,
    /// Canonical lowercase UUID shared by every member.
    uuid: crate::canonical_uuid::CanonicalUuid<String>,
    /// Independent ordered occurrence and OM UUID-value lists of equal cardinality.
    #[serde(flatten)]
    members: UuidGroupMembers,
    /// Directory entry containing the component roster.
    source_entry: String,
    /// Absolute file offset of the roster UUID tag.
    pub(super) source_offset: u64,
}

/// Join fast-load occurrences and OM UUID frames only at the UUID group level.
pub(super) fn fast_load_component_object_groups(
    uuids: &[FastLoadComponentUuid],
    occurrences: &[FastLoadComponentOccurrence],
    object_uuid_values: &[ObjectUuidValue],
) -> Result<Vec<FastLoadComponentObjectGroup>, cadmpeg_core::CodecError> {
    let mut groups = Vec::new();
    for uuid in uuids {
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
        let Ok(members) = UuidGroupMembers::from_admitted_lists(uses, values) else {
            continue;
        };
        let uuid_text = uuid.uuid.as_str().to_owned();
        groups.push(FastLoadComponentObjectGroup {
            id: format!("nx:fast-load:object-group#{}", uuid.ordinal),
            component_uuid: uuid.id.clone(),
            uuid: crate::canonical_uuid::CanonicalUuid::new(uuid_text)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            members,
            source_entry: uuid.source_entry.clone(),
            source_offset: uuid.source_offset,
        });
    }
    Ok(groups)
}

fn roster_offset(entry_offset: u64, local_offset: usize) -> Result<u64, CodecError> {
    entry_offset
        .checked_add(cadmpeg_core::decode::u64_from_index(envelope::LEN))
        .and_then(|offset| offset.checked_add(cadmpeg_core::decode::u64_from_index(local_offset)))
        .ok_or_else(|| CodecError::malformed("NX fast-load roster source offset overflows"))
}

struct Candidate {
    start: usize,
    end: usize,
    prototype_count: usize,
    prototypes_offset: usize,
    occurrence_lane_form: OccurrenceLaneForm,
    occurrence_markers_offset: usize,
    occurrences_offset: usize,
    occurrence_count: usize,
    uuid_count: usize,
    uuids_offset: usize,
    uuid_indices_offset: usize,
}

/// Resolve candidate parses by physical span before applying the one-roster
/// rule. A valid parse nested inside a larger parse is an interpretation of
/// bytes already owned by that larger candidate, not a second roster. Two
/// disjoint candidates or partially overlapping candidates remain ambiguous.
fn select_roster_candidate(candidates: impl IntoIterator<Item = Candidate>) -> Option<Candidate> {
    let mut selected: Option<Candidate> = None;
    let mut furthest_end = 0usize;
    for candidate in candidates {
        furthest_end = furthest_end.max(candidate.end);
        if selected.as_ref().is_none_or(|best| {
            candidate.start < best.start
                || (candidate.start == best.start && candidate.end > best.end)
        }) {
            selected = Some(candidate);
        }
    }
    selected.filter(|candidate| candidate.end == furthest_end)
}

/// Extract the component roster only when its entry and internal frame are
/// unique and every counted lane is complete.
pub(super) fn fast_load_component_roster(
    container: &Container<'_>,
) -> Result<
    (
        Vec<FastLoadComponentPrototype>,
        Vec<FastLoadComponentUuid>,
        FastLoadOccurrences,
    ),
    cadmpeg_core::CodecError,
> {
    let mut entries = container
        .entries
        .iter()
        .filter(|entry| entry.name == ENTRY_NAME && entry.file_span().is_some());
    let Some(entry) = entries.next() else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };
    if entries.next().is_some() {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    }
    let Some((entry_offset, entry_size)) = entry.file_span() else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };
    let (Ok(entry_offset_usize), Ok(entry_size)) =
        (usize::try_from(entry_offset), usize::try_from(entry_size))
    else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };
    let Some(end) = entry_offset_usize.checked_add(entry_size) else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };
    let Some(bytes) = container.data.get(entry_offset_usize..end) else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };
    let Some(payload) = framed_payload(bytes) else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };

    // Every admitted roster has one of two structural anchors before the
    // complete counted lanes. Search for both before invoking the parser;
    // trying the parser at every byte makes a large opaque structure stream
    // quadratic in its candidate count. A MODEL roster can match both anchors;
    // repeated starts produce the same parse and do not alter span selection.
    let candidates = payload
        .windows(ROSTER_ANCHOR.len())
        .enumerate()
        .filter_map(|(offset, window)| {
            (window == ROSTER_ANCHOR)
                .then(|| offset.checked_add(4))
                .flatten()
        })
        .chain(
            payload
                .windows(MODEL_FRAME.len())
                .enumerate()
                .filter_map(|(offset, window)| {
                    (window == MODEL_FRAME)
                        .then(|| offset.checked_sub(2))
                        .flatten()
                }),
        )
        .filter_map(|start| parse_candidate(payload, start));
    let Some(candidate) = select_roster_candidate(candidates) else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };

    let mut prototypes = Vec::new();
    let mut at = candidate.prototypes_offset;
    for ordinal in 0..candidate.prototype_count {
        let (offset, name) = parse_string(payload, &mut at).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("NX fast-load prototype parse changed")
        })?;
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("NX fast-load prototype ordinal exceeds u32"))?;
        prototypes.push(FastLoadComponentPrototype {
            id: format!("nx:fast-load:prototype#{ordinal}"),
            ordinal,
            name: name.to_owned(),
            source_entry: entry.name.clone(),
            source_offset: roster_offset(entry_offset, offset)?,
        });
    }
    let mut uuids = Vec::new();
    let mut at = candidate.uuids_offset;
    for ordinal in 0..candidate.uuid_count {
        let (offset, text) = parse_tagged_string(payload, &mut at, 3).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("NX fast-load UUID parse changed")
        })?;
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("NX fast-load UUID ordinal exceeds u32"))?;
        let text = text.to_owned();
        let uuid = crate::canonical_uuid::CanonicalUuid::new(text)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        uuids.push(FastLoadComponentUuid {
            id: format!("nx:fast-load:uuid#{ordinal}"),
            ordinal,
            uuid,
            source_entry: entry.name.clone(),
            source_offset: roster_offset(entry_offset, offset)?,
        });
    }
    let lane_bytes = |offset: usize| -> Result<&[u8], CodecError> {
        let end = offset
            .checked_add(candidate.occurrence_count)
            .ok_or_else(|| CodecError::malformed("NX fast-load occurrence lane overflows"))?;
        payload
            .get(offset..end)
            .ok_or_else(|| CodecError::malformed("NX fast-load occurrence lane changed"))
    };
    let markers = lane_bytes(candidate.occurrence_markers_offset)?;
    let prototype_indices = lane_bytes(candidate.occurrences_offset)?;
    let uuid_indices = lane_bytes(candidate.uuid_indices_offset)?;
    let mut records = Vec::new();
    for (ordinal, ((marker, prototype_index), uuid_index)) in markers
        .iter()
        .zip(prototype_indices)
        .zip(uuid_indices)
        .enumerate()
    {
        let marker = OccurrenceMarker::try_from(*marker).map_err(CodecError::malformed)?;
        let prototype_index =
            RosterIndex::try_from(*prototype_index).map_err(CodecError::malformed)?;
        let uuid_index = RosterIndex::try_from(*uuid_index).map_err(CodecError::malformed)?;
        let component_uuid = uuids
            .get(uuid_index.ordinal())
            .ok_or_else(|| CodecError::malformed("NX fast-load UUID index changed"))?;
        let ordinal_u32 = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("NX fast-load occurrence ordinal exceeds u32"))?;
        let marker_offset = candidate
            .occurrence_markers_offset
            .checked_add(ordinal)
            .ok_or_else(|| CodecError::malformed("NX fast-load marker offset overflows"))?;
        let uuid_offset = candidate
            .uuid_indices_offset
            .checked_add(ordinal)
            .ok_or_else(|| CodecError::malformed("NX fast-load UUID index offset overflows"))?;
        let occurrence_offset = candidate
            .occurrences_offset
            .checked_add(ordinal)
            .ok_or_else(|| CodecError::malformed("NX fast-load occurrence offset overflows"))?;
        records.push(FastLoadComponentOccurrence {
            id: format!("nx:fast-load:occurrence#{ordinal_u32}"),
            ordinal: ordinal_u32,
            marker,
            marker_source_offset: roster_offset(entry_offset, marker_offset)?,
            prototype_index,
            component_uuid: component_uuid.id.clone(),
            uuid_source_offset: roster_offset(entry_offset, uuid_offset)?,
            source_entry: entry.name.clone(),
            source_offset: roster_offset(entry_offset, occurrence_offset)?,
        });
    }
    let occurrences = FastLoadOccurrences::from_decoded(candidate.occurrence_lane_form, records)?;
    Ok((prototypes, uuids, occurrences))
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
    {
        take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
        let metadata_count = decoded_count(*take(bytes, &mut at, 1)?.first()?)?;
        let mut first_metadata_nonempty = false;
        for index in 0..metadata_count {
            let (_, value) = parse_string(bytes, &mut at)?;
            if index == 0 {
                first_metadata_nonempty = !value.is_empty();
            }
        }
        first_metadata_nonempty.then_some(())?;
        take(bytes, &mut at, 2)?.eq(&[1, 3]).then_some(())?;
        let occurrence_lane_form =
            OccurrenceLaneForm::try_from(*take(bytes, &mut at, 1)?.first()?).ok()?;
        take(bytes, &mut at, 1)?.eq(&[0]).then_some(())?;

        take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
        let occurrence_count = decoded_count(*take(bytes, &mut at, 1)?.first()?)?;
        let occurrence_markers_offset = at;
        take(bytes, &mut at, occurrence_count)?
            .iter()
            .all(|marker| OccurrenceMarker::try_from(*marker).is_ok())
            .then_some(())?;
        take(bytes, &mut at, 6)?
            .eq(&[1, 2, 0xff, 0xff, 0xff, 0xff])
            .then_some(())?;

        take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
        let prototype_count = decoded_count(*take(bytes, &mut at, 1)?.first()?)?;
        let prototypes_offset = at;
        for _ in 0..prototype_count {
            parse_string(bytes, &mut at)?;
        }

        take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
        (decoded_count(*take(bytes, &mut at, 1)?.first()?)? == occurrence_count).then_some(())?;
        let occurrences_offset = at;
        take(bytes, &mut at, occurrence_count)?
            .iter()
            .all(|index| {
                RosterIndex::try_from(*index).is_ok_and(|index| index.ordinal() < prototype_count)
            })
            .then_some(())?;

        take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
        let uuid_count = decoded_count(*take(bytes, &mut at, 1)?.first()?)?;
        let uuids_offset = at;
        for _ in 0..uuid_count {
            let uuid = parse_tagged_string(bytes, &mut at, 3)?;
            crate::canonical_uuid::CanonicalUuid::new(uuid.1).ok()?;
        }
        take(bytes, &mut at, 1)?.eq(&[1]).then_some(())?;
        (decoded_count(*take(bytes, &mut at, 1)?.first()?)? == occurrence_count).then_some(())?;
        let uuid_indices_offset = at;
        take(bytes, &mut at, occurrence_count)?
            .iter()
            .all(|index| {
                RosterIndex::try_from(*index).is_ok_and(|index| index.ordinal() < uuid_count)
            })
            .then_some(())?;

        Some(Candidate {
            start,
            end: at,
            prototype_count,
            prototypes_offset,
            occurrence_lane_form,
            occurrence_markers_offset,
            occurrences_offset,
            occurrence_count,
            uuid_count,
            uuids_offset,
            uuid_indices_offset,
        })
    }
}

fn decoded_count(encoded: u8) -> Option<usize> {
    usize::from(encoded)
        .checked_sub(1)
        .filter(|count| *count > 0)
}

fn parse_string<'a>(bytes: &'a [u8], at: &mut usize) -> Option<(usize, &'a str)> {
    parse_tagged_string(bytes, at, 4)
}

fn parse_tagged_string<'a>(bytes: &'a [u8], at: &mut usize, tag: u8) -> Option<(usize, &'a str)> {
    let offset = *at;
    take(bytes, at, 1)?.eq(&[tag]).then_some(())?;
    let framed_len = decoded_count(*take(bytes, at, 1)?.first()?)?;
    let framed = take(bytes, at, framed_len)?;
    let (&terminator, value) = framed.split_last()?;
    value.iter().all(|byte| *byte >= 0x20).then_some(())?;
    (terminator == 0).then_some(())?;
    Some((offset, std::str::from_utf8(value).ok()?))
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = at.checked_add(len)?;
    let value = bytes.get(*at..end)?;
    *at = end;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::occurrences::FastLoadOccurrences;
    use super::{
        fast_load_component_object_groups, fast_load_component_roster, select_roster_candidate,
        Candidate, FastLoadComponentObjectGroup, OccurrenceLaneForm, ENTRY_NAME,
    };
    use crate::container::Container;
    use crate::container::{DirEntry, Region};
    use crate::native::om::object_uuid::ObjectUuidValue;

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
        let len = cadmpeg_core::decode::u64_from_index(data.len());
        Container {
            data: data.into(),
            physical_size: len,
            layout: crate::container::test_modern_layout(0x06),
            entries: vec![DirEntry {
                name: ENTRY_NAME.into(),
                region: Region::Header,
                body: crate::container::DirEntryBody::File { offset: 0, len },
            }],
            fastload_table: None,
            indexed_section_layouts: std::sync::OnceLock::new(),
            om_section_cache: std::sync::OnceLock::new(),
        }
    }

    fn candidate_span(start: usize, end: usize) -> Candidate {
        Candidate {
            start,
            end,
            prototype_count: 0,
            prototypes_offset: 0,
            occurrence_lane_form: OccurrenceLaneForm::Base,
            occurrence_markers_offset: 0,
            occurrences_offset: 0,
            occurrence_count: 0,
            uuid_count: 0,
            uuids_offset: 0,
            uuid_indices_offset: 0,
        }
    }

    #[test]
    fn extracts_repeated_component_occurrences() {
        let container = container(payload(&["plate", "bolt", "nut"], &[1, 2, 2, 3]));
        let (prototypes, uuids, occurrences) = fast_load_component_roster(&container).unwrap();
        assert_eq!(uuids.len(), 1);
        assert_eq!(
            u8::from(
                occurrences
                    .wire_records()
                    .next()
                    .unwrap()
                    .occurrence_lane_form
            ),
            0
        );
        assert_eq!(u8::from(occurrences.as_slice()[0].marker), b'9');
        assert_eq!(
            prototypes
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["plate", "bolt", "nut"]
        );
        assert_eq!(
            occurrences
                .as_slice()
                .iter()
                .map(|o| u8::from(o.prototype_index))
                .collect::<Vec<_>>(),
            [1, 2, 2, 3]
        );
        assert_eq!(
            occurrences.as_slice()[1].prototype(),
            occurrences.as_slice()[2].prototype()
        );
    }

    #[test]
    fn extracts_roster_with_none_metadata() {
        let (prototypes, uuids, occurrences) = fast_load_component_roster(&container(
            payload_with_metadata("None", &["pin", "head"], &[1, 2], 0, b"99"),
        ))
        .unwrap();
        assert_eq!(
            prototypes
                .iter()
                .map(|prototype| prototype.name.as_str())
                .collect::<Vec<_>>(),
            ["pin", "head"]
        );
        assert_eq!(uuids.len(), 1);
        assert_eq!(occurrences.as_slice().len(), 2);
        assert_eq!(u8::from(occurrences.as_slice()[0].prototype_index), 1);
        assert_eq!(u8::from(occurrences.as_slice()[1].prototype_index), 2);
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
            )))
            .unwrap();
        assert_eq!(
            prototypes
                .iter()
                .map(|prototype| prototype.name.as_str())
                .collect::<Vec<_>>(),
            ["gear", "rod"]
        );
        assert_eq!(uuids.len(), 1);
        assert_eq!(occurrences.as_slice().len(), 2);
        assert_eq!(u8::from(occurrences.as_slice()[1].prototype_index), 2);
    }

    #[test]
    fn extracts_extended_occurrence_form_and_markers() {
        let container = container(payload_with_occurrence_lane(
            &["plate", "bolt"],
            &[1, 2, 2],
            1,
            b"919",
        ));
        let (_, _, occurrences) = fast_load_component_roster(&container).unwrap();
        assert_eq!(
            occurrences
                .wire_records()
                .map(|occurrence| u8::from(occurrence.occurrence_lane_form))
                .collect::<Vec<_>>(),
            [1, 1, 1]
        );
        assert_eq!(
            occurrences
                .as_slice()
                .iter()
                .map(|occurrence| u8::from(occurrence.marker))
                .collect::<Vec<_>>(),
            [b'9', b'1', b'9']
        );
        assert_eq!(
            occurrences.as_slice()[1].marker_source_offset,
            occurrences.as_slice()[0].marker_source_offset + 1
        );
    }

    #[test]
    fn rejects_unknown_occurrence_lane_form_atomically() {
        let bytes = payload_with_occurrence_lane(&["plate"], &[1], 2, b"9");
        assert_eq!(
            fast_load_component_roster(&container(bytes)).unwrap(),
            (Vec::new(), Vec::new(), FastLoadOccurrences::default())
        );
    }

    #[test]
    fn rejects_unknown_occurrence_marker_atomically() {
        let bytes = payload_with_occurrence_lane(&["plate"], &[1], 0, b"7");
        assert_eq!(
            fast_load_component_roster(&container(bytes)).unwrap(),
            (Vec::new(), Vec::new(), FastLoadOccurrences::default())
        );
    }

    #[test]
    fn groups_equal_uuid_multiplicity_without_pairing_instances() {
        let container = container(payload(&["plate", "bolt"], &[1, 2, 2]));
        let (_, uuids, occurrences) = fast_load_component_roster(&container).unwrap();
        let values = (0..3)
            .map(|ordinal| ObjectUuidValue {
                id: format!("nx:test:object-uuid#{ordinal}"),
                section_ordinal: 0,
                uuid: uuids[0].uuid.clone(),
                records: crate::om::nonempty::NonEmpty::new([format!("nx:test:record#{ordinal}")])
                    .unwrap(),
                source_entry: "om".into(),
                source_offset: 200 + ordinal,
            })
            .collect::<Vec<_>>();
        let groups =
            fast_load_component_object_groups(&uuids, occurrences.as_slice(), &values).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members.occurrences().count(), 3);
        assert_eq!(groups[0].members.object_uuid_values().count(), 3);
        assert_eq!(
            groups[0].members.object_uuid_values().nth(1).unwrap(),
            "nx:test:object-uuid#1"
        );

        assert!(
            fast_load_component_object_groups(&uuids, occurrences.as_slice(), &values[..2])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_out_of_range_prototype_index_atomically() {
        let container = container(payload(&["plate"], &[1, 2]));
        assert_eq!(
            fast_load_component_roster(&container).unwrap(),
            (Vec::new(), Vec::new(), FastLoadOccurrences::default())
        );
    }

    #[test]
    fn rejects_mismatched_envelope_length_atomically() {
        let mut container = container(payload(&["plate"], &[1]));
        container.data.to_mut()[11] += 1;
        assert_eq!(
            fast_load_component_roster(&container).unwrap(),
            (Vec::new(), Vec::new(), FastLoadOccurrences::default())
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
            fast_load_component_roster(&container(bytes)).unwrap(),
            (Vec::new(), Vec::new(), FastLoadOccurrences::default())
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
            fast_load_component_roster(&container(bytes)).unwrap(),
            (Vec::new(), Vec::new(), FastLoadOccurrences::default())
        );
    }

    #[test]
    fn rejects_multiple_valid_rosters_atomically() {
        let mut first = payload(&["plate"], &[1]);
        first.extend(payload(&["bolt"], &[1]));
        let container = container(first);
        assert_eq!(
            fast_load_component_roster(&container).unwrap(),
            (Vec::new(), Vec::new(), FastLoadOccurrences::default())
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

    #[test]
    fn occurrence_wire_preserves_closed_source_bytes() {
        let json = r#"{"id":"occurrence","ordinal":0,"occurrence_lane_form":1,"marker":49,"marker_source_offset":12,"prototype":"nx:fast-load:prototype#253","prototype_index":254,"component_uuid":"uuid","uuid_source_offset":20,"source_entry":"om","source_offset":16}"#;
        let occurrences: FastLoadOccurrences = serde_json::from_str(&format!("[{json}]")).unwrap();
        let occurrence = occurrences.wire_records().next().unwrap();
        assert_eq!(serde_json::to_string(&occurrence).unwrap(), json);
        for (field, invalid) in [
            ("occurrence_lane_form", 2),
            ("occurrence_lane_form", 255),
            ("marker", 0),
            ("marker", 50),
            ("prototype_index", 0),
            ("prototype_index", 255),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[field] = invalid.into();
            assert!(
                serde_json::from_value::<FastLoadOccurrences>(serde_json::json!([wire]))
                    .unwrap_err()
                    .to_string()
                    .contains(field)
            );
        }
        let invalid = json.replace("nx:fast-load:prototype#253", "nx:fast-load:prototype#252");
        assert!(serde_json::from_str::<FastLoadOccurrences>(&format!("[{invalid}]")).is_err());
    }

    #[test]
    fn uuid_group_preserves_flat_wire_and_rejects_missing_members() {
        let json = r#"{"id":"group","component_uuid":"component","uuid":"00000000-0000-0000-0000-000000000000","occurrences":["use"],"object_uuid_values":["value"],"source_entry":"om","source_offset":12}"#;
        let group: FastLoadComponentObjectGroup = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&group).unwrap(), json);
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["object_uuid_values"] = serde_json::json!([]);
        assert!(serde_json::from_value::<FastLoadComponentObjectGroup>(wire)
            .unwrap_err()
            .to_string()
            .contains("occurrences/object_uuid_values"));
    }
}
