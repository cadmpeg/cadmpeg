// SPDX-License-Identifier: Apache-2.0
//! Typed conversion refusals.
//!
//! Report envelope `schema_version` 7 serializes [`RefusalCode`] under
//! `refusal.code` with `status: "refused"`. Presentation messages stay on the
//! variant for stderr and `refusal.message`.

use std::borrow::Cow;
use std::fmt;

use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::target::TargetRefusal;
use cadmpeg_ir::codec::DecodeFailure;
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use serde::Serialize;

/// A conversion workflow either refuses a modeled request or fails
/// operationally.
#[derive(Debug)]
pub enum ApplicationError {
    /// A modeled conversion policy or capability refusal.
    Refusal(Box<ConversionRefusal>),
    /// Filesystem, I/O, malformed implementation, or artifact failure.
    Operational(anyhow::Error),
}

impl ApplicationError {
    /// Returns the typed refusal when this is a modeled verdict.
    #[must_use]
    pub fn refusal(&self) -> Option<&ConversionRefusal> {
        match self {
            Self::Refusal(refusal) => Some(refusal.as_ref()),
            Self::Operational(_) => None,
        }
    }

    /// Process exit status for this application result.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        self.refusal().map_or(2, ConversionRefusal::exit_code)
    }
}

impl From<ConversionRefusal> for ApplicationError {
    fn from(refusal: ConversionRefusal) -> Self {
        Self::Refusal(Box::new(refusal))
    }
}

impl From<anyhow::Error> for ApplicationError {
    fn from(error: anyhow::Error) -> Self {
        Self::Operational(error)
    }
}

impl From<std::io::Error> for ApplicationError {
    fn from(error: std::io::Error) -> Self {
        Self::Operational(error.into())
    }
}

impl From<serde_json::Error> for ApplicationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Operational(error.into())
    }
}

impl From<cadmpeg_core::CodecError> for ApplicationError {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        Self::Operational(error.into())
    }
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refusal(refusal) => fmt::Display::fmt(refusal.as_ref(), f),
            Self::Operational(error) if f.alternate() => write!(f, "{error:#}"),
            Self::Operational(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl std::error::Error for ApplicationError {}

/// Classifies a typed loader result without inspecting an erased error chain.
pub(crate) fn classify_load_error(error: crate::loader::LoadError) -> ApplicationError {
    let (path, format_id, failure) = match error {
        crate::loader::LoadError::Decode {
            path,
            format_id,
            failure,
        } => (path, format_id, failure),
        crate::loader::LoadError::Operational(error) => {
            return ApplicationError::Operational(error)
        }
    };

    match *failure {
        DecodeFailure::Codec(cadmpeg_core::CodecError::Io(error)) => ApplicationError::Operational(
            anyhow::Error::new(DecodeFailure::Codec(cadmpeg_core::CodecError::Io(error)))
                .context(format!("decoding {} as {format_id}", path.display())),
        ),
        DecodeFailure::Codec(cadmpeg_core::CodecError::UnsupportedDialect {
            dialects,
            message,
        }) => ConversionRefusal::unsupported_dialect(dialects, message).into(),
        DecodeFailure::StrictRejected {
            loss_code,
            message,
            report,
        } => ConversionRefusal::StrictDecodeRejected {
            loss_code,
            loss_message: message,
            decode_report: *report,
        }
        .into(),
        failure => ConversionRefusal::DecodeFailed {
            message: format!(
                "decode failed: decoding {} as {format_id}: {failure}",
                path.display()
            ),
        }
        .into(),
    }
}

/// Stable refusal code written into v7 command reports and used by tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    /// Native input decoding failed with a classified codec error.
    DecodeFailed,
    /// The input was identified but its dialect has no decode grammar.
    UnsupportedDialect,
    /// Strict decode policy rejected a reported loss.
    StrictDecodeRejected,
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
    /// The selected format has no encoder in this build.
    UnsupportedOutputFormat,
    /// Binary container would stream to stdout without `--binary-stdout`.
    BinaryStdoutRejected,
}

