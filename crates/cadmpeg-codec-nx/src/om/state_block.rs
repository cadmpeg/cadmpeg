// SPDX-License-Identifier: Apache-2.0
//! State-block path selection and ordered status/message phases.

use super::nonempty::NonEmpty;
use super::state_message::{OperationStateMessage, StateMessage};
use super::state_slot_lane::StateSlotLane;
use super::state_status::{operation_state_opaque_lane_end_at, operation_state_status_row_at};
use super::state_table::{OperationStateStatusTable, StateSlotTokens, StateTableEntry};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::num::NonZeroUsize;

pub(super) struct OperationStateBlock<'a, 'ctx> {
    offset: usize,
    status_end: usize,
    body: BlockBody<'a, 'ctx>,
    entry_storage: ScopedReservation<'ctx>,
    // Keeps message workspace charged until the body is consumed.
    message_storage: ScopedReservation<'ctx>,
}

enum BlockBody<'a, 'ctx> {
    Statuses {
        entries: NonEmpty<StateTableEntry<'a, 'ctx>>,
        messages: Vec<StateMessage<&'a str>>,
    },
    Messages(NonEmpty<StateMessage<&'a str>>),
}

impl<'a, 'ctx> OperationStateBlock<'a, 'ctx> {
    fn from_parts(
        ctx: &DecodeContext<'_>,
        offset: usize,
        entries: Vec<StateTableEntry<'a, 'ctx>>,
        messages: Vec<StateMessage<&'a str>>,
        entry_storage: ScopedReservation<'ctx>,
        message_storage: ScopedReservation<'ctx>,
    ) -> Result<Option<Self>, CodecError> {
        let mut end = offset;
        let status_widths = ctx.all_by(
            &entries,
            |entry| {
                let Some(next) = end.checked_add(entry.byte_len(ctx)?) else {
                    return Ok(false);
                };
                end = next;
                Ok(true)
            },
            "NX state block status widths",
        );
        match status_widths {
            Ok(true) => {}
            Ok(false) => {
                drop(entries);
                drop(messages);
                drop(entry_storage);
                drop(message_storage);
                return Ok(None);
            }
            Err(error) => {
                drop(entries);
                drop(messages);
                drop(entry_storage);
                drop(message_storage);
                return Err(error);
            }
        }
        let status_end = end;
        let message_widths = ctx.all_by(
            &messages,
            |message| {
                let Some(next) = end.checked_add(message.byte_len()) else {
                    return Ok(false);
                };
                end = next;
                Ok(true)
            },
            "NX state block message widths",
        );
        match message_widths {
            Ok(true) => {}
            Ok(false) => {
                drop(entries);
                drop(messages);
                drop(entry_storage);
                drop(message_storage);
                return Ok(None);
            }
            Err(error) => {
                drop(entries);
                drop(messages);
                drop(entry_storage);
                drop(message_storage);
                return Err(error);
            }
        }
        let body = match NonEmpty::from_admitted_vec(entries) {
            Some(entries) => BlockBody::Statuses { entries, messages },
            None => {
                let Some(messages) = NonEmpty::from_admitted_vec(messages) else {
                    drop(entry_storage);
                    drop(message_storage);
                    return Ok(None);
                };
                BlockBody::Messages(messages)
            }
        };
        Ok(Some(Self {
            offset,
            status_end,
            body,
            entry_storage,
            message_storage,
        }))
    }
    #[cfg(test)]
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        offset: usize,
        entries: Vec<StateTableEntry<'a, 'ctx>>,
        messages: Vec<StateMessage<&'a str>>,
    ) -> Result<Option<Self>, CodecError> {
        let entry_storage = ctx.reserve_scoped(0, "NX test state entry workspace")?;
        let message_storage = ctx.reserve_scoped(0, "NX test state message workspace")?;
        Self::from_parts(
            ctx,
            offset,
            entries,
            messages,
            entry_storage,
            message_storage,
        )
    }
    pub(super) fn into_status_table(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<(OperationStateStatusTable<'a, 'ctx>, ScopedReservation<'ctx>)>, CodecError>
    {
        let Self {
            offset,
            body,
            entry_storage,
            message_storage,
            ..
        } = self;
        let table = match body {
            BlockBody::Statuses { entries, messages } => {
                let table = OperationStateStatusTable::new(ctx, offset, entries);
                drop(messages);
                table
            }
            BlockBody::Messages(messages) => {
                drop(messages);
                Ok(None)
            }
        };
        drop(message_storage);
        match table {
            Ok(Some(table)) => Ok(Some((table, entry_storage))),
            Ok(None) => {
                drop(entry_storage);
                Ok(None)
            }
            Err(error) => {
                drop(entry_storage);
                Err(error)
            }
        }
    }
    #[cfg(test)]
    fn status_end_offset(&self) -> usize {
        self.status_end
    }
    pub(super) fn into_messages(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Vec<OperationStateMessage<'a>>>, CodecError> {
        let offset = self.status_end;
        let Self {
            body,
            entry_storage,
            message_storage,
            ..
        } = self;
        let located = locate_messages(ctx, offset, &body);
        drop(body);
        drop(entry_storage);
        drop(message_storage);
        located
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
    body: &BlockBody<'a, '_>,
) -> Result<Option<Vec<OperationStateMessage<'a>>>, CodecError> {
    let mut located = Vec::new();
    let (initial, last, mut remaining) = match body {
        BlockBody::Statuses { messages, .. } => (messages.as_slice(), &[][..], messages.len()),
        BlockBody::Messages(messages) => (
            messages.initial(),
            std::slice::from_ref(messages.last()),
            messages.len(),
        ),
    };
    let mut messages = initial.iter().chain(last.iter()).copied();
    while remaining != 0 {
        let Some(body) = ctx.next_charged(&mut messages, "NX state message traversal")? else {
            break;
        };
        remaining -= 1;
        let Some(message) = OperationStateMessage::new(offset, body) else {
            return Ok(None);
        };
        offset = message.end_offset();
        ctx.reserve_vec(&mut located, 1, "nx state messages")?;
        located.push(message);
    }
    Ok(Some(located))
}

fn operation_state_status_end_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
    opaque_lane_starts: Option<&[usize]>,
) -> Result<Option<usize>, CodecError> {
    if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
        let precomputed_end = match opaque_lane_starts {
            Some(starts) => operation_state_opaque_lane_end_at(ctx, starts, at, end)?,
            None => None,
        };
        match precomputed_end {
            Some(end) => Ok(Some(end)),
            None => StateSlotLane::end_at(ctx, bytes, at, end),
        }
    } else {
        Ok(
            operation_state_status_row_at(ctx, bytes, at, end, base_offset, opaque_lane_starts)?
                .map(|row| row.end_offset() - base_offset),
        )
    }
}

