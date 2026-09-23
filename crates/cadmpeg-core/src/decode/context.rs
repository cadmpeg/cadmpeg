// SPDX-License-Identifier: Apache-2.0
//! Decode state, decompression limits, and session lifecycle.

use std::cell::Cell;
use std::io::SeekFrom;

use crate::{CodecError, ReadSeek};

use super::arena::DecodeArena;
use super::budget::{alloc_filled, DecodeBudget, DepthGuard, ScopedReservation, WorkBudget};
use super::error::{ResourceDimension, ResourceFailure, ResourceLimit};
use super::policy::{
    DecodePolicy, DECOMPRESSED_PER_EXPAND_BASE, DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE,
};
use super::space::{ByteRange, SpaceId};
use super::view::View;

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
    budget: DecodeBudget,
    derived_spaces: Cell<usize>,
}

impl<'a> DecodeContext<'a> {
    /// Reads the root input under `max_input_bytes`, copies it into the arena,
    /// registers the root space, establishes input-proportional allowances,
    /// and returns the context and root view.
    pub fn read_root(
        reader: &mut dyn ReadSeek,
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        container_only: bool,
    ) -> Result<(Self, View<'a>), CodecError> {
        let max = policy.limits.max_input_bytes;
        let cap = max.saturating_add(1);
        let size = match reader.seek(SeekFrom::End(0)) {
            Ok(size) => {
                reader.rewind().map_err(CodecError::Io)?;
                Some(size)
            }
            Err(_) => None,
        };
        let buffer = if let Some(size) = size {
            let reserve = size.min(cap);
            let reserve = usize::try_from(reserve)
                .map_err(|_| root_error(ResourceFailure::AllocationFailed, max, reserve))?;
            let mut buffer = Vec::new();
            buffer
                .try_reserve(reserve)
                .map_err(|_| root_error(ResourceFailure::AllocationFailed, max, reserve as u64))?;
            let mut chunk =
                alloc_filled(256 * 1024, 0_u8, "decode root read chunk")?.into_boxed_slice();
            while (buffer.len() as u64) < cap {
                let remaining = cap.saturating_sub(buffer.len() as u64);
                // The chunk length is already the bound: `remaining` above the
                // chunk means this read takes the whole chunk, so the chunk
                // length is the answer rather than a default standing in for
                // a conversion that could not be made.
                let want = match usize::try_from(remaining) {
                    Ok(remaining) if remaining < chunk.len() => remaining,
                    _ => chunk.len(),
                };
                let read = reader.read(&mut chunk[..want]).map_err(CodecError::Io)?;
                if read == 0 {
                    break;
                }
                buffer
                    .try_reserve(read)
                    .map_err(|_| root_error(ResourceFailure::AllocationFailed, max, read as u64))?;
                buffer.extend_from_slice(&chunk[..read]);
            }
            buffer
        } else {
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let remaining = cap.saturating_sub(buffer.len() as u64);
                if remaining == 0 {
                    break;
                }
                // The chunk length is already the bound: `remaining` above the
                // chunk means this read takes the whole chunk, so the chunk
                // length is the answer rather than a default standing in for
                // a conversion that could not be made.
                let want = match usize::try_from(remaining) {
                    Ok(remaining) if remaining < chunk.len() => remaining,
                    _ => chunk.len(),
                };
                let read = reader.read(&mut chunk[..want]).map_err(CodecError::Io)?;
                if read == 0 {
                    break;
                }
                buffer
                    .try_reserve(read)
                    .map_err(|_| root_error(ResourceFailure::AllocationFailed, max, read as u64))?;
                buffer.extend_from_slice(&chunk[..read]);
            }
            buffer
        };
        if buffer.len() as u64 > max {
            return Err(root_error(
                ResourceFailure::BudgetExceeded,
                max,
                buffer.len() as u64,
            ));
        }
        let bytes = arena.alloc(buffer.into_boxed_slice());
        Self::from_bytes(bytes, arena, policy, container_only)
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

