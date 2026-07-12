// SPDX-License-Identifier: Apache-2.0
//! Geometry record decoders shared by the storage-variant paths.
//!
//! Standard nested streams store vertex coordinates in `05 08 01` records and
//! analytic face carriers in `SurfacicReps`. Curved carrier parameters are
//! inline; planes use a tag-linked parameter record. The module also decodes E5
//! and zero-entity analytic carriers, object-stream and consolidated NURBS
//! surfaces, UV pcurves, and selected edge-support records.
//!
//! Length values enter the IR in millimetres. Functions validate framing,
//! finite numeric payloads, structural counts, and family-specific invariants
//! before returning a carrier.

use cadmpeg_ir::be::f32_at as f32_be;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::le::{f64_at, u16_at as u16_le, u32_at as u32_le};
use cadmpeg_ir::math::{Point3, Vector3};

/// The standard-nested plane parameter record.  Its three-byte tag is the
/// bridge to the matching `SurfacicReps` plane marker.
#[derive(Debug, Clone)]
pub struct PlaneParams {
    /// The little-endian u24 carrier tag.
    pub target: u32,
    /// Offset of the `00 02 00 33 32` marker in the BREP stream.
    pub pos: usize,
    /// A point on the plane.
    pub origin: Point3,
    /// A stored in-plane diagonal.  Its y/z components determine the normal.
    pub diagonal: Vector3,
}

/// The `00 33 <kind>` surface kinds and their required strict-template prebyte
/// (the byte at `marker_pos - 1`), which filters collisional signature matches.
fn kind_prebyte(kind: u8) -> Option<u8> {
    match kind {
        0x32 => Some(0x02), // plane
        0x33 => Some(0x1a), // cylinder
        0x34 => Some(0x1a), // cone
        0x35 => Some(0x12), // sphere
        0x38 => Some(0x1e), // torus
        _ => None,
    }
}

/// A located per-face analytic surface record.
#[derive(Debug, Clone)]
pub struct SurfacePrefix {
    /// Offset of the `00 33 <kind>` signature within the BREP stream.
    pub pos: usize,
    /// The little-endian u24 tag that identifies this carrier.
    pub target: u32,
    /// The kind byte (`0x32`..=`0x38`).
    pub kind: u8,
}

/// Read the trailing per-face orientation byte from a complete analytic
/// `SurfacicReps` record. `true` means the face follows the carrier normal.
pub fn face_sense(brep: &[u8], prefix: &SurfacePrefix) -> Option<bool> {
    let length = match prefix.kind {
        0x32 => 49,
        0x33 | 0x34 => 73,
        0x35 => 65,
        0x38 => 77,
        _ => return None,
    };
    match *brep.get(prefix.pos.checked_sub(5)?.checked_add(length - 1)?)? {
        0x01 => Some(true),
        0xff => Some(false),
        _ => None,
    }
}

/// Read every `05 08 01` vertex record as `(x, y, z)` in millimetres.
///
/// Non-finite and out-of-range candidates are filtered (real part coordinates sit
/// well under 10 metres, and the 3-byte signature occurs incidentally in packed
/// sub-streams). Records retain their raw 1:1 correspondence with the
/// STEP `VERTEX_POINT` count.
pub fn vertices(brep: &[u8]) -> Vec<Point3> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 15 <= brep.len() {
        if brep[p] == 0x05 && brep[p + 1] == 0x08 && brep[p + 2] == 0x01 {
            let x = f32_le(brep, p + 3);
            let y = f32_le(brep, p + 7);
            let z = f32_le(brep, p + 11);
            if finite_in_range(x) && finite_in_range(y) && finite_in_range(z) {
                out.push(Point3::new(x as f64, y as f64, z as f64));
            }
            p += 15;
        } else {
            p += 1;
        }
    }
    out
}

/// Locate every per-face analytic surface record by the strict 5-byte template
/// `[target_u24 le][00][prebyte] 00 33 <kind>` ([spec §5.8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#58-analytic-surface-records-in-surfacicreps)). The strict template
/// rejects collisional `00 33` matches inside other binary data.
pub fn surface_prefixes(brep: &[u8]) -> Vec<SurfacePrefix> {
    let mut out = Vec::new();
    if brep.len() < 8 {
        return out;
    }
    for i in 5..brep.len() - 3 {
        if brep[i] != 0x00 || brep[i + 1] != 0x33 {
            continue;
        }
        let kind = brep[i + 2];
        let Some(prebyte) = kind_prebyte(kind) else {
            continue;
        };
        if brep[i - 2] != 0x00 || brep[i - 1] != prebyte {
            continue;
        }
        out.push(SurfacePrefix {
            pos: i,
            target: u24_le(brep, i - 5),
            kind,
        });
    }
    out
}

