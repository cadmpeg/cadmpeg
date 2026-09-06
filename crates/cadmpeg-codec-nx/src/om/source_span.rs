// SPDX-License-Identifier: Apache-2.0
//! Source ranges with checked absolute projections and retained local positions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceSpan {
    base: usize,
    start: usize,
    end: usize,
}

impl SourceSpan {
    pub(crate) fn new(base: usize, start: usize, end: usize) -> Option<Self> {
        if start > end {
            return None;
        }
        base.checked_add(end)?;
        Some(Self { base, start, end })
    }

    pub(crate) fn offset(self) -> usize {
        self.base + self.start
    }
    pub(crate) fn end_offset(self) -> usize {
        self.base + self.end
    }
    #[cfg(test)]
    pub(crate) fn local_start(self) -> usize {
        self.start
    }
    pub(crate) fn local_end(self) -> usize {
        self.end
    }
}

#[cfg(test)]
mod tests {
    use super::SourceSpan;

    #[test]
    fn source_span_retains_local_positions_and_bounds_absolute_projections() {
        let span = SourceSpan::new(100, 2, 7).unwrap();
        assert_eq!((span.offset(), span.end_offset()), (102, 107));
        assert_eq!((span.local_start(), span.local_end()), (2, 7));
        let boundary = SourceSpan::new(usize::MAX - 7, 2, 7).unwrap();
        assert_eq!(boundary.end_offset(), usize::MAX);
        assert!(SourceSpan::new(usize::MAX - 6, 2, 7).is_none());
        assert!(SourceSpan::new(0, 7, 2).is_none());
    }
}
