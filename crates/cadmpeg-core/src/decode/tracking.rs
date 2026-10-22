// SPDX-License-Identifier: Apache-2.0
//! Session-owned record reads, scoped to the record and transaction that used them.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ops::Range;

use super::budget::DecodeBudget;

mod streams;
use super::{
    u64_from_index, DecodePolicy, ResourceDimension, ResourceFailure, ResourceLimit, SpaceId, View,
};
use crate::CodecError;
use streams::{Origin, Streams};

/// The owned identity, extent and reads of a captured record.
pub type RecordCaptureParts = (String, String, u64, u64, BTreeMap<String, Vec<Range<u64>>>);

/// Actual successful source reads made while constructing one record.
///
/// Captures can only be obtained from a completed context read transaction.
#[derive(Debug, PartialEq, Eq)]
pub struct RecordCapture {
    id: String,
    stream: String,
    offset: u64,
    byte_len: u64,
    reads: BTreeMap<String, Vec<Range<u64>>>,
}

impl RecordCapture {
    /// Record identity supplied at its read scope.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Stable source stream containing this record's extent.
    pub fn stream(&self) -> &str {
        &self.stream
    }
    /// Start of the record extent in its stream.
    pub fn offset(&self) -> u64 {
        self.offset
    }
    /// Length of the record extent.
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }
    /// Sorted, disjoint, coalesced successful reads by source stream.
    pub fn reads(&self) -> &BTreeMap<String, Vec<Range<u64>>> {
        &self.reads
    }
    /// Consume the capture in identity, stream, offset, length, reads order.
    pub fn into_parts(self) -> RecordCaptureParts {
        (self.id, self.stream, self.offset, self.byte_len, self.reads)
    }
}

#[derive(Debug, Default)]
pub(super) struct TrackingStore {
    sessions: RefCell<Vec<SessionSlot>>,
    allocation_failure: Cell<Option<ResourceLimit>>,
}

#[derive(Debug)]
struct SessionSlot {
    generation: usize,
    state: Option<Session>,
}

#[derive(Debug)]
struct Session {
    budget: DecodeBudget,
    streams: Streams,
    frames: Vec<Frame>,
    committed: Vec<RecordCapture>,
    next_token: usize,
    error: Option<&'static str>,
    sealed: bool,
    context_alive: bool,
    activated: bool,
}

#[derive(Debug)]
struct Frame {
    token: usize,
    record: Option<RecordCapture>,
    captures: Vec<RecordCapture>,
    deltas: Vec<ReadDelta>,
}

#[derive(Debug)]
struct ReadDelta {
    owner: usize,
    space: SpaceId,
    ranges: Vec<Range<u64>>,
}

/// Copyable reference into arena-owned session state; it never borrows Context.
#[derive(Debug, Clone, Copy)]
pub(super) struct SessionHandle<'a> {
    store: &'a TrackingStore,
    index: Option<usize>,
    generation: usize,
}

impl TrackingStore {
    pub(super) fn session(&self, policy: DecodePolicy, input_bytes: u64) -> SessionHandle<'_> {
        if self.allocation_failure.get().is_some() {
            return SessionHandle {
                store: self,
                index: None,
                generation: 0,
            };
        }
        let mut sessions = self.sessions.borrow_mut();
        if let Some((index, slot)) = sessions
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.state.is_none() && slot.generation < usize::MAX)
        {
            slot.generation += 1;
            slot.state = Some(Session::new(policy, input_bytes));
            return SessionHandle {
                store: self,
                index: Some(index),
                generation: slot.generation,
            };
        }
        let index = if sessions.try_reserve_exact(1).is_ok() {
            let index = sessions.len();
            sessions.push(SessionSlot {
                generation: 0,
                state: Some(Session::new(policy, input_bytes)),
            });
            Some(index)
        } else {
            self.allocation_failure
                .set(Some(ResourceLimit::allocation_failed(
                    ResourceDimension::RetainedBytes,
                    policy.limits.max_retained_bytes,
                    u64_from_index(size_of::<SessionSlot>()),
                    "allocate decode session",
                )));
            None
        };
        SessionHandle {
            store: self,
            index,
            generation: 0,
        }
    }
}

