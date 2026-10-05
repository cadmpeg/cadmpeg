// SPDX-License-Identifier: Apache-2.0
//! Small record-reference, transform, fixed-scalar and operation-code readers shared by several
//! scope families.

use crate::bytes::f64s_at;
use crate::bytes::take_reference;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::reference_runs::reference_position;
use crate::design::decode::sketch::{indexed_record_header_at, IndexedRecordOffsets};
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::identity::ReferenceRun;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

/// Unwrap an `Option`, ending a fallible parse with `Ok(None)` when it is empty.
macro_rules! try_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

/// How many values a search selected, keeping the only one.
pub(super) enum UniqueMatch<T> {
    Zero,
    One(T),
    Many,
}

impl<T> UniqueMatch<T> {
    /// The only selected value.
    pub(super) fn one(self) -> Option<T> {
        match self {
            Self::One(value) => Some(value),
            Self::Zero | Self::Many => None,
        }
    }
}

/// Classify the values `predicate` selects. The search stops at the second
/// selected value and charges only the values it visits.
pub(super) fn unique_match<'values, T>(
    ctx: &DecodeContext<'_>,
    values: &'values [T],
    mut predicate: impl FnMut(&T) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<UniqueMatch<&'values T>, CodecError> {
    let Some(first) = ctx.position_by(values, &mut predicate, operation)? else {
        return Ok(UniqueMatch::Zero);
    };
    let (Some(value), Some(rest)) = (values.get(first), values.get(first + 1..)) else {
        return Ok(UniqueMatch::Zero);
    };
    Ok(if ctx.any_by(rest, predicate, operation)? {
        UniqueMatch::Many
    } else {
        UniqueMatch::One(value)
    })
}

