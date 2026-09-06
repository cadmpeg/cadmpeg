// SPDX-License-Identifier: Apache-2.0
//! Checked common-frame prefixes, repeated-ordinal suffixes and source positions.

use super::compact::CompactIndexAtom;
use super::reference_index::CanonicalFeatureReferenceToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommonFramePrefix([CompactIndexAtom; 3]);

impl CommonFramePrefix {
    pub(crate) fn from_wire(indices: [u32; 3], raw: &[Vec<u8>; 3], marker: [u8; 3]) -> Result<Self, &'static str> {
        let [a, b, c] = [0, 1, 2].map(|slot| CompactIndexAtom::from_wire(indices[slot], &raw[slot])
            .map_err(|_| "indices/raw_indices: invalid compact token"));
        let prefix = Self([a?, b?, c?]);
        let widths = prefix.0.map(|token| token.raw().len());
        if !matches!(widths, [1, 1, 1] | [1, 2, 2]) || prefix.marker() != marker {
            return Err("indices/raw_indices/marker: inconsistent common-frame prefix");
        }
        Ok(prefix)
    }

    pub(crate) fn read(bytes: &[u8], marker: [u8; 3]) -> Option<Self> {
        let widths = match marker { [1, 3, 2] => [1, 2, 2], [1, 1, 1] => [1, 1, 1], _ => return None };
        let marker_at = widths.iter().sum::<usize>();
        (bytes.get(marker_at..marker_at + 3) == Some(&marker)).then_some(())?;
        let first = CompactIndexAtom::read(bytes)?;
        let second = CompactIndexAtom::read(bytes.get(widths[0]..)?)?;
        let third = CompactIndexAtom::read(bytes.get(widths[0] + widths[1]..)?)?;
        let tokens = [first, second, third];
        (tokens.map(|token| token.raw().len()) == widths).then_some(Self(tokens))
    }

    pub(crate) fn indices(self) -> [u32; 3] { self.0.map(CompactIndexAtom::value) }
    pub(crate) fn raw_indices(self) -> [Vec<u8>; 3] { self.0.map(|token| token.raw().to_vec()) }
    pub(crate) fn marker(self) -> [u8; 3] { if self.0[1].raw().len() == 1 { [1, 1, 1] } else { [1, 3, 2] } }
    pub(crate) fn byte_len(self) -> usize { self.0.iter().map(|token| token.raw().len()).sum::<usize>() + 3 }
    fn index_offsets(self) -> [usize; 3] { [0, self.0[0].raw().len(), self.0[0].raw().len() + self.0[1].raw().len()] }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommonFrameSuffix<T = ()> {
    local_ordinal: CanonicalFeatureReferenceToken,
    object: Option<(CanonicalFeatureReferenceToken, T)>,
}

impl CommonFrameSuffix {
    pub(crate) fn from_wire(local_ordinal: u32, raw_local_ordinal: &[u8], object_index: Option<u32>, raw_object_index: &[u8]) -> Result<Self, &'static str> {
        let local_ordinal = CanonicalFeatureReferenceToken::from_wire(local_ordinal, raw_local_ordinal)
            .map_err(|_| "local_ordinal/raw_local_ordinal: invalid canonical token")?;
        let object = match object_index {
            None if raw_object_index == [0xff] => None,
            None => return Err("object_index/raw_object_index: null requires ff"),
            Some(value) => Some(CanonicalFeatureReferenceToken::from_wire(value, raw_object_index)
                .map_err(|_| "object_index/raw_object_index: invalid canonical token")?),
        };
        Ok(Self { local_ordinal, object: object.map(|token| (token, ())) })
    }

    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let local_ordinal = CanonicalFeatureReferenceToken::read(bytes)?;
        let width = local_ordinal.raw().len();
        (bytes.get(width..2 * width) == Some(local_ordinal.raw())).then_some(())?;
        let object_bytes = bytes.get(2 * width..)?;
        let object = if *object_bytes.first()? == 0xff { None }
            else { Some(CanonicalFeatureReferenceToken::read(object_bytes)?) };
        let suffix = Self { local_ordinal, object: object.map(|token| (token, ())) };
        (bytes.get(suffix.byte_len() - 1) == Some(&0)).then_some(suffix)
    }

    pub(crate) fn with_target<T>(self, target: Option<T>) -> Result<CommonFrameSuffix<Option<T>>, &'static str> {
        if self.object.is_none() && target.is_some() { return Err("data_block requires a non-null object_index"); }
        Ok(self.map_target(|_, ()| target))
    }
}