/// Locate plane parameter records and decode the stored point and in-plane
/// diagonal.  The record's cached diagonal length is validated before it is
/// returned, which rejects incidental marker matches.
pub fn plane_params(brep: &[u8]) -> Vec<PlaneParams> {
    const MARKER: &[u8; 5] = b"\x00\x02\x00\x33\x32";

    let mut out = Vec::new();
    let mut p = 0usize;
    while p + MARKER.len() + 40 <= brep.len() {
        let Some(relative) = brep[p..].windows(MARKER.len()).position(|w| w == MARKER) else {
            break;
        };
        let pos = p + relative;
        p = pos + 1;
        if pos < 4 || pos + MARKER.len() + 40 > brep.len() {
            continue;
        }
        let values: Vec<f32> = (0..10)
            .map(|i| f32_le(brep, pos + MARKER.len() + 4 * i))
            .collect();
        if !all_finite(&values) {
            continue;
        }
        let diagonal = Vector3::new(values[3] as f64, values[4] as f64, values[5] as f64);
        let length =
            (diagonal.x * diagonal.x + diagonal.y * diagonal.y + diagonal.z * diagonal.z).sqrt();
        if length <= 0.0 || (length - values[9] as f64).abs() > 1e-4 * length.max(1.0) {
            continue;
        }
        // The standard record stores profile planes with the first diagonal
        // component as the profile coordinate.  The perpendicular in the yz
        // plane is a valid normal; its sign is carrier-equivalent.
        let normal_length = (diagonal.y * diagonal.y + diagonal.z * diagonal.z).sqrt();
        if normal_length <= f64::EPSILON {
            continue;
        }
        out.push(PlaneParams {
            target: u24_le(brep, pos - 3),
            pos,
            origin: Point3::new(values[0] as f64, values[1] as f64, values[2] as f64),
            diagonal,
        });
    }
    out
}

/// Decode a plane carrier from its bridged parameter record.  Reversing the
/// normal represents the same untrimmed plane, so the record's unresolved
/// orientation bit is not needed for the carrier geometry.
pub fn decode_plane(params: &PlaneParams) -> SurfaceGeometry {
    let normal = Vector3::new(0.0, params.diagonal.z, -params.diagonal.y);
    let length = (normal.y * normal.y + normal.z * normal.z).sqrt();
    SurfaceGeometry::Plane {
        origin: params.origin,
        normal: Vector3::new(normal.x / length, normal.y / length, normal.z / length),
        u_axis: unit(params.diagonal)
            .unwrap_or_else(|| cadmpeg_ir::geometry::derive_reference_direction(normal)),
    }
}

/// A directly decoded analytic carrier in the zero-entity `a9 03` stream.
#[derive(Debug, Clone)]
pub struct ZeroEntitySurface {
    /// Offset of the framed record in the file.
    pub pos: usize,
    /// The decoded surface carrier.
    pub geometry: SurfaceGeometry,
}

