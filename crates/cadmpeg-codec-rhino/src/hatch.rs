// SPDX-License-Identifier: Apache-2.0
//! Bounded hatch payload decoding.
#![deny(clippy::disallowed_methods)]

use std::ops::Range;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::mesh::MeshExpand;

use crate::chunks::{chunk_at, ArchiveVersion, FramingError};
use crate::curves::{DecodedCurve, DecodedGeometry, GeometryError};
use crate::objects::parse_class_wrapper;
use crate::objects::UserdataDescriptor;
use crate::settings::{Plane, Point3, Vector3};
use crate::wire::{ExactVec, Uuid};

pub(crate) const CLASS: Uuid = Uuid::from_canonical([
    0x05, 0x59, 0x73, 0x3b, 0x53, 0x32, 0x49, 0xd1, 0xa9, 0x36, 0x05, 0x32, 0xac, 0x76, 0xad, 0xe5,
]);
const MAX_LOOPS: usize = 1 << 20;
pub(crate) const V5_HATCH_EXTRA: Uuid = Uuid::from_canonical([
    0x3f, 0xf7, 0x00, 0x7c, 0x3d, 0x04, 0x46, 0x3f, 0x84, 0xe3, 0x13, 0x2a, 0xce, 0xb9, 0x10, 0x62,
]);

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

fn refused(offset: usize, error: &CodecError) -> GeometryError {
    GeometryError::malformed(offset, format!("hatch allocation refused: {error}"))
}

