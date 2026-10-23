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
    pub(crate) const fn for_dimension(dimension: ResourceDimension) -> Self {
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
    /// Whether policy or allocation refused.
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

impl std::fmt::Display for ResourceLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let boundary = match (self.reason, self.scope, self.dimension) {
            (ResourceFailure::AllocationFailed, _, _) => "allocation failure",
            (ResourceFailure::BudgetExceeded, LimitScope::Session, _) => "session allowance",
            (
                ResourceFailure::BudgetExceeded,
                LimitScope::Part,
                ResourceDimension::DecompressedBytes,
            ) => "per-expansion allowance",
            (ResourceFailure::BudgetExceeded, LimitScope::Part, _) => "part allowance",
        };
        write!(f,
            "resource limit on {:?}: {:?} (scope {:?}, limit {}, used {}, requested {}, operation {}): {}",
            self.dimension, self.reason, self.scope, self.limit, self.used,
            self.additional, self.operation, boundary,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(
        scope: LimitScope,
        dimension: ResourceDimension,
        reason: ResourceFailure,
    ) -> ResourceLimit {
        ResourceLimit {
            scope,
            dimension,
            reason,
            limit: 4096,
            used: 2048,
            additional: 8192,
            operation: "bounded stream expansion",
        }
    }

    #[test]
    fn resource_diagnostic_keeps_operation_scope_and_numeric_evidence() {
        let failure = limit(
            LimitScope::Part,
            ResourceDimension::DecompressedBytes,
            ResourceFailure::BudgetExceeded,
        );
        let message = crate::CodecError::ResourceLimit(failure).to_string();
        for evidence in [
            "Part",
            "DecompressedBytes",
            "BudgetExceeded",
            "limit 4096",
            "used 2048",
            "requested 8192",
            "bounded stream expansion",
        ] {
            assert!(message.contains(evidence), "{message}");
        }
        assert!(message.ends_with("per-expansion allowance"));
        assert_eq!(message, failure.to_string());
    }

    #[test]
    fn session_and_other_part_allowances_remain_distinct() {
        assert!(limit(
            LimitScope::Session,
            ResourceDimension::DecompressedBytes,
            ResourceFailure::BudgetExceeded
        )
        .to_string()
        .ends_with("session allowance"));
        assert!(limit(
            LimitScope::Part,
            ResourceDimension::RecursionDepth,
            ResourceFailure::BudgetExceeded
        )
        .to_string()
        .ends_with("part allowance"));
    }

    #[test]
    fn allocator_refusal_does_not_claim_policy_exhaustion() {
        let message = limit(
            LimitScope::Session,
            ResourceDimension::RetainedBytes,
            ResourceFailure::AllocationFailed,
        )
        .to_string();
        assert!(message.contains("AllocationFailed"));
        assert!(message.ends_with("allocation failure"));
        assert!(!message.contains("allowance"));
    }
}
