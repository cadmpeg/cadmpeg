// SPDX-License-Identifier: Apache-2.0
//! Bounded reference and result-frame reads shared by complete operation cores.

use super::body_write::BodyWriteIndex;
use super::reference_index::{FeatureReferenceToken, PayloadIndexToken};
use std::num::NonZeroU8;

pub(super) struct Reader<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) at: usize,
}
impl<'a> Reader<'a> {
    pub(super) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let bytes = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }
    pub(super) fn byte(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }
    pub(super) fn fixed(&mut self, expected: &[u8]) -> Option<()> {
        (self.take(expected.len())? == expected).then_some(())
    }
    pub(super) fn feature(&mut self) -> Option<FeatureReferenceToken> {
        let token = FeatureReferenceToken::read(self.bytes.get(self.at..)?)?;
        self.at += token.raw().len();
        Some(token)
    }
    pub(super) fn payload(&mut self) -> Option<PayloadIndexToken> {
        let token = PayloadIndexToken::read(self.bytes.get(self.at..)?)?;
        self.at += token.raw().len();
        Some(token)
    }
    pub(super) fn body(&mut self) -> Option<BodyWriteIndex> {
        let token = BodyWriteIndex::read(self.bytes.get(self.at..)?)?;
        self.at += token.raw().len();
        Some(token)
    }
    pub(super) fn image(&mut self) -> Option<(NonZeroU8, BodyWriteIndex, usize)> {
        let class = NonZeroU8::new(self.byte()?)?;
        let offset = self.at;
        let image = self.body()?;
        self.fixed(b"\xff\0\0\0\x0b\0\xf0\x01\x02\x02")?;
        self.feature()?;
        self.fixed(b"\x01")?;
        Some((class, image, offset))
    }
}
