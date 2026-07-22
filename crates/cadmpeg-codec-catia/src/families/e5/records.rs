//! E5 storage-variant record decoders.
//!
//! Decodes E5 `05 08 01` vertex rosters, inline `0xc9` circle carriers,
//! class-`0xc8` planes, `0xff` edge-use records, and cylinder/cone/torus
//! analytic surface carriers.

use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::le::{u16_at as u16_le, u32_at as u32_le};
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

fn e5_records(data: &[u8]) -> Vec<E5Record> {
    const MARKER: &[u8; 3] = b"\xe5\x0d\x03";

    let mut records = Vec::new();
    let mut position = 0;
    while position + 13 <= data.len() {
        let Some(relative) = data[position..]
            .windows(MARKER.len())
            .position(|bytes| bytes == MARKER)
        else {
            break;
        };
        let pos = position + relative;
        let Some(size) = u16_le(data, pos + 5).map(usize::from) else {
            break;
        };
        let Some(end) = pos.checked_add(size + 13) else {
            break;
        };
        if end > data.len() {
            break;
        }
        records.push(E5Record {
            pos,
            end,
            class: data[pos + 3],
            size,
        });
        position = end;
    }
    records
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
                if radius > 0.05 && radius < 1e3 {
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
            record_id: u32_le(data, pos + 9).unwrap_or(0),
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
            0xc9 => e5_cylinder(data, pos).map(|geometry| {
                let SurfaceGeometry::Cylinder { radius, .. } = geometry else {
                    unreachable!()
                };
                (geometry, [1.0 / radius, 1.0])
            }),
            0xca => e5_cone(data, pos).and_then(|geometry| {
                let SurfaceGeometry::Cone { half_angle, .. } = geometry else {
                    unreachable!()
                };
                let u_scale = f64_le(data, pos + 158)?;
                let v_scale = f64_le(data, pos + 166)?;
                (u_scale.is_finite()
                    && u_scale.abs() > 1e-12
                    && v_scale.is_finite()
                    && v_scale.abs() > 1e-12)
                    .then_some((geometry, [1.0 / u_scale, half_angle.cos() / v_scale]))
            }),
            0xcc => e5_torus(data, pos).map(|geometry| {
                let SurfaceGeometry::Torus {
                    major_radius,
                    minor_radius,
                    ..
                } = geometry
                else {
                    unreachable!()
                };
                (geometry, [1.0 / major_radius, 1.0 / minor_radius])
            }),
            _ => None,
        };
        if let Some((geometry, uv_scale)) = decoded {
            out.push(E5Surface {
                pos,
                record_id: u32_le(data, pos + 9).unwrap_or(0),
                geometry,
                uv_scale,
            });
        }
    }
    out
}

fn e5_cylinder(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14);
    let origin = c.point3()?;
    let (geometry, radius) = crate::analytic::cylinder_uvr(&mut c, origin)?;
    if !radius.is_finite() || !(0.05..1e3).contains(&radius) {
        return None;
    }
    Some(geometry)
}

fn e5_cone(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14);
    let (geometry, radius, half_angle) = crate::analytic::cone_ozra(&mut c)?;
    if !(radius > 0.0
        && radius < 1e6
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
    if !(major_radius > 0.0 && major_radius < 1e6 && minor_radius > 0.0 && minor_radius < 1e6) {
        return None;
    }
    Some(geometry)
}

fn e5_ref(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    match *bytes.get(at)? {
        0x38 => Some((u32_le_24(bytes, at + 1)?, at + 4)),
        0x18 => Some((u16_le(bytes, at + 1)? as u32, at + 3)),
        0x10 => Some((u32::from(*bytes.get(at + 1)?) << 8, at + 2)),
        0x08 => Some((*bytes.get(at + 1)? as u32, at + 2)),
        byte if byte >= 0x80 => Some(((byte - 0x80) as u32, at + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::e5_ref;

    #[test]
    fn e5_width_coded_reference_widens_before_shifting() {
        assert_eq!(e5_ref(&[0x10, 0xff], 0), Some((0xff00, 2)));
    }
}
