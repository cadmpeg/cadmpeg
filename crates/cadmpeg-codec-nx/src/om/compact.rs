// SPDX-License-Identifier: Apache-2.0
//! Exact non-null compact indices and bounded counted-lane members.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Direct(u8),
    Extended(ExtendedCompactIndex),
}

/// Exact compact-index encoding, excluding the `ff` null token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactIndexAtom(Encoding);

impl CompactIndexAtom {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        match bytes.first().copied()? {
            value @ 0..=0x7f => Some(Self(Encoding::Direct(value))),
            _ => ExtendedCompactIndex::read(bytes).map(|index| Self(Encoding::Extended(index))),
        }
    }

    pub(crate) fn value(self) -> u32 {
        match self.0 {
            Encoding::Direct(value) => u32::from(value),
            Encoding::Extended(index) => index.value(),
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match &self.0 {
            Encoding::Direct(value) => std::slice::from_ref(value),
            Encoding::Extended(index) => index.raw(),
        }
    }

    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let atom = Self::read(raw).ok_or("index raw token: invalid non-null compact encoding")?;
        if atom.raw().len() != raw.len() || atom.value() != value {
            return Err("index/raw token: value or width mismatch");
        }
        Ok(atom)
    }
}

/// One source index paired with its resolved target, or `()` before resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactIndexTarget<T> {
    pub(crate) atom: CompactIndexAtom,
    pub(crate) target: T,
}

impl From<CompactIndexAtom> for CompactIndexTarget<()> {
    fn from(atom: CompactIndexAtom) -> Self {
        Self { atom, target: () }
    }
}

/// Read-only projection of one compact index and its derived source position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PositionedIndex<'a, T, O> {
    pub(crate) atom: CompactIndexAtom,
    pub(crate) target: &'a T,
    pub(crate) offset: O,
}

/// Non-null compact index restricted to its two-byte encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExtendedCompactIndex([u8; 2]);

impl ExtendedCompactIndex {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [high @ 0x80..=0xfe, low, ..] => Some(Self([*high, *low])),
            _ => None,
        }
    }

    pub(crate) fn value(self) -> u32 {
        u32::from(self.0[0] - 0x80) * 256 + u32::from(self.0[1])
    }

    pub(crate) fn raw(&self) -> &[u8; 2] {
        &self.0
    }

    // Validate the value against the same borrowed raw byte window used by the reader.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(crate) fn from_wire(value: u32, raw: &[u8; 2]) -> Result<Self, &'static str> {
        let index = Self::read(raw).ok_or("invalid two-byte compact index")?;
        if index.value() != value {
            return Err("index/raw token: value mismatch");
        }
        Ok(index)
    }
}

/// Extended compact index inside a `3d high low 00` word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WrappedCompactIndex(ExtendedCompactIndex);

impl WrappedCompactIndex {
    pub(crate) fn read(raw: u32) -> Option<Self> {
        let [marker, high, low, terminal] = raw.to_be_bytes();
        (marker == 0x3d && terminal == 0).then_some(())?;
        ExtendedCompactIndex::read(&[high, low]).map(Self)
    }

    pub(crate) fn value(self) -> u32 {
        self.0.value()
    }

    pub(crate) fn raw(self) -> u32 {
        0x3d00_0000 | (u32::from(self.0.raw()[0]) << 16) | (u32::from(self.0.raw()[1]) << 8)
    }

