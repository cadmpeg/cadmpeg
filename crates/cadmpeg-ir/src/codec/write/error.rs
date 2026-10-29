// SPDX-License-Identifier: Apache-2.0
//! Typed encode failures and format-capacity declarations.
//!
//! The encoder interfaces still return `cadmpeg_core::CodecError`. These
//! types define the narrower error contract for their later signature change.

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_core::target::TargetRefusal;

use crate::validate::ValidatedFinding;

/// A capacity imposed by a target format's stored field.
///
/// Each writer declares its rows from verified format widths or value ranges.
/// A writer's internal buffer size or traversal bound is not a capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityField {
    /// Path of the stored field within the writer's record kind.
    pub field: &'static str,
    /// Largest value the stated field can store.
    pub limit: u64,
    /// Format rule that establishes the width or value range.
    pub source: &'static str,
    /// Capacity helper that alone constructs a refusal for this row.
    pub helper: &'static str,
}

/// Failures permitted when writing document content.
///
/// Callers can name all three public types and consume a proof that the
/// validator owns; this does not provide a way to issue one.
///
/// ```
/// use cadmpeg_ir::codec::write::{CapacityField, EncodeError, ValidatedFinding};
///
/// fn reject_non_finite_written_field(proof: ValidatedFinding) -> EncodeError {
///     EncodeError::InvalidInput(proof)
/// }
/// let _: Option<&'static CapacityField> = None;
/// ```
///
/// Decoder failures cannot be widened into this type. A decoder failure while
/// reading fidelity makes that preservation path ineligible for use.
///
/// ```compile_fail
/// use cadmpeg_core::CodecError;
/// use cadmpeg_ir::codec::write::EncodeError;
///
/// let error = CodecError::Malformed("invalid source framing".into());
/// let _: EncodeError = error.into();
/// ```
///
/// Free-text refusals cannot create an input-validation proof.
///
/// ```compile_fail
/// use cadmpeg_ir::codec::write::EncodeError;
///
/// let _ = EncodeError::InvalidInput("unsupported geometry".into());
/// ```
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// Validator-issued evidence of a non-finite field that the writer writes.
    #[error("invalid encoder input: {0}")]
    InvalidInput(ValidatedFinding),
    /// A stored field exceeds a declared format capacity.
    #[error(
        "target capacity exceeded for {}: limit {limit}, value {value}",
        field.field
    )]
    TargetCapacity {
        /// The writer's static declaration of the stored field.
        field: &'static CapacityField,
        /// Capacity that the helper applied.
        limit: u64,
        /// Value that the helper could not store.
        value: u64,
    },
    /// The requested target cannot be resolved from the catalog.
    #[error("{0}")]
    UnsupportedTarget(#[source] Box<TargetRefusal>),
    /// A caller's resource allowance was exhausted.
    #[error(
        "resource limit on {:?}: {:?} (limit {}, used {}, requested {})",
        .0.dimension, .0.reason, .0.limit, .0.used, .0.additional
    )]
    ResourceLimit(ResourceLimit),
    /// An actual destination write or seek failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<ResourceLimit> for EncodeError {
    fn from(limit: ResourceLimit) -> Self {
        Self::ResourceLimit(limit)
    }
}

#[cfg(test)]
mod tests;
