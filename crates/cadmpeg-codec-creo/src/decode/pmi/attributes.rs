// SPDX-License-Identifier: Apache-2.0
//! Count-bounded neutral attribute rows.

use cadmpeg_core::bytes::find_iter;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, Copy)]
pub(super) enum Value<'a> {
    Text(&'a [u8]),
    Real(Option<f64>),
    Logical(Option<bool>),
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Integer {
    Value(u32),
    Null,
}

impl Integer {
    pub(super) const fn value(self) -> Option<u32> {
        match self {
            Self::Value(value) => Some(value),
            Self::Null => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Attribute<'a> {
    pub(super) name: &'a [u8],
    pub(super) value: Value<'a>,
    pub(super) owner: Option<u32>,
    pub(super) owner_type: Option<u32>,
    pub(super) unit: &'a [u8],
    pub(super) offset: usize,
}

pub(super) fn attributes<'a>(
    ctx: &DecodeContext<'_>,
    mut section: View<'a>,
) -> Result<Option<Vec<Option<Attribute<'a>>>>, CodecError> {
    const LABEL: &[u8] = b"\xe0\x00attributes\0";
    let mut hits = find_iter(section.window(), LABEL);
    let (Some(offset), None) = (hits.next(), hits.next()) else {
        return Ok(None);
    };
    let Some(position) = section.start().checked_add(offset + LABEL.len()) else {
        return Ok(None);
    };
    if section.seek(position).is_none() || expect(&mut section, b"\xf4\x05\xf8").is_none() {
        return Ok(None);
    }
    let Some(count) = integer(&mut section).and_then(Integer::value) else {
        return Ok(None);
    };
    let Some(count) = section.counted(u64::from(count), 1) else {
        return Ok(None);
    };
    if expect(&mut section, b"\xf7").is_none()
        || integer(&mut section).and_then(Integer::value).is_none()
        || expect(&mut section, b"\xfb").is_none()
    {
        return Ok(None);
    }
    let mut rows = ctx.alloc_filled(count.get(), None, "Creo neutral attribute rows")?;
    if rows.is_empty() {
        return Ok(Some(rows));
    }
    let Some(first) = prototype(&mut section) else {
        return Ok(None);
    };
    rows[0] = Some(first);
    for row in rows.iter_mut().skip(1) {
        let Some(value) = positional(&mut section) else {
            return Ok(None);
        };
        *row = Some(value);
    }
    Ok(Some(rows))
}

fn prototype<'a>(view: &mut View<'a>) -> Option<Attribute<'a>> {
    let offset = view.position();
    expect(view, b"\xe3\xe0\x0aname\0")?;
    let name = text(view)?;
    expect(view, b"\xe0\x00value\0\xe2\xe0\x01type\0")?;
    let kind = integer(view)?.value()?;
    let value = typed_value(view, kind, true)?;
    expect(view, b"\xe0\x01ref_id\0")?;
    framing(view)?;
    let owner = integer(view)?.value();
    expect(view, b"\xe0\x01ref_type\0")?;
    let owner_type = integer(view)?.value();
    expect(view, b"\xe0\x0aref_name\0")?;
    text(view)?;
    expect(view, b"\xe0\x0aunit\0")?;
    let unit = text(view)?;
    expect(view, b"\xe0\x01designated\0")?;
    integer(view)?;
    expect(view, b"\xf1\xf7")?;
    integer(view)?.value()?;
    Some(Attribute {
        name,
        value,
        owner,
        owner_type,
        unit,
        offset,
    })
}

fn positional<'a>(view: &mut View<'a>) -> Option<Attribute<'a>> {
    let offset = view.position();
    expect(view, b"\xe3")?;
    class_ref(view)?;
    let name = text(view)?;
    expect(view, b"\xe2")?;
    let kind = integer(view)?.value()?;
    class_ref(view)?;
    let labeled = view.unread().first() == Some(&0xe0);
    let value = typed_value(view, kind, labeled)?;
    if labeled {
        expect(view, b"\xf1\xf7")?;
        integer(view)?.value()?;
    }
    let owner = integer(view)?.value();
    let owner_type = integer(view)?.value();
    text(view)?;
    let unit = text(view)?;
    integer(view)?;
    Some(Attribute {
        name,
        value,
        owner,
        owner_type,
        unit,
        offset,
    })
}

