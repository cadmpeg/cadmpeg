// SPDX-License-Identifier: Apache-2.0
//! Nullable and required operation-state index tokens.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexBytes {
    Direct(u8),
    Compact([u8; 2]),
    Word([u8; 3]),
    Packed([u8; 3]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateIndexToken(IndexBytes);

impl StateIndexToken {
    pub(crate) fn read_at(bytes: &[u8], at: usize) -> Option<Self> {
        let bytes = match bytes.get(at..)? {
            [value @ 0x00..=0x7f, ..] => IndexBytes::Direct(*value),
            [marker @ 0x80..=0x8f, value, ..] => IndexBytes::Compact([*marker, *value]),
            [marker @ (0x90 | 0xf1), a, b, ..] => IndexBytes::Word([*marker, *a, *b]),
            [marker @ 0xa0..=0xaf, a, b, ..] => IndexBytes::Packed([*marker, *a, *b]),
            _ => return None,
        };
        Some(Self(bytes))
    }

    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let token = Self::read_at(raw, 0).filter(|token| token.raw().len() == raw.len())
            .ok_or("invalid non-null state-index token")?;
        if token.value() != value { return Err("decoded value disagrees with raw token"); }
        Ok(token)
    }

    pub(crate) fn value(self) -> u32 {
        match self.0 {
            IndexBytes::Direct(value) => u32::from(value),
            IndexBytes::Compact([marker, value]) => (u32::from(marker - 0x80) << 8) | u32::from(value),
            IndexBytes::Word([_, a, b]) => (u32::from(a) << 8) | u32::from(b),
            IndexBytes::Packed([marker, a, b]) => (u32::from(marker - 0xa0) << 16) | (u32::from(a) << 8) | u32::from(b),
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match &self.0 {
            IndexBytes::Direct(value) => std::slice::from_ref(value),
            IndexBytes::Compact(bytes) => bytes,
            IndexBytes::Word(bytes) | IndexBytes::Packed(bytes) => bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationStateIndex {
    token: Option<StateIndexToken>,
    offset: usize,
}

impl OperationStateIndex {
    pub(crate) fn read_at(bytes: &[u8], at: usize, base_offset: usize) -> Option<Self> {
        let token = if *bytes.get(at)? == 0xff {
            None
        } else {
            Some(StateIndexToken::read_at(bytes, at)?)
        };
        Some(Self { token, offset: base_offset.checked_add(at)? })
    }

    pub(crate) fn token(self) -> Option<StateIndexToken> { self.token }

    pub(crate) fn raw(&self) -> &[u8] { self.token.as_ref().map_or(&[0xff], StateIndexToken::raw) }

    #[cfg(test)]
    pub(crate) fn offset(self) -> usize { self.offset }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NonNullStateIndex {
    token: StateIndexToken,
    offset: usize,
}

impl NonNullStateIndex {
    pub(crate) fn from_index(index: OperationStateIndex) -> Option<Self> {
        Some(Self { token: index.token?, offset: index.offset })
    }

    pub(crate) fn token(self) -> StateIndexToken { self.token }

    pub(crate) fn value(self) -> u32 { self.token.value() }

    pub(crate) fn raw(&self) -> &[u8] { self.token.raw() }

    pub(crate) fn offset(self) -> usize { self.offset }
}

#[cfg(test)]
mod tests {
    use super::{NonNullStateIndex, OperationStateIndex, StateIndexToken};

    #[test]
    fn required_indices_keep_alternate_zero_encodings_and_offsets() {
        for raw in [&[0][..], &[0x80, 0][..], &[0x90, 0, 0][..], &[0xa0, 0, 0][..], &[0xf1, 0, 0][..]] {
            let index = OperationStateIndex::read_at(raw, 0, 100).unwrap();
            let required = NonNullStateIndex::from_index(index).unwrap();
            assert_eq!(required.value(), 0);
            assert_eq!(required.raw(), raw);
            assert_eq!(required.offset(), 100);
        }
    }

    #[test]
    fn null_and_incomplete_indices_cannot_be_required() {
        let null = OperationStateIndex::read_at(&[0xff], 0, 10).unwrap();
        assert_eq!(null.token().map(StateIndexToken::value), None);
        assert_eq!(null.raw(), &[0xff]);
        assert!(NonNullStateIndex::from_index(null).is_none());
        for raw in [&[][..], &[0xff][..], &[0x80][..], &[0x90, 0][..], &[0xa0, 0][..], &[0xf1, 0][..], &[0x91, 0, 0][..]] {
            assert!(StateIndexToken::read_at(raw, 0).is_none());
        }
        assert!(OperationStateIndex::read_at(&[0, 0], 1, usize::MAX).is_none());
    }
}
