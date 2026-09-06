// SPDX-License-Identifier: Apache-2.0
//! Operation-state group framing and count-constrained membership.

/// Two admitted group opener encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationStateGroupOpener {
    /// `01 00` opener.
    Form00,
    /// `01 01` opener.
    Form01,
}

impl OperationStateGroupOpener {
    pub fn bytes(self) -> [u8; 2] {
        match self {
            Self::Form00 => [1, 0],
            Self::Form01 => [1, 1],
        }
    }
}

impl TryFrom<[u8; 2]> for OperationStateGroupOpener {
    type Error = &'static str;
    fn try_from(bytes: [u8; 2]) -> Result<Self, Self::Error> {
        match bytes {
            [1, 0] => Ok(Self::Form00),
            [1, 1] => Ok(Self::Form01),
            _ => Err("invalid operation-state group opener"),
        }
    }
}

/// Empty or explicitly counted operation-state group header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationStateGroupCount {
    /// Single zero byte, without an explicit count.
    Empty,
    /// `01 count`, including the implicit owner slot.
    Counted(u8),
}

impl OperationStateGroupCount {
    pub fn prefix(self) -> Option<u8> {
        match self {
            Self::Empty => None,
            Self::Counted(_) => Some(1),
        }
    }

    pub fn declared_count(self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::Counted(count) => count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupBody<R> {
    Empty,
    CountedZero,
    Counted(Vec<R>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StateGroupMembers<R>(GroupBody<R>);

impl<R> StateGroupMembers<R> {
    pub(crate) fn new(count: OperationStateGroupCount, rows: Vec<R>) -> Result<Self, &'static str> {
        if rows.len() != usize::from(count.declared_count().saturating_sub(1)) {
            return Err("declared_count/rows: row count disagrees with header");
        }
        let body = match count {
            OperationStateGroupCount::Empty => GroupBody::Empty,
            OperationStateGroupCount::Counted(0) => GroupBody::CountedZero,
            OperationStateGroupCount::Counted(_) => GroupBody::Counted(rows),
        };
        Ok(Self(body))
    }

    pub(crate) fn count(&self) -> OperationStateGroupCount {
        match &self.0 {
            GroupBody::Empty => OperationStateGroupCount::Empty,
            GroupBody::CountedZero => OperationStateGroupCount::Counted(0),
            GroupBody::Counted(rows) => OperationStateGroupCount::Counted(rows.len() as u8 + 1),
        }
    }

    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[R] {
        match &self.0 {
            GroupBody::Empty | GroupBody::CountedZero => &[],
            GroupBody::Counted(rows) => rows,
        }
    }

    pub(crate) fn into_rows(self) -> Vec<R> {
        match self.0 {
            GroupBody::Empty | GroupBody::CountedZero => Vec::new(),
            GroupBody::Counted(rows) => rows,
        }
    }

    pub(crate) fn map_rows<U>(self, mut map: impl FnMut(u8, R) -> U) -> StateGroupMembers<U> {
        StateGroupMembers(match self.0 {
            GroupBody::Empty => GroupBody::Empty,
            GroupBody::CountedZero => GroupBody::CountedZero,
            GroupBody::Counted(rows) => GroupBody::Counted(rows.into_iter().enumerate()
                .map(|(ordinal, row)| map(ordinal as u8, row)).collect()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationStateGroupCount, StateGroupMembers};

    #[test]
    fn group_members_preserve_all_three_zero_row_headers() {
        for count in [OperationStateGroupCount::Empty, OperationStateGroupCount::Counted(0), OperationStateGroupCount::Counted(1)] {
            let members = StateGroupMembers::<u8>::new(count, Vec::new()).unwrap();
            assert_eq!(members.count(), count);
            assert!(members.rows().is_empty());
            assert_eq!(members.map_rows(|_, row| u32::from(row)).count(), count);
            assert!(StateGroupMembers::new(count, vec![0]).is_err());
        }
    }

    #[test]
    fn group_members_derive_count_and_bound_ordinals() {
        let members = StateGroupMembers::new(OperationStateGroupCount::Counted(255), vec![(); 254]).unwrap();
        let members = members.map_rows(|ordinal, ()| ordinal);
        assert_eq!(members.count().declared_count(), 255);
        assert_eq!(members.rows().first(), Some(&0));
        assert_eq!(members.rows().last(), Some(&253));
        assert!(StateGroupMembers::new(OperationStateGroupCount::Counted(255), vec![(); 255]).is_err());
        assert!(StateGroupMembers::new(OperationStateGroupCount::Counted(2), Vec::<()>::new()).is_err());
    }
}
