// SPDX-License-Identifier: Apache-2.0
//! Exact scalar, result-association and inherited frames for native families.

use crate::om::core_reader::Reader;
use std::num::NonZeroU8;

#[derive(Clone, Copy)]
pub(super) enum Family {
    Cylinder,
    Cone,
    Sphere,
    MoveObject,
}

pub(super) struct Frame {
    pub(super) byte_len: usize,
    pub(super) owner: u8,
    pub(super) endpoint: u8,
    pub(super) motion: Option<u8>,
    pub(super) mode: Option<&'static str>,
    pub(super) null_terminal: bool,
}

fn scalar(r: &mut Reader<'_>) -> Option<()> {
    match r.bytes.get(r.at)? {
        0 | 128 | 1 | 129 => {
            r.byte()?;
        }
        _ => {
            // This frame scales a shifted atom by one quarter. Admission
            // checks its finite encoding; the raw bytes retain the scale.
            let atom = crate::om::scalar::ShiftedScalar::read(r.bytes.get(r.at..)?)?;
            r.take(atom.raw().len())?;
        }
    }
    Some(())
}
fn inherited_prefix(r: &mut Reader<'_>) -> Option<()> {
    r.fixed(b"\x0d\x01\x02\x01\0\0\0")?;
    r.feature()?;
    r.fixed(b"\x01\x01\x01\x01\x02")?;
    let triplet = r.take(3)?;
    r.fixed(triplet)?;
    r.fixed(b"\x01\0\0\xff\0\xff")?;
    r.byte()?;
    r.fixed(b"\xc0\x1f\xff\xff\x01\0\0")
}
fn terminal(r: &mut Reader<'_>) -> Option<bool> {
    let ordinal = r.feature()?;
    let duplicate = r.feature()?;
    (ordinal.raw() == duplicate.raw()).then_some(())?;
    if r.bytes.get(r.at) == Some(&255) {
        r.byte()?;
        Some(true)
    } else {
        r.feature()?;
        Some(false)
    }
}

pub(super) fn read(bytes: &[u8], family: Family) -> Option<Frame> {
    let mut r = Reader { bytes, at: 0 };
    let owner = NonZeroU8::new(r.byte()?)?.get();
    r.fixed(b"\0\0\x01\0")?;
    let motion = match family {
        Family::Cylinder | Family::Cone | Family::Sphere => {
            r.byte()?;
            for _ in 0..6 {
                scalar(&mut r)?;
            }
            match family {
                Family::Cylinder => {
                    for _ in 0..9 {
                        r.payload()?;
                    }
                    r.fixed(b"\xff\x01\0")?;
                }
                Family::Cone => {
                    for _ in 0..13 {
                        r.payload()?;
                    }
                    r.fixed(b"\x01")?;
                    r.payload()?;
                    r.fixed(b"\xff\xff\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01")?;
                }
                Family::Sphere => {
                    for _ in 0..4 {
                        r.payload()?;
                    }
                    r.fixed(b"\xff")?;
                    r.payload()?;
                    r.fixed(b"\x01")?;
                }
                Family::MoveObject => return None,
            }
            None
        }
        Family::MoveObject => {
            let selector = NonZeroU8::new(r.byte()?)?.get();
            r.payload()?;
            r.fixed(b"\xff\x02")?;
            Some(selector)
        }
    };
    r.fixed(b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0")?;
    r.fixed(b"\x01\x02")?;
    r.byte()?;
    r.body()?;
    r.fixed(b"\x97\x75\x01\x02")?;
    let endpoint = NonZeroU8::new(r.byte()?)?.get();
    r.body()?;
    r.fixed(b"\xff\0\0\0\x0b\0\xf0\x01\x02")?;
    let association = r.byte()?;
    if matches!(family, Family::Sphere) {
        r.body()?;
    } else {
        r.feature()?;
    }
    r.fixed(b"\x01")?;
    match family {
        Family::Cylinder => match association {
            1 => r.fixed(b"\0\0\0\0\0\0")?,
            2 => {
                r.fixed(b"\0\0\0\0\0\x01\x02")?;
                r.byte()?;
                r.body()?;
                r.fixed(b"\x97\x75")?;
            }
            _ => return None,
        },
        Family::Cone => {
            (association == 1).then_some(())?;
            r.fixed(b"\0\0\0\0\0\0")?;
        }
        Family::Sphere => {
            (association == 2).then_some(())?;
            r.fixed(b"\0\0\0\0\0\x01\x02")?;
            r.byte()?;
            r.body()?;
            r.fixed(b"\x97\x75")?;
        }
        Family::MoveObject => {
            (association == 2).then_some(())?;
            r.fixed(b"\0\0\0\0\0\0")?;
        }
    }
    if matches!(family, Family::Sphere) {
        r.fixed(b"\x0d\x01\x02\x01")?;
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
        r.fixed(b"\x01\x01\x01\x01")?;
        let count = r.byte()?.checked_sub(2)?;
        let first = r.take(3)?;
        for _ in 0..count {
            r.take(3)?;
        }
        r.fixed(first)?;
        r.fixed(b"\x01\0\0\xff\x0d\xc0\x1f\xff\xfd\x01\0\0")?;
    } else {
        inherited_prefix(&mut r)?;
    }
    let mode = match family {
        Family::Cylinder => {
            r.fixed(b"\x81\x73\x80\xb4\x01\x03\x02\x01\x02")?;
            let mode = match r.take(3)? {
                b"\x02\0\0" if association == 1 => "create",
                b"\x02\0\x01" if association == 2 => "unite",
                b"\x03\0\0" if association == 2 => "subtract",
                _ => return None,
            };
            r.fixed(b"\x01\0\0\0")?;
            Some(mode)
        }
        Family::Cone => {
            r.fixed(b"\x81\x73\x80\xb4\x01\x03\x02\x01\x02\x02\0\0\x01\0\0\0")?;
            Some("create")
        }
        Family::Sphere => {
            r.feature()?;
            r.feature()?;
            r.fixed(b"\x01\x03\x01\x01\x01\x03\0\x01\0\0\0")?;
            Some("subtract")
        }
        Family::MoveObject => {
            r.feature()?;
            r.feature()?;
            r.fixed(b"\x01\x03\x02")?;
            r.take(9)?;
            None
        }
    };
    let null_terminal = terminal(&mut r)?;
    if matches!(family, Family::Cone | Family::Sphere) && null_terminal {
        return None;
    }
    if !matches!(family, Family::MoveObject) || !null_terminal {
        r.fixed(b"\0")?;
    }
    Some(Frame {
        byte_len: r.at,
        owner,
        endpoint,
        motion,
        mode,
        null_terminal,
    })
}