/// A directly decoded E5 circle carrier.
#[derive(Debug, Clone)]
pub struct E5Circle {
    /// Offset of the `e5 0d 03` record in the source buffer.
    pub pos: usize,
    /// E5 persistent record id, when present.
    pub record_id: u32,
    /// The complete circle carrier.
    pub geometry: CurveGeometry,
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
                    if let Some(axis) = unit(cross(frame_u, frame_v)) {
                        out.push(E5Circle {
                            pos,
                            record_id: u32_le(data, pos + 9).unwrap_or(0),
                            geometry: CurveGeometry::Circle {
                                center: origin,
                                axis,
                                ref_direction: unit(frame_u).unwrap_or_else(|| {
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

/// A directly framed E5 edge-use record.  The endpoint ids are E5 vertex
/// records, not point-table indexes.
#[derive(Debug, Clone)]
pub struct E5Edge {
    /// Offset of the `e5 0d 03` record in the source buffer.
    pub pos: usize,
    /// Referenced curve-support (`0xc0`/`0xc1`) record id.
    pub support_id: u32,
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
            if let Some((support_id, next)) = e5_ref(payload, 1) {
                if let Some((start_vertex_id, next)) = e5_ref(payload, next) {
                    if let Some((end_vertex_id, _)) = e5_ref(payload, next) {
                        out.push(E5Edge {
                            pos,
                            support_id,
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
        let geometry = match record.class {
            0xc9 => e5_cylinder(data, pos),
            0xca => e5_cone(data, pos),
            0xcc => e5_torus(data, pos),
            _ => None,
        };
        if let Some(geometry) = geometry {
            out.push(E5Surface {
                pos,
                record_id: u32_le(data, pos + 9).unwrap_or(0),
                geometry,
            });
        }
    }
    out
}

fn e5_cylinder(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let origin = f64_point(data, pos + 14)?;
    let frame_u = f64_vector(data, pos + 38)?;
    let frame_v = f64_vector(data, pos + 62)?;
    let axis = unit(cross(frame_u, frame_v))?;
    let radius = f64_le(data, pos + 86)?;
    if !radius.is_finite() || !(0.05..1e3).contains(&radius) {
        return None;
    }
    Some(SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction: unit(frame_u)?,
        radius,
    })
}

/// A decoded common-form object-stream NURBS surface (`a8 03 34`).
#[derive(Debug, Clone)]
pub struct A8Surface {
    /// Source offset of the framed record.
    pub pos: usize,
    /// The inline persistent object id.
    pub object_id: u32,
    /// The decoded NURBS carrier.
    pub geometry: SurfaceGeometry,
}

/// A circle carrier in the standard `0x60` edge-support table.
#[derive(Debug, Clone)]
pub struct StandardCircle {
    /// Offset of the support row.
    pub pos: usize,
    /// The two incident standard face ordinals.
    pub faces: [usize; 2],
    /// Circle center in millimetres.
    pub center: Point3,
    /// Circle radius in millimetres.
    pub radius: f64,
}

/// A line carrier in the standard `0x60` edge-support table.
#[derive(Debug, Clone)]
pub struct StandardLine {
    /// Offset of the support row.
    pub pos: usize,
    /// The two incident standard face ordinals.
    pub faces: [usize; 2],
}

/// Geometry family carried by one positional standard `0x60` edge row.
#[derive(Debug, Clone)]
pub enum StandardCurveGeometry {
    /// The line equation is derived from endpoints or adjacent surfaces.
    Line,
    /// Inline circle parameters.
    Circle {
        /// Circle center in millimetres.
        center: Point3,
        /// Circle radius in millimetres.
        radius: f64,
    },
    /// A separately allocated spline carrier.
    Bspline,
}

/// One row of the standard positional edge-support/incidence table (spec
/// [§5.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#55-0x60-curve-support-edge-incidence-table)): `60 <tag:u24le> <curve_body> <face_ref> <face_ref>`, one row per
/// spine edge.
#[derive(Debug, Clone)]
pub struct StandardCurveSupport {
    /// Offset of the `0x60` row marker in the BREP stream.
    pub pos: usize,
    /// Little-endian u24 local allocation tag for this row.
    pub tag: u32,
    /// The two adjacent standard face ordinals forming this edge's
    /// edge-to-face incidence.
    pub faces: [usize; 2],
    /// The row's curve geometry family and, where inline, its parameters.
    pub geometry: StandardCurveGeometry,
}

/// Parse the contiguous standard `0x60` table in physical-edge order.
/// Leading generic spline rows are recovered backwards from the first analytic
/// row only when a unique valid row length lands exactly on the current start.
#[must_use]
pub fn standard_curve_supports(brep: &[u8], face_count: usize) -> Vec<StandardCurveSupport> {
    const LINE: [u8; 5] = [0x00, 0x02, 0x00, 0x33, 0x36];
    const CIRCLE: [u8; 5] = [0x00, 0x12, 0x00, 0x33, 0x37];
    let Some(mut first) = (0..brep.len()).find(|&position| {
        brep.get(position) == Some(&0x60)
            && brep
                .get(position + 4..position + 9)
                .is_some_and(|header| header == LINE || header == CIRCLE)
    }) else {
        return Vec::new();
    };

    loop {
        let mut candidate = None;
        let mut ambiguous = false;
        for row_length in [9usize, 13, 17] {
            let Some(start) = first.checked_sub(row_length) else {
                continue;
            };
            if brep.get(start) != Some(&0x60) || brep.get(start + 4..start + 7) != Some(&[0, 0, 0])
            {
                continue;
            }
            let mut position = start + 7;
            let Some((face0, next)) = face_ref(brep, position) else {
                continue;
            };
            position = next;
            let Some((face1, end)) = face_ref(brep, position) else {
                continue;
            };
            if end != first || face0 >= face_count || face1 >= face_count {
                continue;
            }
            if candidate.replace(start).is_some() {
                ambiguous = true;
                break;
            }
        }
        if ambiguous {
            break;
        }
        let Some(previous) = candidate else {
            break;
        };
        first = previous;
    }

    let mut rows = Vec::new();
    let mut position = first;
    while brep.get(position) == Some(&0x60) {
        let Some(tag_bytes) = brep.get(position + 1..position + 4) else {
            break;
        };
        let tag = u32::from_le_bytes([tag_bytes[0], tag_bytes[1], tag_bytes[2], 0]);
        let header = brep.get(position + 4..position + 9);
        let (geometry, refs) = if header == Some(&LINE) {
            (StandardCurveGeometry::Line, position + 9)
        } else if header == Some(&CIRCLE) {
            let Some(cx) = f32_be(brep, position + 9) else {
                break;
            };
            let Some(cy) = f32_be(brep, position + 13) else {
                break;
            };
            let Some(cz) = f32_be(brep, position + 17) else {
                break;
            };
            let Some(radius) = f32_be(brep, position + 21) else {
                break;
            };
            if !cx.is_finite()
                || !cy.is_finite()
                || !cz.is_finite()
                || !radius.is_finite()
                || radius <= 0.0
                || radius >= 1e6
            {
                break;
            }
            (
                StandardCurveGeometry::Circle {
                    center: Point3::new(f64::from(cx), f64::from(cy), f64::from(cz)),
                    radius: f64::from(radius),
                },
                position + 25,
            )
        } else if brep.get(position + 4..position + 7) == Some(&[0, 0, 0]) {
            (StandardCurveGeometry::Bspline, position + 7)
        } else {
            break;
        };
        let Some((face0, next)) = face_ref(brep, refs) else {
            break;
        };
        let Some((face1, end)) = face_ref(brep, next) else {
            break;
        };
        if face0 >= face_count || face1 >= face_count {
            break;
        }
        rows.push(StandardCurveSupport {
            pos: position,
            tag,
            faces: [face0, face1],
            geometry,
        });
        position = end;
    }
    rows
}

/// Parse complete circle rows from a standard `0x60` support table.  The table
/// is accepted only as a contiguous run whose face references stay in range.
pub fn standard_circles(brep: &[u8], face_count: usize) -> Vec<StandardCircle> {
    standard_curve_supports(brep, face_count)
        .into_iter()
        .filter_map(|row| match row.geometry {
            StandardCurveGeometry::Circle { center, radius } => Some(StandardCircle {
                pos: row.pos,
                faces: row.faces,
                center,
                radius,
            }),
            StandardCurveGeometry::Line | StandardCurveGeometry::Bspline => None,
        })
        .collect()
}

/// Parse standard line support rows.  The line equation is supplied by the two
/// incident plane carriers, not inline in the row.
pub fn standard_lines(brep: &[u8], face_count: usize) -> Vec<StandardLine> {
    standard_curve_supports(brep, face_count)
        .into_iter()
        .filter_map(|row| match row.geometry {
            StandardCurveGeometry::Line => Some(StandardLine {
                pos: row.pos,
                faces: row.faces,
            }),
            StandardCurveGeometry::Circle { .. } | StandardCurveGeometry::Bspline => None,
        })
        .collect()
}

/// Decode common-form object-stream NURBS surfaces.  Every variable-length
/// field is bounded by the record's `payload_len`, so signature collisions do
/// not become carriers.
pub fn a8_surfaces(data: &[u8]) -> Vec<A8Surface> {
    scan_surfaces(data, *b"\xa8\x03\x34", 3, a8_surface)
}

/// Decode consolidated `a5 03 34` NURBS surface carriers.  This family uses
/// implicit clamped multiplicities instead of the explicit `a8` vectors.
pub fn a5_surfaces(data: &[u8]) -> Vec<A8Surface> {
    scan_surfaces(data, *b"\xa5\x03\x34", 1, a5_surface)
}

fn scan_surfaces(
    data: &[u8],
    marker: [u8; 3],
    advance: usize,
    decode: fn(&[u8], usize) -> Option<A8Surface>,
) -> Vec<A8Surface> {
    let mut out = Vec::new();
    let mut start = 0usize;
    while let Some(relative) = data[start..]
        .windows(marker.len())
        .position(|bytes| bytes == marker)
    {
        let pos = start + relative;
        start = pos + advance;
        let Some(surface) = decode(data, pos) else {
            continue;
        };
        out.push(surface);
    }
    out
}

fn a5_surface(data: &[u8], pos: usize) -> Option<A8Surface> {
    let _payload_len = u32_le(data, pos + 3)?;
    let mut at = pos + 8;
    let u_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let u_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let u_distinct = f64_values(data, &mut at, u_distinct_count, data.len())?;
    let v_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let v_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let v_distinct = f64_values(data, &mut at, v_distinct_count, data.len())?;
    let mode = *data.get(at)?;
    at += 1;
    let (u_knots, u_count) = a5_knots(&u_distinct, u_degree)?;
    let (v_knots, v_count) = a5_knots(&v_distinct, v_degree)?;
    if !monotonic(&u_distinct) || !monotonic(&v_distinct) {
        return None;
    }
    let poles = (u_count as usize).checked_mul(v_count as usize)?;
    if at.checked_add(poles.checked_mul(24)?)? > data.len() {
        return None;
    }
    let mut control_points = Vec::with_capacity(poles);
    for _ in 0..poles {
        control_points.push(f64_point(data, at)?);
        at += 24;
    }
    let weights = match mode {
        0x01 => None,
        0x05 => Some(a5_weights(
            data,
            &mut at,
            u_count as usize,
            v_count as usize,
        )?),
        _ => return None,
    };
    // The structured tail is the false-positive gate for this unlength-framed family.
    if !matches!(data.get(at..at + 4), Some([0x05, a, 0x05, b]) if (*a - 1) % 4 == 0 && (*b - 1) % 4 == 0)
    {
        return None;
    }
    Some(A8Surface {
        pos,
        object_id: 0,
        geometry: SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            u_count,
            v_count,
            control_points,
            weights,
            u_periodic: false,
            v_periodic: false,
        }),
    })
}

fn a8_surface(data: &[u8], pos: usize) -> Option<A8Surface> {
    let payload_len = u32_le(data, pos + 3)? as usize;
    let object_id = u32_le(data, pos + 7)?;
    let end = pos.checked_add(11)?.checked_add(payload_len)?;
    if payload_len < 20 || end > data.len() {
        return None;
    }
    let mut at = pos + 12; // framing + lead byte
    let u_degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?; // flags
    let u_distinct_count = compact_int(data, &mut at)? as usize;
    at = consume_array_marker(data, at)?;
    let u_distinct = f64_values(data, &mut at, u_distinct_count, end)?;
    let u_mults = compact_values(data, &mut at, u_distinct_count)?;
    let v_degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?;
    let v_distinct_count = compact_int(data, &mut at)? as usize;
    at = consume_array_marker(data, at)?;
    let v_distinct = f64_values(data, &mut at, v_distinct_count, end)?;
    let v_mults = compact_values(data, &mut at, v_distinct_count)?;
    let mode = *data.get(at)?;
    at += 1;
    if !(1..=9).contains(&u_degree)
        || !(1..=9).contains(&v_degree)
        || !(2..=8192).contains(&u_distinct_count)
        || !(2..=8192).contains(&v_distinct_count)
        || !matches!(mode, 0x01 | 0x05)
        || !monotonic(&u_distinct)
        || !monotonic(&v_distinct)
    {
        return None;
    }
    let u_count = pole_count(&u_mults, u_degree)?;
    let v_count = pole_count(&v_mults, v_degree)?;
    if u_count == 0 || v_count == 0 || u_count > 20_000 || v_count > 20_000 {
        return None;
    }
    let poles = (u_count as usize).checked_mul(v_count as usize)?;
    let pole_bytes = poles.checked_mul(24)?;
    if at.checked_add(pole_bytes)? > end {
        return None;
    }
    let mut control_points = Vec::with_capacity(poles);
    for _ in 0..poles {
        control_points.push(f64_point(data, at)?);
        at += 24;
    }
    let weights = if mode == 0x05 {
        let values = f64_values(data, &mut at, poles, end)?;
        values
            .iter()
            .all(|weight| *weight != 0.0)
            .then_some(values)?
    } else {
        Vec::new()
    };
    if at > end {
        return None;
    }
    Some(A8Surface {
        pos,
        object_id,
        geometry: SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree,
            v_degree,
            u_knots: expand_knots(&u_distinct, &u_mults)?,
            v_knots: expand_knots(&v_distinct, &v_mults)?,
            u_count,
            v_count,
            control_points,
            weights: (mode == 0x05).then_some(weights),
            u_periodic: false,
            v_periodic: false,
        }),
    })
}

fn e5_cone(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let origin = f64_point(data, pos + 14)?;
    let ref_direction = unit(f64_vector(data, pos + 38)?);
    let axis = unit(f64_vector(data, pos + 86)?)?;
    let stored_angle = f64_le(data, pos + 110)?;
    let radius = f64_le(data, pos + 118)?;
    let half_angle = std::f64::consts::FRAC_PI_2 - stored_angle;
    if !(radius > 0.0
        && radius < 1e6
        && half_angle > 0.0
        && half_angle < std::f64::consts::FRAC_PI_2)
    {
        return None;
    }
    Some(SurfaceGeometry::Cone {
        origin,
        axis,
        ref_direction: ref_direction?,
        radius,
        half_angle,
    })
}

fn e5_torus(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let center = f64_point(data, pos + 14)?;
    let ref_direction = unit(f64_vector(data, pos + 38)?);
    let axis = unit(f64_vector(data, pos + 86)?)?;
    let major_radius = f64_le(data, pos + 110)?;
    let minor_radius = f64_le(data, pos + 118)?;
    if !(major_radius > 0.0 && major_radius < 1e6 && minor_radius > 0.0 && minor_radius < 1e6) {
        return None;
    }
    Some(SurfaceGeometry::Torus {
        center,
        axis,
        ref_direction: ref_direction?,
        major_radius,
        minor_radius,
    })
}

/// Decode analytic surface carriers in a zero-entity `a9 03` stream.  The
/// record's second tag byte is also its length code (`length = tag + 12`), so
/// the decoder walks framed records.
pub fn zero_entity_surfaces(data: &[u8]) -> Vec<ZeroEntitySurface> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 4 <= data.len() {
        if data[p..p + 2] != [0xa9, 0x03] {
            p += 1;
            continue;
        }
        let end = p.saturating_add(data[p + 3] as usize + 12);
        if end > data.len() {
            break;
        }
        let payload = &data[p + 4..end];
        let geometry = match (data[p + 2], data[p + 3]) {
            (0x27, 0x6a) => zero_entity_plane(payload),
            (0x28, 0x8a) => zero_entity_cylinder(payload),
            (0x29, 0xb8) => zero_entity_cone(payload),
            (0x2b, 0xc8) => zero_entity_torus(payload),
            (0x34, 0xc8 | 0x5e) => zero_entity_nurbs_surface(data, p),
            _ => None,
        };
        if let Some(geometry) = geometry {
            out.push(ZeroEntitySurface { pos: p, geometry });
        }
        p = end;
    }
    out
}

/// Decode the inline zero-entity non-rational NURBS carrier.  Its pole grid
/// follows the nominal framed record length, so this function receives the full
/// preamble.
fn zero_entity_nurbs_surface(data: &[u8], record: usize) -> Option<SurfaceGeometry> {
    let (u_distinct, after_u) = f64_run_to_one(data, record.checked_add(23)?)?;
    let (u_mults, after_u_mults) = u32_tokens(data, after_u, u_distinct.len())?;
    let u_degree = u_mults.first().copied()?.checked_sub(1)?;
    let u_count = u_mults
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(u_degree + 1)?;
    let (v_distinct, after_v) = f64_monotonic_run(data, after_u_mults.checked_add(1)?)?;
    let (v_mults, after_v_mults) = u32_tokens(data, after_v, v_distinct.len())?;
    let v_degree = v_mults.first().copied()?.checked_sub(1)?;
    let v_count = v_mults
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(v_degree + 1)?;
    if !(1..=9).contains(&u_degree)
        || !(1..=9).contains(&v_degree)
        || !(2..=4096).contains(&u_count)
        || !(2..=4096).contains(&v_count)
    {
        return None;
    }
    let pole_count = (u_count as usize).checked_mul(v_count as usize)?;
    let grid = after_v_mults.checked_add(3)?;
    let mut control_points = Vec::with_capacity(pole_count);
    for pole in 0..pole_count {
        control_points.push(f64_point(data, grid.checked_add(pole.checked_mul(24)?)?)?);
    }
    Some(SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree,
        v_degree,
        u_knots: expand_knots(&u_distinct, &u_mults)?,
        v_knots: expand_knots(&v_distinct, &v_mults)?,
        u_count,
        v_count,
        control_points,
        weights: None,
        u_periodic: false,
        v_periodic: false,
    }))
}

fn zero_entity_plane(payload: &[u8]) -> Option<SurfaceGeometry> {
    let origin = f64_point(payload, 10)?;
    let row0 = f64_vector(payload, 34)?;
    let row1 = f64_vector(payload, 58)?;
    Some(SurfaceGeometry::Plane {
        origin,
        normal: unit(cross(row0, row1))?,
        u_axis: unit(row0)?,
    })
}

fn zero_entity_cylinder(payload: &[u8]) -> Option<SurfaceGeometry> {
    let origin = f64_point(payload, 8)?;
    let row0 = f64_vector(payload, 33)?;
    let row1 = f64_vector(payload, 57)?;
    let radius = f64_le(payload, 81)?;
    if !(radius.is_finite() && radius > 0.0 && radius < 1e6) {
        return None;
    }
    Some(SurfaceGeometry::Cylinder {
        origin,
        axis: unit(cross(row0, row1))?,
        ref_direction: unit(row0)?,
        radius,
    })
}

fn zero_entity_cone(payload: &[u8]) -> Option<SurfaceGeometry> {
    let origin = f64_point(payload, 8)?;
    let ref_direction = unit(f64_vector(payload, 32)?);
    let axis = unit(f64_vector(payload, 80)?)?;
    let stored_angle = f64_le(payload, 104)?;
    let radius = f64_le(payload, 112)?;
    let half_angle = std::f64::consts::FRAC_PI_2 - stored_angle;
    if !(radius.is_finite()
        && radius > 0.0
        && radius < 1e6
        && half_angle.is_finite()
        && half_angle > 0.0
        && half_angle < std::f64::consts::FRAC_PI_2)
    {
        return None;
    }
    Some(SurfaceGeometry::Cone {
        origin,
        axis,
        ref_direction: ref_direction?,
        radius,
        half_angle,
    })
}

fn zero_entity_torus(payload: &[u8]) -> Option<SurfaceGeometry> {
    let center = f64_point(payload, 8)?;
    let ref_direction = unit(f64_vector(payload, 32)?);
    let axis = unit(f64_vector(payload, 80)?)?;
    let major_radius = f64_le(payload, 104)?;
    let minor_radius = f64_le(payload, 112)?;
    if !(major_radius.is_finite()
        && major_radius > 0.0
        && major_radius < 1e6
        && minor_radius.is_finite()
        && minor_radius > 0.0
        && minor_radius < 1e6)
    {
        return None;
    }
    Some(SurfaceGeometry::Torus {
        center,
        axis,
        ref_direction: ref_direction?,
        major_radius,
        minor_radius,
    })
}

/// Decode the analytic parameters carried inline in a curved surface's kind
/// record. The big-endian `f32` payload begins immediately after the 3-byte
/// `00 33 <kind>` marker ([spec §5.8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#58-analytic-surface-records-in-surfacicreps)). Returns `None` for the plane kind (its
/// parameters are in a separate bridged record) and for any non-finite or
/// out-of-range payload.
pub fn decode_curved(brep: &[u8], prefix: &SurfacePrefix) -> Option<SurfaceGeometry> {
    let p = prefix.pos + 3; // skip `00 33 <kind>`
    let be = |i: usize| -> Option<f32> {
        brep.get(p + 4 * i..p + 4 * i + 4)
            .map(|s| f32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    };
    match prefix.kind {
        0x35 => {
            // sphere: cx cy cz radius
            let (cx, cy, cz, r) = (be(0)?, be(1)?, be(2)?, be(3)?);
            if !all_finite(&[cx, cy, cz, r]) || !(0.0..1e6).contains(&r.abs()) || r <= 0.0 {
                return None;
            }
            Some(SurfaceGeometry::Sphere {
                center: pt(cx, cy, cz),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: r as f64,
            })
        }
        0x38 => {
            // torus: cx cy cz ax ay major minor
            let (cx, cy, cz, ax, ay, major, minor) =
                (be(0)?, be(1)?, be(2)?, be(3)?, be(4)?, be(5)?, be(6)?);
            if !all_finite(&[cx, cy, cz, ax, ay, major, minor]) {
                return None;
            }
            if !(major > 0.0 && major < 1e6 && minor > 0.0 && minor < 1e6) {
                return None;
            }
            Some(SurfaceGeometry::Torus {
                center: pt(cx, cy, cz),
                axis: axis_from_xy(ax, ay, 1.0),
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis_from_xy(
                    ax, ay, 1.0,
                )),
                major_radius: major as f64,
                minor_radius: minor as f64,
            })
        }
        0x33 => {
            // cylinder: px py pz ax ay radius; sign(radius) carries sign(az).
            let (px, py, pz, ax, ay, radius) = (be(0)?, be(1)?, be(2)?, be(3)?, be(4)?, be(5)?);
            if !all_finite(&[px, py, pz, ax, ay, radius]) {
                return None;
            }
            if !(radius.abs() > 0.0 && radius.abs() < 1e6) || ax * ax + ay * ay > 1.0 + 1e-4 {
                return None;
            }
            Some(SurfaceGeometry::Cylinder {
                origin: pt(px, py, pz),
                axis: axis_from_xy(ax, ay, radius),
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis_from_xy(
                    ax, ay, radius,
                )),
                radius: radius.abs() as f64,
            })
        }
        0x34 => {
            // cone: apex_x apex_y apex_z ax ay semi_angle; radius at apex is 0.
            let (x, y, z, ax, ay, semi) = (be(0)?, be(1)?, be(2)?, be(3)?, be(4)?, be(5)?);
            if !all_finite(&[x, y, z, ax, ay, semi]) {
                return None;
            }
            if !(semi.abs() > 0.0 && semi.abs() < std::f32::consts::FRAC_PI_2) {
                return None;
            }
            Some(SurfaceGeometry::Cone {
                origin: pt(x, y, z),
                axis: axis_from_xy(ax, ay, semi),
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis_from_xy(
                    ax, ay, semi,
                )),
                radius: 0.0,
                half_angle: semi.abs() as f64,
            })
        }
        _ => None, // plane: parameters in a separate bridged record.
    }
}

fn pt(x: f32, y: f32, z: f32) -> Point3 {
    Point3::new(x as f64, y as f64, z as f64)
}

/// Recover the third axis component from the unit-norm constraint, taking its
/// sign from a companion signed field (the cone/cylinder store `sign(az)` in the
/// sign of the semi-angle / radius).
fn axis_from_xy(ax: f32, ay: f32, signed: f32) -> Vector3 {
    let az2 = (1.0 - (ax * ax + ay * ay) as f64).max(0.0);
    let az = az2.sqrt().copysign(signed as f64);
    Vector3::new(ax as f64, ay as f64, az)
}

fn f32_le(bytes: &[u8], at: usize) -> f32 {
    cadmpeg_ir::le::f32_at(bytes, at).unwrap_or(f32::NAN)
}

fn face_ref(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    match *bytes.get(at)? {
        0xff => Some((u32_le(bytes, at + 1)? as usize, at + 5)),
        value => Some((value as usize, at + 1)),
    }
}

fn u24_le(bytes: &[u8], at: usize) -> u32 {
    bytes[at] as u32 | ((bytes[at + 1] as u32) << 8) | ((bytes[at + 2] as u32) << 16)
}

fn f64_le(bytes: &[u8], at: usize) -> Option<f64> {
    let value = f64_at(bytes, at)?;
    value.is_finite().then_some(value)
}

fn f64_point(bytes: &[u8], at: usize) -> Option<Point3> {
    Some(Point3::new(
        f64_le(bytes, at)?,
        f64_le(bytes, at + 8)?,
        f64_le(bytes, at + 16)?,
    ))
}

fn f64_vector(bytes: &[u8], at: usize) -> Option<Vector3> {
    Some(Vector3::new(
        f64_le(bytes, at)?,
        f64_le(bytes, at + 8)?,
        f64_le(bytes, at + 16)?,
    ))
}

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn unit(v: Vector3) -> Option<Vector3> {
    let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    (length > f64::EPSILON).then(|| Vector3::new(v.x / length, v.y / length, v.z / length))
}

fn f64_run_to_one(bytes: &[u8], mut at: usize) -> Option<(Vec<f64>, usize)> {
    let mut values = Vec::new();
    loop {
        let value = f64_le(bytes, at)?;
        if !(0.0..=1.0).contains(&value) || values.last().is_some_and(|last| value < *last) {
            return None;
        }
        values.push(value);
        at = at.checked_add(8)?;
        if value == 1.0 {
            return (values.len() >= 2).then_some((values, at));
        }
        if values.len() > 4096 {
            return None;
        }
    }
}

fn f64_monotonic_run(bytes: &[u8], mut at: usize) -> Option<(Vec<f64>, usize)> {
    let mut values = Vec::new();
    while let Some(value) = f64_le(bytes, at) {
        if !(0.0..=50.0).contains(&value) || values.last().is_some_and(|last| value < *last) {
            break;
        }
        values.push(value);
        at = at.checked_add(8)?;
        if values.len() > 4096 {
            return None;
        }
    }
    (values.len() >= 2).then_some((values, at))
}

fn u32_tokens(bytes: &[u8], mut at: usize, count: usize) -> Option<(Vec<u32>, usize)> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if *bytes.get(at)? != 0x10 {
            return None;
        }
        let raw: [u8; 4] = bytes.get(at + 1..at + 5)?.try_into().ok()?;
        let value = u32::from_le_bytes(raw);
        if value == 0 {
            return None;
        }
        values.push(value);
        at = at.checked_add(5)?;
    }
    Some((values, at))
}

