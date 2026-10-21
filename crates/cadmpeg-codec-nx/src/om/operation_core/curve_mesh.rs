// SPDX-License-Identifier: Apache-2.0
//! Two ordered native profile groups, without authored direction assignments.

use crate::om::core_reader::Reader;
use crate::om::scalar::ShiftedBinary64;
use std::num::NonZeroU8;

pub(super) const CLASSES: [&str; 4] = [
    "UGS::FF_CURVE_MESH_PARMS",
    "UGS::MODL_refit_data",
    "UGS::CM_profile",
    crate::om::body_write::BODY_IMAGE_CLASS,
];

fn selector(r: &mut Reader<'_>) -> Option<u8> {
    NonZeroU8::new(r.byte()?).map(NonZeroU8::get)
}

pub(super) fn read(bytes: &[u8]) -> Option<[u8; 4]> {
    let mut r = Reader { bytes, at: 0 };
    let owner = selector(&mut r)?;
    r.fixed(b"\0\0\x01\0")?;
    for _ in 0..3 {
        r.payload()?;
    }
    r.fixed(b"\x01\x05\x04\x01\x01\x01\0\x03\x02\xa0\x5e\x38")?;
    let refit = selector(&mut r)?;
    let mut profile = None;
    for group in 0..2 {
        r.fixed(b"\x01")?;
        let count = r.byte()?.checked_sub(1).filter(|n| *n > 0)?;
        for _ in 0..count {
            let class = selector(&mut r)?;
            (*profile.get_or_insert(class) == class).then_some(())?;
            r.fixed(b"\x01\x02")?;
            r.payload()?;
            if group == 0 {
                r.fixed(b"\x01\x02")?;
            }
            r.fixed(b"\0\0\0\0\0\xff\x01\x02")?;
            r.payload()?;
            r.fixed(if group == 0 { b"\0\x89\x5c" } else { b"\0\x1d" })?;
        }
    }
    r.fixed(b"\0\0\0\0\0\xff\0\xff\x01\0\0\x01\x05\x01\x01\x01\x01\x01\x05\x01\x01\x01\x01\0\x01\x05\xff\xff\xff\xff")?;
    r.fixed(&[refit])?;
    r.fixed(
        b"\x01\x10\x04\x04\x02\x02\xa0\x5e\x38\x08\x08\x02\x02\x01\x01\x01\x01\x02\x02\x01\x05",
    )?;
    let atom = r.take(8)?;
    ShiftedBinary64::read(atom)?;
    r.fixed(b"\x03\x03")?;
    r.fixed(atom)?;
    for _ in 0..3 {
        r.payload()?;
    }
    r.fixed(b"\x01\x01\0\x01\x05\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0\x01\x02")?;
    r.byte()?;
    r.body()?;
    r.fixed(b"\x97\x75\x01\x02")?;
    let endpoint = selector(&mut r)?;
    r.body()?;
    r.fixed(b"\xff\0\0\0\x0b\0\xf0\x01\x02\x01")?;
    r.feature()?;
    r.fixed(b"\x01\0\0\0\0\0\0\x0d\x01\x02\x01\0\0\0")?;
    r.feature()?;
    r.fixed(b"\x01\x01\x01\x01\x02")?;
    let triplet = r.take(3)?;
    r.fixed(triplet)?;
    r.fixed(b"\x01\0\0\xff\0\xff")?;
    r.byte()?;
    r.fixed(b"\xc0\x1f\xff\xff\x01\0\0")?;
    r.feature()?;
    r.feature()?;
    r.fixed(b"\x01\x03\x02")?;
    r.take(9)?;
    let ordinal = r.feature()?;
    (r.feature()?.raw() == ordinal.raw()).then_some(())?;
    r.feature()?;
    r.fixed(b"\0")?;
    (r.at == bytes.len()).then_some([owner, refit, profile?, endpoint])
}

#[cfg(test)]
mod tests;
