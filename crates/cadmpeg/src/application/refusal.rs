// SPDX-License-Identifier: Apache-2.0
//! Typed conversion refusals.
//!
//! Report envelope `schema_version` 8 serializes [`RefusalCode`] under
//! `refusal.code` with `status: "refused"`. [`ConversionRefusal::evidence`] is
//! the one projection every surface reads.

use std::borrow::Cow;
use std::fmt;
use std::path::Path;

use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::target::TargetRefusal;
use cadmpeg_ir::codec::DecodeFailure;
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_registry::Format;
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

impl ApplicationError {
    /// Classifies a native codec's decode failure at the load site.
    ///
    /// I/O stays operational; a dialect the codec cannot admit and a strict
    /// floor keep their typed evidence; every other codec error is a decode
    /// refusal carrying the codec's message.
    #[must_use]
    pub fn from_decode_failure(
        path: &Path,
        format_id: &'static str,
        failure: DecodeFailure,
    ) -> Self {
        match failure {
            DecodeFailure::Codec(cadmpeg_core::CodecError::Io(error)) => Self::Operational(
                anyhow::Error::new(DecodeFailure::Codec(cadmpeg_core::CodecError::Io(error)))
                    .context(format!("decoding {} as {format_id}", path.display())),
            ),
            DecodeFailure::Codec(cadmpeg_core::CodecError::UnsupportedDialect {
                dialects,
                message,
            }) => ConversionRefusal::unsupported_dialect(dialects, message).into(),
            DecodeFailure::StrictRejected { rejection } => {
                ConversionRefusal::StrictDecodeRejected { rejection }.into()
            }
            failure => ConversionRefusal::DecodeFailed {
                message: format!(
                    "decode failed: decoding {} as {format_id}: {failure}",
                    path.display()
                ),
            }
            .into(),
        }
    }
}

/// Stable refusal code written into command reports and used by tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Stable workflow metadata shared by every refusal carrying this code.
    const fn disposition(self) -> RefusalDisposition {
        let (stage, may_write_report, exit_code) = match self {
            Self::DecodeFailed => (RefusalStage::Decode, true, 2),
            Self::UnsupportedDialect | Self::StrictDecodeRejected | Self::DecodeLossRejected => {
                (RefusalStage::Decode, true, 1)
            }
            Self::CheckFailed => (RefusalStage::Check, true, 1),
            Self::ExportLossRejected => (RefusalStage::Export, true, 1),
            Self::EmptyGeometry | Self::UnsupportedTarget | Self::UnsupportedOutputFormat => {
                (RefusalStage::Plan, true, 1)
            }
            Self::BinaryStdoutRejected => (RefusalStage::Plan, false, 2),
        };
        RefusalDisposition {
            stage,
            may_write_report,
            exit_code,
        }
    }
}

impl fmt::Display for RefusalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
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
        })
    }
}

impl Serialize for RefusalCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
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

impl fmt::Display for RefusalStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Plan => "plan",
            Self::Decode => "decode",
            Self::Check => "check",
            Self::Export => "export",
        })
    }
}

impl Serialize for RefusalStage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// The operation whose validation failed.
#[derive(Debug, Clone, Copy)]
pub enum CheckOperation {
    /// Report validation findings without an export.
    Check,
    /// Refuse an export after validation.
    Export,
}

/// Typed refusal from the conversion workflow.
///
/// Presentation messages, codes, typed detail, and retained reports are all
/// projected once by [`ConversionRefusal::evidence`].
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
        /// Completed decode report bound to the loss that caused refusal.
        rejection: cadmpeg_ir::codec::StrictDecodeRejection,
    },
    /// The check found errors and `--allow-errors` was not set.
    CheckFailed {
        /// Operation rejected by validation.
        operation: CheckOperation,
        /// Decode report available for an optional `--report`.
        decode_report: Option<DecodeReport>,
        /// Validation report available for an optional `--report`.
        validation: ValidationReport,
    },
    /// Decode reported losses under `--reject-lossy`.
    DecodeLossRejected {
        /// Requested output format.
        format: Format,
        /// Decode report available for an optional `--report`.
        decode_report: DecodeReport,
    },
    /// Export planning reported losses under `--reject-lossy`.
    ExportLossRejected {
        /// Decode report available for an optional `--report`.
        decode_report: Option<DecodeReport>,
        /// Validation report available for an optional `--report`.
        validation: ValidationReport,
        /// Export report computed by encoder planning before rejection.
        export_report: ExportReport,
    },
    /// Geometry export refused because decode transferred no geometry.
    EmptyGeometry {
        /// Requested output format.
        format: Format,
        /// Decode report available for an optional `--report`.
        decode_report: Option<DecodeReport>,
        /// Validation report available for an optional `--report`.
        validation: ValidationReport,
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
        validation: ValidationReport,
    },
    /// Binary container would write to stdout without an override.
    BinaryStdoutRejected {
        /// Human-readable message.
        message: String,
    },
}

