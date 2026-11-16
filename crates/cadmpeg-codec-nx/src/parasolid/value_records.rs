// SPDX-License-Identifier: Apache-2.0
//! Framed Parasolid attribute-value records.

use crate::parasolid::counted_values::{BorrowedValues, CountedValues};
use crate::parasolid::unicode_value::{UnicodeLane, UnicodeValue};
use cadmpeg_core::decode::View;
use cadmpeg_core::printable::PrintableString;
use cadmpeg_core::CodecError;
use cadmpeg_parasolid::pointer::read_at as read_xmt;
use cadmpeg_parasolid::records::node_type::{
    AXIS_VALUES, CHAR_VALUES, DIRECTION_VALUES, INT_VALUES, POINT_VALUES, REAL_VALUES, TAG_VALUES,
    UNICODE_VALUES, VECTOR_VALUES,
};
use cadmpeg_parasolid::records::{DataType, PrimaryRecords};
use cadmpeg_parasolid::xmt::NonNullXmt;
use std::collections::{BTreeMap, BTreeSet};

/// A framed record with a checked family-specific value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValueRecord<T> {
    pub(crate) offset: usize,
    pub(crate) byte_len: usize,
    pub(crate) xmt: NonNullXmt,
    pub(crate) value: T,
}

#[derive(Default)]
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
pub(crate) fn entity_value_records(bytes: &[u8]) -> Result<EntityValueRecords<'_>, CodecError> {
    let mut records = EntityValueRecords::default();
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = value_record_frame_at(bytes, offset) else {
            offset += 1;
            continue;
        };
        offset = frame.end;
        append_value_record(frame, &mut records)?;
    }
    Ok(records)
}

/// Materialize value records at offsets owned by an enclosing ledger.
pub(crate) fn entity_value_records_at(
    bytes: &[u8],
    offsets: impl IntoIterator<Item = usize>,
) -> Result<EntityValueRecords<'_>, CodecError> {
    let mut records = EntityValueRecords::default();
    for offset in offsets {
        if let Some(frame) = value_record_frame_at(bytes, offset) {
            append_value_record(frame, &mut records)?;
        }
    }
    Ok(records)
}

pub(super) fn value_record_candidates(
    bytes: &[u8],
    references: &BTreeSet<u32>,
) -> BTreeMap<u32, Vec<usize>> {
    let mut candidates = BTreeMap::<u32, Vec<usize>>::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let header = bytes
            .get(offset + 1)
            .filter(|tag| matches!(**tag, 0x52..=0x59 | 0x62))
            .and_then(|&tag| value_record_header_at(bytes, offset, tag));
        let Some(header) = header else {
            offset += 1;
            continue;
        };
        // Unowned integer-shaped bytes can span real referenced records.
        // They cannot claim a frame or its payload-validation budget.
        if !references.contains(&u32::from(header.xmt)) {
            offset += 1;
            continue;
        }
        let Some(frame) = value_record_frame_at(bytes, offset) else {
            offset += 1;
            continue;
        };
        candidates
            .entry(u32::from(frame.xmt))
            .or_default()
            .push(offset);
        offset = frame.end;
    }
    candidates
}

struct ValueRecordHeader {
    count: usize,
    xmt: NonNullXmt,
    payload_offset: usize,
}

fn value_record_header_at(bytes: &[u8], offset: usize, tag: u8) -> Option<ValueRecordHeader> {
    let mut at = offset.checked_add(2)?;
    (bytes.get(offset..at) == Some(&[0, tag])).then_some(())?;
    if bytes.get(at) == Some(&0xff) {
        at += 1;
    }
    let count = cadmpeg_core::decode::index_from_u32(View::u32_be_at(bytes, at)?);
    (count != 0).then_some(())?;
    at += 4;
    let xmt = NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?;
    Some(ValueRecordHeader {
        count,
        xmt,
        payload_offset: at,
    })
}

