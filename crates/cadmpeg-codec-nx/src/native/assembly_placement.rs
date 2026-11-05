// SPDX-License-Identifier: Apache-2.0
//! Complete assembly placement frames from the bounded part payload.

use std::collections::{btree_map::Entry, BTreeMap};
use std::fmt::Write;

use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use crate::canonical_uuid::CanonicalUuid;
use crate::container::{Container, EntryContent};
use crate::layout::assembly_placement_prefix::MAGIC_VALUE;
use crate::native::om::ObjectRecord;

const ENTRY_NAME: &str = "/Root/UG_PART/UG_PART";
const ID_PREFIX: &str = "nx:assembly:placement#";
const EPS_PLACEMENT_ROTATION: f64 = 1e-4;

cadmpeg_core::named_optional_field!(deserialize_object_record, String, "object_record");

/// One complete frame, with derived fields checked against its exact bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "AssemblyPlacementWire")]
pub(crate) struct AssemblyPlacement {
    pub(super) id: String,
    raw_frame: NativeBytes,
    /// Row-major three-by-four matrix in the serialized translation unit.
    matrix: [f64; 12],
    label: String,
    uuid: CanonicalUuid<String>,
    /// Bounded object record containing the complete placement frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object_record: Option<String>,
    source_entry: String,
    /// Offset within the container image, including virtual legacy images.
    pub(super) source_offset: u64,
}

#[derive(Deserialize)]
struct AssemblyPlacementWire {
    id: String,
    raw_frame: NativeBytes,
    matrix: [f64; 12],
    label: String,
    uuid: CanonicalUuid<String>,
    #[serde(default, deserialize_with = "deserialize_object_record")]
    object_record: Option<String>,
    source_entry: String,
    source_offset: u64,
}

impl TryFrom<AssemblyPlacementWire> for AssemblyPlacement {
    type Error = &'static str;

    fn try_from(wire: AssemblyPlacementWire) -> Result<Self, Self::Error> {
        let frame = parse_frame(View::over_retained(wire.raw_frame.as_ref()))
            .ok_or("assembly placement raw_frame is incomplete or invalid")?;
        if frame.length != wire.raw_frame.len()
            || frame
                .matrix
                .iter()
                .zip(wire.matrix)
                .any(|(a, b)| a.to_bits() != b.to_bits())
            || frame.label != wire.label
            || frame.uuid.as_str() != wire.uuid.as_str()
        {
            return Err("assembly placement fields must match the complete raw_frame");
        }
        let ordinal = wire
            .id
            .strip_prefix(ID_PREFIX)
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()) && (*s == "0" || !s.starts_with('0')))
            .and_then(|s| s.parse::<u64>().ok());
        if ordinal != Some(wire.source_offset)
            || wire.source_entry != ENTRY_NAME
            || wire
                .source_offset
                .checked_add(u64_from_index(frame.length))
                .is_none()
        {
            return Err("assembly placement source identity is inconsistent");
        }
        Ok(Self {
            id: wire.id,
            raw_frame: wire.raw_frame,
            matrix: wire.matrix,
            label: wire.label,
            uuid: wire.uuid,
            object_record: wire.object_record,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
        })
    }
}

struct ParsedFrame<'a> {
    length: usize,
    matrix: [f64; 12],
    label: &'a str,
    uuid: CanonicalUuid<&'a str>,
}

