// SPDX-License-Identifier: Apache-2.0
//! Turning a fixture and its boundaries into truncation and mutation cases.
//!
//! Two case families:
//!
//! - **Truncation**: at every recognized boundary offset minus one, at it, and
//!   plus one; at a fixed set of stratified fractions of the length; and, only
//!   when the fixture is below a size threshold, at every byte.
//! - **Mutation spot-checks**: single-byte flips at header and count positions.
//!
//! Each case carries a stable label so a sweep failure names the exact
//! transformation that produced it.

use crate::boundary::{Boundary, BoundaryKind, BoundaryProvider};

/// Below this length a fixture gets an every-byte truncation sweep; above it,
/// only boundary and stratified truncations. Keeps the sweep finite on large
/// inputs while staying exhaustive on small ones.
pub const EVERY_BYTE_THRESHOLD: usize = 512;

/// Stratified truncation points as fractions of the fixture length.
const STRATA: &[f64] = &[0.10, 0.25, 0.50, 0.75, 0.90];

/// How a case was derived from its fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseKind {
    /// Kept the first `len` bytes.
    Truncation {
        /// Retained prefix length.
        len: usize,
    },
    /// Replaced one byte.
    Mutation {
        /// Mutated offset.
        offset: usize,
        /// Original byte.
        from: u8,
        /// Replacement byte.
        to: u8,
    },
}

/// One derived input plus the label that names its transformation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepCase {
    /// Stable label, e.g. `trunc@123` or `flip^ff@8`.
    pub label: String,
    /// How the case was derived.
    pub kind: CaseKind,
    /// The transformed bytes.
    pub bytes: Vec<u8>,
}

/// The single-byte replacements applied at each header/count position.
///
/// XOR `0xff`, `+1`, and `0x00` between them flip the high bit, perturb a low
/// bit, and zero a field — the mutations a length/count parser is most
/// sensitive to.
fn mutations(original: u8) -> [u8; 3] {
    [original ^ 0xff, original.wrapping_add(1), 0x00]
}

/// A short mnemonic for a mutation, for the case label.
fn mutation_tag(original: u8, replacement: u8) -> &'static str {
    if replacement == original ^ 0xff {
        "^ff"
    } else if replacement == original.wrapping_add(1) {
        "+1"
    } else {
        "=0"
    }
}

/// Generate the truncation cases for `bytes` given its `boundaries`.
pub fn truncation_cases(bytes: &[u8], boundaries: &[Boundary]) -> Vec<SweepCase> {
    let len = bytes.len();
    let mut lengths: Vec<usize> = Vec::new();

    for boundary in boundaries {
        for delta in [-1i64, 0, 1] {
            let candidate = boundary.offset as i64 + delta;
            if (0..=len as i64).contains(&candidate) {
                lengths.push(candidate as usize);
            }
        }
    }

    for fraction in STRATA {
        let point = (len as f64 * fraction) as usize;
        if point <= len {
            lengths.push(point);
        }
    }

    if len <= EVERY_BYTE_THRESHOLD {
        lengths.extend(0..=len);
    }

    lengths.sort_unstable();
    lengths.dedup();
    lengths.retain(|&l| l < len);

    lengths
        .into_iter()
        .map(|len| SweepCase {
            label: format!("trunc@{len}"),
            kind: CaseKind::Truncation { len },
            bytes: bytes[..len].to_vec(),
        })
        .collect()
}

/// Generate the single-byte mutation spot-checks at header/count positions.
pub fn mutation_cases(bytes: &[u8], boundaries: &[Boundary]) -> Vec<SweepCase> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for boundary in boundaries {
        if !matches!(boundary.kind, BoundaryKind::Header | BoundaryKind::Count) {
            continue;
        }
        let offset = boundary.offset;
        if offset >= bytes.len() || !seen.insert(offset) {
            continue;
        }
        let original = bytes[offset];
        for replacement in mutations(original) {
            if replacement == original {
                continue;
            }
            let mut mutated = bytes.to_vec();
            mutated[offset] = replacement;
            out.push(SweepCase {
                label: format!("flip{}@{offset}", mutation_tag(original, replacement)),
                kind: CaseKind::Mutation {
                    offset,
                    from: original,
                    to: replacement,
                },
                bytes: mutated,
            });
        }
    }
    out
}

/// The full case set for one fixture: truncations then mutations.
pub fn all_cases(provider: &dyn BoundaryProvider, bytes: &[u8]) -> Vec<SweepCase> {
    let boundaries = provider.boundaries(bytes);
    let mut cases = truncation_cases(bytes, &boundaries);
    cases.extend(mutation_cases(bytes, &boundaries));
    cases
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boundary(offset: usize, kind: BoundaryKind) -> Boundary {
        Boundary { offset, kind }
    }

    #[test]
    fn truncation_covers_boundary_neighbourhood() {
        let bytes = vec![0u8; 1000];
        let cases = truncation_cases(&bytes, &[boundary(100, BoundaryKind::Record)]);
        let lengths: Vec<usize> = cases
            .iter()
            .filter_map(|c| match c.kind {
                CaseKind::Truncation { len } => Some(len),
                CaseKind::Mutation { .. } => None,
            })
            .collect();
        assert!(lengths.contains(&99));
        assert!(lengths.contains(&100));
        assert!(lengths.contains(&101));
        assert!(lengths.len() < bytes.len());
    }

    #[test]
    fn every_byte_below_threshold() {
        let bytes = vec![7u8; 16];
        let cases = truncation_cases(&bytes, &[]);
        let lengths: Vec<usize> = cases
            .iter()
            .filter_map(|c| match c.kind {
                CaseKind::Truncation { len } => Some(len),
                CaseKind::Mutation { .. } => None,
            })
            .collect();
        assert_eq!(lengths, (0..16).collect::<Vec<_>>());
    }

    #[test]
    fn mutation_flips_only_header_and_count() {
        let bytes = vec![0x10u8; 32];
        let boundaries = [
            boundary(0, BoundaryKind::Header),
            boundary(4, BoundaryKind::Record),
            boundary(8, BoundaryKind::Count),
        ];
        let cases = mutation_cases(&bytes, &boundaries);
        let offsets: std::collections::BTreeSet<usize> = cases
            .iter()
            .filter_map(|c| match c.kind {
                CaseKind::Mutation { offset, .. } => Some(offset),
                CaseKind::Truncation { .. } => None,
            })
            .collect();
        assert!(offsets.contains(&0));
        assert!(offsets.contains(&8));
        assert!(!offsets.contains(&4));
    }
}
