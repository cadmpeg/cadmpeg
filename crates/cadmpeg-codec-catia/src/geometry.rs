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

use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
};

use cadmpeg_ir::be::f32_at as f32_be;
use cadmpeg_ir::eval::nurbs_surface_partials;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::le::{f64_at, u16_at as u16_le, u32_at as u32_le};
use cadmpeg_ir::math::{Point3, Vector3};

/// The standard-nested plane bounds record. Its three-byte tag is the bridge to
/// the matching `SurfacicReps` plane marker.
#[derive(Debug, Clone)]
pub struct PlaneParams {
    /// The little-endian u24 carrier tag.
    pub target: u32,
    /// Offset of the `00 02 00 33 32` marker in the BREP stream.
    pub pos: usize,
    /// Bounding-sphere center, which lies on the plane.
    pub origin: Point3,
    /// Unit plane normal from the positionally paired trim packet.
    pub normal: Vector3,
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

/// One face-local record in the standard `SurfacicReps` surface roster.
#[derive(Debug, Clone)]
pub enum StandardSurfaceRecord {
    /// Fixed-length analytic carrier record.
    Analytic(SurfacePrefix),
    /// Face bounds and orientation for a carrier linked through an outer alias.
    Freeform {
        /// Record byte offset.
        pos: usize,
        /// Little-endian u24 carrier tag.
        tag: u32,
        /// Trimmed-face spatial bounds stored in the roster core.
        bounds: FreeformFaceBounds,
        /// Face orientation relative to the linked carrier.
        forward: bool,
    },
}

/// Spatial bounds stored by one standard freeform face roster core.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreeformFaceBounds {
    /// Axis-aligned bounding-box centre.
    pub aabb_center: [f64; 3],
    /// Non-negative axis-aligned bounding-box half-extents.
    pub aabb_half_extents: [f64; 3],
    /// Bounding-sphere centre.
    pub sphere_center: [f64; 3],
    /// Non-negative bounding-sphere radius.
    pub sphere_radius: f64,
}

impl StandardSurfaceRecord {
    fn pos(&self) -> usize {
        match self {
            Self::Analytic(prefix) => prefix.pos - 5,
            Self::Freeform { pos, .. } => *pos,
        }
    }

    fn end(&self) -> usize {
        match self {
            Self::Analytic(prefix) => {
                self.pos()
                    + match prefix.kind {
                        0x32 => 49,
                        0x33 | 0x34 => 73,
                        0x35 => 65,
                        0x38 => 77,
                        _ => unreachable!("analytic roster kinds are filtered"),
                    }
            }
            Self::Freeform { pos, .. } => pos + 47,
        }
    }
}

/// Walk the complete face-local surface roster. Records are accepted only as a
/// unique contiguous chain of `face_count` entries terminated by the first
/// curve-support row.
#[must_use]
pub fn standard_surface_records(
    brep: &[u8],
    face_count: usize,
) -> Option<Vec<StandardSurfaceRecord>> {
    if face_count == 0 {
        return None;
    }
    let mut records = BTreeMap::<usize, StandardSurfaceRecord>::new();
    for prefix in surface_prefixes(brep) {
        if face_sense(brep, &prefix).is_some() {
            records.insert(prefix.pos - 5, StandardSurfaceRecord::Analytic(prefix));
        }
    }
    for pos in 0..brep.len().saturating_sub(46) {
        if brep.get(pos + 3..pos + 6) != Some(&[0, 0, 0]) {
            continue;
        }
        let tag = u24_le(brep, pos);
        let forward = match brep[pos + 46] {
            0x01 => true,
            0xff => false,
            _ => continue,
        };
        let values = (0..10)
            .map(|index| f32_le(brep, pos + 6 + 4 * index))
            .collect::<Vec<_>>();
        if tag == 0
            || values.iter().any(|value| !value.is_finite())
            || values[3..6].iter().any(|extent| *extent < 0.0)
            || values[9] < 0.0
            || (0..3)
                .any(|axis| (values[axis] - values[6 + axis]).abs() + values[3 + axis] > values[9])
        {
            continue;
        }
        records.insert(
            pos,
            StandardSurfaceRecord::Freeform {
                pos,
                tag,
                bounds: FreeformFaceBounds {
                    aabb_center: [
                        f64::from(values[0]),
                        f64::from(values[1]),
                        f64::from(values[2]),
                    ],
                    aabb_half_extents: [
                        f64::from(values[3]),
                        f64::from(values[4]),
                        f64::from(values[5]),
                    ],
                    sphere_center: [
                        f64::from(values[6]),
                        f64::from(values[7]),
                        f64::from(values[8]),
                    ],
                    sphere_radius: f64::from(values[9]),
                },
                forward,
            },
        );
    }

    let mut solutions = Vec::new();
    for &start in records.keys() {
        let mut at = start;
        let mut chain = Vec::with_capacity(face_count);
        for _ in 0..face_count {
            let Some(record) = records.get(&at) else {
                break;
            };
            chain.push(record.clone());
            at = record.end();
        }
        if chain.len() == face_count && brep.get(at) == Some(&0x60) {
            solutions.push(chain);
        }
    }
    <[Vec<StandardSurfaceRecord>; 1]>::try_from(solutions)
        .ok()
        .map(|[records]| records)
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

/// Read the unique contiguous standard vertex roster with the requested
/// cardinality. Each seven-byte row stores `54 <identity:u24le> 00 00 00`;
/// roster order is coordinate-table order.
#[must_use]
pub fn standard_vertex_roster(source: &[u8], vertex_count: usize) -> Option<Vec<u32>> {
    if vertex_count == 0 {
        return None;
    }
    let mut solutions = Vec::new();
    let mut position = 0usize;
    while position + 7 <= source.len() {
        if source[position] != 0x54 || source[position + 4..position + 7] != [0, 0, 0] {
            position += 1;
            continue;
        }
        let start = position;
        let mut identities = Vec::new();
        while position + 7 <= source.len()
            && source[position] == 0x54
            && source[position + 4..position + 7] == [0, 0, 0]
        {
            let identity = u32::from_le_bytes([
                source[position + 1],
                source[position + 2],
                source[position + 3],
                0,
            ]);
            if identities
                .last()
                .is_some_and(|previous| *previous >= identity)
            {
                break;
            }
            identities.push(identity);
            position += 7;
        }
        if identities.len() == vertex_count {
            solutions.push(identities);
        }
        if position == start {
            position += 1;
        }
    }
    <[Vec<u32>; 1]>::try_from(solutions)
        .ok()
        .map(|[identities]| identities)
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

/// Locate plane bounds records and bind them positionally to framed planar trim
/// packet normals.
pub fn plane_params(brep: &[u8], normals: &[[f64; 3]]) -> Vec<PlaneParams> {
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
        let half = [values[3].abs(), values[4].abs(), values[5].abs()];
        let sphere = [values[6], values[7], values[8]];
        let radius = values[9];
        if radius <= 0.0
            || (0..3).any(|axis| (values[axis] - sphere[axis]).abs() + half[axis] > radius)
        {
            continue;
        }
        let Some(normal) = normals.get(out.len()).copied() else {
            continue;
        };
        out.push(PlaneParams {
            target: u24_le(brep, pos - 3),
            pos,
            origin: Point3::new(
                f64::from(sphere[0]),
                f64::from(sphere[1]),
                f64::from(sphere[2]),
            ),
            normal: Vector3::new(normal[0], normal[1], normal[2]),
        });
    }
    out
}

/// Decode a plane carrier from its bridged bounds and trim-frame records.
pub fn decode_plane(params: &PlaneParams) -> SurfaceGeometry {
    SurfaceGeometry::Plane {
        origin: params.origin,
        normal: params.normal,
        u_axis: cadmpeg_ir::geometry::derive_reference_direction(params.normal),
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
    pub u_range: [f64; 2],
    /// Natural V-coordinate bounds.
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
            u_range: [bounds[0], bounds[1]],
            v_range: [bounds[2], bounds[3]],
        });
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

/// Parameter lattice decoded from an `a8 03 34` surface record independently
/// of its pole representation.
#[derive(Debug, Clone, PartialEq)]
pub struct A8SurfaceHeader {
    /// Source offset of the framed record.
    pub pos: usize,
    /// Inline persistent object id.
    pub object_id: u32,
    /// U degree.
    pub u_degree: u32,
    /// V degree.
    pub v_degree: u32,
    /// Distinct U knots.
    pub u_distinct_knots: Vec<f64>,
    /// Distinct V knots.
    pub v_distinct_knots: Vec<f64>,
    /// U multiplicities corresponding to `u_distinct_knots`.
    pub u_multiplicities: Vec<u32>,
    /// V multiplicities corresponding to `v_distinct_knots`.
    pub v_multiplicities: Vec<u32>,
    /// Derived U pole count.
    pub u_count: u32,
    /// Derived V pole count.
    pub v_count: u32,
    /// Whether the record selects rational weights.
    pub rational: bool,
    /// The fixed 141-byte surface tail begins immediately after the mode byte,
    /// so no inline pole or weight grid is present.
    pub poles_elided: bool,
}

#[derive(Debug, Clone)]
/// Degree-5 UV jet stored in an `a8 03 20` object record.
pub struct A8Pcurve {
    /// Record byte offset.
    pub pos: usize,
    /// Inline object identifier.
    pub object_id: u32,
    /// Referenced support-surface object identifier.
    pub support_id: u32,
    /// Parametric curve degree.
    pub degree: u32,
    /// Distinct parameter knots.
    pub knots: Vec<f64>,
    /// Multiplicity for each distinct knot.
    pub multiplicities: Vec<u32>,
    /// Stored UV-jet channel-mode byte.
    pub mode: u8,
    /// UV positions at the knot sites.
    pub points: Vec<[f64; 2]>,
    /// UV first derivatives at the knot sites.
    pub first_derivatives: Vec<[f64; 2]>,
    /// UV second derivatives at the knot sites.
    pub second_derivatives: Vec<[f64; 2]>,
    /// Native parameter range.
    pub range: [f64; 2],
}

/// Degree-5 UV jet stored in an `a5 03 20` consolidated record.
#[derive(Debug, Clone)]
pub struct A5Pcurve {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced support-surface identifier.
    pub support_id: u32,
    /// Parametric curve degree.
    pub degree: u32,
    /// Number of leading extrapolation sites encoded by the array marker.
    pub extrapolation_sites: u32,
    /// Global parameters at the stored sites.
    pub knots: Vec<f64>,
    /// UV positions at the stored sites.
    pub points: Vec<[f64; 2]>,
    /// UV first derivatives at the stored sites.
    pub first_derivatives: Vec<[f64; 2]>,
    /// UV second derivatives at the stored sites.
    pub second_derivatives: Vec<[f64; 2]>,
    /// Native parameter range.
    pub range: [f64; 2],
}

/// Offset-surface constructor stored in a `b2 03 31` support record.
#[derive(Debug, Clone)]
pub struct B2OffsetSupport {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced carrier-surface identifier.
    pub support_id: u32,
    /// Signed normal offset distance in millimetres.
    pub distance: f64,
    /// Carrier UV sub-domain `[u0, v0, u1, v1]`.
    pub domain: [f64; 4],
}

/// Parameter-space data stored in a `b2/b3/b4 03 18` record.
#[derive(Debug, Clone, PartialEq)]
pub enum B2ParameterPoint {
    /// Two-coordinate UV point (`L=0x12`).
    Uv {
        /// Record byte offset.
        pos: usize,
        /// Surface-chart coordinates.
        uv: [f64; 2],
    },
    /// Host-chain station followed by UV (`L=0x1a`).
    StationUv {
        /// Record byte offset.
        pos: usize,
        /// Host-chain axial boundary station.
        station: f64,
        /// Surface-chart coordinates.
        uv: [f64; 2],
    },
    /// Unsplit five-scalar layout (`L=0x2a`).
    FiveScalars {
        /// Record byte offset.
        pos: usize,
        /// Stored scalar payload.
        values: [f64; 5],
    },
}

/// Persistent-tag reference list stored in a `b2/b3/b4 03 37` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2ReferenceList {
    /// Record byte offset.
    pub pos: usize,
    /// Compact persistent-tag references in serialization order.
    pub references: Vec<u32>,
}

/// Nine-reference owner packet stored in a `b2/b3/b4 03 62` record with a
/// 62-byte numeric tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2OwnerPacket {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Encoding selected by the first strong reference token.
    pub reference_encoding: B2OwnerReferenceEncoding,
    /// Nine compact persistent identities following the `0x89` count.
    pub references: [u32; 9],
    /// Fixed-width numeric tail retained byte-exactly.
    pub numeric_tail: [u8; 62],
}

/// Reference dialect used by a nine-reference class-`0x62` owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum B2OwnerReferenceEncoding {
    /// Strong identities use `0x0a <u16le>` and weak identities use compact integers.
    TaggedU16Strong,
    /// Strong identities use width-coded compact integers and weak identities
    /// are raw one-byte values.
    WidthCodedStrong,
}

