// SPDX-License-Identifier: Apache-2.0
//! Typed records from the saved toggle-information stream.

use serde::{Deserialize, Serialize};

use cadmpeg_core::decode::View;
use std::collections::BTreeMap;

use super::hex::ToggleId;
use crate::container::{Container, EntryContent};

const ENTRY_NAME: &str = "/Root/UG_PART/LastSavedToggleInfoStream";

/// State text stored by one saved toggle-information member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedToggleState {
    /// Serialized `On` state.
    On,
    /// Serialized `Off` state.
    Off,
}

/// One named member of the saved toggle-information stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SavedToggleEntryWire", into = "SavedToggleEntryWire")]
pub struct SavedToggleEntry {
    /// Zero-based serialized member order.
    pub ordinal: u32,
    /// Lowercase 32-hex-digit toggle identity.
    toggle_id: ToggleId,
    /// Record-order-independent identity when the toggle ID is unique in the stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_identity: Option<String>,
    /// Exact state selected by the member text.
    pub state: SavedToggleState,
    /// Absolute file offset of the member-length word.
    source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct SavedToggleEntryWire {
    id: String,
    ordinal: u32,
    toggle_id: ToggleId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stable_identity: Option<String>,
    state: SavedToggleState,
    raw_byte_len: [u8; 2],
    source_offset: u64,
    value_source_offset: u64,
}
impl TryFrom<SavedToggleEntryWire> for SavedToggleEntry {
    type Error = &'static str;
    fn try_from(wire: SavedToggleEntryWire) -> Result<Self, Self::Error> {
        if wire.id != format!("nx:saved-toggle:entry#{}", wire.ordinal) {
            return Err("SavedToggleEntry.id disagrees with ordinal");
        }
        if wire.raw_byte_len != wire.state.byte_len().to_le_bytes() {
            return Err("SavedToggleEntry.raw_byte_len disagrees with state");
        }
        if wire.source_offset.checked_add(2) != Some(wire.value_source_offset) {
            return Err("SavedToggleEntry.value_source_offset disagrees with source_offset");
        }
        Ok(Self {
            ordinal: wire.ordinal,
            toggle_id: wire.toggle_id,
            stable_identity: wire.stable_identity,
            state: wire.state,
            source_offset: wire.source_offset,
        })
    }
}
impl From<SavedToggleEntry> for SavedToggleEntryWire {
    fn from(value: SavedToggleEntry) -> Self {
        let id = value.id();
        let raw_byte_len = value.state.byte_len().to_le_bytes();
        let value_source_offset = value.source_offset + 2;
        Self {
            id,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SavedToggleStreamWire", into = "SavedToggleStreamWire")]
pub struct SavedToggleStream {
    /// Number of ordered saved-toggle members.
    entry_count: u32,
    /// Exact four-byte terminal word.
    pub trailer: [u8; 4],
    /// Absolute file offset of the version byte.
    pub source_offset: u64,
    /// Absolute file offset of the terminal word.
    pub trailer_source_offset: u64,
}

impl SavedToggleStream {
    /// Native identity of the saved-toggle stream.
    pub const fn id() -> &'static str {
        "nx:saved-toggle:stream#0"
    }
}

#[derive(Serialize, Deserialize)]
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
            .any(|(ordinal, id)| *id != format!("nx:saved-toggle:entry#{ordinal}"))
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
impl From<SavedToggleStream> for SavedToggleStreamWire {
    fn from(value: SavedToggleStream) -> Self {
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
    pub fn id(&self) -> String {
        format!("nx:saved-toggle:entry#{}", self.ordinal)
    }

    /// Absolute file offset of the member-length word.
    pub fn source_offset(&self) -> u64 {
        self.source_offset
    }
}

struct ParsedToggleStream {
    stream: SavedToggleStream,
    entries: Vec<SavedToggleEntry>,
}

/// Decode the unique complete saved toggle-information stream.
pub fn saved_toggle_records(
    container: &Container,
) -> (Vec<SavedToggleStream>, Vec<SavedToggleEntry>) {
    let mut candidates = container
        .entries
        .iter()
        .filter(|entry| entry.content() == EntryContent::SaveToggleInfo);
    let Some(entry) = candidates.next() else {
        return (Vec::new(), Vec::new());
    };
    if candidates.next().is_some() || entry.name != ENTRY_NAME {
        return (Vec::new(), Vec::new());
    }
    let Some((source_offset, byte_len)) = entry.file_span() else {
        return (Vec::new(), Vec::new());
    };
    let (Ok(start), Ok(byte_len)) = (usize::try_from(source_offset), usize::try_from(byte_len))
    else {
        return (Vec::new(), Vec::new());
    };
    let Some(end) = start.checked_add(byte_len) else {
        return (Vec::new(), Vec::new());
    };
    let Some(bytes) = container.data.get(start..end) else {
        return (Vec::new(), Vec::new());
    };
    let Some(parsed) = parse_saved_toggle_stream(bytes, source_offset) else {
        return (Vec::new(), Vec::new());
    };
    (vec![parsed.stream], parsed.entries)
}

/// Whether the canonical saved-toggle entry has a complete admitted grammar.
pub(crate) fn has_complete_saved_toggle_stream(container: &Container) -> bool {
    !saved_toggle_records(container).0.is_empty()
}

fn parse_saved_toggle_stream(bytes: &[u8], source_offset: u64) -> Option<ParsedToggleStream> {
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

    let mut entries = Vec::new();
    entries.try_reserve_exact(count).ok()?;
    for ordinal in 0..count {
        let member_offset = view.position();
        let raw_byte_len = view.array::<2>()?;
        let byte_len = usize::from(View::u16_le_at(&raw_byte_len, 0)?);
        let value_at = view.position();
        let value = std::str::from_utf8(view.take(byte_len)?).ok()?;
        let (toggle_id, state) = value.rsplit_once(':')?;
        let state = match state {
            "On" => SavedToggleState::On,
            "Off" => SavedToggleState::Off,
            _ => return None,
        };
        entries.push(
            SavedToggleEntry::try_from(SavedToggleEntryWire {
                id: format!("nx:saved-toggle:entry#{ordinal}"),
                ordinal: u32::try_from(ordinal).ok()?,
                toggle_id: ToggleId::try_from(toggle_id.to_string()).ok()?,
                stable_identity: None,
                state,
                raw_byte_len,
                source_offset: source_offset.checked_add(member_offset as u64)?,
                value_source_offset: source_offset.checked_add(value_at as u64)?,
            })
            .ok()?,
        );
    }
    assign_stable_toggle_identities(&mut entries);
    let trailer_at = view.position();
    let trailer = view.array::<4>()?;
    if !view.is_empty() {
        return None;
    }
    Some(ParsedToggleStream {
        stream: SavedToggleStream {
            entry_count,
            trailer,
            source_offset,
            trailer_source_offset: source_offset.checked_add(trailer_at as u64)?,
        },
        entries,
    })
}

fn assign_stable_toggle_identities(entries: &mut [SavedToggleEntry]) {
    let mut counts = BTreeMap::<ToggleId, usize>::new();
    for entry in entries.iter() {
        *counts.entry(entry.toggle_id.clone()).or_default() += 1;
    }
    for entry in entries.iter_mut() {
        entry.stable_identity = (counts.get(&entry.toggle_id) == Some(&1))
            .then(|| format!("nx:saved-toggle:identity#{}", entry.toggle_id));
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_saved_toggle_stream, SavedToggleState};

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
        let parsed = parse_saved_toggle_stream(&bytes, 100).expect("complete stream");
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
        let first = parse_saved_toggle_stream(&first, 0).expect("first stream");
        let reordered = parse_saved_toggle_stream(&reordered, 0).expect("reordered stream");
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
        let parsed = parse_saved_toggle_stream(&bytes, 0).expect("complete stream");
        assert!(parsed
            .entries
            .iter()
            .all(|entry| entry.stable_identity.is_none()));
    }

    #[test]
    fn rejects_partial_or_noncanonical_streams_atomically() {
        let complete = stream(&["0123456789abcdef0123456789abcdef:On"], [1, 2, 3, 4]);
        assert!(parse_saved_toggle_stream(&complete[..complete.len() - 1], 0).is_none());

        let uppercase = stream(&["0123456789ABCDEF0123456789abcdef:On"], [1, 2, 3, 4]);
        assert!(parse_saved_toggle_stream(&uppercase, 0).is_none());

        let mut wrong_count = complete;
        wrong_count[1] = 2;
        assert!(parse_saved_toggle_stream(&wrong_count, 0).is_none());
    }

    #[test]
    fn rejects_count_before_count_driven_reservation() {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        assert!(parse_saved_toggle_stream(&bytes, 0).is_none());
    }
}