/// Stable typed contents of the command-report `refusal` object.
#[derive(Debug, Serialize)]
pub(crate) struct RefusalReport<'a> {
    stage: RefusalStage,
    code: RefusalCode,
    message: Cow<'a, str>,
    #[serde(flatten)]
    detail: Option<RefusalDetail<'a>>,
}

/// Everything a surface may show or serialize about one refusal, projected
/// once from the variant.
#[derive(Debug)]
pub struct RefusalEvidence<'a> {
    /// Stable code for the report envelope and tests.
    pub code: RefusalCode,
    /// Presentation message for stderr and `refusal.message`.
    pub message: Cow<'a, str>,
    /// Typed evidence the refusal carries beyond its message.
    pub detail: Option<RefusalDetail<'a>>,
    /// Reports completed before the refusal, for an optional `--report`.
    pub reports: RefusalReports<'a>,
}

/// Typed evidence serialized beside the refusal code.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalDetail<'a> {
    /// Every format layer identified before the codec refused.
    Dialects(&'a DialectLayers),
    /// The encoder's typed target refusal and catalog.
    Target(&'a TargetRefusal),
}

/// Reports a refusal retains from the stages that completed before it.
#[derive(Debug, Default, Clone, Copy)]
pub struct RefusalReports<'a> {
    /// Completed decode report.
    pub decode: Option<&'a DecodeReport>,
    /// Completed validation report.
    pub check: Option<&'a ValidationReport>,
    /// Export report computed by encoder planning before rejection.
    pub export: Option<&'a ExportReport>,
}

/// Variant metadata that does not depend on a refusal's carried reports.
#[derive(Debug, Clone, Copy)]
struct RefusalDisposition {
    stage: RefusalStage,
    may_write_report: bool,
    exit_code: u8,
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

