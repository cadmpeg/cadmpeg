// SPDX-License-Identifier: Apache-2.0
//! Resource-failure types shared by the budget and [`CodecError`].
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
    /// One expansion.
    PerExpand,
}

/// An offset qualified by its address space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    /// The space the offset indexes.
    pub space: SpaceId,
    /// The absolute offset within that space.
    pub offset: u64,
}

/// Allocation-free context attached to a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorContext {
    /// The operation that failed, as a static label.
    pub operation: &'static str,
    /// Where it failed, when a location is known.
    pub location: Option<SourceLocation>,
}

/// A resource refusal.
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
