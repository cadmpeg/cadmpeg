// SPDX-License-Identifier: Apache-2.0
//! Typed conversion refusals.
//!
//! Report envelope `schema_version` 6 serializes [`RefusalCode`] under
//! `refusal.code` with `status: "refused"`. Presentation messages stay on the
//! variant for stderr and `refusal.message`.

use std::fmt;

use cadmpeg_ir::report::{DecodeReport, ValidationReport};
use serde_json::{json, Value};

/// Stable refusal code written into v6 command reports and used by tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalCode {
    /// Native input decoding failed with a classified codec error.
    DecodeFailed,
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

impl RefusalCode {
    /// `snake_case` wire form for `refusal.code`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DecodeFailed => "decode_failed",
            Self::ValidationFailed => "validation_failed",
            Self::DecodeLossRejected => "decode_loss_rejected",
            Self::ExportLossRejected => "export_loss_rejected",
            Self::EmptyGeometry => "empty_geometry",
            Self::UnsupportedTarget => "unsupported_target",
            Self::BinaryStdoutRejected => "binary_stdout_rejected",
        }
    }
}

impl fmt::Display for RefusalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Workflow stage that produced the refusal (`refusal.stage` on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalStage {
    /// Input resolved but conversion planning rejected the request.
    Plan,
    /// Decode completed with losses that the policy rejects.
    Decode,
    /// Neutral or native validation failed.
    Validate,
    /// Export planning refused (loss policy or empty geometry).
    Export,
}

impl RefusalStage {
    /// `snake_case` wire form for `refusal.stage`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Decode => "decode",
            Self::Validate => "validate",
            Self::Export => "export",
        }
    }
}

impl fmt::Display for RefusalStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Typed refusal from the conversion workflow.
///
/// Presentation messages stay on the variant; codes and stages reach the v6
/// report envelope through [`ConversionRefusal::report_fields`].
#[derive(Debug)]
pub enum ConversionRefusal {
    /// Native input decoding failed before a document could be produced.
    DecodeFailed {
        /// Human-readable message.
        message: String,
    },
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
    /// Stable code for tests and the v6 report envelope.
    #[must_use]
    pub const fn code(&self) -> RefusalCode {
        match self {
            Self::DecodeFailed { .. } => RefusalCode::DecodeFailed,
            Self::ValidationFailed { .. } => RefusalCode::ValidationFailed,
            Self::DecodeLossRejected { .. } => RefusalCode::DecodeLossRejected,
            Self::ExportLossRejected { .. } => RefusalCode::ExportLossRejected,
            Self::EmptyGeometry { .. } => RefusalCode::EmptyGeometry,
            Self::UnsupportedTarget { .. } => RefusalCode::UnsupportedTarget,
            Self::BinaryStdoutRejected { .. } => RefusalCode::BinaryStdoutRejected,
        }
    }

    /// Workflow stage for the v6 report envelope.
    #[must_use]
    pub const fn stage(&self) -> RefusalStage {
        match self {
            Self::DecodeFailed { .. } => RefusalStage::Decode,
            Self::UnsupportedTarget { .. } | Self::BinaryStdoutRejected { .. } => {
                RefusalStage::Plan
            }
            Self::DecodeLossRejected { .. } => RefusalStage::Decode,
            Self::ValidationFailed { .. } => RefusalStage::Validate,
            Self::ExportLossRejected { .. } | Self::EmptyGeometry { .. } => RefusalStage::Export,
        }
    }

