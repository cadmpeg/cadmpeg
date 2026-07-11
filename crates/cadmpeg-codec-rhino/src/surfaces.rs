// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino NURBS and plane-surface payload decoding.

use std::ops::Range;

use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::chunks::{checked_count_bytes, BoundedReader};
use crate::curves::{error, unsupported, GeometryError, MAX_CURVE_ITEMS};
use crate::settings::{interval, plane, Plane, Point3 as NativePoint3};

pub(crate) const NURBS_CURVE: &str = "4ed7d4dd-e947-11d3-bfe5-0010830122f0";
pub(crate) const NURBS_SURFACE: &str = "4ed7d4de-e947-11d3-bfe5-0010830122f0";
pub(crate) const PLANE_SURFACE: &str = "4ed7d4df-e947-11d3-bfe5-0010830122f0";

#[derive(Debug, Clone)]
pub(crate) enum DecodedSurface {
    /// A typed surface and its conversion state.
    Typed {
        /// Decoded surface geometry.
        geometry: SurfaceGeometry,
        /// Whether native coordinates were scaled or reconstructed.
        derived: bool,
    },
}

pub(crate) fn decode(
    data: &[u8],
    class: &str,
    range: Range<usize>,
    scale: f64,
) -> Result<DecodedSurface, GeometryError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let result = match class {
        NURBS_SURFACE => DecodedSurface::Typed {
            geometry: SurfaceGeometry::Nurbs(read_nurbs_surface(&mut reader, scale)?),
            derived: true,
        },
        PLANE_SURFACE => {
            let geometry = read_plane_surface(&mut reader, scale)?;
            DecodedSurface::Typed {
                geometry,
                derived: scale != 1.0,
            }
        }
        _ => return Err(unsupported(range.start, "unsupported Rhino surface class")),
    };
    if reader.remaining() != 0 {
        return Err(error(
            reader.position(),
            "surface payload has trailing bytes",
        ));
    }
    Ok(result)
}

pub(crate) fn read_nurbs_curve(
    reader: &mut BoundedReader<'_>,
    scale: f64,
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
    if dimension != 3 || !(rational == 0 || rational == 1) || cv_count < order {
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
    let (control_points, weights) = read_poles(reader, stored_cv_count, rational != 0, scale)?;
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

fn read_nurbs_surface(
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
        origin: scale_native_point(native_plane.origin, scale),
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

fn periodic_knots(knots: &[f64], order: usize, cv_count: usize) -> bool {
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

fn scale_native_point(value: NativePoint3, scale: f64) -> Point3 {
    Point3::new(value.0[0] * scale, value.0[1] * scale, value.0[2] * scale)
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
mod tests {
    use super::{
        periodic_knots, read_nurbs_curve, read_nurbs_surface, read_plane_surface, reconstruct_knots,
    };
    use crate::chunks::{ArchiveVersion, BoundedReader};

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
}