fn typed_value<'a>(view: &mut View<'a>, kind: u32, labeled: bool) -> Option<Value<'a>> {
    if labeled {
        let label: &[u8] = match kind {
            0x32 => b"\xe0\x02value(d_val)\0",
            0x33 => b"\xe0\x0avalue(s_val)\0",
            0x34 => b"\xe0\x01value(i_val)\0",
            0x35 => b"\xe0\x05value(l_val)\0",
            _ => return None,
        };
        expect(view, label)?;
        framing(view)?;
    }
    Some(match kind {
        0x32 => real(view)?,
        0x33 => Value::Text(text(view)?),
        0x34 => {
            integer(view)?;
            Value::Other
        }
        0x35 => Value::Logical(match integer(view)? {
            Integer::Value(0) => Some(false),
            Integer::Value(1) => Some(true),
            Integer::Value(_) | Integer::Null => None,
        }),
        _ => return None,
    })
}

fn real(view: &mut View<'_>) -> Option<Value<'static>> {
    let mut probe = *view;
    let head = probe.u8()?;
    let decoded = match head {
        0x09..=0x0e => constant(head),
        0x00..=0x02 => {
            let selector = probe.u8()?;
            let width = usize::from(head) + 1;
            let tail = probe.take(width)?;
            constant(selector).map(|value| {
                let mut bits = value.to_bits();
                for (index, byte) in tail.iter().rev().enumerate() {
                    let shift = index * 8;
                    bits = (bits & !(0xff_u64 << shift)) | (u64::from(*byte) << shift);
                }
                f64::from_bits(bits)
            })
        }
        _ => crate::assembly::frame_scalar(head, &mut probe),
    };
    let len = match head {
        0x00..=0x02 => usize::from(head) + 3,
        0x04..=0x10 | 0x18 => 1,
        0x19..=0x4a => match (head - if head < 0x32 { 0x19 } else { 0x32 }) % 5 {
            0 => 8,
            1 | 2 => 3,
            4 => 7,
            _ => return None,
        },
        0x4b..=0xdf => 7,
        0xea | 0xee => 4,
        0xec => 8,
        0xed => 9,
        _ => return None,
    };
    view.skip(len)?;
    Some(Value::Real(decoded.filter(|value| value.is_finite())))
}

fn constant(head: u8) -> Option<f64> {
    match head {
        0x04 => Some(f64::from_bits(0x3fef_ffff_ffff_ffff)),
        0x09 => Some(0.005),
        0x0a => Some(0.01),
        0x0b => Some(0.06),
        0x0c => Some(0.1),
        0x0d => Some(0.25),
        0x0e => Some(0.5),
        0x0f => Some(1.0),
        0x10 => Some(-1.0),
        _ => None,
    }
}

pub(super) fn integer(view: &mut View<'_>) -> Option<Integer> {
    while view.unread().first() == Some(&0xf5) {
        view.skip(1)?;
    }
    if view.unread().first() == Some(&0xf6) {
        view.skip(1)?;
        return Some(Integer::Null);
    }
    if view.unread().first() == Some(&0xeb) {
        view.skip(1)?;
        let [a, b, c, d] = view.array()?;
        return Some(Integer::Value(
            (u32::from(a) << 24) | (u32::from(b) << 16) | (u32::from(c) << 8) | u32::from(d),
        ));
    }
    let (value, end) = crate::psb::packed_compact_int(view.unread(), 0)?;
    view.skip(end)?;
    Some(Integer::Value(value))
}

pub(super) fn expect(view: &mut View<'_>, bytes: &[u8]) -> Option<()> {
    (view.take(bytes.len())? == bytes).then_some(())
}

fn framing(view: &mut View<'_>) -> Option<()> {
    if view.unread().first() == Some(&0xf1) {
        view.skip(1)?;
    }
    Some(())
}

fn class_ref(view: &mut View<'_>) -> Option<()> {
    if view.unread().first() == Some(&0xf7) {
        view.skip(1)?;
        integer(view)?.value()?;
    }
    Some(())
}

fn text<'a>(view: &mut View<'a>) -> Option<&'a [u8]> {
    if view.unread().first() == Some(&0xe1) {
        view.skip(1)?;
        return Some(&[]);
    }
    let len = view.unread().iter().position(|byte| *byte == 0)?;
    let text = view.take(len)?;
    view.skip(1)?;
    Some(text)
}

#[cfg(test)]
mod tests;
