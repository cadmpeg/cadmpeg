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
    #[cfg(test)]
    fn prototype(&self) -> String {
        format!("{PROTOTYPE_PREFIX}{}", self.prototype_index.ordinal())
    }

    /// Borrow the serialized form of this occurrence in its roster lane.
    fn view(
        &self,
        occurrence_lane_form: OccurrenceLaneForm,
    ) -> FastLoadComponentOccurrenceView<'_> {
        FastLoadComponentOccurrenceView {
            id: &self.id,
            ordinal: self.ordinal,
            occurrence_lane_form,
            marker: self.marker,
            marker_source_offset: self.marker_source_offset,
            prototype: PrototypeIdentity(self.prototype_index),
            prototype_index: self.prototype_index,
            component_uuid: &self.component_uuid,
            uuid_source_offset: self.uuid_source_offset,
            source_entry: &self.source_entry,
            source_offset: self.source_offset,
        }
    }
}

const PROTOTYPE_PREFIX: &str = "nx:fast-load:prototype#";

/// Prototype identity derived from a one-based roster index.
#[derive(Clone, Copy)]
struct PrototypeIdentity(RosterIndex);

impl PrototypeIdentity {
    /// Whether `text` spells this identity. The ordinal has at most three digits,
    /// so the comparison reads a fixed prefix and at most three digits.
    fn matches(self, text: &str) -> bool {
        let Some((prefix, digits)) = text
            .as_bytes()
            .split_first_chunk::<{ PROTOTYPE_PREFIX.len() }>()
        else {
            return false;
        };
        if prefix != PROTOTYPE_PREFIX.as_bytes() {
            return false;
        }
        let ordinal = self.0 .0 - 1;
        let (hundreds, tens, ones) = (ordinal / 100, ordinal / 10 % 10, ordinal % 10);
        match *digits {
            [only] => hundreds == 0 && tens == 0 && only == b'0' + ones,
            [first, second] => {
                hundreds == 0 && tens != 0 && first == b'0' + tens && second == b'0' + ones
            }
            [first, second, third] => {
                hundreds != 0
                    && first == b'0' + hundreds
                    && second == b'0' + tens
                    && third == b'0' + ones
            }
            _ => false,
        }
    }
}

impl Serialize for PrototypeIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{PROTOTYPE_PREFIX}{}", self.0.ordinal()))
    }
}

/// Borrowed serialized occurrence with its roster-level lane form.
#[derive(Serialize)]
pub(in crate::native) struct FastLoadComponentOccurrenceView<'a> {
    id: &'a str,
    ordinal: u32,
    occurrence_lane_form: OccurrenceLaneForm,
    marker: OccurrenceMarker,
    marker_source_offset: u64,
    prototype: PrototypeIdentity,
    prototype_index: RosterIndex,
    component_uuid: &'a str,
    uuid_source_offset: u64,
    source_entry: &'a str,
    source_offset: u64,
}