impl<T> CommonFrameSuffix<T> {
    pub(crate) fn map_target<U>(self, map: impl FnOnce(u32, T) -> U) -> CommonFrameSuffix<U> {
        CommonFrameSuffix { local_ordinal: self.local_ordinal,
            object: self.object.map(|(token, target)| (token, map(token.value(), target))) }
    }
    pub(crate) fn target(&self) -> Option<&T> { self.object.as_ref().map(|(_, target)| target) }
    pub(crate) fn local_ordinal(&self) -> u32 { self.local_ordinal.value() }
    pub(crate) fn raw_local_ordinal(&self) -> &[u8] { self.local_ordinal.raw() }
    pub(crate) fn object_index(&self) -> Option<u32> { self.object.as_ref().map(|(token, _)| token.value()) }
    pub(crate) fn raw_object_index(&self) -> &[u8] { self.object.as_ref().map_or(&[0xff], |(token, _)| token.raw()) }
    fn object_offset(&self) -> usize { 2 * self.local_ordinal.raw().len() }
    pub(crate) fn byte_len(&self) -> usize { self.object_offset() + self.raw_object_index().len() + 1 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommonFrame<O, T = ()> {
    prefix: CommonFramePrefix,
    state: [u8; 8],
    suffix: CommonFrameSuffix<T>,
    offset: O,
}

impl<O, T> CommonFrame<O, T> {
    pub(crate) fn prefix(&self) -> CommonFramePrefix { self.prefix }
    pub(crate) fn state(&self) -> [u8; 8] { self.state }
    pub(crate) fn suffix(&self) -> &CommonFrameSuffix<T> { &self.suffix }
    pub(crate) fn byte_len(&self) -> usize { self.prefix.byte_len() + 8 + self.suffix.byte_len() }
    pub(crate) fn legacy_inactive_modules(&self) -> Option<bool> { boolean(self.state[3]) }
    pub(crate) fn modifies_parasolid_data(&self) -> Option<bool> { boolean(self.state[4]) }
    pub(crate) fn split_tracking_data(&self) -> [u8; 2] { [self.state[5], self.state[6]] }
    pub(crate) fn group_count(&self) -> u8 { self.state[7] }
}

fn boolean(value: u8) -> Option<bool> { match value { 0 => Some(false), 1 => Some(true), _ => None } }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalFrame<O, T = ()> {
    suffix: CommonFrameSuffix<T>,
    offset: O,
}

impl<O, T> TerminalFrame<O, T> {
    pub(crate) fn suffix(&self) -> &CommonFrameSuffix<T> { &self.suffix }
}

macro_rules! frame_positions {
    ($offset:ty) => {
        impl<T> CommonFrame<$offset, T> {
            pub(crate) fn new(prefix: CommonFramePrefix, state: [u8; 8], suffix: CommonFrameSuffix<T>, offset: $offset) -> Option<Self> {
                let frame = Self { prefix, state, suffix, offset };
                offset.checked_add(frame.byte_len() as $offset)?;
                Some(frame)
            }
            pub(crate) fn offset(&self) -> $offset { self.offset }
            pub(crate) fn state_offset(&self) -> $offset { self.offset + self.prefix.byte_len() as $offset }
            pub(crate) fn local_ordinal_offset(&self) -> $offset { self.state_offset() + 8 }
        }
        impl<T> TerminalFrame<$offset, T> {
            pub(crate) fn new(suffix: CommonFrameSuffix<T>, offset: $offset) -> Option<Self> {
                offset.checked_add(suffix.byte_len() as $offset)?;
                Some(Self { suffix, offset })
            }
            pub(crate) fn offset(&self) -> $offset { self.offset }
        }
    };
}
frame_positions!(usize);
frame_positions!(u64);

impl<T> CommonFrame<usize, T> {
    pub(crate) fn end_offset(&self) -> usize { self.offset + self.byte_len() }
}

impl<T> CommonFrame<u64, T> {
    pub(crate) fn index_offsets(&self) -> [u64; 3] { self.prefix.index_offsets().map(|offset| self.offset + offset as u64) }
    pub(crate) fn object_index_offset(&self) -> u64 { self.local_ordinal_offset() + self.suffix.object_offset() as u64 }
}

impl<T> TerminalFrame<usize, T> {
    pub(crate) fn end_offset(&self) -> usize { self.offset + self.suffix.byte_len() }
}

impl<T> TerminalFrame<u64, T> {
    pub(crate) fn object_index_offset(&self) -> u64 { self.offset + self.suffix.object_offset() as u64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_requires_exact_repetition_canonical_indices_and_terminator() {
        for raw in [&[1, 1, 0xff, 0][..], &[0x80, 0x80, 0x80, 0x80, 1, 0][..],
            &[0x90, 0x10, 0, 0x90, 0x10, 0, 0xff, 0][..]] {
            let suffix = CommonFrameSuffix::read(raw).unwrap();
            assert_eq!(suffix.byte_len(), raw.len());
            assert_eq!(&raw[..suffix.raw_local_ordinal().len()], suffix.raw_local_ordinal());
        }
        for raw in [&[1, 2, 0xff, 0][..], &[1, 0x80, 1, 0xff, 0][..], &[0xff, 0xff, 0xff, 0][..],
            &[0x80, 1, 0x80, 1, 0xff, 0][..], &[1, 1, 0x80, 1, 0][..], &[1, 1, 0xff][..], &[1, 1, 0xff, 1][..]] {
            assert!(CommonFrameSuffix::read(raw).is_none());
        }
    }

    #[test]
    fn prefix_preserves_compact_grammar_and_rejects_marker_width_conflicts() {
        let prefix = CommonFramePrefix::read(&[0, 0x90, 1, 0x80, 0, 1, 3, 2], [1, 3, 2]).unwrap();
        assert_eq!(prefix.indices(), [0, 4097, 0]);
        assert_eq!(prefix.raw_indices(), [vec![0], vec![0x90, 1], vec![0x80, 0]]);
        assert!(CommonFramePrefix::from_wire([0, 4097, 0], &prefix.raw_indices(), [1, 1, 1]).is_err());
        assert!(CommonFramePrefix::read(&[0, 0xff, 1, 0x80, 0, 1, 3, 2], [1, 3, 2]).is_none());
        assert!(CommonFramePrefix::from_wire([0; 3], &std::array::from_fn(|_| vec![0]), [1, 3, 2]).is_err());
    }

    #[test]
    fn checked_positions_admit_exact_end_boundary_and_reject_overflow() {
        let prefix = CommonFramePrefix::read(&[0, 0, 0, 1, 1, 1], [1, 1, 1]).unwrap();
        let suffix = CommonFrameSuffix::read(&[1, 1, 0xff, 0]).unwrap();
        let common = CommonFrame::<usize>::new(prefix, [0; 8], suffix, usize::MAX - 18).unwrap();
        assert_eq!(common.end_offset(), usize::MAX);
        assert!(CommonFrame::<usize>::new(prefix, [0; 8], suffix, usize::MAX - 17).is_none());
        assert!(CommonFrame::<u64>::new(prefix, [0; 8], suffix, u64::MAX - 18).is_some());
        assert!(CommonFrame::<u64>::new(prefix, [0; 8], suffix, u64::MAX - 17).is_none());
        let terminal = TerminalFrame::<usize>::new(suffix, usize::MAX - 4).unwrap();
        assert_eq!(terminal.end_offset(), usize::MAX);
        assert!(TerminalFrame::<usize>::new(suffix, usize::MAX - 3).is_none());
        assert!(TerminalFrame::<u64>::new(suffix, u64::MAX - 4).is_some());
        assert!(TerminalFrame::<u64>::new(suffix, u64::MAX - 3).is_none());
    }

    #[test]
    fn operation_common_frame_types_the_parasolid_modification_field() {
        let mut state = [0; 8];
        assert_eq!(common_frame_with_state(state).modifies_parasolid_data(), Some(false));
        state[4] = 1;
        assert_eq!(common_frame_with_state(state).modifies_parasolid_data(), Some(true));
        state[4] = 2;
        assert_eq!(common_frame_with_state(state).modifies_parasolid_data(), None);
    }

    #[test]
    fn operation_common_frame_retains_the_split_tracking_data_field() {
        assert_eq!(
            common_frame_with_state([1, 2, 3, 0, 1, 0x56, 0xa9, 7]).split_tracking_data(),
            [0x56, 0xa9]
        );
    }

    #[test]
    fn operation_common_frame_types_the_legacy_inactive_modules_field() {
        let mut state = [0; 8];
        assert_eq!(common_frame_with_state(state).legacy_inactive_modules(), Some(false));
        state[3] = 1;
        assert_eq!(common_frame_with_state(state).legacy_inactive_modules(), Some(true));
        state[3] = 2;
        assert_eq!(common_frame_with_state(state).legacy_inactive_modules(), None);
    }

    fn common_frame_with_state(state: [u8; 8]) -> crate::om::common_frame::CommonFrame<u64> {
        crate::om::common_frame::CommonFrame::<u64>::new(
            crate::om::common_frame::CommonFramePrefix::from_wire([0; 3], &[vec![0], vec![0], vec![0]], [1, 1, 1]).unwrap(),
            state, crate::om::common_frame::CommonFrameSuffix::from_wire(0, &[0], None, &[0xff]).unwrap(), 0).unwrap()
    }
}
