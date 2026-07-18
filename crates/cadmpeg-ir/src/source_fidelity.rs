// SPDX-License-Identifier: Apache-2.0
//! Serialized multi-space source-fidelity ledger.
//!
//! Every space carries a stable
//! [`CanonicalSpaceId`] derived from its origin — `source`, `entry:<path>`,
//! `stream:<path>#<n>` — so two decodes of the same file produce identical
//! sidecars regardless of the order in which spaces were registered.
//!
//! A sidecar is a set of [`AddressSpaceLedger`]s. Each ledger tiles its space's
//! byte range `[0, length)` exactly with [`LedgerSpan`]s and records how the
//! space was derived through a [`SerializedOrigin`]. The origins form a DAG
//! rooted at the `source` space; [`SourceFidelity::validate`] rejects cycles,
//! dangling references, and ranges that fall outside the space they index.
//!
//! [`LedgerCapability`] states whether every opaque span additionally resolves
//! to retained bytes. Retention can be refused while classification still
//! succeeds.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::annotations::Annotations;
use crate::byte_ledger::ByteLedger;
use crate::document::CadIr;
use crate::native::NativeConvertError;
use crate::unknown::UnknownRecord;

/// Serialized schema version written into every v2 sidecar.
pub const SOURCE_FIDELITY_VERSION: &str = "2";

/// Whether opaque spans additionally resolve to retained bytes.
///
/// `max_retained_bytes` can refuse the retention recovery needs while
/// classification still succeeds.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LedgerCapability {
    /// Every byte classified; opaque spans carry digests, not necessarily
    /// bytes.
    Accounted,
    /// Additionally, every opaque span resolves to retained bytes through a
    /// subrange reference.
    Recoverable,
}

impl LedgerCapability {
    /// Returns whether this capability requires opaque spans to resolve to
    /// retained bytes.
    pub fn requires_retained_bytes(self) -> bool {
        matches!(self, LedgerCapability::Recoverable)
    }
}

/// A stable serialized address-space identity.
///
/// Never a runtime registration ordinal. Constructed only through
/// [`CanonicalSpaceId::source`], [`CanonicalSpaceId::entry`], or
/// [`CanonicalSpaceId::stream`], which fix the canonical spelling. Serialized
/// as a plain string.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct CanonicalSpaceId(String);

impl CanonicalSpaceId {
    /// The root input space: `source`.
    pub fn source() -> Self {
        CanonicalSpaceId("source".to_string())
    }

    /// A container entry space: `entry:<path>`.
    pub fn entry(path: &str) -> Self {
        CanonicalSpaceId(format!("entry:{path}"))
    }

    /// The `<n>`-th reconstructed stream of a path: `stream:<path>#<n>`.
    pub fn stream(path: &str, ordinal: u32) -> Self {
        CanonicalSpaceId(format!("stream:{path}#{ordinal}"))
    }

    /// Returns the canonical id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether this id names the root `source` space.
    pub fn is_source(&self) -> bool {
        self.0 == "source"
    }
}

/// A half-open byte range `[start, end)` within one space.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct SerializedRange {
    /// Inclusive start offset.
    pub start: u64,
    /// Exclusive end offset.
    pub end: u64,
}

impl SerializedRange {
    /// Returns the range length, saturating an inverted range at zero.
    pub fn len(self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Returns whether the range is empty or inverted.
    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }

    /// Returns whether `[start, end)` fits within `[0, bound)`.
    fn within(self, bound: u64) -> bool {
        self.start <= self.end && self.end <= bound
    }
}

/// A byte range qualified by the canonical space it indexes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SpaceExtent {
    /// The space the range indexes.
    pub space: CanonicalSpaceId,
    /// The range within that space.
    pub range: SerializedRange,
}

/// Stable transform names for derived spaces.
///
/// Names are the serialized contract; new transforms extend the enum without
/// renaming existing variants.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SerializedTransformKind {
    /// The derived bytes are the decompression of the input extents.
    Decompress,
}

/// How a serialized space came to exist, relative to earlier spaces.
///
/// Root spaces name themselves; every other origin references only spaces that
/// exist and that it does not reach through a cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SerializedOrigin {
    /// The root input space.
    Root,
    /// A contiguous borrowed subrange of a parent space.
    Slice {
        /// The parent space.
        parent: CanonicalSpaceId,
        /// The borrowed range within the parent.
        range: SerializedRange,
    },
    /// An assembly of multiple parent extents.
    Concat {
        /// The ordered input extents.
        segments: Vec<SpaceExtent>,
    },
    /// New bytes derived from named input extents by a transform.
    Transform {
        /// The input extents consumed by the transform.
        inputs: Vec<SpaceExtent>,
        /// The transform applied.
        transform: SerializedTransformKind,
    },
}

