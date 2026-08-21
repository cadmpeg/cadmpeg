//! E5 storage-variant record decoders.
//!
//! Decodes E5 `05 08 01` vertex rosters, inline `0xc9` circle carriers,
//! class-`0xc8` planes, `0xff` edge-use records, and cylinder/cone/torus
//! analytic surface carriers.

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

use crate::wire::bytes::{f64_le, f64_point, f64_vector, read_f64_array, u32_le_24};
use crate::wire::records::scan_vertex_records;

/// A directly decoded E5 circle carrier.
#[derive(Debug, Clone)]
pub struct E5Circle {
    /// Offset of the `e5 0d 03` record in the source buffer.
    pub pos: usize,
    /// The complete circle carrier.
    pub geometry: CurveGeometry,
}

/// Partial class-`0xc8` plane carrier containing the fields stored directly.
#[derive(Debug, Clone)]
pub struct E5Plane {
    /// Offset of the framed record.
    pub pos: usize,
    /// Stream-assigned record identifier.
    pub record_id: u32,
    /// Stored plane origin.
    pub origin: [f64; 3],
    /// Natural U-coordinate bounds.
    #[cfg(test)]
    pub u_range: [f64; 2],
    /// Natural V-coordinate bounds.
    #[cfg(test)]
    pub v_range: [f64; 2],
}

/// A directly decoded E5 analytic surface carrier.
#[derive(Debug, Clone)]
pub struct E5Surface {
    /// Offset of the `e5 0d 03` record in the source buffer.
    pub pos: usize,
    /// Persistent E5 record id.
    pub record_id: u32,
    /// The complete analytic surface carrier.
    pub geometry: SurfaceGeometry,
    /// Component-wise scale from native E5 UV coordinates to neutral UV.
    pub uv_scale: [f64; 2],
}

#[derive(Clone, Copy)]
struct E5Record {
    pos: usize,
    end: usize,
    class: u8,
    size: usize,
}

const MARKER: &[u8; 3] = &crate::layout::token::E5_RECORD_FAMILY;
const E5_NURBS_SURFACE_TAIL_BYTES: usize = 148;

fn e5_records(data: &[u8]) -> Vec<E5Record> {
    debug_assert_eq!(MARKER, crate::container::E5_MARKER);
    crate::container::all_e5_record_spans(data)
        .into_iter()
        .filter_map(|range| {
            let pos = range.start;
            let size = View::u16_le_at(data, pos + 5).map(usize::from)?;
            Some(E5Record {
                pos,
                end: range.end,
                class: data[pos + 3],
                size,
            })
        })
        .collect()
}

/// Read the complete ordered E5 `05 08 01` coordinate roster matching the
/// referenced vertex population. The roster may be split into multiple runs;
/// marker-like bytes inside framed payloads are not vertex rows.
#[must_use]
pub fn e5_vertices(data: &[u8], vertex_count: usize) -> Vec<Point3> {
    if vertex_count == 0 {
        return Vec::new();
    }
    let records = e5_records(data);
    let mut runs = Vec::new();
    let mut region_start = 0usize;
    for record in records {
        runs.extend(vertex_runs(&data[region_start..record.pos]));
        region_start = record.end;
    }
    runs.extend(vertex_runs(&data[region_start..]));
    let Some(run_count) = runs
        .iter()
        .try_fold(0usize, |count, run| count.checked_add(run.len()))
    else {
        return Vec::new();
    };
    if run_count != vertex_count {
        return Vec::new();
    }
    runs.into_iter().flatten().collect()
}

fn vertex_runs(bytes: &[u8]) -> Vec<Vec<Point3>> {
    let mut runs = Vec::new();
    let mut position = 0usize;
    while position + 15 <= bytes.len() {
        if bytes[position..position + 3] != [0x05, 0x08, 0x01] {
            position += 1;
            continue;
        }
        let start = position;
        while position + 15 <= bytes.len() && bytes[position..position + 3] == [0x05, 0x08, 0x01] {
            position += 15;
        }
        let vertices = scan_vertex_records(&bytes[start..position]);
        if !vertices.is_empty() {
            runs.push(vertices);
        }
    }
    runs
}