impl RefusalCode {
    /// `snake_case` wire form for `refusal.code`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DecodeFailed => "decode_failed",
            Self::UnsupportedDialect => "unsupported_dialect",
            Self::StrictDecodeRejected => "strict_decode_rejected",
            Self::CheckFailed => "check_failed",
            Self::DecodeLossRejected => "decode_loss_rejected",
            Self::ExportLossRejected => "export_loss_rejected",
            Self::EmptyGeometry => "empty_geometry",
            Self::UnsupportedTarget => "unsupported_target",
            Self::UnsupportedOutputFormat => "unsupported_output_format",
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
/// Presentation messages stay on the variant; codes and stages reach the v7
/// report envelope through [`ConversionRefusal::report`].
#[derive(Debug)]
pub enum ConversionRefusal {
    /// Native input decoding failed before a document could be produced.
    DecodeFailed {
        /// Human-readable message.
        message: String,
    },
    /// Input identity was recovered before the codec refused its dialect.
    UnsupportedDialect {
        /// Every format layer identified before refusal.
        dialects: Box<DialectLayers>,
        /// Codec-owned reason no decode grammar can admit it.
        reason: String,
    },
    /// Decode completed, but strict mode rejected a loss in its report.
    StrictDecodeRejected {
        /// Stable loss code that triggered the strict floor.
        loss_code: String,
        /// The refusing loss's message.
        loss_message: String,
        /// Completed decode report containing all recovered evidence.
        decode_report: DecodeReport,
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
        /// Export report computed by encoder planning before rejection.
        export_report: ExportReport,
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
    /// The command-line target cannot select a writable output format.
    UnsupportedOutputFormat {
        /// Human-readable selection failure.
        message: String,
    },
    /// The encoder could not resolve or deliver the requested target.
    UnsupportedTarget {
        /// Typed request state and the encoder's structured catalog.
        refusal: Box<TargetRefusal>,
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

/// Stable typed contents of the v7 `refusal` object.
#[derive(Debug, Serialize)]
pub(crate) struct RefusalReport<'a> {
    stage: RefusalStage,
    code: RefusalCode,
    message: Cow<'a, str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dialects: Option<&'a DialectLayers>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'a TargetRefusal>,
}

impl ConversionRefusal {
    /// Constructs a decode refusal without duplicating ownership and wording at
    /// each surface that can inspect or load a native container.
    #[must_use]
    pub fn unsupported_dialect(dialects: Box<DialectLayers>, reason: impl Into<String>) -> Self {
        Self::UnsupportedDialect {
            dialects,
            reason: reason.into(),
        }
    }

    /// Stable code for tests and the v7 report envelope.
    #[must_use]
    pub const fn code(&self) -> RefusalCode {
        match self {
            Self::DecodeFailed { .. } => RefusalCode::DecodeFailed,
            Self::UnsupportedDialect { .. } => RefusalCode::UnsupportedDialect,
            Self::StrictDecodeRejected { .. } => RefusalCode::StrictDecodeRejected,
            Self::CheckFailed { .. } => RefusalCode::CheckFailed,
            Self::DecodeLossRejected { .. } => RefusalCode::DecodeLossRejected,
            Self::ExportLossRejected { .. } => RefusalCode::ExportLossRejected,
            Self::EmptyGeometry { .. } => RefusalCode::EmptyGeometry,
            Self::UnsupportedOutputFormat { .. } => RefusalCode::UnsupportedOutputFormat,
            Self::UnsupportedTarget { .. } => RefusalCode::UnsupportedTarget,
            Self::BinaryStdoutRejected { .. } => RefusalCode::BinaryStdoutRejected,
        }
    }

    /// Workflow stage for the v7 report envelope.
    #[must_use]
    pub const fn stage(&self) -> RefusalStage {
        match self {
            Self::DecodeFailed { .. }
            | Self::UnsupportedDialect { .. }
            | Self::StrictDecodeRejected { .. } => RefusalStage::Decode,
            Self::UnsupportedOutputFormat { .. }
            | Self::UnsupportedTarget { .. }
            | Self::BinaryStdoutRejected { .. } => RefusalStage::Plan,
            Self::DecodeLossRejected { .. } => RefusalStage::Decode,
            Self::CheckFailed { .. } => RefusalStage::Check,
            Self::EmptyGeometry { .. } => RefusalStage::Plan,
            Self::ExportLossRejected { .. } => RefusalStage::Export,
        }
    }

    /// Presentation message shown to the user and written to `refusal.message`.
    #[must_use]
    pub fn message(&self) -> Cow<'_, str> {
        match self {
            Self::DecodeFailed { message }
            | Self::CheckFailed { message, .. }
            | Self::DecodeLossRejected { message, .. }
            | Self::ExportLossRejected { message, .. }
            | Self::EmptyGeometry { message, .. }
            | Self::UnsupportedOutputFormat { message }
            | Self::BinaryStdoutRejected { message } => Cow::Borrowed(message),
            Self::StrictDecodeRejected {
                loss_code,
                loss_message,
                ..
            } => Cow::Owned(format!("strict mode rejects {loss_code}: {loss_message}")),
            Self::UnsupportedDialect { dialects, reason } => {
                let primary = dialects.primary();
                let carried = dialects
                    .iter()
                    .skip(1)
                    .map(|layer| layer.dialect().as_str())
                    .collect::<Vec<_>>();
                if carried.is_empty() {
                    Cow::Owned(format!(
                        "unsupported {} dialect {}: {reason}",
                        primary.format(),
                        primary.dialect()
                    ))
                } else {
                    Cow::Owned(format!(
                        "unsupported {} dialect {}; carried layers: {}; {reason}",
                        primary.format(),
                        primary.dialect(),
                        carried.join(", ")
                    ))
                }
            }
            Self::UnsupportedTarget { refusal, .. } => Cow::Owned(refusal.to_string()),
        }
    }

    /// Typed `refusal` object for a v7 command report.
    #[must_use]
    pub(crate) fn report(&self) -> RefusalReport<'_> {
        RefusalReport {
            stage: self.stage(),
            code: self.code(),
            message: self.message(),
            dialects: match self {
                Self::UnsupportedDialect { dialects, .. } => Some(dialects),
                _ => None,
            },
            target: match self {
                Self::UnsupportedTarget { refusal, .. } => Some(refusal),
                _ => None,
            },
        }
    }

