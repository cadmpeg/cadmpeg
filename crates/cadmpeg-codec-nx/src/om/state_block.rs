// SPDX-License-Identifier: Apache-2.0
//! State-block path selection and ordered status/message phases.

use super::nonempty::NonEmpty;
use super::state_message::{OperationStateMessage, StateMessage};
use super::state_slot_lane::StateSlotLane;
use super::state_status::{operation_state_opaque_lane_end_at, operation_state_status_row_at};
use super::state_table::{OperationStateStatusTable, StateTableEntry};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::num::NonZeroUsize;

pub(super) struct OperationStateBlock<'a> {
    offset: usize,
    body: BlockBody<'a>,
}

enum BlockBody<'a> {
    Statuses {
        entries: NonEmpty<StateTableEntry<'a>>,
        messages: Vec<StateMessage<&'a str>>,
    },
    Messages(NonEmpty<StateMessage<&'a str>>),
}

impl<'a> OperationStateBlock<'a> {
    fn new(
        offset: usize,
        entries: Vec<StateTableEntry<'a>>,
        messages: Vec<StateMessage<&'a str>>,
    ) -> Option<Self> {
        let after_status = entries
            .iter()
            .try_fold(offset, |end, entry| end.checked_add(entry.byte_len()))?;
        messages.iter().try_fold(after_status, |end, message| {
            end.checked_add(message.byte_len())
        })?;
        let body = match NonEmpty::new(entries) {
            Some(entries) => BlockBody::Statuses { entries, messages },
            None => BlockBody::Messages(NonEmpty::new(messages)?),
        };
        Some(Self { offset, body })
    }
    pub(super) fn into_status_table(self) -> Option<OperationStateStatusTable<'a>> {
        match self.body {
            BlockBody::Statuses { entries, .. } => {
                OperationStateStatusTable::new(self.offset, entries)
            }
            BlockBody::Messages(_) => None,
        }
    }
    fn status_end_offset(&self) -> usize {
        match &self.body {
            BlockBody::Statuses { entries, .. } => entries
                .iter()
                .fold(self.offset, |end, entry| end + entry.byte_len()),
            BlockBody::Messages(_) => self.offset,
        }
    }
    pub(super) fn into_messages(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Vec<OperationStateMessage<'a>>>, CodecError> {
        let offset = self.status_end_offset();
        match self.body {
            BlockBody::Statuses { messages, .. } => locate_messages(ctx, offset, messages),
            BlockBody::Messages(messages) => locate_messages(ctx, offset, messages),
        }
    }
    #[cfg(test)]
    fn offset(&self) -> usize {
        self.offset
    }
    #[cfg(test)]
    fn rows(&self) -> Vec<&super::state_status::StateStatus<&'a str, &'a [u8]>> {
        match &self.body {
            BlockBody::Statuses { entries, .. } => entries
                .iter()
                .filter_map(|entry| match entry {
                    StateTableEntry::Status(row) => Some(row),
                    StateTableEntry::Slots(_) => None,
                })
                .collect(),
            BlockBody::Messages(_) => Vec::new(),
        }
    }
    #[cfg(test)]
    fn messages(&self) -> Vec<&StateMessage<&'a str>> {
        match &self.body {
            BlockBody::Statuses { messages, .. } => messages.iter().collect(),
            BlockBody::Messages(messages) => messages.iter().collect(),
        }
    }
}

fn locate_messages<'a>(
    ctx: &DecodeContext<'_>,
    mut offset: usize,
    messages: impl IntoIterator<Item = StateMessage<&'a str>>,
) -> Result<Option<Vec<OperationStateMessage<'a>>>, CodecError> {
    let mut located = Vec::new();
    for body in messages {
        let Some(message) = OperationStateMessage::new(offset, body) else {
            return Ok(None);
        };
        offset = message.end_offset();
        ctx.charge_collection_items(1, "nx state messages")?;
        ctx.charge_retained(
            u64_from_index(std::mem::size_of::<OperationStateMessage<'_>>()),
            "retain NX state message",
        )?;
        located
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx state messages", 0, 1))?;
        located.push(message);
    }
    Ok(Some(located))
}

fn operation_state_status_end_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
    opaque_lane_starts: Option<&[usize]>,
) -> Option<usize> {
    if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
        let precomputed_end = opaque_lane_starts
            .and_then(|starts| operation_state_opaque_lane_end_at(starts, at, end));
        precomputed_end.or_else(|| StateSlotLane::end_at(bytes, at, end))
    } else {
        operation_state_status_row_at(bytes, at, end, base_offset, opaque_lane_starts)
            .map(|row| row.end_offset() - base_offset)
    }
}