    pub(crate) fn from_wire(value: u32, raw: u32) -> Result<Self, &'static str> {
        let atom = Self::read(raw).ok_or("invalid wrapped compact index")?;
        if atom.value() != value {
            return Err("index/raw word: value mismatch");
        }
        Ok(atom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocatedCompactIndex<O = usize, T = CompactIndexAtom> {
    pub(crate) atom: T,
    pub(crate) offset: O,
}

impl LocatedCompactIndex {
    pub(crate) fn read(bytes: &[u8], offset: usize) -> Option<Self> {
        Some(Self {
            atom: CompactIndexAtom::read(bytes.get(offset..)?)?,
            offset,
        })
    }
    pub(crate) fn read_array<const N: usize>(bytes: &[u8], at: &mut usize) -> Option<[Self; N]> {
        (0..N)
            .map(|_| {
                let token = Self::read(bytes, *at)?;
                *at += token.atom.raw().len();
                Some(token)
            })
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NullableCompactIndex {
    pub(crate) atom: Option<CompactIndexAtom>,
    pub(crate) offset: usize,
}

impl NullableCompactIndex {
    pub(crate) fn read(bytes: &[u8], offset: usize) -> Option<Self> {
        let atom = if *bytes.get(offset)? == 0xff {
            None
        } else {
            Some(CompactIndexAtom::read(bytes.get(offset..)?)?)
        };
        Some(Self { atom, offset })
    }

    pub(crate) fn raw(&self) -> &[u8] {
        self.atom.as_ref().map_or(&[0xff], CompactIndexAtom::raw)
    }
}

/// Nonempty members of a byte-counted lane reserving entries for its framing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CountedIndexMembers<T, const RESERVED: u8 = 2>(Vec<T>);

impl<T, const RESERVED: u8> CountedIndexMembers<T, RESERVED> {
    pub(crate) fn new(members: Vec<T>) -> Result<Self, &'static str> {
        if !(1..=usize::from(u8::MAX - RESERVED)).contains(&members.len()) {
            return Err("members: must be nonempty and fit the declared byte count");
        }
        Ok(Self(members))
    }

    pub(crate) fn declared_count(&self) -> u8 {
        (self.0.len() + usize::from(RESERVED)) as u8
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        &self.0
    }

    pub(crate) fn map<U>(self, f: impl FnMut(T) -> U) -> CountedIndexMembers<U, RESERVED> {
        CountedIndexMembers(self.0.into_iter().map(f).collect())
    }

    pub(crate) fn try_map<U>(
        self,
        f: impl FnMut(T) -> Option<U>,
    ) -> Option<CountedIndexMembers<U, RESERVED>> {
        Some(CountedIndexMembers(
            self.0.into_iter().map(f).collect::<Option<Vec<_>>>()?,
        ))
    }
}

impl<T, const RESERVED: u8> IntoIterator for CountedIndexMembers<T, RESERVED> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_atoms_preserve_direct_and_extended_spellings() {
        for (raw, value) in [(vec![1], 1), (vec![128, 1], 1), (vec![254, 255], 32511)] {
            let atom = CompactIndexAtom::from_wire(value, &raw).unwrap();
            assert_eq!(atom.value(), value);
            assert_eq!(atom.raw(), raw);
        }
        assert!(CompactIndexAtom::read(&[255]).is_none());
        assert!(CompactIndexAtom::read(&[128]).is_none());
        assert!(CompactIndexAtom::from_wire(2, &[1]).is_err());
        assert!(CompactIndexAtom::from_wire(1, &[1, 0]).is_err());
    }

    #[test]
    fn wrapped_compact_atoms_check_the_word_and_derive_the_index() {
        for (raw, value) in [(0x3d80_0000, 0), (0x3d82_5600, 598), (0x3dfe_ff00, 32511)] {
            let atom = WrappedCompactIndex::from_wire(value, raw).unwrap();
            assert_eq!(atom.value(), value);
            assert_eq!(atom.raw(), raw);
        }
        for raw in [0x3c80_0000, 0x3d7f_0000, 0x3dff_0000, 0x3d80_0001] {
            assert!(WrappedCompactIndex::read(raw).is_none());
        }
        assert!(WrappedCompactIndex::from_wire(1, 0x3d80_0000).is_err());
    }

    #[test]
    fn counted_members_reserve_anchor_and_terminator_in_the_byte_count() {
        for len in [1, 253] {
            let members = CountedIndexMembers::<_>::new(vec![0; len]).unwrap();
            assert_eq!(usize::from(members.declared_count()), len + 2);
            assert_eq!(members.try_map(Some).unwrap().as_slice().len(), len);
        }
        for len in [0, 254] {
            assert!(CountedIndexMembers::<_>::new(vec![0; len]).is_err());
        }
    }
    #[test]
    fn counted_members_with_one_owner_allow_254_references() {
        for len in [1, 254] {
            let members = CountedIndexMembers::<_, 1>::new(vec![0; len]).unwrap();
            assert_eq!(usize::from(members.declared_count()), len + 1);
            assert_eq!(members.map(|value| value + 1).into_iter().count(), len);
        }
        for len in [0, 255] {
            assert!(CountedIndexMembers::<_, 1>::new(vec![0; len]).is_err());
        }
    }
}
