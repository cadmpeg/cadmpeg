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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DecodeReport {
    /// Source format id.
    format: String,
    /// Whether the decode stopped at the container layer (no entity decode).
    /// The shared codec wrapper stamps this from the decode request.
    pub container_only: bool,
    /// Whether the decoder transferred B-rep geometry into the IR.
    pub geometry_transferred: bool,
    /// Decode coverage counts keyed by measure name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub coverage: BTreeMap<String, usize>,
    /// Explicit loss notes.
    pub losses: Vec<LossNote>,
    /// Free-form informational notes (e.g. container findings).
    pub notes: Vec<String>,
    /// Per-source disposition ledger for decoded records and entities.
    #[serde(default, skip_serializing_if = "TransferLedger::is_empty")]
    pub transfer_ledger: TransferLedger,
    /// Dialect identification, one entry per format layer the decode read.
    ///
    /// When classified, the primary layer is mirrored into
    /// [`crate::document::SourceMeta::dialect`]. [`crate::codec::DecodeResult::new`]
    /// performs that projection.
    ///
    /// Always serialized. Reports written before the field existed omit the key
    /// and read back as unclassified.
    #[serde(default)]
    dialects: Option<DialectLayers>,
}

#[derive(Deserialize)]
struct DecodeReportWire {
    format: String,
    container_only: bool,
    geometry_transferred: bool,
    #[serde(default)]
    coverage: BTreeMap<String, usize>,
    losses: Vec<LossNote>,
    notes: Vec<String>,
    #[serde(default)]
    transfer_ledger: TransferLedger,
    #[serde(default)]
    dialects: Option<DialectLayers>,
}

impl<'de> Deserialize<'de> for DecodeReport {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = DecodeReportWire::deserialize(deserializer)?;
        if let Some(dialects) = &wire.dialects {
            let primary_format = dialects.primary().format();
            if wire.format != primary_format {
                return Err(serde::de::Error::custom(format_args!(
                    "decode report format {:?} differs from primary dialect format {:?}",
                    wire.format, primary_format
                )));
            }
        }
        Ok(Self {
            format: wire.format,
            container_only: wire.container_only,
            geometry_transferred: wire.geometry_transferred,
            coverage: wire.coverage,
            losses: wire.losses,
            notes: wire.notes,
            transfer_ledger: wire.transfer_ledger,
            dialects: wire.dialects,
        })
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
            format: dialects.primary().format().to_owned(),
            container_only,
            geometry_transferred,
            coverage,
            losses,
            notes,
            transfer_ledger,
            dialects: Some(dialects),
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
            format: format.into(),
            container_only,
            geometry_transferred,
            coverage,
            losses,
            notes,
            transfer_ledger,
            dialects: None,
        }
    }

    /// Returns the source format id.
    #[must_use]
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Returns the classified dialect layers, if decoding classified them.
    #[must_use]
    pub fn dialects(&self) -> Option<&DialectLayers> {
        self.dialects.as_ref()
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