    /// Presentation message shown to the user and written to `refusal.message`.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::DecodeFailed { message }
            | Self::ValidationFailed { message, .. }
            | Self::DecodeLossRejected { message, .. }
            | Self::ExportLossRejected { message, .. }
            | Self::EmptyGeometry { message, .. }
            | Self::UnsupportedTarget { message }
            | Self::BinaryStdoutRejected { message } => message,
        }
    }

    /// `status` / `refusal` object fields for a v6 command report.
    #[must_use]
    pub fn report_fields(&self) -> Value {
        json!({
            "status": "refused",
            "refusal": {
                "stage": self.stage().as_str(),
                "code": self.code().as_str(),
                "message": self.message(),
            },
        })
    }

    /// Whether an explicitly requested `--report` may still be written.
    ///
    /// Decode, loss, validation, and empty-geometry refusals may write the
    /// report. Binary-stdout and unsupported-target refusals happen before
    /// the input is read and do not write a report.
    #[must_use]
    pub const fn may_write_report(&self) -> bool {
        match self {
            Self::DecodeFailed { .. }
            | Self::ValidationFailed { .. }
            | Self::DecodeLossRejected { .. }
            | Self::ExportLossRejected { .. }
            | Self::EmptyGeometry { .. } => true,
            Self::UnsupportedTarget { .. } | Self::BinaryStdoutRejected { .. } => false,
        }
    }

    /// Decode report to include in an optional command report.
    #[must_use]
    pub fn decode_report(&self) -> Option<&DecodeReport> {
        match self {
            Self::DecodeFailed { .. } => None,
            Self::ValidationFailed { decode_report, .. } => decode_report.as_ref(),
            Self::DecodeLossRejected { decode_report, .. } => Some(decode_report),
            Self::ExportLossRejected { decode_report, .. }
            | Self::EmptyGeometry { decode_report, .. } => decode_report.as_ref(),
            Self::UnsupportedTarget { .. } | Self::BinaryStdoutRejected { .. } => None,
        }
    }

    /// Validation report to include in an optional command report.
    #[must_use]
    pub fn validation_report(&self) -> Option<&ValidationReport> {
        match self {
            Self::DecodeFailed { .. } => None,
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
    #[must_use]
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
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn refusal_codes_are_stable_for_tests_and_absent_from_display() {
        let refusal = ConversionRefusal::UnsupportedTarget {
            message: "--iges-target requires IGES output".into(),
        };
        assert_eq!(refusal.code(), RefusalCode::UnsupportedTarget);
        assert_eq!(refusal.stage(), RefusalStage::Plan);
        assert_eq!(refusal.exit_code(), 1);
        assert!(!refusal.may_write_report());
        assert_eq!(refusal.to_string(), "--iges-target requires IGES output");
        let fields = refusal.report_fields();
        assert_eq!(fields["status"], "refused");
        assert_eq!(fields["refusal"]["code"], "unsupported_target");
        assert_eq!(fields["refusal"]["stage"], "plan");
    }

    #[test]
    fn binary_stdout_keeps_operational_exit_two() {
        let refusal = ConversionRefusal::BinaryStdoutRejected {
            message: "refusing to write binary sldprt to standard output".into(),
        };
        assert_eq!(refusal.code(), RefusalCode::BinaryStdoutRejected);
        assert_eq!(refusal.stage(), RefusalStage::Plan);
        assert_eq!(refusal.exit_code(), 2);
        assert!(!refusal.may_write_report());
    }

    #[test]
    fn decode_failure_is_a_structured_decode_refusal() {
        let refusal = ConversionRefusal::DecodeFailed {
            message: "decode failed: malformed container: test".into(),
        };
        assert_eq!(refusal.code(), RefusalCode::DecodeFailed);
        assert_eq!(refusal.stage(), RefusalStage::Decode);
        assert_eq!(refusal.exit_code(), 1);
        assert!(refusal.may_write_report());
        assert!(refusal.decode_report().is_none());
        assert!(refusal.validation_report().is_none());
        let fields = refusal.report_fields();
        assert_eq!(fields["refusal"]["code"], "decode_failed");
        assert_eq!(fields["refusal"]["stage"], "decode");
    }

    #[test]
    fn validation_refusal_maps_to_validate_stage() {
        let refusal = ConversionRefusal::ValidationFailed {
            message: "validation found 1 error(s)".into(),
            decode_report: None,
            validation: ValidationReport {
                entity_counts: BTreeMap::new(),
                findings: Vec::new(),
                losses: Vec::new(),
            },
        };
        assert_eq!(refusal.stage(), RefusalStage::Validate);
        assert_eq!(refusal.code().as_str(), "validation_failed");
    }
}
