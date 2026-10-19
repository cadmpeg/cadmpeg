// SPDX-License-Identifier: Apache-2.0
//! Complete Delete Face storage; collector and healing controls remain unassigned.

use crate::om::core_reader::Reader;
use crate::om::scalar::ShiftedBinary64;
use std::num::NonZeroU8;

pub(super) const CLASSES: [&str; 3] = [
    "UGS::ModlDirect::DeleteFaceParms",
    "UGS::ModlDirect::Constrain::ManagerOptions",
    crate::om::body_write::BODY_IMAGE_CLASS,
];

fn scalar(r: &mut Reader<'_>) -> Option<()> {
    if r.bytes.get(r.at) == Some(&0) {
        r.byte()?;
    } else {
        ShiftedBinary64::read(r.take(8)?)?;
    }
    Some(())
}

pub(super) fn read(bytes: &[u8]) -> Option<[u8; 3]> {
    let mut r = Reader { bytes, at: 0 };
    let owner = NonZeroU8::new(r.byte()?)?.get();
    r.fixed(b"\0\0\x01\0")?;
    scalar(&mut r)?;
    r.fixed(b"\x03")?;
    r.payload()?;
    let manager = NonZeroU8::new(r.byte()?)?.get();
    scalar(&mut r)?;
    r.fixed(
        b"\x03\x50\x44\0\0\x01\x46\x85\x1a\x01\x8f\xff\x01\x01\x02\x04\x02\x01\x02\x01\0\0\0\0",
    )?;
    r.payload()?;
    r.fixed(b"\0\0")?;
    for _ in 0..2 {
        r.fixed(b"\x01\x08")?;
        for _ in 0..7 {
            r.body()?;
        }
    }
    r.fixed(b"\x01")?;
    r.payload()?;
    r.fixed(b"\xff\x01\x01\x01\0\0\x01\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0\x01\x02")?;
    r.byte()?;
    r.body()?;
    r.fixed(b"\x97\x75\x01\x02")?;
    let endpoint = NonZeroU8::new(r.byte()?)?.get();
    r.body()?;
    r.fixed(b"\xff\0\0\0\x0b\0\xf0\x01\x02\x02")?;
    r.body()?;
    r.fixed(b"\x01\0\0\0\0\0\0\x10\x01\x02\x01")?;
    match r.byte()? {
        0 => r.fixed(b"\0\0")?,
        1 => {
            r.fixed(b"\x02\x11\0")?;
            r.feature()?;
            r.fixed(b"\x01\x01\0\0")?;
        }
        _ => return None,
    }
    r.feature()?;
    r.fixed(b"\x02\x01\x01\x01")?;
    let inner = r.byte()?.checked_sub(2)?;
    let first = r.take(3)?;
    for _ in 0..inner {
        r.take(3)?;
    }
    r.fixed(first)?;
    r.fixed(b"\x01\0\0\xff\x1e\xc0\x1f\xff\xff\x01\0\0")?;
    r.feature()?;
    r.feature()?;
    r.fixed(b"\x01\x03\x01\x01\x01\x01\0\x01\0\0\0")?;
    let ordinal = r.feature()?;
    (r.feature()?.raw() == ordinal.raw()).then_some(())?;
    r.feature()?;
    r.fixed(b"\0")?;
    (r.at == bytes.len()).then_some([owner, manager, endpoint])
}
