// SPDX-License-Identifier: Apache-2.0
//! Persistent polyedge-reference construction decoding.

use std::ops::Range;

use crate::chunks::{checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::objects::parse_class_wrapper;
use crate::wire::Uuid;

const ANONYMOUS: u32 = 0x4000_8000;
const ITEM_CAP: usize = 1 << 20;
pub(crate) const CURVE_CLASS: Uuid = Uuid::from_canonical([
    0x39, 0xff, 0x3d, 0xd3, 0xfe, 0x0f, 0x48, 0x07, 0x9d, 0x59, 0x18, 0x5f, 0x0d, 0x73, 0xc0, 0xe4,
]);
const SEGMENT_CLASS: Uuid = Uuid::from_canonical([
    0x42, 0xf4, 0x7a, 0x87, 0x5b, 0x1b, 0x4e, 0x31, 0xab, 0x87, 0x46, 0x39, 0xd7, 0x83, 0x25, 0xd6,
]);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Segment {
    pub(crate) object_id: Uuid,
    pub(crate) component: [i32; 2],
    pub(crate) edge_domain: [f64; 2],
    pub(crate) trim_domain: [f64; 2],
    pub(crate) reversed: bool,
    pub(crate) domain: [f64; 2],
    pub(crate) proxy_domain: [f64; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PolyEdge {
    pub(crate) parameters: Vec<f64>,
    pub(crate) segments: Vec<Segment>,
}

fn malformed(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

fn count(reader: &mut BoundedReader<'_>, width: usize) -> Result<usize, FramingError> {
    let offset = reader.position();
    let count = reader.i32()?;
    checked_count_bytes(count, width, reader.remaining(), ITEM_CAP, offset)?;
    usize::try_from(count).map_err(|_| FramingError::Overflow { offset })
}

fn interval(reader: &mut BoundedReader<'_>) -> Result<[f64; 2], FramingError> {
    let value = [reader.f64()?, reader.f64()?];
    if value.iter().all(|value| value.is_finite()) {
        Ok(value)
    } else {
        Err(malformed(
            reader.position() - 16,
            "polyedge interval is not finite",
        ))
    }
}

fn segment(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<Segment, FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short || chunk.next_offset != range.end {
        return Err(malformed(range.start, "invalid polyedge-segment framing"));
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if reader.i32()? != 1 || reader.i32()? != 0 {
        return Err(malformed(
            chunk.body.start,
            "unsupported polyedge-segment version",
        ));
    }
    let object_id = Uuid::from_wire(reader.array()?);
    let component = [reader.i32()?, reader.i32()?];
    let edge_domain = interval(&mut reader)?;
    let trim_domain = interval(&mut reader)?;
    let reversed = reader.bool()?;
    let domain = interval(&mut reader)?;
    let proxy_domain = interval(&mut reader)?;
    if reader.remaining() != 0 {
        return Err(malformed(
            reader.position(),
            "polyedge segment has trailing bytes",
        ));
    }
    Ok(Segment {
        object_id,
        component,
        edge_domain,
        trim_domain,
        reversed,
        domain,
        proxy_domain,
    })
}

pub(crate) fn decode(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<PolyEdge, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(malformed(range.start, "unsupported polyedge-curve version"));
    }
    let segment_count = count(&mut reader, 1)?;
    if segment_count == 0 {
        return Err(malformed(
            reader.position(),
            "polyedge curve has no segments",
        ));
    }
    reader.i32()?;
    reader.i32()?;
    reader.take(48)?;
    let parameter_count = count(&mut reader, 8)?;
    if parameter_count != segment_count + 1 {
        return Err(malformed(
            reader.position(),
            "polyedge parameter count mismatch",
        ));
    }
    let mut parameters = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        let value = reader.f64()?;
        if !value.is_finite() || parameters.last().is_some_and(|previous| value <= *previous) {
            return Err(malformed(
                reader.position() - 8,
                "invalid polyedge parameter",
            ));
        }
        parameters.push(value);
    }
    let mut segments = Vec::new();
    for _ in 0..segment_count {
        let start = reader.position();
        let wrapper = chunk_at(data, start, reader.end(), archive, false)?;
        let class =
            parse_class_wrapper(data, start..wrapper.next_offset, archive, &mut Vec::new())?;
        if class.class_uuid != SEGMENT_CLASS {
            return Err(malformed(
                start,
                "polyedge child is not a persistent segment",
            ));
        }
        segments.push(segment(data, class.class_data_range, archive)?);
        reader.skip(wrapper.next_offset - start)?;
    }
    if reader.remaining() != 0 {
        return Err(malformed(
            reader.position(),
            "polyedge curve has trailing bytes",
        ));
    }
    Ok(PolyEdge {
        parameters,
        segments,
    })
}

pub(crate) fn semantic_json(polyedge: &PolyEdge) -> Option<String> {
    let segments = polyedge
        .segments
        .iter()
        .map(|segment| {
            serde_json::json!({
                "object_id": segment.object_id.to_string(),
                "component": segment.component,
                "edge_domain": segment.edge_domain,
                "trim_domain": segment.trim_domain,
                "reversed": segment.reversed,
                "domain": segment.domain,
                "proxy_domain": segment.proxy_domain,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&serde_json::json!({
        "kind": "polyedge_reference",
        "parameters": polyedge.parameters,
        "segments": segments,
    }))
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive_test_support::{class_wrapper, crc_chunk};

    #[test]
    fn decodes_persistent_polyedge_segment_construction() {
        let mut segment = 1_i32.to_le_bytes().to_vec();
        segment.extend(0_i32.to_le_bytes());
        segment.extend([0_u8; 15]);
        segment.push(9);
        segment.extend(2_i32.to_le_bytes());
        segment.extend(17_i32.to_le_bytes());
        for value in [0.0_f64, 4.0, 1.0, 3.0] {
            segment.extend(value.to_le_bytes());
        }
        segment.push(1);
        for value in [10.0_f64, 20.0, 2.0, 6.0] {
            segment.extend(value.to_le_bytes());
        }
        let segment = crc_chunk(ANONYMOUS, &segment);
        let segment_class = [
            0x87, 0x7a, 0xf4, 0x42, 0x1b, 0x5b, 0x31, 0x4e, 0xab, 0x87, 0x46, 0x39, 0xd7, 0x83,
            0x25, 0xd6,
        ];

        let mut payload = vec![0x10];
        payload.extend(1_i32.to_le_bytes());
        payload.extend(0_i32.to_le_bytes());
        payload.extend(0_i32.to_le_bytes());
        payload.extend([0_u8; 48]);
        payload.extend(2_i32.to_le_bytes());
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(10.0_f64.to_le_bytes());
        payload.extend(class_wrapper(segment_class, &segment));

        let decoded = decode(&payload, 0..payload.len(), ArchiveVersion::V8).unwrap();
        assert_eq!(decoded.parameters, [0.0, 10.0]);
        assert_eq!(decoded.segments[0].component, [2, 17]);
        assert_eq!(decoded.segments[0].edge_domain, [0.0, 4.0]);
        assert_eq!(decoded.segments[0].trim_domain, [1.0, 3.0]);
        assert!(decoded.segments[0].reversed);
        assert_eq!(decoded.segments[0].domain, [10.0, 20.0]);
        assert_eq!(decoded.segments[0].proxy_domain, [2.0, 6.0]);
    }
}
