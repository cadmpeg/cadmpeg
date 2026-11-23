// SPDX-License-Identifier: Apache-2.0
//! Bounded feature census results and host-controlled scan budgets.

mod budget;
mod inflate;

#[cfg(test)]
mod tests;

#[cfg(not(target_arch = "wasm32"))]
pub use budget::SystemClock;
pub use budget::{FeatureVisit, ManualClock, ScanBudget, ScanClock, ScanReader};
pub use inflate::InflateFraming;

use std::collections::BTreeMap;
use std::time::Duration;

use cadmpeg_derive::DeserializeName;
use serde::{Deserialize, Serialize};

/// Ceilings for one scan, including nested documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// Cumulative physical bytes read.
    pub source_bytes: u64,
    /// Cumulative bytes produced by decompression.
    pub expanded_bytes: u64,
    /// Cumulative bytes admitted for retained data and parser storage.
    pub retained_bytes: u64,
    /// Host-measured elapsed time.
    pub elapsed_time: Duration,
    /// Maximum admitted feature visits.
    pub feature_count: u64,
    /// Maximum active document or reference depth, with the root at one.
    pub nesting_depth: u64,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            source_bytes: 16 * 1024 * 1024,
            expanded_bytes: 32 * 1024 * 1024,
            retained_bytes: 64 * 1024 * 1024,
            elapsed_time: Duration::from_secs(5),
            feature_count: 100_000,
            nesting_depth: 32,
        }
    }
}

/// Evidence about the listing's readability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum Readability {
    /// The complete feature listing was read.
    #[serde(deserialize_with = "crate::tag_only::refuse_value")]
    KnownYes,
    /// Validated evidence prevents reading the listing.
    KnownNo(String),
    /// Reading the listing needs more evidence or resources.
    Unknown(String),
}

/// Suppression state established from the listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[serde(rename_all = "snake_case")]
pub enum Suppressed {
    /// Suppression is set.
    Yes,
    /// Suppression is clear.
    No,
    /// The listing does not establish suppression.
    Unknown,
}

/// Operand state established from the listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[serde(rename_all = "snake_case")]
pub enum Operands {
    /// All operands have validated bindings.
    Resolved,
    /// A required operand has a validated missing binding.
    Unresolved,
    /// Operand resolution needs deeper reading.
    Unknown,
}

/// One source-ordered feature-list entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureRow {
    /// Zero-based position across the scanned listing.
    pub ordinal: u64,
    /// Native feature kind, or `unknown` when the listing cannot establish it.
    pub kind: String,
    /// Suppression evidence.
    pub suppressed: Suppressed,
    /// Operand evidence.
    pub operands: Operands,
}

/// Validated feature-list evidence.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureCensus {
    /// Validated total, available after the entire listing was read.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_total"
    )]
    pub total: Option<u64>,
    /// Counts of admitted rows by native kind.
    #[serde(deserialize_with = "crate::distinct_keys::btree_map")]
    pub counts_by_kind: BTreeMap<String, u64>,
    /// Rows in source order.
    pub rows: Vec<FeatureRow>,
    /// Whether every feature-list record was visited.
    pub complete: bool,
}

/// An unvalidated size claim made by the source container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredSize {
    /// Source member or table name.
    pub name: String,
    /// Declared byte length.
    pub bytes: u64,
}

/// Why a scan stopped before a complete census.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum StopReason {
    /// The physical read ceiling refused the next operation.
    #[serde(deserialize_with = "crate::tag_only::refuse_value")]
    SourceBytes,
    /// The expansion ceiling refused the next operation.
    #[serde(deserialize_with = "crate::tag_only::refuse_value")]
    ExpandedBytes,
    /// The retained-storage ceiling refused the next operation.
    #[serde(deserialize_with = "crate::tag_only::refuse_value")]
    RetainedBytes,
    /// The elapsed-time ceiling expired.
    #[serde(deserialize_with = "crate::tag_only::refuse_value")]
    ElapsedTime,
    /// The feature-visit ceiling refused the next visit.
    #[serde(deserialize_with = "crate::tag_only::refuse_value")]
    FeatureCount,
    /// The active nesting ceiling refused the next descent.
    #[serde(deserialize_with = "crate::tag_only::refuse_value")]
    NestingDepth,
    /// The host cancelled the scan.
    #[serde(deserialize_with = "crate::tag_only::refuse_value")]
    Cancelled,
    /// Source bytes ended inside a required field.
    Truncated(String),
    /// A parsed record has invalid framing.
    Malformed(String),
    /// The selected reader has no scan for this listing.
    Unsupported(String),
    /// Detection could not select a reader.
    #[serde(deserialize_with = "crate::tag_only::refuse_value")]
    Unrecognized,
}

