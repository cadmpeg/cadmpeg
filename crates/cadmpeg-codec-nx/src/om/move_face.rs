// SPDX-License-Identifier: Apache-2.0
//! Complete native face-motion cores and ordered result lists, without authored roles.

use super::body_write::{BodyWriteFrame, BodyWriteIndex, BODY_IMAGE_CLASS};
use super::operation_record::OperationPayload;
use super::reference_index::{FeatureReferenceToken, PayloadIndexToken};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;

const CLASSES: [&str; 3] = [
    "UGS::ModlDirectDB::AdmMoveFaceParms",
    "UGS::ModlDirect::FaceRecognition",
    "UGS::ModlUtils::OrientXpress",
];

/// A complete unresolved core. Wire admission replays its entire grammar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct MoveFaceCore(
    #[serde(deserialize_with = "deserialize_core")] NativeBytes<Vec<u8>>,
);

fn deserialize_core<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    let bytes = NativeBytes::<Vec<u8>>::deserialize(d)?;
    let view = CoreView::parse(&bytes)
        .ok_or_else(|| serde::de::Error::custom("move_face_core: invalid complete core"))?;
    if view.byte_len != bytes.len() {
        return Err(serde::de::Error::custom(
            "move_face_core: bytes after terminal frame",
        ));
    }
    Ok(bytes)
}

impl MoveFaceCore {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let bytes = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }
    fn byte(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }
    fn fixed(&mut self, expected: &[u8]) -> Option<()> {
        (self.take(expected.len())? == expected).then_some(())
    }
    fn feature(&mut self) -> Option<FeatureReferenceToken> {
        let token = FeatureReferenceToken::read(self.bytes.get(self.at..)?)?;
        self.at += token.raw().len();
        Some(token)
    }
    fn payload(&mut self) -> Option<PayloadIndexToken> {
        let token = PayloadIndexToken::read(self.bytes.get(self.at..)?)?;
        self.at += token.raw().len();
        Some(token)
    }
    fn body(&mut self) -> Option<BodyWriteIndex> {
        let token = BodyWriteIndex::read(self.bytes.get(self.at..)?)?;
        self.at += token.raw().len();
        Some(token)
    }
    fn image(&mut self) -> Option<(NonZeroU8, BodyWriteIndex, usize)> {
        let class = NonZeroU8::new(self.byte()?)?;
        let offset = self.at;
        let image = self.body()?;
        self.fixed(b"\xff\0\0\0\x0b\0\xf0\x01\x02\x02")?;
        self.feature()?;
        self.fixed(b"\x01")?;
        Some((class, image, offset))
    }
}

