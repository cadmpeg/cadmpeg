// SPDX-License-Identifier: Apache-2.0
//! Interpreted deltas between two source-fidelity sidecars.
//!
//! [`diff_source_fidelity`] compares two [`SourceFidelity`] sidecars and reports
//! what changed in conservation terms — level and capability transitions, spaces
//! that appeared or disappeared, and per-space byte movement between the
//! [`SpanClass`] categories — rather than dumping raw spans. A space that exists
//! in both sidecars is summarized by its per-class byte totals, its span count,
//! and whether the ordered digest of its spans changed, so a caller learns
//! *that* a space's bytes moved between classification categories or changed
//! content without walking every tile.

use crate::hash::sha256_hex;
use crate::source_fidelity::{
    AddressSpaceLedger, LedgerCapability, LedgerLevel, SourceFidelity, SpanClass,
};

/// Per-class byte totals for one space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct ClassBytes {
    /// Bytes classified [`SpanClass::Typed`].
    pub typed: u64,
    /// Bytes classified [`SpanClass::Structural`].
    pub structural: u64,
    /// Bytes classified [`SpanClass::Opaque`].
    pub opaque: u64,
}

impl ClassBytes {
    fn of(space: &AddressSpaceLedger) -> Self {
        let mut totals = ClassBytes::default();
        for span in &space.spans {
            let len = span.range.len();
            match span.class {
                SpanClass::Typed => totals.typed = totals.typed.saturating_add(len),
                SpanClass::Structural => {
                    totals.structural = totals.structural.saturating_add(len);
                }
                SpanClass::Opaque => totals.opaque = totals.opaque.saturating_add(len),
            }
        }
        totals
    }
}

/// The interpreted delta for one space present in both sidecars.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SpaceDelta {
    /// The space's canonical id.
    pub id: String,
    /// Total byte length before and after, when it changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<(u64, u64)>,
    /// Per-class byte totals before.
    pub class_bytes_before: ClassBytes,
    /// Per-class byte totals after.
    pub class_bytes_after: ClassBytes,
    /// Span count before and after.
    pub spans: (usize, usize),
    /// Whether the ordered digest of the space's spans changed.
    pub content_changed: bool,
}

impl SpaceDelta {
    /// Returns whether this space is materially unchanged.
    pub fn is_empty(&self) -> bool {
        self.length.is_none()
            && self.class_bytes_before == self.class_bytes_after
            && self.spans.0 == self.spans.1
            && !self.content_changed
    }
}

/// The interpreted delta between two source-fidelity sidecars.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct FidelityDiff {
    /// Ledger-level transition, when it changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<(LedgerLevel, LedgerLevel)>,
    /// Capability transition, when it changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<(LedgerCapability, LedgerCapability)>,
    /// Canonical ids of spaces present only in the right sidecar.
    pub added_spaces: Vec<String>,
    /// Canonical ids of spaces present only in the left sidecar.
    pub removed_spaces: Vec<String>,
    /// Interpreted deltas for spaces present in both, materially changed.
    pub changed_spaces: Vec<SpaceDelta>,
}

impl FidelityDiff {
    /// Returns whether the two sidecars are materially identical.
    pub fn is_empty(&self) -> bool {
        self.level.is_none()
            && self.capability.is_none()
            && self.added_spaces.is_empty()
            && self.removed_spaces.is_empty()
            && self.changed_spaces.is_empty()
    }
}

/// Digest a space's ordered span digests into one content fingerprint.
///
/// Two spaces with the same fingerprint carry byte-identical, identically
/// classified content in the same order; a difference means the space's bytes or
/// their classification changed.
fn content_fingerprint(space: &AddressSpaceLedger) -> String {
    let mut acc = String::new();
    for span in &space.spans {
        acc.push_str(match span.class {
            SpanClass::Typed => "t",
            SpanClass::Structural => "s",
            SpanClass::Opaque => "o",
        });
        acc.push(':');
        acc.push_str(&span.digest);
        acc.push('\n');
    }
    sha256_hex(acc.as_bytes())
}