    /// Whether an explicitly requested `--report` may still be written.
    ///
    /// Every refusal except the binary-stdout guard may write a report. An
    /// early target refusal writes its typed refusal without decode or check
    /// reports; later refusals serialize every report they hold.
    #[must_use]
    pub const fn may_write_report(&self) -> bool {
        match self {
            Self::DecodeFailed { .. }
            | Self::UnsupportedDialect { .. }
            | Self::StrictDecodeRejected { .. }
            | Self::CheckFailed { .. }
            | Self::DecodeLossRejected { .. }
            | Self::ExportLossRejected { .. }
            | Self::EmptyGeometry { .. }
            | Self::UnsupportedOutputFormat { .. }
            | Self::UnsupportedTarget { .. } => true,
            Self::BinaryStdoutRejected { .. } => false,
        }
    }

    /// Decode report to include in an optional command report.
    #[must_use]
    pub fn decode_report(&self) -> Option<&DecodeReport> {
        match self {
            Self::DecodeFailed { .. }
            | Self::UnsupportedDialect { .. }
            | Self::UnsupportedOutputFormat { .. } => None,
            Self::StrictDecodeRejected { decode_report, .. } => Some(decode_report),
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
            Self::DecodeFailed { .. }
            | Self::UnsupportedDialect { .. }
            | Self::StrictDecodeRejected { .. }
            | Self::UnsupportedOutputFormat { .. } => None,
            Self::CheckFailed { validation, .. } => Some(validation),
            Self::ExportLossRejected { validation, .. }
            | Self::EmptyGeometry { validation, .. }
            | Self::UnsupportedTarget { validation, .. } => validation.as_ref(),
            Self::DecodeLossRejected { .. } | Self::BinaryStdoutRejected { .. } => None,
        }
    }

    /// Export report to include in an optional command report.
    #[must_use]
    pub fn export_report(&self) -> Option<&ExportReport> {
        match self {
            Self::ExportLossRejected { export_report, .. } => Some(export_report),
            _ => None,
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
        f.write_str(self.message().as_ref())
    }
}

impl std::error::Error for ConversionRefusal {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
    use cadmpeg_core::target::TargetDescriptor;
    use cadmpeg_ir::report::TransferLedger;

    use super::*;

    const IGES_TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
        id: DialectId::pinned("iges:5.3-fixed-ascii"),
        aliases: &[],
        default: true,
    }];

    fn report_value(refusal: &ConversionRefusal) -> serde_json::Value {
        serde_json::to_value(refusal.report()).expect("serialize refusal report")
    }

    fn decode_load_error(failure: DecodeFailure) -> crate::loader::LoadError {
        crate::loader::LoadError::Decode {
            path: PathBuf::from("part.step"),
            format_id: "step",
            failure: Box::new(failure),
        }
    }

    #[test]
    fn refusal_codes_are_stable_for_tests_and_absent_from_display() {
        let refusal = ConversionRefusal::UnsupportedTarget {
            refusal: Box::new(TargetRefusal::unknown_explicit(
                "iges",
                "iges:9.9",
                IGES_TARGETS,
            )),
            decode_report: None,
            validation: None,
        };
        assert_eq!(refusal.code(), RefusalCode::UnsupportedTarget);
        assert_eq!(refusal.stage(), RefusalStage::Plan);
        assert_eq!(refusal.exit_code(), 1);
        assert!(refusal.may_write_report());
        assert_eq!(refusal.to_string(), "iges cannot write iges:9.9: not a target this encoder can synthesize; available targets: iges:5.3-fixed-ascii");
        let report = report_value(&refusal);
        assert_eq!(report["code"], "unsupported_target");
        assert_eq!(report["stage"], "plan");
        assert_eq!(report["target"]["kind"], "unknown_explicit");
        assert_eq!(report["target"]["format"], "iges");
        assert_eq!(report["target"]["requested"], "iges:9.9");
        assert_eq!(
            report["target"]["available"][0]["id"],
            "iges:5.3-fixed-ascii"
        );
    }