    /// Projects code, message, typed detail, and retained reports at once.
    #[must_use]
    pub fn evidence(&self) -> RefusalEvidence<'_> {
        match self {
            Self::DecodeFailed { message } => RefusalEvidence {
                code: RefusalCode::DecodeFailed,
                message: Cow::Borrowed(message),
                detail: None,
                reports: RefusalReports::default(),
            },
            Self::UnsupportedDialect { dialects, reason } => RefusalEvidence {
                code: RefusalCode::UnsupportedDialect,
                message: Cow::Owned(unsupported_dialect_message(dialects, reason)),
                detail: Some(RefusalDetail::Dialects(dialects)),
                reports: RefusalReports::default(),
            },
            Self::StrictDecodeRejected { rejection } => {
                let loss = rejection.loss();
                RefusalEvidence {
                    code: RefusalCode::StrictDecodeRejected,
                    message: Cow::Owned(format!(
                        "strict mode rejects {}: {}",
                        loss.code, loss.message
                    )),
                    detail: None,
                    reports: RefusalReports {
                        decode: Some(rejection.report()),
                        check: None,
                        export: None,
                    },
                }
            }
            Self::CheckFailed {
                operation,
                decode_report,
                validation,
            } => RefusalEvidence {
                code: RefusalCode::CheckFailed,
                message: Cow::Owned(match operation { CheckOperation::Check => format!("check found {} error(s)", validation.error_count()), CheckOperation::Export => format!("check found {} error(s); refusing to export (use --allow-errors to override)", validation.error_count()) }),
                detail: None,
                reports: RefusalReports {
                    decode: decode_report.as_ref(),
                    check: Some(validation),
                    export: None,
                },
            },
            Self::DecodeLossRejected {
                format,
                decode_report,
            } => RefusalEvidence {
                code: RefusalCode::DecodeLossRejected,
                message: Cow::Owned(format!("decode reported {} loss(es); refusing to write a lossy {} (omit --reject-lossy to allow)", decode_report.losses.len(), format.name())),
                detail: None,
                reports: RefusalReports {
                    decode: Some(decode_report),
                    check: None,
                    export: None,
                },
            },
            Self::ExportLossRejected {
                decode_report,
                validation,
                export_report,
            } => RefusalEvidence {
                code: RefusalCode::ExportLossRejected,
                message: Cow::Owned(format!("export planning reported {} loss(es): {}; refusing to write a lossy {} (omit --reject-lossy to allow)", export_report.losses.len(), export_report.losses.iter().map(|loss| loss.message.as_str()).collect::<Vec<_>>().join("; "), export_report.format())),
                detail: None,
                reports: RefusalReports {
                    decode: decode_report.as_ref(),
                    check: Some(validation),
                    export: Some(export_report),
                },
            },
            Self::EmptyGeometry {
                format,
                decode_report,
                validation,
            } => RefusalEvidence {
                code: RefusalCode::EmptyGeometry,
                message: Cow::Owned(format!("decode transferred no geometry; refusing to write an empty {} (use --allow-empty to override)", format.name())),
                detail: None,
                reports: RefusalReports {
                    decode: decode_report.as_ref(),
                    check: Some(validation),
                    export: None,
                },
            },
            Self::UnsupportedOutputFormat { message } => RefusalEvidence {
                code: RefusalCode::UnsupportedOutputFormat,
                message: Cow::Borrowed(message),
                detail: None,
                reports: RefusalReports::default(),
            },
            Self::UnsupportedTarget {
                refusal,
                decode_report,
                validation,
            } => RefusalEvidence {
                code: RefusalCode::UnsupportedTarget,
                message: Cow::Owned(refusal.to_string()),
                detail: Some(RefusalDetail::Target(refusal)),
                reports: RefusalReports {
                    decode: decode_report.as_ref(),
                    check: Some(validation),
                    export: None,
                },
            },
            Self::BinaryStdoutRejected { message } => RefusalEvidence {
                code: RefusalCode::BinaryStdoutRejected,
                message: Cow::Borrowed(message),
                detail: None,
                reports: RefusalReports::default(),
            },
        }
    }

    /// Stable code for tests and the report envelope.
    #[must_use]
    pub fn code(&self) -> RefusalCode {
        self.evidence().code
    }

    /// Typed `refusal` object for a command report.
    #[must_use]
    pub(crate) fn report(&self) -> RefusalReport<'_> {
        let evidence = self.evidence();
        RefusalReport {
            stage: evidence.code.disposition().stage,
            code: evidence.code,
            message: evidence.message,
            detail: evidence.detail,
        }
    }

    /// Whether an explicitly requested `--report` may still be written.
    ///
    /// Every refusal except the binary-stdout guard may write a report. An
    /// early target refusal writes its typed refusal without decode or check
    /// reports; later refusals serialize every report they hold.
    #[must_use]
    pub fn may_write_report(&self) -> bool {
        self.code().disposition().may_write_report
    }

    /// Process exit status for this refusal.
    ///
    /// Semantic model refusals exit 1. Decode failure and binary-stdout remain
    /// exit 2 because they are operational failures.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        self.code().disposition().exit_code
    }
}

fn unsupported_dialect_message(dialects: &DialectLayers, reason: &str) -> String {
    let primary = dialects.primary();
    let carried = dialects
        .iter()
        .skip(1)
        .map(|layer| layer.dialect().as_str())
        .collect::<Vec<_>>();
    if carried.is_empty() {
        format!(
            "unsupported {} dialect {}: {reason}",
            primary.format(),
            primary.dialect()
        )
    } else {
        format!(
            "unsupported {} dialect {}; carried layers: {}; {reason}",
            primary.format(),
            primary.dialect(),
            carried.join(", ")
        )
    }
}