impl From<crate::decode::ParseError> for StopReason {
    fn from(error: crate::decode::ParseError) -> Self {
        match error.kind {
            crate::decode::ParseErrorKind::UnexpectedEof { .. } => {
                Self::Truncated(error.operation.into())
            }
            crate::decode::ParseErrorKind::InvalidValue
            | crate::decode::ParseErrorKind::InvalidFraming => {
                Self::Malformed(error.operation.into())
            }
        }
    }
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for StopReason {}

/// Feature-only scan evidence, independent of design and geometry arenas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignScan {
    /// Detected format identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_format"
    )]
    pub format: Option<String>,
    /// Version established from source header bytes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_version"
    )]
    pub source_version: Option<String>,
    /// Document kind established from source header bytes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_document_kind"
    )]
    pub document_kind: Option<String>,
    /// Evidence about reading the feature listing.
    pub readability: Readability,
    /// `None` means the feature census did not start.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_census"
    )]
    pub census: Option<FeatureCensus>,
    /// Source claims kept separately from validated counts.
    pub declared_sizes: Vec<DeclaredSize>,
    /// Physical bytes charged before reads.
    pub bytes_read: u64,
    /// Refusal or framing failure that stopped this scan.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stop_reason"
    )]
    pub stop_reason: Option<StopReason>,
}

impl DesignScan {
    /// Starts an unread census for a selected format.
    pub fn new(format: Option<&str>) -> Self {
        Self {
            format: format.map(str::to_owned),
            source_version: None,
            document_kind: None,
            readability: Readability::Unknown("feature census has not started".into()),
            census: None,
            declared_sizes: Vec::new(),
            bytes_read: 0,
            stop_reason: None,
        }
    }

    /// Admits a row before allocating its retained strings and collections.
    pub fn feature(
        &mut self,
        budget: &ScanBudget<'_>,
        kind: &str,
        suppressed: Suppressed,
    ) -> Result<(), StopReason> {
        let visit = budget.feature()?;
        self.record_feature(visit, kind, suppressed)
    }

    /// Retains the row for a visit admitted before its record was read.
    pub fn record_feature(
        &mut self,
        FeatureVisit { budget }: FeatureVisit<'_>,
        kind: &str,
        suppressed: Suppressed,
    ) -> Result<(), StopReason> {
        let length = crate::decode::u64_from_index(kind.len());
        let storage = length
            .checked_mul(2)
            .and_then(|n| n.checked_add(1024))
            .ok_or(StopReason::RetainedBytes)?;
        budget.retain(storage)?;
        let census = self.census.get_or_insert_with(FeatureCensus::default);
        let count = census.counts_by_kind.entry(kind.to_owned()).or_default();
        *count = count.checked_add(1).ok_or(StopReason::FeatureCount)?;
        census.rows.push(FeatureRow {
            ordinal: crate::decode::u64_from_index(census.rows.len()),
            kind: kind.to_owned(),
            suppressed,
            operands: Operands::Unknown,
        });
        Ok(())
    }

    /// Records one source size claim after admitting its storage.
    pub fn declared_size(
        &mut self,
        budget: &ScanBudget<'_>,
        name: &str,
        bytes: u64,
    ) -> Result<(), StopReason> {
        budget.retain(
            crate::decode::u64_from_index(name.len())
                .checked_add(256)
                .ok_or(StopReason::RetainedBytes)?,
        )?;
        self.declared_sizes.push(DeclaredSize {
            name: name.to_owned(),
            bytes,
        });
        Ok(())
    }

    /// Marks a fully read listing, including a validated empty listing.
    pub fn complete(&mut self) {
        let census = self.census.get_or_insert_with(FeatureCensus::default);
        census.total = Some(crate::decode::u64_from_index(census.rows.len()));
        census.complete = true;
        self.readability = Readability::KnownYes;
    }

    /// Applies the final scan status while preserving an admitted prefix.
    pub fn finish(&mut self, budget: &ScanBudget<'_>, result: Result<(), StopReason>) {
        self.bytes_read = budget.bytes_read();
        let result = budget.stop_reason().map_or(result, Err);
        if let Err(reason) = result {
            if let Some(census) = &mut self.census {
                census.complete = false;
                census.total = None;
            }
            self.readability = Readability::Unknown(format!("{reason:?}"));
            self.stop_reason = Some(reason);
        }
    }
}

crate::named_optional_field!(deserialize_total, u64, "total");

crate::named_optional_field!(deserialize_format, String, "format");

crate::named_optional_field!(deserialize_source_version, String, "source_version");

crate::named_optional_field!(deserialize_document_kind, String, "document_kind");

crate::named_optional_field!(deserialize_census, FeatureCensus, "census");

crate::named_optional_field!(deserialize_stop_reason, StopReason, "stop_reason");