/// Borrowed positions are valid only after every component, row and suffix admits.
pub(crate) struct CoreView<'a> {
    bytes: &'a [u8],
    selectors: [u8; 3],
    relation_start: usize,
    groups_start: usize,
    images_start: usize,
    entries: u8,
    pub(crate) byte_len: usize,
    zero_terminated: bool,
}
impl<'a> CoreView<'a> {
    pub(crate) fn retain(&self, ctx: &DecodeContext<'_>) -> Result<MoveFaceCore, CodecError> {
        let bytes = self
            .bytes
            .get(..self.byte_len)
            .ok_or_else(|| ctx.refuse_codec_limit("NX move-face core span", 0, 1))?;
        Ok(MoveFaceCore(
            ctx.copy_slice(bytes, "NX bounded move-face core")?.into(),
        ))
    }
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        let mut r = Reader { bytes, at: 0 };
        let owner = r.byte()?;
        r.fixed(b"\0\0\x01\0")?;
        r.feature()?;
        r.fixed(b"\0\xff\xff")?;
        let face = r.byte()?;
        r.payload()?;
        r.fixed(b"\0")?;
        r.payload()?;
        let orient = r.byte()?;
        r.fixed(b"\x01\x01")?;
        let control = r.byte()?;
        match control {
            5 => (),
            6 => r.fixed(b"\xff")?,
            _ => return None,
        }
        r.payload()?;
        r.fixed(b"\xff")?;
        if control == 5 {
            r.fixed(b"\xff")?;
        }
        r.fixed(b"\0\0\0")?;
        r.payload()?;
        r.fixed(b"\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0")?;
        let relation_start = r.at;
        r.fixed(b"\x01")?;
        let count = r.byte()?;
        let entries = count.checked_sub(1).filter(|n| *n > 0)?;
        let groups_start = r.at;
        for _ in 0..entries {
            r.byte()?;
            r.body()?;
            r.fixed(b"\x97\x75")?;
        }
        r.fixed(&[1, count])?;
        let images_start = r.at;
        for _ in 0..entries {
            r.image()?;
        }
        r.fixed(b"\0\0\0\0\0\0\x0d\x01\x02\x01\0\0\0")?;
        r.feature()?;
        r.fixed(b"\x01\x01\x01\x01\x02")?;
        let duplicated = r.take(3)?;
        r.fixed(duplicated)?;
        r.fixed(b"\x01\0\0\xff\0\xff")?;
        r.byte()?;
        r.fixed(b"\xc0\x1f\xff\xff\x01\0\0")?;
        r.feature()?;
        r.feature()?;
        r.fixed(b"\x01\x03\x02")?;
        r.take(9)?;
        let ordinal = r.feature()?;
        let duplicate = r.feature()?;
        (ordinal.raw() == duplicate.raw()).then_some(())?;
        r.feature()?;
        let zero_terminated = bytes.get(r.at) == Some(&0);
        if zero_terminated {
            r.at += 1;
        }
        [owner, face, orient].iter().all(|s| *s > 0).then_some(())?;
        Some(Self {
            bytes,
            selectors: [owner, face, orient],
            relation_start,
            groups_start,
            images_start,
            entries,
            byte_len: r.at,
            zero_terminated,
        })
    }
    pub(crate) fn read(
        record: OperationPayload<'a>,
        classes: &[super::TypeDefinition<'_>],
    ) -> Option<Self> {
        if record.name() != "MOVE_FACE" {
            return None;
        }
        let core = Self::parse(record.payload())?;
        for (selector, expected) in core.selectors.into_iter().zip(CLASSES) {
            (classes.get(usize::from(selector) - 1)?.name == expected).then_some(())?;
        }
        let mut images = Reader {
            bytes: core.bytes,
            at: core.images_start,
        };
        for _ in 0..core.entries {
            let (class, _, _) = images.image()?;
            (classes.get(usize::from(class.get()) - 1)?.name == BODY_IMAGE_CLASS).then_some(())?;
        }
        if core.byte_len != core.bytes.len() {
            if core.zero_terminated {
                return None;
            }
            let selector = usize::from(*core.bytes.get(core.byte_len)?).checked_sub(1)?;
            (classes.get(selector)?.name == "UGS::CM_object").then_some(())?;
        }
        Some(core)
    }
    pub(crate) fn writes(
        &self,
        ctx: &DecodeContext<'_>,
        base: usize,
    ) -> Result<Vec<BodyWriteFrame<usize>>, CodecError> {
        let mut groups = Reader {
            bytes: self.bytes,
            at: self.groups_start,
        };
        let mut images = Reader {
            bytes: self.bytes,
            at: self.images_start,
        };
        let mut writes = Vec::new();
        for _ in 0..self.entries {
            let body_at = groups.at;
            let parsed = (|| {
                let identity = groups.byte()?;
                let group = groups.body()?;
                groups.fixed(b"\x97\x75")?;
                let (class, image, image_at) = images.image()?;
                if self.entries == 1 {
                    BodyWriteFrame::<usize>::new(
                        identity,
                        group,
                        class,
                        image,
                        base.checked_add(self.relation_start)?,
                    )
                } else {
                    BodyWriteFrame::<usize>::listed(
                        identity,
                        group,
                        class,
                        image,
                        base.checked_add(body_at)?,
                        base.checked_add(image_at)?,
                    )
                }
            })();
            let write = parsed
                .ok_or_else(|| ctx.refuse_codec_limit("NX move-face result positions", 0, 1))?;
            ctx.reserve_vec(&mut writes, 1, "NX ordered move-face body writes")?;
            writes.push(write);
        }
        Ok(writes)
    }
}

pub(crate) fn core_for_decode<'a>(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'a>,
    classes: &[super::TypeDefinition<'_>],
) -> Result<Option<CoreView<'a>>, CodecError> {
    if record.name() != "MOVE_FACE" {
        return Ok(None);
    }
    ctx.charge_work(
        u64_from_index(record.payload().len()),
        "NX move-face complete core",
    )?;
    Ok(CoreView::read(record, classes))
}

#[cfg(test)]
mod tests;
