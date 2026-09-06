// SPDX-License-Identifier: Apache-2.0
//! Exact non-null compact indices and bounded counted-lane members.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding { Direct(u8), Extended([u8; 2]) }

/// Exact compact-index encoding, excluding the `ff` null token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactIndexAtom(Encoding);

impl CompactIndexAtom {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        match bytes.first().copied()? {
            value @ 0..=0x7f => Some(Self(Encoding::Direct(value))),
            high @ 0x80..=0xfe => Some(Self(Encoding::Extended([high, *bytes.get(1)?]))),
            _ => None,
        }
    }

    pub(crate) fn value(self) -> u32 {
        match self.0 {
            Encoding::Direct(value) => u32::from(value),
            Encoding::Extended([high, low]) => u32::from(high - 0x80) * 256 + u32::from(low),
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match &self.0 {
            Encoding::Direct(value) => std::slice::from_ref(value),
            Encoding::Extended(raw) => raw,
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

/// Extended compact index inside a `3d high low 00` word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WrappedCompactIndex([u8; 2]);

impl WrappedCompactIndex {
    pub(crate) fn read(raw: u32) -> Option<Self> {
        let [marker, high, low, terminal] = raw.to_be_bytes();
        (marker == 0x3d && terminal == 0 && (0x80..=0xfe).contains(&high))
            .then_some(Self([high, low]))
    }

    pub(crate) fn value(self) -> u32 {
        u32::from(self.0[0] - 0x80) * 256 + u32::from(self.0[1])
    }

    pub(crate) fn raw(self) -> u32 {
        0x3d00_0000 | (u32::from(self.0[0]) << 16) | (u32::from(self.0[1]) << 8)
    }

    pub(crate) fn from_wire(value: u32, raw: u32) -> Result<Self, &'static str> {
        let atom = Self::read(raw).ok_or("invalid wrapped compact index")?;
        if atom.value() != value { return Err("index/raw word: value mismatch"); }
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
        Some(Self { atom: CompactIndexAtom::read(bytes.get(offset..)?)?, offset })
    }
    pub(crate) fn read_array<const N: usize>(bytes: &[u8], at: &mut usize) -> Option<[Self; N]> {
        (0..N).map(|_| {
            let token = Self::read(bytes, *at)?;
            *at += token.atom.raw().len();
            Some(token)
        }).collect::<Option<Vec<_>>>()?.try_into().ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NullableCompactIndex {
    pub(crate) atom: Option<CompactIndexAtom>,
    pub(crate) offset: usize,
}

impl NullableCompactIndex {
    pub(crate) fn read(bytes: &[u8], offset: usize) -> Option<Self> {
        let atom = if *bytes.get(offset)? == 0xff { None }
            else { Some(CompactIndexAtom::read(bytes.get(offset..)?)?) };
        Some(Self { atom, offset })
    }

    pub(crate) fn raw(&self) -> &[u8] {
        self.atom.as_ref().map_or(&[0xff], CompactIndexAtom::raw)
    }
}

/// Nonempty members of a byte-counted lane with an anchor and a terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CountedIndexMembers<T>(Vec<T>);

impl<T> CountedIndexMembers<T> {
    pub(crate) fn new(members: Vec<T>) -> Result<Self, &'static str> {
        if !(1..=253).contains(&members.len()) {
            return Err("members: must contain 1 through 253 entries");
        }
        Ok(Self(members))
    }

    pub(crate) fn declared_count(&self) -> u8 { (self.0.len() + 2) as u8 }

    pub(crate) fn as_slice(&self) -> &[T] { &self.0 }

    pub(crate) fn map<U>(self, f: impl FnMut(T) -> U) -> CountedIndexMembers<U> {
        CountedIndexMembers(self.0.into_iter().map(f).collect())
    }

    pub(crate) fn try_map<U>(self, f: impl FnMut(T) -> Option<U>) -> Option<CountedIndexMembers<U>> {
        Some(CountedIndexMembers(self.0.into_iter().map(f).collect::<Option<Vec<_>>>()?))
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
            let members = CountedIndexMembers::new(vec![0; len]).unwrap();
            assert_eq!(usize::from(members.declared_count()), len + 2);
            assert_eq!(members.try_map(Some).unwrap().as_slice().len(), len);
        }
        for len in [0, 254] {
            assert!(CountedIndexMembers::new(vec![0; len]).is_err());
        }
    }
}