#[derive(Clone, Copy)]
struct OperationStatePath {
    length: NonZeroUsize,
    end: usize,
}

fn operation_state_path_at(
    paths: &[(usize, OperationStatePath)],
    at: usize,
) -> Option<OperationStatePath> {
    paths
        .binary_search_by(|(offset, _)| offset.cmp(&at).reverse())
        .ok()
        .map(|index| paths[index].1)
}

pub(super) fn operation_state_block_before_boundary<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Result<Option<OperationStateBlock<'a>>, CodecError> {
    const MAX_STATE_BLOCK_TAIL_BYTES: usize = 64 * 1024;

    if start >= end || end > bytes.len() {
        return Ok(None);
    }

    let scanned = end - start;
    ctx.charge_work(u64_from_index(scanned), "scan NX state block")?;
    let path_bytes = scanned
        .checked_mul(std::mem::size_of::<(usize, OperationStatePath)>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx state paths", 0, u64_from_index(scanned)))?;
    let _status_paths_reservation =
        ctx.reserve_scoped(u64_from_index(path_bytes), "scan NX state status paths")?;
    let _message_paths_reservation =
        ctx.reserve_scoped(u64_from_index(path_bytes), "scan NX state message paths")?;
    let opaque_bytes = scanned
        .checked_mul(std::mem::size_of::<usize>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx opaque state lanes", 0, u64_from_index(scanned)))?;
    let _opaque_lanes_reservation =
        ctx.reserve_scoped(u64_from_index(opaque_bytes), "scan NX opaque state lanes")?;

    let mut opaque_lane_starts = Vec::new();
    for at in start..end.saturating_sub(1) {
        if bytes.get(at..at + 2) == Some(&[0x02, 0x11]) {
            ctx.charge_collection_items(1, "nx opaque state lanes")?;
            opaque_lane_starts.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("nx opaque state lanes", 0, 1)
            })?;
            opaque_lane_starts.push(at);
        }
    }

    let mut status_paths = Vec::new();
    let mut message_paths = Vec::new();
    for at in (start..end).rev() {
        if let Some(message) = OperationStateMessage::read(bytes, at, base_offset) {
            let next = message.end_offset() - base_offset;
            if next > at && next <= end {
                let continuation = (next < end)
                    .then(|| operation_state_path_at(&message_paths, next))
                    .flatten();
                let length = continuation
                    .map_or(Some(NonZeroUsize::MIN), |path| path.length.checked_add(1));
                let Some(length) = length else {
                    return Ok(None);
                };
                let path_end = continuation.map_or(next, |path| path.end);
                ctx.charge_collection_items(1, "nx state message paths")?;
                message_paths.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("nx state message paths", 0, 1)
                })?;
                message_paths.push((
                    at,
                    OperationStatePath {
                        length,
                        end: path_end,
                    },
                ));
            }
        }

        let status_path =
            operation_state_status_end_at(bytes, at, end, base_offset, Some(&opaque_lane_starts))
                .filter(|next| *next > at && *next <= end)
                .and_then(|next| {
                    let continuation = (next < end)
                        .then(|| operation_state_path_at(&status_paths, next))
                        .flatten();
                    let length = continuation
                        .map_or(Some(NonZeroUsize::MIN), |path| path.length.checked_add(1))?;
                    Some(OperationStatePath {
                        length,
                        end: continuation.map_or(next, |path| path.end),
                    })
                });
        let message_path = operation_state_path_at(&message_paths, at);
        let best_path = status_path
            .filter(|status| message_path.is_none_or(|message| status.length >= message.length))
            .or(message_path);
        if let Some(path) = best_path {
            ctx.charge_collection_items(1, "nx state status paths")?;
            status_paths.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("nx state status paths", 0, 1)
            })?;
            status_paths.push((at, path));
        }
    }

    let has_exact_boundary_path = status_paths.iter().any(|(_, path)| path.end == end);
    let selected = status_paths
        .iter()
        .filter(|(at, path)| {
            if has_exact_boundary_path {
                path.end == end
            } else {
                path.end >= *at && end.saturating_sub(path.end) <= MAX_STATE_BLOCK_TAIL_BYTES
            }
        })
        .max_by_key(|(at, path)| (path.length, std::cmp::Reverse(*at)))
        .map(|(at, path)| (*at, *path));
    let Some((offset, path)) = selected else {
        return Ok(None);
    };
    let path_end = path.end;
    let mut entries = Vec::new();
    let mut messages = Vec::new();
    let mut at = offset;
    while at < path_end {
        let status_candidate =
            operation_state_status_end_at(bytes, at, end, base_offset, Some(&opaque_lane_starts))
                .filter(|next| {
                    *next > at
                        && *next <= path_end
                        && (*next == path_end
                            || (*next < end
                                && operation_state_path_at(&status_paths, *next)
                                    .is_some_and(|path| path.end == path_end)))
                })
                .and_then(|next| {
                    let length = if next == path_end {
                        NonZeroUsize::MIN
                    } else {
                        operation_state_path_at(&status_paths, next)?
                            .length
                            .checked_add(1)?
                    };
                    Some((next, length))
                });
        let message_candidate = operation_state_path_at(&message_paths, at)
            .filter(|path| path.end == path_end)
            .and_then(|path| {
                OperationStateMessage::read(bytes, at, base_offset)
                    .map(|message| (message, path.length))
            });

        if let Some((next, _)) = status_candidate.filter(|(_, length)| {
            message_candidate
                .as_ref()
                .is_none_or(|(_, message_length)| length >= message_length)
        }) {
            if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
                let Some(lane) = StateSlotLane::read(ctx, bytes, at, end, base_offset)? else {
                    return Ok(None);
                };
                let lane_end = lane.end_offset() - base_offset;
                if lane_end != next {
                    return Ok(None);
                }
                ctx.charge_collection_items(1, "nx state block entries")?;
                ctx.charge_retained(
                    u64_from_index(std::mem::size_of::<StateTableEntry<'_>>()),
                    "retain NX state block entry",
                )?;
                entries.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("nx state block entries", 0, 1)
                })?;
                entries.push(StateTableEntry::Slots(lane.into_slots()));
                at = next;
            } else {
                let Some(row) = operation_state_status_row_at(
                    bytes,
                    at,
                    end,
                    base_offset,
                    Some(&opaque_lane_starts),
                ) else {
                    return Ok(None);
                };
                let row_end = row.end_offset() - base_offset;
                if row_end != next {
                    return Ok(None);
                }
                ctx.charge_collection_items(1, "nx state block entries")?;
                ctx.charge_retained(
                    u64_from_index(std::mem::size_of::<StateTableEntry<'_>>()),
                    "retain NX state block entry",
                )?;
                entries.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("nx state block entries", 0, 1)
                })?;
                entries.push(StateTableEntry::Status(row.body()));
                at = next;
            }
        } else {
            let Some((message, _)) = message_candidate else {
                return Ok(None);
            };
            let next = message.end_offset() - base_offset;
            if next <= at || next > path_end {
                return Ok(None);
            }
            ctx.charge_collection_items(1, "nx state block messages")?;
            ctx.charge_retained(
                u64_from_index(std::mem::size_of::<StateMessage<&str>>()),
                "retain NX state block message",
            )?;
            messages.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("nx state block messages", 0, 1)
            })?;
            messages.push(message.body());
            at = next;
            break;
        }
    }
    while at < path_end {
        let Some(message) = OperationStateMessage::read(bytes, at, base_offset) else {
            return Ok(None);
        };
        let next = message.end_offset() - base_offset;
        if next <= at || next > path_end {
            return Ok(None);
        }
        ctx.charge_collection_items(1, "nx state block messages")?;
        ctx.charge_retained(
            u64_from_index(std::mem::size_of::<StateMessage<&str>>()),
            "retain NX state block message",
        )?;
        messages.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("nx state block messages", 0, 1)
        })?;
        messages.push(message.body());
        at = next;
    }
    let Some(offset) = base_offset.checked_add(offset) else {
        return Ok(None);
    };
    Ok(OperationStateBlock::new(offset, entries, messages))
}