/// Walk an E5 record stream and decode its inline `0xc9` circle carriers.
/// Record strides are derived from the little-endian size field at `+5`.
pub fn e5_circles(data: &[u8]) -> Vec<E5Circle> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        if record.class == 0xc9 && record.size >= 81 {
            let origin = f64_point(data, pos + 14);
            let frame_u = f64_vector(data, pos + 38);
            let frame_v = f64_vector(data, pos + 62);
            let radius = f64_le(data, pos + 86);
            if let (Some(origin), Some(frame_u), Some(frame_v), Some(radius)) =
                (origin, frame_u, frame_v, radius)
            {
                if radius.is_finite() && radius > 0.0 {
                    if let Some(axis) = frame_u.cross(frame_v).unit() {
                        out.push(E5Circle {
                            pos,
                            geometry: CurveGeometry::Circle {
                                center: origin,
                                axis,
                                ref_direction: frame_u.unit().unwrap_or_else(|| {
                                    cadmpeg_ir::geometry::derive_reference_direction(axis)
                                }),
                                radius,
                            },
                        });
                    }
                }
            }
        }
    }
    out
}

/// Decode the byte-explicit origin and natural bounds of E5 class-`0xc8` planes.
///
/// The record does not store a complete in-plane frame, so this function does not
/// synthesize plane axes or a [`SurfaceGeometry`].
#[must_use]
pub fn e5_planes(data: &[u8]) -> Vec<E5Plane> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        if record.class != 0xc8 || record.size < 90 || (record.size - 90) % 8 != 0 {
            continue;
        }
        let Some(origin) = read_f64_array::<3>(data, pos + 14) else {
            continue;
        };
        let scalar_count = (record.size - 58) / 8;
        let scalars_finite = (0..scalar_count)
            .all(|index| f64_le(data, pos + 39 + 8 * index).is_some_and(f64::is_finite));
        let Some(bounds) = read_f64_array::<4>(data, record.end - 32) else {
            continue;
        };
        if !scalars_finite || origin.iter().chain(&bounds).any(|value| !value.is_finite()) {
            continue;
        }
        out.push(E5Plane {
            pos,
            record_id: View::u32_le_at(data, pos + 9).unwrap_or(0),
            origin,
            #[cfg(test)]
            u_range: [bounds[0], bounds[1]],
            #[cfg(test)]
            v_range: [bounds[2], bounds[3]],
        });
    }
    out
}

/// A directly framed E5 edge-use record.  The endpoint ids are E5 vertex
/// records, not point-table indexes.
#[derive(Debug, Clone)]
pub struct E5Edge {
    /// Referenced start-vertex (class `0xfe`) record id.
    pub start_vertex_id: u32,
    /// Referenced end-vertex (class `0xfe`) record id.
    pub end_vertex_id: u32,
}

/// Decode E5 `0xff` five-reference edge records.
pub fn e5_edges(data: &[u8]) -> Vec<E5Edge> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        if record.class == 0xff && data.get(pos + 13) == Some(&0x85) {
            let payload = &data[pos + 13..record.end];
            if let Some((_, next)) = e5_ref(payload, 1) {
                if let Some((start_vertex_id, next)) = e5_ref(payload, next) {
                    if let Some((end_vertex_id, _)) = e5_ref(payload, next) {
                        out.push(E5Edge {
                            start_vertex_id,
                            end_vertex_id,
                        });
                    }
                }
            }
        }
    }
    out
}

/// Decode E5 cylinder (`0xc9`), cone (`0xca`), and torus (`0xcc`) surface
/// records. The E5 plane class does not serialize a standalone normal.
pub fn e5_surfaces(data: &[u8]) -> Vec<E5Surface> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        let decoded = match record.class {
            0xc9 => e5_cylinder(data, pos).and_then(|geometry| {
                let SurfaceGeometry::Cylinder { radius, .. } = geometry else {
                    unreachable!()
                };
                let parameter_scale = [1.0 / radius, 1.0];
                parameter_scale
                    .into_iter()
                    .all(f64::is_finite)
                    .then_some((geometry, parameter_scale))
            }),
            0xca => e5_cone(data, pos).and_then(|geometry| {
                let SurfaceGeometry::Cone { half_angle, .. } = geometry else {
                    unreachable!()
                };
                let u_scale = f64_le(data, pos + 158)?;
                let v_scale = f64_le(data, pos + 166)?;
                let parameter_scale = [1.0 / u_scale, half_angle.cos() / v_scale];
                (u_scale.is_finite()
                    && u_scale != 0.0
                    && v_scale.is_finite()
                    && v_scale != 0.0
                    && parameter_scale.into_iter().all(f64::is_finite))
                .then_some((geometry, parameter_scale))
            }),
            0xcc => e5_torus(data, pos).and_then(|geometry| {
                let SurfaceGeometry::Torus {
                    major_radius,
                    minor_radius,
                    ..
                } = geometry
                else {
                    unreachable!()
                };
                let parameter_scale = [1.0 / major_radius, 1.0 / minor_radius];
                parameter_scale
                    .into_iter()
                    .all(f64::is_finite)
                    .then_some((geometry, parameter_scale))
            }),
            0xe7 => e5_nurbs_surface(data, record).map(|geometry| (geometry, [1.0, 1.0])),
            _ => None,
        };
        if let Some((geometry, uv_scale)) = decoded {
            out.push(E5Surface {
                pos,
                record_id: View::u32_le_at(data, pos + 9).unwrap_or(0),
                geometry,
                uv_scale,
            });
        }
    }
    out
}