impl SerializedOrigin {
    /// Returns the spaces this origin references, in declaration order.
    fn referenced(&self) -> Vec<&SpaceExtent> {
        match self {
            SerializedOrigin::Root => Vec::new(),
            SerializedOrigin::Slice { .. } => Vec::new(),
            SerializedOrigin::Concat { segments } => segments.iter().collect(),
            SerializedOrigin::Transform { inputs, .. } => inputs.iter().collect(),
        }
    }
}

/// Byte classification for one span.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SpanClass {
    /// Interpreted into the typed model.
    Typed,
    /// Container framing with no semantic content.
    Structural,
    /// Uninterpreted payload preserved by digest, optionally by bytes.
    Opaque,
}

/// A containment reference from an opaque span into a retained blob.
///
/// One blob may back many opaque spans through distinct subranges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RetainedRef {
    /// The retained blob's stable identity.
    pub blob: String,
    /// The subrange of the blob this span occupies.
    pub range: SerializedRange,
}

/// One tile of a space's byte range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LedgerSpan {
    /// The span's range within its owning space.
    pub range: SerializedRange,
    /// The span's byte classification.
    pub class: SpanClass,
    /// The producing subsystem or record family.
    pub owner: String,
    /// A human-readable meaning for the span.
    pub meaning: String,
    /// Lowercase hexadecimal SHA-256 digest of the span's bytes.
    pub digest: String,
    /// Retained-bytes reference, present when the span is recoverable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained: Option<RetainedRef>,
}

/// One space: its identity, length, derivation, and complete tiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AddressSpaceLedger {
    /// The space's stable canonical identity.
    pub id: CanonicalSpaceId,
    /// The space's total byte length.
    pub length: u64,
    /// How the space was derived.
    pub origin: SerializedOrigin,
    /// The spans tiling `[0, length)`, in canonical (ascending-start) order.
    pub spans: Vec<LedgerSpan>,
}

/// A validation failure in a serialized sidecar.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FidelityError {
    /// The sidecar declares an unsupported schema version.
    #[error("unsupported source-fidelity version: {found}")]
    Version {
        /// The version string found in the sidecar.
        found: String,
    },
    /// Two spaces share a canonical id.
    #[error("duplicate space id: {id}")]
    DuplicateSpace {
        /// The repeated canonical id.
        id: String,
    },
    /// A nonempty ledger has no root source space.
    #[error("a nonempty ledger requires the source address space")]
    MissingSourceSpace,
    /// A space carries the wrong origin for its id class.
    #[error("space {id} has an origin inconsistent with its id")]
    OriginIdMismatch {
        /// The offending space id.
        id: String,
    },
    /// An origin references a space that is not present.
    #[error("space {id} references unknown space {referenced}")]
    DanglingReference {
        /// The referencing space id.
        id: String,
        /// The missing referenced id.
        referenced: String,
    },
    /// An origin references a range outside the referenced space's length.
    #[error(
        "space {id} references range {start}..{end} outside space {referenced} (length {length})"
    )]
    RangeOutOfBounds {
        /// The referencing space id.
        id: String,
        /// The referenced space id.
        referenced: String,
        /// The referenced range start.
        start: u64,
        /// The referenced range end.
        end: u64,
        /// The referenced space's length.
        length: u64,
    },
    /// The origin graph contains a cycle reaching the named space.
    #[error("origin cycle reaches space {id}")]
    OriginCycle {
        /// A space on the cycle.
        id: String,
    },
    /// A space's spans do not tile `[0, length)` exactly.
    #[error("space {id} spans do not tile [0, {length}) exactly at offset {offset}")]
    TilingGap {
        /// The offending space id.
        id: String,
        /// The space length.
        length: u64,
        /// The offset where tiling broke.
        offset: u64,
    },
    /// A recoverable ledger has an opaque span without retained bytes.
    #[error("space {id} opaque span at {offset} lacks retained bytes required by capability")]
    MissingRetained {
        /// The offending space id.
        id: String,
        /// The opaque span's start offset.
        offset: u64,
    },
    /// Two retained records share an id, making references ambiguous.
    #[error("duplicate retained record id: {id}")]
    DuplicateRetainedRecord {
        /// The repeated retained-record id.
        id: String,
    },
    /// A recoverable reference names no retained record.
    #[error("space {id} opaque span at {offset} references unknown retained record {blob}")]
    DanglingRetainedReference {
        /// The offending space id.
        id: String,
        /// The opaque span's start offset.
        offset: u64,
        /// The missing retained-record id.
        blob: String,
    },
    /// A retained record has no bytes despite a recoverable capability.
    #[error("retained record {id} has no data")]
    MissingRetainedData {
        /// The retained-record id.
        id: String,
    },
    /// Retained metadata disagrees with the actual byte count.
    #[error("retained record {id} declares {declared} bytes but contains {actual}")]
    RetainedRecordLengthMismatch {
        /// The retained-record id.
        id: String,
        /// Declared byte count.
        declared: u64,
        /// Actual byte count.
        actual: u64,
    },
    /// Retained metadata disagrees with the actual byte digest.
    #[error("retained record {id} has an invalid digest")]
    RetainedRecordDigestMismatch {
        /// The retained-record id.
        id: String,
    },
    /// A retained reference indexes bytes outside its retained record.
    #[error("space {id} opaque span at {offset} references {start}..{end} outside retained record {blob} (length {length})")]
    RetainedRangeOutOfBounds {
        /// The offending space id.
        id: String,
        /// The opaque span's start offset.
        offset: u64,
        /// The retained-record id.
        blob: String,
        /// Referenced start.
        start: u64,
        /// Referenced end.
        end: u64,
        /// Retained-record length.
        length: u64,
    },
    /// A recoverable span digest disagrees with its retained bytes.
    #[error("space {id} opaque span at {offset} disagrees with retained bytes")]
    RetainedSpanDigestMismatch {
        /// The offending space id.
        id: String,
        /// The opaque span's start offset.
        offset: u64,
    },
    /// A recoverable opaque span's retained subrange does not cover the span.
    #[error(
        "space {id} opaque span at {offset} spans {span_len} bytes but retains {retained_len}"
    )]
    RetainedLengthMismatch {
        /// The offending space id.
        id: String,
        /// The opaque span's start offset.
        offset: u64,
        /// The span's byte length.
        span_len: u64,
        /// The retained subrange's byte length.
        retained_len: u64,
    },
    /// A non-root space's origin references no parent, fabricating a second root.
    #[error("space {id} is not the root yet its origin references no parent space")]
    OriginWithoutReference {
        /// The offending space id.
        id: String,
    },
}

