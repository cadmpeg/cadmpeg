// SPDX-License-Identifier: Apache-2.0
//! Bounded hatch payload decoding.
#![deny(clippy::disallowed_methods)]

use std::ops::Range;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::mesh::MeshExpand;

use crate::chunks::{checked_count_bytes, chunk_at, ArchiveVersion, FramingError};
use crate::curves::{DecodedCurve, DecodedGeometry, GeometryError};
use crate::objects::parse_class_wrapper;
use crate::objects::UserdataDescriptor;
use crate::settings::{Plane, Point3, Vector3};
use crate::wire::{scaled_coordinate, ExactVec, Uuid};

pub(crate) const CLASS: Uuid = Uuid::from_canonical([
    0x05, 0x59, 0x73, 0x3b, 0x53, 0x32, 0x49, 0xd1, 0xa9, 0x36, 0x05, 0x32, 0xac, 0x76, 0xad, 0xe5,
]);
const MAX_LOOPS: usize = 1 << 20;
pub(crate) const V5_HATCH_EXTRA: Uuid = Uuid::from_canonical([
    0x3f, 0xf7, 0x00, 0x7c, 0x3d, 0x04, 0x46, 0x3f, 0x84, 0xe3, 0x13, 0x2a, 0xce, 0xb9, 0x10, 0x62,
]);
pub(crate) const GRADIENT_COLOR_DATA: Uuid = Uuid::from_canonical([
    0x0c, 0x1a, 0xd6, 0x13, 0x4e, 0xfa, 0x4f, 0x47, 0xa1, 0x47, 0x4d, 0x79, 0xd7, 0x7f, 0xcb, 0x0c,
]);
const ANONYMOUS: u32 = 0x4000_8000;
const MAX_GRADIENT_STOPS: usize = 1 << 20;

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GradientColorStop {
    pub(crate) color: [u8; 4],
    pub(crate) position: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Gradient {
    pub(crate) kind: i32,
    pub(crate) start: [f64; 3],
    pub(crate) end: [f64; 3],
    pub(crate) repeat: f64,
    pub(crate) colors: Vec<GradientColorStop>,
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
    pub(crate) gradient: Option<Gradient>,
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
    scale: f64,
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
    if major != 1 {
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
        if loop_version >> 4 != 1 {
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
        [
            scaled_coordinate(body.req_f64_le()?, scale).ok_or_else(|| {
                GeometryError::malformed(offset, "scaled hatch basepoint is invalid")
            })?,
            scaled_coordinate(body.req_f64_le()?, scale).ok_or_else(|| {
                GeometryError::malformed(offset, "scaled hatch basepoint is invalid")
            })?,
        ]
    } else {
        [0.0, 0.0]
    };
    body.skip(body.remaining())
        .ok_or_else(|| GeometryError::malformed(body.position(), "hatch suffix is out of range"))?;
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
        gradient: None,
        warnings,
    })
}

