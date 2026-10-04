// SPDX-License-Identifier: Apache-2.0
//! Contiguous, nonempty state-journal groups.

use super::nonempty::NonEmpty;
use super::state_journal::JournalRow;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

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
    pub(super) fn read(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        at: usize,
        end: usize,
        base: usize,
    ) -> Result<Option<Self>, CodecError> {
        let Some(tail) = bytes.get(at..end) else {
            return Ok(None);
        };
        let [0x04, a, b, 0x00, ..] = tail else {
            return Ok(None);
        };
        let selector = [*a, *b];
        let header = if tail.get(4) == Some(&0) {
            Header::Padded
        } else {
            Header::Plain
        };
        let Some(mut cursor) = at.checked_add(usize::from(header.byte_len())) else {
            return Ok(None);
        };
        let mut rows = Vec::new();
        while cursor < end {
            ctx.charge_work(1, "scan NX state-journal group")?;
            let Some(row) = JournalRow::read(bytes, cursor, end, base) else {
                break;
            };
            let Some(next) = cursor.checked_add(row.byte_len()) else {
                return Ok(None);
            };
            cursor = next;
            ctx.reserve_vec(&mut rows, 1, "NX state-journal rows")?;
            rows.push(row);
        }
        Ok(NonEmpty::from_admitted_vec(rows).map(|rows| Self {
            selector,
            header,
            rows,
        }))
    }

    pub(super) fn end_offset(&self) -> usize {
        let last = self.rows.last();
        last.offset() + last.byte_len()
    }

    pub(crate) fn into_absolute(
        self,
        ctx: &DecodeContext<'_>,
        base: u64,
    ) -> Result<Option<JournalGroup>, CodecError> {
        Ok(self
            .rows
            .try_map_charged(ctx, |row| row.into_absolute(base))?
            .map(|rows| JournalGroup {
                selector: self.selector,
                header: self.header,
                rows,
            }))
    }
}

impl JournalGroup {
    pub(crate) fn new(
        selector: [u8; 2],
        source_offset: u64,
        rows: Vec<JournalRow>,
    ) -> Result<Self, &'static str> {
        let rows =
            NonEmpty::from_admitted_vec(rows).ok_or("rows: journal group must not be empty")?;
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

#[cfg(test)]
mod tests {
    use super::JournalGroup;
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    #[test]
    fn journal_group_row_scan_refuses_work_before_reading() {
        let bytes = [0x04, 0x01, 0x02, 0x00, 0x7f];
        crate::test_support::with_decode_context_over(
            &bytes,
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                let error = JournalGroup::read(ctx, &bytes, 0, bytes.len(), 0).unwrap_err();
                assert!(matches!(
                    error,
                    CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::WorkUnits
                            && limit.used == 0
                            && limit.additional == 1
                            && limit.operation == "scan NX state-journal group"
                ));
            },
        );
    }
}
