// SPDX-License-Identifier: Apache-2.0
//! Framed Parasolid attribute-value records.
use crate::framing::read_and_advance as read_xmt;
use crate::framing::xmt_reference::NonNullXmt;
use crate::parasolid::counted_values::{BorrowedValues, CountedValues};
use crate::parasolid::unicode_value::{UnicodeLane, UnicodeValue};
use crate::printable_string::PrintableString;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

/// A framed record with a checked family-specific value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValueRecord<T> {
    pub(crate) offset: usize,
    pub(crate) byte_len: usize,
    pub(crate) xmt: NonNullXmt,
    pub(crate) value: T,
}

pub(crate) struct EntityValueRecords<'a, 'ctx> {
    slots: ScopedReservation<'ctx>,
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

impl<'ctx> EntityValueRecords<'_, 'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            slots: ctx.reserve_scoped(0, "NX value record slots")?,
            integers: Vec::new(),
            doubles: Vec::new(),
            strings: Vec::new(),
            points: Vec::new(),
            vectors: Vec::new(),
            axes: Vec::new(),
            tags: Vec::new(),
            directions: Vec::new(),
            unicode: Vec::new(),
        })
    }
}

/// Decode every attribute-value family in one bounded byte pass.
#[cfg(test)]
pub(crate) fn entity_value_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<EntityValueRecords<'a, 'ctx>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX value records",
    )?;
    let mut records = EntityValueRecords::new(ctx)?;
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = value_record_frame_at(ctx, bytes, offset)? else {
            offset += 1;
            continue;
        };
        offset = frame.next_offset();
        append_value_record(ctx, frame, &mut records)?;
    }
    Ok(records)
}

/// Materialize value records at offsets owned by an enclosing ledger.
pub(crate) fn entity_value_records_at<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'a [u8],
    offsets: impl IntoIterator<Item = usize>,
) -> Result<EntityValueRecords<'a, 'ctx>, CodecError> {
    let mut records = EntityValueRecords::new(ctx)?;
    for offset in offsets {
        ctx.charge_work(1, "read NX owned value record")?;
        if let Some(frame) = value_record_frame_at(ctx, bytes, offset)? {
            append_value_record(ctx, frame, &mut records)?;
        }
    }
    Ok(records)
}

pub(super) fn value_record_candidates<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(BTreeMap<u32, Vec<usize>>, ScopedReservation<'ctx>), CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX value identities",
    )?;
    let mut candidates = BTreeMap::<u32, Vec<usize>>::new();
    let mut reservation = ctx.reserve_scoped(0, "NX value candidate index")?;
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(frame) = value_record_frame_at(ctx, bytes, offset)? else {
            offset += 1;
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(candidates.len()),
            "index NX value identities",
        )?;
        ctx.push_scoped_btree_group(
            &mut reservation,
            &mut candidates,
            u32::from(frame.xmt),
            || offset,
            0,
            "NX value candidate index",
        )?;
        offset = frame.next_offset();
    }
    Ok((candidates, reservation))
}

/// Return kind, identity, and extent without allocating a payload.
pub(crate) fn entity_value_record_identity_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
) -> Result<Option<(u16, u32, usize)>, CodecError> {
    Ok(value_record_frame_at(ctx, bytes, offset)?.map(|frame| {
        (
            u16::from(frame.payload.tag()),
            u32::from(frame.xmt),
            frame.end - offset,
        )
    }))
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
impl ValuePayload<'_> {
    fn tag(&self) -> u8 {
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
struct ValueRecordFrame<'a> {
    offset: usize,
    end: usize,
    xmt: NonNullXmt,
    payload: ValuePayload<'a>,
}
impl ValueRecordFrame<'_> {
    fn next_offset(&self) -> usize {
        match &self.payload {
            // A string terminator can also start the next two-byte tag.
            ValuePayload::String(_) => self.end - 1,
            _ => self.end,
        }
    }
}