fn expand_knots(distinct: &[f64], multiplicities: &[u32]) -> Option<Vec<f64>> {
    let capacity = multiplicities
        .iter()
        .try_fold(0usize, |sum, value| sum.checked_add(*value as usize))?;
    let mut knots = Vec::with_capacity(capacity);
    for (&knot, &multiplicity) in distinct.iter().zip(multiplicities) {
        knots.extend(std::iter::repeat_n(knot, multiplicity as usize));
    }
    Some(knots)
}

fn e5_ref(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    match *bytes.get(at)? {
        0x38 => Some((u32_le_24(bytes, at + 1)?, at + 4)),
        0x18 => Some((u16_le(bytes, at + 1)? as u32, at + 3)),
        0x10 => Some(((bytes.get(at + 1)? << 8) as u32, at + 2)),
        0x08 => Some((*bytes.get(at + 1)? as u32, at + 2)),
        byte if byte >= 0x80 => Some(((byte - 0x80) as u32, at + 1)),
        _ => None,
    }
}

fn u32_le_24(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        0,
    ]))
}

fn compact_int(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let byte = *bytes.get(*at)?;
    if byte % 4 == 1 {
        *at += 1;
        Some(((byte - 1) / 4) as u32)
    } else if byte != 0 && byte % 4 == 0 {
        let width = (byte / 4) as usize;
        if width > 4 {
            return None;
        }
        let mut value = 0u32;
        for (shift, byte) in bytes.get(*at + 1..*at + 1 + width)?.iter().enumerate() {
            value |= (*byte as u32) << (shift * 8);
        }
        *at += width + 1;
        Some(value)
    } else {
        None
    }
}