/// Source bytes retained to support exact recovery of opaque ledger spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RetainedSourceRecord {
    /// Stable identifier named by opaque byte-ledger spans.
    pub id: String,
    /// Source stream containing the retained range.
    pub stream: String,
    /// First byte offset in the source stream.
    pub offset: u64,
    /// Number of retained bytes.
    pub byte_len: u64,
    /// Lowercase hexadecimal SHA-256 of `data`.
    pub sha256: String,
    /// Retained bytes, when available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::bytes::option"
    )]
    #[schemars(with = "Option<String>")]
    pub data: Option<Vec<u8>>,
}

/// A complete serialized source-fidelity sidecar (schema v2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceFidelity {
    /// The schema version; always [`SOURCE_FIDELITY_VERSION`] when written.
    pub version: String,
    /// Whether opaque spans resolve to retained bytes.
    pub capability: LedgerCapability,
    /// The address spaces, in canonical (id-ascending) order.
    pub spaces: Vec<AddressSpaceLedger>,
    /// Flat source-stream accounting used by codecs that do not expose derived
    /// address spaces.
    #[serde(default)]
    pub byte_ledger: ByteLedger,
    /// Sparse source locations and conversion exactness.
    #[serde(default)]
    pub annotations: Annotations,
    /// Opaque source ranges retained for exact recovery.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_records: Vec<RetainedSourceRecord>,
}

impl Default for SourceFidelity {
    fn default() -> Self {
        Self {
            version: SOURCE_FIDELITY_VERSION.into(),
            capability: LedgerCapability::Accounted,
            spaces: Vec::new(),
            byte_ledger: ByteLedger::default(),
            annotations: Annotations::default(),
            retained_records: Vec::new(),
        }
    }
}

impl SourceFidelity {
    /// Builds a sidecar and puts it in canonical order.
    ///
    /// Sorts spaces by canonical id and each space's spans by ascending start.
    /// Ordering is independent of the order spaces were supplied, so repeat
    /// decodes serialize identically. Call [`SourceFidelity::validate`] to
    /// enforce the conservation invariant.
    pub fn new(capability: LedgerCapability, spaces: Vec<AddressSpaceLedger>) -> Self {
        let mut sidecar = SourceFidelity {
            version: SOURCE_FIDELITY_VERSION.to_string(),
            capability,
            spaces,
            ..SourceFidelity::default()
        };
        sidecar.canonicalize();
        sidecar
    }