impl SessionHandle<'_> {
    pub(super) fn with_budget<R>(self, run: impl FnOnce(&DecodeBudget) -> R) -> R {
        if let Some(index) = self.index {
            let sessions = self.store.sessions.borrow();
            if let Some(session) = sessions
                .get(index)
                .filter(|slot| slot.generation == self.generation)
                .and_then(|slot| slot.state.as_ref())
            {
                return run(&session.budget);
            }
        }
        // Context::new has no error return. A failed session allocation is a
        // permanent budget fuse, surfaced by every charge and finish_session.
        let failure = self.allocation_failure();
        let budget = DecodeBudget::new(DecodePolicy::default(), 0);
        budget.refuse_limit(
            failure.dimension,
            failure.reason,
            failure.limit,
            failure.used,
            failure.additional,
            failure.operation,
        );
        run(&budget)
    }

    fn allocation_failure(self) -> ResourceLimit {
        self.store.allocation_failure.get().unwrap_or_else(|| {
            ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                0,
                u64_from_index(size_of::<Session>()),
                "allocate decode session",
            )
        })
    }
    pub(super) fn fused(self) -> Option<ResourceLimit> {
        self.with_budget(DecodeBudget::fused)
    }
    pub(super) fn input_bytes(self) -> u64 {
        self.with_budget(DecodeBudget::input_bytes)
    }
    pub(super) fn decompressed_used(self) -> u64 {
        self.with_budget(DecodeBudget::decompressed_used)
    }
    pub(super) fn decompression_allowance(self) -> u64 {
        self.with_budget(DecodeBudget::decompression_allowance)
    }
    pub(super) fn retained_bytes_available(self) -> u64 {
        self.with_budget(DecodeBudget::retained_bytes_available)
    }
    pub(super) fn proportional_allowance(self, ceiling: u64, base: u64, per_input: u64) -> u64 {
        self.with_budget(|budget| budget.proportional_allowance(ceiling, base, per_input))
    }
    pub(super) fn charge_input(
        self,
        amount: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.with_budget(|budget| budget.charge_input(amount, operation))
    }
    pub(super) fn charge_decompressed(
        self,
        amount: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.with_budget(|budget| budget.charge_decompressed(amount, operation))
    }
    pub(super) fn charge_entities(
        self,
        amount: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.with_budget(|budget| budget.charge_entities(amount, operation))
    }
    pub(super) fn retain_bytes(
        self,
        amount: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.with_budget(|budget| budget.retain_bytes(amount, operation))
    }
    pub(super) fn retained_allocation_failed(
        self,
        amount: u64,
        operation: &'static str,
    ) -> CodecError {
        self.with_budget(|budget| budget.retained_allocation_failed(amount, operation))
    }
    pub(super) fn refuse(
        self,
        dimension: ResourceDimension,
        reason: ResourceFailure,
        limit: u64,
        used: u64,
        additional: u64,
        operation: &'static str,
    ) -> CodecError {
        self.with_budget(|budget| {
            budget.refuse(dimension, reason, limit, used, additional, operation)
        })
    }

    fn session<R>(
        self,
        run: impl FnOnce(&mut Session) -> Result<R, CodecError>,
    ) -> Result<R, CodecError> {
        let Some(index) = self.index else {
            return Err(self.allocation_failure().into());
        };
        let mut sessions = self.store.sessions.borrow_mut();
        let session = sessions
            .get_mut(index)
            .filter(|slot| slot.generation == self.generation)
            .and_then(|slot| slot.state.as_mut())
            .ok_or_else(|| CodecError::malformed("decode session has ended"))?;
        session.check()?;
        run(session)
    }

    pub(super) fn same_session(self, other: Self) -> bool {
        std::ptr::eq(self.store, other.store)
            && self.index == other.index
            && self.generation == other.generation
    }

    pub(super) fn end_context(self) {
        self.release(true);
    }

    pub(super) fn release_guard(self) {
        self.release(false);
    }

    fn release(self, end_context: bool) {
        let Some(index) = self.index else {
            return;
        };
        let mut sessions = self.store.sessions.borrow_mut();
        let Some(slot) = sessions
            .get_mut(index)
            .filter(|slot| slot.generation == self.generation)
        else {
            return;
        };
        let Some(session) = &mut slot.state else {
            return;
        };
        if end_context {
            session.context_alive = false;
        }
        if !session.context_alive && session.budget.active_depth() == 0 {
            slot.state = None;
        }
    }

    fn is_live(self) -> bool {
        self.index.is_some_and(|index| {
            self.store
                .sessions
                .borrow()
                .get(index)
                .is_some_and(|slot| slot.generation == self.generation && slot.state.is_some())
        })
    }

    pub(super) fn register_root(self, byte_len: u64) -> Result<(), ResourceLimit> {
        if let Some(limit) = self.fused() {
            return Err(limit);
        }
        let index = self.index.ok_or_else(|| self.allocation_failure())?;
        if let Some(session) = self
            .store
            .sessions
            .borrow_mut()
            .get_mut(index)
            .filter(|slot| slot.generation == self.generation)
            .and_then(|slot| slot.state.as_mut())
        {
            session.streams.root(byte_len);
        }
        Ok(())
    }

    pub(super) fn name_stream(self, view: View<'_>, name: &str) -> Result<(), CodecError> {
        self.session(|session| {
            if view.space() != SpaceId::ROOT {
                return Err(CodecError::malformed("only the root stream can be renamed"));
            }
            if session.streams.name(SpaceId::ROOT)? == name {
                return Ok(());
            }
            if session.sealed || !session.frames.is_empty() || !session.committed.is_empty() {
                return Err(CodecError::malformed(
                    "cannot rename a source stream after record scopes began",
                ));
            }
            session.streams.rename_root(&session.budget, name)
        })
    }

    pub(super) fn register_alias(
        self,
        space: SpaceId,
        name: &str,
        parent: View<'_>,
        byte_len: u64,
    ) -> Result<SpaceId, CodecError> {
        self.register_stream(
            space,
            name,
            byte_len,
            Origin::Alias {
                parent: parent.space(),
                extent: super::ByteRange {
                    start: u64_from_index(parent.start()),
                    end: u64_from_index(parent.end()),
                },
            },
        )
    }

    pub(super) fn register_expanded(
        self,
        space: SpaceId,
        name: &str,
        source: View<'_>,
        byte_len: u64,
    ) -> Result<SpaceId, CodecError> {
        self.register_stream(
            space,
            name,
            byte_len,
            Origin::Compressed {
                parent: source.space(),
                extent: super::ByteRange {
                    start: u64_from_index(source.start()),
                    end: u64_from_index(source.end()),
                },
            },
        )
    }

    pub(super) fn register_composite(
        self,
        space: SpaceId,
        name: &str,
        inputs: &[View<'_>],
        byte_len: u64,
    ) -> Result<SpaceId, CodecError> {
        self.session(|session| {
            if session.sealed {
                return Err(CodecError::malformed(
                    "record captures have already been extracted",
                ));
            }
            session.activate()?;
            session
                .streams
                .register_composite(&session.budget, space, name, inputs, byte_len)
        })
    }

    fn register_stream(
        self,
        space: SpaceId,
        name: &str,
        byte_len: u64,
        origin: Origin,
    ) -> Result<SpaceId, CodecError> {
        self.session(|session| {
            if session.sealed {
                return Err(CodecError::malformed(
                    "record captures have already been extracted",
                ));
            }
            session.activate()?;
            session
                .streams
                .register(&session.budget, space, name, byte_len, origin)
        })
    }

    pub(super) fn begin(self, record: Option<(&str, View<'_>)>) -> Result<usize, CodecError> {
        self.session(|session| {
            if session.sealed {
                return Err(CodecError::malformed(
                    "record captures have already been extracted",
                ));
            }
            session.activate()?;
            let record = record
                .map(|(id, extent)| session.new_record(id, extent))
                .transpose()?;
            reserve(&session.budget, &mut session.frames, 1)?;
            let token = session.next_token;
            session.next_token = token
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("read scope identities exhausted"))?;
            session.frames.push(Frame {
                token,
                record,
                captures: Vec::new(),
                deltas: Vec::new(),
            });
            Ok(token)
        })
    }

    pub(super) fn read(self, space: SpaceId, range: Range<u64>) -> Result<(), CodecError> {
        // Views may outlive their Context. Their bytes remain readable, but an
        // ended generation cannot contribute reads to a replacement session.
        if !self.is_live() {
            return Ok(());
        }
        self.session(|session| {
            let Some(owner) = session
                .frames
                .iter()
                .rev()
                .find(|frame| frame.record.is_some())
                .map(|frame| frame.token)
            else {
                return Ok(());
            };
            if range.is_empty() {
                return Ok(());
            }
            let result = if let Some(frame) = session.frames.last_mut() {
                session
                    .streams
                    .project(&session.budget, space, range, |space, range| {
                        merge_delta(&session.budget, &mut frame.deltas, owner, space, range)
                    })
            } else {
                Ok(())
            };
            if result.is_err() {
                session.error = Some("source stream read projection failed");
            }
            result
        })
    }

    pub(super) fn commit(self, token: usize) -> Result<(), CodecError> {
        self.session(|session| {
            if session
                .frames
                .last()
                .is_none_or(|frame| frame.token != token)
            {
                session.error = Some("read scopes must close in nesting order");
                return Err(CodecError::malformed(
                    "read scopes must close in nesting order",
                ));
            }
            let mut frame = session
                .frames
                .pop()
                .ok_or_else(|| CodecError::malformed("read scope is missing at commit"))?;
            if let Some(mut record) = frame.record.take() {
                while let Some(index) = frame.deltas.iter().position(|delta| delta.owner == token) {
                    let delta = frame.deltas.remove(index);
                    // Retained-byte accounting charges the owned key/value,
                    // with key text and range storage charged separately. As
                    // for other retained maps, allocator node metadata is not
                    // a serialized value; its growth is bounded by this charged
                    // entry count, not guessed from private std node layouts.
                    session.budget.retain_bytes(
                        u64_from_index(size_of::<(String, Vec<Range<u64>>)>()),
                        "record reads table",
                    )?;
                    let stream = copy_string(&session.budget, session.streams.name(delta.space)?)?;
                    record.reads.insert(stream, delta.ranges);
                }
                reserve(&session.budget, &mut frame.captures, 1)?;
                frame.captures.push(record);
            }
            if let Some(parent) = session.frames.last_mut() {
                reserve(&session.budget, &mut parent.captures, frame.captures.len())?;
                parent.captures.extend(frame.captures);
                for delta in frame.deltas {
                    for range in delta.ranges {
                        merge_delta(
                            &session.budget,
                            &mut parent.deltas,
                            delta.owner,
                            delta.space,
                            range,
                        )?;
                    }
                }
            } else {
                if !frame.deltas.is_empty() {
                    session.error = Some("record read lost its owning scope");
                    return Err(CodecError::malformed("record read lost its owning scope"));
                }
                reserve(
                    &session.budget,
                    &mut session.committed,
                    frame.captures.len(),
                )?;
                session.committed.extend(frame.captures);
            }
            Ok(())
        })
    }

    pub(super) fn rollback(self, token: usize) {
        let Some(index) = self.index else {
            return;
        };
        let mut sessions = self.store.sessions.borrow_mut();
        if let Some(session) = sessions
            .get_mut(index)
            .filter(|slot| slot.generation == self.generation)
            .and_then(|slot| slot.state.as_mut())
        {
            if let Some(position) = session.frames.iter().position(|frame| frame.token == token) {
                session.frames.truncate(position);
            }
        }
    }

    pub(super) fn finish(self) -> Result<(), CodecError> {
        self.session(|session| {
            if session.frames.is_empty() {
                Ok(())
            } else {
                Err(CodecError::malformed(
                    "decode ended with unfinished read scopes",
                ))
            }
        })
    }

    pub(super) fn take_captures(self) -> Result<Vec<RecordCapture>, CodecError> {
        self.session(|session| {
            if !session.frames.is_empty() {
                return Err(CodecError::malformed(
                    "record captures are still inside a read scope",
                ));
            }
            if session.sealed {
                return Err(CodecError::malformed(
                    "record captures have already been extracted",
                ));
            }
            session.sealed = true;
            let mut captures = std::mem::take(&mut session.committed);
            captures.sort_unstable_by(|a, b| a.id.cmp(&b.id));
            Ok(captures)
        })
    }
}