fn e5_nurbs_surface(data: &[u8], record: E5Record) -> Option<SurfaceGeometry> {
    let mut view = View::over_retained(data).child(record.pos + 13, record.end)?;
    if view.u8()? != 0x80 {
        return None;
    }
    let (u_degree, u_knots, u_multiplicities) = read_nurbs_axis(&mut view)?;
    let (v_degree, v_knots, v_multiplicities) = read_nurbs_axis(&mut view)?;
    let (u_knots, u_count) = expand_nurbs_axis(u_degree, &u_knots, &u_multiplicities, record.size)?;
    let (v_knots, v_count) = expand_nurbs_axis(v_degree, &v_knots, &v_multiplicities, record.size)?;
    let mode = view.u16_le()?;
    if !matches!(mode, 0 | 1) {
        return None;
    }
    let control_count = u_count.checked_mul(v_count)?;
    let control_points = view.read_counted(u64::try_from(control_count).ok()?, 24, |view| {
        Some(Point3::new(view.f64_le()?, view.f64_le()?, view.f64_le()?))
    })?;
    let weights = if mode == 1 {
        Some(view.read_counted(u64::try_from(control_count).ok()?, 8, View::f64_le)?)
    } else {
        None
    };
    if control_points
        .iter()
        .any(|point| ![point.x, point.y, point.z].into_iter().all(f64::is_finite))
        || weights.as_ref().is_some_and(|weights| {
            weights
                .iter()
                .copied()
                .any(|weight| !weight.is_finite() || weight == 0.0)
        })
        || view.remaining() != E5_NURBS_SURFACE_TAIL_BYTES
    {
        return None;
    }
    view.skip(E5_NURBS_SURFACE_TAIL_BYTES)?;
    view.is_empty()
        .then_some(SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            u_count: u32::try_from(u_count).ok()?,
            v_count: u32::try_from(v_count).ok()?,
            control_points,
            weights,
            u_periodic: false,
            v_periodic: false,
        }))
}

fn read_nurbs_axis(view: &mut View<'_>) -> Option<(u32, Vec<f64>, Vec<u32>)> {
    let degree = view.u32_le()?;
    let zero0 = view.u32_le()?;
    let zero1 = view.u32_le()?;
    let knot_count = usize::try_from(view.u32_le()?).ok()?;
    let zero2 = view.u32_le()?;
    if degree == 0 || [zero0, zero1, zero2] != [0; 3] || knot_count == 0 {
        return None;
    }
    let knot_count_u64 = u64::try_from(knot_count).ok()?;
    let knots = view.read_counted(knot_count_u64, 8, View::f64_le)?;
    let multiplicities = view.read_counted(knot_count_u64, 4, View::u32_le)?;
    Some((degree, knots, multiplicities))
}

fn expand_nurbs_axis(
    degree: u32,
    knots: &[f64],
    multiplicities: &[u32],
    payload_size: usize,
) -> Option<(Vec<f64>, usize)> {
    if knots.len() != multiplicities.len()
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities.contains(&0)
    {
        return None;
    }
    let total = multiplicities
        .iter()
        .try_fold(0usize, |total, multiplicity| {
            total.checked_add(usize::try_from(*multiplicity).ok()?)
        })?;
    if total > payload_size {
        return None;
    }
    let degree = usize::try_from(degree).ok()?;
    let control_count = total.checked_sub(degree.checked_add(1)?)?;
    if control_count <= degree || knots.first()? >= knots.last()? {
        return None;
    }
    let mut expanded = Vec::with_capacity(total);
    for (knot, multiplicity) in knots.iter().zip(multiplicities) {
        expanded.extend(std::iter::repeat_n(
            *knot,
            usize::try_from(*multiplicity).ok()?,
        ));
    }
    (expanded.len() == total).then_some((expanded, control_count))
}

