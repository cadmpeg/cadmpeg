// SPDX-License-Identifier: Apache-2.0
//! Closed FCStd source ledgers retained outside the document.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The three FCStd source accounting arenas, in their original record order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FreecadSourceLedgers {
    /// Ordered physical archive source spans.
    pub physical_ledger: Vec<ArchiveSpan>,
    /// Ordered source spans within logical archive members.
    pub logical_ledger: Vec<LogicalSpan>,
    /// Exact physical and logical source accounting facts.
    pub byte_coverage: ByteCoverageRecord,
}

/// A nonempty half-open byte interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    start: u64,
    end: u64,
}

impl ByteSpan {
    /// Admit a nonempty half-open source byte interval.
    pub fn try_new(start: u64, end: u64) -> Result<Self, String> {
        if start >= end {
            return Err("byte span start must be less than end".to_owned());
        }
        Ok(Self { start, end })
    }
    /// First byte in this source interval.
    pub fn start(self) -> u64 {
        self.start
    }
    /// First byte after this source interval.
    pub fn end(self) -> u64 {
        self.end
    }
}

/// ZIP physical-ledger role stored on one archive span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveSpanRole {
    /// ZIP local-header signature for the named entry.
    LocalSignature(String),
    /// ZIP local-header fields for the named entry.
    LocalFields(String),
    /// ZIP local-header name for the named entry.
    LocalName(String),
    /// ZIP local-header extra data for the named entry.
    LocalExtra(String),
    /// ZIP compressed payload for the named entry.
    CompressedPayload(String),
    /// ZIP data descriptor for the named entry.
    DataDescriptor(String),
    /// ZIP padding following the named entry.
    EntryArchivePadding(String),
    /// ZIP central-header signature for the named entry.
    CentralSignature(String),
    /// ZIP central-header fields for the named entry.
    CentralFields(String),
    /// ZIP central-header name for the named entry.
    CentralName(String),
    /// ZIP central-header extra data for the named entry.
    CentralExtra(String),
    /// ZIP central-header comment for the named entry.
    CentralComment(String),
    /// ZIP64 end-of-central-directory record.
    Zip64EndRecord,
    /// ZIP64 end-of-central-directory locator.
    Zip64EndLocator,
    /// ZIP end-of-central-directory record.
    EndRecord,
    /// ZIP padding not owned by an entry.
    ArchivePadding,
}

impl ArchiveSpanRole {
    /// Stable physical-ledger label retained on the CADIR wire.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalSignature(_) => "local-signature",
            Self::LocalFields(_) => "local-fields",
            Self::LocalName(_) => "local-name",
            Self::LocalExtra(_) => "local-extra",
            Self::CompressedPayload(_) => "compressed-payload",
            Self::DataDescriptor(_) => "data-descriptor",
            Self::EntryArchivePadding(_) | Self::ArchivePadding => "archive-padding",
            Self::CentralSignature(_) => "central-signature",
            Self::CentralFields(_) => "central-fields",
            Self::CentralName(_) => "central-name",
            Self::CentralExtra(_) => "central-extra",
            Self::CentralComment(_) => "central-comment",
            Self::Zip64EndRecord => "zip64-end-record",
            Self::Zip64EndLocator => "zip64-end-locator",
            Self::EndRecord => "end-record",
        }
    }

    /// Owning entry, when the role is entry-owned.
    fn entry(&self) -> Option<&str> {
        match self {
            Self::LocalSignature(entry)
            | Self::LocalFields(entry)
            | Self::LocalName(entry)
            | Self::LocalExtra(entry)
            | Self::CompressedPayload(entry)
            | Self::DataDescriptor(entry)
            | Self::EntryArchivePadding(entry)
            | Self::CentralSignature(entry)
            | Self::CentralFields(entry)
            | Self::CentralName(entry)
            | Self::CentralExtra(entry)
            | Self::CentralComment(entry) => Some(entry),
            Self::Zip64EndRecord
            | Self::Zip64EndLocator
            | Self::EndRecord
            | Self::ArchivePadding => None,
        }
    }

    fn from_label(label: &str, entry: Option<String>) -> Result<Self, String> {
        let named = |entry: Option<String>, ctor: fn(String) -> Self| {
            entry
                .map(ctor)
                .ok_or_else(|| format!("archive span role {label} requires an owning entry"))
        };
        let unit = |entry: Option<String>, role: Self| {
            if entry.is_some() {
                Err(format!(
                    "archive span role {label} cannot carry an owning entry"
                ))
            } else {
                Ok(role)
            }
        };
        match label {
            "local-signature" => named(entry, Self::LocalSignature),
            "local-fields" => named(entry, Self::LocalFields),
            "local-name" => named(entry, Self::LocalName),
            "local-extra" => named(entry, Self::LocalExtra),
            "compressed-payload" => named(entry, Self::CompressedPayload),
            "data-descriptor" => named(entry, Self::DataDescriptor),
            "archive-padding" => match entry {
                Some(entry) => Ok(Self::EntryArchivePadding(entry)),
                None => Ok(Self::ArchivePadding),
            },
            "central-signature" => named(entry, Self::CentralSignature),
            "central-fields" => named(entry, Self::CentralFields),
            "central-name" => named(entry, Self::CentralName),
            "central-extra" => named(entry, Self::CentralExtra),
            "central-comment" => named(entry, Self::CentralComment),
            "zip64-end-record" => unit(entry, Self::Zip64EndRecord),
            "zip64-end-locator" => unit(entry, Self::Zip64EndLocator),
            "end-record" => unit(entry, Self::EndRecord),
            _ => Err(format!("unknown archive span role {label}")),
        }
    }
}

