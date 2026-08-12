// SPDX-License-Identifier: Apache-2.0
//! Typed conversion refusals.
//!
//! Refusal codes are stable test data in this phase. They are not serialized
//! into command reports: the report envelope remains `schema_version` 5.

use std::fmt;

use cadmpeg_ir::report::{DecodeReport, ValidationReport};

/// Stable refusal code for tests. Not written into v5 command reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // exercised by unit tests; not referenced from the binary path
pub enum RefusalCode {
    /// Neutral or native validation failed.
    ValidationFailed,
    /// Decode reported losses under `--reject-lossy`.
    DecodeLossRejected,
    /// Export planning reported losses under `--reject-lossy`.
    ExportLossRejected,
    /// Geometry export with no transferred geometry.
    EmptyGeometry,
    /// Format-specific target flag used with the wrong output format.
    UnsupportedTarget,
    /// Binary container would stream to stdout without `--binary-stdout`.
    BinaryStdoutRejected,
}

/// Typed refusal from the conversion workflow.
///
/// Presentation messages stay on the variant; codes are for tests only.
#[derive(Debug)]
pub enum ConversionRefusal {
    /// Validation found errors and `--allow-invalid` was not set.
    ValidationFailed {
        /// Human-readable message.
        message: String,
        /// Decode report available for an optional `--report`.
        decode_report: Option<DecodeReport>,
        /// Validation report available for an optional `--report`.
        validation: ValidationReport,
    },
    /// Decode reported losses under `--reject-lossy`.
    DecodeLossRejected {
        /// Human-readable message.
        message: String,
        /// Decode report available for an optional `--report`.
        decode_report: DecodeReport,
    },
    /// Export planning reported losses under `--reject-lossy`.
    ExportLossRejected {
        /// Human-readable message.
        message: String,
        /// Decode report available for an optional `--report`.
        decode_report: Option<DecodeReport>,
        /// Validation report available for an optional `--report`.
        validation: Option<ValidationReport>,
    },
    /// Geometry export refused because decode transferred no geometry.
    EmptyGeometry {
        /// Human-readable message.
        message: String,
        /// Decode report available for an optional `--report`.
        decode_report: Option<DecodeReport>,
        /// Validation report available for an optional `--report`.
        validation: Option<ValidationReport>,
    },
    /// A target-only flag was supplied for a different output format.
    UnsupportedTarget {
        /// Human-readable message.
        message: String,
    },
    /// Binary container would write to stdout without an override.
    BinaryStdoutRejected {
        /// Human-readable message.
        message: String,
    },
}

impl ConversionRefusal {
    /// Stable code for tests. Not part of the v5 report schema.
    #[allow(dead_code)] // exercised by unit tests; not referenced from the binary path
    pub const fn code(&self) -> RefusalCode {
        match self {
            Self::ValidationFailed { .. } => RefusalCode::ValidationFailed,
            Self::DecodeLossRejected { .. } => RefusalCode::DecodeLossRejected,
            Self::ExportLossRejected { .. } => RefusalCode::ExportLossRejected,
            Self::EmptyGeometry { .. } => RefusalCode::EmptyGeometry,
            Self::UnsupportedTarget { .. } => RefusalCode::UnsupportedTarget,
            Self::BinaryStdoutRejected { .. } => RefusalCode::BinaryStdoutRejected,
        }
    }

    /// Presentation message shown to the user.
    pub fn message(&self) -> &str {
        match self {
            Self::ValidationFailed { message, .. }
            | Self::DecodeLossRejected { message, .. }
            | Self::ExportLossRejected { message, .. }
            | Self::EmptyGeometry { message, .. }
            | Self::UnsupportedTarget { message }
            | Self::BinaryStdoutRejected { message } => message,
        }
    }

    /// Whether an explicitly requested `--report` may still be written.
    ///
    /// Loss, validation, and empty-geometry refusals may write the report.
    /// Binary-stdout and unsupported-target refusals happen before the input
    /// is read and do not write a report.
    pub const fn may_write_report(&self) -> bool {
        match self {
            Self::ValidationFailed { .. }
            | Self::DecodeLossRejected { .. }
            | Self::ExportLossRejected { .. }
            | Self::EmptyGeometry { .. } => true,
            Self::UnsupportedTarget { .. } | Self::BinaryStdoutRejected { .. } => false,
        }
    }

    /// Decode report to include in an optional command report.
    pub fn decode_report(&self) -> Option<&DecodeReport> {
        match self {
            Self::ValidationFailed { decode_report, .. } => decode_report.as_ref(),
            Self::DecodeLossRejected { decode_report, .. } => Some(decode_report),
            Self::ExportLossRejected { decode_report, .. }
            | Self::EmptyGeometry { decode_report, .. } => decode_report.as_ref(),
            Self::UnsupportedTarget { .. } | Self::BinaryStdoutRejected { .. } => None,
        }
    }

    /// Validation report to include in an optional command report.
    pub fn validation_report(&self) -> Option<&ValidationReport> {
        match self {
            Self::ValidationFailed { validation, .. } => Some(validation),
            Self::ExportLossRejected { validation, .. }
            | Self::EmptyGeometry { validation, .. } => validation.as_ref(),
            Self::DecodeLossRejected { .. }
            | Self::UnsupportedTarget { .. }
            | Self::BinaryStdoutRejected { .. } => None,
        }
    }

    /// Process exit status for this refusal.
    ///
    /// Semantic model refusals exit 1. Binary-stdout remains exit 2 so the
    /// operational mix-up guard stays distinct from model refusals.
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::BinaryStdoutRejected { .. } => 2,
            _ => 1,
        }
    }
}

impl fmt::Display for ConversionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ConversionRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusal_codes_are_stable_for_tests_and_absent_from_display() {
        let refusal = ConversionRefusal::UnsupportedTarget {
            message: "--iges-target requires IGES output".into(),
        };
        assert_eq!(refusal.code(), RefusalCode::UnsupportedTarget);
        assert_eq!(refusal.exit_code(), 1);
        assert!(!refusal.may_write_report());
        assert_eq!(refusal.to_string(), "--iges-target requires IGES output");
    }

    #[test]
    fn binary_stdout_keeps_operational_exit_two() {
        let refusal = ConversionRefusal::BinaryStdoutRejected {
            message: "refusing to write binary sldprt to standard output".into(),
        };
        assert_eq!(refusal.code(), RefusalCode::BinaryStdoutRejected);
        assert_eq!(refusal.exit_code(), 2);
        assert!(!refusal.may_write_report());
    }
}
