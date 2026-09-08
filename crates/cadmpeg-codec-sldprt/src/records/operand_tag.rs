// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};

/// A native operand tag outside the absent and dedicated reference tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub(crate) struct NativeOperandTag(u16);

impl NativeOperandTag {
    pub(crate) fn value(self) -> u16 {
        self.0
    }
    pub(crate) const TAG_80AC: Self = Self(0x80ac);
    pub(crate) const TAG_80CC: Self = Self(0x80cc);
    pub(crate) const TAG_80D4: Self = Self(0x80d4);
    pub(crate) const TAG_80D5: Self = Self(0x80d5);
    pub(crate) const TAG_8100: Self = Self(0x8100);
    pub(crate) const TAG_810F: Self = Self(0x810f);
    pub(crate) const TAG_8138: Self = Self(0x8138);
    pub(crate) const TAG_814C: Self = Self(0x814c);
    pub(crate) const TAG_8152: Self = Self(0x8152);
    pub(crate) const TAG_81B2: Self = Self(0x81b2);
    pub(crate) const TAG_81DD: Self = Self(0x81dd);
    pub(crate) const TAG_81E7: Self = Self(0x81e7);
    pub(crate) const TAG_820F: Self = Self(0x820f);
    pub(crate) const TAG_836E: Self = Self(0x836e);
    pub(crate) const TAG_837B: Self = Self(0x837b);
    pub(crate) const TAG_8386: Self = Self(0x8386);
    pub(crate) const TAG_83FE: Self = Self(0x83fe);
    pub(crate) const TAG_8AB6: Self = Self(0x8ab6);
    pub(crate) const TAG_8DCB: Self = Self(0x8dcb);
    pub(crate) const TAG_8DDA: Self = Self(0x8dda);
    pub(crate) const TAG_929D: Self = Self(0x929d);
    pub(crate) const TAG_BC7C: Self = Self(0xbc7c);
    pub(crate) const TAG_BC87: Self = Self(0xbc87);
    pub(crate) const TAG_BD69: Self = Self(0xbd69);
}

#[cfg(test)]
impl NativeOperandTag {
    pub(crate) const TAG_1234: Self = Self(0x1234);
    pub(crate) const TAG_69BD: Self = Self(0x69bd);
    pub(crate) const TAG_80DD: Self = Self(0x80dd);
    pub(crate) const TAG_80F7: Self = Self(0x80f7);
    pub(crate) const TAG_80FE: Self = Self(0x80fe);
    pub(crate) const TAG_8124: Self = Self(0x8124);
    pub(crate) const TAG_8129: Self = Self(0x8129);
    pub(crate) const TAG_812A: Self = Self(0x812a);
    pub(crate) const TAG_812E: Self = Self(0x812e);
    pub(crate) const TAG_81D5: Self = Self(0x81d5);
    pub(crate) const TAG_8207: Self = Self(0x8207);
    pub(crate) const TAG_825C: Self = Self(0x825c);
    pub(crate) const TAG_8263: Self = Self(0x8263);
    pub(crate) const TAG_829A: Self = Self(0x829a);
    pub(crate) const TAG_8452: Self = Self(0x8452);
    pub(crate) const TAG_88E7: Self = Self(0x88e7);
    pub(crate) const TAG_8C44: Self = Self(0x8c44);
}

impl TryFrom<u16> for NativeOperandTag {
    type Error = &'static str;
    fn try_from(tag: u16) -> Result<Self, Self::Error> {
        match tag {
            0 | 0xffff | 0x80d6 | 0x80e1 => Err("native operand tag is reserved"),
            tag => Ok(Self(tag)),
        }
    }
}

impl From<NativeOperandTag> for u16 {
    fn from(tag: NativeOperandTag) -> Self {
        tag.value()
    }
}
