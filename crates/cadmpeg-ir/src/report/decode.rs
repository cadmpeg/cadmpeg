// SPDX-License-Identifier: Apache-2.0
//! Decode reports, coverage, and source-transfer disposition.

use std::collections::BTreeMap;

use cadmpeg_core::dialect::{DialectLayers, FormatIdentity};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{LossNote, Severity};

/// Transfer status and loss details from a successful decode.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(into = "DecodeReportWire")]
pub struct DecodeReport {
    classification: FormatIdentity<DialectLayers>,
    transfer: DecodeTransfer,
    /// Decode coverage counts keyed by measure name.
    pub coverage: BTreeMap<String, usize>,
    /// Explicit loss notes.
    pub losses: Vec<LossNote>,
    /// Free-form informational notes (e.g. container findings).
    pub notes: Vec<String>,
    /// Per-source disposition ledger for decoded records and entities.
    pub transfer_ledger: TransferLedger,
}

/// Mutually exclusive source-transfer states for a decode report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeTransfer {
    /// The request stopped after container inspection and transferred no
    /// semantic geometry.
    ContainerOnly,
    /// The request ran the full decoder, with the recorded B-rep geometry
    /// outcome.
    Full {
        /// Whether B-rep geometry was transferred into the IR.
        geometry_transferred: bool,
    },
}

impl DecodeTransfer {
    /// Constructs the state for a full decode with its geometry outcome.
    #[must_use]
    pub const fn full(geometry_transferred: bool) -> Self {
        Self::Full {
            geometry_transferred,
        }
    }

    /// Returns whether the request stopped at the container layer.
    #[must_use]
    pub const fn container_only(self) -> bool {
        matches!(self, Self::ContainerOnly)
    }

    /// Returns whether B-rep geometry was transferred into the IR.
    #[must_use]
    pub const fn geometry_transferred(self) -> bool {
        matches!(
            self,
            Self::Full {
                geometry_transferred: true
            }
        )
    }
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

#[cfg(test)]
mod tests {
    #[cfg(feature = "schema")]
    #[test]
    fn current_decode_report_schema_requires_dialects() {
        let schema = serde_json::to_value(schemars::schema_for!(super::DecodeReport))
            .expect("decode report schema serializes");
        let required = schema["required"]
            .as_array()
            .expect("decode report schema has required fields");
        assert!(
            required.iter().any(|field| field == "dialects"),
            "{schema:#}"
        );
    }
}

impl From<DecodeReport> for DecodeReportWire {
    fn from(report: DecodeReport) -> Self {
        let DecodeReport {
            classification,
            transfer,
            coverage,
            losses,
            notes,
            transfer_ledger,
        } = report;
        let (format, dialects) = classification.into_wire_parts();
        Self {
            format,
            container_only: transfer.container_only(),
            geometry_transferred: transfer.geometry_transferred(),
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
        let classification = FormatIdentity::from_wire(wire.format, wire.dialects)
            .map_err(serde::de::Error::custom)?;
        let transfer = match (wire.container_only, wire.geometry_transferred) {
            (true, true) => {
                return Err(serde::de::Error::custom(
                    "container-only decode report cannot claim geometry transfer",
                ));
            }
            (true, false) => DecodeTransfer::ContainerOnly,
            (false, geometry_transferred) => DecodeTransfer::full(geometry_transferred),
        };
        Ok(Self {
            classification,
            transfer,
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
        let mut schema = DecodeReportWire::json_schema(generator);
        crate::schema::require_object_fields(&mut schema, ["dialects"]);
        schema
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
        transfer: DecodeTransfer,
        coverage: BTreeMap<String, usize>,
        losses: Vec<LossNote>,
        notes: Vec<String>,
        transfer_ledger: TransferLedger,
    ) -> Self {
        Self {
            classification: FormatIdentity::classified(dialects),
            transfer,
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
        transfer: DecodeTransfer,
        coverage: BTreeMap<String, usize>,
        losses: Vec<LossNote>,
        notes: Vec<String>,
        transfer_ledger: TransferLedger,
    ) -> Self {
        Self {
            classification: FormatIdentity::unclassified(format),
            transfer,
            coverage,
            losses,
            notes,
            transfer_ledger,
        }
    }

    /// Returns the source format id.
    #[must_use]
    pub fn format(&self) -> &str {
        self.classification.format()
    }

    /// Returns the classified dialect layers, if decoding classified them.
    #[must_use]
    pub fn dialects(&self) -> Option<&DialectLayers> {
        self.classification.classified_payload()
    }

    /// Returns the typed source-transfer state.
    #[must_use]
    pub const fn transfer(&self) -> DecodeTransfer {
        self.transfer
    }

    /// Returns whether the decode stopped at the container layer.
    #[must_use]
    pub const fn container_only(&self) -> bool {
        self.transfer.container_only()
    }

    /// Returns whether B-rep geometry was transferred into the IR.
    #[must_use]
    pub const fn geometry_transferred(&self) -> bool {
        self.transfer.geometry_transferred()
    }

    /// Records that a full decode transferred B-rep geometry.
    pub fn mark_geometry_transferred(&mut self) {
        self.transfer = DecodeTransfer::full(true);
    }

    /// Stamps the caller's requested decode scope while retaining the
    /// backend's full-decode geometry outcome.
    pub(crate) fn stamp_request_scope(&mut self, container_only: bool) {
        self.transfer = if container_only {
            DecodeTransfer::ContainerOnly
        } else {
            DecodeTransfer::full(self.geometry_transferred())
        };
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