fn e5_cylinder(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14);
    let origin = c.point3()?;
    let (geometry, radius) = crate::analytic::cylinder_uvr(&mut c, origin)?;
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    Some(geometry)
}

fn e5_cone(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14);
    let (geometry, radius, half_angle) = crate::analytic::cone_ozra(&mut c)?;
    if !(radius.is_finite()
        && radius > 0.0
        && half_angle.is_finite()
        && half_angle > 0.0
        && half_angle < std::f64::consts::FRAC_PI_2)
    {
        return None;
    }
    Some(geometry)
}

fn e5_torus(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14);
    let (geometry, major_radius, minor_radius) = crate::analytic::torus_ozrr(&mut c)?;
    if !(major_radius.is_finite()
        && major_radius > 0.0
        && minor_radius.is_finite()
        && minor_radius > 0.0)
    {
        return None;
    }
    Some(geometry)
}

fn e5_ref(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    match *bytes.get(at)? {
        0x38 => Some((u32_le_24(bytes, at + 1)?, at + 4)),
        0x18 => Some((View::u16_le_at(bytes, at + 1)? as u32, at + 3)),
        0x10 => Some((u32::from(*bytes.get(at + 1)?) << 8, at + 2)),
        0x08 => Some((*bytes.get(at + 1)? as u32, at + 2)),
        byte if byte >= 0x80 => Some(((byte - 0x80) as u32, at + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{e5_ref, e5_surfaces};

    fn append_e5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
        bytes.extend_from_slice(&[0xe5, 0x0d, 0x03, class, 0]);
        bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(payload);
    }

    fn append_nurbs_axis(payload: &mut Vec<u8>, degree: u32) {
        payload.extend_from_slice(&degree.to_le_bytes());
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[0.0_f64.to_le_bytes(), 1.0_f64.to_le_bytes()].concat());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
    }

    fn nurbs_surface_payload(mode: u16) -> Vec<u8> {
        let mut payload = vec![0x80];
        append_nurbs_axis(&mut payload, 1);
        append_nurbs_axis(&mut payload, 1);
        payload.extend_from_slice(&mode.to_le_bytes());
        for point in [
            [0.0_f64, 0.0, 0.0],
            [0.0_f64, 1.0, 0.0],
            [1.0_f64, 0.0, 0.0],
            [1.0_f64, 1.0, 0.0],
        ] {
            for value in point {
                payload.extend_from_slice(&value.to_le_bytes());
            }
        }
        if mode == 1 {
            for weight in [1.0_f64, 1.0, 1.0, 1.0] {
                payload.extend_from_slice(&weight.to_le_bytes());
            }
        }
        payload.extend_from_slice(&[0; 148]);
        payload
    }

    #[test]
    fn e5_width_coded_reference_widens_before_shifting() {
        assert_eq!(e5_ref(&[0x10, 0xff], 0), Some((0xff00, 2)));
    }

    #[test]
    fn e7_nurbs_surface_decodes_polynomial_and_rational_modes() {
        for mode in [0, 1] {
            let mut bytes = Vec::new();
            append_e5_record(&mut bytes, 0xe7, 116, &nurbs_surface_payload(mode));
            let surfaces = e5_surfaces(&bytes);
            let [surface] = surfaces.as_slice() else {
                panic!("E7 surface did not decode");
            };
            let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &surface.geometry else {
                panic!("E7 surface was not NURBS");
            };
            assert_eq!(nurbs.u_degree, 1);
            assert_eq!(nurbs.v_degree, 1);
            assert_eq!(nurbs.u_knots, [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.v_knots, [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.u_count, 2);
            assert_eq!(nurbs.v_count, 2);
            assert_eq!(nurbs.control_points.len(), 4);
            assert_eq!(nurbs.weights.is_some(), mode == 1);
        }
    }

    #[test]
    fn e7_nurbs_surface_requires_its_fixed_trailing_lane() {
        let mut payload = nurbs_surface_payload(0);
        payload.pop();
        let mut bytes = Vec::new();
        append_e5_record(&mut bytes, 0xe7, 116, &payload);
        assert!(e5_surfaces(&bytes).is_empty());
    }
}
