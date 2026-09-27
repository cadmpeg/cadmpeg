// SPDX-License-Identifier: Apache-2.0
//! Typed records from the saved toggle-information stream.

use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
use std::fmt::Write;

use super::hex::ToggleId;
use crate::container::{Container, EntryContent};

const ENTRY_NAME: &str = "/Root/UG_PART/LastSavedToggleInfoStream";

/// State text stored by one saved toggle-information member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SavedToggleState {
    /// Serialized `On` state.
    On,
    /// Serialized `Off` state.
    Off,
}

/// One named member of the saved toggle-information stream.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "SavedToggleEntryWire")]
pub(super) struct SavedToggleEntry {
    id: String,
    /// Zero-based serialized member order.
    ordinal: u32,
    /// Lowercase 32-hex-digit toggle identity.
    toggle_id: ToggleId,
    /// Record-order-independent identity when the toggle ID is unique in the stream.
    stable_identity: Option<String>,
    /// Exact state selected by the member text.
    state: SavedToggleState,
    /// Absolute file offset of the member-length word.
    source_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static ENTRY_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static ENTRY_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static STREAM_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for SavedToggleEntry {
    fn clone(&self) -> Self {
        ENTRY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            ordinal: self.ordinal,
            toggle_id: self.toggle_id.clone(),
            stable_identity: self.stable_identity.clone(),
            state: self.state,
            source_offset: self.source_offset,
        }
    }
}

struct EntryId(u32);

fn is_canonical_entry_id(id: &str, ordinal: usize) -> bool {
    const PREFIX: &str = "nx:saved-toggle:entry#";
    let Some(decimal) = id.strip_prefix(PREFIX) else {
        return false;
    };
    let mut value = ordinal;
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    decimal.len() == digits && decimal.parse::<usize>() == Ok(ordinal)
}

impl std::fmt::Display for EntryId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "nx:saved-toggle:entry#{}", self.0)
    }
}

impl Serialize for EntryId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

struct EntryIds(u32);

impl Serialize for EntryIds {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut entries = serializer.serialize_seq(usize::try_from(self.0).ok())?;
        for ordinal in 0..self.0 {
            entries.serialize_element(&EntryId(ordinal))?;
        }
        entries.end()
    }
}

#[derive(Serialize)]
struct SavedToggleEntryRef<'a> {
    id: &'a str,
    ordinal: u32,
    toggle_id: &'a ToggleId,
    #[serde(skip_serializing_if = "Option::is_none")]
    stable_identity: Option<&'a str>,
    state: SavedToggleState,
    raw_byte_len: [u8; 2],
    source_offset: u64,
    value_source_offset: u64,
}

impl Serialize for SavedToggleEntry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SavedToggleEntryRef {
            id: &self.id,
            ordinal: self.ordinal,
            toggle_id: &self.toggle_id,
            stable_identity: self.stable_identity.as_deref(),
            state: self.state,
            raw_byte_len: self.state.byte_len().to_le_bytes(),
            source_offset: self.source_offset,
            value_source_offset: self.source_offset + 2,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct SavedToggleEntryWire {
    id: String,
    ordinal: u32,
    toggle_id: ToggleId,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stable_identity"
    )]
    stable_identity: Option<String>,
    state: SavedToggleState,
    raw_byte_len: [u8; 2],
    source_offset: u64,
    value_source_offset: u64,
}
impl TryFrom<SavedToggleEntryWire> for SavedToggleEntry {
    type Error = &'static str;
    fn try_from(wire: SavedToggleEntryWire) -> Result<Self, Self::Error> {
        if !usize::try_from(wire.ordinal)
            .ok()
            .is_some_and(|ordinal| is_canonical_entry_id(&wire.id, ordinal))
        {
            return Err("SavedToggleEntry.id disagrees with ordinal");
        }
        if wire.raw_byte_len != wire.state.byte_len().to_le_bytes() {
            return Err("SavedToggleEntry.raw_byte_len disagrees with state");
        }
        if wire.source_offset.checked_add(2) != Some(wire.value_source_offset) {
            return Err("SavedToggleEntry.value_source_offset disagrees with source_offset");
        }
        Ok(Self {
            id: wire.id,
            ordinal: wire.ordinal,
            toggle_id: wire.toggle_id,
            stable_identity: wire.stable_identity,
            state: wire.state,
            source_offset: wire.source_offset,
        })
    }
}
#[cfg(test)]
impl From<SavedToggleEntry> for SavedToggleEntryWire {
    fn from(value: SavedToggleEntry) -> Self {
        ENTRY_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        let raw_byte_len = value.state.byte_len().to_le_bytes();
        let value_source_offset = value.source_offset + 2;
        Self {
            id: value.id,
            ordinal: value.ordinal,
            toggle_id: value.toggle_id,
            stable_identity: value.stable_identity,
            state: value.state,
            raw_byte_len,
            source_offset: value.source_offset,
            value_source_offset,
        }
    }
}