/// Count-prefixed class-`0x61` reference record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2Counted61 {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Compact values selected by the leading `0x80+n` count.
    pub references: Vec<u32>,
    /// Remaining class-specific bytes, including the terminal `0x03`.
    pub tail: Vec<u8>,
}

/// Long-form class-`0x61` record with a monotone u16 member lane.
#[derive(Debug, Clone, PartialEq)]
pub struct B2Long61 {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Eight opaque bytes preceding the `0x06` list marker.
    pub prefix: [u8; 8],
    /// Strictly increasing little-endian u16 values.
    pub members: Vec<u16>,
    /// Five `0x0a <u16le>` persistent identities after delimiter `0xfe`.
    pub references: [u16; 5],
    /// Finite scalar preceding the terminal byte.
    pub scalar: f64,
}

/// Fixed-shape class-`0x5f` link record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct B2Link5f {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Width-coded persistent target between `0x82` and the `03 05` tail.
    pub target: u32,
}

/// Adjacent class-`0x5f` link and class-`0x62` owner packet joined by their
/// allocation-successor identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2LinkedOwner {
    /// Fixed link immediately preceding the owner packet.
    pub link: B2Link5f,
    /// Nine-reference owner packet.
    pub owner: B2OwnerPacket,
}

/// Cone-face chart descriptor stored in a `b2/b3/b4 03 3b` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2ConeFace {
    /// Record byte offset.
    pub pos: usize,
    /// Compact persistent-tag references.
    pub references: Vec<u32>,
    /// Stored angular chart scale.
    pub angular_scale: f64,
    /// Cone half-angle in radians.
    pub half_angle: f64,
}

/// Settled terminal sense code in a class-`0x06` consolidated use record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum B2UseSense {
    /// Terminal byte `0x84`.
    Sense84,
    /// Terminal byte `0x88`.
    Sense88,
}

/// Byte-level metadata from a class-`0x06` consolidated use record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2UseMetadata {
    /// Record byte offset.
    pub pos: usize,
    /// Complete payload bytes.
    pub payload: Vec<u8>,
    /// Compact persistent references following the `0x80+n` count and
    /// preceding a settled terminal sense. `None` when the payload does not
    /// close under that grammar.
    pub references: Option<Vec<u32>>,
    /// Decoded terminal sense when the payload ends in `0x84` or `0x88`.
    pub sense: Option<B2UseSense>,
}

/// Byte-level metadata from a class-`0x5e` consolidated record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2EdgeMetadata {
    /// Record byte offset.
    pub pos: usize,
    /// Complete payload bytes.
    pub payload: Vec<u8>,
    /// Values carried by each `0x0a <u16le>` reference token.
    pub references: Vec<u16>,
}

/// Structurally decoded width-coded class-`0x5e` edge node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct B2EdgeNode {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token following the payload length.
    pub header_token: u32,
    /// Native curve-support identity.
    pub curve_ref: u32,
    /// Native start-vertex identity.
    pub start_vertex_ref: u32,
    /// Native end-vertex identity.
    pub end_vertex_ref: u32,
    /// Native start-parameter identity.
    pub start_parameter_ref: u32,
    /// Native end-parameter identity.
    pub end_parameter_ref: u32,
    /// Terminal layout byte following the five references.
    pub tail: u8,
}

/// Decode class-`0x06` payloads and their settled terminal sense codes.
#[must_use]
pub fn b2_use_metadata(data: &[u8]) -> Vec<B2UseMetadata> {
    b_family_frames(data, 0x06)
        .into_iter()
        .map(|frame| {
            let payload = data[frame.payload..frame.end].to_vec();
            let sense = match payload.last() {
                Some(0x84) => Some(B2UseSense::Sense84),
                Some(0x88) => Some(B2UseSense::Sense88),
                _ => None,
            };
            let references = sense.and_then(|_| {
                let end = frame.end.checked_sub(1)?;
                let count = usize::from(data.get(frame.payload)?.checked_sub(0x80)?);
                let mut at = frame.payload + 1;
                let mut references = Vec::new();
                for _ in 0..count {
                    references.push(compact_int(data, &mut at)?);
                }
                (at == end).then_some(references)
            });
            B2UseMetadata {
                pos: frame.pos,
                payload,
                references,
                sense,
            }
        })
        .collect()
}

/// Decode class-`0x5e` payloads and their `0x0a <u16le>` reference tokens.
#[must_use]
pub fn b2_edge_metadata(data: &[u8]) -> Vec<B2EdgeMetadata> {
    b_family_frames(data, 0x5e)
        .into_iter()
        .map(|frame| {
            let payload = data[frame.payload..frame.end].to_vec();
            let mut references = Vec::new();
            let mut at = 0;
            while at < payload.len() {
                if payload[at] == 0x0a && at + 3 <= payload.len() {
                    references.push(u16::from_le_bytes([payload[at + 1], payload[at + 2]]));
                    at += 3;
                } else {
                    at += 1;
                }
            }
            B2EdgeMetadata {
                pos: frame.pos,
                payload,
                references,
            }
        })
        .collect()
}

/// Decode length-closed `b2/b3/b4 03 5e` records containing exactly five
/// compact references followed by one terminal byte.
#[must_use]
pub fn b2_edge_nodes(data: &[u8]) -> Vec<B2EdgeNode> {
    b_family_frames(data, 0x5e)
        .into_iter()
        .filter_map(|frame| {
            let mut at = frame.payload;
            let references = (0..5)
                .map(|_| compact_int(data, &mut at))
                .collect::<Option<Vec<_>>>()?;
            let tail = *data.get(at)?;
            (at + 1 == frame.end).then_some(B2EdgeNode {
                pos: frame.pos,
                header_token: frame.header_token,
                curve_ref: references[0],
                start_vertex_ref: references[1],
                end_vertex_ref: references[2],
                start_parameter_ref: references[3],
                end_parameter_ref: references[4],
                tail,
            })
        })
        .collect()
}

/// Decode width-coded `b2/b3/b4 03 3b` cone-face descriptors.
#[must_use]
pub fn b2_cone_faces(data: &[u8]) -> Vec<B2ConeFace> {
    b_family_frames(data, 0x3b)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 || frame.end - frame.payload != 0x20 {
                return None;
            }
            let scalar_at = frame.end - 16;
            let angular_scale = f64_le(data, scalar_at)?;
            let half_angle = f64_le(data, scalar_at + 8)?;
            if !angular_scale.is_finite()
                || !(0.0..std::f64::consts::FRAC_PI_2).contains(&half_angle)
            {
                return None;
            }
            let mut at = frame.payload;
            let mut references = Vec::new();
            while at < scalar_at {
                references.push(compact_int(data, &mut at)?);
            }
            (at == scalar_at).then_some(B2ConeFace {
                pos: frame.pos,
                references,
                angular_scale,
                half_angle,
            })
        })
        .collect()
}

/// Decode `b2/b3/b4 03 37` compact reference lists with their unit tail.
#[must_use]
pub fn b2_reference_lists(data: &[u8]) -> Vec<B2ReferenceList> {
    b_family_frames(data, 0x37)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5
                || !matches!(frame.end - frame.payload, 0x22 | 0x24 | 0x26)
                || f64_le(data, frame.end.checked_sub(8)?)? != 1.0
            {
                return None;
            }
            let refs_end = frame.end - 8;
            let mut at = frame.payload;
            let mut references = Vec::new();
            while at < refs_end {
                references.push(compact_int(data, &mut at)?);
            }
            (at == refs_end).then_some(B2ReferenceList {
                pos: frame.pos,
                references,
            })
        })
        .collect()
}

/// Decode width-coded class-`0x62` owner packets whose counted references and
/// fixed numeric tail consume the complete frame.
#[must_use]
pub fn b2_owner_packets(data: &[u8]) -> Vec<B2OwnerPacket> {
    b_family_frames(data, 0x62)
        .into_iter()
        .filter_map(|frame| {
            if data.get(frame.payload) != Some(&0x89) {
                return None;
            }
            let mut at = frame.payload + 1;
            let reference_encoding = if data.get(at) == Some(&0x0a) {
                B2OwnerReferenceEncoding::TaggedU16Strong
            } else {
                B2OwnerReferenceEncoding::WidthCodedStrong
            };
            let mut references = [0u32; 9];
            for (index, reference) in references.iter_mut().enumerate() {
                *reference = match (reference_encoding, index % 2) {
                    (B2OwnerReferenceEncoding::TaggedU16Strong, 0) => {
                        persistent_ref(data, &mut at)?
                    }
                    (B2OwnerReferenceEncoding::TaggedU16Strong, 1)
                    | (B2OwnerReferenceEncoding::WidthCodedStrong, 0) => {
                        compact_int(data, &mut at)?
                    }
                    (B2OwnerReferenceEncoding::WidthCodedStrong, 1) => {
                        let value = u32::from(*data.get(at)?);
                        at += 1;
                        value
                    }
                    _ => unreachable!(),
                };
            }
            let numeric_tail = data.get(at..frame.end)?.try_into().ok()?;
            Some(B2OwnerPacket {
                pos: frame.pos,
                header_token: frame.header_token,
                reference_encoding,
                references,
                numeric_tail,
            })
        })
        .collect()
}

/// Decode the count-prefixed class-`0x61` payload family. Long class-`0x61`
/// records without a leading count belong to a separate grammar and are not
/// returned.
#[must_use]
pub fn b2_counted_61(data: &[u8]) -> Vec<B2Counted61> {
    b_family_frames(data, 0x61)
        .into_iter()
        .filter_map(|frame| {
            let count = usize::from(data.get(frame.payload)?.checked_sub(0x80)?);
            if count == 0 {
                return None;
            }
            let mut at = frame.payload + 1;
            let references = (0..count)
                .map(|_| compact_int(data, &mut at))
                .collect::<Option<Vec<_>>>()?;
            let tail = data.get(at..frame.end)?;
            if tail.is_empty() || tail.last() != Some(&0x03) {
                return None;
            }
            Some(B2Counted61 {
                pos: frame.pos,
                header_token: frame.header_token,
                references,
                tail: tail.to_vec(),
            })
        })
        .collect()
}

/// Decode the long class-`0x61` form. Its fixed 25-byte suffix determines the
/// monotone member-list boundary without searching for delimiter bytes.
#[must_use]
pub fn b2_long_61(data: &[u8]) -> Vec<B2Long61> {
    b_family_frames(data, 0x61)
        .into_iter()
        .filter_map(|frame| {
            let payload_len = frame.end.checked_sub(frame.payload)?;
            let delimiter = frame.end.checked_sub(25)?;
            if payload_len < 36
                || data.get(frame.payload + 8) != Some(&0x06)
                || data.get(delimiter) != Some(&0xfe)
                || (delimiter - (frame.payload + 9)) % 2 != 0
                || data.get(frame.end - 1) != Some(&0x03)
            {
                return None;
            }
            let prefix = data
                .get(frame.payload..frame.payload + 8)?
                .try_into()
                .ok()?;
            let members = data[frame.payload + 9..delimiter]
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>();
            if members.is_empty() || members.windows(2).any(|pair| pair[0] >= pair[1]) {
                return None;
            }
            let mut at = delimiter + 1;
            let mut references = [0u16; 5];
            for reference in &mut references {
                if data.get(at) != Some(&0x0a) {
                    return None;
                }
                *reference = u16_le(data, at + 1)?;
                at += 3;
            }
            let scalar = f64_le(data, at)?;
            if !scalar.is_finite() || at + 9 != frame.end {
                return None;
            }
            Some(B2Long61 {
                pos: frame.pos,
                header_token: frame.header_token,
                prefix,
                members,
                references,
                scalar,
            })
        })
        .collect()
}

/// Decode fixed `82 <width-coded target> 03 05` class-`0x5f` links.
#[must_use]
pub fn b2_links_5f(data: &[u8]) -> Vec<B2Link5f> {
    b_family_frames(data, 0x5f)
        .into_iter()
        .filter_map(|frame| {
            if frame.end - frame.payload != 6 || data.get(frame.payload) != Some(&0x82) {
                return None;
            }
            let mut at = frame.payload + 1;
            let target = compact_int(data, &mut at)?;
            (at + 2 == frame.end && data.get(at..frame.end) == Some(&[0x03, 0x05])).then_some(
                B2Link5f {
                    pos: frame.pos,
                    header_token: frame.header_token,
                    target,
                },
            )
        })
        .collect()
}