    fn from_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        container_only: bool,
    ) -> Result<(Self, View<'a>), CodecError> {
        let length = bytes.len() as u64;
        if length > policy.limits.max_input_bytes {
            return Err(root_error(
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

    fn per_expand_allowance(&self) -> u64 {
        let proportional = DECOMPRESSED_PER_EXPAND_BASE.saturating_add(
            DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE.saturating_mul(self.budget.input_bytes()),
        );
        self.budget
            .policy()
            .limits
            .max_decompressed_bytes_per_expand
            .min(proportional)
    }

    fn allocate_space(&self) -> Result<SpaceId, CodecError> {
        let used = self.derived_spaces.get();
        let index = used.checked_add(1).ok_or_else(|| {
            // Root owns zero; every other `usize` value identifies a derived space.
            self.budget.refuse(
                ResourceDimension::Codec("decode address spaces"),
                ResourceFailure::BudgetExceeded,
                usize::MAX as u64,
                used as u64,
                1,
                "decode address spaces",
            )
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

    /// Reserves bytes held by a temporary materialization.
    pub fn reserve_scoped(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, CodecError> {
        self.budget.reserve_scoped(bytes, operation)
    }

    /// Charges bytes retained for the remainder of this session.
    pub fn charge_retained(&self, bytes: u64, operation: &'static str) -> Result<(), CodecError> {
        self.budget.charge_retained(bytes, operation)
    }

    /// Copies bytes into session-retained storage after charging and reserving safely.
    pub fn copy_retained(
        &self,
        bytes: &[u8],
        operation: &'static str,
    ) -> Result<Vec<u8>, CodecError> {
        self.charge_retained(bytes.len() as u64, operation)?;
        let mut copy = Vec::new();
        copy.try_reserve_exact(bytes.len()).map_err(|_| {
            self.budget
                .retained_allocation_failed(bytes.len() as u64, operation)
        })?;
        copy.extend_from_slice(bytes);
        Ok(copy)
    }

    /// Allocates `count` copies of `value` after charging collection items and
    /// reserving without panicking on allocator refusal.
    ///
    /// Prefer this over `vec![value; parsed_count]` for attacker-influenced sizes.
    pub fn alloc_filled<T: Clone>(
        &self,
        count: usize,
        value: T,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.charge_collection_items(count as u64, operation)?;
        alloc_filled(count, value, operation)
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
        let additional = current.saturating_sub(*admitted);
        self.charge_entities(additional, operation)?;
        *admitted = current;
        Ok(())
    }

    /// Charges admitted collection items.
    pub fn charge_collection_items(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.budget.charge_collection_items(count, operation)
    }

    /// Enters one recursive nesting level until the returned guard is dropped.
    pub fn enter_nested(&self, operation: &'static str) -> Result<DepthGuard<'_>, CodecError> {
        self.budget.enter_nested(operation)
    }

    /// Charges session-global algorithm work, fusing on refusal.
    pub fn charge_work(&self, units: u64, operation: &'static str) -> Result<(), CodecError> {
        self.budget.charge_work(units, operation)
    }

    /// Permanently refuses a codec-local resource request.
    ///
    /// Codecs use this when a bounded recovery algorithm reaches a fixed
    /// local ceiling instead of a session-wide dimension. The refusal fuses
    /// the session so a caller cannot accidentally turn it into a semantic
    /// fallback or report success after the limit was reached.
    pub fn refuse_codec_limit(
        &self,
        operation: &'static str,
        limit: u64,
        requested: u64,
    ) -> CodecError {
        self.budget.refuse(
            ResourceDimension::Codec(operation),
            ResourceFailure::BudgetExceeded,
            limit,
            requested.min(limit),
            requested.saturating_sub(limit),
            operation,
        )
    }

    /// Creates a local work slice that also draws from the session allowance.
    pub fn work_budget(&self, local_limit: u64) -> WorkBudget<'_> {
        WorkBudget::for_session(local_limit, &self.budget)
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
            if size
                > self
                    .decompression_allowance()
                    .saturating_sub(self.budget.decompressed_used())
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
                    reserve as u64,
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
                    total as u64,
                    view.window().len() as u64,
                    "concat_views",
                )
            })
        })?;
        let reservation = self.reserve_scoped(total as u64, "concat_views")?;
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(total).map_err(|_| {
            self.budget.refuse(
                ResourceDimension::MaterializedBytes,
                ResourceFailure::AllocationFailed,
                self.budget.policy().limits.max_materialized_bytes,
                0,
                total as u64,
                "concat_views",
            )
        })?;
        for view in inputs {
            buffer.extend_from_slice(view.window());
        }
        let bytes = self.arena.alloc(buffer.into_boxed_slice());
        reservation.commit()?;
        let space = self.allocate_space()?;
        Ok(View::over_space(bytes, space))
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

/// Builds the root-input resource error before a context exists.
fn root_error(reason: ResourceFailure, limit: u64, used: u64) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::InputBytes,
        reason,
        limit,
        used,
        additional: used.saturating_sub(limit),
        operation: "read_root",
    })
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
        let len = data.len() as u64;
        let new_written = self.written().saturating_add(len);
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
        if let ExpandSpec::Exact(size) = self.spec {
            if self.written() != size {
                return Err(CodecError::malformed(format_args!(
                    "expansion produced {} of declared exact {size} bytes",
                    self.written()
                )));
            }
        }
        let bytes = self.ctx.arena.alloc(self.buffer.into_boxed_slice());
        let space = self.ctx.allocate_space()?;
        Ok(View::over_space(bytes, space))
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        self.buffer.len() as u64
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
        let crate::CodecError::ResourceLimit(limit) = error else {
            panic!("exhausted IDs return a resource refusal");
        };
        assert_eq!(limit.limit, usize::MAX as u64);
        assert_eq!(limit.used, usize::MAX as u64);
        assert_eq!(limit.additional, 1);
        assert!(ctx.register_slice(root, range).is_err());
        assert!(ctx.finish_session().is_err());
    }
}