/// Complete saved toggle-information stream envelope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "SavedToggleStreamWire")]
pub(super) struct SavedToggleStream {
    /// Number of ordered saved-toggle members.
    entry_count: u32,
    /// Exact four-byte terminal word.
    trailer: [u8; 4],
    /// Absolute file offset of the version byte.
    pub(super) source_offset: u64,
    /// Absolute file offset of the terminal word.
    trailer_source_offset: u64,
}

#[derive(Serialize)]
struct SavedToggleStreamRef {
    id: &'static str,
    version: u8,
    raw_count: [u8; 4],
    entries: EntryIds,
    trailer: [u8; 4],
    source_offset: u64,
    trailer_source_offset: u64,
}

impl Serialize for SavedToggleStream {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SavedToggleStreamRef {
            id: Self::id(),
            version: 1,
            raw_count: self.entry_count.to_le_bytes(),
            entries: EntryIds(self.entry_count),
            trailer: self.trailer,
            source_offset: self.source_offset,
            trailer_source_offset: self.trailer_source_offset,
        }
        .serialize(serializer)
    }
}

impl SavedToggleStream {
    /// Native identity of the saved-toggle stream.
    pub(super) const fn id() -> &'static str {
        "nx:saved-toggle:stream#0"
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct SavedToggleStreamWire {
    id: String,
    version: u8,
    raw_count: [u8; 4],
    entries: Vec<String>,
    trailer: [u8; 4],
    source_offset: u64,
    trailer_source_offset: u64,
}
impl TryFrom<SavedToggleStreamWire> for SavedToggleStream {
    type Error = &'static str;
    fn try_from(wire: SavedToggleStreamWire) -> Result<Self, Self::Error> {
        if wire.id != Self::id() {
            return Err("SavedToggleStream.id is not the saved-toggle stream identity");
        }
        if wire.version != 1 {
            return Err("SavedToggleStream.version must be 1");
        }
        let count = u32::try_from(wire.entries.len())
            .map_err(|_| "SavedToggleStream.entries exceeds u32 count")?;
        if wire.raw_count != count.to_le_bytes() {
            return Err("SavedToggleStream.raw_count disagrees with entries");
        }
        if wire
            .entries
            .iter()
            .enumerate()
            .any(|(ordinal, id)| !is_canonical_entry_id(id, ordinal))
        {
            return Err("SavedToggleStream.entries must match their ordinal identities");
        }
        Ok(Self {
            entry_count: count,
            trailer: wire.trailer,
            source_offset: wire.source_offset,
            trailer_source_offset: wire.trailer_source_offset,
        })
    }
}
#[cfg(test)]
impl From<SavedToggleStream> for SavedToggleStreamWire {
    fn from(value: SavedToggleStream) -> Self {
        STREAM_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        let id = SavedToggleStream::id().to_string();
        let version = 1;
        let raw_count = value.entry_count.to_le_bytes();
        Self {
            id,
            version,
            raw_count,
            entries: (0..value.entry_count)
                .map(|ordinal| format!("nx:saved-toggle:entry#{ordinal}"))
                .collect(),
            trailer: value.trailer,
            source_offset: value.source_offset,
            trailer_source_offset: value.trailer_source_offset,
        }
    }
}

impl SavedToggleState {
    fn byte_len(self) -> u16 {
        match self {
            Self::On => 35,
            Self::Off => 36,
        }
    }
}

impl SavedToggleEntry {
    /// Native identity derived from the member ordinal.
    pub(super) fn id(&self) -> &str {
        &self.id
    }