fn consume_array_marker(bytes: &[u8], at: usize) -> Option<usize> {
    if *bytes.get(at)? == 0x08 {
        bytes.get(at + 1).map(|_| at + 2)
    } else {
        Some(at + 1)
    }
}

fn f64_values(bytes: &[u8], at: &mut usize, count: usize, end: usize) -> Option<Vec<f64>> {
    if at.checked_add(count.checked_mul(8)?)? > end {
        return None;
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(f64_le(bytes, *at)?);
        *at += 8;
    }
    Some(values)
}

fn compact_values(bytes: &[u8], at: &mut usize, count: usize) -> Option<Vec<u32>> {
    (0..count).map(|_| compact_int(bytes, at)).collect()
}

fn monotonic(values: &[f64]) -> bool {
    values.windows(2).all(|pair| pair[0] <= pair[1])
}

fn pole_count(multiplicities: &[u32], degree: u32) -> Option<u32> {
    multiplicities
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(degree + 1)
}

fn a5_int(byte: u8) -> Option<u32> {
    (byte % 4 == 1).then_some(((byte - 1) / 4) as u32)
}

fn a5_array_marker(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes.get(at..at + 2) {
        Some([0x0c, ..]) => Some(at + 1),
        Some([0x08, 0x09]) => Some(at + 2),
        _ => None,
    }
}

