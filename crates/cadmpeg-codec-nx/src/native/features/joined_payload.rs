// SPDX-License-Identifier: Apache-2.0
//! Joined payload bytes and the source spans that produced them.

use std::collections::BTreeMap;

struct SourceSpan {
    byte_len: u64,
    source_offset: u64,
}

pub(super) struct JoinedPayload {
    bytes: Vec<u8>,
    sources: Vec<SourceSpan>,
}

impl JoinedPayload {
    pub(super) fn from_source<'a>(
        ids: impl ExactSizeIterator<Item = &'a String> + Clone,
        blocks: &BTreeMap<String, (&[u8], u64)>,
    ) -> Option<Self> {
        let byte_len = ids.clone().try_fold(0usize, |total, id| {
            let (bytes, _) = blocks.get(id).copied()?;
            total.checked_add(bytes.len())
        })?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(byte_len).ok()?;
        let mut sources = Vec::new();
        sources.try_reserve_exact(ids.len()).ok()?;
        for id in ids {
            let (fragment, source_offset) = blocks.get(id).copied()?;
            source_offset.checked_add(fragment.len() as u64)?;
            bytes.extend_from_slice(fragment);
            sources.push(SourceSpan { byte_len: fragment.len() as u64, source_offset });
        }
        Some(Self { bytes, sources })
    }

    pub(super) fn bytes(&self) -> &[u8] { &self.bytes }

    pub(super) fn source_spans(&self) -> impl Iterator<Item = (u64, u64, u64)> + '_ {
        let mut at = 0;
        self.sources.iter().map(move |span| {
            let start = at;
            at += span.byte_len;
            (start, span.byte_len, span.source_offset)
        })
    }

    pub(super) fn source_offset(&self, relative: u64) -> Option<u64> {
        self.source_spans().find_map(|(start, length, source)| {
            (relative >= start && relative - start < length)
                .then(|| source + (relative - start))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_locations_follow_fragment_boundaries_and_skip_empty_blocks() {
        let ids = ["a".to_owned(), "empty".to_owned(), "b".to_owned()];
        let blocks = BTreeMap::from([
            ("a".to_owned(), (&[1, 2][..], 10)),
            ("empty".to_owned(), (&[][..], u64::MAX)),
            ("b".to_owned(), (&[3, 4, 5][..], 100)),
        ]);
        let joined = JoinedPayload::from_source(ids.iter(), &blocks).unwrap();
        assert_eq!(joined.bytes(), [1, 2, 3, 4, 5]);
        assert_eq!([0, 1, 2, 4, 5, u64::MAX].map(|offset| joined.source_offset(offset)),
            [Some(10), Some(11), Some(100), Some(102), None, None]);
    }

    #[test]
    fn source_extent_overflow_is_rejected_at_construction() {
        let ids = ["a".to_owned()];
        let blocks = BTreeMap::from([("a".to_owned(), (&[1, 2][..], u64::MAX))]);
        assert!(JoinedPayload::from_source(ids.iter(), &blocks).is_none());
    }
}
