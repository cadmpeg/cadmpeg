// SPDX-License-Identifier: Apache-2.0
//! Contiguous, nonempty state-journal groups.

use super::nonempty::NonEmpty;
use super::state_journal::JournalRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Header {
    Plain,
    Padded,
}

impl Header {
    fn byte_len(self) -> u8 {
        match self {
            Self::Plain => 4,
            Self::Padded => 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalGroup<O = u64> {
    selector: [u8; 2],
    header: Header,
    rows: NonEmpty<JournalRow<O>>,
}

impl<O> JournalGroup<O> {
    pub(crate) fn selector(&self) -> [u8; 2] {
        self.selector
    }
    pub(crate) fn rows(&self) -> &NonEmpty<JournalRow<O>> {
        &self.rows
    }
}

impl<O: Copy + From<u8> + std::ops::Sub<Output = O>> JournalGroup<O> {
    pub(crate) fn offset(&self) -> O {
        self.rows.first().offset() - O::from(self.header.byte_len())
    }
}

impl JournalGroup<usize> {
    pub(crate) fn read(bytes: &[u8], at: usize, end: usize, base: usize) -> Option<Self> {
        let tail = bytes.get(at..end)?;
        let [0x04, a, b, 0x00, ..] = tail else {
            return None;
        };
        let selector = [*a, *b];
        let header = if tail.get(4) == Some(&0) {
            Header::Padded
        } else {
            Header::Plain
        };
        let mut cursor = at.checked_add(usize::from(header.byte_len()))?;
        let mut rows = Vec::new();
        while cursor < end {
            let Some(row) = JournalRow::read(bytes, cursor, end, base) else {
                break;
            };
            cursor = cursor.checked_add(row.byte_len())?;
            rows.push(row);
        }
        Some(Self {
            selector,
            header,
            rows: NonEmpty::new(rows)?,
        })
    }

    pub(crate) fn end_offset(&self) -> usize {
        let last = self.rows.last();
        last.offset() + last.byte_len()
    }

    pub(crate) fn into_absolute(self, base: u64) -> Option<JournalGroup> {
        Some(JournalGroup {
            selector: self.selector,
            header: self.header,
            rows: self.rows.map(|row| row.into_absolute(base)).transpose()?,
        })
    }
}

impl JournalGroup {
    pub(crate) fn new(
        selector: [u8; 2],
        source_offset: u64,
        rows: Vec<JournalRow>,
    ) -> Result<Self, &'static str> {
        let rows = NonEmpty::new(rows).ok_or("rows: journal group must not be empty")?;
        let header =
            match rows.first().offset().checked_sub(source_offset) {
                Some(4) => Header::Plain,
                Some(5) => Header::Padded,
                _ => return Err(
                    "source_offset/rows.source_offset: journal header must span four or five bytes",
                ),
            };
        let mut expected = rows.first().offset();
        for row in rows.iter() {
            if row.offset() != expected {
                return Err("rows.source_offset: journal rows must be contiguous");
            }
            expected = row.end_offset();
        }
        Ok(Self {
            selector,
            header,
            rows,
        })
    }

    pub(crate) fn end_offset(&self) -> u64 {
        self.rows.last().end_offset()
    }
}