/// Compare two source-fidelity sidecars into an interpreted delta.
///
/// Spaces are matched by canonical id. Both sidecars are assumed canonical (as
/// produced by [`SourceFidelity::new`]); the result's space lists preserve that
/// canonical order.
pub fn diff_source_fidelity(left: &SourceFidelity, right: &SourceFidelity) -> FidelityDiff {
    let level = (left.level != right.level).then_some((left.level, right.level));
    let capability =
        (left.capability != right.capability).then_some((left.capability, right.capability));

    let mut added_spaces = Vec::new();
    let mut removed_spaces = Vec::new();
    let mut changed_spaces = Vec::new();

    for right_space in &right.spaces {
        if !left.spaces.iter().any(|s| s.id == right_space.id) {
            added_spaces.push(right_space.id.as_str().to_string());
        }
    }
    for left_space in &left.spaces {
        let Some(right_space) = right.spaces.iter().find(|s| s.id == left_space.id) else {
            removed_spaces.push(left_space.id.as_str().to_string());
            continue;
        };
        let delta = SpaceDelta {
            id: left_space.id.as_str().to_string(),
            length: (left_space.length != right_space.length)
                .then_some((left_space.length, right_space.length)),
            class_bytes_before: ClassBytes::of(left_space),
            class_bytes_after: ClassBytes::of(right_space),
            spans: (left_space.spans.len(), right_space.spans.len()),
            content_changed: content_fingerprint(left_space) != content_fingerprint(right_space),
        };
        if !delta.is_empty() {
            changed_spaces.push(delta);
        }
    }

    FidelityDiff {
        level,
        capability,
        added_spaces,
        removed_spaces,
        changed_spaces,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_fidelity::{
        AddressSpaceLedger, CanonicalSpaceId, LedgerSpan, SerializedOrigin, SerializedRange,
    };

    fn span(start: u64, end: u64, class: SpanClass, digest: &str) -> LedgerSpan {
        LedgerSpan {
            range: SerializedRange { start, end },
            class,
            owner: "o".to_string(),
            meaning: "m".to_string(),
            digest: digest.to_string(),
            retained: None,
        }
    }

    fn source(spans: Vec<LedgerSpan>, length: u64) -> SourceFidelity {
        SourceFidelity::new(
            LedgerLevel::L2,
            LedgerCapability::Accounted,
            vec![AddressSpaceLedger {
                id: CanonicalSpaceId::source(),
                length,
                origin: SerializedOrigin::Root,
                spans,
            }],
        )
    }

    #[test]
    fn identical_sidecars_have_an_empty_diff() {
        let a = source(vec![span(0, 4, SpanClass::Typed, "aa")], 4);
        let b = source(vec![span(0, 4, SpanClass::Typed, "aa")], 4);
        assert!(diff_source_fidelity(&a, &b).is_empty());
    }

    #[test]
    fn reclassification_moves_bytes_between_categories() {
        let a = source(vec![span(0, 4, SpanClass::Opaque, "aa")], 4);
        let b = source(vec![span(0, 4, SpanClass::Typed, "aa")], 4);
        let diff = diff_source_fidelity(&a, &b);
        assert!(!diff.is_empty());
        let delta = &diff.changed_spaces[0];
        assert_eq!(delta.class_bytes_before.opaque, 4);
        assert_eq!(delta.class_bytes_after.typed, 4);
        assert!(delta.content_changed);
    }

    #[test]
    fn added_and_removed_spaces_are_named() {
        let a = source(vec![span(0, 4, SpanClass::Typed, "aa")], 4);
        let mut b = source(vec![span(0, 4, SpanClass::Typed, "aa")], 4);
        b.spaces.push(AddressSpaceLedger {
            id: CanonicalSpaceId::stream("s", 0),
            length: 2,
            origin: SerializedOrigin::Transform {
                inputs: vec![crate::source_fidelity::SpaceExtent {
                    space: CanonicalSpaceId::source(),
                    range: SerializedRange { start: 0, end: 4 },
                }],
                transform: crate::source_fidelity::SerializedTransformKind::Decompress,
            },
            spans: vec![span(0, 2, SpanClass::Opaque, "bb")],
        });
        b.canonicalize();
        let diff = diff_source_fidelity(&a, &b);
        assert_eq!(diff.added_spaces, vec!["stream:s#0".to_string()]);
        assert!(diff.removed_spaces.is_empty());
    }

    #[test]
    fn level_transition_is_reported() {
        let a = SourceFidelity::new(
            LedgerLevel::L1,
            LedgerCapability::Accounted,
            vec![AddressSpaceLedger {
                id: CanonicalSpaceId::source(),
                length: 4,
                origin: SerializedOrigin::Root,
                spans: vec![span(0, 4, SpanClass::Opaque, "aa")],
            }],
        );
        let b = source(vec![span(0, 4, SpanClass::Opaque, "aa")], 4);
        let diff = diff_source_fidelity(&a, &b);
        assert_eq!(diff.level, Some((LedgerLevel::L1, LedgerLevel::L2)));
    }
}
