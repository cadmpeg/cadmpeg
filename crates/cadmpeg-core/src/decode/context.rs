// SPDX-License-Identifier: Apache-2.0
//! Decode state, decompression limits, and session lifecycle.

use std::cell::Cell;
use std::io::SeekFrom;

use crate::{CodecError, ReadSeek};

use super::arena::DecodeArena;
use super::budget::{DecodeBudget, DepthGuard};
use super::depth::DecodeDepth;
use super::error::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimit};
use super::policy::{
    DecodePolicy, DECOMPRESSED_PER_EXPAND_BASE, DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE,
};
use super::space::{ByteRange, SpaceId};
use super::view::{u64_from_index, View};

#[derive(Clone, Copy)]
enum AllowanceScope {
    Global,
    PerExpand,
}

/// Cap on the initial per-expand reservation before any output is produced.
const RESERVE_CLAMP: usize = 8 * 1024 * 1024;

/// Shared monotonic decode state.
#[derive(Debug)]
pub struct DecodeContext<'a> {
    arena: &'a DecodeArena,
    depth: DecodeDepth,
    encoding: bool,
    pub(super) budget: DecodeBudget,
    derived_spaces: Cell<usize>,
}

impl<'a> DecodeContext<'a> {
    pub(crate) fn operation_budget(
        &self,
    ) -> std::sync::Arc<crate::operation_budget::OperationBudget> {
        self.budget.operation_budget()
    }

    /// Creates one session before input acquisition and detection.
    pub fn new(arena: &'a DecodeArena, policy: &DecodePolicy, depth: DecodeDepth) -> Self {
        Self {
            arena,
            depth,
            encoding: false,
            budget: DecodeBudget::new(*policy, 0),
            derived_spaces: Cell::new(0),
        }
    }

    /// Creates source-free encode helpers under the caller's absolute ceilings.
    ///
    /// Retained helper output shares the account used by
    /// [`crate::encode::EncodeSession::from_context`]. No source byte length
    /// or source stream is invented to obtain an encode allowance.
    pub fn for_encode(arena: &'a DecodeArena, policy: &DecodePolicy) -> Self {
        let mut ctx = Self::new(arena, policy, DecodeDepth::Full);
        ctx.encoding = true;
        ctx
    }

    /// Opens an independent source domain on an existing encode caller account.
    ///
    /// Caller policy and ceilings are inherited from the session. This neither
    /// registers source bytes nor borrows another context or its arena. Input,
    /// decompressed bytes, retained output, entities and the first refusal are
    /// shared; local work stays separate.
    pub fn for_encode_session(
        arena: &'a DecodeArena,
        session: &crate::encode::EncodeSession,
    ) -> Result<Self, ResourceLimit> {
        session.finish()?;
        let policy = session.policy;
        let budget = DecodeBudget::from_account(policy, session.account());
        Ok(Self {
            arena,
            depth: DecodeDepth::Full,
            encoding: true,
            budget,
            derived_spaces: Cell::new(0),
        })
    }

    /// Whether this context owns temporary projections for an encode operation.
    pub const fn is_encoding(&self) -> bool {
        self.encoding
    }

    /// Starts post-decode processing with the measured byte length of an already
    /// loaded input. This admits the input-proportional allowances without
    /// keeping another copy of the source bytes. Format-declared counts must
    /// not be used as the input length.
    pub fn for_loaded_input(
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        input_bytes: u64,
    ) -> Result<Self, CodecError> {
        let ctx = Self::new(arena, policy, DecodeDepth::Full);
        ctx.budget
            .charge_input(input_bytes, "admit loaded input length")?;
        Ok(ctx)
    }

    /// Reads the root input under `max_input_bytes`, copies it into the arena,
    /// registers the root space, establishes input-proportional allowances,
    /// and returns the context and root view.
    pub fn read_root(
        reader: &mut dyn ReadSeek,
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        depth: DecodeDepth,
    ) -> Result<(Self, View<'a>), CodecError> {
        if reader.seek(SeekFrom::End(0)).is_ok() {
            reader.rewind().map_err(CodecError::Io)?;
        }
        let ctx = Self::new(arena, policy, depth);
        let mut buffer = Vec::new();
        ctx.complete_input(reader, &mut buffer)?;
        let bytes = arena.alloc(buffer.into_boxed_slice());
        Ok((ctx, View::over_space(bytes, SpaceId::ROOT)))
    }

