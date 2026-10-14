// SPDX-License-Identifier: Apache-2.0
//! Exact body-write relation tokens and immutable frame positions.

use super::state_index::StateIndexToken;
use std::num::NonZeroU8;
use std::ops::Add;

pub(crate) const BODY_IMAGE_CLASS: &str = "UGS::CM_BREF_DATA";

/// Canonical feature indices plus the packed and f1 operation-reference forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BodyWriteIndex(StateIndexToken);

impl BodyWriteIndex {
    pub(super) fn read(bytes: &[u8]) -> Option<Self> {
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
pub(crate) struct BodyWriteFrame<O> {
    body_identity: u8,
    group_node: BodyWriteIndex,
    endpoint_class_index: NonZeroU8,
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
    pub(crate) fn endpoint_class_index(&self) -> NonZeroU8 {
        self.endpoint_class_index
    }
    /// Resolve the selector in the complete owning section registry.
    pub(crate) fn selects_body_image_class(&self, classes: &[super::TypeDefinition<'_>]) -> bool {
        classes
            .get(usize::from(self.endpoint_class_index.get()) - 1)
            .is_some_and(|class| class.name == BODY_IMAGE_CLASS)
    }
    pub(crate) fn body_image(&self) -> BodyWriteIndex {
        self.body_image
    }
    pub(crate) fn byte_len(&self) -> u8 {
        9 + self.group_node.0.byte_len() + self.body_image.0.byte_len()
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
        self.offset + O::from(8 + self.group_node.0.byte_len())
    }
    pub(super) fn end_offset(&self) -> O {
        self.offset + O::from(self.byte_len())
    }
}

macro_rules! positioned_frame {
    ($offset:ty) => {
        impl BodyWriteFrame<$offset> {
            pub(crate) fn new(
                body_identity: u8,
                group_node: BodyWriteIndex,
                endpoint_class_index: NonZeroU8,
                body_image: BodyWriteIndex,
                offset: $offset,
            ) -> Option<Self> {
                let frame = Self {
                    body_identity,
                    group_node,
                    endpoint_class_index,
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
    use super::{BodyWriteFrame, BodyWriteIndex};
    use std::num::NonZeroU8;

    #[test]
    fn body_image_class_indices_resolve_the_one_based_owner_registry() {
        let classes = [
            super::super::TypeDefinition {
                offset: 10,
                name: "UGS::OTHER",
                registry_tail: &[],
            },
            super::super::TypeDefinition {
                offset: 20,
                name: "UGS::CM_BREF_DATA",
                registry_tail: &[],
            },
        ];
        let object = BodyWriteIndex::from_wire(1, &[1]).unwrap();
        for (index, admitted) in [(1, false), (2, true), (3, false), (255, false)] {
            let frame =
                BodyWriteFrame::<usize>::new(11, object, NonZeroU8::new(index).unwrap(), object, 0)
                    .unwrap();
            assert_eq!(frame.selects_body_image_class(&classes), admitted);
            assert!(!frame.selects_body_image_class(&[]));
        }
    }

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
        let frame = BodyWriteFrame::<usize>::new(
            255,
            group,
            std::num::NonZeroU8::new(0x15).unwrap(),
            image,
            100,
        )
        .unwrap();
        assert_eq!(frame.group_node_offset(), 103);
        assert_eq!(frame.body_image_offset(), 111);
        assert_eq!(frame.end_offset(), 115);
        assert_eq!(frame.byte_len(), 15);
        assert!(BodyWriteFrame::<usize>::new(
            0,
            group,
            std::num::NonZeroU8::new(0x10).unwrap(),
            image,
            usize::MAX - 15
        )
        .is_some());
        assert!(BodyWriteFrame::<usize>::new(
            0,
            group,
            std::num::NonZeroU8::new(0x10).unwrap(),
            image,
            usize::MAX - 14
        )
        .is_none());
        assert!(BodyWriteFrame::<u64>::new(
            0,
            group,
            std::num::NonZeroU8::new(0x12).unwrap(),
            image,
            u64::MAX - 15
        )
        .is_some());
        assert!(BodyWriteFrame::<u64>::new(
            0,
            group,
            std::num::NonZeroU8::new(0x12).unwrap(),
            image,
            u64::MAX - 14
        )
        .is_none());
        assert!(NonZeroU8::new(0).is_none());
        for index in 1..=u8::MAX {
            assert_eq!(NonZeroU8::new(index).unwrap().get(), index);
        }
    }
}
