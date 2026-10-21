// SPDX-License-Identifier: Apache-2.0
//! Exact face-copy components; recipe atoms retain their ordinary signed encoding.

use crate::om::core_reader::Reader;
use crate::om::scalar::ShiftedBinary64;
use std::num::NonZeroU8;

pub(super) const CLASSES: [&str; 7] = [
    "UGS::ModlDirect::CopyFaceParms",
    "UGS::LABEL_tag_labels",
    "UGS::ModlDirect::FaceRecognition",
    "UGS::ModlUtils::OrientXpress",
    "UGS::ModlDirect::Constrain::ManagerOptions",
    "UGS::CM_RECIPE_tags",
    crate::om::body_write::BODY_IMAGE_CLASS,
];

fn selector(r: &mut Reader<'_>) -> Option<u8> {
    NonZeroU8::new(r.byte()?).map(NonZeroU8::get)
}
fn recipe_atom(r: &mut Reader<'_>) -> Option<()> {
    if r.bytes.get(r.at) == Some(&0) {
        r.byte()?;
    } else {
        ShiftedBinary64::read(r.take(8)?)?;
    }
    Some(())
}

/// Return selectors only after the entire record grammar admits.
pub(super) fn read(bytes: &[u8]) -> Option<[u8; 7]> {
    let mut r = Reader { bytes, at: 0 };
    let owner = selector(&mut r)?;
    r.fixed(b"\0\0\x01\0")?;
    r.feature()?;
    r.fixed(b"\x01")?;
    let count = r.byte()?;
    let entries = count.checked_sub(1).filter(|n| *n > 0)?;
    let label = selector(&mut r)?;
    for ordinal in 2..=count {
        if ordinal != 2 {
            r.fixed(&[label])?;
        }
        r.fixed(&[1, 2, ordinal])?;
        r.feature()?;
    }
    r.fixed(b"\xff\xff")?;
    let face = selector(&mut r)?;
    r.payload()?;
    r.fixed(b"\0")?;
    r.payload()?;
    let orient = selector(&mut r)?;
    r.fixed(b"\x01\x01\x06\xff")?;
    r.payload()?;
    r.fixed(b"\xff\0\0")?;
    let options = selector(&mut r)?;
    r.fixed(b"\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\0\0\x81\x01\x46\x8b\xba\x01\xa3\xae\x30\x01\x01\x02\x04\x02\x01\x02\x01\0\0\0\0\x01\0")?;
    r.payload()?;
    r.fixed(b"\0\0\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0\x01\x02")?;
    r.byte()?;
    r.body()?;
    r.fixed(b"\x97\x75\x01\x02")?;
    let endpoint = selector(&mut r)?;
    r.body()?;
    r.fixed(b"\xff")?;
    let recipe = selector(&mut r)?;
    r.fixed(b"\0")?;
    recipe_atom(&mut r)?;
    recipe_atom(&mut r)?;
    r.fixed(&[1, count])?;
    for _ in 0..entries {
        r.feature()?;
    }
    r.fixed(b"\0\0\x0b\0\xf0\x01\x02\x01")?;
    r.feature()?;
    r.fixed(b"\x01\0\0\0\0\0\0\x0d\x01\x02\x01\0\0\0")?;
    r.feature()?;
    r.fixed(b"\x01\x01\x01\x01\x02")?;
    let triplet = r.take(3)?;
    r.fixed(triplet)?;
    r.fixed(b"\x01\0\0\xff\0\xff")?;
    r.byte()?;
    r.fixed(b"\xc0\x1f\xff\xff\x01\x02\x01")?;
    r.feature()?;
    r.feature()?;
    r.fixed(b"\x01\x03\x02")?;
    r.take(9)?;
    let ordinal = r.feature()?;
    (r.feature()?.raw() == ordinal.raw()).then_some(())?;
    r.feature()?;
    r.fixed(b"\0")?;
    (r.at == bytes.len()).then_some([owner, label, face, orient, options, recipe, endpoint])
}

#[cfg(test)]
mod tests;