    /// Absolute file offset of the member-length word.
    pub(super) fn source_offset(&self) -> u64 {
        self.source_offset
    }
}

#[derive(Debug)]
struct ParsedToggleStream {
    stream: SavedToggleStream,
    entries: Vec<SavedToggleEntry>,
}

/// Decode the unique complete saved toggle-information stream.
pub(super) fn saved_toggle_records(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<(Vec<SavedToggleStream>, Vec<SavedToggleEntry>), CodecError> {
    let Some((bytes, source_offset)) = saved_toggle_bytes(container) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let Some(parsed) = parse_saved_toggle_stream(ctx, bytes, source_offset)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    ctx.charge_collection_items(1, "store NX saved toggle stream")?;
    ctx.charge_retained(
        std::mem::size_of::<SavedToggleStream>() as u64,
        "retain NX saved toggle stream",
    )?;
    let mut streams = Vec::new();
    streams
        .try_reserve_exact(1)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX saved toggle stream", 0, 1))?;
    streams.push(parsed.stream);
    Ok((streams, parsed.entries))
}

fn saved_toggle_bytes<'a>(container: &'a Container<'_>) -> Option<(&'a [u8], u64)> {
    let mut candidates = container
        .entries
        .iter()
        .filter(|entry| entry.content() == EntryContent::SaveToggleInfo);
    let entry = candidates.next()?;
    if candidates.next().is_some() || entry.name != ENTRY_NAME {
        return None;
    }
    let (source_offset, byte_len) = entry.file_span()?;
    let start = usize::try_from(source_offset).ok()?;
    let byte_len = usize::try_from(byte_len).ok()?;
    let end = start.checked_add(byte_len)?;
    Some((container.data.get(start..end)?, source_offset))
}

/// Whether the canonical saved-toggle entry has a complete admitted grammar.
pub(crate) fn has_complete_saved_toggle_stream(container: &Container) -> bool {
    saved_toggle_bytes(container)
        .and_then(|(bytes, _)| validate_saved_toggle_stream(bytes))
        .is_some()
}

fn validate_saved_toggle_stream(bytes: &[u8]) -> Option<u32> {
    let mut view = View::over_retained(bytes);
    let version = view.u8()?;
    if version != 1 || bytes.len() < 9 {
        return None;
    }
    let entry_count = view.u32_le()?;
    let count = usize::try_from(entry_count).ok()?;
    // The shortest canonical member is a two-byte length plus 32 hex digits,
    // a colon, and `On`. Bound allocation before reading any member lengths.
    if count > (bytes.len() - 9) / 37 {
        return None;
    }
    for _ in 0..count {
        let raw_byte_len = view.array::<2>()?;
        let byte_len = usize::from(View::u16_le_at(&raw_byte_len, 0)?);
        let value = std::str::from_utf8(view.take(byte_len)?).ok()?;
        let (toggle_id, state) = value.rsplit_once(':')?;
        let state = match state {
            "On" => SavedToggleState::On,
            "Off" => SavedToggleState::Off,
            _ => return None,
        };
        if !ToggleId::is_valid(toggle_id) || byte_len != usize::from(state.byte_len()) {
            return None;
        }
    }
    view.array::<4>()?;
    if !view.is_empty() {
        return None;
    }
    Some(entry_count)
}

fn parse_saved_toggle_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    source_offset: u64,
) -> Result<Option<ParsedToggleStream>, CodecError> {
    let Some(entry_count) = validate_saved_toggle_stream(bytes) else {
        return Ok(None);
    };
    let Some(end_offset) = u64::try_from(bytes.len())
        .ok()
        .and_then(|len| source_offset.checked_add(len))
    else {
        return Ok(None);
    };
    let count = usize::try_from(entry_count).map_err(|_| {
        ctx.refuse_codec_limit("index NX saved toggle entries", 0, u64::from(entry_count))
    })?;
    ctx.charge_collection_items(u64::from(entry_count), "store NX saved toggle entries")?;
    let entry_bytes = count
        .checked_mul(std::mem::size_of::<SavedToggleEntry>())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("size NX saved toggle entries", 0, u64::from(entry_count))
        })?;
    ctx.charge_retained(
        u64::try_from(entry_bytes).map_err(|_| {
            ctx.refuse_codec_limit("size NX saved toggle entries", 0, u64::from(entry_count))
        })?,
        "retain NX saved toggle entries",
    )?;
    let mut entries = Vec::new();
    entries.try_reserve_exact(count).map_err(|_| {
        ctx.refuse_codec_limit(
            "allocate NX saved toggle entries",
            0,
            u64::from(entry_count),
        )
    })?;
    let mut view = View::over_retained(bytes);
    let Some(_version) = view.u8() else {
        return Ok(None);
    };
    let Some(_count) = view.u32_le() else {
        return Ok(None);
    };
    for ordinal in 0..entry_count {
        let member_offset = view.position();
        let Some(raw_byte_len) = view.array::<2>() else {
            return Ok(None);
        };
        let Some(byte_len) = View::u16_le_at(&raw_byte_len, 0) else {
            return Ok(None);
        };
        let Some(value) = view.take(usize::from(byte_len)) else {
            return Ok(None);
        };
        let Some(value) = std::str::from_utf8(value).ok() else {
            return Ok(None);
        };
        let Some((toggle_id, state)) = value.rsplit_once(':') else {
            return Ok(None);
        };
        ctx.charge_retained(toggle_id.len() as u64, "retain NX saved toggle identity")?;
        let mut owned_id = String::new();
        owned_id.try_reserve_exact(toggle_id.len()).map_err(|_| {
            ctx.refuse_codec_limit(
                "allocate NX saved toggle identity",
                0,
                toggle_id.len() as u64,
            )
        })?;
        owned_id.push_str(toggle_id);
        let Ok(toggle_id) = ToggleId::try_from(owned_id) else {
            return Ok(None);
        };
        let state = match state {
            "On" => SavedToggleState::On,
            "Off" => SavedToggleState::Off,
            _ => return Ok(None),
        };
        let Some(member_offset) = u64::try_from(member_offset)
            .ok()
            .and_then(|off| source_offset.checked_add(off))
        else {
            return Ok(None);
        };
        const PREFIX: &str = "nx:saved-toggle:entry#";
        let digits = if ordinal == 0 {
            1
        } else {
            ordinal.ilog10() as usize + 1
        };
        let id_len = PREFIX.len() + digits;
        ctx.charge_retained(id_len as u64, "retain NX saved toggle entry id")?;
        let mut id = String::new();
        id.try_reserve_exact(id_len).map_err(|_| {
            ctx.refuse_codec_limit("allocate NX saved toggle entry id", 0, id_len as u64)
        })?;
        id.push_str(PREFIX);
        if write!(&mut id, "{ordinal}").is_err() {
            return Ok(None);
        }
        entries.push(SavedToggleEntry {
            id,
            ordinal,
            toggle_id,
            stable_identity: None,
            state,
            source_offset: member_offset,
        });
    }
    assign_stable_toggle_identities(ctx, &mut entries)?;
    let trailer_at = view.position();
    let Some(trailer) = view.array::<4>() else {
        return Ok(None);
    };
    let Some(trailer_source_offset) = u64::try_from(trailer_at)
        .ok()
        .and_then(|off| source_offset.checked_add(off))
    else {
        return Ok(None);
    };
    if trailer_source_offset > end_offset {
        return Ok(None);
    }
    Ok(Some(ParsedToggleStream {
        stream: SavedToggleStream {
            entry_count,
            trailer,
            source_offset,
            trailer_source_offset,
        },
        entries,
    }))
}