impl Session {
    fn new(policy: DecodePolicy, input_bytes: u64) -> Self {
        Self {
            budget: DecodeBudget::new(policy, input_bytes),
            streams: Streams::default(),
            frames: Vec::new(),
            committed: Vec::new(),
            next_token: 0,
            error: None,
            sealed: false,
            context_alive: true,
            activated: false,
        }
    }

    fn activate(&mut self) -> Result<(), CodecError> {
        if !self.activated {
            self.budget.retain_bytes(
                u64_from_index(size_of::<SessionSlot>()),
                "record tracking session",
            )?;
            self.activated = true;
        }
        Ok(())
    }

    fn check(&self) -> Result<(), CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(limit.into());
        }
        if let Some(error) = self.error {
            return Err(CodecError::malformed(error));
        }
        Ok(())
    }

    fn new_record(&self, id: &str, extent: View<'_>) -> Result<RecordCapture, CodecError> {
        if id.is_empty() {
            return Err(CodecError::malformed("record identity is empty"));
        }
        let duplicate = self
            .committed
            .iter()
            .chain(self.frames.iter().flat_map(|frame| frame.captures.iter()))
            .any(|record| record.id == id)
            || self
                .frames
                .iter()
                .filter_map(|frame| frame.record.as_ref())
                .any(|record| record.id == id);
        if duplicate {
            return Err(CodecError::malformed("duplicate record read scope"));
        }
        let stream = self.streams.extent_name(
            extent.space(),
            super::ByteRange {
                start: u64_from_index(extent.start()),
                end: u64_from_index(extent.end()),
            },
        )?;
        Ok(RecordCapture {
            id: copy_string(&self.budget, id)?,
            stream: copy_string(&self.budget, stream)?,
            offset: u64_from_index(extent.start()),
            byte_len: u64_from_index(extent.end().abs_diff(extent.start())),
            reads: BTreeMap::new(),
        })
    }
}

