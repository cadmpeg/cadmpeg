// SPDX-License-Identifier: Apache-2.0
//! Serialized multi-space source-fidelity ledger (sidecar schema v2).
//!
//! The runtime space registry (`decode::space`) is the skeleton of this
//! schema: it names spaces by a registration ordinal that is never serialized.
//! This module fixes the *serialized* form. Every space carries a stable
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
//! The schema carries the accounting-vs-recovery distinction: a
//! [`LedgerLevel`] states how completely the ledger tiles, and a
//! [`LedgerCapability`] states whether every opaque span additionally resolves
//! to retained bytes. The two are orthogonal — retention can be refused while
//! classification still succeeds.
//!
//! [`SourceFidelity`] is the sole current writer. The prior single-stream form
//! is preserved as [`v1::ByteLedgerV1`] with [`migrate_v1`] and the
//! version-detecting [`SourceFidelity::from_json_any`] loader, so a sidecar
//! written before the generalization still reads.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Serialized schema version written into every v2 sidecar.
pub const SOURCE_FIDELITY_VERSION: &str = "2";

/// How completely a ledger tiles its spaces.
///
/// Levels only ratchet up, and every level above [`LedgerLevel::L0`] tiles
/// completely: a partial ledger violates the conservation invariant and is not
/// a level.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LedgerLevel {
    /// No ledger.
    L0,
    /// Complete coarse tiling: container framing structural, every unrefined
    /// payload one opaque span.
    L1,
    /// Complete refined tiling at record or field granularity.
    L2,
}

/// Whether opaque spans additionally resolve to retained bytes.
///
/// Orthogonal to [`LedgerLevel`]: `max_retained_bytes` can refuse the retention
/// recovery needs while classification still succeeds.
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

/// Byte classification for one span, unchanged from the single-stream schema.
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
/// Replaces the single-stream one-record-per-span rule: one blob may back many
/// opaque spans through distinct subranges.
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
}

/// A complete serialized source-fidelity sidecar (schema v2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceFidelity {
    /// The schema version; always [`SOURCE_FIDELITY_VERSION`] when written.
    pub version: String,
    /// How completely the sidecar tiles its spaces.
    pub level: LedgerLevel,
    /// Whether opaque spans resolve to retained bytes.
    pub capability: LedgerCapability,
    /// The address spaces, in canonical (id-ascending) order.
    pub spaces: Vec<AddressSpaceLedger>,
}