#[derive(Deserialize)]
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
        if !PrototypeIdentity(wire.prototype_index).matches(&wire.prototype) {
            return Err("FastLoadComponentOccurrence.prototype disagrees with prototype_index");
        }
        Ok(Self {
            id: wire.id,
            ordinal: wire.ordinal,
            marker: wire.marker,
            marker_source_offset: wire.marker_source_offset,
            prototype_index: wire.prototype_index,
            component_uuid: wire.component_uuid,
            uuid_source_offset: wire.uuid_source_offset,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
        })
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    uuids: &[FastLoadComponentUuid],
    occurrences: &[FastLoadComponentOccurrence],
    object_uuid_values: &[ObjectUuidValue],
) -> Result<Vec<FastLoadComponentObjectGroup>, cadmpeg_core::CodecError> {
    let (uses_by_uuid, _uses_storage) = ctx.collect_scoped_btree_groups(
        occurrences
            .iter()
            .map(|occurrence| (occurrence.component_uuid.as_str(), occurrence.id.as_str())),
        "NX fast-load group occurrences",
    )?;
    let (values_by_uuid, _values_storage) = ctx.collect_scoped_btree_groups(
        object_uuid_values
            .iter()
            .map(|value| (value.uuid.as_str(), value.id.as_str())),
        "NX fast-load group values",
    )?;
    let mut groups = Vec::new();
    for uuid in ctx.admit_iter(uuids, "NX fast-load object groups")? {
        let Some(uses) = ctx.get_btree_map(
            &uses_by_uuid,
            uuid.id.as_str(),
            "NX fast-load group occurrences",
        )?
        else {
            continue;
        };
        let Some(values) = ctx.get_btree_map(
            &values_by_uuid,
            uuid.uuid.as_str(),
            "NX fast-load group values",
        )?
        else {
            continue;
        };
        if uses.len() != values.len() {
            continue;
        }
        let uses =
            ctx.collect_retained_texts(uses.iter().copied(), "allocate NX fast-load group uses")?;
        let values = ctx
            .collect_retained_texts(values.iter().copied(), "allocate NX fast-load group values")?;
        let Some(members) = UuidGroupMembers::new_charged(ctx, uses, values)? else {
            continue;
        };
        ctx.reserve_vec(&mut groups, 1, "NX fast-load object groups")?;
        let uuid_text =
            ctx.copy_retained_text(uuid.uuid.as_str(), "retain NX fast-load group UUID")?;
        groups.push(FastLoadComponentObjectGroup {
            id: structure_identity(
                ctx,
                "nx:fast-load:object-group#",
                uuid.ordinal,
                "retain NX fast-load group identity",
            )?,
            component_uuid: ctx
                .copy_retained_text(&uuid.id, "retain NX fast-load group component UUID")?,
            uuid: crate::canonical_uuid::CanonicalUuid::new(uuid_text)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            members,
            source_entry: ctx
                .copy_retained_text(&uuid.source_entry, "retain NX fast-load group source entry")?,
            source_offset: uuid.source_offset,
        });
    }
    Ok(groups)
}

fn structure_identity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    prefix: &str,
    ordinal: u32,
    operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    ctx.format_retained(format_args!("{prefix}{ordinal}"), operation)
}

fn roster_offset(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entry_offset: u64,
    local_offset: usize,
) -> Result<u64, cadmpeg_core::CodecError> {
    entry_offset
        .checked_add(cadmpeg_core::decode::u64_from_index(envelope::LEN))
        .and_then(|offset| offset.checked_add(cadmpeg_core::decode::u64_from_index(local_offset)))
        .ok_or_else(|| ctx.refuse_codec_limit("NX fast-load roster source offset", 0, 1))
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
fn select_roster_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: impl IntoIterator<Item = Result<Option<Candidate>, CodecError>>,
) -> Result<Option<Candidate>, cadmpeg_core::CodecError> {
    let mut selected: Option<Candidate> = None;
    let mut furthest_end = 0usize;
    for candidate in candidates {
        ctx.charge_work(1, "select NX fast-load roster candidate")?;
        let Some(candidate) = candidate? else {
            continue;
        };
        furthest_end = furthest_end.max(candidate.end);
        if selected.as_ref().is_none_or(|best| {
            candidate.start < best.start
                || (candidate.start == best.start && candidate.end > best.end)
        }) {
            selected = Some(candidate);
        }
    }
    Ok(selected.filter(|candidate| candidate.end == furthest_end))
}

/// Extract the component roster only when its entry and internal frame are
/// unique and every counted lane is complete.
pub(super) fn fast_load_component_roster(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container<'_>,
) -> Result<
    (
        Vec<FastLoadComponentPrototype>,
        Vec<FastLoadComponentUuid>,
        FastLoadOccurrences,
    ),
    cadmpeg_core::CodecError,