    /// Builds a full-depth context over caller-owned root bytes, for fuzz
    /// targets and tests. The arena still backs any expansions produced during
    /// decode.
    pub fn from_root_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), CodecError> {
        Self::from_root_bytes_at(bytes, arena, policy, DecodeDepth::Full)
    }

    /// Builds a context over caller-owned root bytes that requests `depth`.
    pub fn from_root_bytes_at(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        depth: DecodeDepth,
    ) -> Result<(Self, View<'a>), CodecError> {
        Self::from_bytes_limit(bytes, arena, policy, depth).map_err(Into::into)
    }

    /// Builds a full-depth root context with a resource-only refusal channel.
    pub fn from_root_bytes_limit(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), ResourceLimit> {
        Self::from_bytes_limit(bytes, arena, policy, DecodeDepth::Full)
    }

    fn from_bytes_limit(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        depth: DecodeDepth,
    ) -> Result<(Self, View<'a>), ResourceLimit> {
        let length = u64_from_index(bytes.len());
        if length > policy.limits.max_input_bytes {
            return Err(root_limit(
                ResourceFailure::BudgetExceeded,
                policy.limits.max_input_bytes,
                length,
            ));
        }
        let ctx = DecodeContext {
            arena,
            depth,
            encoding: false,
            budget: DecodeBudget::new(*policy, length),
            derived_spaces: Cell::new(0),
        };
        Ok((ctx, View::over_space(bytes, SpaceId::ROOT)))
    }

    /// Returns the decode policy in force.
    pub fn policy(&self) -> &DecodePolicy {
        self.budget.policy()
    }

    /// Returns the depth the caller requested.
    pub fn depth(&self) -> DecodeDepth {
        self.depth
    }

    /// Returns whether the caller requested only the container layer.
    pub fn container_only(&self) -> bool {
        self.depth == DecodeDepth::Container
    }

    fn decompression_allowance(&self) -> u64 {
        self.budget.decompression_allowance()
    }

    /// The largest size one expansion may declare: the smaller of
    /// `max_decompressed_bytes_per_expand` and its input-proportional
    /// allowance.
    ///
    /// A codec compares a declared expansion size against it before
    /// [`DecodeContext::begin_expand`], so a declared size above it is a
    /// per-input bound that skips one member instead of fusing the session.
    #[must_use]
    pub fn per_expand_allowance(&self) -> u64 {
        self.budget.proportional_allowance(
            self.budget
                .policy()
                .limits
                .max_decompressed_bytes_per_expand,
            DECOMPRESSED_PER_EXPAND_BASE,
            DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE,
        )
    }

    /// Charges bytes kept in output for the rest of the operation.
    ///
    /// Decode uses its input-proportional allowance. Encode helpers use the
    /// shared account's absolute caller ceiling. Temporary and derived storage
    /// is not charged.
    pub fn retain_bytes(&self, bytes: u64, operation: &'static str) -> Result<(), CodecError> {
        if self.encoding {
            return self
                .operation_budget()
                .charge_retained(bytes, operation)
                .map_err(Into::into);
        }
        self.budget
            .retain_bytes(bytes, operation)
            .map_err(Into::into)
    }

    /// Retained bytes the operation can still afford, or zero after a refusal.
    ///
    /// Decode uses its input-proportional allowance. Encode helpers use the
    /// absolute caller ceiling. Both include actual live serialized output.
    ///
    /// A codec uses this only to decide whether to skip optional enrichment
    /// before it builds the enrichment, and reports the skip as a loss. It
    /// does not reserve anything: the bytes the codec keeps are still charged
    /// through [`DecodeContext::retain_bytes`], which can still refuse.
    #[must_use]
    pub fn retained_bytes_available(&self) -> u64 {
        if self.encoding {
            return self.operation_budget().retained_bytes_available();
        }
        self.budget.retained_bytes_available()
    }

    fn allocation_failed(&self, requested: u64, operation: &'static str) -> CodecError {
        if !self.encoding {
            return self.budget.retained_allocation_failed(requested, operation);
        }
        let account = self.operation_budget();
        let failure = ResourceLimit {
            scope: LimitScope::Session,
            dimension: ResourceDimension::RetainedBytes,
            reason: ResourceFailure::AllocationFailed,
            limit: account.limits.max_retained_bytes,
            used: account.retained_used(),
            additional: requested,
            operation,
        };
        account.refuse(failure).into()
    }

    fn allocate_space(&self) -> Result<SpaceId, CodecError> {
        let used = self.derived_spaces.get();
        // Root owns zero; every other `usize` value identifies a derived space.
        let index = used.checked_add(1).ok_or_else(|| {
            CodecError::NotImplemented("decode address spaces are exhausted".into())
        })?;
        self.derived_spaces.set(index);
        Ok(SpaceId::from_index(index))
    }

    /// Records a scoped refusal and returns the resource error to propagate.
    fn fuse(
        &self,
        reason: ResourceFailure,
        scope: AllowanceScope,
        local_used: u64,
        amount: u64,
        operation: &'static str,
    ) -> CodecError {
        let limit = match scope {
            AllowanceScope::Global => self.decompression_allowance(),
            AllowanceScope::PerExpand => self.per_expand_allowance(),
        };
        self.budget
            .refuse_resource(ResourceLimit {
                scope: match scope {
                    AllowanceScope::Global => LimitScope::Session,
                    AllowanceScope::PerExpand => LimitScope::Part,
                },
                dimension: ResourceDimension::DecompressedBytes,
                reason,
                limit,
                used: match scope {
                    AllowanceScope::Global => self.budget.decompressed_used(),
                    AllowanceScope::PerExpand => local_used,
                },
                additional: amount,
                operation,
            })
            .into()
    }

    /// Allocates `count` copies of `value` with fallible allocation.
    ///
    /// Use this instead of `vec![value; parsed_count]` for a count read from
    /// the input; validate the count against the bytes it describes first
    /// (`View::counted`, `bounded_len`).
    pub fn alloc_filled<T: Clone>(
        &self,
        count: usize,
        value: T,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        if self.encoding {
            if let Some(refusal) = self.resource_refusal() {
                return Err(refusal.into());
            }
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| self.allocation_failed(u64_from_index(count), operation))?;
        values.resize(count, value);
        Ok(values)
    }

    /// Reserves temporary vector storage without charging retained output.
    pub fn reserve_working<T>(
        &self,
        values: &mut Vec<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(refusal) = self.resource_refusal() {
            return Err(refusal.into());
        }
        let bytes = additional
            .checked_mul(size_of::<T>())
            .ok_or_else(|| self.allocation_failed(u64::MAX, operation))?;
        values
            .try_reserve_exact(additional)
            .map_err(|_| self.allocation_failed(u64_from_index(bytes), operation))?;
        Ok(())
    }

    /// Move a value into fallible working heap storage.
    ///
    /// This does not charge retained output or consume an entity slot.
    ///
    /// # Errors
    ///
    /// Returns the caller's existing refusal, or records an allocation failure.
    pub fn alloc_box_working<T>(
        &self,
        value: T,
        operation: &'static str,
    ) -> Result<Box<T>, CodecError> {
        if let Some(refusal) = self.resource_refusal() {
            return Err(refusal.into());
        }
        if size_of::<T>() == 0 {
            return Ok(Box::new(value));
        }
        trybox::or_drop(value)
            .map_err(|_| self.allocation_failed(u64_from_index(size_of::<T>()), operation))
    }

    /// Reserves temporary UTF-8 string storage without charging retained output.
    pub fn reserve_text_working(
        &self,
        text: &mut String,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(refusal) = self.resource_refusal() {
            return Err(refusal.into());
        }
        text.try_reserve_exact(additional)
            .map_err(|_| self.allocation_failed(u64_from_index(additional), operation))?;
        Ok(())
    }

    /// Charges admitted entities.
    pub fn charge_entities(&self, count: u64, operation: &'static str) -> Result<(), CodecError> {
        self.budget.charge_entities(count, operation)
    }

    /// Charges entities newly present since `admitted`, then advances `admitted`.
    ///
    /// Codecs call this at admission boundaries so `max_entities` refuses further
    /// work instead of only reporting after a finished IR is built.
    pub fn admit_entities(
        &self,
        current: u64,
        admitted: &mut u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if current < *admitted {
            *admitted = current;
            return Ok(());
        }
        let additional = current - *admitted;
        self.charge_entities(additional, operation)?;
        *admitted = current;
        Ok(())
    }

    /// Enters one recursive nesting level until the returned guard is dropped.
    pub fn enter_nested(&self, operation: &'static str) -> Result<DepthGuard<'_>, CodecError> {
        self.enter_nested_limit(operation).map_err(Into::into)
    }

    /// Enters one nesting level with a resource-only refusal.
    pub fn enter_nested_limit(
        &self,
        operation: &'static str,
    ) -> Result<DepthGuard<'_>, ResourceLimit> {
        self.budget.enter_nested(operation)
    }

    /// Returns the pending part or session resource refusal.
    pub fn resource_refusal(&self) -> Option<ResourceLimit> {
        self.budget.fused()
    }

    // --- decompression ------------------------------------------------------

    /// Begins an expansion whose output is charged incrementally and becomes
    /// available only after successful finalization.
    pub fn begin_expand(&self, spec: ExpandSpec) -> Result<ExpandWriter<'_, 'a>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        if let ExpandSpec::Exact(size) = spec {
            let per_expand = self.per_expand_allowance();
            if size > per_expand {
                return Err(self.fuse(
                    ResourceFailure::BudgetExceeded,
                    AllowanceScope::PerExpand,
                    0,
                    size,
                    "begin_expand",
                ));
            }
            if self
                .decompression_allowance()
                .checked_sub(self.budget.decompressed_used())
                .is_none_or(|remaining| size > remaining)
            {
                return Err(self.fuse(
                    ResourceFailure::BudgetExceeded,
                    AllowanceScope::Global,
                    0,
                    size,
                    "begin_expand",
                ));
            }
        }
        let mut buffer: Vec<u8> = Vec::new();
        let reserve = match spec {
            // A declared size the address space cannot name is above the cap,
            // so the cap is the reservation either way.
            ExpandSpec::Exact(size) => match usize::try_from(size) {
                Ok(size) => size.min(RESERVE_CLAMP),
                Err(_) => RESERVE_CLAMP,
            },
            ExpandSpec::Unknown => 0,
        };
        if reserve > 0 {
            buffer.try_reserve(reserve).map_err(|_| {
                self.fuse(
                    ResourceFailure::AllocationFailed,
                    AllowanceScope::PerExpand,
                    0,
                    u64_from_index(reserve),
                    "begin_expand",
                )
            })?;
        }
        Ok(ExpandWriter {
            ctx: self,
            spec,
            buffer,
        })
    }

    /// Copies several input extents into one derived view.
    pub fn concat_views(&self, inputs: &[View<'_>]) -> Result<View<'a>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        if inputs.is_empty() {
            return Err(CodecError::Malformed(
                "cannot concatenate an empty view list".into(),
            ));
        }
        let total = inputs.iter().try_fold(0usize, |total, view| {
            total.checked_add(view.window().len()).ok_or_else(|| {
                self.budget.refuse(
                    ResourceDimension::RetainedBytes,
                    ResourceFailure::BudgetExceeded,
                    self.budget.policy().limits.max_retained_bytes,
                    u64_from_index(total),
                    u64_from_index(view.window().len()),
                    "concat_views",
                )
            })
        })?;
        self.retain_bytes(u64_from_index(total), "concat_views")?;
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(total).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(total), "concat_views")
        })?;
        for view in inputs {
            buffer.extend_from_slice(view.window());
        }
        let bytes = self.arena.alloc(buffer.into_boxed_slice());
        let space = self.allocate_space()?;
        Ok(View::over_space(bytes, space))
    }

    /// Concatenates owned chunks into one retained buffer without an arena copy.
    pub fn concat_retained(
        &self,
        inputs: &[Vec<u8>],
        operation: &'static str,
    ) -> Result<Vec<u8>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        if inputs.is_empty() {
            return Err(CodecError::Malformed(
                "cannot concatenate an empty buffer list".into(),
            ));
        }
        let total = inputs.iter().try_fold(0_usize, |total, input| {
            total.checked_add(input.len()).ok_or_else(|| {
                CodecError::NotImplemented("retained concatenation exceeds usize".into())
            })
        })?;
        let total_bytes = crate::decode::u64_from_index(total);
        self.retain_bytes(total_bytes, operation)?;
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(total).map_err(|_| {
            self.budget
                .retained_allocation_failed(total_bytes, operation)
        })?;
        for input in inputs {
            buffer.extend_from_slice(input);
        }
        Ok(buffer)
    }

    /// Registers a stored (uncompressed) child range as a space that borrows
    /// the parent bytes without copying.
    ///
    /// `range` is expressed in the parent view's own space coordinates and must
    /// lie within the parent window; a range that escapes the parent is refused
    /// here, at the request site, exactly as [`View::child`] refuses. No bytes
    /// are copied. It is the archive-entry counterpart of [`DecodeContext::begin_expand`] —
    /// stored ZIP entries take this path, compressed ones take the expander.
    /// Registration still refuses on a fused context so a stored entry cannot be
    /// admitted after a refusal.
    pub fn register_slice<'v>(
        &self,
        parent: View<'v>,
        range: ByteRange,
    ) -> Result<View<'v>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let start = usize::try_from(range.start).ok();
        let end = usize::try_from(range.end).ok();
        let child = start
            .zip(end)
            .and_then(|(start, end)| parent.child(start, end))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "stored slice [{}, {}) escapes parent space {}",
                    range.start,
                    range.end,
                    parent.space().index()
                ))
            })?;

        let space = self.allocate_space()?;
        Ok(View::over_space(child.window(), space))
    }

    // --- lifecycle ----------------------------------------------------------

    /// Closes a decode or inspection session, returning a fused resource error
    /// even when codec code swallowed the charge that caused it.
    pub fn finish_session(self) -> Result<(), CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        Ok(())
    }
}

