// SPDX-License-Identifier: Apache-2.0
//! Decode reports, coverage, and delivered content scope.

use std::collections::BTreeMap;

use crate::document::ContentScope;
use cadmpeg_core::dialect::{DialectLayers, FormatIdentity};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::report::loss::LossNote;

/// Transfer status and loss details from a successful decode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "DecodeReportWire", from = "DecodeReportWire")]
#[serde(deny_unknown_fields)]
pub struct DecodeReport {
    classification: FormatIdentity<DialectLayers>,
    transfer: DecodeTransfer,
    /// Decode coverage counts keyed by declared measure name.
    coverage: Coverage,
    /// Explicit loss notes.
    pub losses: Vec<LossNote>,
    /// Free-form informational notes (e.g. container findings).
    pub notes: Vec<String>,
}

/// Delivered content scope and B-rep geometry outcome.
///
/// The sealed wrapper derives `scope` from the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DecodeTransfer {
    /// Classes held by the decoded document.
    pub scope: ContentScope,
    /// Whether any B-rep geometry was transferred into the document.
    pub geometry_transferred: bool,
}

impl DecodeTransfer {
    /// Whether the document holds no neutral class.
    pub const fn container_only(self) -> bool {
        self.scope.is_empty()
    }
    /// Whether B-rep geometry was transferred into the document.
    pub const fn geometry_transferred(self) -> bool {
        self.geometry_transferred
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct DecodeReportWire {
    /// Source format identity and its classified dialect layers.
    identity: FormatIdentity<DialectLayers>,
    /// Typed source-transfer state.
    transfer: DecodeTransfer,
    /// Decode coverage counts keyed by declared measure name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    coverage: BTreeMap<String, usize>,
    /// Explicit loss notes.
    losses: Vec<LossNote>,
    /// Free-form informational notes (e.g. container findings).
    notes: Vec<String>,
}

impl From<DecodeReport> for DecodeReportWire {
    fn from(report: DecodeReport) -> Self {
        let DecodeReport {
            classification,
            transfer,
            coverage,
            losses,
            notes,
        } = report;
        Self {
            identity: classification,
            transfer,
            coverage: coverage.into_wire(),
            losses,
            notes,
        }
    }
}

impl From<DecodeReportWire> for DecodeReport {
    fn from(wire: DecodeReportWire) -> Self {
        Self {
            classification: wire.identity,
            transfer: wire.transfer,
            coverage: Coverage::from_wire(wire.coverage),
            losses: wire.losses,
            notes: wire.notes,
        }
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

/// A typed decode-coverage measure name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoverageKey(&'static str);

impl CoverageKey {
    /// Declares a coverage key with a static wire name.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// Returns the coverage key's wire name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0
    }
}

/// A statically declared coverage-name template with one numeric component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexedCoverageKey {
    prefix: &'static str,
    suffix: &'static str,
}

/// A statically declared coverage-name template with one hexadecimal byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexByteCoverageKey {
    prefix: &'static str,
    suffix: &'static str,
}

impl IndexedCoverageKey {
    /// Declares a coverage-name template whose index uses decimal notation.
    #[must_use]
    pub const fn decimal(prefix: &'static str, suffix: &'static str) -> Self {
        Self { prefix, suffix }
    }
}

impl HexByteCoverageKey {
    /// Declares a coverage-name template whose byte uses two lowercase
    /// hexadecimal digits.
    #[must_use]
    pub const fn new(prefix: &'static str, suffix: &'static str) -> Self {
        Self { prefix, suffix }
    }
}

/// Decode coverage whose entries can be written only through declared keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    entries: BTreeMap<String, usize>,
}

impl Coverage {
    /// Records a static key.
    pub fn record(&mut self, key: CoverageKey, count: usize) {
        self.entries.insert(key.0.to_owned(), count);
    }

    /// Records a decimal-indexed key.
    pub fn record_indexed(&mut self, key: IndexedCoverageKey, index: u32, count: usize) {
        self.entries
            .insert(format!("{}{index}{}", key.prefix, key.suffix), count);
    }

