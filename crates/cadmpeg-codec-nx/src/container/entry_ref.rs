// SPDX-License-Identifier: Apache-2.0
//! A directory position retained with the immutable directory that bounds it.

use super::DirEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntryRef<'a> {
    entries: &'a [DirEntry],
    index: usize,
}

impl<'a> EntryRef<'a> {
    pub(crate) fn new(entries: &'a [DirEntry], index: usize) -> Option<Self> {
        entries.get(index)?;
        Some(Self { entries, index })
    }

    pub(crate) fn index(self) -> usize {
        self.index
    }
}

impl std::ops::Deref for EntryRef<'_> {
    type Target = DirEntry;

    fn deref(&self) -> &Self::Target {
        &self.entries[self.index]
    }
}

#[cfg(test)]
mod tests {
    use super::EntryRef;
    use crate::container::{DirEntry, Region};

    #[test]
    fn entry_reference_preserves_duplicate_entry_positions() {
        let entry = DirEntry {
            name: "same".into(),
            region: Region::Header,
            body: crate::container::DirEntryBody::Directory,
        };
        let entries = [entry.clone(), entry];
        let first = EntryRef::new(&entries, 0).unwrap();
        let second = EntryRef::new(&entries, 1).unwrap();
        assert_eq!((first.index(), second.index()), (0, 1));
        assert!(std::ptr::eq(&raw const *first, &raw const entries[0]));
        assert!(std::ptr::eq(&raw const *second, &raw const entries[1]));
        assert!(EntryRef::new(&entries, 2).is_none());
    }
}
