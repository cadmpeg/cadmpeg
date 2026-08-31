// SPDX-License-Identifier: Apache-2.0
//! Decode reports, coverage, and source-transfer disposition.

use std::collections::BTreeMap;

use cadmpeg_core::dialect::DialectLayers;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{LossNote, Severity};

/// Transfer status and loss details from a successful decode.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(into = "DecodeReportWire")]
pub struct DecodeReport {
    classification: DecodeClassification,
    /// Whether the decode stopped at the container layer (no entity decode).
    /// The shared codec wrapper stamps this from the decode request.
    pub container_only: bool,
    /// Whether the decoder transferred B-rep geometry into the IR.
    pub geometry_transferred: bool,
    /// Decode coverage counts keyed by measure name.
    pub coverage: BTreeMap<String, usize>,
    /// Explicit loss notes.
    pub losses: Vec<LossNote>,
    /// Free-form informational notes (e.g. container findings).
    pub notes: Vec<String>,
    /// Per-source disposition ledger for decoded records and entities.
    pub transfer_ledger: TransferLedger,
}

#[derive(Debug, Clone, PartialEq)]
enum DecodeClassification {
    Classified(DialectLayers),
    Unclassified { format: String },
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DecodeReportWire {
    format: String,
    container_only: bool,
    geometry_transferred: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    coverage: BTreeMap<String, usize>,
    losses: Vec<LossNote>,
    notes: Vec<String>,
    #[serde(default, skip_serializing_if = "TransferLedger::is_empty")]
    transfer_ledger: TransferLedger,
    #[serde(default)]
    dialects: Option<DialectLayers>,
}

impl From<DecodeReport> for DecodeReportWire {
    fn from(report: DecodeReport) -> Self {
        let DecodeReport {
            classification,
            container_only,
            geometry_transferred,
            coverage,
            losses,
            notes,
            transfer_ledger,
        } = report;
        let (format, dialects) = match classification {
            DecodeClassification::Classified(dialects) => {
                (dialects.primary().format().to_owned(), Some(dialects))
            }
            DecodeClassification::Unclassified { format } => (format, None),
        };
        Self {
            format,
            container_only,
            geometry_transferred,
            coverage,
            losses,
            notes,
            transfer_ledger,
            dialects,
        }
    }
}

impl<'de> Deserialize<'de> for DecodeReport {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = DecodeReportWire::deserialize(deserializer)?;
        let classification = match wire.dialects {
            Some(dialects) => {
                let primary_format = dialects.primary().format();
                if wire.format != primary_format {
                    return Err(serde::de::Error::custom(format_args!(
                        "decode report format {:?} differs from primary dialect format {:?}",
                        wire.format, primary_format
                    )));
                }
                DecodeClassification::Classified(dialects)
            }
            None => DecodeClassification::Unclassified {
                format: wire.format,
            },
        };
        Ok(Self {
            classification,
            container_only: wire.container_only,
            geometry_transferred: wire.geometry_transferred,
            coverage: wire.coverage,
            losses: wire.losses,
            notes: wire.notes,
            transfer_ledger: wire.transfer_ledger,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for DecodeReport {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DecodeReport".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::DecodeReport").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        DecodeReportWire::json_schema(generator)
    }
}

/// Final disposition of one source record or semantic object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TransferDisposition {
    /// Transferred as an exact neutral or native entity.
    Emitted,
    /// Preserved in a native retained-record arena.
    Retained,
    /// Transferred with an explicit approximation.
    Approximated,
    /// Deliberately not transferred.
    Omitted,
}

/// One source object's transfer disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TransferRecord {
    /// Stable source identity or source-local record key.
    pub source: String,
    /// Resulting neutral or native identity, when one was produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Final transfer disposition.
    pub disposition: TransferDisposition,
    /// Concise reason for approximation or omission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Complete source-to-result accounting for a decode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TransferLedger {
    /// Entries in deterministic source traversal order.
    pub entries: Vec<TransferRecord>,
}

impl TransferLedger {
    /// Returns whether the ledger contains no transfer entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records one source disposition.
    pub fn record(
        &mut self,
        source: impl Into<String>,
        target: Option<String>,
        disposition: TransferDisposition,
        note: Option<String>,
    ) {
        self.entries.push(TransferRecord {
            source: source.into(),
            target,
            disposition,
            note,
        });
    }

    /// Verifies every produced target against a finalized model index.
    pub fn verify(&self, index: &crate::index::ModelIndex<'_>) -> Result<(), String> {
        for entry in &self.entries {
            let produces_target = matches!(
                entry.disposition,
                TransferDisposition::Emitted
                    | TransferDisposition::Retained
                    | TransferDisposition::Approximated
            );
            match (&entry.target, produces_target) {
                (Some(target), true) if !index.contains(target) => {
                    return Err(format!(
                        "transfer source {:?} targets unresolved identity {:?}",
                        entry.source, target
                    ));
                }
                (None, true) => {
                    return Err(format!(
                        "transfer source {:?} has {:?} disposition without a target",
                        entry.source, entry.disposition
                    ));
                }
                (Some(_), false) => {
                    return Err(format!(
                        "omitted transfer source {:?} unexpectedly has a target",
                        entry.source
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// A statically declared decode-coverage measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoverageKey(pub &'static str);

impl DecodeReport {
    /// Constructs a classified report whose format is its primary layer's format.
    #[must_use]
    pub fn classified(
        dialects: DialectLayers,
        container_only: bool,
        geometry_transferred: bool,
        coverage: BTreeMap<String, usize>,
        losses: Vec<LossNote>,
        notes: Vec<String>,
        transfer_ledger: TransferLedger,
    ) -> Self {
        Self {
            classification: DecodeClassification::Classified(dialects),
            container_only,
            geometry_transferred,
            coverage,
            losses,
            notes,
            transfer_ledger,
        }
    }

    /// Constructs an unclassified report for a known source format.
    #[must_use]
    pub fn unclassified(
        format: impl Into<String>,
        container_only: bool,
        geometry_transferred: bool,
        coverage: BTreeMap<String, usize>,
        losses: Vec<LossNote>,
        notes: Vec<String>,
        transfer_ledger: TransferLedger,
    ) -> Self {
        Self {
            classification: DecodeClassification::Unclassified {
                format: format.into(),
            },
            container_only,
            geometry_transferred,
            coverage,
            losses,
            notes,
            transfer_ledger,
        }
    }

    /// Returns the source format id.
    #[must_use]
    pub fn format(&self) -> &str {
        match &self.classification {
            DecodeClassification::Classified(dialects) => dialects.primary().format(),
            DecodeClassification::Unclassified { format } => format,
        }
    }

    /// Returns the classified dialect layers, if decoding classified them.
    #[must_use]
    pub fn dialects(&self) -> Option<&DialectLayers> {
        match &self.classification {
            DecodeClassification::Classified(dialects) => Some(dialects),
            DecodeClassification::Unclassified { .. } => None,
        }
    }

    /// Records a coverage measure count for a statically declared key.
    ///
    /// Producers pass the observed count (not an implied +1). Repeated calls
    /// for the same key replace the prior value.
    pub fn record_coverage(&mut self, key: CoverageKey, count: usize) {
        self.coverage.insert(key.0.to_owned(), count);
    }

    /// Returns a coverage measure, treating an unobserved measure as zero.
    pub fn coverage_count(&self, key: CoverageKey) -> usize {
        self.coverage.get(key.0).copied().unwrap_or(0)
    }

    /// Count loss notes at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|l| l.severity >= Severity::Error)
            .count()
    }
}
