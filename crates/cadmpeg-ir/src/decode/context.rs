// SPDX-License-Identifier: Apache-2.0
//! Decode state, decompression limits, and session lifecycle.

use std::cell::Cell;

use crate::codec::{CodecError, DecodeResult, ReadSeek};
use crate::report::StrictConsequence;

use super::arena::DecodeArena;
use super::error::{
    ErrorContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation,
};
use super::policy::{
    DecodeMode, DecodePolicy, DECOMPRESSED_PER_EXPAND_BASE, DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE,
    DECOMPRESSED_TOTAL_BASE, DECOMPRESSED_TOTAL_PER_INPUT_BYTE,
};
use super::space::{ByteRange, SpaceId};
use super::view::View;

/// Initial per-expand reservation clamp: an attacker's declared size cannot
/// force a large up-front reservation before any output is produced.
const RESERVE_CLAMP: u64 = 8 * 1024 * 1024;

/// Shared monotonic decode state.
#[derive(Debug)]
pub struct DecodeContext<'a> {
    arena: &'a DecodeArena,
    policy: DecodePolicy,
    container_only: bool,
    input_bytes: u64,
    decompressed_bytes: Cell<u64>,
    next_space: Cell<u32>,
    fuse: Cell<Option<ResourceLimit>>,
}

impl<'a> DecodeContext<'a> {
    /// Reads the root input under `max_input_bytes`, copies it into the arena,
    /// registers the root space, establishes input-proportional allowances,
    /// and returns the context and root view.
    pub fn read_root(
        reader: &mut dyn ReadSeek,
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), CodecError> {
        let max = policy.limits.max_input_bytes;
        let cap = max.saturating_add(1);
        let mut buffer: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let remaining = cap.saturating_sub(buffer.len() as u64);
            if remaining == 0 {
                break;
            }
            let want = usize::try_from(remaining.min(chunk.len() as u64)).unwrap_or(chunk.len());
            let read = reader.read(&mut chunk[..want]).map_err(CodecError::Io)?;
            if read == 0 {
                break;
            }
            buffer
                .try_reserve(read)
                .map_err(|_| root_error(ResourceFailure::AllocationFailed, max, read as u64))?;
            buffer.extend_from_slice(&chunk[..read]);
        }
        if buffer.len() as u64 > max {
            return Err(root_error(
                ResourceFailure::BudgetExceeded,
                max,
                buffer.len() as u64,
            ));
        }
        let bytes = arena.alloc(buffer.into_boxed_slice());
        Self::from_bytes(bytes, arena, policy)
    }

    /// Builds a context over caller-owned root bytes, for fuzz targets and
    /// tests. The arena still backs any expansions produced during decode.
    pub fn from_root_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), CodecError> {
        Self::from_bytes(bytes, arena, policy)
    }

    fn from_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
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
            policy: *policy,
            container_only: false,
            input_bytes: length,
            decompressed_bytes: Cell::new(0),
            next_space: Cell::new(1),
            fuse: Cell::new(None),
        };
        Ok((ctx, View::over_space(bytes, SpaceId::ROOT)))
    }

    /// Returns the decode policy in force.
    pub fn policy(&self) -> &DecodePolicy {
        &self.policy
    }

    /// Returns whether the caller requested container-only decoding.
    pub fn container_only(&self) -> bool {
        self.container_only
    }

    /// Records the caller's container-only request before decoding begins.
    pub fn set_container_only(&mut self, value: bool) {
        self.container_only = value;
    }

    fn decompression_allowance(&self) -> u64 {
        let proportional = DECOMPRESSED_TOTAL_BASE
            .saturating_add(DECOMPRESSED_TOTAL_PER_INPUT_BYTE.saturating_mul(self.input_bytes));
        self.policy
            .limits
            .max_decompressed_bytes_total
            .min(proportional)
    }

    fn per_expand_allowance(&self) -> u64 {
        let proportional = DECOMPRESSED_PER_EXPAND_BASE.saturating_add(
            DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE.saturating_mul(self.input_bytes),
        );
        self.policy
            .limits
            .max_decompressed_bytes_per_expand
            .min(proportional)
    }

    fn allocate_space(&self) -> SpaceId {
        let index = self.next_space.get();
        self.next_space.set(index.saturating_add(1));
        SpaceId::from_index(index)
    }

    fn charge_decompressed(
        &self,
        scope: LimitScope,
        amount: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let allowance = self.decompression_allowance();
        let used = self.decompressed_bytes.get();
        if amount > allowance.saturating_sub(used) {
            return Err(self.fuse(
                ResourceFailure::BudgetExceeded,
                scope,
                amount,
                operation,
                location,
            ));
        }
        self.decompressed_bytes.set(used.saturating_add(amount));
        Ok(())
    }

    /// Records a permanent fuse and returns the resource error to propagate.
    fn fuse(
        &self,
        reason: ResourceFailure,
        scope: LimitScope,
        amount: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> CodecError {
        let limit = match scope {
            LimitScope::Global => self.decompression_allowance(),
            LimitScope::PerExpand => self.per_expand_allowance(),
        };
        let used = self.decompressed_bytes.get();
        let resource = ResourceLimit {
            dimension: ResourceDimension::DecompressedBytes,
            reason,
            scope,
            limit,
            used,
            additional: amount,
            context: ErrorContext {
                operation,
                location,
            },
        };
        self.fuse.set(Some(resource));
        CodecError::ResourceLimit(resource)
    }

    // --- decompression ------------------------------------------------------

    /// Begins an expansion whose output is charged incrementally and becomes
    /// available only after successful finalization.
    pub fn begin_expand(
        &self,
        source: View<'_>,
        spec: ExpandSpec,
    ) -> Result<ExpandWriter<'_, 'a>, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let mut buffer: Vec<u8> = Vec::new();
        let reserve = match spec {
            ExpandSpec::Exact(size) => size.min(RESERVE_CLAMP),
            ExpandSpec::Unknown => 0,
        };
        if reserve > 0 {
            let reserve = usize::try_from(reserve).unwrap_or(usize::MAX);
            buffer.try_reserve(reserve).map_err(|_| {
                self.fuse(
                    ResourceFailure::AllocationFailed,
                    LimitScope::PerExpand,
                    reserve as u64,
                    "begin_expand",
                    Some(source.location()),
                )
            })?;
        }
        Ok(ExpandWriter {
            ctx: self,
            spec,
            location: source.location(),
            buffer,
            written: 0,
        })
    }

    /// Copies several input extents into one derived view.
    pub fn concat_views(&self, inputs: &[View<'_>]) -> Result<View<'a>, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let total = inputs.iter().try_fold(0usize, |total, view| {
            total.checked_add(view.window().len()).ok_or_else(|| {
                CodecError::Io(std::io::Error::other("concatenated view is too large"))
            })
        })?;
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(total).map_err(|_| {
            CodecError::Io(std::io::Error::other("concatenated view allocation failed"))
        })?;
        for view in inputs {
            buffer.extend_from_slice(view.window());
        }
        let bytes = self.arena.alloc(buffer.into_boxed_slice());
        let space = self.allocate_space();
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
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let start = usize::try_from(range.start).ok();
        let end = usize::try_from(range.end).ok();
        let child = start
            .zip(end)
            .and_then(|(start, end)| parent.child(start, end))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "stored slice [{}, {}) escapes parent space {}",
                    range.start,
                    range.end,
                    parent.space().index()
                ))
            })?;
        let space = self.allocate_space();
        Ok(View::over_space(child.window(), space))
    }

    // --- lifecycle ----------------------------------------------------------

    /// Closes an inspection, returning a fused resource error even when codec
    /// code swallowed the charge that caused it.
    pub(crate) fn finish_inspection<T>(
        self,
        result: Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        result
    }

    /// Closes a decode, returning a fused resource error even when codec code
    /// swallowed the charge that caused it.
    pub fn finish(
        self,
        result: Result<DecodeResult, CodecError>,
    ) -> Result<DecodeResult, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let result = result?;
        if self.policy.mode == DecodeMode::Strict && !result.report.container_only {
            if let Some(loss) = result
                .report
                .losses
                .iter()
                .find(|loss| loss.code.strict_consequence() == StrictConsequence::Reject)
            {
                return Err(CodecError::Malformed(format!(
                    "strict mode rejects {}: {}",
                    loss.code, loss.message
                )));
            }
        }
        Ok(result)
    }
}