fn a5_knots(distinct: &[f64], degree: u32) -> Option<(Vec<f64>, u32)> {
    let multiplicities = match degree {
        5 if distinct.len() >= 2 => {
            let mut values = vec![6u32];
            values.extend(std::iter::repeat_n(3, distinct.len() - 2));
            values.push(6);
            values
        }
        1 if distinct.len() == 2 => vec![2, 2],
        _ => return None,
    };
    let count = pole_count(&multiplicities, degree)?;
    Some((expand_knots(distinct, &multiplicities)?, count))
}

fn a5_weights(bytes: &[u8], at: &mut usize, rows: usize, cols: usize) -> Option<Vec<f64>> {
    if bytes.get(*at..*at + 3)? != [0x01, 0x07, 0x00] {
        return None;
    }
    *at += 3;
    let mut seed = Vec::new();
    while bytes.get(*at) != Some(&0x02) {
        seed.push(f64_le(bytes, *at)?);
        *at += 8;
    }
    let row = if seed.len() * 2 == cols {
        seed.iter()
            .copied()
            .chain(seed.iter().rev().copied())
            .collect()
    } else if seed.len() == cols {
        seed
    } else {
        return None;
    };
    let mut copies = 0usize;
    while bytes.get(*at) == Some(&0x02) {
        copies += 1;
        *at += 1;
    }
    if copies + 1 != rows || row.contains(&0.0) {
        return None;
    }
    Some((0..rows).flat_map(|_| row.iter().copied()).collect())
}

fn finite_in_range(v: f32) -> bool {
    v.is_finite() && v.abs() < 1e4
}

fn all_finite(vs: &[f32]) -> bool {
    vs.iter().all(|v| v.is_finite())
}