    /// Records a hexadecimal-byte key.
    pub fn record_hex_byte(&mut self, key: HexByteCoverageKey, value: u8, count: usize) {
        self.entries
            .insert(format!("{}{:02x}{}", key.prefix, value, key.suffix), count);
    }

    /// Records an already-copied declared key without allocating another key.
    pub fn record_owned(
        &mut self,
        key: CoverageKey,
        name: String,
        count: usize,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if name != key.0 {
            return Err(cadmpeg_core::CodecError::malformed(
                "coverage name does not match its declared key",
            ));
        }
        self.entries.insert(name, count);
        Ok(())
    }

    /// Returns all recorded counts by their wire names.
    #[must_use]
    fn as_map(&self) -> &BTreeMap<String, usize> {
        &self.entries
    }

    fn from_wire(entries: BTreeMap<String, usize>) -> Self {
        Self { entries }
    }

    fn into_wire(self) -> BTreeMap<String, usize> {
        self.entries
    }
}

impl Extend<(CoverageKey, usize)> for Coverage {
    fn extend<T: IntoIterator<Item = (CoverageKey, usize)>>(&mut self, iter: T) {
        for (key, count) in iter {
            self.entries.insert(key.0.to_owned(), count);
        }
    }
}

impl FromIterator<(CoverageKey, usize)> for Coverage {
    fn from_iter<T: IntoIterator<Item = (CoverageKey, usize)>>(iter: T) -> Self {
        let mut coverage = Self::default();
        coverage.extend(iter);
        coverage
    }
}

impl std::ops::Deref for Coverage {
    type Target = BTreeMap<String, usize>;

    fn deref(&self) -> &Self::Target {
        self.as_map()
    }
}

impl DecodeReport {
    /// Reports a discarded stale sidecar during a current neutral CADIR parse.
    #[must_use]
    pub fn stale_sidecar(ir: &crate::CadIr, message: impl Into<String>) -> Self {
        Self {
            classification: FormatIdentity::unclassified("cadir"),
            transfer: DecodeTransfer {
                scope: ir.scope,
                geometry_transferred: !ir.model.bodies.is_empty(),
            },
            coverage: Coverage::default(),
            losses: vec![crate::codec::DecodeLossCode::SidecarStale.note(message)],
            notes: Vec::new(),
        }
    }

    /// Stamps a classification onto a report body.
    ///
    /// The sealed decode wrapper is the production caller: the classification
    /// is the document's own source identity, authored once by the backend.
    #[must_use]
    pub(crate) fn from_body(
        classification: FormatIdentity<DialectLayers>,
        body: crate::codec::DecodeBody,
    ) -> Self {
        Self {
            classification,
            transfer: body.transfer,
            coverage: body.coverage,
            losses: body.losses,
            notes: body.notes,
        }
    }

    /// Constructs a classified report whose format is its primary layer's format.
    #[must_use]
    #[cfg(test)]
    fn classified(
        dialects: DialectLayers,
        transfer: DecodeTransfer,
        coverage: BTreeMap<String, usize>,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            classification: FormatIdentity::classified(dialects),
            transfer,
            coverage: Coverage::from_wire(coverage),
            losses,
            notes,
        }
    }

    /// Constructs an unclassified report for a known source format.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn unclassified(
        format: impl Into<String>,
        transfer: DecodeTransfer,
        coverage: BTreeMap<String, usize>,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            classification: FormatIdentity::unclassified(format),
            transfer,
            coverage: Coverage::from_wire(coverage),
            losses,
            notes,
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

    /// Returns whether the decode stopped at the container layer.
    #[must_use]
    pub const fn container_only(&self) -> bool {
        self.transfer.container_only()
    }

    /// Classes held by the decoded document.
    #[must_use]
    pub const fn scope(&self) -> ContentScope {
        self.transfer.scope
    }

    /// Returns whether B-rep geometry was transferred into the IR.
    #[must_use]
    pub const fn geometry_transferred(&self) -> bool {
        self.transfer.geometry_transferred()
    }
}

#[cfg(test)]
mod tests;
