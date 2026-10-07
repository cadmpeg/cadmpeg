// SPDX-License-Identifier: Apache-2.0
//! Counted roll-forward group rows and their derived source positions.

use super::discriminators::OperationStatePairTag;
use super::state_group::{OperationStateGroupCount, OperationStateGroupOpener, StateGroupMembers};
use super::state_index::{OperationStateIndex, StateIndexToken};

/// One row in an `m_rollForwardStates` group table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationStateGroupRow {
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
pub(crate) struct OperationStateGroup<O = usize> {
    offset: O,
    byte_len: u16,
    opener: OperationStateGroupOpener,
    members: StateGroupMembers<OperationStateGroupRow>,
}

impl OperationStateGroupRow {
    pub(crate) fn byte_len(self) -> u16 {
        match self {
            Self::List {
                object_index,
                position,
            } => 2 + u16::from(object_index.byte_len()) + u16::from(position.byte_len()),
            Self::Pair { first, second, .. } => {
                3 + u16::from(first.byte_len()) + u16::from(second.byte_len())
            }
        }
    }
}

impl<O: Copy + From<u16> + std::ops::Add<Output = O>> OperationStateGroup<O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn opener(&self) -> OperationStateGroupOpener {
        self.opener
    }
    pub(crate) fn members(&self) -> &StateGroupMembers<OperationStateGroupRow> {
        &self.members
    }
    #[cfg(test)]
    fn header_len(&self) -> u16 {
        3 + u16::from(self.members.count().prefix().is_some())
    }
    fn byte_len(&self) -> u16 {
        self.byte_len
    }

    fn extent<'a, E, I: Iterator<Item = &'a OperationStateGroupRow>>(
        members: &'a StateGroupMembers<OperationStateGroupRow>,
        admit: impl FnOnce(&'a [OperationStateGroupRow]) -> Result<I, E>,
    ) -> Result<Option<u16>, E> {
        let header = 3 + u16::from(members.count().prefix().is_some());
        Ok(admit(members.rows())?
            .try_fold(header, |length, row| length.checked_add(row.byte_len())))
    }
    pub(crate) fn end_offset(&self) -> O {
        self.offset + O::from(self.byte_len())
    }
    #[cfg(test)]
    pub(crate) fn map_rows<R>(
        self,
        mut map: impl FnMut(u8, O, OperationStateGroupRow) -> R,
    ) -> StateGroupMembers<R> {
        let mut offset = self.offset + O::from(self.header_len());
        self.members.map_rows(|ordinal, row| {
            let row_offset = offset;
            offset = offset + O::from(row.byte_len());
            map(ordinal, row_offset, row)
        })
    }
}

impl OperationStateGroup {
    pub(crate) fn into_absolute(self, base: u64) -> Option<OperationStateGroup<u64>> {
        let offset = base.checked_add(u64::try_from(self.offset).ok()?)?;
        offset.checked_add(u64::from(self.byte_len))?;
        Some(OperationStateGroup {
            offset,
            byte_len: self.byte_len,
            opener: self.opener,
            members: self.members,
        })
    }
}