fn assign_stable_toggle_identities(
    ctx: &DecodeContext<'_>,
    entries: &mut [SavedToggleEntry],
) -> Result<(), CodecError> {
    let scratch_bytes = entries
        .len()
        .checked_mul(std::mem::size_of::<(&ToggleId, usize)>() * 4)
        .and_then(|len| u64::try_from(len).ok())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("size NX saved toggle index", 0, entries.len() as u64)
        })?;
    let _reservation = ctx.reserve_scoped(scratch_bytes, "index NX saved toggle identities")?;
    let mut counts = BTreeMap::<ToggleId, usize>::new();
    for entry in entries.iter() {
        ctx.charge_work(1, "index NX saved toggle identities")?;
        if let Some(count) = counts.get_mut(&entry.toggle_id) {
            *count += 1;
        } else {
            ctx.charge_collection_items(1, "index NX saved toggle identities")?;
            counts.insert(entry.toggle_id.clone(), 1);
        }
    }
    for entry in entries.iter_mut() {
        ctx.charge_work(1, "resolve NX saved toggle identity")?;
        if counts.get(&entry.toggle_id) == Some(&1) {
            const PREFIX: &str = "nx:saved-toggle:identity#";
            let byte_len = PREFIX.len() + 32;
            ctx.charge_retained(byte_len as u64, "retain NX stable toggle identity")?;
            let mut identity = String::new();
            identity.try_reserve_exact(byte_len).map_err(|_| {
                ctx.refuse_codec_limit("allocate NX stable toggle identity", 0, byte_len as u64)
            })?;
            identity.push_str(PREFIX);
            identity.push_str(entry.toggle_id.as_str());
            entry.stable_identity = Some(identity);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        SavedToggleEntryWire, SavedToggleState, SavedToggleStreamWire, ENTRY_CLONE_COUNT,
        ENTRY_INTO_WIRE_COUNT, STREAM_INTO_WIRE_COUNT,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn parse_service(bytes: &[u8], source_offset: u64) -> Option<super::ParsedToggleStream> {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        super::parse_saved_toggle_stream(&ctx, bytes, source_offset).unwrap()
    }

    fn stream(members: &[&str], trailer: [u8; 4]) -> Vec<u8> {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
        for member in members {
            bytes.extend_from_slice(&(member.len() as u16).to_le_bytes());
            bytes.extend_from_slice(member.as_bytes());
        }
        bytes.extend_from_slice(&trailer);
        bytes
    }

    #[test]
    fn saved_toggle_entry_borrowed_wire_refuses_before_clone() {
        for members in [
            &["0123456789abcdef0123456789abcdef:Off"][..],
            &[
                "0123456789abcdef0123456789abcdef:On",
                "0123456789abcdef0123456789abcdef:Off",
            ][..],
        ] {
            let bytes = stream(members, [0; 4]);
            let parsed = parse_service(&bytes, 100).unwrap();
            for entry in &parsed.entries {
                let old = SavedToggleEntryWire::from(entry.clone());
                assert_eq!(
                    serde_json::to_vec(entry).unwrap(),
                    serde_json::to_vec(&old).unwrap()
                );
                let expected = serde_json::to_value(&old).unwrap();
                ENTRY_CLONE_COUNT.with(|count| count.set(0));
                ENTRY_INTO_WIRE_COUNT.with(|count| count.set(0));
                cadmpeg_test_support::native_serialization::assert_native_limit(entry, expected);
                ENTRY_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
                ENTRY_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
            }
        }
    }

    #[test]
    fn saved_toggle_stream_borrowed_wire_refuses_before_entry_list_allocation() {
        let bytes = stream(
            &[
                "0123456789abcdef0123456789abcdef:On",
                "fedcba9876543210fedcba9876543210:Off",
            ],
            [0xde, 0xad, 0xbe, 0xef],
        );
        let parsed = parse_service(&bytes, 100).unwrap();
        let old = SavedToggleStreamWire::from(parsed.stream.clone());
        assert_eq!(
            serde_json::to_vec(&parsed.stream).unwrap(),
            serde_json::to_vec(&old).unwrap()
        );
        let expected = serde_json::to_value(&old).unwrap();
        STREAM_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(&parsed.stream, expected);
        STREAM_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn saved_toggle_stream_rejects_noncanonical_entry_sequences() {
        let wire = serde_json::json!({
            "id": "nx:saved-toggle:stream#0", "version": 1, "raw_count": [2, 0, 0, 0],
            "entries": ["nx:saved-toggle:entry#0", "nx:saved-toggle:entry#1"],
            "trailer": [0, 0, 0, 0], "source_offset": 0, "trailer_source_offset": 79
        });
        let stream: super::SavedToggleStream = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(stream).unwrap(), wire);
        for entries in [
            ["nx:saved-toggle:entry#1", "nx:saved-toggle:entry#0"],
            ["nx:saved-toggle:entry#0", "nx:saved-toggle:entry#0"],
            ["other", "nx:saved-toggle:entry#1"],
            ["nx:saved-toggle:entry#00", "nx:saved-toggle:entry#1"],
        ] {
            let mut invalid = wire.clone();
            invalid["entries"] = serde_json::json!(entries);
            assert!(serde_json::from_value::<super::SavedToggleStream>(invalid).is_err());
        }
    }

    #[test]
    fn parses_complete_counted_toggle_stream() {
        let bytes = stream(
            &["0123456789abcdef0123456789abcdef:Off"],
            [0xde, 0xad, 0xbe, 0xef],
        );
        let parsed = parse_service(&bytes, 100).expect("complete stream");
        assert_eq!(
            super::SavedToggleStreamWire::from(parsed.stream.clone()).version,
            1
        );
        assert_eq!(
            super::SavedToggleStreamWire::from(parsed.stream.clone()).raw_count,
            [1, 0, 0, 0]
        );
        assert_eq!(parsed.stream.trailer, [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(parsed.stream.trailer_source_offset, 143);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].state, SavedToggleState::Off);
        assert_eq!(
            parsed.entries[0].stable_identity.as_deref(),
            Some("nx:saved-toggle:identity#0123456789abcdef0123456789abcdef")
        );
        assert_eq!(parsed.entries[0].source_offset, 105);
        let entry_wire = serde_json::to_value(&parsed.entries[0]).unwrap();
        assert_eq!(
            serde_json::from_value::<super::SavedToggleEntry>(entry_wire.clone()).unwrap(),
            parsed.entries[0]
        );
        for (field, invalid) in [
            ("id", serde_json::json!("other")),
            ("id", serde_json::json!("nx:saved-toggle:entry#00")),
            ("raw_byte_len", serde_json::json!([35, 0])),
            ("value_source_offset", serde_json::json!(108)),
        ] {
            let mut wire = entry_wire.clone();
            wire[field] = invalid;
            assert!(serde_json::from_value::<super::SavedToggleEntry>(wire).is_err());
        }
        let stream_wire = serde_json::to_value(&parsed.stream).unwrap();
        assert_eq!(
            serde_json::from_value::<super::SavedToggleStream>(stream_wire.clone()).unwrap(),
            parsed.stream
        );
        for (field, invalid) in [
            ("id", serde_json::json!("other")),
            ("version", serde_json::json!(2)),
            ("raw_count", serde_json::json!([2, 0, 0, 0])),
        ] {
            let mut wire = stream_wire.clone();
            wire[field] = invalid;
            assert!(serde_json::from_value::<super::SavedToggleStream>(wire).is_err());
        }
        assert_eq!(
            super::SavedToggleEntryWire::from(parsed.entries[0].clone()).value_source_offset,
            107
        );
    }

    #[test]
    fn stable_toggle_identity_ignores_member_order_and_state() {
        let first = stream(
            &[
                "0123456789abcdef0123456789abcdef:On",
                "fedcba9876543210fedcba9876543210:Off",
            ],
            [1, 2, 3, 4],
        );
        let reordered = stream(
            &[
                "fedcba9876543210fedcba9876543210:On",
                "0123456789abcdef0123456789abcdef:Off",
            ],
            [1, 2, 3, 4],
        );
        let first = parse_service(&first, 0).expect("first stream");
        let reordered = parse_service(&reordered, 0).expect("reordered stream");
        assert_eq!(
            first.entries[0].stable_identity,
            reordered.entries[1].stable_identity
        );
        assert_eq!(
            first.entries[1].stable_identity,
            reordered.entries[0].stable_identity
        );
    }

    #[test]
    fn duplicate_toggle_ids_have_no_stable_identity() {
        let bytes = stream(
            &[
                "0123456789abcdef0123456789abcdef:On",
                "0123456789abcdef0123456789abcdef:Off",
            ],
            [1, 2, 3, 4],
        );
        let parsed = parse_service(&bytes, 0).expect("complete stream");
        assert!(parsed
            .entries
            .iter()
            .all(|entry| entry.stable_identity.is_none()));
    }

    #[test]
    fn rejects_partial_or_noncanonical_streams_atomically() {
        let complete = stream(&["0123456789abcdef0123456789abcdef:On"], [1, 2, 3, 4]);
        assert!(parse_service(&complete[..complete.len() - 1], 0).is_none());

        let uppercase = stream(&["0123456789ABCDEF0123456789abcdef:On"], [1, 2, 3, 4]);
        assert!(parse_service(&uppercase, 0).is_none());

        let mut wrong_count = complete;
        wrong_count[1] = 2;
        assert!(parse_service(&wrong_count, 0).is_none());
    }

    #[test]
    fn rejects_count_before_count_driven_reservation() {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        assert!(parse_service(&bytes, 0).is_none());
    }

    #[test]
    fn saved_toggle_entry_count_refuses_before_vector_reservation() {
        let bytes = stream(&["0123456789abcdef0123456789abcdef:On"], [0; 4]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_saved_toggle_stream(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "store NX saved toggle entries")
        );
        assert!(parse_service(&bytes, 0).is_some());
    }

    #[test]
    fn saved_toggle_identity_copy_refuses_before_string_allocation() {
        let bytes = stream(&["0123456789abcdef0123456789abcdef:On"], [0; 4]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = std::mem::size_of::<super::SavedToggleEntry>() as u64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_saved_toggle_stream(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain NX saved toggle identity")
        );
        assert!(parse_service(&bytes, 0).is_some());
    }

    #[test]
    fn saved_toggle_index_refuses_before_tree_insertion() {
        let bytes = stream(&["0123456789abcdef0123456789abcdef:On"], [0; 4]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_saved_toggle_stream(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "index NX saved toggle identities")
        );
        assert!(parse_service(&bytes, 0).is_some());
    }

    #[test]
    fn saved_toggle_lookup_refuses_before_index_work() {
        let bytes = stream(&["0123456789abcdef0123456789abcdef:On"], [0; 4]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_saved_toggle_stream(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "index NX saved toggle identities")
        );
        assert!(parse_service(&bytes, 0).is_some());
    }

    #[test]
    fn saved_toggle_entries_refuse_before_retained_vector_allocation() {
        let bytes = stream(&["0123456789abcdef0123456789abcdef:On"], [0; 4]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            std::mem::size_of::<super::SavedToggleEntry>() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_saved_toggle_stream(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain NX saved toggle entries")
        );
        assert!(parse_service(&bytes, 0).is_some());
    }

    #[test]
    fn saved_toggle_entry_id_refuses_before_string_allocation() {
        let bytes = stream(&["0123456789abcdef0123456789abcdef:On"], [0; 4]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = std::mem::size_of::<super::SavedToggleEntry>() as u64
            + 32
            + "nx:saved-toggle:entry#0".len() as u64
            - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_saved_toggle_stream(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain NX saved toggle entry id")
        );
        assert!(parse_service(&bytes, 0).is_some());
    }

    #[test]
    fn saved_toggle_index_refuses_before_scoped_tree_allocation() {
        let bytes = stream(&["0123456789abcdef0123456789abcdef:On"], [0; 4]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes =
            (std::mem::size_of::<(&super::ToggleId, usize)>() * 4 - 1) as u64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_saved_toggle_stream(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "index NX saved toggle identities")
        );
        assert!(parse_service(&bytes, 0).is_some());
    }

    #[test]
    fn saved_toggle_stable_identity_refuses_before_string_allocation() {
        let bytes = stream(&["0123456789abcdef0123456789abcdef:On"], [0; 4]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = std::mem::size_of::<super::SavedToggleEntry>() as u64
            + 32
            + "nx:saved-toggle:entry#0".len() as u64
            + "nx:saved-toggle:identity#0123456789abcdef0123456789abcdef".len() as u64
            - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_saved_toggle_stream(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain NX stable toggle identity")
        );
        assert!(parse_service(&bytes, 0).is_some());
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_stable_identity, String, "stable_identity");
