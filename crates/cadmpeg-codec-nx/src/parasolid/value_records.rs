// SPDX-License-Identifier: Apache-2.0
//! Framed Parasolid attribute-value records.
use crate::framing::read_and_advance as read_xmt;
use crate::framing::xmt_reference::NonNullXmt;
use crate::parasolid::counted_values::{CountedLane, CountedValues};
use crate::parasolid::unicode_value::{UnicodeLane, UnicodeValue};
use crate::printable_string::PrintableString;
use cadmpeg_core::decode::View;
use std::collections::BTreeMap;

/// A framed record with a checked family-specific value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValueRecord<T> {
    pub(crate) offset: usize,
    pub(crate) byte_len: usize,
    pub(crate) xmt: NonNullXmt,
    pub(crate) value: T,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct EntityValueRecords<'a> {
    pub(crate) integers: Vec<ValueRecord<CountedValues<u32>>>,
    pub(crate) doubles: Vec<ValueRecord<CountedValues<f64>>>,
    pub(crate) strings: Vec<ValueRecord<PrintableString<&'a str>>>,
    pub(crate) points: Vec<ValueRecord<CountedValues<[f64; 3]>>>,
    pub(crate) vectors: Vec<ValueRecord<CountedValues<[f64; 3]>>>,
    pub(crate) axes: Vec<ValueRecord<CountedValues<[[f64; 3]; 2]>>>,
    pub(crate) tags: Vec<ValueRecord<CountedValues<u32>>>,
    pub(crate) directions: Vec<ValueRecord<CountedValues<[f64; 3]>>>,
    pub(crate) unicode: Vec<ValueRecord<UnicodeValue>>,
}

/// Decode every attribute-value family in one bounded byte pass.
#[cfg(test)]
pub(crate) fn entity_value_records(bytes: &[u8]) -> EntityValueRecords<'_> {
    let mut records = EntityValueRecords::default();
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = value_record_frame_at(bytes, offset) else {
            offset += 1;
            continue;
        };
        if append_value_record(frame, &mut records).is_some() {
            offset = frame.next_offset();
        } else {
            // The frame has already passed its family-specific validation. If
            // materialization ever disagrees, preserve the old recovery rule
            // and keep looking for a later record instead of owning a partial
            // candidate.
            offset += 1;
        }
    }
    records
}

/// Decode value records at offsets owned by an enclosing record ledger.
pub(crate) fn entity_value_records_at(
    bytes: &[u8],
    offsets: impl IntoIterator<Item = usize>,
) -> EntityValueRecords<'_> {
    let mut records = EntityValueRecords::default();
    for offset in offsets {
        if let Some(frame) = value_record_frame_at(bytes, offset) {
            let _ = append_value_record(frame, &mut records);
        }
    }
    records
}

pub(super) fn value_record_candidates(bytes: &[u8]) -> BTreeMap<u32, Vec<usize>> {
    let mut candidates = BTreeMap::<u32, Vec<usize>>::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = value_record_frame_at(bytes, offset) else {
            offset += 1;
            continue;
        };
        candidates
            .entry(u32::from(frame.xmt))
            .or_default()
            .push(offset);
        offset = frame.next_offset();
    }
    candidates
}

/// Return `(kind, xmt, byte_len)` for one complete value record.
pub(crate) fn entity_value_record_identity_at(
    bytes: &[u8],
    offset: usize,
) -> Option<(u16, u32, usize)> {
    let frame = value_record_frame_at(bytes, offset)?;
    Some((
        u16::from(frame.payload.tag()),
        u32::from(frame.xmt),
        frame.end - offset,
    ))
}