fn root_limit(reason: ResourceFailure, limit: u64, used: u64) -> ResourceLimit {
    if used <= limit {
        return ResourceLimit {
            scope: LimitScope::Session,
            dimension: ResourceDimension::InputBytes,
            reason,
            limit,
            used,
            additional: 0,
            operation: "read_root",
        };
    }
    let additional = used - limit;
    ResourceLimit {
        scope: LimitScope::Session,
        dimension: ResourceDimension::InputBytes,
        reason,
        limit,
        used,
        additional,
        operation: "read_root",
    }
}

/// How much output an expansion is expected to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandSpec {
    /// A declared exact size, enforced per-write and at finalize.
    Exact(u64),
    /// No trustworthy declared size: the decompression limits apply.
    Unknown,
}

/// Writes decompressed output under incremental charging.
#[derive(Debug)]
pub struct ExpandWriter<'ctx, 'a> {
    ctx: &'ctx DecodeContext<'a>,
    spec: ExpandSpec,
    buffer: Vec<u8>,
}

impl<'a> ExpandWriter<'_, 'a> {
    /// Appends decompressed output, charging before it is retained.
    pub fn write(&mut self, data: &[u8]) -> Result<(), CodecError> {
        let len = u64_from_index(data.len());
        let new_written = self.written().checked_add(len).ok_or_else(|| {
            self.ctx.fuse(
                ResourceFailure::BudgetExceeded,
                AllowanceScope::PerExpand,
                self.written(),
                len,
                "expand_write",
            )
        })?;
        match self.spec {
            ExpandSpec::Exact(size) if new_written > size => {
                return Err(CodecError::malformed(format_args!(
                    "expansion exceeded declared exact size {size}"
                )))
            }
            _ => {}
        }
        let per_expand = self.ctx.per_expand_allowance();
        if new_written > per_expand {
            return Err(self.ctx.fuse(
                ResourceFailure::BudgetExceeded,
                AllowanceScope::PerExpand,
                self.written(),
                len,
                "expand_write",
            ));
        }
        self.ctx.budget.charge_decompressed(len, "expand_write")?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
                ResourceFailure::AllocationFailed,
                AllowanceScope::PerExpand,
                self.written(),
                len,
                "expand_write",
            )
        })?;
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// Finalizes the expansion, stores it in the arena, and registers its space.
    pub fn finalize(self) -> Result<View<'a>, CodecError> {
        self.check_exact()?;
        let bytes = self.ctx.arena.alloc(self.buffer.into_boxed_slice());
        let space = self.ctx.allocate_space()?;
        Ok(View::over_space(bytes, space))
    }

    /// Finalizes an expansion directly into a caller-owned buffer.
    /// The expansion budget already admits its bytes, so no retained copy occurs.
    pub fn finalize_owned(self) -> Result<Vec<u8>, CodecError> {
        self.check_exact()?;
        Ok(self.buffer)
    }

    fn check_exact(&self) -> Result<(), CodecError> {
        if let ExpandSpec::Exact(size) = self.spec {
            if self.written() != size {
                return Err(CodecError::malformed(format_args!(
                    "expansion produced {} of declared exact {size} bytes",
                    self.written()
                )));
            }
        }
        Ok(())
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        u64_from_index(self.buffer.len())
    }
}