/// One physical archive span.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "ArchiveSpanWire"))]
#[serde(try_from = "ArchiveSpanWire")]
pub struct ArchiveSpan {
    /// Stable span identity.
    pub id: String,
    /// Nonempty byte interval.
    pub span: ByteSpan,
    /// Structural role.
    pub role: ArchiveSpanRole,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ArchiveSpanWire {
    /// Original physical span identity.
    id: String,
    /// First physical source byte.
    start: u64,
    /// First byte after the physical source span.
    end: u64,
    /// Closed physical source span role.
    role: String,
    /// Exact archive entry name for an entry-owned role.
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    entry: Option<String>,
}

#[derive(Serialize)]
struct ArchiveSpanOut<'a> {
    id: &'a str,
    start: u64,
    end: u64,
    role: &'static str,
    entry: Option<&'a str>,
}

impl Serialize for ArchiveSpan {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ArchiveSpanOut {
            id: &self.id,
            start: self.span.start(),
            end: self.span.end(),
            role: self.role.as_str(),
            entry: self.role.entry(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<ArchiveSpanWire> for ArchiveSpan {
    type Error = String;

    fn try_from(wire: ArchiveSpanWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            span: ByteSpan::try_new(wire.start, wire.end)?,
            role: ArchiveSpanRole::from_label(&wire.role, wire.entry)?,
        })
    }
}

/// One non-overlapping logical-entry byte span.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "LogicalSpanWire"))]
#[serde(try_from = "LogicalSpanWire")]
pub struct LogicalSpan {
    /// Stable span identity.
    pub id: String,
    /// Owning archive entry.
    pub entry: String,
    /// Nonempty byte interval.
    pub span: ByteSpan,
    /// Span family and owner.
    pub classification: LogicalClassification,
}

/// Logical-entry family. Typed and named-opaque spans own a native record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogicalClassification {
    /// Framing bytes with no native owner.
    Structural,
    /// Typed native record bytes.
    Typed {
        /// Native record that owns the bytes.
        owner: String,
    },
    /// Named opaque payload bytes.
    NamedOpaque {
        /// Native record that owns the bytes.
        owner: String,
    },
}

impl LogicalClassification {
    /// Classification string retained on the CADIR wire.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Structural => "structural",
            Self::Typed { .. } => "typed",
            Self::NamedOpaque { .. } => "named_opaque",
        }
    }

    /// Native record that owns typed or opaque bytes.
    pub fn owner(&self) -> Option<&str> {
        match self {
            Self::Structural => None,
            Self::Typed { owner } | Self::NamedOpaque { owner } => Some(owner.as_str()),
        }
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LogicalSpanWire {
    /// Original logical source span identity.
    id: String,
    /// Exact archive entry name.
    entry: String,
    /// First byte within the logical archive member.
    start: u64,
    /// First byte after the logical source span.
    end: u64,
    /// Closed logical source span classification.
    classification: String,
    /// Original native owner identity for typed or named opaque content.
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    owner: Option<String>,
}

#[derive(Serialize)]
struct LogicalSpanOut<'a> {
    id: &'a str,
    entry: &'a str,
    start: u64,
    end: u64,
    classification: &'static str,
    owner: Option<&'a str>,
}

impl Serialize for LogicalSpan {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LogicalSpanOut {
            id: &self.id,
            entry: &self.entry,
            start: self.span.start(),
            end: self.span.end(),
            classification: self.classification.as_str(),
            owner: self.classification.owner(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<LogicalSpanWire> for LogicalSpan {
    type Error = String;

    fn try_from(wire: LogicalSpanWire) -> Result<Self, Self::Error> {
        let classification = match (wire.classification.as_str(), wire.owner) {
            ("structural", None) => LogicalClassification::Structural,
            ("typed", Some(owner)) => LogicalClassification::Typed { owner },
            ("named_opaque", Some(owner)) => LogicalClassification::NamedOpaque { owner },
            ("structural", Some(_)) => {
                return Err("structural logical span cannot carry an owner".to_owned());
            }
            ("typed" | "named_opaque", None) => {
                return Err("typed and named_opaque logical spans require an owner".to_owned());
            }
            _ => return Err("unknown logical classification".to_owned()),
        };
        Ok(Self {
            id: wire.id,
            entry: wire.entry,
            span: ByteSpan::try_new(wire.start, wire.end)?,
            classification,
        })
    }
}

/// Deterministic whole-archive byte-accounting summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ByteCoverageRecord {
    /// Stable report identity.
    pub id: String,
    /// Complete physical archive length.
    pub physical_byte_len: u64,
    /// Number of physical partition spans.
    pub physical_span_count: usize,
    /// Number of logical archive entries.
    pub logical_entry_count: usize,
    /// Sum of complete logical entry lengths.
    pub logical_byte_len: u64,
    /// Number of logical partition spans.
    pub logical_span_count: usize,
    /// Byte count for each closed classification.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub classification_bytes: BTreeMap<String, u64>,
    /// Sorted logical entries containing named opaque bytes.
    pub named_opaque_entries: Vec<String>,
    /// Whether both physical and logical partitions close exactly.
    pub exact: bool,
}

#[cfg(test)]
mod tests;