    /// Sorts spaces and spans into canonical order in place.
    pub fn canonicalize(&mut self) {
        for space in &mut self.spaces {
            space.spans.sort_by(|a, b| {
                a.range
                    .start
                    .cmp(&b.range.start)
                    .then(a.range.end.cmp(&b.range.end))
            });
        }
        self.spaces.sort_by(|a, b| a.id.cmp(&b.id));
        self.byte_ledger.finalize();
        self.retained_records.sort_by(|left, right| {
            (&left.stream, left.offset, &left.id).cmp(&(&right.stream, right.offset, &right.id))
        });
    }

    /// Canonicalize every sidecar collection independently from the product model.
    pub fn finalize(&mut self) {
        self.canonicalize();
    }

    /// Find one retained source record by stable identifier.
    pub fn retained_record(&self, id: &str) -> Option<&RetainedSourceRecord> {
        self.retained_records.iter().find(|record| record.id == id)
    }

    /// Retain source records without adding them to the product model.
    pub fn retain_unknown_records(&mut self, stream: &str, records: &[UnknownRecord]) {
        self.retained_records
            .extend(records.iter().map(|record| RetainedSourceRecord {
                id: record.id.to_string(),
                stream: stream.into(),
                offset: record.offset,
                byte_len: record.byte_len,
                sha256: record.sha256.clone(),
                data: record.data.clone(),
            }));
    }

    /// Store source records in this sidecar and source-independent refs in the product model.
    pub fn attach_native_unknown_records(
        &mut self,
        ir: &mut CadIr,
        format: &str,
        records: &[UnknownRecord],
    ) -> Result<(), NativeConvertError> {
        self.retained_records.extend(records.iter().map(|record| {
            let stream = self
                .annotations
                .provenance
                .get(&record.id.0)
                .and_then(|provenance| self.annotations.streams.get(provenance.stream as usize))
                .cloned()
                .unwrap_or_else(|| "source".into());
            RetainedSourceRecord {
                id: record.id.to_string(),
                stream,
                offset: record.offset,
                byte_len: record.byte_len,
                sha256: record.sha256.clone(),
                data: record.data.clone(),
            }
        }));
        let product_records = records
            .iter()
            .map(crate::NativeUnknownRecord::from)
            .collect::<Vec<_>>();
        ir.set_native_unknowns(format, &product_records)
    }

    /// Join product links with retained records into a codec-local source view.
    pub fn native_unknown_records(
        &self,
        ir: &CadIr,
        format: &str,
    ) -> Result<Vec<UnknownRecord>, NativeConvertError> {
        ir.native_unknowns(format)?
            .into_iter()
            .map(|reference| {
                let retained = self.retained_record(&reference.id.0).ok_or_else(|| {
                    NativeConvertError::MissingRetainedSourceRecord(reference.id.0.clone())
                })?;
                Ok(UnknownRecord {
                    id: reference.id,
                    offset: retained.offset,
                    byte_len: retained.byte_len,
                    sha256: retained.sha256.clone(),
                    data: retained.data.clone(),
                    links: reference.links,
                })
            })
            .collect()
    }

    /// Validates the conservation invariant.
    ///
    /// Checks the schema version, unique ids, origin/id consistency, that every
    /// origin references only present spaces and in-bounds ranges, that every
    /// non-root space references at least one parent, that the origin graph is
    /// acyclic, that each space's spans tile `[0, length)` exactly, and — for a
    /// recoverable ledger — that every opaque span resolves to retained bytes
    /// whose subrange covers the span exactly.
    pub fn validate(&self) -> Result<(), FidelityError> {
        if self.version != SOURCE_FIDELITY_VERSION {
            return Err(FidelityError::Version {
                found: self.version.clone(),
            });
        }

        let mut lengths: BTreeMap<CanonicalSpaceId, u64> = BTreeMap::new();
        for space in &self.spaces {
            if lengths.insert(space.id.clone(), space.length).is_some() {
                return Err(FidelityError::DuplicateSpace {
                    id: space.id.as_str().to_string(),
                });
            }
        }
        let mut retained = BTreeMap::new();
        for record in &self.retained_records {
            if retained.insert(record.id.as_str(), record).is_some() {
                return Err(FidelityError::DuplicateRetainedRecord {
                    id: record.id.clone(),
                });
            }
            if self.capability.requires_retained_bytes() {
                let data =
                    record
                        .data
                        .as_deref()
                        .ok_or_else(|| FidelityError::MissingRetainedData {
                            id: record.id.clone(),
                        })?;
                let actual = u64::try_from(data.len()).unwrap_or(u64::MAX);
                if record.byte_len != actual {
                    return Err(FidelityError::RetainedRecordLengthMismatch {
                        id: record.id.clone(),
                        declared: record.byte_len,
                        actual,
                    });
                }
                if crate::hash::sha256_hex(data) != record.sha256 {
                    return Err(FidelityError::RetainedRecordDigestMismatch {
                        id: record.id.clone(),
                    });
                }
            }
        }

        for space in &self.spaces {
            Self::validate_origin(space, &lengths)?;
            Self::validate_tiling(space)?;
            self.validate_retention(space)?;
        }

        if !self.spaces.is_empty() && !lengths.contains_key(&CanonicalSpaceId::source()) {
            return Err(FidelityError::MissingSourceSpace);
        }

        self.validate_acyclic()?;
        Ok(())
    }