#[cfg(test)]
mod tests {
    use super::{
        operation_state_block_before_boundary, OperationStateBlock, OperationStateMessage,
        StateSlotLane, StateTableEntry,
    };
    use crate::om::operation_state_group_table;
    use crate::om::state_status::{operation_state_status_row_at, StateStatusPayload};
    use crate::om::tests::message_bytes;

    #[test]
    fn table_entries_and_messages_follow_one_block_origin() {
        let row = operation_state_status_row_at(&[0x41, 1, 0x3f], 0, 3, 0, None)
            .unwrap()
            .body();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let slots = StateSlotLane::read(&ctx, &[2, 1, 0x11, 0xff, 2, 0x11], 0, 6, 0)
            .unwrap()
            .unwrap()
            .into_slots();
        let message =
            OperationStateMessage::read(&[3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0], 0, 0)
                .unwrap()
                .body();
        let entries = vec![
            StateTableEntry::Status(row),
            StateTableEntry::Slots(slots),
            StateTableEntry::Status(row),
        ];
        let block = OperationStateBlock::new(100, entries.clone(), vec![message]).unwrap();
        assert_eq!(block.status_end_offset(), 112);
        let table = block.into_status_table().unwrap();
        let positioned: Vec<_> = table.into_entries().collect();
        assert_eq!(
            positioned
                .iter()
                .map(|(offset, _)| *offset)
                .collect::<Vec<_>>(),
            [100, 103, 109]
        );
        assert!(matches!(positioned[1].1, StateTableEntry::Slots(_)));
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[], &arena, &policy,
        ).unwrap();
        let messages = OperationStateBlock::new(100, entries, vec![message])
            .unwrap()
            .into_messages(&ctx)
            .unwrap()
            .unwrap();
        assert_eq!((messages[0].offset(), messages[0].end_offset()), (112, 125));
    }

    #[test]
    fn message_only_blocks_are_nonempty_and_bound_their_derived_end() {
        let message =
            OperationStateMessage::read(&[3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0], 0, 0)
                .unwrap()
                .body();
        assert!(OperationStateBlock::new(100, Vec::new(), Vec::new()).is_none());
        let block = OperationStateBlock::new(100, Vec::new(), vec![message]).unwrap();
        assert_eq!(block.status_end_offset(), 100);
        assert!(block.into_status_table().is_none());
        let block = OperationStateBlock::new(usize::MAX - 13, Vec::new(), vec![message]).unwrap();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(
            block.into_messages(&ctx).unwrap().unwrap()[0].end_offset(),
            usize::MAX
        );
        assert!(OperationStateBlock::new(usize::MAX - 12, Vec::new(), vec![message]).is_none());
    }

    #[test]
    fn operation_state_block_keeps_inline_diagnostics_out_of_standalone_messages() {
        let mut bytes = vec![0x3c, 0x81, 0x23];
        let diagnostic = message_bytes(b"inline", &[0xaa, 0x60, 0x6b], [0, 1]);
        bytes.extend_from_slice(&diagnostic);
        bytes.extend(message_bytes(b"standalone", &[0xaa, 0x39, 0x4e], [0, 2]));

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes, &arena, &policy,
        ).unwrap();
        let block = operation_state_block_before_boundary(&ctx, &bytes, 0, bytes.len(), 500)
            .unwrap().expect("complete operation-state block");
        assert_eq!(block.rows().len(), 1);
        assert!(matches!(
            block.rows()[0].payload,
            StateStatusPayload::Diagnostic(..)
        ));
        assert_eq!(block.messages().len(), 1);
        assert_eq!(block.messages()[0].text.as_str(), "standalone");
        assert_eq!(block.status_end_offset(), 500 + 3 + diagnostic.len());
    }

    #[test]
    fn operation_state_status_table_ignores_incomplete_preceding_operation_lane() {
        let mut bytes = vec![
            0x41, 0x80, 0x01, 0x3f, 0x31, 0x80, 0x55, 0x87, 0xb3, 0xff, 0x81, 0x36, 0xff, 0x41,
            0x80, 0x20, 0x3f, 0x44, 0x80, 0x21, 0x4b, 0xff, 0x80, 0x22, 0xff,
        ];
        let message = message_bytes(b"boundary", &[0xaa, 0x01, 0x02], [0, 1]);
        let boundary = bytes.len();
        bytes.extend(message);

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes, &arena, &policy,
        ).unwrap();
        let block = operation_state_block_before_boundary(&ctx, &bytes, 0, boundary, 500)
            .unwrap().expect("complete status chain");
        assert_eq!(block.offset(), 500 + 13);
        assert_eq!(block.rows().len(), 2);
        assert_eq!(Some(block.rows()[0].object_index.value()), Some(0x20));
        assert_eq!(block.rows()[1].status_code.value(), 0x44);
        assert_eq!(block.status_end_offset(), 500 + boundary);
    }

    #[test]
    fn operation_state_block_stops_before_untyped_tail() {
        let mut bytes = vec![
            0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff,
        ];
        let status_end = bytes.len();
        bytes.extend([0x31, 0x80, 0x01, 0x01, 0x02, 0x55, 0x99]);

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes, &arena, &policy,
        ).unwrap();
        let block = operation_state_block_before_boundary(&ctx, &bytes, 0, bytes.len(), 500)
            .unwrap().expect("status chain before bounded tail");
        assert_eq!(block.offset(), 500);
        assert_eq!(block.rows().len(), 2);
        assert!(block.messages().is_empty());
        assert_eq!(block.status_end_offset(), 500 + status_end);
    }

    #[test]
    fn operation_state_block_keeps_a_large_opaque_prefix_sparse() {
        const OPAQUE_PREFIX_BYTES: usize = 128 * 1024;
        let mut bytes = vec![0xf0; OPAQUE_PREFIX_BYTES];
        let status_start = bytes.len();
        bytes.extend([
            0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff,
        ]);
        let boundary = bytes.len();

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes, &arena, &policy,
        ).unwrap();
        let block = operation_state_block_before_boundary(&ctx, &bytes, 0, boundary, 500)
            .unwrap().expect("status chain after large opaque prefix");
        assert_eq!(block.offset(), 500 + status_start);
        assert_eq!(block.rows().len(), 2);
        assert!(block.messages().is_empty());
        assert_eq!(block.status_end_offset(), 500 + boundary);
    }

    #[test]
    fn operation_state_messages_accept_terminal_count_shared_with_group_opener() {
        let mut bytes = message_bytes(b"terminal", &[0xaa, 0x39, 0x4e], [1, 0]);
        let group_start = bytes.len() - 2;
        bytes.extend([0x01, 0x02, 0x4a, 0x83, 0x20, 0x01, 0xff]);
        let table = operation_state_group_table(&bytes, group_start, bytes.len(), 500)
            .expect("group table");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes, &arena, &policy,
        ).unwrap();
        let messages = operation_state_block_before_boundary(&ctx, &bytes, 0, group_start + 2, 500)
            .unwrap().expect("terminal message")
            .into_messages(&ctx)
            .unwrap()
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body().text.as_str(), "terminal");
        assert_eq!(messages[0].end_offset(), 500 + group_start + 2);
        assert_eq!(table.offset(), 500 + group_start);
        assert_eq!(table.groups().first().opener().bytes(), [0x01, 0x00]);
    }

    #[test]
    fn operation_state_block_prefers_boundary_closed_path() {
        let mut bytes = vec![
            0x41, 0x80, 0x01, 0x3f, 0x41, 0x80, 0x02, 0x3f, 0x41, 0x80, 0x03, 0x3f, 0x31, 0x80,
            0x04, 0x01,
        ];
        let closed_path_start = bytes.len();
        bytes.extend([0x44, 0x80, 0x05, 0x3f]);
        bytes.extend(message_bytes(b"closed", &[0xaa, 0x01, 0x02], [0, 1]));

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes, &arena, &policy,
        ).unwrap();
        let block = operation_state_block_before_boundary(&ctx, &bytes, 0, bytes.len(), 500)
            .unwrap().expect("boundary-closed state path");
        assert_eq!(block.offset(), 500 + closed_path_start);
        assert_eq!(block.rows().len(), 1);
        assert_eq!(block.messages().len(), 1);
        assert_eq!(block.messages()[0].text.as_str(), "closed");
    }
    #[test]
    fn operation_state_block_refuses_collection_limit() {
        let bytes = [0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = operation_state_block_before_boundary(&ctx, &bytes, 0, bytes.len(), 0)
            .err().expect("resource refusal");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn operation_state_block_refuses_retained_limit() {
        let bytes = [0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = operation_state_block_before_boundary(&ctx, &bytes, 0, bytes.len(), 0)
            .err().expect("resource refusal");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
    }

    #[test]
    fn operation_state_block_refuses_scoped_limit() {
        let bytes = [0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = operation_state_block_before_boundary(&ctx, &bytes, 0, bytes.len(), 0)
            .err().expect("resource refusal");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn operation_state_block_refuses_work_limit() {
        let bytes = [0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = operation_state_block_before_boundary(&ctx, &bytes, 0, bytes.len(), 0)
            .err().expect("resource refusal");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
    }

}