fn retained<T>(offset: usize, end: usize, xmt: NonNullXmt, value: T) -> ValueRecord<T> {
    ValueRecord {
        offset,
        byte_len: end - offset,
        xmt,
        value,
    }
}
fn value_record_frame_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    offset: usize,
) -> Result<Option<ValueRecordFrame<'a>>, CodecError> {
    let value: Option<Result<_, CodecError>> = (|| {
        let tag = *bytes.get(offset.checked_add(1)?)?;
        Some(Ok(match tag {
            0x52 => propagate_resource!(frame_at(bytes, offset, tag, 4, |raw| {
                Ok(BorrowedValues::new(ctx, raw)?.map(ValuePayload::Integers))
            }))?,
            0x53 => propagate_resource!(frame_at(bytes, offset, tag, 8, |raw| {
                Ok(BorrowedValues::new(ctx, raw)?.map(ValuePayload::Doubles))
            }))?,
            0x54 => propagate_resource!(frame_at(bytes, offset, tag, 1, |raw| {
                let Ok(text) = std::str::from_utf8(raw) else { return Ok(None); };
                Ok(PrintableString::from_wire(ctx, text)?.ok().map(ValuePayload::String))
            }))?,
            0x55 => propagate_resource!(frame_at(bytes, offset, tag, 24, |raw| {
                Ok(BorrowedValues::new(ctx, raw)?.map(ValuePayload::Points))
            }))?,
            0x56 => propagate_resource!(frame_at(bytes, offset, tag, 24, |raw| {
                Ok(BorrowedValues::new(ctx, raw)?.map(ValuePayload::Vectors))
            }))?,
            0x57 => propagate_resource!(frame_at(bytes, offset, tag, 24, |raw| {
                Ok(BorrowedValues::new(ctx, raw)?.map(ValuePayload::Axes))
            }))?,
            0x58 => propagate_resource!(frame_at(bytes, offset, tag, 4, |raw| {
                Ok(BorrowedValues::new(ctx, raw)?.map(ValuePayload::Tags))
            }))?,
            0x59 => propagate_resource!(frame_at(bytes, offset, tag, 24, |raw| {
                Ok(BorrowedValues::new(ctx, raw)?.map(ValuePayload::Directions))
            }))?,
            0x62 => propagate_resource!(frame_at(bytes, offset, tag, 2, |raw| {
                Ok(UnicodeLane::new(ctx, raw)?.map(ValuePayload::Unicode))
            }))?,
            _ => return None,
        }))
    })();
    value.transpose()
}
fn frame_at<'a>(
    bytes: &'a [u8],
    offset: usize,
    tag: u8,
    width: usize,
    decode: impl FnOnce(&'a [u8]) -> Result<Option<ValuePayload<'a>>, CodecError>,
) -> Result<Option<ValueRecordFrame<'a>>, CodecError> {
    let parsed: Option<Result<_, CodecError>> = (|| {
        let mut at = offset.checked_add(2)?;
        (bytes.get(offset..at) == Some(&[0, tag])).then_some(())?;
        if bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let count = cadmpeg_core::decode::index_from_u32(View::u32_be_at(bytes, at)?);
        at += 4;
        let xmt = NonNullXmt::try_from(read_xmt(bytes, &mut at)?).ok()?;
        let mut end = at.checked_add(count.checked_mul(width)?)?;
        let payload = propagate_resource!(decode(bytes.get(at..end)?))?;
        if matches!(payload, ValuePayload::String(_)) {
            (bytes.get(end) == Some(&0)).then_some(())?;
            end = end.checked_add(1)?;
        }
        Some(Ok(ValueRecordFrame {
            offset,
            end,
            xmt,
            payload,
        }))
    })();
    parsed.transpose()
}
/// Materialize one value-record frame into its family's list.
/// Storage and work refusals propagate through the caller context.
fn append_value_record<'a>(
    ctx: &DecodeContext<'_>,
    frame: ValueRecordFrame<'a>,
    records: &mut EntityValueRecords<'a, '_>,
) -> Result<(), CodecError> {
    match frame.payload {
        ValuePayload::Integers(value) => ctx.push_scoped_vec(
            &mut records.slots,
            &mut records.integers,
            retained(frame.offset, frame.end, frame.xmt, value.materialize(ctx)?),
            "NX value record slots",
        )?,
        ValuePayload::Doubles(value) => ctx.push_scoped_vec(
            &mut records.slots,
            &mut records.doubles,
            retained(frame.offset, frame.end, frame.xmt, value.materialize(ctx)?),
            "NX value record slots",
        )?,
        ValuePayload::String(value) => ctx.push_scoped_vec(
            &mut records.slots,
            &mut records.strings,
            retained(frame.offset, frame.end, frame.xmt, value),
            "NX value record slots",
        )?,
        ValuePayload::Points(value) => ctx.push_scoped_vec(
            &mut records.slots,
            &mut records.points,
            retained(frame.offset, frame.end, frame.xmt, value.materialize(ctx)?),
            "NX value record slots",
        )?,
        ValuePayload::Vectors(value) => ctx.push_scoped_vec(
            &mut records.slots,
            &mut records.vectors,
            retained(frame.offset, frame.end, frame.xmt, value.materialize(ctx)?),
            "NX value record slots",
        )?,
        ValuePayload::Axes(value) => ctx.push_scoped_vec(
            &mut records.slots,
            &mut records.axes,
            retained(frame.offset, frame.end, frame.xmt, value.materialize(ctx)?),
            "NX value record slots",
        )?,
        ValuePayload::Tags(value) => ctx.push_scoped_vec(
            &mut records.slots,
            &mut records.tags,
            retained(frame.offset, frame.end, frame.xmt, value.materialize(ctx)?),
            "NX value record slots",
        )?,
        ValuePayload::Directions(value) => ctx.push_scoped_vec(
            &mut records.slots,
            &mut records.directions,
            retained(frame.offset, frame.end, frame.xmt, value.materialize(ctx)?),
            "NX value record slots",
        )?,
        ValuePayload::Unicode(value) => ctx.push_scoped_vec(
            &mut records.slots,
            &mut records.unicode,
            retained(frame.offset, frame.end, frame.xmt, value.materialize(ctx)?),
            "NX value record slots",
        )?,
    }
    Ok(())
}

