// SPDX-License-Identifier: Apache-2.0
//! Positional byte diff between two files.
//!
//! The comparison is positional, not an edit script: byte `n` of the first file
//! is compared with byte `n` of the second. Probe variants of one exporter keep
//! their field offsets, so a positional diff points straight at the changed
//! fields. An inserted byte shifts everything after it and shows up as one long
//! run, which is itself the signal that the files are not positional variants.

/// A maximal span of differing bytes, after gap coalescing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffRun {
    /// Offset of the first differing byte in the run.
    pub start: u64,
    /// Number of bytes the run covers, including coalesced equal bytes.
    pub len: u64,
}

impl DiffRun {
    /// Returns the exclusive end offset of the run.
    pub const fn end(self) -> u64 {
        self.start + self.len
    }
}

/// A positional comparison of two byte strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSummary {
    /// Length of the first input.
    len_a: u64,
    /// Length of the second input.
    len_b: u64,
    /// Count of differing bytes inside the compared prefix.
    differing: u64,
    /// Differing spans after coalescing, in offset order.
    runs: Vec<DiffRun>,
}

impl DiffSummary {
    /// Returns the first differing offset.
    pub fn first(&self) -> Option<u64> {
        self.runs.first().map(|run| run.start)
    }

    /// Returns `len_a` for the compared inputs.
    pub const fn len_a(&self) -> u64 {
        self.len_a
    }

    /// Returns `len_b` for the compared inputs.
    pub const fn len_b(&self) -> u64 {
        self.len_b
    }

    /// Returns `compared` for the compared inputs.
    pub fn compared(&self) -> u64 {
        self.len_a.min(self.len_b)
    }

    /// Returns `differing` for the compared inputs.
    pub const fn differing(&self) -> u64 {
        self.differing
    }

    /// Returns the coalesced differing spans.
    pub fn runs(&self) -> &[DiffRun] {
        &self.runs
    }

    /// Returns true when the inputs are byte identical.
    pub const fn identical(&self) -> bool {
        self.len_a == self.len_b && self.differing == 0
    }
}

/// Compares `a` and `b` positionally over their common prefix.
///
/// Two differing spans separated by `gap` or fewer equal bytes are reported as
/// one run, so a changed multi-field record reads as a single region instead of
/// one run per byte.
pub fn compare(a: &[u8], b: &[u8], gap: u64) -> DiffSummary {
    let compared = a.len().min(b.len());
    let mut runs: Vec<DiffRun> = Vec::new();
    let mut differing = 0u64;
    for offset in 0..compared {
        if a[offset] == b[offset] {
            continue;
        }
        differing += 1;
        let offset = offset as u64;
        match runs.last_mut() {
            Some(last) if offset <= last.end().saturating_add(gap) => {
                last.len = offset - last.start + 1;
            }
            _ => runs.push(DiffRun {
                start: offset,
                len: 1,
            }),
        }
    }
    DiffSummary {
        len_a: a.len() as u64,
        len_b: b.len() as u64,
        differing,
        runs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_inputs_produce_no_runs() {
        let summary = compare(b"abcd", b"abcd", 0);
        assert!(summary.identical());
        assert_eq!(summary.differing, 0);
        assert_eq!(summary.first(), None);
        assert!(summary.runs.is_empty());
    }

    #[test]
    fn a_length_difference_alone_is_not_identical() {
        let summary = compare(b"abcd", b"abcdef", 0);
        assert!(!summary.identical());
        assert_eq!(summary.compared(), 4);
        assert_eq!(summary.differing, 0);
        assert_eq!(summary.first(), None);
        assert_eq!((summary.len_a, summary.len_b), (4, 6));
    }

    #[test]
    fn reports_the_first_difference_and_each_byte_at_gap_zero() {
        // Differ at offsets 1 and 3 only.
        let summary = compare(b"abcde", b"aXcYe", 0);
        assert_eq!(summary.first(), Some(1));
        assert_eq!(summary.differing, 2);
        assert_eq!(
            summary.runs,
            [DiffRun { start: 1, len: 1 }, DiffRun { start: 3, len: 1 },]
        );
    }

    #[test]
    fn coalesces_spans_separated_by_at_most_the_gap() {
        // Differ at 1 and 3, so one equal byte lies between them.
        let summary = compare(b"abcde", b"aXcYe", 1);
        assert_eq!(summary.differing, 2);
        assert_eq!(summary.runs, [DiffRun { start: 1, len: 3 }]);

        // Differ at 0 and 4, so three equal bytes lie between them.
        let wide = compare(b"abcde", b"XbcdY", 1);
        assert_eq!(
            wide.runs,
            [DiffRun { start: 0, len: 1 }, DiffRun { start: 4, len: 1 },]
        );
        let merged = compare(b"abcde", b"XbcdY", 3);
        assert_eq!(merged.runs, [DiffRun { start: 0, len: 5 }]);
    }

    #[test]
    fn a_run_of_adjacent_differences_merges_at_any_gap() {
        let summary = compare(&[0, 0, 0, 0], &[0, 1, 2, 3], 0);
        assert_eq!(summary.runs, [DiffRun { start: 1, len: 3 }]);
        assert_eq!(summary.differing, 3);
    }

    #[test]
    fn only_the_common_prefix_is_compared() {
        let summary = compare(b"abc", b"abcZZZZ", 0);
        assert_eq!(summary.compared(), 3);
        assert_eq!(summary.differing, 0);
        assert_eq!(summary.runs, []);
    }

    #[test]
    fn empty_inputs_compare_cleanly() {
        let summary = compare(&[], &[], 0);
        assert!(summary.identical());
        assert_eq!(summary.compared(), 0);
    }
}
