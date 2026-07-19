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
use super::space::{ByteRange, SpaceId, SpaceRegistry};
use super::view::{BoundedCount, View};

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
    spaces: SpaceRegistry,
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
        let spaces = SpaceRegistry::default();
        let root_space = spaces.register_root();
        let ctx = DecodeContext {
            arena,
            policy: *policy,
            container_only: false,
            input_bytes: length,
            decompressed_bytes: Cell::new(0),
            spaces,
            fuse: Cell::new(None),
        };
        Ok((ctx, View::over_space(bytes, root_space)))
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

    /// Allocates a fixed-capacity vector after its count has been proven to fit
    /// in the unread input.
    pub fn exact_vec<T>(&self, count: BoundedCount) -> Result<ExactVec<T>, CodecError> {
        ExactVec::with_capacity(count.get())
    }

    /// Allocates a fixed-capacity vector for a count bounded by format-local
    /// validation rather than a fixed encoded element width.
    pub fn alloc_unfloored<T>(&self, count: usize) -> Result<ExactVec<T>, CodecError> {
        ExactVec::with_capacity(count)
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

    /// Begins an expansion whose output is charged incrementally and stored in
    /// the arena; the derived space is registered only on successful finalize.
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
            ExpandSpec::Exact(size) | ExpandSpec::AtMost(size) => size.min(RESERVE_CLAMP),
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

    /// Begins assembling a derived space from several input extents. Concatenation
    /// copies the extents immediately; transforms stream output through the
    /// decompression limits.
    pub fn begin_derived_space(
        &self,
        inputs: &[View<'_>],
        kind: DerivedKind,
    ) -> Result<DerivedWriter<'_, 'a>, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let location = inputs.first().map(|view| view.location());
        let mut writer = DerivedWriter {
            ctx: self,
            kind,
            location,
            buffer: Vec::new(),
            written: 0,
        };
        if matches!(kind, DerivedKind::Concat) {
            for view in inputs {
                writer.append_extent(view.window())?;
            }
        }
        Ok(writer)
    }

    /// Registers a stored (uncompressed) child range as a space that borrows
    /// the parent bytes without copying, returning the new
    /// space id and a view whose coordinates are absolute within it.
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
    ) -> Result<(SpaceId, View<'v>), CodecError> {
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
        let space = self.spaces.register();
        Ok((space, View::over_space(child.window(), space)))
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

/// A fixed-capacity vector used after validating an untrusted count.
#[derive(Debug)]
pub struct ExactVec<T> {
    vec: Vec<T>,
    capacity: usize,
}

impl<T> ExactVec<T> {
    fn with_capacity(capacity: usize) -> Result<Self, CodecError> {
        let mut vec = Vec::new();
        vec.try_reserve_exact(capacity)
            .map_err(|_| CodecError::Io(std::io::Error::other("allocation failed")))?;
        Ok(Self { vec, capacity })
    }

    /// Appends one value without allowing the vector to grow beyond the
    /// validated count.
    pub fn push(&mut self, value: T) -> Result<(), CodecError> {
        if self.vec.len() == self.capacity {
            return Err(CodecError::Malformed(
                "fixed-capacity vector overflow".to_owned(),
            ));
        }
        self.vec.push(value);
        Ok(())
    }

    /// Returns the number of stored values.
    pub fn len(&self) -> usize {
        self.vec.len()
    }

    /// Returns whether the vector is empty.
    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    /// Returns the validated capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the populated values.
    pub fn finish(self) -> Vec<T> {
        self.vec
    }

    /// Returns the values only when the validated count was filled exactly.
    pub fn finish_exact(self) -> Result<Vec<T>, CodecError> {
        if self.vec.len() == self.capacity {
            Ok(self.vec)
        } else {
            Err(CodecError::Malformed(format!(
                "fixed-capacity vector contains {} of {} values",
                self.vec.len(),
                self.capacity
            )))
        }
    }
}

/// How much output an expansion is expected to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandSpec {
    /// A declared exact size, enforced per-write and at finalize.
    Exact(u64),
    /// A declared upper bound only.
    AtMost(u64),
    /// No trustworthy declared size: the decompression limits apply.
    Unknown,
}

/// How a multi-input derived space assembles its output bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedKind {
    /// The output is the ordered concatenation of the input extents.
    Concat,
    /// The output is decompressed from the named input extents.
    Transform,
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
            ExpandSpec::AtMost(size) if new_written > size => {
                return Err(CodecError::Malformed(format!(
                    "expansion exceeded declared upper bound {size}"
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
    pub fn finalize(self) -> Result<(SpaceId, View<'a>), CodecError> {
        if let ExpandSpec::Exact(size) = self.spec {
            if self.written != size {
                return Err(CodecError::Malformed(format!(
                    "expansion produced {} of declared exact {size} bytes",
                    self.written
                )));
            }
        }
        let bytes = self.ctx.arena.alloc(self.buffer.into_boxed_slice());
        let space = self.ctx.spaces.register();
        Ok((space, View::over_space(bytes, space)))
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        self.written
    }
}

/// Assembles a derived space.
#[derive(Debug)]
pub struct DerivedWriter<'ctx, 'a> {
    ctx: &'ctx DecodeContext<'a>,
    kind: DerivedKind,
    location: Option<SourceLocation>,
    buffer: Vec<u8>,
    written: u64,
}

impl<'a> DerivedWriter<'_, 'a> {
    /// Copies one concatenated extent.
    fn append_extent(&mut self, data: &[u8]) -> Result<(), CodecError> {
        let len = data.len() as u64;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            CodecError::Io(std::io::Error::other("derived-space allocation failed"))
        })?;
        self.buffer.extend_from_slice(data);
        self.written = self.written.saturating_add(len);
        Ok(())
    }

    /// Appends transform output under the decompression limits.
    pub fn write(&mut self, data: &[u8]) -> Result<(), CodecError> {
        if !matches!(self.kind, DerivedKind::Transform) {
            return Err(CodecError::Malformed(
                "a derived Concat space assembles from its declared inputs; \
                 write is only for Transform output"
                    .to_owned(),
            ));
        }
        let len = data.len() as u64;
        let new_written = self.written.saturating_add(len);
        let per_expand = self.ctx.per_expand_allowance();
        if new_written > per_expand {
            return Err(self.ctx.fuse(
                ResourceFailure::BudgetExceeded,
                LimitScope::PerExpand,
                len,
                "derived_write",
                self.location,
            ));
        }
        self.ctx
            .charge_decompressed(LimitScope::Global, len, "derived_write", self.location)?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
                ResourceFailure::AllocationFailed,
                LimitScope::PerExpand,
                len,
                "derived_write",
                self.location,
            )
        })?;
        self.buffer.extend_from_slice(data);
        self.written = new_written;
        Ok(())
    }

    /// Stores and registers the derived space.
    pub fn finalize(self) -> Result<(SpaceId, View<'a>), CodecError> {
        let bytes = self.ctx.arena.alloc(self.buffer.into_boxed_slice());
        let space = self.ctx.spaces.register();
        Ok((space, View::over_space(bytes, space)))
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        self.written
    }
}