/// The class tag of the indexed-record header at `start` when that header
/// carries `record_index`.
pub(in crate::design::decode) fn exact_indexed_header_at(
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Option<&[u8; 3]> {
    indexed_record_header_at(bytes, start)
        .filter(|header| header.record_index == record_index)
        .map(|header| header.class_tag)
}

pub(super) fn exact_same_segment_record_reference(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
) -> Result<Option<(u32, u64)>, CodecError> {
    let mut cursor = at;
    let reference = try_some!(take_reference(ctx, bytes, &mut cursor)?);
    let target = try_some!(u32::try_from(try_some!(reference.local()).0).ok());
    Ok((cursor == try_some!(at.checked_add(11))).then_some((
        target,
        try_some!(u64::try_from(try_some!(at.checked_add(1))).ok()),
    )))
}

pub(in crate::design::decode) fn rigid_transform_at(
    bytes: &[u8],
    at: usize,
) -> Option<crate::records::sketch_placement::SketchPlacementMatrix> {
    let values = f64s_at::<16>(bytes, at)?;
    let mut transform = [[0.0; 4]; 4];
    for (ordinal, value) in values.into_iter().enumerate() {
        transform[ordinal / 4][ordinal % 4] = value;
    }
    transform.try_into().ok()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FixedScalarFrame<T = FiniteReal> {
    pub(super) owner_record_index: Option<u32>,
    pub(super) ordinal: u8,
    pub(super) value: T,
    pub(super) value_offset: u64,
}

/// The first frame of `record_index`, in byte order, for which `visit`
/// returns `true`. Each visited frame is admitted before its test; the search
/// stops at the match.
pub(super) fn find_frame(
    ctx: &DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    record_index: u32,
    mut visit: impl FnMut(usize, usize) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Option<(usize, usize)>, CodecError> {
    let offsets = records.offsets(record_index);
    let Some(last) = offsets.len().checked_sub(1) else {
        return Ok(None);
    };
    let (Some(paired), Some(starts)) = (offsets.get(1..), offsets.get(..last)) else {
        return Ok(None);
    };
    let mut next_start = starts.iter();
    let mut found = None;
    ctx.position_by(
        paired,
        |paired| {
            let Some(start) = next_start.next() else {
                return Ok(false);
            };
            if visit(*start, *paired)? {
                found = Some((*start, *paired));
                return Ok(true);
            }
            Ok(false)
        },
        operation,
    )?;
    Ok(found)
}

/// The frames of the records `run` names, in run order and then byte order,
/// until `visit` returns `true` for a record index and frame. Each visited reference and frame is admitted
/// before its test; the search stops at the match.
pub(super) fn find_reference_frame(
    ctx: &DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    run: &ReferenceRun<u32>,
    mut visit: impl FnMut(u32, usize, usize) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<bool, CodecError> {
    Ok(reference_position(
        ctx,
        run,
        |record_index| {
            let record_index = *record_index;
            Ok(find_frame(
                ctx,
                records,
                record_index,
                |start, paired| visit(record_index, start, paired),
                operation,
            )?
            .is_some())
        },
        operation,
    )?
    .is_some())
}

/// The fixed scalar of the only frame of `record_index` that holds one.
pub(super) fn exact_fixed_scalar(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Result<Option<FixedScalarFrame>, CodecError> {
    const OPERATION: &str = "scan F3D fixed-scalar frames";
    let mut candidate = None;
    let ambiguous = find_frame(
        ctx,
        records,
        record_index,
        |start, end| {
            let Some(scalar) = fixed_scalar_frame_at(bytes, start, end) else {
                return Ok(false);
            };
            Ok(candidate.replace(scalar).is_some())
        },
        OPERATION,
    )?
    .is_some();
    Ok(candidate.filter(|_| !ambiguous))
}

/// The fixed scalar of the frame from `start` to its paired header at `end`.
fn fixed_scalar_frame_at(bytes: &[u8], start: usize, end: usize) -> Option<FixedScalarFrame> {
    let frame_length = end.checked_sub(start)?;
    if !matches!(frame_length, 100 | 103 | 104 | 105) {
        return None;
    }
    if (frame_length == 100 || frame_length == 103)
        && (!zeros_at::<8>(bytes, start + 11)
            || bytes_at::<5>(bytes, start + 19) != Some(&[1, 1, 0, 0, 0])
            || !zeros_at::<6>(bytes, start + 29)
            || !zeros_at::<4>(bytes, start + 36))
    {
        return None;
    }
    if frame_length == 103
        && (marked_record_reference(bytes, start + 24).is_none()
            || marked_record_reference(bytes, start + 48).is_none()
            || marked_record_reference(bytes, start + 67) != View::u32_le_at(bytes, start + 25)
            || !zeros_at::<2>(bytes, start + 78)
            || marked_record_reference(bytes, start + 80).is_none()
            || !zeros_at::<7>(bytes, start + 85)
            || marked_record_reference(bytes, start + 92) != View::u32_le_at(bytes, start + 25))
    {
        return None;
    }
    let value = FiniteReal::new(View::f64_le_at(bytes, start + 40)?)?;
    Some(FixedScalarFrame {
        owner_record_index: (bytes.get(start + 24) == Some(&1))
            .then(|| View::u32_le_at(bytes, start + 25))
            .flatten(),
        ordinal: *bytes.get(start + 35)?,
        value,
        value_offset: u64::try_from(start + 40).ok()?,
    })
}

pub(in crate::design::decode) fn marked_reference(bytes: &[u8], at: usize) -> Option<u32> {
    (bytes.get(at) == Some(&1)).then(|| View::u32_le_at(bytes, at + 1))?
}

pub(in crate::design::decode) fn marked_record_reference(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1) || !zeros_at::<6>(bytes, at + 5) {
        return None;
    }
    View::u32_le_at(bytes, at + 1)
}

pub(super) fn extrude_operation_at(bytes: &[u8], offset: usize) -> Option<DesignExtrudeOperation> {
    match View::u32_le_at(bytes, offset)? {
        1 => Some(DesignExtrudeOperation::Join),
        2 => Some(DesignExtrudeOperation::Cut),
        3 => Some(DesignExtrudeOperation::Intersect),
        4 => Some(DesignExtrudeOperation::NewBody),
        _ => None,
    }
}