    fn validate_origin(
        space: &AddressSpaceLedger,
        lengths: &BTreeMap<CanonicalSpaceId, u64>,
    ) -> Result<(), FidelityError> {
        let is_root = matches!(space.origin, SerializedOrigin::Root);
        if is_root != space.id.is_source() {
            return Err(FidelityError::OriginIdMismatch {
                id: space.id.as_str().to_string(),
            });
        }

        let is_slice = matches!(space.origin, SerializedOrigin::Slice { .. });
        if let SerializedOrigin::Slice { parent, range } = &space.origin {
            Self::check_extent(space, lengths, parent, *range)?;
        }
        for extent in space.origin.referenced() {
            Self::check_extent(space, lengths, &extent.space, extent.range)?;
        }

        // A non-root space must reference at least one parent. Combined with
        // acyclicity this forces every space to reach `source` by following
        // origins; an empty `Concat`/`Transform` would otherwise validate as a
        // second, un-derived root.
        if !is_root && !is_slice && space.origin.referenced().is_empty() {
            return Err(FidelityError::OriginWithoutReference {
                id: space.id.as_str().to_string(),
            });
        }
        Ok(())
    }

    fn check_extent(
        space: &AddressSpaceLedger,
        lengths: &BTreeMap<CanonicalSpaceId, u64>,
        referenced: &CanonicalSpaceId,
        range: SerializedRange,
    ) -> Result<(), FidelityError> {
        let Some(&length) = lengths.get(referenced) else {
            return Err(FidelityError::DanglingReference {
                id: space.id.as_str().to_string(),
                referenced: referenced.as_str().to_string(),
            });
        };
        if !range.within(length) {
            return Err(FidelityError::RangeOutOfBounds {
                id: space.id.as_str().to_string(),
                referenced: referenced.as_str().to_string(),
                start: range.start,
                end: range.end,
                length,
            });
        }
        Ok(())
    }

    fn validate_tiling(space: &AddressSpaceLedger) -> Result<(), FidelityError> {
        let mut cursor = 0_u64;
        for span in &space.spans {
            if span.range.start != cursor || span.range.end < span.range.start {
                return Err(FidelityError::TilingGap {
                    id: space.id.as_str().to_string(),
                    length: space.length,
                    offset: cursor,
                });
            }
            cursor = span.range.end;
        }
        if cursor != space.length {
            return Err(FidelityError::TilingGap {
                id: space.id.as_str().to_string(),
                length: space.length,
                offset: cursor,
            });
        }
        Ok(())
    }

    fn validate_retention(&self, space: &AddressSpaceLedger) -> Result<(), FidelityError> {
        if !self.capability.requires_retained_bytes() {
            return Ok(());
        }
        for span in &space.spans {
            if span.class != SpanClass::Opaque {
                continue;
            }
            let Some(retained) = &span.retained else {
                return Err(FidelityError::MissingRetained {
                    id: space.id.as_str().to_string(),
                    offset: span.range.start,
                });
            };
            if retained.range.len() != span.range.len() {
                return Err(FidelityError::RetainedLengthMismatch {
                    id: space.id.as_str().to_string(),
                    offset: span.range.start,
                    span_len: span.range.len(),
                    retained_len: retained.range.len(),
                });
            }
            let Some(record) = self.retained_record(&retained.blob) else {
                return Err(FidelityError::DanglingRetainedReference {
                    id: space.id.as_str().to_string(),
                    offset: span.range.start,
                    blob: retained.blob.clone(),
                });
            };
            if !retained.range.within(record.byte_len) {
                return Err(FidelityError::RetainedRangeOutOfBounds {
                    id: space.id.as_str().to_string(),
                    offset: span.range.start,
                    blob: retained.blob.clone(),
                    start: retained.range.start,
                    end: retained.range.end,
                    length: record.byte_len,
                });
            }
            let data =
                record
                    .data
                    .as_deref()
                    .ok_or_else(|| FidelityError::MissingRetainedData {
                        id: record.id.clone(),
                    })?;
            let start = usize::try_from(retained.range.start).map_err(|_| {
                FidelityError::RetainedRangeOutOfBounds {
                    id: space.id.as_str().to_string(),
                    offset: span.range.start,
                    blob: retained.blob.clone(),
                    start: retained.range.start,
                    end: retained.range.end,
                    length: record.byte_len,
                }
            })?;
            let end = usize::try_from(retained.range.end).map_err(|_| {
                FidelityError::RetainedRangeOutOfBounds {
                    id: space.id.as_str().to_string(),
                    offset: span.range.start,
                    blob: retained.blob.clone(),
                    start: retained.range.start,
                    end: retained.range.end,
                    length: record.byte_len,
                }
            })?;
            if crate::hash::sha256_hex(&data[start..end]) != span.digest {
                return Err(FidelityError::RetainedSpanDigestMismatch {
                    id: space.id.as_str().to_string(),
                    offset: span.range.start,
                });
            }
        }
        Ok(())
    }