/// Bind immediately adjacent `5f,62` records when the owner's ninth identity
/// is the checked successor of the link target.
#[must_use]
pub fn b2_linked_owners(data: &[u8]) -> Vec<B2LinkedOwner> {
    let links = b2_links_5f(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let owners = b2_owner_packets(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    consolidated_records(data)
        .windows(2)
        .filter_map(|window| {
            let [link_record, owner_record] = window else {
                return None;
            };
            let link = links.get(&link_record.range.start)?;
            let owner = owners.get(&owner_record.range.start)?;
            (link.target.checked_add(1) == Some(owner.references[8])).then(|| B2LinkedOwner {
                link: *link,
                owner: owner.clone(),
            })
        })
        .collect()
}

/// Decode width-coded `b2/b3/b4 03 18` parameter-space records.
#[must_use]
pub fn b2_parameter_points(data: &[u8]) -> Vec<B2ParameterPoint> {
    b_family_frames(data, 0x18)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 || data.get(frame.payload) != Some(&0x05) {
                return None;
            }
            let at = frame.payload + 2;
            match frame.end - frame.payload {
                0x12 => Some(B2ParameterPoint::Uv {
                    pos: frame.pos,
                    uv: read_f64_array::<2>(data, at)?,
                }),
                0x1a => {
                    let values = read_f64_array::<3>(data, at)?;
                    Some(B2ParameterPoint::StationUv {
                        pos: frame.pos,
                        station: values[0],
                        uv: [values[1], values[2]],
                    })
                }
                0x2a => Some(B2ParameterPoint::FiveScalars {
                    pos: frame.pos,
                    values: read_f64_array::<5>(data, at)?,
                }),
                _ => None,
            }
            .filter(|value| match value {
                B2ParameterPoint::Uv { uv, .. } => uv.iter().all(|v| v.is_finite()),
                B2ParameterPoint::StationUv { station, uv, .. } => {
                    station.is_finite() && uv.iter().all(|v| v.is_finite())
                }
                B2ParameterPoint::FiveScalars { values, .. } => {
                    values.iter().all(|v| v.is_finite())
                }
            })
        })
        .collect()
}

/// Shared-edge parameter range stored in a `b2 03 23` packet.
#[derive(Debug, Clone)]
pub struct B2EdgeParameters {
    /// Record byte offset.
    pub pos: usize,
    /// Native shared-edge parameter range.
    pub range: [f64; 2],
    /// Shared-edge geometric tolerance.
    pub tolerance: f64,
}

/// Serialized consolidated edge block formed by two pcurves and one range packet.
#[derive(Debug, Clone)]
pub struct A5EdgeBlock {
    /// The two face-side UV definitions in serialization order.
    pub pcurves: [A5Pcurve; 2],
    /// Shared parameter range and tolerance packet.
    pub parameters: B2EdgeParameters,
    /// Both pcurves and the edge packet store the same native range and site count.
    pub co_parametric: bool,
}

/// Complete consolidated edge run serialized as two side pcurves, their shared
/// parameter packet, two oriented uses, and one native edge node.
#[derive(Debug, Clone)]
pub struct A5TopologyEdgeRun {
    /// Co-parametric side definitions and shared range packet.
    pub edge: A5EdgeBlock,
    /// The two serialized edge uses, in side order.
    pub uses: [B2UseMetadata; 2],
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub node: B2EdgeNode,
    /// Whether the two counted use-reference vectors reproduce the edge
    /// node's endpoint-parameter/curve identity chain.
    pub identity_chain_consistent: bool,
}

/// Native endpoint-incidence graph of complete consolidated edge runs.
#[derive(Debug, Clone)]
pub struct A5NativeEdgeGraph {
    /// Persistent native vertex identities in first-incidence order.
    pub vertex_identities: Vec<u32>,
    /// Edge runs in serialization order, with endpoints indexing
    /// `vertex_identities`.
    pub edges: Vec<A5NativeGraphEdge>,
    /// Connected edge components, expressed as edge ordinals.
    pub components: Vec<Vec<usize>>,
}

/// One edge in a consolidated native endpoint-incidence graph.
#[derive(Debug, Clone)]
pub struct A5NativeGraphEdge {
    /// Complete serialized edge run.
    pub run: A5TopologyEdgeRun,
    /// Compact endpoint indices into [`A5NativeEdgeGraph::vertex_identities`].
    pub vertices: [usize; 2],
}