    #[test]
    fn unsupported_decode_keeps_the_identification() {
        let matched = DialectMatch::refused(DialectId::pinned("step:part-28-xml"));
        let refusal = ConversionRefusal::UnsupportedDialect {
            dialects: Box::new(DialectLayers::of(matched.clone())),
            reason: "the XML encoding has no decode grammar".into(),
        };

        let ConversionRefusal::UnsupportedDialect { dialects, .. } = &refusal else {
            panic!("the variant just constructed is preserved");
        };
        assert_eq!(dialects.primary(), &matched);
        assert_eq!(refusal.code(), RefusalCode::UnsupportedDialect);
        assert_eq!(refusal.stage(), RefusalStage::Decode);
        assert_eq!(refusal.exit_code(), 1);
        assert_eq!(report_value(&refusal)["code"], "unsupported_dialect");
    }

    #[test]
    fn unsupported_decode_serializes_every_identified_layer() {
        let layers = DialectLayers::new(
            DialectMatch::refused(DialectId::pinned("sldprt:sw-2024")),
            vec![DialectMatch::residual(DialectId::pinned(
                "parasolid:unknown",
            ))],
        )
        .expect("host and kernel layers are distinct");
        let refusal = ConversionRefusal::UnsupportedDialect {
            dialects: Box::new(layers),
            reason: "no decoder admits the host dialect".into(),
        };

        let report = report_value(&refusal);
        assert_eq!(report["dialects"]["primary"]["dialect"], "sldprt:sw-2024");
        assert_eq!(
            report["dialects"]["extra"][0]["dialect"],
            "parasolid:unknown"
        );
        assert!(refusal.to_string().contains("parasolid:unknown"));
    }

    #[test]
    fn unwritable_format_has_its_own_wire_code() {
        let refusal = ConversionRefusal::UnsupportedOutputFormat {
            message: "catia is not writable".into(),
        };

        assert_eq!(refusal.code(), RefusalCode::UnsupportedOutputFormat);
        assert_eq!(report_value(&refusal)["code"], "unsupported_output_format");
    }

    #[test]
    fn decode_classifier_preserves_an_unsupported_dialect_variant() {
        let matched = DialectMatch::refused(DialectId::pinned("step:part-28-xml"));
        let classified = classify_load_error(decode_load_error(DecodeFailure::Codec(
            cadmpeg_core::CodecError::UnsupportedDialect {
                dialects: Box::new(DialectLayers::of(matched.clone())),
                message: "the XML encoding has no decode grammar".into(),
            },
        )));
        let refusal = classified
            .refusal()
            .expect("codec refusal becomes an application refusal");

        let ConversionRefusal::UnsupportedDialect { dialects, .. } = refusal else {
            panic!("unsupported identity must not be flattened to DecodeFailed");
        };
        assert_eq!(dialects.primary(), &matched);
    }

    #[test]
    fn decode_classifier_preserves_a_strict_refusal_and_its_completed_report() {
        let report = DecodeReport::unclassified(
            "test",
            cadmpeg_ir::DecodeTransfer::full(false),
            BTreeMap::new(),
            Vec::new(),
            vec!["decode completed".into()],
            TransferLedger::default(),
        );
        let classified = classify_load_error(decode_load_error(DecodeFailure::StrictRejected {
            loss_code: "test/source.dialect-unverified".into(),
            message: "the dialect was recovered provisionally".into(),
            report: Box::new(report),
        }));
        let refusal = classified
            .refusal()
            .expect("strict policy failure becomes an application refusal");

        assert_eq!(refusal.code(), RefusalCode::StrictDecodeRejected);
        assert_eq!(refusal.stage(), RefusalStage::Decode);
        assert_eq!(refusal.exit_code(), 1);
        assert_eq!(report_value(refusal)["code"], "strict_decode_rejected");
        assert_eq!(
            refusal.decode_report().expect("completed report").notes,
            ["decode completed"]
        );
    }

    #[test]
    fn decode_classifier_keeps_io_operational() {
        let classified = classify_load_error(decode_load_error(DecodeFailure::Codec(
            cadmpeg_core::CodecError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "short read",
            )),
        )));

        assert!(classified.refusal().is_none());
        assert_eq!(classified.exit_code(), 2);
        assert_eq!(classified.to_string(), "decoding part.step as step");
        assert!(format!("{classified:#}").contains("short read"));
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
        let report = report_value(&refusal);
        assert_eq!(report["code"], "decode_failed");
        assert_eq!(report["stage"], "decode");
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
