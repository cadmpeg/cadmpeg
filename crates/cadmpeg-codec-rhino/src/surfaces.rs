// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino NURBS and plane-surface payload decoding.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::ops::Range;

use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::chunks::{checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader};
use crate::curves::{
    decode_embedded_curve, error, exact_nurbs, unsupported, DecodedCurve, GeometryError,
    MAX_CURVE_ITEMS,
};
use crate::settings::{
    bbox, interval, plane, point, vector as native_vector, Plane, Point3 as NativePoint3,
};
use crate::wire::Uuid;

pub(crate) const NURBS_CURVE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xdd, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const NURBS_SURFACE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xde, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const NURBS_SURFACE_TL: Uuid = Uuid::from_canonical([
    0x47, 0x60, 0xc8, 0x17, 0x0b, 0xe3, 0x11, 0xd4, 0xbf, 0xfe, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const NURBS_SURFACE_LEGACY: Uuid = Uuid::from_canonical([
    0xfa, 0x4f, 0xd4, 0xb5, 0x16, 0x13, 0x11, 0xd4, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const PLANE_SURFACE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xdf, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const CLIPPING_PLANE_SURFACE: Uuid = Uuid::from_canonical([
    0xdb, 0xc5, 0xa5, 0x84, 0xce, 0x3f, 0x41, 0x70, 0x98, 0xa8, 0x49, 0x70, 0x69, 0xca, 0x5c, 0x36,
]);
pub(crate) const REV_SURFACE: Uuid = Uuid::from_canonical([
    0xa1, 0x62, 0x20, 0xd3, 0x16, 0x3b, 0x11, 0xd4, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const REV_SURFACE_LEGACY: Uuid = Uuid::from_canonical([
    0x0a, 0x84, 0x01, 0xb6, 0x4d, 0x34, 0x4b, 0x99, 0x86, 0x15, 0x1b, 0x4e, 0x72, 0x3d, 0xc4, 0xe5,
]);
pub(crate) const SUM_SURFACE: Uuid = Uuid::from_canonical([
    0xc4, 0xcd, 0x53, 0x59, 0x44, 0x6d, 0x46, 0x90, 0x9f, 0xf5, 0x29, 0x05, 0x97, 0x32, 0x47, 0x2b,
]);

/// Returns whether a class is one of the native procedural surfaces.
pub(crate) fn is_procedural_class(uuid: Uuid) -> bool {
    matches!(uuid, REV_SURFACE | REV_SURFACE_LEGACY | SUM_SURFACE)
}

#[derive(Debug, Clone)]
pub(crate) enum DecodedSurface {
    /// A typed surface and its conversion state.
    Typed {
        /// Decoded surface geometry.
        geometry: SurfaceGeometry,
        /// Whether native coordinates were scaled or reconstructed.
        derived: bool,
    },
    /// A solved native procedural surface and its ordered child trees.
    Procedural {
        /// Exact solved NURBS carrier.
        geometry: NurbsSurface,
        /// Native construction fields.
        definition: DecodedProceduralSurface,
        /// Ordered embedded child curves.
        children: Vec<DecodedCurve>,
    },
}

/// Native procedural fields before deterministic child IDs are assigned.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DecodedProceduralSurface {
    /// Revolution of the first child.
    Revolution {
        /// Scaled axis origin.
        axis_origin: Point3,
        /// Unit axis direction.
        axis_direction: Vector3,
        /// Native angular interval.
        angular_interval: [f64; 2],
        /// Native revolution parameter interval.
        parameter_interval: [f64; 2],
        /// Source parameter-direction transpose flag.
        transposed: bool,
    },
    /// Sum of the first and second children.
    Sum {
        /// Scaled basepoint vector.
        basepoint: Vector3,
    },
}

pub(crate) fn decode(
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedSurface, GeometryError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let result = if matches!(
        class,
        NURBS_SURFACE | NURBS_SURFACE_TL | NURBS_SURFACE_LEGACY
    ) {
        DecodedSurface::Typed {
            geometry: SurfaceGeometry::Nurbs(read_nurbs_surface(&mut reader, scale)?),
            derived: true,
        }
    } else if class == PLANE_SURFACE {
        let geometry = read_plane_surface(&mut reader, scale)?;
        DecodedSurface::Typed {
            geometry,
            derived: scale != 1.0,
        }
    } else if class == CLIPPING_PLANE_SURFACE {
        read_clipping_plane_surface(data, &mut reader, scale, archive)?
    } else if matches!(class, REV_SURFACE | REV_SURFACE_LEGACY) {
        read_revolution(data, &mut reader, scale, archive, depth)?
    } else if class == SUM_SURFACE {
        read_sum(data, &mut reader, scale, archive, depth)?
    } else {
        return Err(unsupported(range.start, "unsupported Rhino surface class"));
    };
    if reader.remaining() != 0 {
        return Err(error(
            reader.position(),
            "surface payload has trailing bytes",
        ));
    }
    Ok(result)
}

fn read_clipping_plane_surface(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<DecodedSurface, GeometryError> {
    const ANONYMOUS: u32 = 0x4000_8000;
    let outer = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if outer.typecode != ANONYMOUS || outer.short || outer.next_offset != reader.end() {
        return Err(error(
            reader.position(),
            "invalid clipping-plane outer chunk",
        ));
    }
    let mut payload = BoundedReader::new(data, outer.body.start, outer.body.end)?;
    if payload.i32()? != 1 || payload.i32()? != 0 {
        return Err(unsupported(
            outer.body.start,
            "unsupported clipping-plane surface version",
        ));
    }
    let plane_chunk = chunk_at(data, payload.position(), payload.end(), archive, false)?;
    if plane_chunk.typecode != ANONYMOUS || plane_chunk.short {
        return Err(error(
            plane_chunk.header_start,
            "invalid clipping-plane carrier chunk",
        ));
    }
    let mut plane_reader = BoundedReader::new(data, plane_chunk.body.start, plane_chunk.body.end)?;
    let geometry = read_plane_surface(&mut plane_reader, scale)?;
    if plane_reader.remaining() != 0 {
        return Err(error(
            plane_reader.position(),
            "clipping-plane carrier has trailing bytes",
        ));
    }
    payload.skip(plane_chunk.next_offset - payload.position())?;
    read_clipping_plane(data, &mut payload, archive)?;
    if payload.remaining() != 0 {
        return Err(error(
            payload.position(),
            "clipping-plane surface has trailing bytes",
        ));
    }
    reader.skip(outer.next_offset - reader.position())?;
    Ok(DecodedSurface::Typed {
        geometry,
        derived: scale != 1.0,
    })
}

fn read_clipping_plane(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(), GeometryError> {
    const ANONYMOUS: u32 = 0x4000_8000;
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(error(chunk.header_start, "invalid clipping-plane chunk"));
    }
    let mut payload = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if payload.i32()? != 1 {
        return Err(unsupported(
            chunk.body.start,
            "unsupported clipping-plane major version",
        ));
    }
    let minor = payload.i32()?;
    if !(0..=5).contains(&minor) {
        return Err(unsupported(
            chunk.body.start,
            "unsupported clipping-plane minor version",
        ));
    }
    let first_viewport = Uuid::from_wire(payload.array()?);
    let _plane_id = Uuid::from_wire(payload.array()?);
    let native_plane = plane(&mut payload)?;
    validate_plane(native_plane, payload.position())?;
    let _enabled = payload.bool()?;
    if minor == 0 {
        let _ = first_viewport;
    } else {
        read_uuid_list(data, &mut payload, archive)?;
    }
    if minor >= 2 {
        let depth = payload.f64()?;
        if !depth.is_finite() {
            return Err(error(payload.position() - 8, "invalid clipping depth"));
        }
    }
    if minor >= 4 {
        payload.bool()?;
    }
    if minor >= 5 {
        read_clipping_participation(&mut payload)?;
    }
    if payload.remaining() != 0 {
        return Err(error(
            payload.position(),
            "clipping plane has trailing bytes",
        ));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(())
}

fn read_uuid_list(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(), GeometryError> {
    const ANONYMOUS: u32 = 0x4000_8000;
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(error(chunk.header_start, "invalid clipping viewport list"));
    }
    let mut payload = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if payload.i32()? != 1 || payload.i32()? != 0 {
        return Err(unsupported(
            chunk.body.start,
            "unsupported clipping viewport-list version",
        ));
    }
    let count = checked_count(&mut payload, 16)?;
    payload.skip(count * 16)?;
    if payload.remaining() != 0 {
        return Err(error(
            payload.position(),
            "clipping viewport list has trailing bytes",
        ));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(())
}

fn read_clipping_participation(reader: &mut BoundedReader<'_>) -> Result<(), GeometryError> {
    let mut item = reader.u8()?;
    if item == 10 {
        let count = checked_count(reader, 16)?;
        reader.skip(count * 16)?;
        item = reader.u8()?;
    }
    if item == 11 {
        let count = checked_count(reader, 4)?;
        reader.skip(count * 4)?;
        item = reader.u8()?;
    }
    if item == 12 {
        reader.bool()?;
        item = reader.u8()?;
    }
    if item == 13 {
        reader.bool()?;
        item = reader.u8()?;
    }
    if item != 0 {
        return Err(error(
            reader.position() - 1,
            "clipping participation item is invalid or out of order",
        ));
    }
    Ok(())
}

fn read_revolution(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedSurface, GeometryError> {
    let version_offset = reader.position();
    let version = reader.u8()?;
    let major = version >> 4;
    if !(major == 1 || major == 2) || version & 0x0f != 0 {
        return Err(unsupported(
            version_offset,
            "unsupported revolution-surface version",
        ));
    }
    let from = scale_native_point(point(reader)?, scale)
        .ok_or_else(|| error(reader.position(), "scaled revolution axis is invalid"))?;
    let to = scale_native_point(point(reader)?, scale)
        .ok_or_else(|| error(reader.position(), "scaled revolution axis is invalid"))?;
    let angular_interval =
        finite_increasing(interval(reader)?.0, reader.position(), "revolution angle")?;
    if angular_interval[1] - angular_interval[0] > TAU + 1.0e-10 {
        return Err(error(
            reader.position(),
            "revolution angle span exceeds one turn",
        ));
    }
    let parameter_interval = if major >= 2 {
        finite_increasing(
            interval(reader)?.0,
            reader.position(),
            "revolution parameter interval",
        )?
    } else {
        angular_interval
    };
    bbox(reader)?;
    let transposed = match reader.i32()? {
        0 => false,
        1 => true,
        _ => {
            return Err(error(
                reader.position(),
                "revolution transpose flag is invalid",
            ))
        }
    };
    if reader.u8()? != 1 {
        return Err(error(
            reader.position(),
            "revolution profile presence flag is invalid",
        ));
    }
    let axis_delta = Vector3::new(to.x - from.x, to.y - from.y, to.z - from.z);
    let axis_length = axis_delta.norm();
    if !axis_length.is_finite() || axis_length <= 0.0 {
        return Err(error(reader.position(), "revolution axis is invalid"));
    }
    let axis_direction = Vector3::new(
        axis_delta.x / axis_length,
        axis_delta.y / axis_length,
        axis_delta.z / axis_length,
    );
    let child = decode_embedded_curve(data, reader, scale, archive, depth + 1)?;
    let profile = exact_nurbs(&child, version_offset)?;
    let geometry = revolution_nurbs(
        &profile,
        from,
        axis_direction,
        angular_interval,
        parameter_interval,
        transposed,
        version_offset,
    )?;
    Ok(DecodedSurface::Procedural {
        geometry,
        definition: DecodedProceduralSurface::Revolution {
            axis_origin: from,
            axis_direction,
            angular_interval,
            parameter_interval,
            transposed,
        },
        children: vec![child],
    })
}

fn read_sum(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedSurface, GeometryError> {
    let version_offset = reader.position();
    if reader.u8()? != 0x10 {
        return Err(unsupported(
            version_offset,
            "unsupported sum-surface version",
        ));
    }
    let native = native_vector(reader)?;
    let basepoint = Vector3::new(
        crate::wire::scaled_coordinate(native.0[0], scale)
            .ok_or_else(|| error(reader.position(), "scaled sum basepoint is invalid"))?,
        crate::wire::scaled_coordinate(native.0[1], scale)
            .ok_or_else(|| error(reader.position(), "scaled sum basepoint is invalid"))?,
        crate::wire::scaled_coordinate(native.0[2], scale)
            .ok_or_else(|| error(reader.position(), "scaled sum basepoint is invalid"))?,
    );
    bbox(reader)?;
    let first = decode_embedded_curve(data, reader, scale, archive, depth + 1)?;
    let second = decode_embedded_curve(data, reader, scale, archive, depth + 1)?;
    let first_nurbs = exact_nurbs(&first, version_offset)?;
    let second_nurbs = exact_nurbs(&second, version_offset)?;
    let geometry = sum_nurbs(&first_nurbs, &second_nurbs, basepoint, version_offset)?;
    Ok(DecodedSurface::Procedural {
        geometry,
        definition: DecodedProceduralSurface::Sum { basepoint },
        children: vec![first, second],
    })
}

fn revolution_nurbs(
    profile: &NurbsCurve,
    axis_origin: Point3,
    axis: Vector3,
    angle: [f64; 2],
    parameter: [f64; 2],
    transposed: bool,
    offset: usize,
) -> Result<NurbsSurface, GeometryError> {
    validate_curve_shape(profile, offset)?;
    let span_count = ((angle[1] - angle[0]) / FRAC_PI_2).ceil().max(1.0) as usize;
    let angular_count = span_count
        .checked_mul(2)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| error(offset, "revolution control count overflow"))?;
    let profile_count = profile.control_points.len();
    angular_count
        .checked_mul(profile_count)
        .filter(|count| *count <= MAX_CURVE_ITEMS)
        .ok_or_else(|| error(offset, "revolution surface is too large"))?;
    let angle_step = (angle[1] - angle[0]) / span_count as f64;
    let parameter_step = (parameter[1] - parameter[0]) / span_count as f64;
    let mut angular = Vec::with_capacity(angular_count);
    let mut angular_weights = Vec::with_capacity(angular_count);
    let mut knots = Vec::with_capacity(angular_count + 3);
    for span in 0..span_count {
        let a0 = angle[0] + angle_step * span as f64;
        let a1 = angle[0] + angle_step * (span + 1) as f64;
        let middle = (a0 + a1) * 0.5;
        let middle_weight = ((a1 - a0) * 0.5).cos();
        if span == 0 {
            angular.push((a0, 1.0));
            angular_weights.push(1.0);
        }
        angular.push((middle, 1.0 / middle_weight));
        angular_weights.push(middle_weight);
        angular.push((a1, 1.0));
        angular_weights.push(1.0);
        let t0 = parameter[0] + parameter_step * span as f64;
        let t1 = parameter[0] + parameter_step * (span + 1) as f64;
        if span == 0 {
            knots.extend([t0, t0, t0]);
        } else {
            knots.extend([t0, t0]);
        }
        if span + 1 == span_count {
            knots.extend([t1, t1, t1]);
        }
    }
    let profile_weights = profile
        .weights
        .clone()
        .unwrap_or_else(|| vec![1.0; profile_count]);
    let mut control_points = Vec::with_capacity(angular_count * profile_count);
    let mut weights = Vec::with_capacity(control_points.capacity());
    for ((theta, radial_scale), angular_weight) in angular.into_iter().zip(angular_weights) {
        for (profile_point, profile_weight) in profile
            .control_points
            .iter()
            .zip(profile_weights.iter().copied())
        {
            let relative = Vector3::new(
                profile_point.x - axis_origin.x,
                profile_point.y - axis_origin.y,
                profile_point.z - axis_origin.z,
            );
            let axial_length = dot(relative, axis);
            let axial = scale_vector(axis, axial_length);
            let radial = subtract(relative, axial);
            let rotated = rodrigues(radial, axis, theta);
            let point =
                add_point_vector(axis_origin, add(axial, scale_vector(rotated, radial_scale)));
            control_points.push(point);
            weights.push(profile_weight * angular_weight);
        }
    }
    let mut result = NurbsSurface {
        u_degree: 2,
        v_degree: profile.degree,
        u_knots: knots,
        v_knots: profile.knots.clone(),
        u_count: u32::try_from(angular_count)
            .map_err(|_| error(offset, "revolution U count overflow"))?,
        v_count: u32::try_from(profile_count)
            .map_err(|_| error(offset, "revolution V count overflow"))?,
        control_points,
        weights: Some(weights),
        u_periodic: false,
        v_periodic: profile.periodic,
    };
    if transposed {
        transpose_surface(&mut result, offset)?;
    }
    Ok(result)
}

fn sum_nurbs(
    first: &NurbsCurve,
    second: &NurbsCurve,
    basepoint: Vector3,
    offset: usize,
) -> Result<NurbsSurface, GeometryError> {
    validate_curve_shape(first, offset)?;
    validate_curve_shape(second, offset)?;
    let u_count = first.control_points.len();
    let v_count = second.control_points.len();
    u_count
        .checked_mul(v_count)
        .filter(|count| *count <= MAX_CURVE_ITEMS)
        .ok_or_else(|| error(offset, "sum surface is too large"))?;
    let first_weights = first.weights.clone().unwrap_or_else(|| vec![1.0; u_count]);
    let second_weights = second.weights.clone().unwrap_or_else(|| vec![1.0; v_count]);
    let rational = first.weights.is_some() || second.weights.is_some();
    let mut control_points = Vec::with_capacity(u_count * v_count);
    let mut weights = rational.then(|| Vec::with_capacity(control_points.capacity()));
    for (first_point, first_weight) in first
        .control_points
        .iter()
        .zip(first_weights.iter().copied())
    {
        for (second_point, second_weight) in second
            .control_points
            .iter()
            .zip(second_weights.iter().copied())
        {
            let product = first_weight * second_weight;
            if !product.is_finite() || product == 0.0 {
                return Err(error(offset, "sum surface weight is invalid"));
            }
            control_points.push(Point3::new(
                first_point.x + second_point.x + basepoint.x,
                first_point.y + second_point.y + basepoint.y,
                first_point.z + second_point.z + basepoint.z,
            ));
            if let Some(values) = &mut weights {
                values.push(product);
            }
        }
    }
    Ok(NurbsSurface {
        u_degree: first.degree,
        v_degree: second.degree,
        u_knots: first.knots.clone(),
        v_knots: second.knots.clone(),
        u_count: u32::try_from(u_count).map_err(|_| error(offset, "sum U count overflow"))?,
        v_count: u32::try_from(v_count).map_err(|_| error(offset, "sum V count overflow"))?,
        control_points,
        weights,
        u_periodic: first.periodic,
        v_periodic: second.periodic,
    })
}

/// Constructs the exact degree-one tensor interpolation between two profile curves.
pub(crate) fn extrusion_nurbs(
    start: &NurbsCurve,
    end: &NurbsCurve,
    path_domain: [f64; 2],
    transposed: bool,
    offset: usize,
) -> Result<NurbsSurface, GeometryError> {
    validate_curve_shape(start, offset)?;
    validate_curve_shape(end, offset)?;
    if start.degree != end.degree
        || start.knots != end.knots
        || start.control_points.len() != end.control_points.len()
        || start.weights != end.weights
        || start.periodic != end.periodic
        || !path_domain.iter().all(|value| value.is_finite())
        || path_domain[0] >= path_domain[1]
    {
        return Err(error(offset, "extrusion tensor inputs are incompatible"));
    }
    let profile_count = start.control_points.len();
    profile_count
        .checked_mul(2)
        .filter(|count| *count <= MAX_CURVE_ITEMS)
        .ok_or_else(|| error(offset, "extrusion surface is too large"))?;
    let mut control_points = Vec::with_capacity(profile_count * 2);
    let mut weights = start
        .weights
        .as_ref()
        .map(|_| Vec::with_capacity(profile_count * 2));
    for index in 0..profile_count {
        control_points.push(start.control_points[index]);
        control_points.push(end.control_points[index]);
        if let (Some(source), Some(target)) = (&start.weights, &mut weights) {
            target.push(source[index]);
            target.push(source[index]);
        }
    }
    let mut surface = NurbsSurface {
        u_degree: start.degree,
        v_degree: 1,
        u_knots: start.knots.clone(),
        v_knots: vec![
            path_domain[0],
            path_domain[0],
            path_domain[1],
            path_domain[1],
        ],
        u_count: u32::try_from(profile_count)
            .map_err(|_| error(offset, "extrusion profile count overflow"))?,
        v_count: 2,
        control_points,
        weights,
        u_periodic: start.periodic,
        v_periodic: false,
    };
    if transposed {
        transpose_surface(&mut surface, offset)?;
    }
    Ok(surface)
}

fn validate_curve_shape(curve: &NurbsCurve, offset: usize) -> Result<(), GeometryError> {
    let expected_knots = usize::try_from(curve.degree)
        .ok()
        .and_then(|degree| degree.checked_add(curve.control_points.len()))
        .and_then(|value| value.checked_add(1));
    if curve.control_points.is_empty()
        || expected_knots != Some(curve.knots.len())
        || curve
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != curve.control_points.len())
    {
        return Err(error(offset, "child NURBS shape is invalid"));
    }
    Ok(())
}

fn transpose_surface(surface: &mut NurbsSurface, offset: usize) -> Result<(), GeometryError> {
    let old_u = usize::try_from(surface.u_count)
        .map_err(|_| error(offset, "surface U count does not fit memory"))?;
    let old_v = usize::try_from(surface.v_count)
        .map_err(|_| error(offset, "surface V count does not fit memory"))?;
    let mut points = Vec::with_capacity(surface.control_points.len());
    let mut weights = surface
        .weights
        .as_ref()
        .map(|_| Vec::with_capacity(surface.control_points.len()));
    for new_u in 0..old_v {
        for new_v in 0..old_u {
            let old_index = new_v * old_v + new_u;
            points.push(surface.control_points[old_index]);
            if let (Some(source), Some(target)) = (&surface.weights, &mut weights) {
                target.push(source[old_index]);
            }
        }
    }
    std::mem::swap(&mut surface.u_degree, &mut surface.v_degree);
    std::mem::swap(&mut surface.u_knots, &mut surface.v_knots);
    std::mem::swap(&mut surface.u_count, &mut surface.v_count);
    std::mem::swap(&mut surface.u_periodic, &mut surface.v_periodic);
    surface.control_points = points;
    surface.weights = weights;
    Ok(())
}

fn scale_vector(value: Vector3, factor: f64) -> Vector3 {
    Vector3::new(value.x * factor, value.y * factor, value.z * factor)
}

fn add(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn subtract(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn add_point_vector(point: Point3, vector: Vector3) -> Point3 {
    Point3::new(point.x + vector.x, point.y + vector.y, point.z + vector.z)
}

fn rodrigues(value: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cosine = angle.cos();
    let sine = angle.sin();
    let cross = Vector3::new(
        axis.y * value.z - axis.z * value.y,
        axis.z * value.x - axis.x * value.z,
        axis.x * value.y - axis.y * value.x,
    );
    add(
        add(scale_vector(value, cosine), scale_vector(cross, sine)),
        scale_vector(axis, dot(axis, value) * (1.0 - cosine)),
    )
}

pub(crate) fn read_nurbs_curve(
    reader: &mut BoundedReader<'_>,
    scale: f64,
) -> Result<NurbsCurve, GeometryError> {
    read_nurbs_curve_inner(reader, scale, Some(3))
}

/// Reads a Rhino NURBS curve whose poles are two-dimensional UV values.
pub(crate) fn read_nurbs_curve_2d(
    reader: &mut BoundedReader<'_>,
) -> Result<NurbsCurve, GeometryError> {
    read_nurbs_curve_inner(reader, 1.0, Some(2))
}

fn read_nurbs_curve_inner(
    reader: &mut BoundedReader<'_>,
    scale: f64,
    expected_dimension: Option<i32>,
) -> Result<NurbsCurve, GeometryError> {
    let version_offset = reader.position();
    let version = reader.u8()?;
    let major = version >> 4;
    let minor = version & 0x0f;
    if major != 1 {
        return Err(unsupported(
            version_offset,
            "unsupported NURBS curve version",
        ));
    }
    if minor > 1 {
        return Err(unsupported(
            version_offset,
            "unsupported NURBS curve minor version",
        ));
    }
    let dimension = reader.i32()?;
    let rational = reader.i32()?;
    let order = checked_positive(reader.i32()?, reader.position(), "curve order")?;
    let cv_count = checked_positive(reader.i32()?, reader.position(), "curve CV count")?;
    reader.i32()?;
    reader.i32()?;
    reader.skip(48)?;
    if expected_dimension.is_some_and(|expected| dimension != expected)
        || !(2..=3).contains(&dimension)
        || !(rational == 0 || rational == 1)
        || cv_count < order
    {
        return Err(error(reader.position(), "invalid NURBS curve header"));
    }
    let stored_knot_count = checked_count(reader, 8)?;
    let expected_knot_count = order
        .checked_add(cv_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| error(reader.position(), "NURBS knot count overflow"))?;
    if stored_knot_count != expected_knot_count {
        return Err(error(reader.position(), "NURBS curve knot count mismatch"));
    }
    let knots = read_knots(reader, stored_knot_count)?;
    validate_stored_domain(&knots, order, cv_count, reader.position())?;
    let stored_cv_count = checked_count(reader, (dimension + rational) as usize * 8)?;
    if stored_cv_count != cv_count {
        return Err(error(reader.position(), "NURBS curve CV count mismatch"));
    }
    let (control_points, weights) =
        read_curve_poles(reader, stored_cv_count, rational != 0, dimension, scale)?;
    if minor >= 1 {
        reader.bool()?;
    }
    let periodic = periodic_knots(&knots, order, cv_count);
    let full_knots = reconstruct_knots(&knots, order, cv_count)?;
    Ok(NurbsCurve {
        degree: u32::try_from(order - 1).expect("validated order fits u32"),
        knots: full_knots,
        control_points,
        weights,
        periodic,
    })
}

fn read_curve_poles(
    reader: &mut BoundedReader<'_>,
    count: usize,
    rational: bool,
    dimension: i32,
    scale: f64,
) -> Result<(Vec<Point3>, Option<Vec<f64>>), GeometryError> {
    let mut points = Vec::with_capacity(count);
    let mut weights = rational.then(|| Vec::with_capacity(count));
    for _ in 0..count {
        let x = reader.f64()?;
        let y = reader.f64()?;
        let z = if dimension == 3 { reader.f64()? } else { 0.0 };
        let weight = rational.then(|| reader.f64()).transpose()?;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(error(reader.position(), "NURBS pole is not finite"));
        }
        let point = if let Some(weight) = weight {
            if !weight.is_finite() || weight == 0.0 {
                return Err(error(reader.position(), "NURBS weight is invalid"));
            }
            weights.as_mut().expect("rational weights").push(weight);
            [x / weight, y / weight, z / weight]
        } else {
            [x, y, z]
        };
        points.push(Point3::new(
            crate::wire::scaled_coordinate(point[0], scale)
                .ok_or_else(|| error(reader.position(), "scaled NURBS pole is invalid"))?,
            crate::wire::scaled_coordinate(point[1], scale)
                .ok_or_else(|| error(reader.position(), "scaled NURBS pole is invalid"))?,
            crate::wire::scaled_coordinate(point[2], scale)
                .ok_or_else(|| error(reader.position(), "scaled NURBS pole is invalid"))?,
        ));
    }
    Ok((points, weights))
}

pub(crate) fn read_nurbs_surface(
    reader: &mut BoundedReader<'_>,
    scale: f64,
) -> Result<NurbsSurface, GeometryError> {
    let version_offset = reader.position();
    let version = reader.u8()?;
    if version >> 4 != 1 || version & 0x0f != 0 {
        return Err(unsupported(
            version_offset,
            "unsupported NURBS surface version",
        ));
    }
    let dimension = reader.i32()?;
    let rational = reader.i32()?;
    let u_order = checked_positive(reader.i32()?, reader.position(), "surface U order")?;
    let v_order = checked_positive(reader.i32()?, reader.position(), "surface V order")?;
    let u_count = checked_positive(reader.i32()?, reader.position(), "surface U CV count")?;
    let v_count = checked_positive(reader.i32()?, reader.position(), "surface V CV count")?;
    reader.i32()?;
    reader.i32()?;
    reader.skip(48)?;
    if dimension != 3 || !(rational == 0 || rational == 1) || u_count < u_order || v_count < v_order
    {
        return Err(error(reader.position(), "invalid NURBS surface header"));
    }
    let u_knot_count = checked_count(reader, 8)?;
    let expected_u = u_order
        .checked_add(u_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| error(reader.position(), "surface U knot count overflow"))?;
    if u_knot_count != expected_u {
        return Err(error(reader.position(), "surface U knot count mismatch"));
    }
    let u_knots = read_knots(reader, u_knot_count)?;
    validate_stored_domain(&u_knots, u_order, u_count, reader.position())?;
    let v_knot_count = checked_count(reader, 8)?;
    let expected_v = v_order
        .checked_add(v_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| error(reader.position(), "surface V knot count overflow"))?;
    if v_knot_count != expected_v {
        return Err(error(reader.position(), "surface V knot count mismatch"));
    }
    let v_knots = read_knots(reader, v_knot_count)?;
    validate_stored_domain(&v_knots, v_order, v_count, reader.position())?;
    let u_periodic = periodic_knots(&u_knots, u_order, u_count);
    let v_periodic = periodic_knots(&v_knots, v_order, v_count);
    let stored_cv_count = checked_count(reader, (dimension + rational) as usize * 8)?;
    let expected_cv_count = u_count
        .checked_mul(v_count)
        .ok_or_else(|| error(reader.position(), "surface CV count overflow"))?;
    if stored_cv_count != expected_cv_count {
        return Err(error(reader.position(), "NURBS surface CV count mismatch"));
    }
    let (control_points, weights) = read_poles(reader, stored_cv_count, rational != 0, scale)?;
    let u_knots = reconstruct_knots(&u_knots, u_order, u_count)?;
    let v_knots = reconstruct_knots(&v_knots, v_order, v_count)?;
    Ok(NurbsSurface {
        u_degree: u32::try_from(u_order - 1).expect("validated order fits u32"),
        v_degree: u32::try_from(v_order - 1).expect("validated order fits u32"),
        u_knots: u_knots.clone(),
        v_knots: v_knots.clone(),
        u_count: u32::try_from(u_count).expect("validated count fits u32"),
        v_count: u32::try_from(v_count).expect("validated count fits u32"),
        control_points,
        weights,
        u_periodic,
        v_periodic,
    })
}

fn read_plane_surface(
    reader: &mut BoundedReader<'_>,
    scale: f64,
) -> Result<SurfaceGeometry, GeometryError> {
    let version_offset = reader.position();
    let version = reader.u8()?;
    if version >> 4 != 1 || version & 0x0f > 1 {
        return Err(unsupported(
            version_offset,
            "unsupported plane-surface version",
        ));
    }
    let native_plane = plane(reader)?;
    validate_plane(native_plane, reader.position())?;
    let domain = finite_increasing(interval(reader)?.0, reader.position(), "plane U domain")?;
    let v_domain = finite_increasing(interval(reader)?.0, reader.position(), "plane V domain")?;
    let (u_extents, v_extents) = if version & 0x0f == 1 {
        (
            finite_increasing(interval(reader)?.0, reader.position(), "plane U extents")?,
            finite_increasing(interval(reader)?.0, reader.position(), "plane V extents")?,
        )
    } else {
        (domain, v_domain)
    };
    let _ = (u_extents, v_extents);
    let plane = SurfaceGeometry::Plane {
        origin: scale_native_point(native_plane.origin, scale)
            .ok_or_else(|| error(reader.position(), "scaled plane origin is invalid"))?,
        normal: vector(native_plane.zaxis),
        u_axis: vector(native_plane.xaxis),
    };
    Ok(plane)
}

fn read_knots(reader: &mut BoundedReader<'_>, count: usize) -> Result<Vec<f64>, GeometryError> {
    let mut knots = Vec::with_capacity(count);
    for _ in 0..count {
        let value = reader.f64()?;
        if !value.is_finite() || knots.last().is_some_and(|last| value < *last) {
            return Err(error(reader.position(), "NURBS knots are invalid"));
        }
        knots.push(value);
    }
    Ok(knots)
}

fn read_poles(
    reader: &mut BoundedReader<'_>,
    count: usize,
    rational: bool,
    scale: f64,
) -> Result<(Vec<Point3>, Option<Vec<f64>>), GeometryError> {
    let mut points = Vec::with_capacity(count);
    let mut weights = rational.then(|| Vec::with_capacity(count));
    for _ in 0..count {
        let x = reader.f64()?;
        let y = reader.f64()?;
        let z = reader.f64()?;
        let weight = if rational { Some(reader.f64()?) } else { None };
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(error(reader.position(), "NURBS pole is not finite"));
        }
        let point = if let Some(weight) = weight {
            if !weight.is_finite() || weight == 0.0 {
                return Err(error(reader.position(), "NURBS weight is invalid"));
            }
            weights.as_mut().expect("rational weights").push(weight);
            [x / weight, y / weight, z / weight]
        } else {
            [x, y, z]
        };
        let scaled = [point[0] * scale, point[1] * scale, point[2] * scale];
        if !scaled.iter().all(|value| value.is_finite()) {
            return Err(error(reader.position(), "scaled NURBS pole is not finite"));
        }
        points.push(Point3::new(scaled[0], scaled[1], scaled[2]));
    }
    Ok((points, weights))
}

/// Reconstruct the omitted endpoints. The indexes below are zero-based.
pub(crate) fn reconstruct_knots(
    knots: &[f64],
    order: usize,
    cv_count: usize,
) -> Result<Vec<f64>, GeometryError> {
    let m = order
        .checked_add(cv_count)
        .and_then(|value| value.checked_sub(2))
        .ok_or_else(|| error(0, "NURBS knot arithmetic overflow"))?;
    if knots.len() != m || order < 2 || cv_count < order {
        return Err(error(0, "NURBS knot reconstruction input is invalid"));
    }
    let mut start = knots[0];
    if order > 2 && cv_count >= 2 * order - 2 && cv_count >= 6 && knots[0] < knots[order - 2] {
        start = knots[0] - (knots[cv_count - order + 1] - knots[cv_count - order]);
    }
    let mut end = knots[m - 1];
    if order > 2 && cv_count >= 2 * order - 2 && cv_count >= 6 && knots[cv_count - 1] < knots[m - 1]
    {
        end = knots[m - 1] + (knots[order + 1] - knots[order]);
    }
    if !start.is_finite() || !end.is_finite() || start > knots[0] || end < knots[m - 1] {
        return Err(error(0, "NURBS reconstructed knots are invalid"));
    }
    let mut result = Vec::with_capacity(order + cv_count);
    result.push(start);
    result.extend_from_slice(knots);
    result.push(end);
    Ok(result)
}

pub(crate) fn periodic_knots(knots: &[f64], order: usize, cv_count: usize) -> bool {
    // This is ON_IsKnotVectorPeriodic over the stored, zero-based knot array.
    if order < 3 || cv_count < order || (order <= 4 && cv_count < order + 2) {
        return false;
    }
    if order > 4 && cv_count < 2 * order - 2 {
        return false;
    }
    let mut tolerance = (knots[order - 1] - knots[order - 3]).abs() * f64::EPSILON.sqrt();
    tolerance = tolerance.max((knots[cv_count - 1] - knots[order - 2]).abs() * f64::EPSILON.sqrt());
    let mut paired = 2 * (order - 2);
    let mut index = 0;
    let mut other = cv_count - order + 1;
    while paired > 0 {
        if ((knots[index + 1] - knots[index]) + (knots[other] - knots[other + 1])).abs() > tolerance
        {
            return false;
        }
        index += 1;
        other += 1;
        paired -= 1;
    }
    true
}

fn validate_stored_domain(
    knots: &[f64],
    order: usize,
    cv_count: usize,
    offset: usize,
) -> Result<(), GeometryError> {
    if knots[order - 2] < knots[cv_count - 1] {
        Ok(())
    } else {
        Err(error(offset, "NURBS native domain is not increasing"))
    }
}

fn checked_positive(value: i32, offset: usize, label: &str) -> Result<usize, GeometryError> {
    if value < 2 && label.ends_with("order") || value <= 0 {
        return Err(error(offset, label));
    }
    usize::try_from(value).map_err(|_| error(offset, label))
}

fn checked_count(reader: &mut BoundedReader<'_>, width: usize) -> Result<usize, GeometryError> {
    let raw = reader.i32()?;
    let bytes = checked_count_bytes(
        raw,
        width,
        reader.remaining(),
        MAX_CURVE_ITEMS,
        reader.position() - 4,
    )?;
    Ok(bytes / width)
}

fn finite_increasing(
    value: [f64; 2],
    offset: usize,
    label: &str,
) -> Result<[f64; 2], GeometryError> {
    if value[0].is_finite() && value[1].is_finite() && value[0] < value[1] {
        Ok(value)
    } else {
        Err(error(offset, label))
    }
}

fn validate_plane(value: Plane, offset: usize) -> Result<(), GeometryError> {
    let x = vector(value.xaxis);
    let y = vector(value.yaxis);
    let z = vector(value.zaxis);
    if ![value.origin.0[0], value.origin.0[1], value.origin.0[2]]
        .into_iter()
        .chain(value.equation)
        .all(f64::is_finite)
        || (x.norm() - 1.0).abs() > 1.0e-10
        || (y.norm() - 1.0).abs() > 1.0e-10
        || (z.norm() - 1.0).abs() > 1.0e-10
        || dot(x, y).abs() > 1.0e-10
        || dot(x, z).abs() > 1.0e-10
        || dot(y, z).abs() > 1.0e-10
        || !close(cross(x, y), z)
    {
        return Err(error(
            offset,
            "plane frame is not orthonormal and right-handed",
        ));
    }
    Ok(())
}

fn scale_native_point(value: NativePoint3, scale: f64) -> Option<Point3> {
    Some(Point3::new(
        crate::wire::scaled_coordinate(value.0[0], scale)?,
        crate::wire::scaled_coordinate(value.0[1], scale)?,
        crate::wire::scaled_coordinate(value.0[2], scale)?,
    ))
}

fn vector(value: crate::settings::Vector3) -> Vector3 {
    Vector3::new(value.0[0], value.0[1], value.0[2])
}

fn dot(a: Vector3, b: Vector3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn close(a: Vector3, b: Vector3) -> bool {
    (a.x - b.x).abs() <= 1.0e-10 && (a.y - b.y).abs() <= 1.0e-10 && (a.z - b.z).abs() <= 1.0e-10
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        decode, periodic_knots, read_nurbs_curve, read_nurbs_curve_2d, read_nurbs_surface,
        read_plane_surface, reconstruct_knots, revolution_nurbs, sum_nurbs, DecodedSurface,
        CLIPPING_PLANE_SURFACE,
    };
    use crate::chunks::{ArchiveVersion, BoundedReader};
    use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn push_i32(bytes: &mut Vec<u8>, value: i32) {
        bytes.extend(value.to_le_bytes());
    }

    fn push_f64(bytes: &mut Vec<u8>, value: f64) {
        bytes.extend(value.to_le_bytes());
    }

    fn curve_payload(version: u8, rational: bool, knots: &[f64]) -> Vec<u8> {
        let mut bytes = vec![version];
        push_i32(&mut bytes, 3);
        push_i32(&mut bytes, i32::from(rational));
        push_i32(&mut bytes, 3);
        push_i32(&mut bytes, 6);
        push_i32(&mut bytes, 0);
        push_i32(&mut bytes, 0);
        bytes.extend([0; 48]);
        push_i32(&mut bytes, i32::try_from(knots.len()).expect("test count"));
        for knot in knots {
            push_f64(&mut bytes, *knot);
        }
        push_i32(&mut bytes, 6);
        for index in 0..6 {
            push_f64(&mut bytes, index as f64);
            push_f64(&mut bytes, 0.0);
            push_f64(&mut bytes, 0.0);
            if rational {
                push_f64(&mut bytes, if index == 0 { 2.0 } else { 1.0 });
            }
        }
        if version & 0x0f >= 1 {
            bytes.push(0);
        }
        bytes
    }

    fn curve_2d_payload(rational: bool) -> Vec<u8> {
        let mut bytes = vec![0x10];
        push_i32(&mut bytes, 2);
        push_i32(&mut bytes, i32::from(rational));
        push_i32(&mut bytes, 3);
        push_i32(&mut bytes, 6);
        push_i32(&mut bytes, 0);
        push_i32(&mut bytes, 0);
        bytes.extend([0; 48]);
        let knots = [0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0];
        push_i32(&mut bytes, knots.len() as i32);
        for knot in knots {
            push_f64(&mut bytes, knot);
        }
        push_i32(&mut bytes, 6);
        for index in 0..6 {
            push_f64(&mut bytes, index as f64);
            push_f64(&mut bytes, 2.0 * index as f64);
            if rational {
                push_f64(&mut bytes, if index == 0 { 2.0 } else { 1.0 });
            }
        }
        bytes
    }

    fn surface_payload(
        u_order: i32,
        v_order: i32,
        u_count: i32,
        v_count: i32,
        rational: bool,
        u_knots: &[f64],
        v_knots: &[f64],
    ) -> Vec<u8> {
        let mut bytes = vec![0x10];
        push_i32(&mut bytes, 3);
        push_i32(&mut bytes, i32::from(rational));
        push_i32(&mut bytes, u_order);
        push_i32(&mut bytes, v_order);
        push_i32(&mut bytes, u_count);
        push_i32(&mut bytes, v_count);
        push_i32(&mut bytes, 0);
        push_i32(&mut bytes, 0);
        bytes.extend([0; 48]);
        push_i32(
            &mut bytes,
            i32::try_from(u_knots.len()).expect("test count"),
        );
        for knot in u_knots {
            push_f64(&mut bytes, *knot);
        }
        push_i32(
            &mut bytes,
            i32::try_from(v_knots.len()).expect("test count"),
        );
        for knot in v_knots {
            push_f64(&mut bytes, *knot);
        }
        push_i32(&mut bytes, u_count * v_count);
        for i in 0..u_count {
            for j in 0..v_count {
                push_f64(&mut bytes, f64::from(i));
                push_f64(&mut bytes, f64::from(j));
                push_f64(&mut bytes, 0.0);
                if rational {
                    push_f64(&mut bytes, f64::from(i + j + 1));
                }
            }
        }
        bytes
    }

    fn plane_payload(version: u8, bad_frame: bool, bad_range: bool) -> Vec<u8> {
        let mut bytes = vec![version];
        push_f64(&mut bytes, 1.0);
        push_f64(&mut bytes, 2.0);
        push_f64(&mut bytes, 3.0);
        for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
            for value in axis {
                push_f64(&mut bytes, value);
            }
        }
        for value in [0.0, 0.0, 1.0, -3.0] {
            push_f64(&mut bytes, value);
        }
        if bad_frame {
            bytes[1 + 24..1 + 32].copy_from_slice(&2.0_f64.to_le_bytes());
        }
        for range in [[0.0, 1.0], [2.0, 3.0]] {
            push_f64(&mut bytes, range[0]);
            push_f64(&mut bytes, if bad_range { range[0] } else { range[1] });
        }
        if version & 0x0f == 1 {
            for range in [[4.0, 5.0], [6.0, 7.0]] {
                push_f64(&mut bytes, range[0]);
                push_f64(&mut bytes, range[1]);
            }
        }
        bytes
    }

    fn test_curve(points: Vec<Point3>, weights: Option<Vec<f64>>, domain: [f64; 2]) -> NurbsCurve {
        NurbsCurve {
            degree: 1,
            knots: vec![domain[0], domain[0], domain[1], domain[1]],
            control_points: points,
            weights,
            periodic: false,
        }
    }

    fn revolution_prefix(version: u8) -> Vec<u8> {
        let mut bytes = vec![version];
        for value in [1.0, 2.0, 3.0, 1.0, 2.0, 5.0, 0.25, 1.25] {
            push_f64(&mut bytes, value);
        }
        if version >> 4 >= 2 {
            for value in [4.0, 9.0] {
                push_f64(&mut bytes, value);
            }
        }
        for value in [-10.0, -10.0, -10.0, 10.0, 10.0, 10.0] {
            push_f64(&mut bytes, value);
        }
        push_i32(&mut bytes, 0);
        bytes.push(0);
        bytes
    }

    fn long_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = typecode.to_le_bytes().to_vec();
        bytes.extend((body.len() as i64).to_le_bytes());
        bytes.extend(body);
        bytes
    }

    fn crc_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut payload = body.to_vec();
        payload.extend(crc32fast::hash(body).to_le_bytes());
        long_chunk(typecode, &payload)
    }

    fn anonymous(minor: i32, body: &[u8]) -> Vec<u8> {
        let mut payload = 1_i32.to_le_bytes().to_vec();
        payload.extend(minor.to_le_bytes());
        payload.extend(body);
        crc_chunk(0x4000_8000, &payload)
    }

    fn clipping_plane_payload(item_order_valid: bool) -> Vec<u8> {
        let carrier = crc_chunk(0x4000_8000, &plane_payload(0x11, false, false));
        let mut clipping = [0x11; 16].to_vec();
        clipping.extend([0x22; 16]);
        clipping.extend(&plane_payload(0x11, false, false)[1..129]);
        clipping.push(1);
        let mut viewports = 1_i32.to_le_bytes().to_vec();
        viewports.extend([0x33; 16]);
        clipping.extend(anonymous(0, &viewports));
        clipping.extend(2.5_f64.to_le_bytes());
        clipping.push(1);
        clipping.push(if item_order_valid { 10 } else { 13 });
        if item_order_valid {
            clipping.extend(1_i32.to_le_bytes());
            clipping.extend([0x44; 16]);
            clipping.push(11);
            clipping.extend(1_i32.to_le_bytes());
            clipping.extend(7_i32.to_le_bytes());
            clipping.push(12);
            clipping.push(0);
            clipping.push(13);
            clipping.push(1);
        } else {
            clipping.push(1);
            clipping.push(10);
            clipping.extend(0_i32.to_le_bytes());
        }
        clipping.push(0);
        let clipping = anonymous(5, &clipping);
        let mut outer = 1_i32.to_le_bytes().to_vec();
        outer.extend(0_i32.to_le_bytes());
        outer.extend(carrier);
        outer.extend(clipping);
        crc_chunk(0x4000_8000, &outer)
    }

    #[test]
    fn clipping_plane_decodes_plane_carrier_and_all_v8_suffix_items() {
        let bytes = clipping_plane_payload(true);
        let decoded = decode(
            &bytes,
            CLIPPING_PLANE_SURFACE,
            0..bytes.len(),
            25.4,
            ArchiveVersion::V8,
            0,
        )
        .expect("clipping plane");
        let DecodedSurface::Typed {
            geometry: SurfaceGeometry::Plane { origin, .. },
            derived,
        } = decoded
        else {
            panic!("typed plane carrier");
        };
        assert_eq!(origin, Point3::new(25.4, 50.8, 76.199_999_999_999_99));
        assert!(derived);

        let invalid = clipping_plane_payload(false);
        assert!(decode(
            &invalid,
            CLIPPING_PLANE_SURFACE,
            0..invalid.len(),
            1.0,
            ArchiveVersion::V8,
            0,
        )
        .is_err());
    }

    fn line_wrapper(scale_source: f64) -> Vec<u8> {
        let mut line = vec![0x10];
        for value in [
            2.0 * scale_source,
            0.0,
            0.0,
            3.0 * scale_source,
            0.0,
            0.0,
            6.0,
            8.0,
        ] {
            push_f64(&mut line, value);
        }
        push_i32(&mut line, 3);
        let wire_uuid = [
            0xdb, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ];
        let mut uuid_body = wire_uuid.to_vec();
        uuid_body.extend(crc32fast::hash(&wire_uuid).to_le_bytes());
        let mut class_body = long_chunk(0x0002_fffb, &uuid_body);
        class_body.extend(crc_chunk(0x0002_fffc, &line));
        class_body.extend(0x8002_7fff_u32.to_le_bytes());
        class_body.extend(0_i64.to_le_bytes());
        long_chunk(0x0002_7ffa, &class_body)
    }

    pub(crate) fn valid_revolution_payload(version: u8) -> Vec<u8> {
        let mut bytes = revolution_prefix(version);
        *bytes.last_mut().unwrap() = 1;
        bytes.extend(line_wrapper(1.0));
        bytes
    }

    fn valid_sum_payload() -> Vec<u8> {
        let mut bytes = vec![0x10];
        for value in [1.0, 2.0, 3.0, -10.0, -10.0, -10.0, 10.0, 10.0, 10.0] {
            push_f64(&mut bytes, value);
        }
        bytes.extend(line_wrapper(1.0));
        bytes.extend(line_wrapper(1.0));
        bytes
    }

    #[test]
    fn reconstructs_spec_examples_and_one_sided_vectors() {
        assert_eq!(
            reconstruct_knots(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0], 3, 6).unwrap(),
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0]
        );
        assert_eq!(
            reconstruct_knots(&[0.0, 1.0, 2.0, 3.0, 5.0, 6.0, 7.0], 3, 6).unwrap(),
            vec![-2.0, 0.0, 1.0, 2.0, 3.0, 5.0, 6.0, 7.0, 9.0]
        );
        assert_eq!(
            reconstruct_knots(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0], 3, 6).unwrap()[0],
            0.0
        );
        assert_eq!(
            reconstruct_knots(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 5.0], 3, 6).unwrap()[8],
            5.0
        );
    }

    #[test]
    fn curve_versions_cross_archive_bands_and_consume_tag_gate() {
        for (archive, version) in [(ArchiveVersion::V5, 0x10), (ArchiveVersion::V8, 0x11)] {
            let bytes = curve_payload(version, false, &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0]);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
            let curve = read_nurbs_curve(&mut reader, 1.0).unwrap();
            assert_eq!(curve.control_points.len(), 6);
            assert_eq!(reader.remaining(), 0);
            assert!(matches!(archive, ArchiveVersion::V5 | ArchiveVersion::V8));
        }
    }

    #[test]
    fn curve_payload_validates_rational_weights_counts_and_domain() {
        let mut bytes = curve_payload(0x10, true, &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let curve = read_nurbs_curve(&mut reader, 2.0).unwrap();
        assert_eq!(curve.control_points[0].x, 0.0);
        assert_eq!(curve.weights.as_ref().unwrap()[0], 2.0);
        let weight_offset = 1 + 28 + 48 + 4 + 7 * 8 + 4 + 24;
        bytes[weight_offset..weight_offset + 8].copy_from_slice(&0.0_f64.to_le_bytes());
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        assert!(read_nurbs_curve(&mut reader, 1.0).is_err());
    }

    #[test]
    fn c2_nurbs_reads_two_dimensions_without_scaling_uv() {
        let bytes = curve_2d_payload(true);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let curve = read_nurbs_curve_2d(&mut reader).unwrap();
        assert_eq!(reader.remaining(), 0);
        assert_eq!(curve.control_points[1].x, 1.0);
        assert_eq!(curve.control_points[1].y, 2.0);
        assert_eq!(curve.weights.as_ref().unwrap()[0], 2.0);
        assert_eq!(
            curve.knots,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0]
        );
    }

    #[test]
    fn c2_nurbs_preserves_periodic_parameterization() {
        let mut bytes = curve_2d_payload(false);
        let knots: [f64; 7] = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let start = 1 + 24 + 48 + 4;
        for (index, knot) in knots.into_iter().enumerate() {
            bytes[start + index * 8..start + index * 8 + 8].copy_from_slice(&knot.to_le_bytes());
        }
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        assert!(read_nurbs_curve_2d(&mut reader).unwrap().periodic);
    }

    #[test]
    fn periodic_rule_matches_native_tolerance_and_rejects_clamping() {
        assert!(!periodic_knots(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0], 3, 6));
        assert!(periodic_knots(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 3, 6));
        assert!(!periodic_knots(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 7.0], 3, 6));
        assert!(!periodic_knots(&[0.0, 1.0, 2.0, 3.0], 2, 4));
        let mut near = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        near[6] += 1.0e-8;
        assert!(periodic_knots(&near, 3, 6));
    }

    #[test]
    fn surface_periodicity_is_derived_independently_in_u_and_v() {
        let periodic = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let nonperiodic = [0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0];
        let bytes = surface_payload(3, 3, 6, 6, false, &periodic, &nonperiodic);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let surface = read_nurbs_surface(&mut reader, 1.0).unwrap();
        assert!(surface.u_periodic);
        assert!(!surface.v_periodic);
    }

    #[test]
    fn surface_bytes_preserve_asymmetric_u_major_rational_poles() {
        let bytes = surface_payload(2, 2, 2, 3, true, &[0.0, 1.0], &[0.0, 1.0, 2.0]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let surface = read_nurbs_surface(&mut reader, 1.0).unwrap();
        assert_eq!(surface.control_points[1].y, 1.0 / 2.0);
        assert_eq!(surface.control_points[3].x, 1.0 / 2.0);
        assert_eq!(surface.weights.as_ref().unwrap()[5], 4.0);
    }

    #[test]
    fn surface_bytes_reconstruct_independent_knots_and_reject_count_mismatch() {
        let bytes = surface_payload(2, 2, 3, 2, false, &[0.0, 1.0, 2.0], &[0.0, 1.0]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let surface = read_nurbs_surface(&mut reader, 1.0).unwrap();
        assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
        assert_eq!(surface.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
        let mut bad = bytes;
        let count_offset = bad.len() - 6 * 24 - 4;
        bad[count_offset..count_offset + 4].copy_from_slice(&99_i32.to_le_bytes());
        let mut reader = BoundedReader::new(&bad, 0, bad.len()).unwrap();
        assert!(read_nurbs_surface(&mut reader, 1.0).is_err());
    }

    #[test]
    fn plane_versions_consume_defaults_and_explicit_extents() {
        for version in [0x10, 0x11] {
            let bytes = plane_payload(version, false, false);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
            let plane = read_plane_surface(&mut reader, 1.0).unwrap();
            assert_eq!(reader.remaining(), 0);
            assert!(matches!(
                plane,
                cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
            ));
        }
        for (bad_frame, bad_range) in [(true, false), (false, true)] {
            let bytes = plane_payload(0x11, bad_frame, bad_range);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
            assert!(read_plane_surface(&mut reader, 1.0).is_err());
        }
    }

    #[test]
    fn sum_surface_preserves_asymmetric_domains_and_u_major_order() {
        let first = test_curve(
            vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
            None,
            [2.0, 5.0],
        );
        let second = NurbsCurve {
            degree: 2,
            knots: vec![7.0, 7.0, 7.0, 9.0, 9.0, 9.0],
            control_points: vec![
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(20.0, 0.0, 0.0),
                Point3::new(30.0, 0.0, 0.0),
            ],
            weights: None,
            periodic: false,
        };
        let surface = sum_nurbs(&first, &second, Vector3::new(0.5, 1.5, 2.5), 0).unwrap();
        assert_eq!((surface.u_count, surface.v_count), (2, 3));
        assert_eq!(surface.u_knots, first.knots);
        assert_eq!(surface.v_knots, second.knots);
        assert_eq!(surface.control_points[0], Point3::new(11.5, 3.5, 5.5));
        assert_eq!(surface.control_points[3], Point3::new(14.5, 6.5, 8.5));
        assert!(surface.weights.is_none());
    }

    #[test]
    fn sum_surface_multiplies_each_rational_weight_pair() {
        for (first_weights, second_weights, expected) in [
            (Some(vec![2.0, 3.0]), None, vec![2.0, 2.0, 3.0, 3.0]),
            (None, Some(vec![5.0, 7.0]), vec![5.0, 7.0, 5.0, 7.0]),
            (
                Some(vec![2.0, 3.0]),
                Some(vec![5.0, 7.0]),
                vec![10.0, 14.0, 15.0, 21.0],
            ),
        ] {
            let first = test_curve(
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                first_weights,
                [0.0, 1.0],
            );
            let second = test_curve(
                vec![Point3::new(0.0, 3.0, 0.0), Point3::new(0.0, 4.0, 0.0)],
                second_weights,
                [4.0, 8.0],
            );
            let surface = sum_nurbs(&first, &second, Vector3::new(9.0, 8.0, 7.0), 0).unwrap();
            assert_eq!(surface.weights.unwrap(), expected);
            assert_eq!(surface.control_points[3], Point3::new(11.0, 12.0, 7.0));
        }
    }

    #[test]
    fn extrusion_tensor_preserves_rational_profile_knots_weights_and_transpose() {
        let start = NurbsCurve {
            degree: 2,
            knots: vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(3.0, 0.0, 0.0),
            ],
            weights: Some(vec![1.0, 0.5, 1.0]),
            periodic: false,
        };
        let mut end = start.clone();
        for point in &mut end.control_points {
            point.z = 7.0;
        }
        let plain = super::extrusion_nurbs(&start, &end, [10.0, 20.0], false, 0).unwrap();
        assert_eq!((plain.u_degree, plain.v_degree), (2, 1));
        assert_eq!(plain.u_knots, start.knots);
        assert_eq!(plain.v_knots, vec![10.0, 10.0, 20.0, 20.0]);
        assert_eq!(plain.weights, Some(vec![1.0, 1.0, 0.5, 0.5, 1.0, 1.0]));
        assert_eq!(plain.control_points[3], end.control_points[1]);
        let transposed = super::extrusion_nurbs(&start, &end, [10.0, 20.0], true, 0).unwrap();
        assert_eq!((transposed.u_degree, transposed.v_degree), (1, 2));
        assert_eq!((transposed.u_count, transposed.v_count), (2, 3));
        assert_eq!(transposed.u_knots, vec![10.0, 10.0, 20.0, 20.0]);
        assert_eq!(transposed.control_points[1], start.control_points[1]);
        assert_eq!(transposed.control_points[3], end.control_points[0]);
    }

    #[test]
    fn revolution_preserves_partial_angle_parameter_domain_and_product_weights() {
        let profile = test_curve(
            vec![Point3::new(3.0, 0.0, 1.0), Point3::new(4.0, 0.0, 2.0)],
            Some(vec![2.0, 3.0]),
            [11.0, 13.0],
        );
        let surface = revolution_nurbs(
            &profile,
            Point3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            [0.0, std::f64::consts::FRAC_PI_2],
            [20.0, 30.0],
            false,
            0,
        )
        .unwrap();
        assert_eq!((surface.u_count, surface.v_count), (3, 2));
        assert_eq!(surface.u_knots, vec![20.0, 20.0, 20.0, 30.0, 30.0, 30.0]);
        assert_eq!(surface.v_knots, profile.knots);
        assert_eq!(surface.weights.as_ref().unwrap()[0], 2.0);
        assert!((surface.weights.as_ref().unwrap()[2] - 2.0 / 2.0_f64.sqrt()).abs() < 1.0e-12);
        assert_eq!(surface.control_points[0], profile.control_points[0]);
        assert!((surface.control_points[4].x - 1.0).abs() < 1.0e-12);
        assert!((surface.control_points[4].y - 2.0).abs() < 1.0e-12);
    }

    #[test]
    fn revolution_transpose_swaps_shape_and_reindexes_u_major_poles() {
        let profile = test_curve(
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
            None,
            [4.0, 6.0],
        );
        let plain = revolution_nurbs(
            &profile,
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            [0.0, std::f64::consts::FRAC_PI_2],
            [8.0, 9.0],
            false,
            0,
        )
        .unwrap();
        let transposed = revolution_nurbs(
            &profile,
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            [0.0, std::f64::consts::FRAC_PI_2],
            [8.0, 9.0],
            true,
            0,
        )
        .unwrap();
        assert_eq!((transposed.u_count, transposed.v_count), (2, 3));
        assert_eq!((transposed.u_degree, transposed.v_degree), (1, 2));
        assert_eq!(transposed.u_knots, profile.knots);
        assert_eq!(transposed.control_points[1], plain.control_points[2]);
        assert_eq!(transposed.control_points[3], plain.control_points[1]);
    }

    #[test]
    fn revolution_rejects_versions_axis_intervals_transpose_and_presence() {
        let bad_version = [0x30];
        let mut reader = BoundedReader::new(&bad_version, 0, bad_version.len()).unwrap();
        assert!(
            super::read_revolution(&bad_version, &mut reader, 1.0, ArchiveVersion::V5, 0).is_err()
        );

        let valid = revolution_prefix(0x20);
        let axis_end_offset = 1 + 3 * 8;
        let angle_end_offset = 1 + 6 * 8 + 8;
        let parameter_end_offset = 1 + 8 * 8 + 8;
        let transpose_offset = valid.len() - 5;
        let mut cases = Vec::new();
        let mut zero_axis = valid.clone();
        let start = zero_axis[1..1 + 24].to_vec();
        zero_axis[axis_end_offset..axis_end_offset + 24].copy_from_slice(&start);
        cases.push(zero_axis);
        let mut bad_angle = valid.clone();
        bad_angle[angle_end_offset..angle_end_offset + 8].copy_from_slice(&0.25_f64.to_le_bytes());
        cases.push(bad_angle);
        let mut bad_parameter = valid.clone();
        bad_parameter[parameter_end_offset..parameter_end_offset + 8]
            .copy_from_slice(&4.0_f64.to_le_bytes());
        cases.push(bad_parameter);
        let mut bad_transpose = valid.clone();
        bad_transpose[transpose_offset..transpose_offset + 4].copy_from_slice(&2_i32.to_le_bytes());
        cases.push(bad_transpose);
        for bytes in cases {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
            assert!(
                super::read_revolution(&bytes, &mut reader, 1.0, ArchiveVersion::V5, 0).is_err()
            );
        }
        let mut reader = BoundedReader::new(&valid, 0, valid.len()).unwrap();
        assert!(super::read_revolution(&valid, &mut reader, 1.0, ArchiveVersion::V5, 0).is_err());
    }

    #[test]
    fn sum_surface_rejects_future_packed_version_before_children() {
        let bytes = [0x11];
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        assert!(matches!(
            super::read_sum(&bytes, &mut reader, 1.0, ArchiveVersion::V5, 0),
            Err(crate::curves::GeometryError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn revolution_major_versions_decode_child_and_scale_coordinates_once() {
        for version in [0x10, 0x20] {
            let bytes = valid_revolution_payload(version);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
            let decoded =
                super::read_revolution(&bytes, &mut reader, 25.4, ArchiveVersion::V5, 0).unwrap();
            let super::DecodedSurface::Procedural {
                geometry,
                definition,
                children,
            } = decoded
            else {
                panic!("expected procedural revolution");
            };
            assert_eq!(reader.remaining(), 0);
            assert_eq!(children.len(), 1);
            let super::DecodedProceduralSurface::Revolution {
                axis_origin,
                angular_interval,
                parameter_interval,
                ..
            } = definition
            else {
                panic!("expected revolution fields");
            };
            assert!((axis_origin.x - 25.4).abs() < 1.0e-12);
            assert!((axis_origin.y - 50.8).abs() < 1.0e-12);
            assert!((axis_origin.z - 76.2).abs() < 1.0e-12);
            assert_eq!(angular_interval, [0.25, 1.25]);
            assert_eq!(
                parameter_interval,
                if version == 0x10 {
                    [0.25, 1.25]
                } else {
                    [4.0, 9.0]
                }
            );
            let CurveGeometry::Nurbs(child) = &children[0].geometry else {
                panic!("expected NURBS child");
            };
            assert_eq!(child.control_points[0].x, 2.0 * 25.4);
            assert_eq!(geometry.u_knots[2], parameter_interval[0]);
        }
    }

    #[test]
    fn sum_surface_decodes_ordered_children_and_scales_once() {
        let bytes = valid_sum_payload();
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let decoded = super::read_sum(&bytes, &mut reader, 25.4, ArchiveVersion::V5, 0).unwrap();
        let super::DecodedSurface::Procedural {
            geometry,
            definition,
            children,
        } = decoded
        else {
            panic!("expected procedural sum");
        };
        assert_eq!(reader.remaining(), 0);
        assert_eq!(children.len(), 2);
        let super::DecodedProceduralSurface::Sum { basepoint } = definition else {
            panic!("expected sum fields");
        };
        assert!((basepoint.x - 25.4).abs() < 1.0e-12);
        assert!((basepoint.y - 50.8).abs() < 1.0e-12);
        assert!((basepoint.z - 76.2).abs() < 1.0e-12);
        assert!((geometry.control_points[0].x - 127.0).abs() < 1.0e-12);
        assert!((geometry.control_points[0].y - 50.8).abs() < 1.0e-12);
        assert!((geometry.control_points[0].z - 76.2).abs() < 1.0e-12);
    }
}