/// Uniquely resolved carrier for one side of a consolidated edge block.
#[derive(Debug, Clone, PartialEq)]
pub enum A5SupportBinding {
    /// Standalone `b2 03 28` cylinder record.
    Cylinder {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// Cylinder frame embedded in a `b2 03 60` wrapper.
    EmbeddedCylinder {
        /// Embedded frame byte offset.
        pos: usize,
        /// Enclosing wrapper byte offset.
        wrapper_pos: usize,
    },
    /// `b2 03 19` circle selected by constant-V and exact arc range.
    Circle {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// `b2 03 29` cone selected by endpoint lifts.
    Cone {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// Consolidated `a5 03 34` NURBS carrier, optionally at a constant normal offset.
    NurbsCarrier {
        /// Carrier record byte offset.
        pos: usize,
        /// Signed normal offset from the stored carrier to the shared 3D edge.
        offset: f64,
    },
}

/// Consolidated edge block with uniquely resolved side carriers.
#[derive(Debug, Clone)]
pub struct ResolvedA5EdgeBlock {
    /// Parsed pcurve pair and shared edge packet.
    pub block: A5EdgeBlock,
    /// Carrier binding for each pcurve side.
    pub supports: [Option<A5SupportBinding>; 2],
    /// Shared lifted 3D definition sites when every liftable side agrees
    /// pointwise in the common edge parameterization.
    pub shared_loci: Option<Vec<Point3>>,
    /// Unordered 3D endpoint loci when at least one uniquely bound side can be
    /// lifted and every liftable side agrees.
    pub endpoint_loci: Option<[Point3; 2]>,
}

/// Group ordered `(a5 03 20, a5 03 20, b2 03 23)` consolidated edge blocks.
#[must_use]
pub fn a5_edge_blocks(data: &[u8]) -> Vec<A5EdgeBlock> {
    let pcurves = a5_pcurves(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let parameters = b2_edge_parameters(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    consolidated_records(data)
        .windows(3)
        .filter_map(|window| {
            let [first_record, second_record, parameter_record] = window else {
                return None;
            };
            if first_record.family == ConsolidatedFamily::A
                && first_record.class == 0x20
                && second_record.family == ConsolidatedFamily::A
                && second_record.class == 0x20
                && parameter_record.family == ConsolidatedFamily::B
                && parameter_record.class == 0x23
            {
                let first = pcurves.get(&first_record.range.start)?;
                let second = pcurves.get(&second_record.range.start)?;
                let parameters = parameters.get(&parameter_record.range.start)?;
                let co_parametric = first.points.len() == second.points.len()
                    && first.range == second.range
                    && first.range == parameters.range;
                Some(A5EdgeBlock {
                    pcurves: [first.clone(), second.clone()],
                    parameters: parameters.clone(),
                    co_parametric,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Decode complete six-record consolidated edge runs. Records separated by any
/// other framed record do not form a run.
#[must_use]
pub fn a5_topology_edge_runs(data: &[u8]) -> Vec<A5TopologyEdgeRun> {
    let edges = a5_edge_blocks(data)
        .into_iter()
        .map(|edge| (edge.pcurves[0].pos, edge))
        .collect::<BTreeMap<_, _>>();
    let uses = b2_use_metadata(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let nodes = b2_edge_nodes(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    consolidated_records(data)
        .windows(6)
        .filter_map(|window| {
            let [pcurve0, pcurve1, parameters, use0, use1, node] = window else {
                return None;
            };
            if pcurve0.family == ConsolidatedFamily::A
                && pcurve0.class == 0x20
                && pcurve1.family == ConsolidatedFamily::A
                && pcurve1.class == 0x20
                && parameters.family == ConsolidatedFamily::B
                && parameters.class == 0x23
                && use0.family == ConsolidatedFamily::B
                && use0.class == 0x06
                && use1.family == ConsolidatedFamily::B
                && use1.class == 0x06
                && node.family == ConsolidatedFamily::B
                && node.class == 0x5e
            {
                let node = *nodes.get(&node.range.start)?;
                let uses = [
                    uses.get(&use0.range.start)?.clone(),
                    uses.get(&use1.range.start)?.clone(),
                ];
                let identity_chain_consistent = uses[0].references.as_deref()
                    == Some(&[node.end_parameter_ref, node.start_parameter_ref])
                    && uses[1].references.as_deref()
                        == Some(&[node.start_parameter_ref, node.curve_ref]);
                Some(A5TopologyEdgeRun {
                    edge: edges.get(&pcurve0.range.start)?.clone(),
                    uses,
                    node,
                    identity_chain_consistent,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Build the native endpoint-incidence graph for all complete consolidated
/// edge runs. A broken use/edge identity chain or duplicate curve identity
/// invalidates the graph rather than silently accepting contradictory runs.
#[must_use]
pub fn a5_native_edge_graph(data: &[u8]) -> Option<A5NativeEdgeGraph> {
    let runs = a5_topology_edge_runs(data);
    if runs.is_empty() {
        return None;
    }
    let mut curve_identities = std::collections::HashSet::new();
    let mut vertex_indices = HashMap::new();
    let mut vertex_identities = Vec::new();
    let mut edges = Vec::with_capacity(runs.len());
    for run in runs {
        if !run.identity_chain_consistent || !curve_identities.insert(run.node.curve_ref) {
            return None;
        }
        let vertices = [run.node.start_vertex_ref, run.node.end_vertex_ref].map(|identity| {
            *vertex_indices.entry(identity).or_insert_with(|| {
                let index = vertex_identities.len();
                vertex_identities.push(identity);
                index
            })
        });
        edges.push(A5NativeGraphEdge { run, vertices });
    }
    let mut vertex_edges = vec![Vec::new(); vertex_identities.len()];
    for (edge, value) in edges.iter().enumerate() {
        for vertex in value.vertices {
            vertex_edges[vertex].push(edge);
        }
    }
    let mut unseen = (0..edges.len()).collect::<std::collections::BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(&first) = unseen.first() {
        let mut component = Vec::new();
        let mut stack = vec![first];
        unseen.remove(&first);
        while let Some(edge) = stack.pop() {
            component.push(edge);
            for vertex in edges[edge].vertices {
                for &neighbor in &vertex_edges[vertex] {
                    if unseen.remove(&neighbor) {
                        stack.push(neighbor);
                    }
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    Some(A5NativeEdgeGraph {
        vertex_identities,
        edges,
        components,
    })
}

/// Resolve consolidated edge sides against B2 cylinder charts by endpoint lifts.
///
/// A carrier wins only when it is the sole chart whose two lifted pcurve endpoints
/// coincide with serialized `05 08 01` vertices at single-precision tolerance.
#[must_use]
pub fn resolve_a5_edge_blocks(data: &[u8]) -> Vec<ResolvedA5EdgeBlock> {
    let points = vertices(data);
    let standalone = b2_cylinders(data);
    let embedded = b2_embedded_cylinders(data);
    let circles = b2_circles(data);
    let cones = b2_cones(data);
    let surfaces = a5_surfaces(data);
    a5_edge_blocks(data)
        .into_iter()
        .map(|block| {
            let mut supports = std::array::from_fn(|side| {
                let pcurve = &block.pcurves[side];
                let mut winners = Vec::new();
                for cylinder in &standalone {
                    if cylinder.geometry.is_some()
                        && pcurve_endpoints_match_vertices(pcurve, cylinder, &points)
                    {
                        winners.push(A5SupportBinding::Cylinder { pos: cylinder.pos });
                    }
                }
                for value in &embedded {
                    if pcurve_endpoints_match_vertices(pcurve, &value.cylinder, &points) {
                        winners.push(A5SupportBinding::EmbeddedCylinder {
                            pos: value.pos,
                            wrapper_pos: value.wrapper_pos,
                        });
                    }
                }
                if winners.is_empty() {
                    let mut circle_winners: Vec<_> = circles
                        .iter()
                        .filter(|circle| pcurve_matches_circle(pcurve, circle))
                        .map(|circle| A5SupportBinding::Circle { pos: circle.pos })
                        .collect();
                    if circle_winners.len() == 1 {
                        winners.append(&mut circle_winners);
                    }
                }
                if winners.is_empty() {
                    let mut cone_winners: Vec<_> = cones
                        .iter()
                        .filter(|cone| pcurve_endpoints_match_cone(pcurve, cone, &points))
                        .map(|cone| A5SupportBinding::Cone { pos: cone.pos })
                        .collect();
                    if cone_winners.len() == 1 {
                        winners.append(&mut cone_winners);
                    }
                }
                (winners.len() == 1).then(|| winners.remove(0))
            });
            for anchor_side in [0, 1] {
                let partner = 1 - anchor_side;
                if supports[partner].is_some() {
                    continue;
                }
                let Some(anchor_points) = supports[anchor_side].as_ref().and_then(|binding| {
                    support_points(
                        binding,
                        &block.pcurves[anchor_side],
                        &standalone,
                        &embedded,
                        &cones,
                        &surfaces,
                    )
                }) else {
                    continue;
                };
                let winners: Vec<_> = surfaces
                    .iter()
                    .filter_map(|surface| {
                        nurbs_carrier_offset(
                            &surface.geometry,
                            &block.pcurves[partner].points,
                            &anchor_points,
                        )
                        .map(|offset| A5SupportBinding::NurbsCarrier {
                            pos: surface.pos,
                            offset,
                        })
                    })
                    .collect();
                if let [winner] = winners.as_slice() {
                    supports[partner] = Some(winner.clone());
                }
            }
            let shared_loci =
                resolved_support_loci(&block, &supports, &standalone, &embedded, &cones, &surfaces);
            let endpoint_loci = shared_loci
                .as_ref()
                .and_then(|points| Some([*points.first()?, *points.last()?]));
            ResolvedA5EdgeBlock {
                block,
                supports,
                shared_loci,
                endpoint_loci,
            }
        })
        .collect()
}

fn resolved_support_loci(
    block: &A5EdgeBlock,
    supports: &[Option<A5SupportBinding>; 2],
    cylinders: &[B2Cylinder],
    embedded: &[B2EmbeddedCylinder],
    cones: &[B2Cone],
    surfaces: &[A8Surface],
) -> Option<Vec<Point3>> {
    let candidates = supports
        .iter()
        .zip(&block.pcurves)
        .filter_map(|(binding, pcurve)| {
            let points = support_points(
                binding.as_ref()?,
                pcurve,
                cylinders,
                embedded,
                cones,
                surfaces,
            )?;
            (!points.is_empty()).then_some(points)
        })
        .collect::<Vec<_>>();
    let first = candidates.first()?;
    candidates
        .iter()
        .all(|candidate| {
            candidate.len() == first.len()
                && first
                    .iter()
                    .zip(candidate)
                    .all(|(&left, &right)| point_distance(left, right) <= 2e-3)
        })
        .then(|| first.clone())
}

fn support_points(
    binding: &A5SupportBinding,
    pcurve: &A5Pcurve,
    cylinders: &[B2Cylinder],
    embedded: &[B2EmbeddedCylinder],
    cones: &[B2Cone],
    surfaces: &[A8Surface],
) -> Option<Vec<Point3>> {
    match binding {
        A5SupportBinding::Cylinder { pos } => {
            let carrier = cylinders.iter().find(|value| value.pos == *pos)?;
            pcurve
                .points
                .iter()
                .map(|uv| b2_cylinder_point(carrier, *uv))
                .collect()
        }
        A5SupportBinding::EmbeddedCylinder { pos, .. } => {
            let carrier = &embedded.iter().find(|value| value.pos == *pos)?.cylinder;
            pcurve
                .points
                .iter()
                .map(|uv| b2_cylinder_point(carrier, *uv))
                .collect()
        }
        A5SupportBinding::Cone { pos } => {
            let carrier = cones.iter().find(|value| value.pos == *pos)?;
            pcurve
                .points
                .iter()
                .map(|uv| b2_cone_point(carrier, *uv))
                .collect()
        }
        A5SupportBinding::NurbsCarrier { pos, offset } => {
            let SurfaceGeometry::Nurbs(surface) = &surfaces
                .iter()
                .find(|surface| surface.pos == *pos)?
                .geometry
            else {
                return None;
            };
            pcurve
                .points
                .iter()
                .map(|&[u, v]| {
                    let partials = nurbs_surface_partials(surface, u, v)?;
                    let normal = unit(cross(partials.du, partials.dv))?;
                    Some(Point3::new(
                        partials.point.x + offset * normal.x,
                        partials.point.y + offset * normal.y,
                        partials.point.z + offset * normal.z,
                    ))
                })
                .collect()
        }
        A5SupportBinding::Circle { .. } => None,
    }
}

fn nurbs_carrier_offset(
    geometry: &SurfaceGeometry,
    parameters: &[[f64; 2]],
    anchors: &[Point3],
) -> Option<f64> {
    let SurfaceGeometry::Nurbs(surface) = geometry else {
        return None;
    };
    if parameters.len() != anchors.len() || parameters.is_empty() {
        return None;
    }
    let mut offsets = Vec::with_capacity(parameters.len());
    for (&[u, v], &anchor) in parameters.iter().zip(anchors) {
        let partials = nurbs_surface_partials(surface, u, v)?;
        let point = partials.point;
        let residual = Vector3::new(anchor.x - point.x, anchor.y - point.y, anchor.z - point.z);
        let residual_length = (residual.x.powi(2) + residual.y.powi(2) + residual.z.powi(2)).sqrt();
        if residual_length < 1e-6 {
            offsets.push(0.0);
            continue;
        }
        let normal = unit(cross(partials.du, partials.dv))?;
        let distance = residual.x * normal.x + residual.y * normal.y + residual.z * normal.z;
        let perpendicular_squared = residual_length.powi(2) - distance.powi(2);
        if perpendicular_squared > 1e-12 {
            return None;
        }
        offsets.push(distance);
    }
    let first = offsets[0];
    if offsets.iter().any(|value| (value - first).abs() > 1e-6)
        || !(first.abs() < 1e-6 || (first.abs() - 2.0).abs() < 1e-6)
    {
        return None;
    }
    Some(if first.abs() < 1e-6 { 0.0 } else { first })
}

fn pcurve_matches_circle(pcurve: &A5Pcurve, circle: &B2Circle) -> bool {
    let (Some(first), Some(last)) = (pcurve.points.first(), pcurve.points.last()) else {
        return false;
    };
    (first[1] - last[1]).abs() <= 1e-6
        && (first[0].min(last[0]) - circle.range[0]).abs() < 1e-9
        && (first[0].max(last[0]) - circle.range[1]).abs() < 1e-9
}

fn pcurve_endpoints_match_cone(pcurve: &A5Pcurve, cone: &B2Cone, vertices: &[Point3]) -> bool {
    let (Some(first), Some(last)) = (pcurve.points.first(), pcurve.points.last()) else {
        return false;
    };
    [*first, *last].into_iter().all(|uv| {
        b2_cone_point(cone, uv).is_some_and(|point| {
            vertices
                .iter()
                .any(|vertex| point_distance(point, *vertex) < 2e-3)
        })
    })
}

fn b2_cone_point(cone: &B2Cone, uv: [f64; 2]) -> Option<Point3> {
    if !(cone.slant_range[0] - 1e-6..=cone.slant_range[1] + 1e-6).contains(&uv[1]) {
        return None;
    }
    let phi = uv[0] / cone.angular_scale;
    let radial = [
        phi.cos() * cone.t1[0] + phi.sin() * cone.t2[0],
        phi.cos() * cone.t1[1] + phi.sin() * cone.t2[1],
        phi.cos() * cone.t1[2] + phi.sin() * cone.t2[2],
    ];
    let axial = cone.half_angle.cos();
    let transverse = cone.half_angle.sin();
    Some(Point3::new(
        cone.apex[0] + uv[1] * (axial * cone.axis[0] + transverse * radial[0]),
        cone.apex[1] + uv[1] * (axial * cone.axis[1] + transverse * radial[1]),
        cone.apex[2] + uv[1] * (axial * cone.axis[2] + transverse * radial[2]),
    ))
}

fn pcurve_endpoints_match_vertices(
    pcurve: &A5Pcurve,
    cylinder: &B2Cylinder,
    vertices: &[Point3],
) -> bool {
    let Some(first) = pcurve
        .points
        .first()
        .and_then(|uv| b2_cylinder_point(cylinder, *uv))
    else {
        return false;
    };
    let Some(last) = pcurve
        .points
        .last()
        .and_then(|uv| b2_cylinder_point(cylinder, *uv))
    else {
        return false;
    };
    [first, last].iter().all(|point| {
        vertices
            .iter()
            .any(|vertex| point_distance(*point, *vertex) < 2e-3)
    })
}

fn b2_cylinder_point(cylinder: &B2Cylinder, uv: [f64; 2]) -> Option<Point3> {
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = cylinder.geometry.as_ref()?
    else {
        return None;
    };
    if !(cylinder.u_range[0] - 1e-6..=cylinder.u_range[1] + 1e-6).contains(&uv[0])
        || !(cylinder.v_range[0] - 1e-6..=cylinder.v_range[1] + 1e-6).contains(&uv[1])
    {
        return None;
    }
    let angle = uv[0] / radius;
    let perpendicular = cross(*axis, *ref_direction);
    Some(Point3::new(
        origin.x
            + uv[1] * axis.x
            + radius * (angle.cos() * ref_direction.x + angle.sin() * perpendicular.x),
        origin.y
            + uv[1] * axis.y
            + radius * (angle.cos() * ref_direction.y + angle.sin() * perpendicular.y),
        origin.z
            + uv[1] * axis.z
            + radius * (angle.cos() * ref_direction.z + angle.sin() * perpendicular.z),
    ))
}

fn point_distance(a: Point3, b: Point3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// Arc-length circle support stored in a `b2 03 19` record.
#[derive(Debug, Clone)]
pub struct B2Circle {
    /// Record byte offset.
    pub pos: usize,
    /// Compact persistent record identifier.
    pub record_id: u32,
    /// Frame token following the record length.
    pub frame_token: u8,
    /// Two center coordinates in the host-implied carrier plane.
    pub center_pair: [f64; 2],
    /// Circle radius in millimetres.
    pub radius: f64,
    /// Arc-length parameter interval.
    pub range: [f64; 2],
    /// Whether the interval spans one complete circumference.
    pub full_circle: bool,
}

/// Analytic cylinder support stored in a `b2 03 28` record.
#[derive(Debug, Clone)]
pub struct B2Cylinder {
    /// Record byte offset.
    pub pos: usize,
    /// Payload-layout discriminator (`0x52`, `0x5a`, or `0x62`).
    pub layout: u8,
    /// Stored frame token.
    pub frame_token: u8,
    /// Decoded carrier; absent for the unresolved phase-tailed `0x62` frame.
    pub geometry: Option<SurfaceGeometry>,
    /// Arc-length circumferential range.
    pub u_range: [f64; 2],
    /// Axial range.
    pub v_range: [f64; 2],
    /// Stored planar vector for a phase-tailed `0x62` frame.
    pub stored_vector: Option<[f64; 2]>,
    /// Phase scalar for a phase-tailed `0x62` frame.
    pub phase: Option<f64>,
}

/// Slant-coordinate cone chart stored in a `b2 03 29` record.
#[derive(Debug, Clone)]
pub struct B2Cone {
    /// Record byte offset.
    pub pos: usize,
    /// Cone apex.
    pub apex: [f64; 3],
    /// First transverse unit direction.
    pub t1: [f64; 3],
    /// Second transverse unit direction.
    pub t2: [f64; 3],
    /// Cone-axis unit direction.
    pub axis: [f64; 3],
    /// Cone half-angle in radians.
    pub half_angle: f64,
    /// Stored angular-origin offset.
    pub angular_offset: f64,
    /// Native slant-coordinate range.
    pub slant_range: [f64; 2],
    /// Divisor mapping the stored U coordinate to azimuth.
    pub angular_scale: f64,
}

/// Axis-and-profile surface of revolution stored in a `b2 03 2d` record.
#[derive(Debug, Clone)]
pub struct B2Revolution {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced profile-curve identifier.
    pub profile_curve_id: u16,
    /// Axis-frame origin.
    pub origin: [f64; 3],
    /// First transverse basis direction.
    pub basis_u: [f64; 3],
    /// Second transverse basis direction.
    pub basis_v: [f64; 3],
    /// Revolution-axis direction.
    pub axis: [f64; 3],
    /// Stored angular parameter interval.
    pub angular_range: [f64; 2],
    /// Stored profile parameter interval.
    pub profile_range: [f64; 2],
    /// Divisor mapping the stored angular parameter to radians.
    pub angular_scale: f64,
    /// Stored mean angular parameter.
    pub mean_angle_parameter: f64,
}

/// Constant `b2 03 65` separator preceding a typed group opener.
#[derive(Debug, Clone)]
pub struct B2GroupSeparator {
    /// Record byte offset.
    pub pos: usize,
    /// Consolidated-frame header token.
    pub token: u32,
}

/// Typed group opener stored in a `b2 03 60` record.
#[derive(Debug, Clone)]
pub struct B2Group {
    /// Record byte offset.
    pub pos: usize,
    /// Compact group identifier.
    pub group_id: u32,
    /// Compact group-type code; type `3` opens a cylinder chain.
    pub group_type: u32,
}

/// Construction-use wrapper stored in a `b2 03 30` record.
#[derive(Debug, Clone)]
pub struct B2ConstructionUse {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced support identifier.
    pub support_id: u32,
    /// Signed wall or offset scalar.
    pub distance: f64,
    /// Construction-type discriminant.
    pub kind: u8,
    /// Four kind-specific stored scalars.
    pub fields: [f64; 4],
    /// Carrier domain `[u0, v0, u1, v1]` for kind `0x01`.
    pub domain: Option<[f64; 4]>,
    /// Active parameter interval for kind `0x19`.
    pub active_interval: Option<[f64; 2]>,
    /// Effective rolling-ball radius for kind `0x19`.
    pub effective_radius: Option<f64>,
}

/// Cylinder frame following a type-3 `b2 03 60` group opener.
#[derive(Debug, Clone)]
pub struct B2EmbeddedCylinder {
    /// Group-opener byte offset.
    pub wrapper_pos: usize,
    /// Embedded frame byte offset, including its varying pre-byte.
    pub pos: usize,
    /// Compact embedded object identifier.
    pub object_id: u32,
    /// Decoded `0x5a` cylinder frame.
    pub cylinder: B2Cylinder,
}

/// Decode `0x5a` cylinder frames following type-3 `b2 03 60` group openers.
#[must_use]
pub fn b2_embedded_cylinders(data: &[u8]) -> Vec<B2EmbeddedCylinder> {
    let groups = b2_groups(data);
    let mut out = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        if group.group_type != 3 {
            continue;
        }
        let wrapper_pos = group.pos;
        let end = groups
            .get(index + 1)
            .map_or(data.len(), |next| next.pos)
            .min(wrapper_pos.saturating_add(2500));
        let mut search = wrapper_pos + 3;
        while search + 3 <= end {
            let Some(relative) = data[search..end]
                .windows(3)
                .position(|bytes| bytes == [0x03, 0x28, 0x5a])
            else {
                break;
            };
            let marker = search + relative;
            search = marker + 3;
            let mut payload = marker + 3;
            let Some(object_id) = compact_int(data, &mut payload) else {
                continue;
            };
            let Some(payload_end) = payload.checked_add(90) else {
                continue;
            };
            if payload_end > end {
                continue;
            }
            let mut standalone = vec![0xb2, 0x03, 0x28, 0x5a, 0];
            standalone.extend_from_slice(&data[payload..payload_end]);
            let Some(mut cylinder) = parse_b2_cylinder(
                &standalone,
                ConsolidatedFrame {
                    pos: 0,
                    payload: 5,
                    end: 95,
                    header_token: 0,
                },
            ) else {
                continue;
            };
            cylinder.pos = marker - 1;
            out.push(B2EmbeddedCylinder {
                wrapper_pos,
                pos: marker - 1,
                object_id,
                cylinder,
            });
        }
    }
    out
}

/// Decode `b2 03 30` construction-use wrappers.
#[must_use]
pub fn b2_construction_uses(data: &[u8]) -> Vec<B2ConstructionUse> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x30) {
        let pos = frame.pos;
        let payload = frame.payload;
        if frame.header_token != 5 || data.get(payload) != Some(&0x05) {
            continue;
        }
        let (support_id, at) = match data.get(payload + 1) {
            Some(0x08) => {
                let Some(value) = u16_le(data, payload + 2) else {
                    continue;
                };
                (u32::from(value), payload + 4)
            }
            Some(0x0c) => {
                let Some(value) = u32_le_24(data, payload + 2) else {
                    continue;
                };
                (value, payload + 5)
            }
            _ => continue,
        };
        let Some(distance) = f64_le(data, at) else {
            continue;
        };
        let Some(&kind) = data.get(at + 8) else {
            continue;
        };
        let Some(fields) = read_f64_array::<4>(data, at + 9) else {
            continue;
        };
        if !distance.is_finite() || fields.iter().any(|v| !v.is_finite()) {
            continue;
        }
        out.push(B2ConstructionUse {
            pos,
            support_id,
            distance,
            kind,
            fields,
            domain: (kind == 0x01).then_some([fields[0], fields[2], fields[1], fields[3]]),
            active_interval: (kind == 0x19).then_some([fields[0], fields[1]]),
            effective_radius: (kind == 0x19).then_some(fields[3]),
        });
    }
    out
}

/// Decode `b2 03 29` analytic cone charts.
#[must_use]
pub fn b2_cones(data: &[u8]) -> Vec<B2Cone> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x29) {
        let pos = frame.pos;
        let p = frame.payload;
        if frame.end - p != 0xb8 || p + 153 > frame.end {
            continue;
        }
        let Some(apex) = read_f64_array::<3>(data, p) else {
            continue;
        };
        let Some(t1) = read_f64_array::<3>(data, p + 24) else {
            continue;
        };
        let Some(t2) = read_f64_array::<3>(data, p + 48) else {
            continue;
        };
        let Some(axis) = read_f64_array::<3>(data, p + 72) else {
            continue;
        };
        let Some(half_angle) = f64_le(data, p + 96) else {
            continue;
        };
        let Some(angular_offset) = f64_le(data, p + 120) else {
            continue;
        };
        let Some(slant_range) = read_f64_array::<2>(data, p + 128) else {
            continue;
        };
        let Some(angular_scale) = f64_le(data, p + 144) else {
            continue;
        };
        let unit = |v: [f64; 3]| ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) - 1.0).abs() < 1e-9;
        if unit(t1)
            && unit(t2)
            && unit(axis)
            && (0.0..std::f64::consts::FRAC_PI_2).contains(&half_angle)
            && (0.0..1e6).contains(&angular_scale)
            && 0.0 < slant_range[0]
            && slant_range[0] < slant_range[1]
            && slant_range[1] < 1e6
            && apex
                .iter()
                .chain(&[angular_offset])
                .all(|value| value.is_finite())
        {
            out.push(B2Cone {
                pos,
                apex,
                t1,
                t2,
                axis,
                half_angle,
                angular_offset,
                slant_range,
                angular_scale,
            });
        }
    }
    out
}

/// Decode `b2 03 2d` axis-and-profile surfaces of revolution.
#[must_use]
pub fn b2_revolutions(data: &[u8]) -> Vec<B2Revolution> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x2d) {
        let p = frame.payload;
        if frame.end - p != 0xae
            || !matches!(data.get(p), Some(0x08 | 0x0a))
            || data.get(p + 131..p + 133) != Some(&[0x05, 0x05])
            || f64_le(data, p + 141) != Some(1.0)
            || f64_le(data, p + 149) != Some(1.0)
            || f64_le(data, p + 157) != Some(0.0)
            || data.get(p + 165) != Some(&0x01)
        {
            continue;
        }
        let Some(profile_curve_id) = u16_le(data, p + 1) else {
            continue;
        };
        let Some(axis_frame) = read_f64_array::<12>(data, p + 3) else {
            continue;
        };
        let Some(bounds) = read_f64_array::<4>(data, p + 99) else {
            continue;
        };
        let Some(angular_scale) = f64_le(data, p + 133) else {
            continue;
        };
        let Some(mean_angle_parameter) = f64_le(data, p + 166) else {
            continue;
        };
        if axis_frame
            .iter()
            .chain(&bounds)
            .chain(&[angular_scale, mean_angle_parameter])
            .any(|value| !value.is_finite())
            || angular_scale <= 0.0
            || bounds[0] / angular_scale != 0.5
            || (bounds[1] - bounds[0]) / angular_scale != std::f64::consts::TAU
            || mean_angle_parameter / angular_scale != std::f64::consts::PI + 0.5
        {
            continue;
        }
        out.push(B2Revolution {
            pos: frame.pos,
            profile_curve_id,
            origin: axis_frame[0..3].try_into().expect("three origin values"),
            basis_u: axis_frame[3..6].try_into().expect("three basis values"),
            basis_v: axis_frame[6..9].try_into().expect("three basis values"),
            axis: axis_frame[9..12].try_into().expect("three axis values"),
            angular_range: [bounds[0], bounds[1]],
            profile_range: [bounds[2], bounds[3]],
            angular_scale,
            mean_angle_parameter,
        });
    }
    out
}

/// Decode constant `b2 03 65` group separators.
#[must_use]
pub fn b2_group_separators(data: &[u8]) -> Vec<B2GroupSeparator> {
    b_family_frames(data, 0x65)
        .into_iter()
        .filter(|frame| data.get(frame.payload..frame.end) == Some(&[0x81, 0x03, 0x05, 0x0d]))
        .map(|frame| B2GroupSeparator {
            pos: frame.pos,
            token: frame.header_token,
        })
        .collect()
}

/// Decode `b2 03 60` typed group openers.
#[must_use]
pub fn b2_groups(data: &[u8]) -> Vec<B2Group> {
    b_family_frames(data, 0x60)
        .into_iter()
        .filter_map(|frame| {
            let mut at = frame.payload;
            let group_id = compact_int(data, &mut at)?;
            let group_type = compact_int(data, &mut at)?;
            (at == frame.end).then_some(B2Group {
                pos: frame.pos,
                group_id,
                group_type,
            })
        })
        .collect()
}

/// Convert a decoded B2 slant-coordinate cone chart to its equivalent IR carrier.
#[must_use]
pub fn b2_cone_geometry(cone: &B2Cone) -> SurfaceGeometry {
    let slant = cone.slant_range[0];
    let axial = slant * cone.half_angle.cos();
    SurfaceGeometry::Cone {
        origin: Point3::new(
            cone.apex[0] + axial * cone.axis[0],
            cone.apex[1] + axial * cone.axis[1],
            cone.apex[2] + axial * cone.axis[2],
        ),
        axis: Vector3::new(cone.axis[0], cone.axis[1], cone.axis[2]),
        ref_direction: Vector3::new(cone.t1[0], cone.t1[1], cone.t1[2]),
        radius: slant * cone.half_angle.sin(),
        ratio: 1.0,
        half_angle: cone.half_angle,
    }
}

/// Decode standalone `b2 03 28` analytic cylinder supports.
#[must_use]
pub fn b2_cylinders(data: &[u8]) -> Vec<B2Cylinder> {
    b_family_frames(data, 0x28)
        .into_iter()
        .filter_map(|frame| parse_b2_cylinder(data, frame))
        .collect()
}

fn parse_b2_cylinder(data: &[u8], frame: ConsolidatedFrame) -> Option<B2Cylinder> {
    let pos = frame.pos;
    let layout = u8::try_from(frame.end.checked_sub(frame.payload)?).ok()?;
    let p = frame.payload;
    let origin_values = read_f64_array::<3>(data, p)?;
    let origin = Point3::new(origin_values[0], origin_values[1], origin_values[2]);
    let frame_token = *data.get(p + 24)?;
    match layout {
        0x5a => {
            if data.get(p + 89) != Some(&0x07) {
                return None;
            }
            let vector = read_f64_array::<2>(data, p + 25)?;
            let one = f64_le(data, p + 41)?;
            let radius = f64_le(data, p + 49)?;
            let u_range = read_f64_array::<2>(data, p + 57)?;
            let v_range = read_f64_array::<2>(data, p + 73)?;
            if one != 1.0
                || !(0.0..1e6).contains(&radius)
                || (vector[0].hypot(vector[1]) - 1.0).abs() > 1e-9
                || ((u_range[1] - u_range[0]) - 2.0 * std::f64::consts::PI * radius).abs() > 1e-6
            {
                return None;
            }
            let axis = match frame_token {
                0x19 => Vector3::new(vector[0], vector[1], 0.0),
                0x1c => Vector3::new(vector[1], -vector[0], 0.0),
                _ => return None,
            };
            let ref_direction = Vector3::new(-axis.y, axis.x, 0.0);
            Some(B2Cylinder {
                pos,
                layout,
                frame_token,
                geometry: Some(SurfaceGeometry::Cylinder {
                    origin,
                    axis,
                    ref_direction,
                    radius,
                }),
                u_range,
                v_range,
                stored_vector: None,
                phase: None,
            })
        }
        0x52 => {
            if frame_token != 0x1d
                || f64_le(data, p + 25)? != 1.0
                || f64_le(data, p + 33)? != 1.0
                || data.get(p + 81) != Some(&0x07)
            {
                return None;
            }
            let radius = f64_le(data, p + 41)?;
            let u_range = read_f64_array::<2>(data, p + 49)?;
            let v_range = read_f64_array::<2>(data, p + 65)?;
            if !(0.0..1e6).contains(&radius)
                || ((u_range[1] - u_range[0]) - 2.0 * std::f64::consts::PI * radius).abs() > 1e-6
            {
                return None;
            }
            Some(B2Cylinder {
                pos,
                layout,
                frame_token,
                geometry: Some(SurfaceGeometry::Cylinder {
                    origin,
                    axis: Vector3::new(1.0, 0.0, 0.0),
                    ref_direction: Vector3::new(0.0, 1.0, 0.0),
                    radius,
                }),
                u_range,
                v_range,
                stored_vector: None,
                phase: None,
            })
        }
        0x62 if frame_token == 0x0e && data.get(p + 89) == Some(&0x03) => {
            let radius = f64_le(data, p + 49)?;
            if !(0.0..1e6).contains(&radius) {
                return None;
            }
            Some(B2Cylinder {
                pos,
                layout,
                frame_token,
                geometry: None,
                u_range: read_f64_array::<2>(data, p + 57)?,
                v_range: read_f64_array::<2>(data, p + 73)?,
                stored_vector: Some(read_f64_array::<2>(data, p + 25)?),
                phase: Some(f64_le(data, p + 90)?),
            })
        }
        _ => None,
    }
}

/// Decode `b2 03 19` arc-length circle supports.
#[must_use]
pub fn b2_circles(data: &[u8]) -> Vec<B2Circle> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x19) {
        let pos = frame.pos;
        if !(0x32..=0x34).contains(&(frame.end - frame.payload)) {
            continue;
        }
        let Ok(frame_token) = u8::try_from(frame.header_token) else {
            continue;
        };
        let mut at = frame.payload;
        let Some(record_id) = compact_int(data, &mut at) else {
            continue;
        };
        let Some(values) = read_f64_array::<5>(data, at) else {
            continue;
        };
        let [c1, c2, radius, lo, hi] = values;
        if values.iter().all(|v| v.is_finite())
            && (0.0..1e6).contains(&radius)
            && c1.abs() <= 1e6
            && c2.abs() <= 1e6
            && hi > lo
        {
            out.push(B2Circle {
                pos,
                record_id,
                frame_token,
                center_pair: [c1, c2],
                radius,
                range: [lo, hi],
                full_circle: ((hi - lo) - 2.0 * std::f64::consts::PI * radius).abs() < 1e-9,
            });
        }
    }
    out
}

/// Decode structurally repeated `b2 03 23` edge-range packets.
#[must_use]
pub fn b2_edge_parameters(data: &[u8]) -> Vec<B2EdgeParameters> {
    let mut out = Vec::new();
    for frame in b_family_frames(data, 0x23) {
        let pos = frame.pos;
        if frame.end - frame.payload != 0x4e {
            continue;
        }
        let Some(values) = read_f64_array::<9>(data, frame.payload + 6) else {
            continue;
        };
        if values.iter().all(|v| v.is_finite())
            && values[0] == values[3]
            && values[0] == values[6]
            && values[1] == values[4]
            && values[1] == values[7]
            && values[5] == 1.0
            && values[2] == values[8]
        {
            out.push(B2EdgeParameters {
                pos,
                range: [values[0], values[1]],
                tolerance: values[2],
            });
        }
    }
    out
}

/// Decode `b2 03 31` offset-surface constructors.
#[must_use]
pub fn b2_offset_supports(data: &[u8]) -> Vec<B2OffsetSupport> {
    b_family_frames(data, 0x31)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 {
                return None;
            }
            let length = frame.end - frame.payload;
            let (support_id, at) = match data.get(frame.payload) {
                Some(0x08) if length == 0x2b => (
                    u32::from(u16_le(data, frame.payload + 1)?),
                    frame.payload + 3,
                ),
                Some(0x0c) if length == 0x2c => {
                    (u32_le_24(data, frame.payload + 1)?, frame.payload + 4)
                }
                _ => return None,
            };
            let values = read_f64_array::<5>(data, at)?;
            values
                .iter()
                .all(|v| v.is_finite())
                .then_some(B2OffsetSupport {
                    pos: frame.pos,
                    support_id,
                    distance: values[0],
                    domain: [values[1], values[2], values[3], values[4]],
                })
        })
        .collect()
}

/// Bind each offset constructor to the unique consolidated NURBS carrier whose
/// parameter domain contains the offset box and whose V-knot lane contains both
/// serialized V limits.
#[must_use]
pub fn offset_support_carriers(
    offsets: &[B2OffsetSupport],
    carriers: &[A8Surface],
) -> Vec<Option<usize>> {
    const PARAMETER_TOLERANCE: f64 = 1e-3;
    offsets
        .iter()
        .map(|offset| {
            let [u0, v0, u1, v1] = offset.domain;
            let candidates = carriers
                .iter()
                .enumerate()
                .filter_map(|(index, carrier)| {
                    let SurfaceGeometry::Nurbs(surface) = &carrier.geometry else {
                        return None;
                    };
                    let u_min = *surface.u_knots.first()?;
                    let u_max = *surface.u_knots.last()?;
                    let v_min = *surface.v_knots.first()?;
                    let v_max = *surface.v_knots.last()?;
                    let contains = u0 >= u_min - PARAMETER_TOLERANCE
                        && u1 <= u_max + PARAMETER_TOLERANCE
                        && v0 >= v_min - PARAMETER_TOLERANCE
                        && v1 <= v_max + PARAMETER_TOLERANCE;
                    let has_v_limit = |limit: f64| {
                        surface
                            .v_knots
                            .iter()
                            .any(|knot| (*knot - limit).abs() <= PARAMETER_TOLERANCE)
                    };
                    (contains && has_v_limit(v0) && has_v_limit(v1)).then_some(index)
                })
                .collect::<Vec<_>>();
            <[usize; 1]>::try_from(candidates).ok().map(|[index]| index)
        })
        .collect()
}

/// Decode framed `a5 03 20` consolidated UV jets.
#[must_use]
pub fn a5_pcurves(data: &[u8]) -> Vec<A5Pcurve> {
    a_family_frames(data, 0x20)
        .into_iter()
        .filter_map(|frame| parse_a5_pcurve(data, frame.pos, frame.payload, frame.end))
        .collect()
}

/// Decode width-coded `b2/b3/b4 03 20` consolidated UV jets.
#[must_use]
pub fn b2_pcurves(data: &[u8]) -> Vec<A5Pcurve> {
    b_family_frames(data, 0x20)
        .into_iter()
        .filter_map(|frame| parse_a5_pcurve(data, frame.pos, frame.payload, frame.end))
        .collect()
}

fn parse_a5_pcurve(data: &[u8], pos: usize, payload: usize, end: usize) -> Option<A5Pcurve> {
    let mut at = payload;
    let support_id = compact_int(data, &mut at)?;
    let degree = compact_int(data, &mut at)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    if !(1..=9).contains(&degree) || !(2..=4096).contains(&count) {
        return None;
    }
    let extrapolation_sites = match *data.get(at)? {
        0x0c => {
            at += 1;
            0
        }
        0x08 => {
            let encoded = *data.get(at + 1)?;
            if encoded % 4 != 1 {
                return None;
            }
            at += 2;
            u32::from((encoded - 1) / 4)
        }
        _ => return None,
    };
    let read = |at: &mut usize| -> Option<Vec<f64>> {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(f64_le(data, *at)?);
            *at += 8;
        }
        Some(values)
    };
    let knots = read(&mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
        return None;
    }
    at += 1;
    data.get(..at)?;
    let u = read(&mut at)?;
    let v = read(&mut at)?;
    let du = read(&mut at)?;
    let dv = read(&mut at)?;
    if data.get(at) != Some(&0x05) {
        return None;
    }
    at += 1;
    let ddu = read(&mut at)?;
    let ddv = read(&mut at)?;
    let range = [f64_le(data, at)?, f64_le(data, at + 8)?];
    at += 16;
    if at > end
        || knots.windows(2).any(|v| v[0] >= v[1])
        || knots
            .iter()
            .chain(&u)
            .chain(&v)
            .chain(&du)
            .chain(&dv)
            .chain(&ddu)
            .chain(&ddv)
            .chain(&range)
            .any(|x| !x.is_finite())
    {
        return None;
    }
    Some(A5Pcurve {
        pos,
        support_id,
        degree,
        extrapolation_sites,
        knots,
        points: u.into_iter().zip(v).map(|p| [p.0, p.1]).collect(),
        first_derivatives: du.into_iter().zip(dv).map(|p| [p.0, p.1]).collect(),
        second_derivatives: ddu.into_iter().zip(ddv).map(|p| [p.0, p.1]).collect(),
        range,
    })
}

/// One knot-site value in an `a5 03 32` rolling-ball program.
#[derive(Debug, Clone)]
pub struct RollingBallSite {
    /// First limiting curve point.
    pub limit1: [f64; 3],
    /// Second limiting curve point.
    pub limit2: [f64; 3],
    /// Rolling-ball centre.
    pub center: [f64; 3],
    /// Stored opening angle.
    pub theta: f64,
    /// Radius derived from centre to either limit.
    pub radius: f64,
}

/// Consolidated degree-5 rolling-ball jet.
#[derive(Debug, Clone)]
pub struct A5FreeformCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Schema token immediately before the payload.
    pub header_token: u32,
    /// Parametric degree.
    pub degree: u32,
    /// Distinct knots.
    pub knots: Vec<f64>,
    /// Position channels at each knot.
    pub sites: Vec<RollingBallSite>,
    /// Ten first-derivative channels per knot.
    pub first_derivatives: Vec<[f64; 10]>,
    /// Ten second-derivative channels per knot.
    pub second_derivatives: Vec<[f64; 10]>,
    /// Minimum stored-site radius.
    pub radius_min: f64,
    /// Maximum stored-site radius.
    pub radius_max: f64,
    /// Whether all stored-site radii are equal.
    pub radius_constant: bool,
}

/// One position and unit reference direction in an `a5/a6/a7 03 39` jet.
#[derive(Debug, Clone, PartialEq)]
pub struct GuideCurveSite {
    /// Guide-curve point.
    pub point: [f64; 3],
    /// Unit direction from the first stored triple to the second.
    pub direction: [f64; 3],
}

/// Width-coded guide-curve and reference-direction jet.
#[derive(Debug, Clone)]
pub struct A5GuideCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Parametric degree.
    pub degree: u32,
    /// Distinct parameter knots.
    pub knots: Vec<f64>,
    /// Position and unit-direction values at the knot sites.
    pub sites: Vec<GuideCurveSite>,
    /// Six first-derivative channels per site.
    pub first_derivatives: Vec<[f64; 6]>,
    /// Six second-derivative channels per site.
    pub second_derivatives: Vec<[f64; 6]>,
}

/// Decode `a5/a6/a7 03 39` guide-curve and unit-direction jets.
#[must_use]
pub fn a5_guide_curves(data: &[u8]) -> Vec<A5GuideCurve> {
    a_family_frames(data, 0x39)
        .into_iter()
        .filter_map(|frame| parse_a5_guide_curve(data, frame))
        .collect()
}

fn parse_a5_guide_curve(data: &[u8], frame: ConsolidatedFrame) -> Option<A5GuideCurve> {
    let mut at = frame.payload;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    let degree = compact_int(data, &mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count
        || !(2..=4096).contains(&count)
        || !(1..=9).contains(&degree)
    {
        return None;
    }
    at = consume_array_marker(data, at)?;
    let knots = f64_values(data, &mut at, count, frame.end)?;
    if !monotonic(&knots) {
        return None;
    }
    let block_bytes = count.checked_mul(48)?;
    if at.checked_add(3 * block_bytes)?.checked_add(48)? != frame.end {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[f64; 6]>> {
        (0..count)
            .map(|site| read_f64_array::<6>(data, start + site * 48))
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites: Option<Vec<_>> = positions
        .into_iter()
        .map(|value| {
            let point = [value[0], value[1], value[2]];
            let direction = [
                value[3] - value[0],
                value[4] - value[1],
                value[5] - value[2],
            ];
            let length =
                (direction[0].powi(2) + direction[1].powi(2) + direction[2].powi(2)).sqrt();
            ((length - 1.0).abs() < 1e-9).then_some(GuideCurveSite { point, direction })
        })
        .collect();
    Some(A5GuideCurve {
        pos: frame.pos,
        header_token: frame.header_token,
        degree,
        knots,
        sites: sites?,
        first_derivatives,
        second_derivatives,
    })
}

/// Common-form degree-5 rolling-ball jet stored in an `a8 03 32` object record.
#[derive(Debug, Clone)]
pub struct A8FreeformCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Inline persistent object identifier.
    pub object_id: u32,
    /// Parametric degree.
    pub degree: u32,
    /// Distinct parameter knots.
    pub knots: Vec<f64>,
    /// Multiplicity for each distinct knot.
    pub multiplicities: Vec<u32>,
    /// Position channels at each knot.
    pub sites: Vec<RollingBallSite>,
    /// Ten first-derivative channels per knot.
    pub first_derivatives: Vec<[f64; 10]>,
    /// Ten second-derivative channels per knot.
    pub second_derivatives: Vec<[f64; 10]>,
    /// Bytes following the three jet blocks inside the payload.
    pub tail_len: usize,
}

/// Decode framed `a8 03 32` common-form rolling-ball jet records.
#[must_use]
pub fn a8_freeform_curves(data: &[u8]) -> Vec<A8FreeformCurve> {
    let mut out = Vec::new();
    let mut search = 0;
    while let Some(relative) = data[search..].windows(3).position(|v| v == [0xa8, 3, 0x32]) {
        let pos = search + relative;
        search = pos + 3;
        let Some(length) = u32_le(data, pos + 3).and_then(|v| usize::try_from(v).ok()) else {
            continue;
        };
        let Some(end) = pos.checked_add(11).and_then(|v| v.checked_add(length)) else {
            continue;
        };
        if end <= data.len() {
            if let Some(value) = parse_a8_curve(data, pos, end) {
                out.push(value);
            }
        }
    }
    out
}

fn parse_a8_curve(data: &[u8], pos: usize, end: usize) -> Option<A8FreeformCurve> {
    let object_id = u32_le(data, pos + 7)?;
    let mut at = pos + 12;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    let degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count
        || !(2..=8192).contains(&count)
        || !(1..=9).contains(&degree)
    {
        return None;
    }
    at += if data.get(at) == Some(&0x08) { 2 } else { 1 };
    let mut knots = Vec::with_capacity(count);
    for _ in 0..count {
        knots.push(f64_le(data, at)?);
        at += 8;
    }
    let mut multiplicities = Vec::with_capacity(count);
    for _ in 0..count {
        multiplicities.push(compact_int(data, &mut at)?);
    }
    if knots.iter().any(|v| !v.is_finite()) || knots.windows(2).any(|v| v[0] > v[1]) {
        return None;
    }
    let block_bytes = count.checked_mul(80)?;
    let blocks_end = at.checked_add(3 * block_bytes)?;
    if blocks_end > end || end - blocks_end > 8192 {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[f64; 10]>> {
        (0..count)
            .map(|site| {
                let mut values = [0.0; 10];
                for (channel, value) in values.iter_mut().enumerate() {
                    *value = f64_le(data, start + site * 80 + channel * 8)?;
                }
                values.iter().all(|v| v.is_finite()).then_some(values)
            })
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites = rolling_ball_sites(positions)?;
    Some(A8FreeformCurve {
        pos,
        object_id,
        degree,
        knots,
        multiplicities,
        sites,
        first_derivatives,
        second_derivatives,
        tail_len: end - blocks_end,
    })
}

/// Decode framed `a5 03 32` rolling-ball jet records.
#[must_use]
pub fn a5_freeform_curves(data: &[u8]) -> Vec<A5FreeformCurve> {
    a_family_frames(data, 0x32)
        .into_iter()
        .filter_map(|frame| parse_a5_curve(data, frame))
        .collect()
}

fn parse_a5_curve(data: &[u8], frame: ConsolidatedFrame) -> Option<A5FreeformCurve> {
    let ConsolidatedFrame {
        pos,
        payload,
        end,
        header_token,
    } = frame;
    let mut at = payload;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    let degree = compact_int(data, &mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count
        || !(2..=4096).contains(&count)
        || !(1..=9).contains(&degree)
    {
        return None;
    }
    match data.get(at..at + 2) {
        Some([0x0c, _]) => at += 1,
        Some([0x08, 0x09 | 0x05]) => at += 2,
        _ => return None,
    }
    let mut knots = Vec::with_capacity(count);
    for _ in 0..count {
        knots.push(f64_le(data, at)?);
        at += 8;
    }
    if knots.iter().any(|v| !v.is_finite()) || knots.windows(2).any(|v| v[0] >= v[1]) {
        return None;
    }
    let block_bytes = count.checked_mul(80)?;
    if at.checked_add(3 * block_bytes)? > end || end - (at + 3 * block_bytes) > 4096 {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[f64; 10]>> {
        (0..count)
            .map(|site| {
                let mut values = [0.0; 10];
                for (channel, value) in values.iter_mut().enumerate() {
                    *value = f64_le(data, start + site * 80 + channel * 8)?;
                }
                values.iter().all(|v| v.is_finite()).then_some(values)
            })
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites = rolling_ball_sites(positions)?;
    let radius_min = sites.iter().map(|s| s.radius).fold(f64::INFINITY, f64::min);
    let radius_max = sites
        .iter()
        .map(|s| s.radius)
        .fold(f64::NEG_INFINITY, f64::max);
    Some(A5FreeformCurve {
        pos,
        header_token,
        degree,
        knots,
        sites,
        first_derivatives,
        second_derivatives,
        radius_min,
        radius_max,
        radius_constant: radius_max - radius_min < 1e-9,
    })
}

fn rolling_ball_sites(positions: Vec<[f64; 10]>) -> Option<Vec<RollingBallSite>> {
    let mut sites = Vec::with_capacity(positions.len());
    for v in positions {
        let limit1 = [v[0], v[1], v[2]];
        let limit2 = [v[3], v[4], v[5]];
        let center = [v[6], v[7], v[8]];
        let radius = distance3(center, limit1);
        let other = distance3(center, limit2);
        let chord = distance3(limit1, limit2);
        if radius <= f64::EPSILON
            || (radius - other).abs() > 1e-9 * radius.max(1.0)
            || (v[9] - 2.0 * (chord / (2.0 * radius)).clamp(-1.0, 1.0).asin()).abs() > 1e-9
        {
            return None;
        }
        sites.push(RollingBallSite {
            limit1,
            limit2,
            center,
            theta: v[9],
            radius,
        });
    }
    Some(sites)
}

fn distance3(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[derive(Clone, Copy)]
struct ConsolidatedFrame {
    pos: usize,
    payload: usize,
    end: usize,
    header_token: u32,
}

/// Width-coded consolidated record family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsolidatedFamily {
    /// U32-length A family (`a5/a6/a7`).
    A,
    /// U8-length B family (`b2/b3/b4`).
    B,
}

/// One length-closed record in a consolidated A/B cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidatedRecord {
    /// Record family.
    pub family: ConsolidatedFamily,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent flag byte (`0x03`, `0x13`, or `0x83`).
    pub flag: u8,
    /// Record class byte.
    pub class: u8,
    /// Little-endian width-coded header token.
    pub header_token: u32,
    /// Complete record byte range.
    pub range: Range<usize>,
    /// Payload byte range.
    pub payload: Range<usize>,
}

/// Inventory length-closed consolidated A/B records while suppressing candidates
/// nested inside the payload of an already accepted frame.
#[must_use]
pub fn consolidated_records(data: &[u8]) -> Vec<ConsolidatedRecord> {
    let flags = [0x03, 0x13, 0x83];
    let mut candidates = Vec::new();
    for pos in 0..data.len().saturating_sub(4) {
        let (family, width, token_at, length) = if let Some(width) = data[pos]
            .checked_sub(0xa4)
            .filter(|width| (1..=3).contains(width))
        {
            let Some(length) = u32_le(data, pos + 3).and_then(|v| usize::try_from(v).ok()) else {
                continue;
            };
            (ConsolidatedFamily::A, width, pos + 7, length)
        } else if let Some(width) = data[pos]
            .checked_sub(0xb1)
            .filter(|width| (1..=3).contains(width))
        {
            (
                ConsolidatedFamily::B,
                width,
                pos + 4,
                usize::from(data[pos + 3]),
            )
        } else {
            continue;
        };
        let Some(&flag) = data.get(pos + 1) else {
            continue;
        };
        let Some(&class) = data.get(pos + 2) else {
            continue;
        };
        if !flags.contains(&flag) {
            continue;
        }
        let width_usize = usize::from(width);
        let Some(payload_start) = token_at.checked_add(width_usize) else {
            continue;
        };
        let Some(end) = payload_start.checked_add(length) else {
            continue;
        };
        if end > data.len() {
            continue;
        }
        let header_token = data[token_at..payload_start]
            .iter()
            .enumerate()
            .fold(0u32, |value, (shift, byte)| {
                value | (u32::from(*byte) << (8 * shift))
            });
        candidates.push(ConsolidatedRecord {
            family,
            width,
            flag,
            class,
            header_token,
            range: pos..end,
            payload: payload_start..end,
        });
    }
    let mut records: Vec<ConsolidatedRecord> = Vec::new();
    let mut active_payload: Option<Range<usize>> = None;
    for candidate in candidates {
        if active_payload
            .as_ref()
            .is_some_and(|payload| payload.contains(&candidate.range.start))
        {
            continue;
        }
        active_payload = Some(candidate.payload.clone());
        records.push(candidate);
    }
    records
}

fn a_family_frames(data: &[u8], class: u8) -> Vec<ConsolidatedFrame> {
    consolidated_records(data)
        .into_iter()
        .filter(|record| record.family == ConsolidatedFamily::A && record.class == class)
        .map(|record| ConsolidatedFrame {
            pos: record.range.start,
            payload: record.payload.start,
            end: record.range.end,
            header_token: record.header_token,
        })
        .collect()
}

fn b_family_frames(data: &[u8], class: u8) -> Vec<ConsolidatedFrame> {
    consolidated_records(data)
        .into_iter()
        .filter(|record| record.family == ConsolidatedFamily::B && record.class == class)
        .map(|record| ConsolidatedFrame {
            pos: record.range.start,
            payload: record.payload.start,
            end: record.range.end,
            header_token: record.header_token,
        })
        .collect()
}

fn read_f64_array<const N: usize>(data: &[u8], start: usize) -> Option<[f64; N]> {
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = f64_le(data, start.checked_add(index.checked_mul(8)?)?)?;
    }
    Some(values)
}

/// Decode framed `a8 03 20` UV jet records.
#[must_use]
pub fn a8_pcurves(data: &[u8]) -> Vec<A8Pcurve> {
    object_stream_pcurves(data)
        .into_iter()
        .filter(|pcurve| data.get(pcurve.pos) == Some(&0xa8))
        .collect()
}

/// Decode framed `a8 03 20` and `b5 03 20` object-stream UV jet records.
#[must_use]
pub fn object_stream_pcurves(data: &[u8]) -> Vec<A8Pcurve> {
    let mut out = Vec::new();
    for pos in 0..data.len().saturating_sub(11) {
        if data.get(pos + 1..pos + 3) != Some(&[0x03, 0x20]) {
            continue;
        }
        let Some((payload, length, object_id)) = (match data[pos] {
            0xa8 => u32_le(data, pos + 3)
                .and_then(|length| usize::try_from(length).ok())
                .zip(u32_le(data, pos + 7))
                .map(|(length, object_id)| (pos + 11, length, object_id)),
            0xb5 => data
                .get(pos + 3)
                .zip(u32_le(data, pos + 4))
                .map(|(length, object_id)| (pos + 8, usize::from(*length), object_id)),
            _ => None,
        }) else {
            continue;
        };
        let Some(end) = payload.checked_add(length) else {
            continue;
        };
        if end > data.len() {
            continue;
        }
        if let Some(value) = parse_object_stream_pcurve(data, pos, payload, end, object_id) {
            out.push(value);
        }
    }
    out
}

fn parse_object_stream_pcurve(
    data: &[u8],
    pos: usize,
    payload: usize,
    end: usize,
    object_id: u32,
) -> Option<A8Pcurve> {
    let mut at = payload + 1;
    let support_id = object_stream_reference(data, &mut at)?;
    let degree = compact_int(data, &mut at)?;
    at += 2;
    data.get(..at)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    at += if data.get(at) == Some(&0x08) { 2 } else { 1 };
    if !(2..=8192).contains(&count) || !(1..=9).contains(&degree) {
        return None;
    }
    let read = |at: &mut usize| -> Option<Vec<f64>> {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(f64_le(data, *at)?);
            *at += 8;
        }
        Some(values)
    };
    let knots = read(&mut at)?;
    let mut multiplicities = Vec::with_capacity(count);
    for _ in 0..count {
        multiplicities.push(compact_int(data, &mut at)?);
    }
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
        return None;
    }
    let mode = *data.get(at)?;
    at += 1;
    let u = read(&mut at)?;
    let v = read(&mut at)?;
    let du = read(&mut at)?;
    let dv = read(&mut at)?;
    if data.get(at) != Some(&0x05) {
        return None;
    }
    at += 1;
    let ddu = read(&mut at)?;
    let ddv = read(&mut at)?;
    let range = [f64_le(data, at)?, f64_le(data, at + 8)?];
    at += 16;
    if data.get(at) != Some(&0x07)
        || end - (at + 1) > 256
        || knots
            .iter()
            .chain(&u)
            .chain(&v)
            .chain(&du)
            .chain(&dv)
            .chain(&ddu)
            .chain(&ddv)
            .chain(&range)
            .any(|x| !x.is_finite() || x.abs() >= 1e12)
    {
        return None;
    }
    Some(A8Pcurve {
        pos,
        object_id,
        support_id,
        degree,
        knots,
        multiplicities,
        mode,
        points: u.into_iter().zip(v).map(|p| [p.0, p.1]).collect(),
        first_derivatives: du.into_iter().zip(dv).map(|p| [p.0, p.1]).collect(),
        second_derivatives: ddu.into_iter().zip(ddv).map(|p| [p.0, p.1]).collect(),
        range,
    })
}

/// Convert degree-5 position/first/second-derivative knot jets into an exact
/// piecewise Bézier B-spline control net.
pub(crate) fn quintic_jet_bspline(
    degree: u32,
    knots: &[f64],
    points: &[[f64; 2]],
    first: &[[f64; 2]],
    second: &[[f64; 2]],
) -> Option<(Vec<f64>, Vec<[f64; 2]>)> {
    if degree != 5
        || knots.len() < 2
        || points.len() != knots.len()
        || first.len() != knots.len()
        || second.len() != knots.len()
    {
        return None;
    }
    let mut controls = Vec::with_capacity(6 * (knots.len() - 1));
    let mut full_knots = vec![knots[0]; 6];
    for index in 0..knots.len() - 1 {
        let h = knots[index + 1] - knots[index];
        if !h.is_finite() || h <= 0.0 {
            return None;
        }
        let p0 = points[index];
        let p1 = points[index + 1];
        let d0 = first[index];
        let d1 = first[index + 1];
        let dd0 = second[index];
        let dd1 = second[index + 1];
        controls.extend([
            p0,
            [p0[0] + h * d0[0] / 5.0, p0[1] + h * d0[1] / 5.0],
            [
                p0[0] + 2.0 * h * d0[0] / 5.0 + h * h * dd0[0] / 20.0,
                p0[1] + 2.0 * h * d0[1] / 5.0 + h * h * dd0[1] / 20.0,
            ],
            [
                p1[0] - 2.0 * h * d1[0] / 5.0 + h * h * dd1[0] / 20.0,
                p1[1] - 2.0 * h * d1[1] / 5.0 + h * h * dd1[1] / 20.0,
            ],
            [p1[0] - h * d1[0] / 5.0, p1[1] - h * d1[1] / 5.0],
            p1,
        ]);
        full_knots.extend([knots[index + 1]; 6]);
    }
    Some((full_knots, controls))
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
    if let Some(first) = standard_surface_records(brep, face_count)
        .and_then(|records| records.last().map(StandardSurfaceRecord::end))
    {
        let rows = standard_curve_supports_at(brep, face_count, first);
        if !rows.is_empty() {
            return rows;
        }
    }

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

    standard_curve_supports_at(brep, face_count, first)
}

fn standard_curve_supports_at(
    brep: &[u8],
    face_count: usize,
    mut position: usize,
) -> Vec<StandardCurveSupport> {
    const LINE: [u8; 5] = [0x00, 0x02, 0x00, 0x33, 0x36];
    const CIRCLE: [u8; 5] = [0x00, 0x12, 0x00, 0x33, 0x37];

    let mut rows = Vec::new();
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

/// Decode every structurally complete `a8 03 34` parameter lattice, including
/// records whose pole representation is not inline.
#[must_use]
pub fn a8_surface_headers(data: &[u8]) -> Vec<A8SurfaceHeader> {
    let mut out = Vec::new();
    let mut search = 0usize;
    while let Some(relative) = data[search..].windows(3).position(|v| v == [0xa8, 3, 0x34]) {
        let pos = search + relative;
        search = pos + 3;
        if let Some(parsed) = parse_a8_surface_header(data, pos) {
            out.push(parsed.header);
        }
    }
    out
}

/// Decode consolidated `a5 03 34` NURBS surface carriers.  This family uses
/// implicit clamped multiplicities instead of the explicit `a8` vectors.
pub fn a5_surfaces(data: &[u8]) -> Vec<A8Surface> {
    a_family_frames(data, 0x34)
        .into_iter()
        .filter_map(|frame| a5_surface(data, frame))
        .collect()
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

fn a5_surface(data: &[u8], frame: ConsolidatedFrame) -> Option<A8Surface> {
    let ConsolidatedFrame {
        pos, payload, end, ..
    } = frame;
    let mut at = payload;
    let u_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let u_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let u_distinct = f64_values(data, &mut at, u_distinct_count, end)?;
    let v_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let v_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let v_distinct = f64_values(data, &mut at, v_distinct_count, end)?;
    let mode = *data.get(at)?;
    at += 1;
    let (u_knots, u_count) = a5_knots(&u_distinct, u_degree)?;
    let (v_knots, v_count) = a5_knots(&v_distinct, v_degree)?;
    if !monotonic(&u_distinct) || !monotonic(&v_distinct) {
        return None;
    }
    let poles = (u_count as usize).checked_mul(v_count as usize)?;
    if at.checked_add(poles.checked_mul(24)?)? > end {
        return None;
    }
    let mut control_points = Vec::with_capacity(poles);
    for _ in 0..poles {
        control_points.push(f64_point(data, at)?);
        at += 24;
    }
    if control_points
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .any(|coordinate| !coordinate.is_finite() || coordinate.abs() >= 1e12)
    {
        return None;
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
    if at + 4 > end
        || !matches!(data.get(at..at + 4), Some([0x05, a, 0x05, b]) if (*a - 1) % 4 == 0 && (*b - 1) % 4 == 0)
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

struct ParsedA8SurfaceHeader {
    header: A8SurfaceHeader,
    pole_start: usize,
    end: usize,
}

fn parse_a8_surface_header(data: &[u8], pos: usize) -> Option<ParsedA8SurfaceHeader> {
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
    let tail_end = at.checked_add(141)?;
    let poles_elided = matches!(data.get(at..at + 4), Some([0x05, a, 0x05, b]) if *a % 4 == 1 && *b % 4 == 1)
        && tail_end <= end
        && (tail_end == end
            || matches!(
                data.get(tail_end..tail_end + 3),
                Some([
                    0xa5 | 0xa8 | 0xa9 | 0xb2 | 0xb3 | 0xb4 | 0xb5 | 0xb6,
                    0x03,
                    _
                ])
            ));
    Some(ParsedA8SurfaceHeader {
        header: A8SurfaceHeader {
            pos,
            object_id,
            u_degree,
            v_degree,
            u_distinct_knots: u_distinct,
            v_distinct_knots: v_distinct,
            u_multiplicities: u_mults,
            v_multiplicities: v_mults,
            u_count,
            v_count,
            rational: mode == 0x05,
            poles_elided,
        },
        pole_start: at,
        end,
    })
}

fn a8_surface(data: &[u8], pos: usize) -> Option<A8Surface> {
    let ParsedA8SurfaceHeader {
        header,
        mut pole_start,
        end,
    } = parse_a8_surface_header(data, pos)?;
    let A8SurfaceHeader {
        object_id,
        u_degree,
        v_degree,
        u_distinct_knots,
        v_distinct_knots,
        u_multiplicities,
        v_multiplicities,
        u_count,
        v_count,
        rational,
        poles_elided,
        ..
    } = header;
    if poles_elided {
        return None;
    }
    let poles = (u_count as usize).checked_mul(v_count as usize)?;
    let pole_bytes = poles.checked_mul(24)?;
    if pole_start.checked_add(pole_bytes)? > end {
        return None;
    }
    let mut control_points = Vec::with_capacity(poles);
    for _ in 0..poles {
        control_points.push(f64_point(data, pole_start)?);
        pole_start += 24;
    }
    if control_points
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .any(|coordinate| !coordinate.is_finite() || coordinate.abs() >= 1e12)
    {
        return None;
    }
    let weights = if rational {
        let values = f64_values(data, &mut pole_start, poles, end)?;
        values
            .iter()
            .all(|weight| *weight != 0.0)
            .then_some(values)?
    } else {
        Vec::new()
    };
    if pole_start > end {
        return None;
    }
    Some(A8Surface {
        pos,
        object_id,
        geometry: SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree,
            v_degree,
            u_knots: expand_knots(&u_distinct_knots, &u_multiplicities)?,
            v_knots: expand_knots(&v_distinct_knots, &v_multiplicities)?,
            u_count,
            v_count,
            control_points,
            weights: rational.then_some(weights),
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
        ratio: 1.0,
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
        let geometry = zero_entity_surface_at(data, p);
        if let Some(geometry) = geometry {
            out.push(ZeroEntitySurface { pos: p, geometry });
        }
        p = end;
    }
    out
}

pub(crate) fn zero_entity_surface_at(data: &[u8], record: usize) -> Option<SurfaceGeometry> {
    let payload_end = record.checked_add(*data.get(record + 3)? as usize + 12)?;
    let payload = data.get(record + 4..payload_end)?;
    match (*data.get(record + 2)?, *data.get(record + 3)?) {
        (0x27, 0x6a) => zero_entity_plane(payload),
        (0x28, 0x8a) => zero_entity_cylinder(payload),
        (0x29, 0xb8) => zero_entity_cone(payload),
        (0x2b, 0xc8) => zero_entity_torus(payload),
        (0x34, 0xc8 | 0x5e) => zero_entity_nurbs_surface(data, record),
        _ => None,
    }
}

/// Contract one parameter of a tensor-product NURBS surface into its exact
/// rational isocurve.
pub(crate) fn nurbs_surface_isocurve(
    surface: &NurbsSurface,
    parameter: f64,
    fix_u: bool,
) -> Option<NurbsCurve> {
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let (fixed_basis, varying_count, degree, knots) = if fix_u {
        (
            nurbs_basis_values(
                &surface.u_knots,
                usize::try_from(surface.u_degree).ok()?,
                parameter,
                u_count,
            )?,
            v_count,
            surface.v_degree,
            surface.v_knots.clone(),
        )
    } else {
        (
            nurbs_basis_values(
                &surface.v_knots,
                usize::try_from(surface.v_degree).ok()?,
                parameter,
                v_count,
            )?,
            u_count,
            surface.u_degree,
            surface.u_knots.clone(),
        )
    };
    let mut control_points = Vec::with_capacity(varying_count);
    let mut weights = Vec::with_capacity(varying_count);
    for varying in 0..varying_count {
        let mut numerator = [0.0; 3];
        let mut denominator = 0.0;
        for (fixed, basis) in fixed_basis.iter().copied().enumerate() {
            let index = if fix_u {
                fixed.checked_mul(v_count)?.checked_add(varying)?
            } else {
                varying.checked_mul(v_count)?.checked_add(fixed)?
            };
            let point = surface.control_points.get(index)?;
            let weight = surface
                .weights
                .as_ref()
                .and_then(|values| values.get(index))
                .copied()
                .unwrap_or(1.0);
            let factor = basis * weight;
            numerator[0] += factor * point.x;
            numerator[1] += factor * point.y;
            numerator[2] += factor * point.z;
            denominator += factor;
        }
        if !denominator.is_finite() || denominator.abs() <= f64::EPSILON {
            return None;
        }
        control_points.push(Point3::new(
            numerator[0] / denominator,
            numerator[1] / denominator,
            numerator[2] / denominator,
        ));
        weights.push(denominator);
    }
    Some(NurbsCurve {
        degree,
        knots,
        control_points,
        weights: surface.weights.is_some().then_some(weights),
        periodic: if fix_u {
            surface.v_periodic
        } else {
            surface.u_periodic
        },
    })
}

fn nurbs_basis_values(
    knots: &[f64],
    degree: usize,
    parameter: f64,
    count: usize,
) -> Option<Vec<f64>> {
    if knots.len() != count.checked_add(degree)?.checked_add(1)? || count == 0 {
        return None;
    }
    let mut basis = vec![0.0; count + degree];
    for (index, value) in basis.iter_mut().enumerate() {
        if (knots.get(index)? <= &parameter && &parameter < knots.get(index + 1)?)
            || (parameter == *knots.last()? && index + 1 == count)
        {
            *value = 1.0;
        }
    }
    for level in 1..=degree {
        for index in 0..count + degree - level {
            let left_denominator = knots[index + level] - knots[index];
            let right_denominator = knots[index + level + 1] - knots[index + 1];
            let left = if left_denominator.abs() <= f64::EPSILON {
                0.0
            } else {
                (parameter - knots[index]) / left_denominator * basis[index]
            };
            let right = if right_denominator.abs() <= f64::EPSILON {
                0.0
            } else {
                (knots[index + level + 1] - parameter) / right_denominator * basis[index + 1]
            };
            basis[index] = left + right;
        }
    }
    basis.truncate(count);
    basis.iter().all(|value| value.is_finite()).then_some(basis)
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
    let after_u_tokens = skip_u32_token_run(data, after_u_mults)?;
    let extra_u_bytes = after_u_tokens.checked_sub(after_u_mults)?;
    if extra_u_bytes != 0 && extra_u_bytes < 10 {
        return None;
    }
    let (v_distinct, after_v) = f64_monotonic_run(data, after_u_tokens.checked_add(1)?)?;
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
    let grid = skip_u32_token_run(data, after_v_mults)?.checked_add(3)?;
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

fn skip_u32_token_run(data: &[u8], mut at: usize) -> Option<usize> {
    while data.get(at) == Some(&0x10) {
        u32_le(data, at + 1)?;
        at = at.checked_add(5)?;
    }
    Some(at)
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
        ratio: 1.0,
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
                ratio: 1.0,
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

fn object_stream_reference(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let lead = *bytes.get(*at)?;
    let (value, width) = match lead {
        0x38 => (u32_le_24(bytes, *at + 1)?, 4),
        0x30 => (u32::from(u16_le(bytes, *at + 1)?) << 8, 3),
        0x28 => (
            u32::from(*bytes.get(*at + 1)?) | (u32::from(*bytes.get(*at + 2)?) << 16),
            3,
        ),
        0x20 => (u32::from(*bytes.get(*at + 1)?) << 16, 2),
        0x18 => (u32::from(u16_le(bytes, *at + 1)?), 3),
        0x10 => (u32::from(*bytes.get(*at + 1)?) << 8, 2),
        0x08 => (u32::from(*bytes.get(*at + 1)?), 2),
        0x80..=0xff => (u32::from(lead - 0x80), 1),
        _ => return None,
    };
    *at += width;
    Some(value)
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

fn persistent_ref(bytes: &[u8], at: &mut usize) -> Option<u32> {
    if bytes.get(*at) == Some(&0x0a) {
        let value = u32::from(u16_le(bytes, *at + 1)?);
        *at += 3;
        Some(value)
    } else {
        compact_int(bytes, at)
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
