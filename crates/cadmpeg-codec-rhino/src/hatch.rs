// SPDX-License-Identifier: Apache-2.0
//! Bounded hatch payload decoding.
#![deny(clippy::disallowed_methods)]

use std::ops::Range;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::View;

use crate::mesh::MeshExpand;

use crate::chunks::{chunk_at, ArchiveVersion, FramingError};
use crate::curves::{DecodedCurve, DecodedGeometry, GeometryError};
use crate::objects::parse_class_wrapper;
use crate::settings::{Plane, Point3, Vector3};
use crate::wire::{ExactVec, Uuid};

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

fn refused(offset: usize, error: &CodecError) -> GeometryError {
    structural(offset, format!("hatch allocation refused: {error}"))
}

fn req_u8(view: &mut View<'_>) -> Result<u8, GeometryError> {
    let offset = view.position();
    view.req_u8()
        .map_err(|_| structural(offset, "hatch record truncated"))
}

fn req_i32(view: &mut View<'_>) -> Result<i32, GeometryError> {
    let offset = view.position();
    view.req_i32_le()
        .map_err(|_| structural(offset, "hatch record truncated"))
}

fn req_f64(view: &mut View<'_>) -> Result<f64, GeometryError> {
    let offset = view.position();
    view.req_f64_le()
        .map_err(|_| structural(offset, "hatch record truncated"))
}

fn coordinate3(view: &mut View<'_>, label: &str) -> Result<[f64; 3], GeometryError> {
    let offset = view.position();
    let values = [req_f64(view)?, req_f64(view)?, req_f64(view)?];
    if values.iter().all(|value| value.is_finite()) {
        Ok(values)
    } else {
        Err(structural(
            offset,
            format!("{label} contains a nonfinite value"),
        ))
    }
}

fn read_plane(view: &mut View<'_>) -> Result<Plane, GeometryError> {
    let origin = Point3(coordinate3(view, "point")?);
    let xaxis = Vector3(coordinate3(view, "vector")?);
    let yaxis = Vector3(coordinate3(view, "vector")?);
    let zaxis = Vector3(coordinate3(view, "vector")?);
    let equation_offset = view.position();
    let equation = [
        req_f64(view)?,
        req_f64(view)?,
        req_f64(view)?,
        req_f64(view)?,
    ];
    if !equation.iter().all(|value| value.is_finite()) {
        return Err(structural(
            equation_offset,
            "plane equation contains a nonfinite value",
        ));
    }
    Ok(Plane {
        origin,
        xaxis,
        yaxis,
        zaxis,
        equation,
    })
}

pub(crate) fn decode(
    expand: MeshExpand<'_>,
    range: Range<usize>,
    _scale: f64,
    archive: ArchiveVersion,
) -> Result<Hatch, GeometryError> {
    let data = expand.data();
    let mut body = expand
        .root()
        .child(range.start, range.end)
        .ok_or_else(|| structural(range.start, "hatch body out of range"))?;

    let version_offset = body.position();
    let version = req_u8(&mut body)?;
    let (major, minor) = (version >> 4, version & 0x0f);
    if major != 1 || minor > 2 {
        return Err(GeometryError::UnsupportedVersion {
            offset: version_offset,
            message: format!("unsupported hatch version {major}.{minor}"),
        });
    }
    let plane = read_plane(&mut body)?;
    let scale_offset = body.position();
    let pattern_scale = req_f64(&mut body)?;
    if !pattern_scale.is_finite() {
        return Err(structural(
            scale_offset,
            "hatch pattern scale is not finite",
        ));
    }
    if pattern_scale <= 0.0 {
        return Err(structural(
            scale_offset,
            "hatch pattern scale is not positive",
        ));
    }
    let rotation_offset = body.position();
    let pattern_rotation = req_f64(&mut body)?;
    if !pattern_rotation.is_finite() {
        return Err(structural(
            rotation_offset,
            "hatch pattern rotation is not finite",
        ));
    }
    let pattern_index = req_i32(&mut body)?;

    let count_offset = body.position();
    let signed_count = req_i32(&mut body)?;
    let count = usize::try_from(signed_count).map_err(|_| FramingError::Overflow {
        offset: count_offset,
    })?;
    if count > MAX_LOOPS {
        return Err(structural(count_offset, "hatch loop count exceeds cap"));
    }
    // A loop contributes at least a five-byte header (`u8` version + `i32`
    // type) before its curve wrapper, so the count is proven against the
    // remaining window at that minimum element size.
    let loop_bound = body
        .counted(count as u64, 5)
        .ok_or_else(|| structural(count_offset, "hatch loop count exceeds remaining window"))?;
    let mut loops =
        ExactVec::<HatchLoop>::new(loop_bound).map_err(|error| refused(body.position(), &error))?;
    let mut warnings = Vec::new();
    for loop_index in 0..count {
        let loop_offset = body.position();
        let loop_version = req_u8(&mut body)?;
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
        let kind = match req_i32(&mut body)? {
            0 => LoopKind::Outer,
            1 => LoopKind::Inner,
            _ => return Err(structural(loop_offset + 1, "invalid hatch loop type")),
        };
        let wrapper_offset = body.position();
        let wrapper = chunk_at(data, wrapper_offset, range.end, archive, false)?;
        let mut loop_warnings = Vec::new();
        let class = parse_class_wrapper(
            data,
            wrapper_offset..wrapper.next_offset,
            archive,
            &mut loop_warnings,
        )?;
        body.skip(wrapper.next_offset - wrapper_offset)
            .ok_or_else(|| structural(body.position(), "hatch loop overruns body"))?;
        let decoded =
            crate::curves::decode_2d(data, class.class_uuid, class.class_data_range, archive)?;
        let DecodedGeometry::Curve { curve } = decoded else {
            return Err(structural(
                wrapper_offset,
                "hatch loop object is not a curve",
            ));
        };
        loops
            .push(HatchLoop { kind, curve })
            .map_err(|error| refused(body.position(), &error))?;
        for warning in loop_warnings {
            warnings.push(warning);
        }
    }
    let basepoint = if minor >= 2 {
        let offset = body.position();
        let basepoint = [req_f64(&mut body)?, req_f64(&mut body)?];
        if !basepoint.into_iter().all(f64::is_finite) {
            return Err(structural(offset, "hatch basepoint is invalid"));
        }
        basepoint
    } else {
        [0.0, 0.0]
    };
    if body.remaining() != 0 {
        return Err(structural(body.position(), "hatch has trailing bytes"));
    }
    let loops = loops
        .finish()
        .map_err(|error| refused(body.position(), &error))?;
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

    fn version_two_hatch_payload() -> Vec<u8> {
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
        payload
    }

    #[test]
    fn decodes_version_two_loop_geometry_and_pattern_state() {
        let payload = version_two_hatch_payload();
        let hatch = crate::decode::with_expand_bytes(&payload, |expand| {
            decode(expand, 0..payload.len(), 10.0, ArchiveVersion::V8)
        })
        .unwrap();
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

    #[test]
    fn truncating_the_loop_record_is_rejected_at_the_record_boundary() {
        // Drop the trailing basepoint and the tail of the loop's curve wrapper so
        // the count-framed loop's child record runs past the body's proven window.
        let mut payload = version_two_hatch_payload();
        payload.truncate(payload.len() - 24);
        assert!(crate::decode::with_expand_bytes(&payload, |expand| decode(
            expand,
            0..payload.len(),
            10.0,
            ArchiveVersion::V8
        ))
        .is_err());
    }
}