/// Builds the root-input resource error before a context exists.
fn root_error(reason: ResourceFailure, limit: u64, used: u64) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::InputBytes,
        reason,
        scope: LimitScope::Global,
        limit,
        used,
        additional: used.saturating_sub(limit),
        context: ErrorContext {
            operation: "read_root",
            location: None,
        },
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
    location: SourceLocation,
    buffer: Vec<u8>,
    written: u64,
}

impl<'a> ExpandWriter<'_, 'a> {
    /// Appends decompressed output, charging before it is retained.
    pub fn write(&mut self, data: &[u8]) -> Result<(), CodecError> {
        let len = data.len() as u64;
        let new_written = self.written.saturating_add(len);
        match self.spec {
            ExpandSpec::Exact(size) if new_written > size => {
                return Err(CodecError::Malformed(format!(
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
                Some(self.location),
            ));
        }
        self.ctx.charge_decompressed(
            LimitScope::Global,
            len,
            "expand_write",
            Some(self.location),
        )?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
                ResourceFailure::AllocationFailed,
                LimitScope::PerExpand,
                len,
                "expand_write",
                Some(self.location),
            )
        })?;
        self.buffer.extend_from_slice(data);
        self.written = new_written;
        Ok(())
    }

    /// Finalizes the expansion, stores it in the arena, and registers its space.
    pub fn finalize(self) -> Result<View<'a>, CodecError> {
        if let ExpandSpec::Exact(size) = self.spec {
            if self.written != size {
                return Err(CodecError::Malformed(format!(
                    "expansion produced {} of declared exact {size} bytes",
                    self.written
                )));
            }
        }
        let bytes = self.ctx.arena.alloc(self.buffer.into_boxed_slice());
        let space = self.ctx.allocate_space();
        Ok(View::over_space(bytes, space))
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        self.written
    }
}