impl OperationStateGroup<u64> {
    pub(crate) fn new(
        offset: u64,
        opener: OperationStateGroupOpener,
        members: StateGroupMembers<OperationStateGroupRow>,
    ) -> Result<Self, &'static str> {
        let byte_len = match Self::extent(&members, |rows| {
            Ok::<_, std::convert::Infallible>(rows.iter())
        }) {
            Ok(width) => width.ok_or("source_offset: roll-forward group extent overflows")?,
            Err(error) => match error {},
        };
        offset
            .checked_add(u64::from(byte_len))
            .ok_or("source_offset: roll-forward group extent overflows")?;
        Ok(Self {
            offset,
            byte_len,
            opener,
            members,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let parsed: Option<Result<_, cadmpeg_core::CodecError>> = (|| {
        let (_, count, mut cursor) = operation_state_group_header_at(bytes, at)?;
        let member_count = count.member_row_count();
        for _ in propagate_resource!(ctx
            .admit_iter(&(0..member_count), "NX operation-state group validation")
            .map_err(cadmpeg_core::CodecError::from))
        {
            cursor = operation_state_group_row_at(bytes, cursor, base_offset)?.1;
        }
        ((cursor <= end).then_some(cursor)).map(Ok)
    })();
    parsed.transpose()
}

pub(super) fn operation_state_group_at(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Result<Option<OperationStateGroup>, cadmpeg_core::CodecError> {
    let Some((opener, count, mut cursor)) = operation_state_group_header_at(bytes, at) else {
        return Ok(None);
    };
    let Some(group_end) = operation_state_group_end_at(ctx, bytes, at, end, base_offset)? else {
        return Ok(None);
    };
    let Some(offset) = base_offset.checked_add(at) else {
        return Ok(None);
    };
    if base_offset.checked_add(group_end).is_none() {
        return Ok(None);
    }
    let member_count = count.member_row_count();
    let operation = "NX operation-state group rows";
    let mut rows = ctx.collection_vec(member_count, operation)?;
    for _ in ctx.admit_iter(&(0..member_count), operation)? {
        let Some((row, row_end)) = operation_state_group_row_at(bytes, cursor, base_offset) else {
            return Ok(None);
        };
        rows.push(row);
        cursor = row_end;
    }
    if cursor != group_end {
        return Ok(None);
    }
    let Some(members) = StateGroupMembers::new(count, rows).ok() else {
        return Ok(None);
    };
    let byte_len = OperationStateGroup::<usize>::extent(&members, |rows| {
        ctx.admit_iter(rows, "NX roll-forward row widths")
    })?
    .ok_or_else(|| ctx.refuse_codec_limit("NX roll-forward extent", u64::MAX, u64::MAX))?;
    Ok(Some(OperationStateGroup {
        offset,
        byte_len,
        opener,
        members,
    }))
}

/// A nonempty contiguous group sequence with its exact boundary suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationStateGroupTable {
    groups: super::nonempty::NonEmpty<OperationStateGroup>,
    footer: GroupTableFooter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupTableFooter {
    Empty,
    Marker,
}

impl TryFrom<&[u8]> for GroupTableFooter {
    type Error = &'static str;
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        match bytes {
            [] => Ok(Self::Empty),
            [1, 1] => Ok(Self::Marker),
            _ => Err("table_trailing_bytes: invalid roll-forward table footer"),
        }
    }
}

impl GroupTableFooter {
    pub(crate) fn bytes(self) -> &'static [u8] {
        match self {
            Self::Empty => &[],
            Self::Marker => &[1, 1],
        }
    }
}

impl OperationStateGroupTable {
    pub(super) fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        groups: Vec<OperationStateGroup>,
        trailing_bytes: &[u8],
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        (|| {
            let footer = GroupTableFooter::try_from(trailing_bytes).ok()?;
            let groups = super::nonempty::NonEmpty::from_admitted_vec(groups)?;
            let mut end = groups.first().offset();
            for group in propagate_resource!(ctx
                .admit_iter(groups.initial(), "NX roll-forward group continuity")
                .map_err(cadmpeg_core::CodecError::from))
            .chain(propagate_resource!(ctx
                .admit_iter(
                    std::slice::from_ref(groups.last()),
                    "NX roll-forward group continuity"
                )
                .map_err(cadmpeg_core::CodecError::from)))
            {
                if group.offset() != end {
                    return None;
                }
                end = group.end_offset();
            }
            end.checked_add(trailing_bytes.len())?;
            Some(Ok(Self { groups, footer }))
        })()
        .transpose()
    }
    pub(super) fn offset(&self) -> usize {
        self.groups.first().offset()
    }
    pub(crate) fn end_offset(&self) -> usize {
        self.groups.last().end_offset() + self.trailing_bytes().len()
    }
    pub(crate) fn footer(&self) -> GroupTableFooter {
        self.footer
    }
    pub(super) fn trailing_bytes(&self) -> &'static [u8] {
        self.footer.bytes()
    }
    pub(super) fn groups(&self) -> &super::nonempty::NonEmpty<OperationStateGroup> {
        &self.groups
    }
    pub(crate) fn into_groups(self) -> super::nonempty::NonEmpty<OperationStateGroup> {
        self.groups
    }
}

#[cfg(test)]
mod tests {
    use super::{operation_state_group_at, OperationStateGroupTable};

    fn group_at(
        bytes: &[u8],
        at: usize,
        end: usize,
        base_offset: usize,
    ) -> Option<super::OperationStateGroup> {
        crate::test_support::with_decode_context(|ctx| {
            operation_state_group_at(ctx, bytes, at, end, base_offset)
        })
        .unwrap()
    }

    #[test]
    fn row_positions_follow_mixed_token_widths() {
        let bytes = [
            1, 0, 1, 3, 0x4a, 0x83, 0xba, 1, 0xff, 0x4f, 0xf1, 4, 0x2d, 0x83, 0xe1, 0xff, 0xff,
        ];
        let group = group_at(&bytes, 0, bytes.len(), 900).unwrap();
        assert_eq!(group.offset(), 900);
        assert_eq!(group.end_offset(), 917);
        let positions = group.map_rows(|_, offset, _| offset);
        assert_eq!(positions.rows(), &[904, 909]);
        assert!(group_at(&bytes, 0, bytes.len(), usize::MAX - 16).is_none());
    }

    #[test]
    fn table_bounds_follow_groups_and_closed_footer() {
        let bytes = [1, 0, 0];
        let first = group_at(&bytes, 0, 3, 10).unwrap();
        let second = group_at(&bytes, 0, 3, 13).unwrap();
        for footer in [&[][..], &[1, 1][..]] {
            let table = crate::test_support::with_decode_context(|ctx| {
                OperationStateGroupTable::new(ctx, vec![first.clone(), second.clone()], footer)
            })
            .unwrap()
            .unwrap();
            assert_eq!(table.offset(), 10);
            assert_eq!(table.end_offset(), 16 + footer.len());
            assert_eq!(table.trailing_bytes(), footer);
        }
        assert!(
            crate::test_support::with_decode_context(|ctx| OperationStateGroupTable::new(
                ctx,
                Vec::new(),
                &[]
            ))
            .unwrap()
            .is_none()
        );
        assert!(
            crate::test_support::with_decode_context(|ctx| OperationStateGroupTable::new(
                ctx,
                vec![first.clone(), first.clone()],
                &[]
            ))
            .unwrap()
            .is_none()
        );
        assert!(
            crate::test_support::with_decode_context(|ctx| OperationStateGroupTable::new(
                ctx,
                vec![first],
                &[1]
            ))
            .unwrap()
            .is_none()
        );
        let last = group_at(&bytes, 0, 3, usize::MAX - 3).unwrap();
        assert!(
            crate::test_support::with_decode_context(|ctx| OperationStateGroupTable::new(
                ctx,
                vec![last],
                &[1, 1]
            ))
            .unwrap()
            .is_none()
        );
    }
}