fn coordinate3(view: &mut View<'_>, label: &str) -> Result<[f64; 3], GeometryError> {
    let offset = view.position();
    let values = [view.req_f64_le()?, view.req_f64_le()?, view.req_f64_le()?];
    if values.iter().all(|value| value.is_finite()) {
        Ok(values)
    } else {
        Err(GeometryError::malformed(
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
        view.req_f64_le()?,
        view.req_f64_le()?,
        view.req_f64_le()?,
        view.req_f64_le()?,
    ];
    if !equation.iter().all(|value| value.is_finite()) {
        return Err(GeometryError::malformed(
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
        .ok_or_else(|| GeometryError::malformed(range.start, "hatch body out of range"))?;

    let version_offset = body.position();
    let version = body.req_u8()?;
    let (major, minor) = (version >> 4, version & 0x0f);
    if major != 1 || minor > 2 {
        return Err(GeometryError::UnsupportedVersion {
            offset: version_offset,
            message: format!("unsupported hatch version {major}.{minor}"),
        });
    }
    let plane = read_plane(&mut body)?;
    let scale_offset = body.position();
    let pattern_scale = body.req_f64_le()?;
    if !pattern_scale.is_finite() {
        return Err(GeometryError::malformed(
            scale_offset,
            "hatch pattern scale is not finite",
        ));
    }
    if pattern_scale <= 0.0 {
        return Err(GeometryError::malformed(
            scale_offset,
            "hatch pattern scale is not positive",
        ));
    }
    let rotation_offset = body.position();
    let pattern_rotation = body.req_f64_le()?;
    if !pattern_rotation.is_finite() {
        return Err(GeometryError::malformed(
            rotation_offset,
            "hatch pattern rotation is not finite",
        ));
    }
    let pattern_index = body.req_i32_le()?;

    let count_offset = body.position();
    let signed_count = body.req_i32_le()?;
    let Ok(count) = usize::try_from(signed_count) else {
        return Err(FramingError::Overflow {
            offset: count_offset,
        }
        .into());
    };
    if count > MAX_LOOPS {
        return Err(GeometryError::malformed(
            count_offset,
            "hatch loop count exceeds cap",
        ));
    }
    // A loop contributes at least a five-byte header (`u8` version + `i32`
    // type) before its curve wrapper, so the count is proven against the
    // remaining window at that minimum element size.
    let loop_bound = body.counted(count as u64, 5).ok_or_else(|| {
        GeometryError::malformed(count_offset, "hatch loop count exceeds remaining window")
    })?;
    let mut loops = match ExactVec::<HatchLoop>::new(loop_bound) {
        Ok(loops) => loops,
        Err(error) => return Err(refused(body.position(), &error)),
    };
    let mut warnings = Vec::new();
    for loop_index in 0..count {
        let loop_offset = body.position();
        let loop_version = body.req_u8()?;
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
        let kind = match body.req_i32_le()? {
            0 => LoopKind::Outer,
            1 => LoopKind::Inner,
            _ => {
                return Err(GeometryError::malformed(
                    loop_offset + 1,
                    "invalid hatch loop type",
                ))
            }
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
            .ok_or_else(|| GeometryError::malformed(body.position(), "hatch loop overruns body"))?;
        let decoded =
            crate::curves::decode_2d(data, class.class_uuid, class.class_data_range, archive)?;
        let DecodedGeometry::Curve { curve } = decoded else {
            return Err(GeometryError::malformed(
                wrapper_offset,
                "hatch loop object is not a curve",
            ));
        };
        if let Err(error) = loops.push(HatchLoop { kind, curve }) {
            return Err(refused(body.position(), &error));
        }
        for warning in loop_warnings {
            warnings.push(warning);
        }
    }
    let basepoint = if minor >= 2 {
        let offset = body.position();
        let basepoint = [body.req_f64_le()?, body.req_f64_le()?];
        if !basepoint.into_iter().all(f64::is_finite) {
            return Err(GeometryError::malformed(
                offset,
                "hatch basepoint is invalid",
            ));
        }
        basepoint
    } else {
        [0.0, 0.0]
    };
    if body.remaining() != 0 {
        return Err(GeometryError::malformed(
            body.position(),
            "hatch has trailing bytes",
        ));
    }
    let loops = match loops.finish() {
        Ok(loops) => loops,
        Err(error) => return Err(refused(body.position(), &error)),
    };
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

pub(crate) fn apply_userdata(
    data: &[u8],
    userdata: &[UserdataDescriptor],
    scale: f64,
    hatch: &mut Hatch,
) -> Result<(), GeometryError> {
    let Some(extra) = userdata
        .iter()
        .find(|value| value.class_uuid == V5_HATCH_EXTRA)
    else {
        return Ok(());
    };
    let mut reader = crate::chunks::BoundedReader::new(
        data,
        extra.payload_range.start,
        extra.payload_range.end,
    )?;
    if reader.i32()? != 1 || reader.i32()? < 0 {
        return Err(GeometryError::malformed(
            reader.position() - 8,
            "unsupported V5 hatch-extra version",
        ));
    }
    reader.take(16)?;
    let basepoint = [
        crate::wire::scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
            GeometryError::malformed(reader.position() - 8, "invalid V5 hatch base point")
        })?,
        crate::wire::scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
            GeometryError::malformed(reader.position() - 8, "invalid V5 hatch base point")
        })?,
    ];
    hatch.basepoint = basepoint;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_support::{class_wrapper, polyline_payload, POLYLINE_CLASS};

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

    pub(crate) fn version_two_hatch_payload() -> Vec<u8> {
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
    fn v5_hatch_extra_supplies_scaled_base_point() {
        let payload = version_two_hatch_payload();
        crate::decode::with_expand_bytes(&payload, |expand| {
            let mut hatch =
                decode(expand, 0..payload.len(), 1.0, ArchiveVersion::V5).expect("hatch");
            let mut extra = 1_i32.to_le_bytes().to_vec();
            extra.extend(0_i32.to_le_bytes());
            extra.extend([0; 16]);
            extra.extend(2.0_f64.to_le_bytes());
            extra.extend(3.0_f64.to_le_bytes());
            let descriptor = UserdataDescriptor {
                range: 0..extra.len(),
                version: (2, 3),
                class_uuid: V5_HATCH_EXTRA,
                item_uuid: Uuid::nil(),
                copy_count: 0,
                transform_range: 0..0,
                application_uuid: None,
                last_saved_as_goo: None,
                archive_version: None,
                writer_version: None,
                payload_range: 0..extra.len(),
                unknown_version: false,
            };
            apply_userdata(&extra, std::slice::from_ref(&descriptor), 10.0, &mut hatch)
                .expect("hatch extra");
            assert_eq!(hatch.basepoint, [20.0, 30.0]);

            let mut second = 1_i32.to_le_bytes().to_vec();
            second.extend(0_i32.to_le_bytes());
            second.extend([0; 16]);
            second.extend(4.0_f64.to_le_bytes());
            second.extend(5.0_f64.to_le_bytes());
            let second_start = extra.len();
            let mut combined = extra.clone();
            combined.extend(second);
            let mut second_descriptor = descriptor.clone();
            second_descriptor.range = second_start..combined.len();
            second_descriptor.payload_range = second_start..combined.len();
            apply_userdata(
                &combined,
                &[descriptor, second_descriptor],
                10.0,
                &mut hatch,
            )
            .expect("first duplicate hatch extension");
            assert_eq!(hatch.basepoint, [20.0, 30.0]);
        });
    }

    #[test]
    fn decodes_version_two_loop_geometry_and_pattern_state() {
        let payload = version_two_hatch_payload();
        let hatch = crate::decode::with_expand_bytes(&payload, |expand| {
            decode(expand, 0..payload.len(), 10.0, ArchiveVersion::V8)
        })
        .expect("required invariant");
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
