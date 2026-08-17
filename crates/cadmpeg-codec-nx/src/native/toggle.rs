// SPDX-License-Identifier: Apache-2.0
//! Typed records from the saved toggle-information stream.

use serde::{Deserialize, Serialize};

use cadmpeg_core::decode::View;

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
pub struct SavedToggleEntry {
    /// Globally unique native member identity.
    pub id: String,
    /// Zero-based serialized member order.
    pub ordinal: u32,
    /// Lowercase 32-hex-digit toggle identity.
    pub toggle_id: String,
    /// Exact state selected by the member text.
    pub state: SavedToggleState,
    /// Exact little-endian member-length word.
    pub raw_byte_len: [u8; 2],
    /// Absolute file offset of the member-length word.
    pub source_offset: u64,
    /// Absolute file offset of the first toggle-identity byte.
    pub value_source_offset: u64,
}

/// Complete saved toggle-information stream envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedToggleStream {
    /// Globally unique native stream identity.
    pub id: String,
    /// Serialized stream version.
    pub version: u8,
    /// Exact little-endian member-count word.
    pub raw_count: [u8; 4],
    /// Ordered saved-toggle members.
    pub entries: Vec<String>,
    /// Exact four-byte terminal word.
    pub trailer: [u8; 4],
    /// Absolute file offset of the version byte.
    pub source_offset: u64,
    /// Absolute file offset of the terminal word.
    pub trailer_source_offset: u64,
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
    let Some((source_offset, byte_len)) = entry.file_span else {
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
    let raw_count = view.array::<4>()?;
    let count = usize::try_from(View::u32_le_at(&raw_count, 0)?).ok()?;
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
        if toggle_id.len() != 32
            || !toggle_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return None;
        }
        let state = match state {
            "On" => SavedToggleState::On,
            "Off" => SavedToggleState::Off,
            _ => return None,
        };
        entries.push(SavedToggleEntry {
            id: format!("nx:saved-toggle:entry#{ordinal}"),
            ordinal: u32::try_from(ordinal).ok()?,
            toggle_id: toggle_id.to_string(),
            state,
            raw_byte_len,
            source_offset: source_offset.checked_add(member_offset as u64)?,
            value_source_offset: source_offset.checked_add(value_at as u64)?,
        });
    }
    let trailer_at = view.position();
    let trailer = view.array::<4>()?;
    if !view.is_empty() {
        return None;
    }
    let mut entry_ids = Vec::new();
    entry_ids.try_reserve_exact(entries.len()).ok()?;
    entry_ids.extend(entries.iter().map(|entry| entry.id.clone()));
    Some(ParsedToggleStream {
        stream: SavedToggleStream {
            id: "nx:saved-toggle:stream#0".to_string(),
            version,
            raw_count,
            entries: entry_ids,
            trailer,
            source_offset,
            trailer_source_offset: source_offset.checked_add(trailer_at as u64)?,
        },
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_saved_toggle_stream, SavedToggleState};

    fn stream(member: &str, trailer: [u8; 4]) -> Vec<u8> {
        let mut bytes = vec![1, 1, 0, 0, 0];
        bytes.extend_from_slice(&(member.len() as u16).to_le_bytes());
        bytes.extend_from_slice(member.as_bytes());
        bytes.extend_from_slice(&trailer);
        bytes
    }

    #[test]
    fn parses_complete_counted_toggle_stream() {
        let bytes = stream(
            "0123456789abcdef0123456789abcdef:Off",
            [0xde, 0xad, 0xbe, 0xef],
        );
        let parsed = parse_saved_toggle_stream(&bytes, 100).expect("complete stream");
        assert_eq!(parsed.stream.version, 1);
        assert_eq!(parsed.stream.raw_count, [1, 0, 0, 0]);
        assert_eq!(parsed.stream.trailer, [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(parsed.stream.trailer_source_offset, 143);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].state, SavedToggleState::Off);
        assert_eq!(parsed.entries[0].source_offset, 105);
        assert_eq!(parsed.entries[0].value_source_offset, 107);
    }

    #[test]
    fn rejects_partial_or_noncanonical_streams_atomically() {
        let complete = stream("0123456789abcdef0123456789abcdef:On", [1, 2, 3, 4]);
        assert!(parse_saved_toggle_stream(&complete[..complete.len() - 1], 0).is_none());

        let uppercase = stream("0123456789ABCDEF0123456789abcdef:On", [1, 2, 3, 4]);
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