#[cfg(test)]
fn entity_52_integer_record_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
) -> Result<Option<ValueRecord<CountedValues<u32>>>, CodecError> {
    let Some(frame) = value_record_frame_at(ctx, bytes, offset)? else {
        return Ok(None);
    };
    let ValuePayload::Integers(value) = frame.payload else {
        return Ok(None);
    };
    Ok(Some(retained(
        frame.offset,
        frame.end,
        frame.xmt,
        value.materialize(ctx)?,
    )))
}
#[cfg(test)]
fn entity_53_double_record_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
) -> Result<Option<ValueRecord<CountedValues<f64>>>, CodecError> {
    let Some(frame) = value_record_frame_at(ctx, bytes, offset)? else {
        return Ok(None);
    };
    let ValuePayload::Doubles(value) = frame.payload else {
        return Ok(None);
    };
    Ok(Some(retained(
        frame.offset,
        frame.end,
        frame.xmt,
        value.materialize(ctx)?,
    )))
}
#[cfg(test)]
fn entity_54_string_record_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    offset: usize,
) -> Result<Option<ValueRecord<PrintableString<&'a str>>>, CodecError> {
    let Some(frame) = value_record_frame_at(ctx, bytes, offset)? else {
        return Ok(None);
    };
    let ValuePayload::String(value) = frame.payload else {
        return Ok(None);
    };
    Ok(Some(retained(frame.offset, frame.end, frame.xmt, value)))
}

#[cfg(test)]
mod tests;