impl fmt::Display for ConversionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.evidence().message.as_ref())
    }
}

impl std::error::Error for ConversionRefusal {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
    use cadmpeg_core::target::{TargetCatalog, TargetDescriptor};

    use super::*;

    const IGES_TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
        id: DialectId::pinned("iges:5.3-fixed-ascii"),
        aliases: &[],
    }];
    const IGES_CATALOG: TargetCatalog = TargetCatalog::new(IGES_TARGETS, Some(0));

    fn report_value(refusal: &ConversionRefusal) -> serde_json::Value {
        serde_json::to_value(refusal.report()).expect("serialize refusal report")
    }

    fn classify(failure: DecodeFailure) -> ApplicationError {
        ApplicationError::from_decode_failure(&PathBuf::from("part.step"), "step", failure)
    }

    #[test]
    fn refusal_codes_are_stable_for_tests_and_absent_from_display() {
        let refusal = ConversionRefusal::UnsupportedTarget {
            refusal: Box::new(TargetRefusal::unknown_explicit("iges:9.9", IGES_CATALOG)),
            decode_report: None,
            validation: ValidationReport {
                entity_counts: BTreeMap::new(),
                findings: Vec::new(),
                losses: Vec::new(),
            },
        };
        assert_eq!(refusal.code(), RefusalCode::UnsupportedTarget);
        assert_eq!(report_value(&refusal)["stage"], "plan");
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
        assert_eq!(report_value(&refusal)["stage"], "decode");
        assert_eq!(refusal.exit_code(), 1);
        assert_eq!(report_value(&refusal)["code"], "unsupported_dialect");
    }

    #[test]
    fn unsupported_decode_serializes_every_identified_layer() {
        let layers = DialectLayers::of(DialectMatch::refused(DialectId::pinned("sldprt:sw-2024")))
            .with(DialectMatch::residual(DialectId::pinned(
                "parasolid:unknown",
            )));
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
        let classified = classify(DecodeFailure::Codec(
            cadmpeg_core::CodecError::UnsupportedDialect {
                dialects: Box::new(DialectLayers::of(matched.clone())),
                message: "the XML encoding has no decode grammar".into(),
            },
        ));
        let refusal = classified
            .refusal()
            .expect("codec refusal becomes an application refusal");

        let ConversionRefusal::UnsupportedDialect { dialects, .. } = refusal else {
            panic!("unsupported identity must not be flattened to DecodeFailed");
        };
        assert_eq!(dialects.primary(), &matched);
    }

    #[test]
    fn decode_classifier_keeps_io_operational() {
        let classified = classify(DecodeFailure::Codec(cadmpeg_core::CodecError::Io(
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "short read"),
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
        assert_eq!(report_value(&refusal)["stage"], "plan");
        assert_eq!(refusal.exit_code(), 2);
        assert!(!refusal.may_write_report());
    }

    #[test]
    fn decode_failure_is_a_structured_decode_refusal() {
        let refusal = ConversionRefusal::DecodeFailed {
            message: "decode failed: malformed container: test".into(),
        };
        assert_eq!(refusal.code(), RefusalCode::DecodeFailed);
        assert_eq!(report_value(&refusal)["stage"], "decode");
        assert_eq!(refusal.exit_code(), 2);
        assert!(refusal.may_write_report());
        let evidence = refusal.evidence();
        assert!(evidence.reports.decode.is_none());
        assert!(evidence.reports.check.is_none());
        assert!(evidence.detail.is_none());
        let report = report_value(&refusal);
        assert_eq!(report["code"], "decode_failed");
        assert_eq!(report["stage"], "decode");
    }

    #[test]
    fn check_refusal_maps_to_check_stage() {
        let refusal = ConversionRefusal::CheckFailed {
            operation: CheckOperation::Check,
            decode_report: None,
            validation: ValidationReport {
                entity_counts: BTreeMap::new(),
                findings: Vec::new(),
                losses: Vec::new(),
            },
        };
        assert_eq!(report_value(&refusal)["stage"], "check");
        assert_eq!(refusal.code().to_string(), "check_failed");
    }
}