enum ValuePayload<'a> {
    Integers(BorrowedValues<'a, u32>),
    Doubles(BorrowedValues<'a, f64>),
    String(PrintableString<&'a str>),
    Points(BorrowedValues<'a, [f64; 3]>),
    Vectors(BorrowedValues<'a, [f64; 3]>),
    Axes(BorrowedValues<'a, [[f64; 3]; 2]>),
    Tags(BorrowedValues<'a, u32>),
    Directions(BorrowedValues<'a, [f64; 3]>),
    Unicode(UnicodeLane<'a>),
}
struct ValueRecordFrame<'a> {
    offset: usize,
    end: usize,
    xmt: NonNullXmt,
    payload: ValuePayload<'a>,
}
fn retained<T>(offset: usize, end: usize, xmt: NonNullXmt, value: T) -> ValueRecord<T> {
    ValueRecord {
        offset,
        byte_len: end - offset,
        xmt,
        value,
    }
}
fn value_record_frame_at(bytes: &[u8], offset: usize) -> Option<ValueRecordFrame<'_>> {
    let tag = *bytes.get(offset.checked_add(1)?)?;
    let header = value_record_header_at(bytes, offset, tag)?;
    let at = header.payload_offset;
    let end = at.checked_add(header.count.checked_mul(value_width(tag)?)?)?;
    let payload = value_payload(tag, bytes.get(at..end)?)?;
    if matches!(payload, ValuePayload::String(_)) {
        (bytes.get(end) == Some(&0)).then_some(())?;
    }
    Some(ValueRecordFrame {
        offset,
        end,
        xmt: header.xmt,
        payload,
    })
}

/// Source bytes of one value of a value-record family.
fn value_width(tag: u8) -> Option<usize> {
    match tag {
        0x52 | 0x58 => Some(4),
        0x53 => Some(8),
        0x54 => Some(1),
        0x55..=0x57 | 0x59 => Some(24),
        0x62 => Some(2),
        _ => None,
    }
}

fn value_payload(tag: u8, raw: &[u8]) -> Option<ValuePayload<'_>> {
    match tag {
        0x52 => BorrowedValues::new(raw).map(ValuePayload::Integers),
        0x53 => BorrowedValues::new(raw).map(ValuePayload::Doubles),
        0x54 => std::str::from_utf8(raw)
            .ok()
            .and_then(|text| PrintableString::new(text).ok())
            .map(ValuePayload::String),
        0x55 => BorrowedValues::new(raw).map(ValuePayload::Points),
        0x56 => BorrowedValues::new(raw).map(ValuePayload::Vectors),
        0x57 => BorrowedValues::new(raw).map(ValuePayload::Axes),
        0x58 => BorrowedValues::new(raw).map(ValuePayload::Tags),
        0x59 => BorrowedValues::new(raw).map(ValuePayload::Directions),
        0x62 => UnicodeLane::new(raw).map(ValuePayload::Unicode),
        _ => None,
    }
}

/// Materialize the value nodes of a terminated node sequence that start at
/// `starts`.
pub(crate) fn entity_value_records_of_nodes<'a>(
    bytes: &'a [u8],
    primary: &PrimaryRecords<'_>,
    starts: &BTreeSet<usize>,
) -> Result<EntityValueRecords<'a>, CodecError> {
    let mut records = EntityValueRecords::default();
    for node in primary.nodes() {
        let range = node.range();
        if !starts.contains(&range.start) {
            continue;
        }
        let frame = (|| {
            let tag = u8::try_from(node.kind()).ok()?;
            let values = node.variable_range("values", value_data_type(node.kind())?)?;
            let payload = value_payload(tag, bytes.get(values)?)?;
            Some(ValueRecordFrame {
                offset: range.start,
                end: range.end,
                xmt: NonNullXmt::try_from(node.index()).ok()?,
                payload,
            })
        })();
        if let Some(frame) = frame {
            append_value_record(frame, &mut records)?;
        }
    }
    Ok(records)
}

/// The data type of the `values` field of a value node type.
fn value_data_type(kind: u16) -> Option<DataType> {
    match kind {
        INT_VALUES => Some(DataType::Integer),
        REAL_VALUES => Some(DataType::Real),
        CHAR_VALUES => Some(DataType::Char),
        POINT_VALUES | VECTOR_VALUES | AXIS_VALUES | DIRECTION_VALUES => Some(DataType::Vector),
        TAG_VALUES => Some(DataType::Unestablished(b't')),
        UNICODE_VALUES => Some(DataType::Word),
        _ => None,
    }
}

/// Materialize one value-record frame into its family's list.
fn append_value_record<'a>(
    frame: ValueRecordFrame<'a>,
    records: &mut EntityValueRecords<'a>,
) -> Result<(), CodecError> {
    let (offset, end, xmt) = (frame.offset, frame.end, frame.xmt);
    match frame.payload {
        ValuePayload::Integers(value) => {
            records
                .integers
                .push(retained(offset, end, xmt, value.materialize()?));
        }
        ValuePayload::Doubles(value) => {
            records
                .doubles
                .push(retained(offset, end, xmt, value.materialize()?));
        }
        ValuePayload::String(value) => {
            records.strings.push(retained(offset, end, xmt, value));
        }
        ValuePayload::Points(value) => {
            records
                .points
                .push(retained(offset, end, xmt, value.materialize()?));
        }
        ValuePayload::Vectors(value) => {
            records
                .vectors
                .push(retained(offset, end, xmt, value.materialize()?));
        }
        ValuePayload::Axes(value) => {
            records
                .axes
                .push(retained(offset, end, xmt, value.materialize()?));
        }
        ValuePayload::Tags(value) => {
            records
                .tags
                .push(retained(offset, end, xmt, value.materialize()?));
        }
        ValuePayload::Directions(value) => {
            records
                .directions
                .push(retained(offset, end, xmt, value.materialize()?));
        }
        ValuePayload::Unicode(value) => {
            records
                .unicode
                .push(retained(offset, end, xmt, value.materialize()?));
        }
    }
    Ok(())
}

#[cfg(test)]
fn entity_52_integer_record_at(
    bytes: &[u8],
    offset: usize,
) -> Result<Option<ValueRecord<CountedValues<u32>>>, CodecError> {
    let Some(frame) = value_record_frame_at(bytes, offset) else {
        return Ok(None);
    };
    let ValuePayload::Integers(value) = frame.payload else {
        return Ok(None);
    };
    Ok(Some(retained(
        frame.offset,
        frame.end,
        frame.xmt,
        value.materialize()?,
    )))
}
#[cfg(test)]
fn entity_53_double_record_at(
    bytes: &[u8],
    offset: usize,
) -> Result<Option<ValueRecord<CountedValues<f64>>>, CodecError> {
    let Some(frame) = value_record_frame_at(bytes, offset) else {
        return Ok(None);
    };
    let ValuePayload::Doubles(value) = frame.payload else {
        return Ok(None);
    };
    Ok(Some(retained(
        frame.offset,
        frame.end,
        frame.xmt,
        value.materialize()?,
    )))
}
#[cfg(test)]
fn entity_54_string_record_at(
    bytes: &[u8],
    offset: usize,
) -> Option<ValueRecord<PrintableString<&str>>> {
    let frame = value_record_frame_at(bytes, offset)?;
    let ValuePayload::String(value) = frame.payload else {
        return None;
    };
    Some(retained(frame.offset, frame.end, frame.xmt, value))
}

#[cfg(test)]
mod tests;
