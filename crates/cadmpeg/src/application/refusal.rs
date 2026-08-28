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
    /// The check found errors and `--allow-errors` was not set.
    CheckFailed,
    /// Decode reported losses under `--reject-lossy`.
    DecodeLossRejected,
    /// Export planning reported losses under `--reject-lossy`.
    ExportLossRejected,
    /// Geometry export with no transferred geometry.
    EmptyGeometry,
    /// The encoder cannot write the dialect `--to` named.
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
            Self::CheckFailed => "check_failed",
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
    /// The check found errors and `--allow-errors` was not set.
    Check,
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
            Self::Check => "check",
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
    /// The check found errors and `--allow-errors` was not set.
    CheckFailed {
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
    /// The encoder cannot write the dialect `--to` named. The message is the
    /// encoder's own, and carries its catalog.
    UnsupportedTarget {
        /// Human-readable message.
        message: String,
        /// Decode report available for an optional `--report`.
        decode_report: Option<DecodeReport>,
        /// Validation report available for an optional `--report`.
        validation: Option<ValidationReport>,
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
            Self::CheckFailed { .. } => RefusalCode::CheckFailed,
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
            Self::CheckFailed { .. } => RefusalStage::Check,
            Self::ExportLossRejected { .. } | Self::EmptyGeometry { .. } => RefusalStage::Export,
        }
    }

    /// Presentation message shown to the user and written to `refusal.message`.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::DecodeFailed { message }
            | Self::CheckFailed { message, .. }
            | Self::DecodeLossRejected { message, .. }
            | Self::ExportLossRejected { message, .. }
            | Self::EmptyGeometry { message, .. }
            | Self::UnsupportedTarget { message, .. }
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
    /// Refusals that have read a document may write the report. The
    /// binary-stdout refusal happens before the input is read and does not.
    #[must_use]
    pub const fn may_write_report(&self) -> bool {
        match self {
            Self::DecodeFailed { .. }
            | Self::CheckFailed { .. }
            | Self::DecodeLossRejected { .. }
            | Self::ExportLossRejected { .. }
            | Self::EmptyGeometry { .. }
            | Self::UnsupportedTarget { .. } => true,
            Self::BinaryStdoutRejected { .. } => false,
        }
    }

    /// Decode report to include in an optional command report.
    #[must_use]
    pub fn decode_report(&self) -> Option<&DecodeReport> {
        match self {
            Self::DecodeFailed { .. } => None,
            Self::CheckFailed { decode_report, .. } => decode_report.as_ref(),
            Self::DecodeLossRejected { decode_report, .. } => Some(decode_report),
            Self::ExportLossRejected { decode_report, .. }
            | Self::EmptyGeometry { decode_report, .. }
            | Self::UnsupportedTarget { decode_report, .. } => decode_report.as_ref(),
            Self::BinaryStdoutRejected { .. } => None,
        }
    }

    /// Check report to include in an optional command report.
    #[must_use]
    pub fn check_report(&self) -> Option<&ValidationReport> {
        match self {
            Self::DecodeFailed { .. } => None,
            Self::CheckFailed { validation, .. } => Some(validation),
            Self::ExportLossRejected { validation, .. }
            | Self::EmptyGeometry { validation, .. }
            | Self::UnsupportedTarget { validation, .. } => validation.as_ref(),
            Self::DecodeLossRejected { .. } | Self::BinaryStdoutRejected { .. } => None,
        }
    }

    /// Process exit status for this refusal.
    ///
    /// Semantic model refusals exit 1. Decode failure and binary-stdout remain
    /// exit 2 because they are operational failures.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::DecodeFailed { .. } | Self::BinaryStdoutRejected { .. } => 2,
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
            message: "iges cannot write iges:9.9: not a target this encoder can synthesize; available targets: iges:5.3-fixed-ascii".into(),
            decode_report: None,
            validation: None,
        };
        assert_eq!(refusal.code(), RefusalCode::UnsupportedTarget);
        assert_eq!(refusal.stage(), RefusalStage::Plan);
        assert_eq!(refusal.exit_code(), 1);
        assert!(refusal.may_write_report());
        assert_eq!(refusal.to_string(), "iges cannot write iges:9.9: not a target this encoder can synthesize; available targets: iges:5.3-fixed-ascii");
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
        assert_eq!(refusal.exit_code(), 2);
        assert!(refusal.may_write_report());
        assert!(refusal.decode_report().is_none());
        assert!(refusal.check_report().is_none());
        let fields = refusal.report_fields();
        assert_eq!(fields["refusal"]["code"], "decode_failed");
        assert_eq!(fields["refusal"]["stage"], "decode");
    }

    #[test]
    fn check_refusal_maps_to_check_stage() {
        let refusal = ConversionRefusal::CheckFailed {
            message: "check found 1 error(s)".into(),
            decode_report: None,
            validation: ValidationReport {
                entity_counts: BTreeMap::new(),
                findings: Vec::new(),
                losses: Vec::new(),
            },
        };
        assert_eq!(refusal.stage(), RefusalStage::Check);
        assert_eq!(refusal.code().as_str(), "check_failed");
    }
}