pub(crate) fn apply_userdata(
    data: &[u8],
    userdata: &[UserdataDescriptor],
    scale: f64,
    archive: ArchiveVersion,
    hatch: &mut Hatch,
) -> Result<(), GeometryError> {
    let mut last_basepoint = None;
    let mut first_error = None;
    let mut first_gradient = None;
    for extra in userdata
        .iter()
        .filter(|value| value.class_uuid == V5_HATCH_EXTRA)
    {
        match parse_userdata(data, extra, archive, scale) {
            Ok(basepoint) => last_basepoint = Some(basepoint),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    for extra in userdata
        .iter()
        .filter(|value| value.class_uuid == GRADIENT_COLOR_DATA)
    {
        match parse_gradient_userdata(data, extra, scale, archive) {
            Ok(gradient) => {
                first_gradient.get_or_insert(gradient);
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    if let Some(basepoint) = last_basepoint {
        hatch.basepoint = basepoint;
    }
    if let Some(gradient) = first_gradient {
        hatch.gradient = Some(gradient);
    }
    if last_basepoint.is_none() && hatch.gradient.is_none() {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    Ok(())
}

fn parse_gradient_userdata(
    data: &[u8],
    extra: &UserdataDescriptor,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Gradient, GeometryError> {
    let outer = chunk_at(
        data,
        extra.payload_range.start,
        extra.payload_range.end,
        archive,
        false,
    )?;
    if outer.typecode != ANONYMOUS || outer.short {
        return Err(GeometryError::malformed(
            outer.header_start,
            "gradient userdata payload is not an anonymous chunk",
        ));
    }
    let mut reader = crate::chunks::BoundedReader::new(data, outer.body.start, outer.body.end)?;
    let version_offset = reader.position();
    let major = reader.i32()?;
    let _minor = reader.i32()?;
    if major != 1 {
        return Err(GeometryError::UnsupportedVersion {
            offset: version_offset,
            message: format!("unsupported gradient userdata version {major}"),
        });
    }
    let gradient_type_offset = reader.position();
    let gradient_type = reader.i32()?;
    if !(0..=4).contains(&gradient_type) {
        return Err(GeometryError::malformed(
            gradient_type_offset,
            "invalid gradient type",
        ));
    }
    let start = gradient_point(&mut reader, scale, "gradient start point")?;
    let end = gradient_point(&mut reader, scale, "gradient end point")?;
    let repeat_offset = reader.position();
    let repeat = reader.f64()?;
    if !repeat.is_finite() {
        return Err(GeometryError::malformed(
            repeat_offset,
            "gradient repeat is not finite",
        ));
    }
    let count_offset = reader.position();
    let count = reader.i32()?;
    checked_count_bytes(
        count,
        1,
        reader.remaining(),
        MAX_GRADIENT_STOPS,
        count_offset,
    )?;
    let count = usize::try_from(count).map_err(|_| FramingError::Overflow {
        offset: count_offset,
    })?;
    let mut colors = Vec::new();
    colors
        .try_reserve_exact(count)
        .map_err(|_| GeometryError::malformed(count_offset, "gradient color allocation refused"))?;
    for index in 0..count {
        let stop_offset = reader.position();
        let stop = chunk_at(data, stop_offset, outer.body.end, archive, false)?;
        if stop.typecode != ANONYMOUS || stop.short {
            return Err(GeometryError::malformed(
                stop_offset,
                format!("gradient color stop {index} is not an anonymous chunk"),
            ));
        }
        let mut stop_reader =
            crate::chunks::BoundedReader::new(data, stop.body.start, stop.body.end)?;
        let stop_version_offset = stop_reader.position();
        let stop_major = stop_reader.i32()?;
        let _stop_minor = stop_reader.i32()?;
        if stop_major != 1 {
            return Err(GeometryError::UnsupportedVersion {
                offset: stop_version_offset,
                message: format!("unsupported gradient color stop version {stop_major}"),
            });
        }
        let color = stop_reader.array::<4>()?;
        let position_offset = stop_reader.position();
        let position = stop_reader.f64()?;
        if !position.is_finite() {
            return Err(GeometryError::malformed(
                position_offset,
                "gradient color stop position is not finite",
            ));
        }
        stop_reader.skip_remaining()?;
        reader.skip(stop.next_offset - reader.position())?;
        colors.push(GradientColorStop { color, position });
    }
    reader.skip_remaining()?;
    Ok(Gradient {
        kind: gradient_type,
        start,
        end,
        repeat,
        colors,
    })
}

fn gradient_point(
    reader: &mut crate::chunks::BoundedReader<'_>,
    scale: f64,
    label: &str,
) -> Result<[f64; 3], GeometryError> {
    let offset = reader.position();
    let values = [reader.f64()?, reader.f64()?, reader.f64()?];
    let values = values
        .into_iter()
        .map(|value| crate::wire::scaled_coordinate(value, scale))
        .collect::<Option<Vec<_>>>()
        .and_then(|values| values.try_into().ok())
        .ok_or_else(|| GeometryError::malformed(offset, format!("{label} is invalid")))?;
    Ok(values)
}

pub(crate) fn gradient_json(gradient: &Gradient) -> Option<String> {
    let gradient_type = match gradient.kind {
        0 => "none",
        1 => "linear",
        2 => "radial",
        3 => "linear_disabled",
        4 => "radial_disabled",
        _ => return None,
    };
    serde_json::to_string(&serde_json::json!({
        "type": gradient_type,
        "type_value": gradient.kind,
        "start": gradient.start,
        "end": gradient.end,
        "repeat": gradient.repeat,
        "colors": gradient.colors.iter().map(|stop| serde_json::json!({
            "color": stop.color,
            "position": stop.position,
        })).collect::<Vec<_>>(),
    }))
    .ok()
}

fn parse_userdata(
    data: &[u8],
    extra: &UserdataDescriptor,
    archive: ArchiveVersion,
    scale: f64,
) -> Result<[f64; 2], GeometryError> {
    let payload = chunk_at(
        data,
        extra.payload_range.start,
        extra.payload_range.end,
        archive,
        false,
    )?;
    if payload.typecode != ANONYMOUS || payload.short {
        return Err(GeometryError::malformed(
            payload.header_start,
            "V5 hatch userdata payload is not an anonymous chunk",
        ));
    }
    let mut reader = crate::chunks::BoundedReader::new(data, payload.body.start, payload.body.end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::malformed(
            payload.body.start,
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
    reader.skip_remaining()?;
    Ok(basepoint)
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

    fn gradient_userdata_payload(gradient_type: i32, outer_suffix: &[u8]) -> Vec<u8> {
        let mut first_stop = Vec::new();
        first_stop.extend([255, 0, 0, 0]);
        first_stop.extend(0.0_f64.to_le_bytes());
        first_stop.extend([0xaa, 0xbb]);
        let first_stop =
            crate::test_support::test_dump::anonymous_chunk(ArchiveVersion::V8, 0, &first_stop);

        let mut second_stop = Vec::new();
        second_stop.extend([0, 0, 255, 0]);
        second_stop.extend(1.0_f64.to_le_bytes());
        let second_stop =
            crate::test_support::test_dump::anonymous_chunk(ArchiveVersion::V8, 0, &second_stop);

        let mut body = Vec::new();
        body.extend(gradient_type.to_le_bytes());
        for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0, 1.5] {
            body.extend(value.to_le_bytes());
        }
        body.extend(2_i32.to_le_bytes());
        body.extend(first_stop);
        body.extend(second_stop);
        body.extend(outer_suffix);
        crate::test_support::test_dump::anonymous_chunk(ArchiveVersion::V8, 0, &body)
    }

    fn gradient_descriptor(payload: &[u8]) -> UserdataDescriptor {
        UserdataDescriptor {
            range: 0..payload.len(),
            version: (2, 2),
            class_uuid: GRADIENT_COLOR_DATA,
            item_uuid: Uuid::nil(),
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: 0..payload.len(),
            unknown_version: false,
        }
    }

    #[test]
    fn v5_hatch_extra_supplies_scaled_base_point() {
        let payload = version_two_hatch_payload();
        crate::decode::with_expand_bytes(&payload, |expand| {
            let mut hatch =
                decode(expand, 0..payload.len(), 1.0, ArchiveVersion::V5).expect("hatch");
            let mut body = Vec::new();
            body.extend([0; 16]);
            body.extend(2.0_f64.to_le_bytes());
            body.extend(3.0_f64.to_le_bytes());
            let extra =
                crate::test_support::test_dump::anonymous_chunk(ArchiveVersion::V5, 0, &body);
            let descriptor = UserdataDescriptor {
                range: 0..extra.len(),
                version: (2, 2),
                class_uuid: V5_HATCH_EXTRA,
                item_uuid: V5_HATCH_EXTRA,
                copy_count: 0,
                transform_range: 0..0,
                application_uuid: None,
                last_saved_as_goo: None,
                archive_version: None,
                writer_version: None,
                payload_range: 0..extra.len(),
                unknown_version: false,
            };
            apply_userdata(
                &extra,
                std::slice::from_ref(&descriptor),
                10.0,
                ArchiveVersion::V5,
                &mut hatch,
            )
            .expect("hatch extra");
            assert_eq!(hatch.basepoint, [20.0, 30.0]);

            let mut second_body = Vec::new();
            second_body.extend([0; 16]);
            second_body.extend(4.0_f64.to_le_bytes());
            second_body.extend(5.0_f64.to_le_bytes());
            let second = crate::test_support::test_dump::anonymous_chunk(
                ArchiveVersion::V5,
                0,
                &second_body,
            );
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
                ArchiveVersion::V5,
                &mut hatch,
            )
            .expect("duplicate hatch extensions");
            assert_eq!(hatch.basepoint, [40.0, 50.0]);
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
        assert_eq!(hatch.basepoint, [30.0, 40.0]);
        assert_eq!(hatch.loops.len(), 1);
        assert_eq!(hatch.loops[0].kind, LoopKind::Outer);
        assert!(matches!(
            hatch.loops[0].curve.geometry,
            cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
        ));
    }

    #[test]
    fn gradient_userdata_reads_source_fields_and_skips_bounded_suffixes() {
        let payload = gradient_userdata_payload(1, &[0xaa, 0xbb]);
        let hatch_payload = version_two_hatch_payload();
        crate::decode::with_expand_bytes(&hatch_payload, |expand| {
            let mut hatch =
                decode(expand, 0..hatch_payload.len(), 1.0, ArchiveVersion::V8).expect("hatch");
            apply_userdata(
                &payload,
                &[gradient_descriptor(&payload)],
                2.0,
                ArchiveVersion::V8,
                &mut hatch,
            )
            .expect("gradient userdata");
            let gradient = hatch.gradient.expect("gradient");
            assert_eq!(gradient.kind, 1);
            assert_eq!(gradient.start, [2.0, 4.0, 6.0]);
            assert_eq!(gradient.end, [8.0, 10.0, 12.0]);
            assert_eq!(gradient.repeat, 1.5);
            assert_eq!(gradient.colors.len(), 2);
            assert_eq!(gradient.colors[0].color, [255, 0, 0, 0]);
            assert_eq!(gradient.colors[0].position, 0.0);
            assert_eq!(gradient.colors[1].color, [0, 0, 255, 0]);
            assert_eq!(gradient.colors[1].position, 1.0);
            let semantic: serde_json::Value =
                serde_json::from_str(&gradient_json(&gradient).expect("gradient JSON"))
                    .expect("gradient JSON object");
            assert_eq!(semantic["type"], "linear");
            assert_eq!(semantic["type_value"], 1);
            assert_eq!(semantic["start"], serde_json::json!([2.0, 4.0, 6.0]));
        });
    }

    #[test]
    fn gradient_userdata_rejects_an_unknown_gradient_type() {
        let payload = gradient_userdata_payload(5, &[]);
        let hatch_payload = version_two_hatch_payload();
        crate::decode::with_expand_bytes(&hatch_payload, |expand| {
            let mut hatch =
                decode(expand, 0..hatch_payload.len(), 1.0, ArchiveVersion::V8).expect("hatch");
            assert!(apply_userdata(
                &payload,
                &[gradient_descriptor(&payload)],
                1.0,
                ArchiveVersion::V8,
                &mut hatch,
            )
            .is_err());
            assert!(hatch.gradient.is_none());
        });
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
