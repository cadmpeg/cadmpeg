// SPDX-License-Identifier: Apache-2.0
//! Bounded hatch payload decoding.

use std::ops::Range;

use crate::chunks::{checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::curves::{DecodedCurve, DecodedGeometry, GeometryError};
use crate::objects::parse_class_wrapper;
use crate::settings::{plane, Plane};
use crate::wire::Uuid;

pub(crate) const CLASS: Uuid = Uuid::from_canonical([
    0x05, 0x59, 0x73, 0x3b, 0x53, 0x32, 0x49, 0xd1, 0xa9, 0x36, 0x05, 0x32, 0xac, 0x76, 0xad, 0xe5,
]);
const MAX_LOOPS: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopKind {
    Outer,
    Inner,
}

#[derive(Debug, Clone)]
pub(crate) struct HatchLoop {
    pub(crate) kind: LoopKind,
    pub(crate) curve: DecodedCurve,
}

#[derive(Debug, Clone)]
pub(crate) struct Hatch {
    pub(crate) source_range: Range<usize>,
    pub(crate) plane: Plane,
    pub(crate) pattern_scale: f64,
    pub(crate) pattern_rotation: f64,
    pub(crate) pattern_index: i32,
    pub(crate) loops: Vec<HatchLoop>,
    pub(crate) basepoint: [f64; 2],
    pub(crate) warnings: Vec<String>,
}

fn structural(offset: usize, message: impl Into<String>) -> GeometryError {
    GeometryError::Malformed(FramingError::Structural {
        offset,
        message: message.into(),
    })
}

fn finite(value: f64, offset: usize, label: &str) -> Result<f64, GeometryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(structural(offset, format!("hatch {label} is not finite")))
    }
}

pub(crate) fn decode(
    data: &[u8],
    range: Range<usize>,
    _scale: f64,
    archive: ArchiveVersion,
) -> Result<Hatch, GeometryError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let version_offset = reader.position();
    let version = reader.u8()?;
    let (major, minor) = (version >> 4, version & 0x0f);
    if major != 1 || minor > 2 {
        return Err(GeometryError::UnsupportedVersion {
            offset: version_offset,
            message: format!("unsupported hatch version {major}.{minor}"),
        });
    }
    let plane = plane(&mut reader)?;
    let pattern_scale = finite(reader.f64()?, reader.position() - 8, "pattern scale")?;
    if pattern_scale <= 0.0 {
        return Err(structural(
            reader.position() - 8,
            "hatch pattern scale is not positive",
        ));
    }
    let pattern_rotation = finite(reader.f64()?, reader.position() - 8, "pattern rotation")?;
    let pattern_index = reader.i32()?;
    let count_offset = reader.position();
    let signed_count = reader.i32()?;
    checked_count_bytes(signed_count, 5, reader.remaining(), MAX_LOOPS, count_offset)?;
    let count = usize::try_from(signed_count).map_err(|_| FramingError::Overflow {
        offset: count_offset,
    })?;
    let mut loops = Vec::with_capacity(count);
    let mut warnings = Vec::new();
    for loop_index in 0..count {
        let loop_offset = reader.position();
        let loop_version = reader.u8()?;
        if loop_version >> 4 != 1 || loop_version & 0x0f > 1 {
            return Err(GeometryError::UnsupportedVersion {
                offset: loop_offset,
                message: format!(
                    "unsupported hatch loop {loop_index} of {count} version {}.{}",
                    loop_version >> 4,
                    loop_version & 0x0f
                ),
            });
        }
        let kind = match reader.i32()? {
            0 => LoopKind::Outer,
            1 => LoopKind::Inner,
            _ => return Err(structural(loop_offset + 1, "invalid hatch loop type")),
        };
        let wrapper_offset = reader.position();
        let wrapper = chunk_at(data, wrapper_offset, reader.end(), archive, false)?;
        let class = parse_class_wrapper(
            data,
            wrapper_offset..wrapper.next_offset,
            archive,
            &mut warnings,
        )?;
        reader.skip(wrapper.next_offset - wrapper_offset)?;
        let decoded =
            crate::curves::decode_2d(data, class.class_uuid, class.class_data_range, archive)?;
        let DecodedGeometry::Curve { curve } = decoded else {
            return Err(structural(
                wrapper_offset,
                "hatch loop object is not a curve",
            ));
        };
        loops.push(HatchLoop { kind, curve });
    }
    let basepoint = if minor >= 2 {
        let offset = reader.position();
        let basepoint = [reader.f64()?, reader.f64()?];
        if !basepoint.into_iter().all(f64::is_finite) {
            return Err(structural(offset, "hatch basepoint is invalid"));
        }
        basepoint
    } else {
        [0.0, 0.0]
    };
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "hatch has trailing bytes"));
    }
    Ok(Hatch {
        source_range: range,
        plane,
        pattern_scale,
        pattern_rotation,
        pattern_index,
        loops,
        basepoint,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive_test_support::{class_wrapper, polyline_payload, POLYLINE_CLASS};

    fn plane_bytes() -> Vec<u8> {
        [
            10.0, 20.0, 30.0, // origin
            0.0, 1.0, 0.0, // x axis
            -1.0, 0.0, 0.0, // y axis
            0.0, 0.0, 1.0, // z axis
            0.0, 0.0, 1.0, -30.0, // equation
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect()
    }

    #[test]
    fn decodes_version_two_loop_geometry_and_pattern_state() {
        let mut loop_payload = polyline_payload(
            &[
                [0.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [2.0, 1.0, 0.0],
                [0.0, 0.0, 0.0],
            ],
            &[0.0, 1.0, 2.0, 3.0],
        );
        let end = loop_payload.len();
        loop_payload[end - 4..].copy_from_slice(&2_i32.to_le_bytes());

        let mut payload = vec![0x12];
        payload.extend(plane_bytes());
        payload.extend(2.5_f64.to_le_bytes());
        payload.extend(0.25_f64.to_le_bytes());
        payload.extend(7_i32.to_le_bytes());
        payload.extend(1_i32.to_le_bytes());
        payload.push(0x11);
        payload.extend(0_i32.to_le_bytes());
        payload.extend(class_wrapper(POLYLINE_CLASS, &loop_payload));
        payload.extend(3.0_f64.to_le_bytes());
        payload.extend(4.0_f64.to_le_bytes());

        let hatch = decode(&payload, 0..payload.len(), 10.0, ArchiveVersion::V8).unwrap();
        assert_eq!(hatch.pattern_index, 7);
        assert_eq!(hatch.pattern_scale, 2.5);
        assert_eq!(hatch.pattern_rotation, 0.25);
        assert_eq!(hatch.basepoint, [3.0, 4.0]);
        assert_eq!(hatch.loops.len(), 1);
        assert_eq!(hatch.loops[0].kind, LoopKind::Outer);
        assert!(matches!(
            hatch.loops[0].curve.geometry,
            cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
        ));
    }
}