#[derive(Clone, Copy)]
enum ValuePayload<'a> {
    Integers(CountedLane<'a, u32>),
    Doubles(CountedLane<'a, f64>),
    String(PrintableString<&'a str>),
    Points(CountedLane<'a, [f64; 3]>),
    Vectors(CountedLane<'a, [f64; 3]>),
    Axes(CountedLane<'a, [[f64; 3]; 2]>),
    Tags(CountedLane<'a, u32>),
    Directions(CountedLane<'a, [f64; 3]>),
    Unicode(UnicodeLane<'a>),
}
impl ValuePayload<'_> {
    fn tag(self) -> u8 {
        match self {
            Self::Integers(..) => 0x52,
            Self::Doubles(..) => 0x53,
            Self::String(..) => 0x54,
            Self::Points(..) => 0x55,
            Self::Vectors(..) => 0x56,
            Self::Axes(..) => 0x57,
            Self::Tags(..) => 0x58,
            Self::Directions(..) => 0x59,
            Self::Unicode(..) => 0x62,
        }
    }
}
#[derive(Clone, Copy)]
struct ValueRecordFrame<'a> {
    offset: usize,
    end: usize,
    xmt: NonNullXmt,
    payload: ValuePayload<'a>,
}
impl ValueRecordFrame<'_> {
    fn next_offset(self) -> usize {
        match self.payload {
            // A string terminator can also start the next two-byte tag.
            ValuePayload::String(_) => self.end - 1,
            _ => self.end,
        }
    }
    fn retained<T>(self, value: T) -> ValueRecord<T> {
        ValueRecord {
            offset: self.offset,
            byte_len: self.end - self.offset,
            xmt: self.xmt,
            value,
        }
    }
}
fn value_record_frame_at(bytes: &[u8], offset: usize) -> Option<ValueRecordFrame<'_>> {
    let tag = *bytes.get(offset.checked_add(1)?)?;
    match tag {
        0x52 => frame_at(bytes, offset, tag, 4, |raw| {
            CountedLane::new(raw).map(ValuePayload::Integers)
        }),
        0x53 => frame_at(bytes, offset, tag, 8, |raw| {
            CountedLane::new(raw).map(ValuePayload::Doubles)
        }),
        0x54 => frame_at(bytes, offset, tag, 1, |raw| {
            PrintableString::new(std::str::from_utf8(raw).ok()?)
                .ok()
                .map(ValuePayload::String)
        }),
        0x55 => frame_at(bytes, offset, tag, 24, |raw| {
            CountedLane::new(raw).map(ValuePayload::Points)
        }),
        0x56 => frame_at(bytes, offset, tag, 24, |raw| {
            CountedLane::new(raw).map(ValuePayload::Vectors)
        }),
        0x57 => frame_at(bytes, offset, tag, 24, |raw| {
            CountedLane::new(raw).map(ValuePayload::Axes)
        }),
        0x58 => frame_at(bytes, offset, tag, 4, |raw| {
            CountedLane::new(raw).map(ValuePayload::Tags)
        }),
        0x59 => frame_at(bytes, offset, tag, 24, |raw| {
            CountedLane::new(raw).map(ValuePayload::Directions)
        }),
        0x62 => frame_at(bytes, offset, tag, 2, |raw| {
            UnicodeLane::new(raw).map(ValuePayload::Unicode)
        }),
        _ => None,
    }
}
fn frame_at<'a>(
    bytes: &'a [u8],
    offset: usize,
    tag: u8,
    width: usize,
    decode: impl FnOnce(&'a [u8]) -> Option<ValuePayload<'a>>,
) -> Option<ValueRecordFrame<'a>> {
    let mut at = offset.checked_add(2)?;
    (bytes.get(offset..at) == Some(&[0, tag])).then_some(())?;
    if bytes.get(at) == Some(&0xff) {
        at += 1;
    }
    let count = View::u32_be_at(bytes, at)? as usize;
    at += 4;
    let xmt = NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?;
    let mut end = at.checked_add(count.checked_mul(width)?)?;
    let payload = decode(bytes.get(at..end)?)?;
    if matches!(payload, ValuePayload::String(_)) {
        (bytes.get(end) == Some(&0)).then_some(())?;
        end = end.checked_add(1)?;
    }
    Some(ValueRecordFrame {
        offset,
        end,
        xmt,
        payload,
    })
}
fn append_value_record<'a>(
    frame: ValueRecordFrame<'a>,
    records: &mut EntityValueRecords<'a>,
) -> Option<()> {
    match frame.payload {
        ValuePayload::Integers(value) => {
            records.integers.push(frame.retained(value.materialize()?))
        }
        ValuePayload::Doubles(value) => records.doubles.push(frame.retained(value.materialize()?)),
        ValuePayload::String(value) => records.strings.push(frame.retained(value)),
        ValuePayload::Points(value) => records.points.push(frame.retained(value.materialize()?)),
        ValuePayload::Vectors(value) => records.vectors.push(frame.retained(value.materialize()?)),
        ValuePayload::Axes(value) => records.axes.push(frame.retained(value.materialize()?)),
        ValuePayload::Tags(value) => records.tags.push(frame.retained(value.materialize()?)),
        ValuePayload::Directions(value) => records
            .directions
            .push(frame.retained(value.materialize()?)),
        ValuePayload::Unicode(value) => records.unicode.push(frame.retained(value.materialize()?)),
    }
    Some(())
}
#[cfg(test)]
pub(crate) fn entity_52_integer_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<ValueRecord<CountedValues<u32>>> {
    let frame = value_record_frame_at(bytes, offset)?;
    let ValuePayload::Integers(value) = frame.payload else {
        return None;
    };
    Some(frame.retained(value.materialize()?))
}
#[cfg(test)]
pub(crate) fn entity_53_double_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<ValueRecord<CountedValues<f64>>> {
    let frame = value_record_frame_at(bytes, offset)?;
    let ValuePayload::Doubles(value) = frame.payload else {
        return None;
    };
    Some(frame.retained(value.materialize()?))
}
#[cfg(test)]
pub(crate) fn entity_54_string_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<ValueRecord<PrintableString<&'_ str>>> {
    let frame = value_record_frame_at(bytes, offset)?;
    let ValuePayload::String(value) = frame.payload else {
        return None;
    };
    Some(frame.retained(value))
}

#[cfg(test)]
mod tests;
