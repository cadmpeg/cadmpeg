// SPDX-License-Identifier: Apache-2.0
//! Counted roll-forward group rows and their derived source positions.

use super::discriminators::OperationStatePairTag;
use super::state_group::{OperationStateGroupCount, OperationStateGroupOpener, StateGroupMembers};
use super::state_index::{OperationStateIndex, StateIndexToken};

/// One row in an `m_rollForwardStates` group table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStateGroupRow {
    /// `4a object_index position ff` list member. The common position is a
    /// direct byte; one generation uses the same compact token family as an
    /// object index for positions above the direct range.
    List {
        /// Ordered feature-record member.
        object_index: StateIndexToken,
        /// Serialized list-position token.
        position: StateIndexToken,
    },
    /// `tag object_index object_index ff ff` relation member.
    Pair {
        /// Schema-generation relation tag (`4f` or `48`).
        tag: OperationStatePairTag,
        /// First relation endpoint.
        first: StateIndexToken,
        /// Second relation endpoint.
        second: StateIndexToken,
    },
}

/// One counted group whose row positions follow from its header and tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationStateGroup {
    offset: usize,
    opener: OperationStateGroupOpener,
    members: StateGroupMembers<OperationStateGroupRow>,
}

impl OperationStateGroupRow {
    fn byte_len(self) -> usize {
        match self {
            Self::List {
                object_index,
                position,
            } => 2 + object_index.raw().len() + position.raw().len(),
            Self::Pair { first, second, .. } => 3 + first.raw().len() + second.raw().len(),
        }
    }
}

impl OperationStateGroup {
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }
    pub(crate) fn opener(&self) -> OperationStateGroupOpener {
        self.opener
    }
    #[cfg(test)]
    pub(crate) fn members(&self) -> &StateGroupMembers<OperationStateGroupRow> {
        &self.members
    }
    fn header_len(&self) -> usize {
        3 + usize::from(self.members.count().prefix().is_some())
    }
    pub(crate) fn end_offset(&self) -> usize {
        self.offset
            + self.header_len()
            + self
                .members
                .rows()
                .iter()
                .copied()
                .map(OperationStateGroupRow::byte_len)
                .sum::<usize>()
    }
    pub(crate) fn map_rows<R>(
        self,
        mut map: impl FnMut(usize, OperationStateGroupRow) -> R,
    ) -> StateGroupMembers<R> {
        let mut offset = self.offset + self.header_len();
        self.members.map_rows(|_, row| {
            let row_offset = offset;
            offset += row.byte_len();
            map(row_offset, row)
        })
    }
}

fn operation_state_group_header_at(
    bytes: &[u8],
    at: usize,
) -> Option<(OperationStateGroupOpener, OperationStateGroupCount, usize)> {
    let raw_opener: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
    let opener = OperationStateGroupOpener::try_from(raw_opener).ok()?;
    let count_at = at.checked_add(2)?;
    let (count, cursor) = match bytes.get(count_at) {
        Some(0) => (OperationStateGroupCount::Empty, count_at + 1),
        Some(1) => (
            OperationStateGroupCount::Counted(*bytes.get(count_at + 1)?),
            count_at + 2,
        ),
        _ => return None,
    };
    Some((opener, count, cursor))
}

fn operation_state_group_row_at(
    bytes: &[u8],
    cursor: usize,
    base_offset: usize,
) -> Option<(OperationStateGroupRow, usize)> {
    let tag = *bytes.get(cursor)?;
    match tag {
        0x4a => {
            let object_at = cursor.checked_add(1)?;
            let object_index =
                OperationStateIndex::read_at(bytes, object_at, base_offset)?.token()?;
            let position_at = object_at.checked_add(object_index.raw().len())?;
            let position =
                OperationStateIndex::read_at(bytes, position_at, base_offset)?.token()?;
            let sentinel_at = position_at.checked_add(position.raw().len())?;
            let row_end = sentinel_at.checked_add(1)?;
            (bytes.get(sentinel_at) == Some(&0xff)).then_some((
                OperationStateGroupRow::List {
                    object_index,
                    position,
                },
                row_end,
            ))
        }
        tag => {
            let tag = OperationStatePairTag::try_from(tag).ok()?;
            let first_at = cursor.checked_add(1)?;
            let first = OperationStateIndex::read_at(bytes, first_at, base_offset)?.token()?;
            let second_at = first_at.checked_add(first.raw().len())?;
            let second = OperationStateIndex::read_at(bytes, second_at, base_offset)?.token()?;
            let sentinels_at = second_at.checked_add(second.raw().len())?;
            let row_end = sentinels_at.checked_add(2)?;
            (bytes.get(sentinels_at..row_end) == Some(&[0xff, 0xff]))
                .then_some((OperationStateGroupRow::Pair { tag, first, second }, row_end))
        }
    }
}

