// SPDX-License-Identifier: Apache-2.0
//! Complete physical-thread storage without inferred construction controls.

use crate::om::core_reader::Reader;
use std::num::NonZeroU8;

#[derive(Clone, Copy)]
pub(super) enum Family {
    Threads,
    DetailedThread,
}
pub(super) const OWNER: &str = "UGS::THD_PARMS";

fn text<'a>(r: &mut Reader<'a>) -> Option<&'a [u8]> {
    if r.bytes.get(r.at) == Some(&0) {
        r.byte()?;
        return Some(b"");
    }
    r.fixed(b"\x03")?;
    let count = r.byte()?.checked_sub(2)?;
    let value = r.take(usize::from(count))?;
    value.iter().all(|b| matches!(b, 32..=126)).then_some(())?;
    r.fixed(b"\0")?;
    Some(value)
}

pub(super) fn read(bytes: &[u8], family: Family) -> Option<[u8; 2]> {
    let mut r = Reader { bytes, at: 0 };
    let owner = NonZeroU8::new(r.byte()?)?.get();
    r.fixed(b"\0\0\x01\0\0\x01\x03")?;
    r.feature()?;
    r.feature()?;
    for (prefix, count) in [
        (b"".as_slice(), 1),
        (b"\x01\x02", 1),
        (b"\x01\x03", 3),
        (b"\x01\x07", 6),
        (b"\x01\x07", 6),
    ] {
        r.fixed(prefix)?;
        for _ in 0..count {
            r.payload()?;
        }
    }
    for _ in 0..4 {
        text(&mut r)?;
    }
    r.payload()?;
    (text(&mut r)? == b"Cut").then_some(())?;
    r.fixed(b"\xff")?;
    for _ in 0..3 {
        if r.bytes.get(r.at) == Some(&255) {
            r.byte()?;
        } else {
            r.payload()?;
        }
    }
    r.fixed(b"\xff")?;
    r.take(10)?;
    r.fixed(b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0\x01\x02")?;
    r.byte()?;
    r.body()?;
    r.fixed(b"\x97\x75\x01\x02")?;
    let endpoint = NonZeroU8::new(r.byte()?)?.get();
    r.body()?;
    r.fixed(b"\xff\0\0\0\x0b\0")?;
    r.payload()?;
    r.fixed(b"\x02\x02")?;
    r.body()?;
    r.fixed(b"\x01\0\0\0\0\0")?;
    match r.byte()? {
        0 => (),
        1 => {
            r.fixed(b"\x02")?;
            r.byte()?;
            r.body()?;
            r.fixed(b"\x97\x75")?;
        }
        _ => return None,
    }
    r.fixed(b"\x0d\x01\x02\x01\0\0\0")?;
    r.feature()?;
    r.fixed(b"\x01\x01\x01\x01\x02")?;
    let triplet = r.take(3)?;
    r.fixed(triplet)?;
    r.fixed(b"\x01\0\0\xff\0\xff")?;
    r.byte()?;
    r.fixed(b"\xc0\x1f\xff\xff\x01\0\0")?;
    r.feature()?;
    r.feature()?;
    r.fixed(b"\x01\x03\x02\x01")?;
    r.fixed(&[match family {
        Family::Threads => 2,
        Family::DetailedThread => 1,
    }])?;
    match r.take(3)? {
        b"\x03\0\0" => r.fixed(b"\x01\0\0\0")?,
        b"\x03\0\x01" => r.fixed(b"\0\0\0")?,
        _ => return None,
    }
    let ordinal = r.feature()?;
    (r.feature()?.raw() == ordinal.raw()).then_some(())?;
    r.feature()?;
    r.fixed(b"\0")?;
    (r.at == bytes.len()).then_some([owner, endpoint])
}
