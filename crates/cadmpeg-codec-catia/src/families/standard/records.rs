//! Standard-nested `SurfacicReps` record decoders.
//!
//! Decodes per-face analytic surface records, plane bounds, the `0x60`
//! curve-support/edge-incidence table, standard vertex rosters, and the
//! inline big-endian curved-surface parameter block.

use cadmpeg_ir::be::f32_at as f32_be;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::le::u32_at as u32_le;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, HashMap};

/// The standard-nested plane bounds record. Its three-byte tag is the bridge to
/// the matching `SurfacicReps` plane marker.
#[derive(Debug, Clone)]
pub struct PlaneParams {
    /// The little-endian u24 carrier tag.
    pub target: u32,
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

/// Scan every `05 08 01` coordinate row in `bytes`, returning the decoded
/// vertex points in stream order.
pub fn scan_vertex_records(bytes: &[u8]) -> Vec<Point3> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 15 <= bytes.len() {
        if bytes[p] == 0x05 && bytes[p + 1] == 0x08 && bytes[p + 2] == 0x01 {
            let x = f32_le(bytes, p + 3);
            let y = f32_le(bytes, p + 7);
            let z = f32_le(bytes, p + 11);
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

/// Locate plane bounds records and bind each persistent carrier tag to the
/// frame vector of its face-local trim packet.
pub fn plane_params<S: std::hash::BuildHasher>(
    brep: &[u8],
    normals: &HashMap<u32, [f64; 3], S>,
) -> Vec<PlaneParams> {
    const MARKER: &[u8; 5] = b"\x00\x02\x00\x33\x32";
    const TOLERANCE: f32 = 1e-5;

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
            || (0..3).any(|axis| {
                let center_delta = (values[axis] - sphere[axis]).abs();
                center_delta + half[axis]
                    > radius + TOLERANCE * (1.0 + center_delta.max(half[axis]).max(radius.abs()))
            })
        {
            continue;
        }
        let target = u24_le(brep, pos - 3);
        let Some(normal) = normals.get(&target).copied() else {
            continue;
        };
        out.push(PlaneParams {
            target,
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

/// A circle carrier in the standard `0x60` edge-support table.
#[derive(Debug, Clone)]
pub struct StandardCircle {
    /// Offset of the support row.
    pub pos: usize,
    /// Native allocation tag of the support row.
    pub tag: u32,
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
    /// Native allocation tag of the support row.
    pub tag: u32,
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
                tag: row.tag,
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
                tag: row.tag,
                faces: row.faces,
            }),
            StandardCurveGeometry::Circle { .. } | StandardCurveGeometry::Bspline => None,
        })
        .collect()
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
            // torus: cx cy cz ax ay signed_major minor; sign(major) carries sign(az).
            let (cx, cy, cz, ax, ay, major, minor) =
                (be(0)?, be(1)?, be(2)?, be(3)?, be(4)?, be(5)?, be(6)?);
            if !all_finite(&[cx, cy, cz, ax, ay, major, minor]) {
                return None;
            }
            if !(major.abs() > 0.0
                && major.abs() < 1e6
                && minor > 0.0
                && minor < 1e6
                && ax * ax + ay * ay <= 1.0 + 1e-4)
            {
                return None;
            }
            let axis = axis_from_xy(ax, ay, major);
            Some(SurfaceGeometry::Torus {
                center: pt(cx, cy, cz),
                axis,
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
                major_radius: major.abs() as f64,
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

/// Read the face-side witness point following a standard cylinder or torus
/// carrier's big-endian parameter block.
#[must_use]
pub fn standard_face_witness(brep: &[u8], marker_pos: usize) -> Option<Point3> {
    if brep.get(marker_pos..marker_pos + 2) != Some(&[0x00, 0x33]) {
        return None;
    }
    let kind = *brep.get(marker_pos + 2)?;
    let offset = match kind {
        0x33 => 27,
        0x38 => 31,
        _ => return None,
    };
    let values = [
        f32_le(brep, marker_pos + offset),
        f32_le(brep, marker_pos + offset + 4),
        f32_le(brep, marker_pos + offset + 8),
    ];
    values
        .iter()
        .all(|value| value.is_finite())
        .then(|| pt(values[0], values[1], values[2]))
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

fn finite_in_range(v: f32) -> bool {
    v.is_finite() && v.abs() < 1e4
}

fn all_finite(vs: &[f32]) -> bool {
    vs.iter().all(|v| v.is_finite())
}