#[derive(Clone, Copy)]
struct OperationStatePath {
    length: NonZeroUsize,
    end: usize,
}

fn operation_state_path_at(
    ctx: &DecodeContext<'_>,
    paths: &[(usize, OperationStatePath)],
    at: usize,
) -> Result<Option<OperationStatePath>, CodecError> {
    Ok(ctx
        .binary_search_by(
            paths,
            |(offset, _)| Ok(offset.cmp(&at).reverse()),
            "NX operation-state path lookup",
        )?
        .ok()
        .map(|index| paths[index].1))
}

pub(super) fn operation_state_block_before_boundary<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Result<Option<OperationStateBlock<'a, 'ctx>>, CodecError> {
    const MAX_STATE_BLOCK_TAIL_BYTES: usize = 64 * 1024;

    if start >= end || end > bytes.len() {
        return Ok(None);
    }

    let mut scratch = ctx.reserve_scoped(0, "NX state block workspace")?;

    let mut opaque_lane_starts = Vec::new();
    let Some(last_pair) = end.checked_sub(1) else {
        return Ok(None);
    };
    for at in ctx.admit_iter(
        &(start..last_pair),
        "NX state block opaque boundary discovery",
    )? {
        if bytes.get(at..at + 2) == Some(&[0x02, 0x11]) {
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut opaque_lane_starts, 1, "nx opaque state lanes")
            })?;
            opaque_lane_starts.push(at);
        }
    }

    let mut status_paths = Vec::new();
    let mut message_paths = Vec::new();
    for at in ctx
        .admit_iter(&(start..end), "NX state block path discovery")?
        .rev()
    {
        if let Some(message) = OperationStateMessage::read(ctx, bytes, at, base_offset)? {
            let next = message.end_offset() - base_offset;
            if next > at && next <= end {
                let continuation = if next < end {
                    operation_state_path_at(ctx, &message_paths, next)?
                } else {
                    None
                };
                let length =
                    continuation.map_or(Some(NonZeroUsize::MIN), |path| path.length.checked_add(1));
                let Some(length) = length else {
                    return Ok(None);
                };
                let path_end = continuation.map_or(next, |path| path.end);
                scratch.with_storage(|| {
                    ctx.reserve_vec(&mut message_paths, 1, "nx state message paths")
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

        let next = operation_state_status_end_at(
            ctx,
            bytes,
            at,
            end,
            base_offset,
            Some(&opaque_lane_starts),
        )?;
        let status_path = if let Some(next) = next.filter(|next| *next > at && *next <= end) {
            let continuation = if next < end {
                operation_state_path_at(ctx, &status_paths, next)?
            } else {
                None
            };
            continuation
                .map_or(Some(NonZeroUsize::MIN), |path| path.length.checked_add(1))
                .map(|length| OperationStatePath {
                    length,
                    end: continuation.map_or(next, |path| path.end),
                })
        } else {
            None
        };
        let message_path = operation_state_path_at(ctx, &message_paths, at)?;
        let best_path = status_path
            .filter(|status| message_path.is_none_or(|message| status.length >= message.length))
            .or(message_path);
        if let Some(path) = best_path {
            scratch
                .with_storage(|| ctx.reserve_vec(&mut status_paths, 1, "nx state status paths"))?;
            status_paths.push((at, path));
        }
    }

    let has_exact_boundary_path = ctx.any_by(
        &status_paths,
        |(_, path)| Ok(path.end == end),
        "NX exact state boundary search",
    )?;
    let selected = ctx
        .admit_iter(&status_paths, "NX state block path selection")?
        .filter(|(at, path)| {
            if has_exact_boundary_path {
                path.end == end
            } else {
                path.end >= *at
                    && end
                        .checked_sub(path.end)
                        .is_some_and(|tail| tail <= MAX_STATE_BLOCK_TAIL_BYTES)
            }
        })
        .max_by_key(|(at, path)| (path.length, std::cmp::Reverse(*at)))
        .map(|(at, path)| (*at, *path));
    let Some((offset, path)) = selected else {
        return Ok(None);
    };
    let path_end = path.end;
    let mut entry_storage = ctx.reserve_scoped(0, "NX state block candidate storage")?;
    let mut message_storage = ctx.reserve_scoped(0, "NX state block message workspace")?;
    let mut entries = Vec::new();
    let mut messages = Vec::new();
    let mut at = offset;
    while at < path_end {
        ctx.charge_work(1, "NX state block status reconstruction")?;
        let next = operation_state_status_end_at(
            ctx,
            bytes,
            at,
            end,
            base_offset,
            Some(&opaque_lane_starts),
        )?;
        let status_candidate =
            if let Some(next) = next.filter(|next| *next > at && *next <= path_end) {
                let continuation = if next < end {
                    operation_state_path_at(ctx, &status_paths, next)?
                } else {
                    None
                };
                if next == path_end {
                    Some((next, NonZeroUsize::MIN))
                } else {
                    continuation
                        .filter(|path| path.end == path_end)
                        .and_then(|path| path.length.checked_add(1).map(|length| (next, length)))
                }
            } else {
                None
            };
        let message_candidate = match operation_state_path_at(ctx, &message_paths, at)?
            .filter(|path| path.end == path_end)
        {
            Some(path) => OperationStateMessage::read(ctx, bytes, at, base_offset)?
                .map(|message| (message, path.length)),
            None => None,
        };

        if let Some((next, _)) = status_candidate.filter(|(_, length)| {
            message_candidate
                .as_ref()
                .is_none_or(|(_, message_length)| length >= message_length)
        }) {
            if bytes.get(at..at + 3) == Some(&[0x02, 0x01, 0x11]) {
                let Some((lane, token_storage)) =
                    StateSlotLane::read(ctx, bytes, at, end, base_offset)?
                else {
                    return Ok(None);
                };
                let lane_end = lane.end_offset() - base_offset;
                if lane_end != next {
                    drop(lane);
                    drop(token_storage);
                    return Ok(None);
                }
                if let Err(error) = entry_storage
                    .with_storage(|| ctx.reserve_vec(&mut entries, 1, "nx state block entries"))
                {
                    drop(lane);
                    drop(token_storage);
                    return Err(error);
                }
                entries.push(StateTableEntry::Slots(StateSlotTokens::new(
                    lane.into_slots(),
                    token_storage,
                )));
                at = next;
            } else {
                let Some(row) = operation_state_status_row_at(
                    ctx,
                    bytes,
                    at,
                    end,
                    base_offset,
                    Some(&opaque_lane_starts),
                )?
                else {
                    return Ok(None);
                };
                let row_end = row.end_offset() - base_offset;
                if row_end != next {
                    return Ok(None);
                }
                entry_storage
                    .with_storage(|| ctx.reserve_vec(&mut entries, 1, "nx state block entries"))?;
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
            message_storage
                .with_storage(|| ctx.reserve_vec(&mut messages, 1, "nx state block messages"))?;
            messages.push(message.body());
            at = next;
            break;
        }
    }
    while at < path_end {
        ctx.charge_work(1, "NX state block message reconstruction")?;
        let Some(message) = OperationStateMessage::read(ctx, bytes, at, base_offset)? else {
            return Ok(None);
        };
        let next = message.end_offset() - base_offset;
        if next <= at || next > path_end {
            return Ok(None);
        }
        message_storage
            .with_storage(|| ctx.reserve_vec(&mut messages, 1, "nx state block messages"))?;
        messages.push(message.body());
        at = next;
    }
    let Some(offset) = base_offset.checked_add(offset) else {
        return Ok(None);
    };
    drop(opaque_lane_starts);
    drop(status_paths);
    drop(message_paths);
    drop(scratch);
    OperationStateBlock::from_parts(
        ctx,
        offset,
        entries,
        messages,
        entry_storage,
        message_storage,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        operation_state_block_before_boundary, OperationStateBlock, OperationStateMessage,
        StateSlotLane, StateSlotTokens, StateTableEntry,
    };
    use crate::om::operation_state_group_table;
    use crate::om::state_status::{operation_state_status_row_at, StateStatusPayload};
    use crate::om::tests::message_bytes;

    #[test]
    fn message_projection_refuses_at_the_variable_initial_traversal() {
        let bytes = [3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0];
        let message = crate::test_support::with_decode_context(|ctx| {
            OperationStateMessage::read(ctx, &bytes, 0, 0)
        })
        .unwrap()
        .unwrap()
        .body();
        let error = crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "NX state message traversal",
            |ctx| {
                OperationStateBlock::new(ctx, 100, Vec::new(), vec![message, message])?
                    .unwrap()
                    .into_messages(ctx)
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(first_visit) = error else {
            unreachable!("the operation-specific probe returned a work refusal");
        };
        assert_eq!(first_visit.operation, "NX state message traversal");
        assert_eq!(first_visit.additional, 1);
        let second_visit_cap = first_visit
            .used
            .checked_add(1)
            .expect("second message visit cap fits");
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = second_visit_cap,
            |ctx| {
                let error = OperationStateBlock::new(ctx, 100, Vec::new(), vec![message, message])
                    .and_then(|block| block.unwrap().into_messages(ctx))
                    .expect_err("the second message visit must be admitted separately");
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.operation == "NX state message traversal"
                        && limit.used == second_visit_cap
                        && limit.additional == 1)
                );
            },
        );
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = first_visit
                    .used
                    .checked_add(2)
                    .expect("exact message visit cap fits");
            },
            |ctx| {
                let messages =
                    OperationStateBlock::new(ctx, 100, Vec::new(), vec![message, message])
                        .and_then(|block| block.unwrap().into_messages(ctx))
                        .expect("both message visits fit exactly")
                        .expect("message block is nonempty");
                assert_eq!(messages.len(), 2);
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
    }

    #[test]
    fn table_entries_and_messages_follow_one_block_origin() {
        let row = crate::test_support::with_decode_context(|ctx| {
            operation_state_status_row_at(ctx, &[0x41, 1, 0x3f], 0, 3, 0, None)
        })
        .unwrap()
        .unwrap()
        .body();

        crate::test_support::with_decode_context(|ctx| {
            let (lane, token_storage) =
                StateSlotLane::read(ctx, &[2, 1, 0x11, 0xff, 2, 0x11], 0, 6, 0)
                    .unwrap()
                    .unwrap();
            let message = crate::test_support::with_decode_context(|ctx| {
                OperationStateMessage::read(
                    ctx,
                    &[3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0],
                    0,
                    0,
                )
            })
            .unwrap()
            .unwrap()
            .body();
            let entries = vec![
                StateTableEntry::Status(row),
                StateTableEntry::Slots(StateSlotTokens::new(lane.into_slots(), token_storage)),
                StateTableEntry::Status(row),
            ];
            let block = OperationStateBlock::new(ctx, 100, entries, vec![message])
                .unwrap()
                .unwrap();
            assert_eq!(block.status_end_offset(), 112);
            let (table, entry_storage) = block.into_status_table(ctx).unwrap().unwrap();
            let positioned: Vec<_> = table.into_entries(ctx).collect::<Result<_, _>>().unwrap();
            assert_eq!(
                positioned
                    .iter()
                    .map(|(offset, _)| *offset)
                    .collect::<Vec<_>>(),
                [100, 103, 109]
            );
            assert!(matches!(positioned[1].1, StateTableEntry::Slots(_)));
            drop(positioned);
            drop(entry_storage);

            let (lane, token_storage) =
                StateSlotLane::read(ctx, &[2, 1, 0x11, 0xff, 2, 0x11], 0, 6, 0)
                    .unwrap()
                    .unwrap();
            let entries = vec![
                StateTableEntry::Status(row),
                StateTableEntry::Slots(StateSlotTokens::new(lane.into_slots(), token_storage)),
                StateTableEntry::Status(row),
            ];
            let messages = OperationStateBlock::new(ctx, 100, entries, vec![message])
                .unwrap()
                .unwrap()
                .into_messages(ctx)
                .unwrap()
                .unwrap();
            assert_eq!((messages[0].offset(), messages[0].end_offset()), (112, 125));
        });
    }

    #[test]
    fn message_only_blocks_are_nonempty_and_bound_their_derived_end() {
        let message = crate::test_support::with_decode_context(|ctx| {
            OperationStateMessage::read(ctx, &[3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0], 0, 0)
        })
        .unwrap()
        .unwrap()
        .body();
        crate::test_support::with_decode_context(|ctx| {
            assert!(OperationStateBlock::new(ctx, 100, Vec::new(), Vec::new())
                .unwrap()
                .is_none());
            let block = OperationStateBlock::new(ctx, 100, Vec::new(), vec![message])
                .unwrap()
                .unwrap();
            assert_eq!(block.status_end_offset(), 100);
            assert!(
                crate::test_support::with_decode_context(|ctx| block.into_status_table(ctx))
                    .unwrap()
                    .is_none()
            );
            let block = OperationStateBlock::new(ctx, usize::MAX - 13, Vec::new(), vec![message])
                .unwrap()
                .unwrap();

            crate::test_support::with_decode_context(|ctx| {
                assert_eq!(
                    block.into_messages(ctx).unwrap().unwrap()[0].end_offset(),
                    usize::MAX
                );
                assert!(
                    OperationStateBlock::new(ctx, usize::MAX - 12, Vec::new(), vec![message])
                        .unwrap()
                        .is_none()
                );
            });
        });
    }

    #[test]
    fn operation_state_block_keeps_inline_diagnostics_out_of_standalone_messages() {
        let mut bytes = vec![0x3c, 0x81, 0x23];
        let diagnostic = message_bytes(b"inline", &[0xaa, 0x60, 0x6b], [0, 1]);
        bytes.extend_from_slice(&diagnostic);
        bytes.extend(message_bytes(b"standalone", &[0xaa, 0x39, 0x4e], [0, 2]));

        crate::test_support::with_decode_context_over(
            &bytes,
            |_| {},
            |ctx| {
                let block = operation_state_block_before_boundary(ctx, &bytes, 0, bytes.len(), 500)
                    .unwrap()
                    .expect("complete operation-state block");
                assert_eq!(block.rows().len(), 1);
                assert!(matches!(
                    block.rows()[0].payload,
                    StateStatusPayload::Diagnostic(..)
                ));
                assert_eq!(block.messages().len(), 1);
                assert_eq!(block.messages()[0].text.as_str(), "standalone");
                assert_eq!(block.status_end_offset(), 500 + 3 + diagnostic.len());
            },
        );
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

        crate::test_support::with_decode_context_over(
            &bytes,
            |_| {},
            |ctx| {
                let block = operation_state_block_before_boundary(ctx, &bytes, 0, boundary, 500)
                    .unwrap()
                    .expect("complete status chain");
                assert_eq!(block.offset(), 500 + 13);
                assert_eq!(block.rows().len(), 2);
                assert_eq!(Some(block.rows()[0].object_index.value()), Some(0x20));
                assert_eq!(block.rows()[1].status_code.value(), 0x44);
                assert_eq!(block.status_end_offset(), 500 + boundary);
            },
        );
    }

    #[test]
    fn operation_state_block_stops_before_untyped_tail() {
        let mut bytes = vec![
            0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff,
        ];
        let status_end = bytes.len();
        bytes.extend([0x31, 0x80, 0x01, 0x01, 0x02, 0x55, 0x99]);

        crate::test_support::with_decode_context_over(
            &bytes,
            |_| {},
            |ctx| {
                let block = operation_state_block_before_boundary(ctx, &bytes, 0, bytes.len(), 500)
                    .unwrap()
                    .expect("status chain before bounded tail");
                assert_eq!(block.offset(), 500);
                assert_eq!(block.rows().len(), 2);
                assert!(block.messages().is_empty());
                assert_eq!(block.status_end_offset(), 500 + status_end);
            },
        );
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

        crate::test_support::with_decode_context_over(
            &bytes,
            |_| {},
            |ctx| {
                let block = operation_state_block_before_boundary(ctx, &bytes, 0, boundary, 500)
                    .unwrap()
                    .expect("status chain after large opaque prefix");
                assert_eq!(block.offset(), 500 + status_start);
                assert_eq!(block.rows().len(), 2);
                assert!(block.messages().is_empty());
                assert_eq!(block.status_end_offset(), 500 + boundary);
            },
        );
    }

    #[test]
    fn operation_state_messages_accept_terminal_count_shared_with_group_opener() {
        let mut bytes = message_bytes(b"terminal", &[0xaa, 0x39, 0x4e], [1, 0]);
        let group_start = bytes.len() - 2;
        bytes.extend([0x01, 0x02, 0x4a, 0x83, 0x20, 0x01, 0xff]);
        let table = operation_state_group_table(&bytes, group_start, bytes.len(), 500)
            .expect("group table");

        crate::test_support::with_decode_context_over(
            &bytes,
            |_| {},
            |ctx| {
                let messages =
                    operation_state_block_before_boundary(ctx, &bytes, 0, group_start + 2, 500)
                        .unwrap()
                        .expect("terminal message")
                        .into_messages(ctx)
                        .unwrap()
                        .unwrap();
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0].body().text.as_str(), "terminal");
                assert_eq!(messages[0].end_offset(), 500 + group_start + 2);
                assert_eq!(table.offset(), 500 + group_start);
                assert_eq!(table.groups().first().opener().bytes(), [0x01, 0x00]);
            },
        );
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

        crate::test_support::with_decode_context_over(
            &bytes,
            |_| {},
            |ctx| {
                let block = operation_state_block_before_boundary(ctx, &bytes, 0, bytes.len(), 500)
                    .unwrap()
                    .expect("boundary-closed state path");
                assert_eq!(block.offset(), 500 + closed_path_start);
                assert_eq!(block.rows().len(), 1);
                assert_eq!(block.messages().len(), 1);
                assert_eq!(block.messages()[0].text.as_str(), "closed");
            },
        );
    }
    #[test]
    fn operation_state_block_refuses_collection_limit() {
        let bytes = [
            0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff,
        ];

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let error = operation_state_block_before_boundary(ctx, &bytes, 0, bytes.len(), 0)
                    .err()
                    .expect("resource refusal");
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
                );
            },
        );
    }

    #[test]
    fn operation_state_block_reader_uses_scoped_storage() {
        let bytes = [
            0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff,
        ];

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                let block = operation_state_block_before_boundary(ctx, &bytes, 0, bytes.len(), 0)
                    .expect("reader storage is scoped")
                    .expect("the same bounded status block is valid");
                drop(block);
                assert!(ctx.resource_refusal().is_none());
            },
        );
    }

    #[test]
    fn operation_state_block_refuses_scoped_limit() {
        let bytes = [
            0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff,
        ];

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_materialized_bytes = 0;
            },
            |ctx| {
                let error = operation_state_block_before_boundary(ctx, &bytes, 0, bytes.len(), 0)
                    .err()
                    .expect("resource refusal");
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
                );
            },
        );
    }

    #[test]
    fn operation_state_block_refuses_work_limit() {
        let bytes = [
            0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff,
        ];

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 0;
            },
            |ctx| {
                let error = operation_state_block_before_boundary(ctx, &bytes, 0, bytes.len(), 0)
                    .err()
                    .expect("resource refusal");
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
                );
            },
        );
    }
    #[test]
    fn state_block_width_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let row = crate::test_support::with_decode_context(|ctx| {
            operation_state_status_row_at(ctx, &[0x41, 1, 0x3f], 0, 3, 0, None)
        })
        .unwrap()
        .unwrap()
        .body();
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "NX state block status widths",
            |ctx| {
                OperationStateBlock::new(ctx, 100, vec![StateTableEntry::Status(row)], Vec::new())
                    .map(|block| block.is_some())
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX state block status widths"));
    }

    #[test]
    fn state_message_singleton_charges_its_input_sized_visit() {
        let message = crate::test_support::with_decode_context(|ctx| {
            OperationStateMessage::read(ctx, &[3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0], 0, 0)
        })
        .unwrap()
        .unwrap()
        .body();
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                let error = super::locate_messages(
                    ctx,
                    100,
                    &super::BlockBody::Messages(super::NonEmpty::new([message]).unwrap()),
                )
                .expect_err("the first input-sized visit must be admitted");
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.operation == "NX state message traversal"
                        && limit.used == 0 && limit.additional == 1)
                );
            },
        );
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 1,
            |ctx| {
                let located = super::locate_messages(
                    ctx,
                    100,
                    &super::BlockBody::Messages(super::NonEmpty::new([message]).unwrap()),
                )
                .unwrap()
                .unwrap();
                assert_eq!(located.len(), 1);
                assert_eq!((located[0].offset(), located[0].end_offset()), (100, 113));
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
    }
}