    /// Detects an origin cycle with an iterative depth-first colouring.
    fn validate_acyclic(&self) -> Result<(), FidelityError> {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            Unvisited,
            OnStack,
            Done,
        }
        let mut marks: Vec<Mark> = vec![Mark::Unvisited; self.spaces.len()];

        let index: BTreeMap<&CanonicalSpaceId, usize> = self
            .spaces
            .iter()
            .enumerate()
            .map(|(i, space)| (&space.id, i))
            .collect();
        let targets: Vec<Vec<usize>> = self
            .spaces
            .iter()
            .map(|space| {
                let mut edges = Vec::new();
                if let SerializedOrigin::Slice { parent, .. } = &space.origin {
                    if let Some(&i) = index.get(parent) {
                        edges.push(i);
                    }
                }
                for extent in space.origin.referenced() {
                    if let Some(&i) = index.get(&extent.space) {
                        edges.push(i);
                    }
                }
                edges
            })
            .collect();

        for root in 0..self.spaces.len() {
            if marks[root] != Mark::Unvisited {
                continue;
            }
            let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
            marks[root] = Mark::OnStack;
            while let Some(&(node, cursor)) = stack.last() {
                let node_targets = &targets[node];
                if cursor < node_targets.len() {
                    stack.last_mut().expect("stack is non-empty").1 += 1;
                    let child = node_targets[cursor];
                    match marks[child] {
                        Mark::OnStack => {
                            return Err(FidelityError::OriginCycle {
                                id: self.spaces[child].id.as_str().to_string(),
                            });
                        }
                        Mark::Done => {}
                        Mark::Unvisited => {
                            marks[child] = Mark::OnStack;
                            stack.push((child, 0));
                        }
                    }
                } else {
                    marks[node] = Mark::Done;
                    stack.pop();
                }
            }
        }
        Ok(())
    }

    /// Serializes to versioned canonical JSON.
    ///
    /// Call [`SourceFidelity::new`] or [`SourceFidelity::canonicalize`] first so
    /// spaces and spans are in canonical order; the output is then byte-stable
    /// across repeat decodes.
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parses a v2 sidecar, rejecting any other declared version.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(s)?;
        let version = value.get("version").and_then(serde_json::Value::as_str);
        if version != Some(SOURCE_FIDELITY_VERSION) {
            return Err(<serde_json::Error as serde::de::Error>::custom(format!(
                "expected source-fidelity version {SOURCE_FIDELITY_VERSION}, found {version:?}"
            )));
        }
        serde_json::from_value(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u64, end: u64, class: SpanClass) -> LedgerSpan {
        LedgerSpan {
            range: SerializedRange { start, end },
            class,
            owner: "owner".to_string(),
            meaning: "meaning".to_string(),
            digest: "00".to_string(),
            retained: None,
        }
    }

    fn root_space(length: u64, spans: Vec<LedgerSpan>) -> AddressSpaceLedger {
        AddressSpaceLedger {
            id: CanonicalSpaceId::source(),
            length,
            origin: SerializedOrigin::Root,
            spans,
        }
    }

    #[test]
    fn canonical_ids_have_stable_spellings() {
        assert_eq!(CanonicalSpaceId::source().as_str(), "source");
        assert_eq!(
            CanonicalSpaceId::entry("Document.xml").as_str(),
            "entry:Document.xml"
        );
        assert_eq!(
            CanonicalSpaceId::stream("part", 3).as_str(),
            "stream:part#3"
        );
    }

    #[test]
    fn canonicalize_orders_spaces_and_spans_independent_of_input_order() {
        let source = root_space(
            10,
            vec![span(5, 10, SpanClass::Opaque), span(0, 5, SpanClass::Typed)],
        );
        let entry = AddressSpaceLedger {
            id: CanonicalSpaceId::entry("a"),
            length: 4,
            origin: SerializedOrigin::Slice {
                parent: CanonicalSpaceId::source(),
                range: SerializedRange { start: 0, end: 4 },
            },
            spans: vec![span(0, 4, SpanClass::Opaque)],
        };
        let a = SourceFidelity::new(
            LedgerCapability::Accounted,
            vec![entry.clone(), source.clone()],
        );
        let b = SourceFidelity::new(LedgerCapability::Accounted, vec![source, entry]);
        assert_eq!(a, b);
        assert_eq!(a.spaces[0].id, CanonicalSpaceId::entry("a"));
        assert_eq!(a.spaces[1].id, CanonicalSpaceId::source());
        assert_eq!(a.spaces[1].spans[0].range.start, 0);
    }

    #[test]
    fn validate_accepts_a_tiled_dag() {
        let sidecar = SourceFidelity::new(
            LedgerCapability::Accounted,
            vec![
                root_space(8, vec![span(0, 8, SpanClass::Opaque)]),
                AddressSpaceLedger {
                    id: CanonicalSpaceId::stream("s", 0),
                    length: 3,
                    origin: SerializedOrigin::Transform {
                        inputs: vec![SpaceExtent {
                            space: CanonicalSpaceId::source(),
                            range: SerializedRange { start: 0, end: 8 },
                        }],
                        transform: SerializedTransformKind::Decompress,
                    },
                    spans: vec![span(0, 3, SpanClass::Typed)],
                },
            ],
        );
        assert_eq!(sidecar.validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_tiling_gap() {
        let sidecar = SourceFidelity::new(
            LedgerCapability::Accounted,
            vec![root_space(10, vec![span(0, 4, SpanClass::Opaque)])],
        );
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::TilingGap { .. })
        ));
    }

    #[test]
    fn validate_rejects_dangling_reference() {
        let sidecar = SourceFidelity {
            version: SOURCE_FIDELITY_VERSION.to_string(),
            capability: LedgerCapability::Accounted,
            spaces: vec![AddressSpaceLedger {
                id: CanonicalSpaceId::entry("a"),
                length: 2,
                origin: SerializedOrigin::Slice {
                    parent: CanonicalSpaceId::source(),
                    range: SerializedRange { start: 0, end: 2 },
                },
                spans: vec![span(0, 2, SpanClass::Opaque)],
            }],
            ..SourceFidelity::default()
        };
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::DanglingReference { .. })
        ));
    }

    #[test]
    fn validate_rejects_out_of_bounds_range() {
        let sidecar = SourceFidelity::new(
            LedgerCapability::Accounted,
            vec![
                root_space(4, vec![span(0, 4, SpanClass::Opaque)]),
                AddressSpaceLedger {
                    id: CanonicalSpaceId::entry("a"),
                    length: 2,
                    origin: SerializedOrigin::Slice {
                        parent: CanonicalSpaceId::source(),
                        range: SerializedRange { start: 0, end: 99 },
                    },
                    spans: vec![span(0, 2, SpanClass::Opaque)],
                },
            ],
        );
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::RangeOutOfBounds { .. })
        ));
    }

    #[test]
    fn validate_rejects_origin_cycle() {
        let sidecar = SourceFidelity {
            version: SOURCE_FIDELITY_VERSION.to_string(),
            capability: LedgerCapability::Accounted,
            spaces: vec![
                root_space(4, vec![span(0, 4, SpanClass::Opaque)]),
                AddressSpaceLedger {
                    id: CanonicalSpaceId::stream("x", 0),
                    length: 4,
                    origin: SerializedOrigin::Slice {
                        parent: CanonicalSpaceId::stream("x", 1),
                        range: SerializedRange { start: 0, end: 4 },
                    },
                    spans: vec![span(0, 4, SpanClass::Opaque)],
                },
                AddressSpaceLedger {
                    id: CanonicalSpaceId::stream("x", 1),
                    length: 4,
                    origin: SerializedOrigin::Slice {
                        parent: CanonicalSpaceId::stream("x", 0),
                        range: SerializedRange { start: 0, end: 4 },
                    },
                    spans: vec![span(0, 4, SpanClass::Opaque)],
                },
            ],
            ..SourceFidelity::default()
        };
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::OriginCycle { .. })
        ));
    }

    #[test]
    fn validate_rejects_origin_id_mismatch() {
        let sidecar = SourceFidelity {
            version: SOURCE_FIDELITY_VERSION.to_string(),
            capability: LedgerCapability::Accounted,
            spaces: vec![AddressSpaceLedger {
                id: CanonicalSpaceId::entry("a"),
                length: 4,
                origin: SerializedOrigin::Root,
                spans: vec![span(0, 4, SpanClass::Opaque)],
            }],
            ..SourceFidelity::default()
        };
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::OriginIdMismatch { .. })
        ));
    }

    #[test]
    fn validate_rejects_recoverable_without_retained_bytes() {
        let sidecar = SourceFidelity::new(
            LedgerCapability::Recoverable,
            vec![root_space(4, vec![span(0, 4, SpanClass::Opaque)])],
        );
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::MissingRetained { .. })
        ));
    }

    #[test]
    fn validate_rejects_recoverable_with_short_retained_range() {
        let sidecar = SourceFidelity::new(
            LedgerCapability::Recoverable,
            vec![root_space(
                4,
                vec![LedgerSpan {
                    range: SerializedRange { start: 0, end: 4 },
                    class: SpanClass::Opaque,
                    owner: "owner".to_string(),
                    meaning: "meaning".to_string(),
                    digest: "00".to_string(),
                    retained: Some(RetainedRef {
                        blob: "b".to_string(),
                        range: SerializedRange { start: 0, end: 0 },
                    }),
                }],
            )],
        );
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::RetainedLengthMismatch {
                span_len: 4,
                retained_len: 0,
                ..
            })
        ));
    }

    fn recoverable_sidecar(data: Vec<u8>) -> SourceFidelity {
        let digest = crate::hash::sha256_hex(&data);
        let mut sidecar = SourceFidelity::new(
            LedgerCapability::Recoverable,
            vec![root_space(
                4,
                vec![LedgerSpan {
                    range: SerializedRange { start: 0, end: 4 },
                    class: SpanClass::Opaque,
                    owner: "owner".to_string(),
                    meaning: "meaning".to_string(),
                    digest: digest.clone(),
                    retained: Some(RetainedRef {
                        blob: "b".to_string(),
                        range: SerializedRange { start: 0, end: 4 },
                    }),
                }],
            )],
        );
        sidecar.retained_records.push(RetainedSourceRecord {
            id: "b".to_string(),
            stream: "source".to_string(),
            offset: 0,
            byte_len: 4,
            sha256: digest,
            data: Some(data),
        });
        sidecar
    }

    #[test]
    fn validate_recoverable_proves_span_bytes_can_be_recovered() {
        assert_eq!(recoverable_sidecar(vec![1, 2, 3, 4]).validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_recoverable_record_with_false_digest() {
        let mut sidecar = recoverable_sidecar(vec![1, 2, 3, 4]);
        sidecar.retained_records[0].sha256 = crate::hash::sha256_hex(&[4, 3, 2, 1]);
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::RetainedRecordDigestMismatch { .. })
        ));
    }

    #[test]
    fn validate_rejects_span_that_disagrees_with_retained_subrange() {
        let mut sidecar = recoverable_sidecar(vec![1, 2, 3, 4]);
        sidecar.spaces[0].spans[0].digest = crate::hash::sha256_hex(&[4, 3, 2, 1]);
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::RetainedSpanDigestMismatch { .. })
        ));
    }

    #[test]
    fn validate_rejects_non_root_without_reference() {
        let sidecar = SourceFidelity::new(
            LedgerCapability::Accounted,
            vec![
                root_space(4, vec![span(0, 4, SpanClass::Opaque)]),
                AddressSpaceLedger {
                    id: CanonicalSpaceId::entry("x"),
                    length: 4,
                    origin: SerializedOrigin::Concat { segments: vec![] },
                    spans: vec![span(0, 4, SpanClass::Opaque)],
                },
            ],
        );
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::OriginWithoutReference { .. })
        ));
    }

    #[test]
    fn canonical_json_round_trips() {
        let sidecar = SourceFidelity::new(
            LedgerCapability::Accounted,
            vec![root_space(4, vec![span(0, 4, SpanClass::Typed)])],
        );
        let json = sidecar.to_canonical_json().expect("serialize");
        let parsed = SourceFidelity::from_json(&json).expect("parse");
        assert_eq!(sidecar, parsed);
        assert_eq!(json, parsed.to_canonical_json().expect("reserialize"));
    }

    #[test]
    fn from_json_rejects_wrong_version() {
        let sidecar = SourceFidelity::new(LedgerCapability::Accounted, vec![root_space(0, vec![])]);
        let mut value: serde_json::Value =
            serde_json::from_str(&sidecar.to_canonical_json().expect("serialize"))
                .expect("parse value");
        value["version"] = serde_json::Value::String("9".to_string());
        let json = serde_json::to_string(&value).expect("reserialize");
        assert!(SourceFidelity::from_json(&json).is_err());
    }
}