pub(super) fn operation_state_group_end_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Option<usize> {
    let (_, count, mut cursor) = operation_state_group_header_at(bytes, at)?;
    let member_count = usize::from(count.declared_count().saturating_sub(1));
    for _ in 0..member_count {
        cursor = operation_state_group_row_at(bytes, cursor, base_offset)?.1;
    }
    (cursor <= end).then_some(cursor)
}

pub(super) fn operation_state_group_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateGroup> {
    let (opener, count, mut cursor) = operation_state_group_header_at(bytes, at)?;
    let member_count = usize::from(count.declared_count().saturating_sub(1));
    let mut rows = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        let (row, row_end) = operation_state_group_row_at(bytes, cursor, base_offset)?;
        rows.push(row);
        cursor = row_end;
    }
    base_offset.checked_add(cursor)?;
    (cursor <= end).then_some(OperationStateGroup {
        offset: base_offset.checked_add(at)?,
        opener,
        members: StateGroupMembers::new(count, rows).ok()?,
    })
}

/// A nonempty contiguous group sequence with its exact boundary suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationStateGroupTable {
    groups: super::nonempty::NonEmpty<OperationStateGroup>,
    footer: GroupTableFooter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupTableFooter {
    Empty,
    Marker,
}

impl OperationStateGroupTable {
    pub(super) fn new(groups: Vec<OperationStateGroup>, trailing_bytes: &[u8]) -> Option<Self> {
        let footer = match trailing_bytes {
            [] => GroupTableFooter::Empty,
            [1, 1] => GroupTableFooter::Marker,
            _ => return None,
        };
        let groups = super::nonempty::NonEmpty::new(groups)?;
        let mut end = groups.first().offset();
        for group in groups.iter() {
            if group.offset() != end {
                return None;
            }
            end = group.end_offset();
        }
        end.checked_add(trailing_bytes.len())?;
        Some(Self { groups, footer })
    }
    pub(crate) fn offset(&self) -> usize {
        self.groups.first().offset()
    }
    pub(crate) fn end_offset(&self) -> usize {
        self.groups.last().end_offset() + self.trailing_bytes().len()
    }
    pub(crate) fn trailing_bytes(&self) -> &'static [u8] {
        match self.footer {
            GroupTableFooter::Empty => &[],
            GroupTableFooter::Marker => &[1, 1],
        }
    }
    pub(crate) fn groups(&self) -> &super::nonempty::NonEmpty<OperationStateGroup> {
        &self.groups
    }
    pub(crate) fn into_groups(self) -> super::nonempty::NonEmpty<OperationStateGroup> {
        self.groups
    }
}

#[cfg(test)]
mod tests {
    use super::{operation_state_group_at, OperationStateGroupTable};

    #[test]
    fn row_positions_follow_mixed_token_widths() {
        let bytes = [
            1, 0, 1, 3, 0x4a, 0x83, 0xba, 1, 0xff, 0x4f, 0xf1, 4, 0x2d, 0x83, 0xe1, 0xff, 0xff,
        ];
        let group = operation_state_group_at(&bytes, 0, bytes.len(), 900).unwrap();
        assert_eq!(group.offset(), 900);
        assert_eq!(group.end_offset(), 917);
        let positions = group.map_rows(|offset, _| offset);
        assert_eq!(positions.rows(), &[904, 909]);
        assert!(operation_state_group_at(&bytes, 0, bytes.len(), usize::MAX - 16).is_none());
    }

    #[test]
    fn table_bounds_follow_groups_and_closed_footer() {
        let bytes = [1, 0, 0];
        let first = operation_state_group_at(&bytes, 0, 3, 10).unwrap();
        let second = operation_state_group_at(&bytes, 0, 3, 13).unwrap();
        for footer in [&[][..], &[1, 1][..]] {
            let table =
                OperationStateGroupTable::new(vec![first.clone(), second.clone()], footer).unwrap();
            assert_eq!(table.offset(), 10);
            assert_eq!(table.end_offset(), 16 + footer.len());
            assert_eq!(table.trailing_bytes(), footer);
        }
        assert!(OperationStateGroupTable::new(Vec::new(), &[]).is_none());
        assert!(OperationStateGroupTable::new(vec![first.clone(), first.clone()], &[]).is_none());
        assert!(OperationStateGroupTable::new(vec![first], &[1]).is_none());
        let last = operation_state_group_at(&bytes, 0, 3, usize::MAX - 3).unwrap();
        assert!(OperationStateGroupTable::new(vec![last], &[1, 1]).is_none());
    }
}
