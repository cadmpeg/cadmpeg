// SPDX-License-Identifier: Apache-2.0
//! Per-object status payloads and their derived source extents.

use super::state_index::{OperationStateIndex, StateIndexToken};
use super::state_link::StateLinkCode;
use super::state_message::{OperationStateMessage, StateMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateStatusPayload<S, B> {
    Plain,
    Linked {
        link_code: StateLinkCode,
        object_index: StateIndexToken,
    },
    Diagnostic(StateMessage<S>),
    Opaque {
        raw: B,
    },
}

impl<S: AsRef<str>, B: AsRef<[u8]>> StateStatusPayload<S, B> {
    fn byte_len(&self) -> usize {
        match self {
            Self::Plain => 1,
            Self::Linked { object_index, .. } => 3 + usize::from(object_index.byte_len()),
            Self::Diagnostic(message) => message.byte_len(),
            Self::Opaque { raw } => raw.as_ref().len(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateStatus<S, B> {
    pub(crate) status_code: StateIndexToken,
    pub(crate) object_index: StateIndexToken,
    pub(crate) payload: StateStatusPayload<S, B>,
}

impl<S: AsRef<str>, B: AsRef<[u8]>> StateStatus<S, B> {
    pub(crate) fn byte_len(&self) -> usize {
        usize::from(self.status_code.byte_len())
            + usize::from(self.object_index.byte_len())
            + self.payload.byte_len()
    }
}

impl StateStatus<&str, &[u8]> {
    pub(crate) fn into_owned(self) -> StateStatus<String, Vec<u8>> {
        let payload = match self.payload {
            StateStatusPayload::Plain => StateStatusPayload::Plain,
            StateStatusPayload::Linked {
                link_code,
                object_index,
            } => StateStatusPayload::Linked {
                link_code,
                object_index,
            },
            StateStatusPayload::Diagnostic(message) => {
                StateStatusPayload::Diagnostic(message.into_owned())
            }
            StateStatusPayload::Opaque { raw } => StateStatusPayload::Opaque { raw: raw.to_vec() },
        };
        StateStatus {
            status_code: self.status_code,
            object_index: self.object_index,
            payload,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationStateStatus<'a> {
    offset: usize,
    body: StateStatus<&'a str, &'a [u8]>,
}

impl<'a> OperationStateStatus<'a> {
    pub(crate) fn offset(self) -> usize {
        self.offset
    }
    pub(crate) fn end_offset(self) -> usize {
        self.offset + self.body.byte_len()
    }
    pub(crate) fn body(self) -> StateStatus<&'a str, &'a [u8]> {
        self.body
    }
}

pub(super) fn operation_state_opaque_lane_end_at(
    lane_starts: &[usize],
    at: usize,
    end: usize,
) -> Option<usize> {
    let index = lane_starts.binary_search(&at).unwrap_or_else(|index| index);
    let lane_start = *lane_starts.get(index)?;
    let lane_end = lane_start.checked_add(2)?;
    (lane_end <= end).then_some(lane_end)
}

fn operation_state_opaque_payload_end(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
    const MAX_OPAQUE_STATUS_BYTES: usize = 64 * 1024;
    let first = *bytes.get(at)?;
    if !matches!(first, 0x02 | 0x1e | 0xff) {
        return None;
    }
    if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
        return Some(at + 3);
    }
    let search_end = end.min(at.saturating_add(MAX_OPAQUE_STATUS_BYTES));
    for cursor in at..search_end.saturating_sub(1) {
        if bytes.get(cursor..cursor + 2) == Some(&[0x02, 0x11]) {
            return Some(cursor + 2);
        }
    }
    None
}

fn operation_state_link_payload(
    bytes: &[u8],
    payload_at: usize,
    end: usize,
    base_offset: usize,
) -> Option<(StateStatusPayload<&str, &[u8]>, usize)> {
    let link_code = StateLinkCode::try_from(*bytes.get(payload_at)?).ok()?;
    if bytes.get(payload_at + 1) != Some(&0xff) {
        return None;
    }
    let linked_at = payload_at.checked_add(2)?;
    let linked = OperationStateIndex::read_at(bytes, linked_at, base_offset)?.token()?;
    let sentinel_at = linked_at.checked_add(linked.raw().len())?;
    if bytes.get(sentinel_at) != Some(&0xff) {
        return None;
    }
    let payload_end = sentinel_at.checked_add(1)?;
    (payload_end <= end).then_some((
        StateStatusPayload::Linked {
            link_code,
            object_index: linked,
        },
        payload_end,
    ))
}

pub(super) fn operation_state_status_row_at<'a>(
    bytes: &'a [u8],
    at: usize,
    end: usize,
    base_offset: usize,
    opaque_lane_starts: Option<&[usize]>,
) -> Option<OperationStateStatus<'a>> {
    let status_code = OperationStateIndex::read_at(bytes, at, base_offset)?.token()?;
    let object_at = at.checked_add(status_code.raw().len())?;
    let object_index = OperationStateIndex::read_at(bytes, object_at, base_offset)?.token()?;
    let payload_at = object_at.checked_add(object_index.raw().len())?;
    if payload_at >= end {
        return None;
    }
    let (payload, payload_end) = match bytes[payload_at] {
        0x3f => (StateStatusPayload::Plain, payload_at + 1),
        0x03 => {
            let message = OperationStateMessage::read(bytes, payload_at, base_offset)?;
            let payload_end = message.end_offset() - base_offset;
            (StateStatusPayload::Diagnostic(message.body()), payload_end)
        }
        0x02 | 0x1e | 0xff => {
            let precomputed_end = opaque_lane_starts
                .and_then(|starts| operation_state_opaque_lane_end_at(starts, payload_at, end));
            let payload_end = precomputed_end
                .or_else(|| operation_state_opaque_payload_end(bytes, payload_at, end))?;
            (
                StateStatusPayload::Opaque {
                    raw: bytes.get(payload_at..payload_end)?,
                },
                payload_end,
            )
        }
        _ => operation_state_link_payload(bytes, payload_at, end, base_offset)?,
    };
    base_offset.checked_add(payload_end)?;
    (payload_end <= end).then_some(OperationStateStatus {
        offset: base_offset.checked_add(at)?,
        body: StateStatus {
            status_code,
            object_index,
            payload,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::operation_state_status_row_at;

    #[test]
    fn source_status_extent_follows_payload_and_bounds_absolute_end() {
        for payload in [
            &[0x3f][..],
            &[0x4b, 0xff, 0x90, 0, 1, 0xff],
            &[3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0],
            &[2, 1, 0x11],
        ] {
            let mut bytes = vec![0x41, 1];
            bytes.extend_from_slice(payload);
            let row = operation_state_status_row_at(&bytes, 0, bytes.len(), 100, None).unwrap();
            assert_eq!(row.offset(), 100);
            assert_eq!(row.end_offset(), 102 + payload.len());
            assert!(operation_state_status_row_at(
                &bytes,
                0,
                bytes.len(),
                usize::MAX - bytes.len(),
                None
            )
            .is_some());
            assert!(operation_state_status_row_at(
                &bytes,
                0,
                bytes.len(),
                usize::MAX - bytes.len() + 1,
                None
            )
            .is_none());
        }
    }
}
