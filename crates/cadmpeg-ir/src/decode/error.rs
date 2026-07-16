// SPDX-License-Identifier: Apache-2.0
//! Resource-failure vocabulary shared by the budget and [`CodecError`].
//!
//! A resource refusal states a fact about policy (the decode may not commit
//! this much of a resource) or the allocator (the request was refused).
//! Neither is ever reported as `Malformed`: malformed is a statement about
//! the input. These types are constructed on the fused path, so they never
//! allocate; [`ErrorContext::operation`] is a `&'static str`.
//!
//! [`CodecError`]: crate::codec::CodecError

use super::space::SpaceId;

/// Which resource a limit governs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceDimension {
    /// Physical input bytes read at the root.
    InputBytes,
    /// Bytes produced by decompression.
    DecompressedBytes,
    /// Cumulative committed heap bytes.
    AllocBytes,
    /// Abstract work units charged at commit boundaries and probe scans.
    Work,
    /// Recursion depth.
    Depth,
    /// Bytes retained opaque in salvage mode.
    RetainedBytes,
}

/// Why a resource request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceFailure {
    /// Policy refused: the request would exceed the allowance.
    BudgetExceeded,
    /// The allocator refused, surfaced via `try_reserve`.
    AllocationFailed,
}

/// The extent a limit applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitScope {
    /// The whole decode.
    Global,
    /// One address space.
    PerSpace,
    /// One expansion.
    PerExpand,
}

/// An offset qualified by its space.
///
/// An offset alone is ambiguous once a decode holds multiple address spaces,
/// so every location names the space it indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    /// The space the offset indexes.
    pub space: SpaceId,
    /// The absolute offset within that space.
    pub offset: u64,
}

/// Static context attached to a failure.
///
/// Built while the allocator may be refusing requests, so it never allocates:
/// `operation` is a static label and `location` is a copied value. Richer
/// text may be attached outside the fused path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorContext {
    /// The operation that failed, as a static label.
    pub operation: &'static str,
    /// Where it failed, when a location is known.
    pub location: Option<SourceLocation>,
}

/// A resource refusal, whole and self-describing.
///
/// `limit` is the allowance in force, `used` is the amount already charged,
/// and `additional` is the saturating size of the request that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimit {
    /// The dimension that refused the request.
    pub dimension: ResourceDimension,
    /// Whether policy or the allocator refused.
    pub reason: ResourceFailure,
    /// The extent the limit applies to.
    pub scope: LimitScope,
    /// The allowance in force.
    pub limit: u64,
    /// The amount already charged before this request.
    pub used: u64,
    /// The saturating size of the request that failed.
    pub additional: u64,
    /// Static context for the failure.
    pub context: ErrorContext,
}