fn matrix_atom(view: &mut View<'_>) -> Option<f64> {
    let marker = *view.unread().first()?;
    let value = match marker {
        0 | 1 | 0x80 | 0x81 => {
            view.skip(1)?;
            match marker {
                0 => 0.0,
                1 => 1.0,
                0x80 => -0.0,
                _ => -1.0,
            }
        }
        0x20..=0x3f | 0xa0..=0xbf => {
            let mut bytes = view.array::<8>()?;
            bytes[0] += 0x10;
            // endian-exception: reconstructed-scalar
            f64::from_be_bytes(bytes) / 4.0
        }
        0x40..=0x5f | 0xc0..=0xdf => {
            let [first, second, third, fourth] = view.array::<4>()?;
            let bytes = [first - 0x10, second, third, fourth, 0, 0, 0, 0];
            // endian-exception: reconstructed-scalar
            f64::from_be_bytes(bytes) / 4.0
        }
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn rigid_rotation(matrix: &[f64; 12]) -> bool {
    for row in 0..3 {
        for other in 0..3 {
            let dot: f64 = (0..3)
                .map(|column| matrix[4 * row + column] * matrix[4 * other + column])
                .sum();
            let expected = if row == other { 1.0 } else { 0.0 };
            if (dot - expected).abs() >= EPS_PLACEMENT_ROTATION {
                return false;
            }
        }
    }
    let [m00, m01, m02, _, m10, m11, m12, _, m20, m21, m22, _] = *matrix;
    let determinant = m00 * (m11 * m22 - m12 * m21) - m01 * (m10 * m22 - m12 * m20)
        + m02 * (m10 * m21 - m11 * m20);
    (determinant - 1.0).abs() < EPS_PLACEMENT_ROTATION
}

fn text_frame<'a>(view: &mut View<'a>) -> Option<&'a str> {
    if view.u8()? != 3 {
        return None;
    }
    let length = usize::from(view.u8()?).checked_sub(2)?;
    let bytes = view.take(length)?;
    if !bytes.is_ascii() || bytes.contains(&0) || view.u8()? != 0 {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

fn parse_frame(mut view: View<'_>) -> Option<ParsedFrame<'_>> {
    let start = view.position();
    if view.array::<6>()? != MAGIC_VALUE {
        return None;
    }
    let mut matrix = [0.0; 12];
    for entry in &mut matrix {
        *entry = matrix_atom(&mut view)?;
    }
    if !rigid_rotation(&matrix) || view.array::<4>()? != [0, 0, 0, 1] {
        return None;
    }
    if view.array::<5>()?[1] != 0xf0 {
        return None;
    }
    let label = text_frame(&mut view)?;
    let uuid = CanonicalUuid::new(text_frame(&mut view)?).ok()?;
    Some(ParsedFrame {
        length: view.position().checked_sub(start)?,
        matrix,
        label,
        uuid,
    })
}

/// Retain complete frames without inferring a child identity or unit conversion.
pub(in crate::native) fn assembly_placements(
    container: &Container<'_>,
) -> Result<Vec<AssemblyPlacement>, CodecError> {
    let mut entries = container
        .entries
        .iter()
        .filter(|e| e.content() == EntryContent::PartPayload);
    let Some(entry) = entries.next() else {
        return Ok(Vec::new());
    };
    if entries.next().is_some() {
        return Ok(Vec::new());
    }
    let Some((offset, length)) = entry.file_span() else {
        return Ok(Vec::new());
    };
    let image = container.data.view();
    let Some(start) = usize::try_from(offset)
        .ok()
        .and_then(|n| image.start().checked_add(n))
    else {
        return Ok(Vec::new());
    };
    let Some(end) = usize::try_from(length)
        .ok()
        .and_then(|n| start.checked_add(n))
    else {
        return Ok(Vec::new());
    };
    let Some(payload) = image.child(start, end) else {
        return Ok(Vec::new());
    };
    let mut records = Vec::new();
    for (relative, bytes) in payload.window().windows(MAGIC_VALUE.len()).enumerate() {
        if bytes != MAGIC_VALUE {
            continue;
        }
        let Some(candidate) = payload.child(start + relative, end) else {
            continue;
        };
        let Some(frame) = parse_frame(candidate) else {
            continue;
        };
        let Some(source_offset) = offset.checked_add(u64_from_index(relative)) else {
            continue;
        };
        let raw = candidate
            .window()
            .get(..frame.length)
            .ok_or_else(|| CodecError::malformed("NX placement frame span"))?;
        let mut id = String::new();
        write!(id, "{ID_PREFIX}{source_offset}").map_err(|_| {
            CodecError::malformed(format_args!(
                "{} exceeds its local bound",
                "write NX placement identity"
            ))
        })?;
        records.push(AssemblyPlacement {
            id,
            raw_frame: (raw).to_vec().into(),
            matrix: frame.matrix,
            label: (frame.label).to_owned(),
            uuid: CanonicalUuid::new((frame.uuid.as_str()).to_owned())
                .map_err(|_| CodecError::malformed("NX placement UUID"))?,
            object_record: None,
            source_entry: entry.name.clone(),
            source_offset,
        });
    }
    Ok(records)
}

fn record_starts(records: &[ObjectRecord]) -> BTreeMap<(&str, u64), Option<&ObjectRecord>> {
    let mut starts = BTreeMap::new();
    for record in records {
        match starts.entry((record.source_entry.as_str(), record.source_offset)) {
            Entry::Vacant(entry) => {
                entry.insert(Some(record));
            }
            Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
        }
    }
    starts
}

fn placement_record<'a>(
    placement: &AssemblyPlacement,
    starts: &BTreeMap<(&str, u64), Option<&'a ObjectRecord>>,
) -> Option<&'a ObjectRecord> {
    let record = starts
        .get(&(placement.source_entry.as_str(), placement.source_offset))?
        .as_ref()?;
    record.source_offset.checked_add(record.byte_len)?;
    (record.byte_len >= u64_from_index(placement.raw_frame.len())).then_some(*record)
}

pub(in crate::native) fn bind_object_records(
    placements: &mut [AssemblyPlacement],
    records: &[ObjectRecord],
) {
    if placements.is_empty() {
        return;
    }
    let starts = record_starts(records);
    for placement in placements {
        placement.object_record =
            placement_record(placement, &starts).map(|record| record.id.clone());
    }
}

pub(crate) fn admit_object_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<(), cadmpeg_ir::native::NativeConvertError> {
    let mut placements =
        namespace.arena_iter_as_for_decode::<AssemblyPlacement>(ctx, "assembly_placements");
    let mut first = None;
    for placement in placements.by_ref() {
        let placement = placement?;
        if placement.object_record.is_some() {
            first = Some(placement);
            break;
        }
    }
    let Some(first) = first else {
        return Ok(());
    };
    let records: Vec<ObjectRecord> = namespace.arena_as_for_decode(ctx, "object_records")?;
    let starts = record_starts(&records);
    for placement in std::iter::once(Ok(first)).chain(placements) {
        let placement = placement?;
        if let Some(id) = &placement.object_record {
            if placement_record(&placement, &starts).is_none_or(|record| &record.id != id) {
                return Err(cadmpeg_ir::native::NativeConvertError::InvalidCollection(
                    "assembly placement object_record must identify its uniquely bounded source record".into(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
