// SPDX-License-Identifier: Apache-2.0
//! Exact body-write relation tokens and immutable frame positions.

use super::state_index::StateIndexToken;
use std::ops::Add;

/// Canonical feature indices plus the packed and f1 operation-reference forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BodyWriteIndex(StateIndexToken);

impl BodyWriteIndex {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let token = StateIndexToken::read_at(bytes, 0)?;
        let canonical = matches!(
            (token.value(), token.raw()),
            (0..=0x7f, [_])
                | (0x80..=0xfff, [0x80..=0x8f, _])
                | (0x1000..=0xffff, [0x90, _, _])
                | (_, [0xa0..=0xaf | 0xf1, _, _])
        );
        canonical.then_some(Self(token))
    }
    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let token = Self::read(raw).ok_or("invalid body-write reference token")?;
        if token.value() != value || token.raw().len() != raw.len() {
            return Err("body-write index/raw token: value or width mismatch");
        }
        Ok(token)
    }
    pub(crate) fn value(self) -> u32 {
        self.0.value()
    }
    pub(crate) fn raw(&self) -> &[u8] {
        self.0.raw()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyImageTag {
    Form10,
    Form12,
    Form15,
}

impl BodyImageTag {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Form10 => 0x10,
            Self::Form12 => 0x12,
            Self::Form15 => 0x15,
        }
    }
}

impl TryFrom<u8> for BodyImageTag {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x10 => Ok(Self::Form10),
            0x12 => Ok(Self::Form12),
            0x15 => Ok(Self::Form15),
            _ => Err("endpoint_tag: unsupported body-image tag"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BodyWriteFrame<O> {
    body_identity: u8,
    group_node: BodyWriteIndex,
    endpoint_tag: BodyImageTag,
    body_image: BodyWriteIndex,
    offset: O,
}

impl<O> BodyWriteFrame<O> {
    pub(crate) fn body_identity(&self) -> u8 {
        self.body_identity
    }
    pub(crate) fn group_node(&self) -> BodyWriteIndex {
        self.group_node
    }
    pub(crate) fn endpoint_tag(&self) -> BodyImageTag {
        self.endpoint_tag
    }
    pub(crate) fn body_image(&self) -> BodyWriteIndex {
        self.body_image
    }
    pub(crate) fn byte_len(&self) -> u8 {
        9 + self.group_node.raw().len() as u8 + self.body_image.raw().len() as u8
    }
}

impl<O: Copy + Add<Output = O> + From<u8>> BodyWriteFrame<O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn group_node_offset(&self) -> O {
        self.offset + O::from(3)
    }
    pub(crate) fn body_image_offset(&self) -> O {
        self.offset + O::from(8 + self.group_node.raw().len() as u8)
    }
    pub(crate) fn end_offset(&self) -> O {
        self.offset + O::from(self.byte_len())
    }
}

macro_rules! positioned_frame {
    ($offset:ty) => {
        impl BodyWriteFrame<$offset> {
            pub(crate) fn new(
                body_identity: u8,
                group_node: BodyWriteIndex,
                endpoint_tag: BodyImageTag,
                body_image: BodyWriteIndex,
                offset: $offset,
            ) -> Option<Self> {
                let frame = Self {
                    body_identity,
                    group_node,
                    endpoint_tag,
                    body_image,
                    offset,
                };
                offset.checked_add(<$offset>::from(frame.byte_len()))?;
                Some(frame)
            }
        }
    };
}
positioned_frame!(usize);
positioned_frame!(u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_write_indices_preserve_packed_aliases_and_canonical_feature_widths() {
        for (value, raw) in [
            (0, &[0][..]),
            (128, &[0x80, 0x80][..]),
            (4096, &[0x90, 0x10, 0][..]),
            (0, &[0xa0, 0, 0][..]),
            (0, &[0xf1, 0, 0][..]),
            (0xfffff, &[0xaf, 0xff, 0xff][..]),
        ] {
            let token = BodyWriteIndex::from_wire(value, raw).unwrap();
            assert_eq!(token.value(), value);
            assert_eq!(token.raw(), raw);
        }
        for raw in [
            &[][..],
            &[0xff][..],
            &[0x80, 0][..],
            &[0x90, 0, 0][..],
            &[0x90, 0][..],
            &[0xf0, 0][..],
        ] {
            assert!(BodyWriteIndex::read(raw).is_none());
        }
        assert!(BodyWriteIndex::from_wire(1, &[0]).is_err());
        assert!(BodyWriteIndex::from_wire(0, &[0, 0]).is_err());
    }

    #[test]
    fn body_write_frames_derive_offsets_and_bound_their_complete_end() {
        let group = BodyWriteIndex::from_wire(0, &[0xa0, 0, 0]).unwrap();
        let image = BodyWriteIndex::from_wire(0, &[0xf1, 0, 0]).unwrap();
        let frame =
            BodyWriteFrame::<usize>::new(255, group, BodyImageTag::Form15, image, 100).unwrap();
        assert_eq!(frame.group_node_offset(), 103);
        assert_eq!(frame.body_image_offset(), 111);
        assert_eq!(frame.end_offset(), 115);
        assert_eq!(frame.byte_len(), 15);
        assert!(BodyWriteFrame::<usize>::new(
            0,
            group,
            BodyImageTag::Form10,
            image,
            usize::MAX - 15
        )
        .is_some());
        assert!(BodyWriteFrame::<usize>::new(
            0,
            group,
            BodyImageTag::Form10,
            image,
            usize::MAX - 14
        )
        .is_none());
        assert!(
            BodyWriteFrame::<u64>::new(0, group, BodyImageTag::Form12, image, u64::MAX - 15)
                .is_some()
        );
        assert!(
            BodyWriteFrame::<u64>::new(0, group, BodyImageTag::Form12, image, u64::MAX - 14)
                .is_none()
        );
        for tag in [0x10, 0x12, 0x15] {
            assert_eq!(BodyImageTag::try_from(tag).unwrap().code(), tag);
        }
        for tag in [0, 0x11, 0x13, 0xff] {
            assert!(BodyImageTag::try_from(tag).is_err());
        }
    }
}
