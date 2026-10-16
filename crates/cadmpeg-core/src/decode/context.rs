// SPDX-License-Identifier: Apache-2.0
//! Decode state, decompression limits, and session lifecycle.

use std::cell::Cell;
use std::io::SeekFrom;

use crate::{CodecError, ReadSeek};

use super::arena::DecodeArena;
use super::budget::{DecodeBudget, DepthGuard};
use super::error::{ResourceDimension, ResourceFailure, ResourceLimit};
use super::policy::{
    DecodePolicy, DECOMPRESSED_PER_EXPAND_BASE, DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE,
};
use super::space::{ByteRange, SpaceId};
use super::view::{u64_from_index, View};

#[derive(Clone, Copy)]
enum LimitScope {
    Global,
    PerExpand,
}

/// Cap on the initial per-expand reservation before any output is produced.
const RESERVE_CLAMP: usize = 8 * 1024 * 1024;

/// Shared monotonic decode state.
#[derive(Debug)]
pub struct DecodeContext<'a> {
    arena: &'a DecodeArena,
    container_only: bool,
    pub(super) budget: DecodeBudget,
    derived_spaces: Cell<usize>,
}

impl<'a> DecodeContext<'a> {
    /// Creates one session before input acquisition and detection.
    pub fn new(arena: &'a DecodeArena, policy: &DecodePolicy, container_only: bool) -> Self {
        Self {
            arena,
            container_only,
            budget: DecodeBudget::new(*policy, 0),
            derived_spaces: Cell::new(0),
        }
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
        let ctx = Self::new(arena, policy, false);
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
        container_only: bool,
    ) -> Result<(Self, View<'a>), CodecError> {
        if reader.seek(SeekFrom::End(0)).is_ok() {
            reader.rewind().map_err(CodecError::Io)?;
        }
        let ctx = Self::new(arena, policy, container_only);
        let mut buffer = Vec::new();
        ctx.complete_input(reader, &mut buffer)?;
        let bytes = arena.alloc(buffer.into_boxed_slice());
        Ok((ctx, View::over_space(bytes, SpaceId::ROOT)))
    }

    /// Builds a context over caller-owned root bytes, for fuzz targets and
    /// tests. The arena still backs any expansions produced during decode.
    pub fn from_root_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), CodecError> {
        Self::from_bytes(bytes, arena, policy, false)
    }

    /// Builds a root context with a resource-only refusal channel.
    pub fn from_root_bytes_limit(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), ResourceLimit> {
        Self::from_bytes_limit(bytes, arena, policy, false)
    }

    fn from_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        container_only: bool,
    ) -> Result<(Self, View<'a>), CodecError> {
        Self::from_bytes_limit(bytes, arena, policy, container_only).map_err(Into::into)
    }

    fn from_bytes_limit(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        container_only: bool,
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
            container_only,
            budget: DecodeBudget::new(*policy, length),
            derived_spaces: Cell::new(0),
        };
        Ok((ctx, View::over_space(bytes, SpaceId::ROOT)))
    }

    /// Returns the decode policy in force.
    pub fn policy(&self) -> &DecodePolicy {
        self.budget.policy()
    }

    /// Returns whether the caller requested container-only decoding.
    pub fn container_only(&self) -> bool {
        self.container_only
    }

    fn decompression_allowance(&self) -> u64 {
        self.budget.decompression_allowance()
    }

    pub(super) fn per_expand_allowance(&self) -> u64 {
        self.budget.proportional_allowance(
            self.budget
                .policy()
                .limits
                .max_decompressed_bytes_per_expand,
            DECOMPRESSED_PER_EXPAND_BASE,
            DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE,
        )
    }

    /// Charges bytes that decoded output keeps for the rest of the session.
    ///
    /// This is the one retained-bytes chokepoint: native record text, arena
    /// buffers, and retained copies of source bytes call it. Temporary and
    /// derived storage is not charged.
    pub fn retain_bytes(&self, bytes: u64, operation: &'static str) -> Result<(), CodecError> {
        self.budget
            .retain_bytes(bytes, operation)
            .map_err(Into::into)
    }

    /// Retained bytes the session can still afford: the retained-byte
    /// allowance (the smaller of `max_retained_bytes` and the
    /// input-proportional allowance) minus the bytes already retained, or zero
    /// after a refusal fused the session.
    ///
    /// A codec uses this only to decide whether to skip optional enrichment
    /// before it builds the enrichment, and reports the skip as a loss. It
    /// does not reserve anything: the bytes the codec keeps are still charged
    /// through [`DecodeContext::retain_bytes`], which can still refuse.
    #[must_use]
    pub fn retained_bytes_available(&self) -> u64 {
        self.budget.retained_bytes_available()
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

    /// Records a permanent fuse and returns the resource error to propagate.
    fn fuse(
        &self,
        reason: ResourceFailure,
        scope: LimitScope,
        amount: u64,
        operation: &'static str,
    ) -> CodecError {
        let limit = match scope {
            LimitScope::Global => self.decompression_allowance(),
            LimitScope::PerExpand => self.per_expand_allowance(),
        };
        self.budget.refuse(
            ResourceDimension::DecompressedBytes,
            reason,
            limit,
            self.budget.decompressed_used(),
            amount,
            operation,
        )
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
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(count), operation)
        })?;
        values.resize(count, value);
        Ok(values)
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

    /// Returns the resource refusal that has fused this decode session.
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
                    LimitScope::PerExpand,
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
                    LimitScope::Global,
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
                    LimitScope::PerExpand,
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
                LimitScope::PerExpand,
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
                LimitScope::PerExpand,
                len,
                "expand_write",
            ));
        }
        self.ctx.budget.charge_decompressed(len, "expand_write")?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
                ResourceFailure::AllocationFailed,
                LimitScope::PerExpand,
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
    use super::{ByteRange, DecodeArena, DecodeContext, DecodePolicy};

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
        let (_, root) =
            DecodeContext::read_root(&mut reader, &arena, &DecodePolicy::default(), false)
                .expect("root is admitted");
        assert_eq!(root.window(), bytes.as_slice());
    }

    #[test]
    fn root_reader_propagates_failed_rewind_after_a_successful_size_probe() {
        let arena = DecodeArena::new();
        let mut reader = RewindFails(Cursor::new(b"not empty".to_vec()));
        assert!(matches!(
            DecodeContext::read_root(&mut reader, &arena, &DecodePolicy::default(), false),
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