impl SourceFidelity {
    /// Builds a sidecar and puts it in canonical order.
    ///
    /// Sorts spaces by canonical id and each space's spans by ascending start.
    /// Ordering is independent of the order spaces were supplied, so repeat
    /// decodes serialize identically. Call [`SourceFidelity::validate`] to
    /// enforce the conservation invariant.
    pub fn new(
        level: LedgerLevel,
        capability: LedgerCapability,
        spaces: Vec<AddressSpaceLedger>,
    ) -> Self {
        let mut sidecar = SourceFidelity {
            version: SOURCE_FIDELITY_VERSION.to_string(),
            level,
            capability,
            spaces,
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
    }

    /// Validates the conservation invariant.
    ///
    /// Checks the schema version, unique ids, origin/id consistency, that every
    /// origin references only present spaces and in-bounds ranges, that the
    /// origin graph is acyclic, that each space's spans tile `[0, length)`
    /// exactly, and — for a recoverable ledger — that every opaque span
    /// resolves to retained bytes.
    pub fn validate(&self) -> Result<(), FidelityError> {
        if self.version != SOURCE_FIDELITY_VERSION {
            return Err(FidelityError::Version {
                found: self.version.clone(),
            });
        }

        let mut ids = BTreeSet::new();
        for space in &self.spaces {
            if !ids.insert(space.id.clone()) {
                return Err(FidelityError::DuplicateSpace {
                    id: space.id.as_str().to_string(),
                });
            }
        }

        for space in &self.spaces {
            self.validate_origin(space, &ids)?;
            Self::validate_tiling(space)?;
            self.validate_retention(space)?;
        }

        self.validate_acyclic()?;
        Ok(())
    }

    fn validate_origin(
        &self,
        space: &AddressSpaceLedger,
        ids: &BTreeSet<CanonicalSpaceId>,
    ) -> Result<(), FidelityError> {
        let is_root = matches!(space.origin, SerializedOrigin::Root);
        if is_root != space.id.is_source() {
            return Err(FidelityError::OriginIdMismatch {
                id: space.id.as_str().to_string(),
            });
        }

        // Slice carries its own referenced range that `referenced()` omits;
        // handle it explicitly alongside the extent-bearing variants.
        if let SerializedOrigin::Slice { parent, range } = &space.origin {
            self.check_extent(space, ids, parent, *range)?;
        }
        for extent in space.origin.referenced() {
            self.check_extent(space, ids, &extent.space, extent.range)?;
        }
        Ok(())
    }

    fn check_extent(
        &self,
        space: &AddressSpaceLedger,
        ids: &BTreeSet<CanonicalSpaceId>,
        referenced: &CanonicalSpaceId,
        range: SerializedRange,
    ) -> Result<(), FidelityError> {
        if !ids.contains(referenced) {
            return Err(FidelityError::DanglingReference {
                id: space.id.as_str().to_string(),
                referenced: referenced.as_str().to_string(),
            });
        }
        let length = self
            .spaces
            .iter()
            .find(|candidate| &candidate.id == referenced)
            .map_or(0, |candidate| candidate.length);
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
            if span.class == SpanClass::Opaque && span.retained.is_none() {
                return Err(FidelityError::MissingRetained {
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
        let index_of = |id: &CanonicalSpaceId| self.spaces.iter().position(|space| &space.id == id);

        for root in 0..self.spaces.len() {
            if marks[root] != Mark::Unvisited {
                continue;
            }
            // (node, next-child-cursor) frames simulate recursion.
            let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
            marks[root] = Mark::OnStack;
            while let Some(&(node, cursor)) = stack.last() {
                let targets = self.origin_targets(node);
                if cursor < targets.len() {
                    stack.last_mut().expect("stack is non-empty").1 += 1;
                    let Some(child) = index_of(&targets[cursor]) else {
                        continue;
                    };
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

    fn origin_targets(&self, node: usize) -> Vec<CanonicalSpaceId> {
        let space = &self.spaces[node];
        let mut targets = Vec::new();
        if let SerializedOrigin::Slice { parent, .. } = &space.origin {
            targets.push(parent.clone());
        }
        for extent in space.origin.referenced() {
            targets.push(extent.space.clone());
        }
        targets
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

    /// Parses a sidecar of any supported version, migrating v1 forward.
    ///
    /// A v1 sidecar migrates to v2 through [`migrate_v1`], preserving its level
    /// and capability. The result is canonical but not validated; call
    /// [`SourceFidelity::validate`] to enforce the invariant.
    pub fn from_json_any(
        s: &str,
        v1_level: LedgerLevel,
        v1_capability: LedgerCapability,
    ) -> Result<Self, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(s)?;
        let version = value.get("version").and_then(serde_json::Value::as_str);
        match version {
            Some(v) if v == SOURCE_FIDELITY_VERSION => serde_json::from_value(value),
            Some(v) if v == v1::SOURCE_FIDELITY_VERSION_V1 => {
                let ledger: v1::ByteLedgerV1 = serde_json::from_value(value)?;
                Ok(migrate_v1(ledger, v1_level, v1_capability))
            }
            other => Err(<serde_json::Error as serde::de::Error>::custom(format!(
                "unsupported source-fidelity version: {other:?}"
            ))),
        }
    }
}

/// Migrates a single-stream v1 ledger to a v2 sidecar.
///
/// The v1 stream becomes the `source` space with a [`SerializedOrigin::Root`]
/// origin. Each v1 span keeps its range, class, and digest; a v1 retained
/// record id becomes a full-extent [`RetainedRef`], matching the v1
/// one-record-per-span containment.
pub fn migrate_v1(
    ledger: v1::ByteLedgerV1,
    level: LedgerLevel,
    capability: LedgerCapability,
) -> SourceFidelity {
    let spans = ledger
        .spans
        .into_iter()
        .map(|span| {
            let retained = span.retained_record.map(|blob| RetainedRef {
                range: SerializedRange {
                    start: 0,
                    end: span.range.len(),
                },
                blob,
            });
            LedgerSpan {
                range: span.range,
                class: span.class,
                owner: span.owner,
                meaning: span.meaning,
                digest: span.digest,
                retained,
            }
        })
        .collect();
    let space = AddressSpaceLedger {
        id: CanonicalSpaceId::source(),
        length: ledger.source_length,
        origin: SerializedOrigin::Root,
        spans,
    };
    SourceFidelity::new(level, capability, vec![space])
}

/// The prior single-stream sidecar schema, retained for migration.
pub mod v1 {
    use super::{SerializedRange, SpanClass};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    /// The v1 schema version string.
    pub const SOURCE_FIDELITY_VERSION_V1: &str = "1";

    /// One span of the single flat `source` stream.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
    pub struct ByteSpanV1 {
        /// The span's range within the source stream.
        pub range: SerializedRange,
        /// The span's byte classification.
        pub class: SpanClass,
        /// The producing subsystem or record family.
        pub owner: String,
        /// A human-readable meaning for the span.
        pub meaning: String,
        /// Lowercase hexadecimal SHA-256 digest of the span's bytes.
        pub digest: String,
        /// A retained record id backing an opaque span, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub retained_record: Option<String>,
    }

    /// The v1 flat-stream ledger.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
    pub struct ByteLedgerV1 {
        /// The schema version; always [`SOURCE_FIDELITY_VERSION_V1`].
        #[serde(default = "default_v1_version")]
        pub version: String,
        /// The source stream's total length.
        pub source_length: u64,
        /// The spans tiling the source stream.
        pub spans: Vec<ByteSpanV1>,
    }

    fn default_v1_version() -> String {
        SOURCE_FIDELITY_VERSION_V1.to_string()
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
            LedgerLevel::L2,
            LedgerCapability::Accounted,
            vec![entry.clone(), source.clone()],
        );
        let b = SourceFidelity::new(
            LedgerLevel::L2,
            LedgerCapability::Accounted,
            vec![source, entry],
        );
        assert_eq!(a, b);
        assert_eq!(a.spaces[0].id, CanonicalSpaceId::entry("a"));
        assert_eq!(a.spaces[1].id, CanonicalSpaceId::source());
        assert_eq!(a.spaces[1].spans[0].range.start, 0);
    }

    #[test]
    fn validate_accepts_a_tiled_dag() {
        let sidecar = SourceFidelity::new(
            LedgerLevel::L2,
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
            LedgerLevel::L1,
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
            level: LedgerLevel::L1,
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
        };
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::DanglingReference { .. })
        ));
    }

    #[test]
    fn validate_rejects_out_of_bounds_range() {
        let sidecar = SourceFidelity::new(
            LedgerLevel::L1,
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
        // Two non-root spaces reference each other; neither is `source`, and
        // both carry non-Root origins so the id/origin check passes first.
        let sidecar = SourceFidelity {
            version: SOURCE_FIDELITY_VERSION.to_string(),
            level: LedgerLevel::L1,
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
            level: LedgerLevel::L1,
            capability: LedgerCapability::Accounted,
            spaces: vec![AddressSpaceLedger {
                id: CanonicalSpaceId::entry("a"),
                length: 4,
                origin: SerializedOrigin::Root,
                spans: vec![span(0, 4, SpanClass::Opaque)],
            }],
        };
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::OriginIdMismatch { .. })
        ));
    }

    #[test]
    fn validate_rejects_recoverable_without_retained_bytes() {
        let sidecar = SourceFidelity::new(
            LedgerLevel::L2,
            LedgerCapability::Recoverable,
            vec![root_space(4, vec![span(0, 4, SpanClass::Opaque)])],
        );
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::MissingRetained { .. })
        ));
    }

    #[test]
    fn canonical_json_round_trips() {
        let sidecar = SourceFidelity::new(
            LedgerLevel::L2,
            LedgerCapability::Accounted,
            vec![root_space(4, vec![span(0, 4, SpanClass::Typed)])],
        );
        let json = sidecar.to_canonical_json().expect("serialize");
        let parsed = SourceFidelity::from_json(&json).expect("parse");
        assert_eq!(sidecar, parsed);
        // Re-serialization is byte-stable.
        assert_eq!(json, parsed.to_canonical_json().expect("reserialize"));
    }

    #[test]
    fn from_json_rejects_wrong_version() {
        let sidecar = SourceFidelity::new(
            LedgerLevel::L0,
            LedgerCapability::Accounted,
            vec![root_space(0, vec![])],
        );
        let mut value: serde_json::Value =
            serde_json::from_str(&sidecar.to_canonical_json().expect("serialize"))
                .expect("parse value");
        value["version"] = serde_json::Value::String("9".to_string());
        let json = serde_json::to_string(&value).expect("reserialize");
        assert!(SourceFidelity::from_json(&json).is_err());
    }

    #[test]
    fn migrates_v1_to_v2() {
        let v1 = v1::ByteLedgerV1 {
            version: v1::SOURCE_FIDELITY_VERSION_V1.to_string(),
            source_length: 8,
            spans: vec![
                v1::ByteSpanV1 {
                    range: SerializedRange { start: 0, end: 4 },
                    class: SpanClass::Structural,
                    owner: "framing".to_string(),
                    meaning: "header".to_string(),
                    digest: "aa".to_string(),
                    retained_record: None,
                },
                v1::ByteSpanV1 {
                    range: SerializedRange { start: 4, end: 8 },
                    class: SpanClass::Opaque,
                    owner: "body".to_string(),
                    meaning: "payload".to_string(),
                    digest: "bb".to_string(),
                    retained_record: Some("rec#1".to_string()),
                },
            ],
        };
        let migrated = migrate_v1(v1, LedgerLevel::L2, LedgerCapability::Recoverable);
        assert_eq!(migrated.version, SOURCE_FIDELITY_VERSION);
        assert_eq!(migrated.spaces.len(), 1);
        let source = &migrated.spaces[0];
        assert_eq!(source.id, CanonicalSpaceId::source());
        assert_eq!(source.length, 8);
        let retained = source.spans[1].retained.as_ref().expect("retained");
        assert_eq!(retained.blob, "rec#1");
        assert_eq!(retained.range, SerializedRange { start: 0, end: 4 });
        assert_eq!(migrated.validate(), Ok(()));
    }

    #[test]
    fn from_json_any_migrates_v1_document() {
        let v1 = v1::ByteLedgerV1 {
            version: v1::SOURCE_FIDELITY_VERSION_V1.to_string(),
            source_length: 4,
            spans: vec![v1::ByteSpanV1 {
                range: SerializedRange { start: 0, end: 4 },
                class: SpanClass::Typed,
                owner: "o".to_string(),
                meaning: "m".to_string(),
                digest: "cc".to_string(),
                retained_record: None,
            }],
        };
        let json = serde_json::to_string(&v1).expect("serialize v1");
        let loaded =
            SourceFidelity::from_json_any(&json, LedgerLevel::L2, LedgerCapability::Accounted)
                .expect("migrate");
        assert_eq!(loaded.version, SOURCE_FIDELITY_VERSION);
        assert_eq!(loaded.validate(), Ok(()));
    }
}