fn reserve<T>(
    budget: &DecodeBudget,
    values: &mut Vec<T>,
    additional: usize,
) -> Result<(), CodecError> {
    let required = values
        .len()
        .checked_add(additional)
        .ok_or_else(|| budget.retained_allocation_failed(u64::MAX, "record tracking capacity"))?;
    if required <= values.capacity() {
        return Ok(());
    }
    let bytes = required
        .abs_diff(values.capacity())
        .checked_mul(size_of::<T>())
        .ok_or_else(|| budget.retained_allocation_failed(u64::MAX, "record tracking capacity"))?;
    budget.retain_bytes(u64_from_index(bytes), "record tracking storage")?;
    values.try_reserve_exact(additional).map_err(|_| {
        budget.retained_allocation_failed(u64_from_index(bytes), "record tracking storage")
    })
}

fn copy_string(budget: &DecodeBudget, value: &str) -> Result<String, CodecError> {
    budget.retain_bytes(u64_from_index(value.len()), "record tracking text")?;
    let mut text = String::new();
    text.try_reserve_exact(value.len()).map_err(|_| {
        budget.retained_allocation_failed(u64_from_index(value.len()), "record tracking text")
    })?;
    text.push_str(value);
    Ok(text)
}

fn merge_delta(
    budget: &DecodeBudget,
    deltas: &mut Vec<ReadDelta>,
    owner: usize,
    space: SpaceId,
    range: Range<u64>,
) -> Result<bool, CodecError> {
    let index = if let Some(index) = deltas
        .iter()
        .position(|delta| delta.owner == owner && delta.space == space)
    {
        index
    } else {
        reserve(budget, deltas, 1)?;
        deltas.push(ReadDelta {
            owner,
            space,
            ranges: Vec::new(),
        });
        deltas.len() - 1
    };
    let ranges = &mut deltas[index].ranges;
    let first = ranges.partition_point(|old| old.end < range.start);
    let after = ranges.partition_point(|old| old.start <= range.end);
    if first < after {
        if after == first + 1
            && ranges[first].start <= range.start
            && ranges[first].end >= range.end
        {
            return Ok(false);
        }
        let start = range.start.min(ranges[first].start);
        let end = range.end.max(ranges[after - 1].end);
        ranges[first] = start..end;
        drop(ranges.drain(first + 1..after));
    } else {
        reserve(budget, ranges, 1)?;
        ranges.insert(first, range);
    }
    Ok(true)
}

/// Staged record reads. Dropping without commit discards this scope and children.
#[derive(Debug)]
pub struct ReadTransaction<'a> {
    session: SessionHandle<'a>,
    token: usize,
    active: bool,
    _depth: super::DepthGuard<'a>,
}

impl<'a> ReadTransaction<'a> {
    pub(super) fn begin(
        session: SessionHandle<'a>,
        record: Option<(&str, View<'_>)>,
    ) -> Result<Self, CodecError> {
        let depth = session.enter_nested("record read transaction")?;
        let token = session.begin(record)?;
        Ok(Self {
            session,
            token,
            active: true,
            _depth: depth,
        })
    }
    /// Merge reads and captures into the containing transaction.
    pub fn commit(mut self) -> Result<(), CodecError> {
        self.session.commit(self.token)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for ReadTransaction<'_> {
    fn drop(&mut self) {
        if self.active {
            self.session.rollback(self.token);
        }
    }
}

#[cfg(test)]
mod tests;