> {
    let is_roster_entry = |entry: &crate::container::DirEntry| {
        Ok(entry.name == ENTRY_NAME && entry.file_span().is_some())
    };
    let operation = "locate NX fast-load roster entry";
    let Some(first) = ctx.position_by(&container.entries, is_roster_entry, operation)? else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };
    let (found, rest) = container.entries.split_at(first + 1);
    let (Some(entry), false) = (found.last(), ctx.any_by(rest, is_roster_entry, operation)?) else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };
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
    let anchor_work = payload
        .len()
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("scan NX fast-load roster anchors", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(anchor_work),
        "scan NX fast-load roster anchors",
    )?;
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
        .map(|start| parse_candidate(ctx, payload, start));
    let Some(candidate) = select_roster_candidate(ctx, candidates)? else {
        return Ok((Vec::new(), Vec::new(), FastLoadOccurrences::default()));
    };

    let mut prototypes: Vec<FastLoadComponentPrototype> =
        ctx.vector_storage(candidate.prototype_count, "NX fast-load prototypes")?;
    let mut at = candidate.prototypes_offset;
    for ordinal in ctx.admit_iter(&(0..candidate.prototype_count), "NX fast-load prototypes")? {
        ctx.reserve_vec(&mut prototypes, 1, "NX fast-load prototypes")?;
        let (offset, name) = parse_string(ctx, payload, &mut at)?.ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("NX fast-load prototype parse changed")
        })?;
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX fast-load prototype ordinal", 0, 1))?;
        prototypes.push(FastLoadComponentPrototype {
            id: structure_identity(
                ctx,
                "nx:fast-load:prototype#",
                ordinal,
                "retain NX fast-load prototype identity",
            )?,
            ordinal,
            name: ctx.copy_retained_text(name, "retain NX fast-load prototype name")?,
            source_entry: ctx
                .copy_retained_text(&entry.name, "retain NX fast-load prototype source entry")?,
            source_offset: roster_offset(ctx, entry_offset, offset)?,
        });
    }
    let mut uuids: Vec<FastLoadComponentUuid> =
        ctx.vector_storage(candidate.uuid_count, "NX fast-load UUIDs")?;
    let mut at = candidate.uuids_offset;
    for ordinal in ctx.admit_iter(&(0..candidate.uuid_count), "NX fast-load UUIDs")? {
        ctx.reserve_vec(&mut uuids, 1, "NX fast-load UUIDs")?;
        let (offset, text) = parse_tagged_string(ctx, payload, &mut at, 3)?.ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("NX fast-load UUID parse changed")
        })?;
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX fast-load UUID ordinal", 0, 1))?;
        let text = ctx.copy_retained_text(text, "retain NX fast-load UUID text")?;
        let uuid = crate::canonical_uuid::CanonicalUuid::new(text)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        uuids.push(FastLoadComponentUuid {
            id: structure_identity(
                ctx,
                "nx:fast-load:uuid#",
                ordinal,
                "retain NX fast-load UUID identity",
            )?,
            ordinal,
            uuid,
            source_entry: ctx
                .copy_retained_text(&entry.name, "retain NX fast-load UUID source entry")?,
            source_offset: roster_offset(ctx, entry_offset, offset)?,
        });
    }
    let lane_bytes = |offset: usize| -> Result<&[u8], CodecError> {
        let end = offset
            .checked_add(candidate.occurrence_count)
            .ok_or_else(|| ctx.refuse_codec_limit("NX fast-load occurrence lane", 0, 1))?;
        payload
            .get(offset..end)
            .ok_or_else(|| CodecError::malformed("NX fast-load occurrence lane changed"))
    };
    let markers = lane_bytes(candidate.occurrence_markers_offset)?;
    let prototype_indices = lane_bytes(candidate.occurrences_offset)?;
    let uuid_indices = lane_bytes(candidate.uuid_indices_offset)?;
    let mut records: Vec<FastLoadComponentOccurrence> =
        ctx.vector_storage(candidate.occurrence_count, "NX fast-load occurrences")?;
    for ordinal in ctx.admit_iter(&(0..candidate.occurrence_count), "NX fast-load occurrences")? {
        let (Some(marker), Some(prototype_index), Some(uuid_index)) = (
            markers.get(ordinal),
            prototype_indices.get(ordinal),
            uuid_indices.get(ordinal),
        ) else {
            return Err(CodecError::malformed(
                "NX fast-load occurrence lane changed",
            ));
        };
        ctx.reserve_vec(&mut records, 1, "NX fast-load occurrences")?;
        let marker = OccurrenceMarker::try_from(*marker).map_err(CodecError::malformed)?;
        let prototype_index =
            RosterIndex::try_from(*prototype_index).map_err(CodecError::malformed)?;
        let uuid_index = RosterIndex::try_from(*uuid_index).map_err(CodecError::malformed)?;
        let component_uuid = uuids
            .get(uuid_index.ordinal())
            .ok_or_else(|| CodecError::malformed("NX fast-load UUID index changed"))?;
        let ordinal_u32 = u32::try_from(ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX fast-load occurrence ordinal", 0, 1))?;
        let marker_offset = candidate
            .occurrence_markers_offset
            .checked_add(ordinal)
            .ok_or_else(|| ctx.refuse_codec_limit("NX fast-load marker offset", 0, 1))?;
        let uuid_offset = candidate
            .uuid_indices_offset
            .checked_add(ordinal)
            .ok_or_else(|| ctx.refuse_codec_limit("NX fast-load UUID index offset", 0, 1))?;
        let occurrence_offset = candidate
            .occurrences_offset
            .checked_add(ordinal)
            .ok_or_else(|| ctx.refuse_codec_limit("NX fast-load occurrence offset", 0, 1))?;
        records.push(FastLoadComponentOccurrence {
            id: structure_identity(
                ctx,
                "nx:fast-load:occurrence#",
                ordinal_u32,
                "retain NX fast-load occurrence identity",
            )?,
            ordinal: ordinal_u32,
            marker,
            marker_source_offset: roster_offset(ctx, entry_offset, marker_offset)?,
            prototype_index,
            component_uuid: ctx.copy_retained_text(
                &component_uuid.id,
                "retain NX fast-load occurrence UUID reference",
            )?,
            uuid_source_offset: roster_offset(ctx, entry_offset, uuid_offset)?,
            source_entry: ctx
                .copy_retained_text(&entry.name, "retain NX fast-load occurrence source entry")?,
            source_offset: roster_offset(ctx, entry_offset, occurrence_offset)?,
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

/// Parse one roster candidate at `start`, admitting each scanned lane before
/// it is read. Every count and string length is framed by one byte.
fn parse_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<Candidate>, CodecError> {
    const OPERATION: &str = "scan NX fast-load roster candidate";
    macro_rules! require {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let byte_at = |at: &mut usize| take(bytes, at, 1).and_then(|byte| byte.first().copied());
    let mut at = start;
    require!((byte_at(&mut at) == Some(1)).then_some(()));
    let metadata_count = require!(byte_at(&mut at).and_then(decoded_count));
    let mut first_metadata_nonempty = false;
    for index in ctx.admit_iter(&(0..metadata_count), OPERATION)? {
        let (_, value) = require!(parse_string(ctx, bytes, &mut at)?);
        if index == 0 {
            first_metadata_nonempty = !value.is_empty();
        }
    }
    require!(first_metadata_nonempty.then_some(()));
    require!((take(bytes, &mut at, 2) == Some(&[1, 3])).then_some(()));
    let occurrence_lane_form =
        require!(byte_at(&mut at).and_then(|form| OccurrenceLaneForm::try_from(form).ok()));
    require!((byte_at(&mut at) == Some(0)).then_some(()));

    require!((byte_at(&mut at) == Some(1)).then_some(()));
    let occurrence_count = require!(byte_at(&mut at).and_then(decoded_count));
    let occurrence_markers_offset = at;
    let markers = require!(take(bytes, &mut at, occurrence_count));
    require!(ctx
        .all_by(
            markers,
            |marker| Ok(OccurrenceMarker::try_from(*marker).is_ok()),
            OPERATION,
        )?
        .then_some(()));
    require!((take(bytes, &mut at, 6) == Some(&[1, 2, 0xff, 0xff, 0xff, 0xff])).then_some(()));

    require!((byte_at(&mut at) == Some(1)).then_some(()));
    let prototype_count = require!(byte_at(&mut at).and_then(decoded_count));
    let prototypes_offset = at;
    for _ in ctx.admit_iter(&(0..prototype_count), OPERATION)? {
        require!(parse_string(ctx, bytes, &mut at)?);
    }

    require!((byte_at(&mut at) == Some(1)).then_some(()));
    require!((byte_at(&mut at).and_then(decoded_count) == Some(occurrence_count)).then_some(()));
    let occurrences_offset = at;
    let prototype_indices = require!(take(bytes, &mut at, occurrence_count));
    require!(ctx
        .all_by(
            prototype_indices,
            |index| {
                Ok(RosterIndex::try_from(*index)
                    .is_ok_and(|index| index.ordinal() < prototype_count))
            },
            OPERATION,
        )?
        .then_some(()));

    require!((byte_at(&mut at) == Some(1)).then_some(()));
    let uuid_count = require!(byte_at(&mut at).and_then(decoded_count));
    let uuids_offset = at;
    for _ in ctx.admit_iter(&(0..uuid_count), OPERATION)? {
        let (_, uuid) = require!(parse_tagged_string(ctx, bytes, &mut at, 3)?);
        require!(crate::canonical_uuid::CanonicalUuid::new(uuid).ok());
    }
    require!((byte_at(&mut at) == Some(1)).then_some(()));
    require!((byte_at(&mut at).and_then(decoded_count) == Some(occurrence_count)).then_some(()));
    let uuid_indices_offset = at;
    let uuid_indices = require!(take(bytes, &mut at, occurrence_count));
    require!(ctx
        .all_by(
            uuid_indices,
            |index| {
                Ok(RosterIndex::try_from(*index).is_ok_and(|index| index.ordinal() < uuid_count))
            },
            OPERATION,
        )?
        .then_some(()));

    Ok(Some(Candidate {
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
    }))
}

fn decoded_count(encoded: u8) -> Option<usize> {
    usize::from(encoded)
        .checked_sub(1)
        .filter(|count| *count > 0)
}

fn parse_string<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &'a [u8],
    at: &mut usize,
) -> Result<Option<(usize, &'a str)>, CodecError> {
    parse_tagged_string(ctx, bytes, at, 4)
}

/// Parse one tagged, length-framed, NUL-terminated printable string.
fn parse_tagged_string<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &'a [u8],
    at: &mut usize,
    tag: u8,
) -> Result<Option<(usize, &'a str)>, CodecError> {
    const OPERATION: &str = "scan NX fast-load roster string";
    let offset = *at;
    if take(bytes, at, 1) != Some(&[tag]) {
        return Ok(None);
    }
    let Some(framed_len) = take(bytes, at, 1)
        .and_then(|byte| byte.first().copied())
        .and_then(decoded_count)
    else {
        return Ok(None);
    };
    let Some((&terminator, value)) = take(bytes, at, framed_len).and_then(<[u8]>::split_last)
    else {
        return Ok(None);
    };
    if terminator != 0 || !ctx.all_by(value, |byte| Ok(*byte >= 0x20), OPERATION)? {
        return Ok(None);
    }
    Ok(ctx
        .validate_utf8(value, OPERATION)?
        .ok()
        .map(|value| (offset, value)))
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
        fast_load_component_object_groups, Candidate, FastLoadComponentObjectGroup,
        OccurrenceLaneForm, ENTRY_NAME,
    };
    use crate::container::Container;
    use crate::container::{DirEntry, Region};
    use crate::native::om::object_uuid::ObjectUuidValue;

    fn fast_load_component_roster(
        container: &Container<'_>,
    ) -> Result<
        (
            Vec<super::FastLoadComponentPrototype>,
            Vec<super::FastLoadComponentUuid>,
            FastLoadOccurrences,
        ),
        cadmpeg_core::CodecError,
    > {
        crate::test_support::with_decode_context(|ctx| {
            super::fast_load_component_roster(ctx, container)
        })
    }

    fn select_roster_candidate(candidates: Vec<Candidate>) -> Option<Candidate> {
        crate::test_support::with_decode_context(|ctx| {
            super::select_roster_candidate(
                ctx,
                candidates.into_iter().map(|candidate| Ok(Some(candidate))),
            )
        })
        .expect("default test work limit admits candidate selection")
    }

    #[test]
    fn fast_load_roster_refuses_candidate_work_limit() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 0;
            },
            |ctx| {
                let file = container(payload(&["plate"], &[1]));
                let error = super::fast_load_component_roster(ctx, &file)
                    .expect_err("roster candidate selection needs work");
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
                );
            },
        );
    }

    fn fast_load_roster_refusal(
        configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                configure(policy);
            },
            |ctx| {
                let file = container(payload(&["plate"], &[1]));
                super::fast_load_component_roster(ctx, &file).unwrap_err()
            },
        )
    }

    #[test]
    fn fast_load_roster_refuses_collection_limit() {
        let error = fast_load_roster_refusal(|policy| policy.limits.max_collection_items = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn fast_load_roster_refuses_retained_limit() {
        let error = fast_load_roster_refusal(|policy| policy.limits.max_retained_bytes = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

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
            segment_index: None,
            segment_wrappers: Vec::new(),
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
        let groups = crate::test_support::with_decode_context(|ctx| {
            fast_load_component_object_groups(ctx, &uuids, occurrences.as_slice(), &values)
        })
        .unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members.occurrences().count(), 3);
        assert_eq!(groups[0].members.object_uuid_values().count(), 3);
        assert_eq!(
            groups[0].members.object_uuid_values().nth(1).unwrap(),
            "nx:test:object-uuid#1"
        );

        assert!(
            crate::test_support::with_decode_context(|ctx| fast_load_component_object_groups(
                ctx,
                &uuids,
                occurrences.as_slice(),
                &values[..2]
            ))
            .unwrap()
            .is_empty()
        );
    }

    fn fast_load_object_group_refusal(
        configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        let container = container(payload(&["plate"], &[1]));
        let (_, uuids, occurrences) = fast_load_component_roster(&container).unwrap();
        let value = ObjectUuidValue {
            id: "object-uuid#0".to_string(),
            section_ordinal: 0,
            uuid: uuids[0].uuid.clone(),
            records: crate::om::nonempty::NonEmpty::new(["record#0".to_string()]).unwrap(),
            source_entry: "om".to_string(),
            source_offset: 0,
        };

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                configure(policy);
            },
            |ctx| {
                fast_load_component_object_groups(ctx, &uuids, occurrences.as_slice(), &[value])
                    .unwrap_err()
            },
        )
    }

    #[test]
    fn fast_load_object_groups_refuse_collection_limit() {
        let error = fast_load_object_group_refusal(|policy| policy.limits.max_collection_items = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn fast_load_object_groups_refuse_retained_limit() {
        let error = fast_load_object_group_refusal(|policy| policy.limits.max_retained_bytes = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn fast_load_object_groups_refuse_scoped_limit() {
        let error =
            fast_load_object_group_refusal(|policy| policy.limits.max_materialized_bytes = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
        );
    }

    #[test]
    fn fast_load_object_groups_refuse_work_limit() {
        let error = fast_load_object_group_refusal(|policy| policy.limits.max_work_units = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
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