#[cfg(test)]
mod tests {
    mod shared_encode;
    mod working_box;
    mod working_storage;
    mod working_text;

    use super::{ByteRange, DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};

    use std::io::{self, Cursor, Read, Seek, SeekFrom};

    struct RewindFails(Cursor<Vec<u8>>);

    impl Read for RewindFails {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.0.read(buffer)
        }
    }

    impl Seek for RewindFails {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            if position == SeekFrom::Start(0) {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "rewind denied",
                ))
            } else {
                self.0.seek(position)
            }
        }
    }

    #[test]
    fn root_reader_preserves_bytes_across_fixed_chunk_boundaries() {
        let mut bytes = vec![0x5a_u8; 8193];
        bytes[8192] = 0x7f;
        let mut reader = Cursor::new(bytes.clone());
        let arena = DecodeArena::new();
        let (_, root) = DecodeContext::read_root(
            &mut reader,
            &arena,
            &DecodePolicy::default(),
            DecodeDepth::Full,
        )
        .expect("root is admitted");
        assert_eq!(root.window(), bytes.as_slice());
    }

    #[test]
    fn root_reader_propagates_failed_rewind_after_a_successful_size_probe() {
        let arena = DecodeArena::new();
        let mut reader = RewindFails(Cursor::new(b"not empty".to_vec()));
        assert!(matches!(
            DecodeContext::read_root(
                &mut reader,
                &arena,
                &DecodePolicy::default(),
                DecodeDepth::Full
            ),
            Err(crate::CodecError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn exhausted_space_ids_refuse_registration_without_reusing_an_id() {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(b"x", &arena, &DecodePolicy::default())
            .expect("test input fits the policy");
        ctx.derived_spaces.set(usize::MAX - 1);
        let range = ByteRange { start: 0, end: 1 };
        let view = ctx
            .register_slice(root, range)
            .expect("maximum space id is allocatable");
        assert_eq!(view.space().index(), usize::MAX);
        let error = ctx
            .register_slice(root, range)
            .expect_err("space IDs are exhausted");
        assert!(matches!(error, crate::CodecError::NotImplemented(_)));
        assert!(ctx.register_slice(root, range).is_err());
    }
}
