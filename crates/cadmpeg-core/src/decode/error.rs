// SPDX-License-Identifier: Apache-2.0
//! Resource-failure types shared by the budget and [`CodecError`].
//!
//! [`CodecError`]: crate::CodecError

use super::space::SpaceId;

/// Which resource a limit governs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceDimension {
    /// Physical input bytes read at the root.
    InputBytes,
    /// Bytes produced by decompression.
    DecompressedBytes,
    /// Bytes retained for the decode session outside expansion output.
    RetainedBytes,
    /// Admitted semantic or native entities.
    Entities,
    /// Active recursive nesting depth.
    RecursionDepth,
}

/// Why a resource request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceFailure {
    /// Policy refused: the request would exceed the allowance.
    BudgetExceeded,
    /// The allocator refused, surfaced via `try_reserve`.
    AllocationFailed,
}

/// An offset qualified by its address space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    /// The space the offset indexes.
    pub space: SpaceId,
    /// The absolute offset within that space.
    pub offset: u64,
}

/// The boundary at which a resource refusal applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitScope {
    /// The shared decode session cannot continue.
    Session,
    /// The current independently framed part cannot continue.
    Part,
}

impl LimitScope {
    pub(super) const fn for_dimension(dimension: ResourceDimension) -> Self {
        match dimension {
            ResourceDimension::RecursionDepth => Self::Part,
            _ => Self::Session,
        }
    }
}

/// A resource refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimit {
    /// Whether the refusal stops the session or its current part.
    pub scope: LimitScope,
    /// The dimension that refused the request.
    pub dimension: ResourceDimension,
    /// Whether policy or the allocator refused.
    pub reason: ResourceFailure,
    /// The allowance in force.
    pub limit: u64,
    /// The amount already charged before this request.
    pub used: u64,
    /// The saturating size of the request that failed.
    pub additional: u64,
    /// The operation that failed, as a static label.
    pub operation: &'static str,
}

impl ResourceLimit {
    /// Reports an allocator refusal before any resource is charged.
    #[must_use]
    pub const fn allocation_failed(
        dimension: ResourceDimension,
        limit: u64,
        additional: u64,
        operation: &'static str,
    ) -> Self {
        Self {
            scope: LimitScope::for_dimension(dimension),
            dimension,
            reason: ResourceFailure::AllocationFailed,
            limit,
            used: 0,
            additional,
            operation,
        }
    }
}
