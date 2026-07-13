// SPDX-License-Identifier: Apache-2.0
//! Curve namespace prototypes and topology rows.
//!
//! Prototype rows identify curves and their generating features. Topology rows
//! add the two face sides and successor curve for each native half-edge. Curve
//! parameter bodies are not interpreted here.

use crate::psb::{self, compact_int, reference_id};
use crate::scalar;

/// A labeled curve namespace entry.
///
/// `type_byte` remains raw because the namespace grammar does not define its
/// geometric interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurvePrototype {
    /// The row's `crv_id`: the curve's identifier in the `crv_array`
    /// namespace, referenced by `srf_array` and topology row `E0`/`E1`
    /// fields.
    pub id: u32,
    /// The row's raw `type` byte. Its geometric meaning is not identified by
    /// the namespace grammar alone ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)); the curve-body evaluator
    /// determines the interpretation.
    pub type_byte: u8,
    /// The `feat_id` compact integer, when the labeled row has one: the
    /// feature that generated this curve.
    pub feature_id: Option<u32>,
    /// Byte offset of this prototype's `crv_array` label in the original
    /// stream.
    pub offset: usize,
}

/// A curve row with a uniquely delimited topology suffix.
///
/// `faces` and `next_edges` preserve the two native sides in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveTopologyRow {
    /// The row's `crv_id`, matching a [`CurvePrototype::id`] in the same
    /// `crv_array` namespace.
    pub id: u32,
    /// The row's raw `type` byte; see [`CurvePrototype::type_byte`].
    pub type_byte: u8,
    /// The `feat_id` compact integer: the feature that generated this
    /// curve.
    pub feature_id: u32,
    /// The two `crv_pnt_dir` orientation-flag bytes, one per half-edge side.
    /// These are per-side orientation flags, not a tangent vector
    /// ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)).
    pub directions: [u8; 2],
    /// The `F0`/`F1` suffix fields: the `srf_array` face identifiers
    /// bounding the curve's two half-edge sides.
    pub faces: [u32; 2],
    /// The `E0`/`E1` suffix fields: the `crv_array` identifier of the next
    /// edge for each of the two half-edge sides, used to walk loops
    /// ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)).
    pub next_edges: [u32; 2],
    /// Byte offset of the row's `crv_id` field in the original stream.
    pub offset: usize,
}

/// Resolution state of a curve row's four-reference topology suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveSuffixStatus {
    /// Exactly one canonical suffix boundary exists.
    Unique,
    /// Multiple canonical suffix boundaries exist; connectivity is withheld.
    Ambiguous {
        /// Number of byte-valid suffix boundaries.
        candidate_count: usize,
    },
}

/// Bounded analytic parameter body from one positional `crv_array` row.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveParameterRecord {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Raw curve-family discriminator.
    pub type_byte: u8,
    /// Exact bytes between direction flags and the selected suffix boundary.
    pub body: Vec<u8>,
    /// Decoded scalar values in byte order.
    pub scalar_values: Vec<f64>,
    /// Canonical entity references skipped while walking the scalar lane.
    pub skipped_references: Vec<u32>,
    /// Whether the topology suffix boundary is unique.
    pub suffix: CurveSuffixStatus,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
    /// Byte offset of the first parameter-body byte in the original stream.
    pub body_offset: usize,
    /// Byte offset of the selected body/suffix boundary in the original stream.
    pub suffix_offset: usize,
}

/// Two pcurve endpoints represented in both adjacent face parameter frames.
#[derive(Debug, Clone, PartialEq)]
pub struct PcurveEndpoints {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Adjacent face identifiers corresponding to face frames zero and one.
    pub faces: [u32; 2],
    /// Endpoint A then B in the first face's local UV frame.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in the second face's local UV frame.
    pub face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// Ordered world-coordinate lane from an `fc <subtype>` dense curve body.
#[derive(Debug, Clone, PartialEq)]
pub struct FcCurveControlPoints {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Byte following the `fc` body prefix.
    pub subtype: u8,
    /// Ordered exact world-coordinate values, in mm.
    pub values_mm: Vec<f64>,
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// Circle proven by the decoded points of an `fc 05` curve body.
#[derive(Debug, Clone, PartialEq)]
pub struct Fc05Circle {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Circle center in the FC row's in-plane coordinate frame.
    pub center_row_frame: [f64; 2],
    /// Exact radius in mm.
    pub radius_mm: f64,
    /// Constant cap-plane ordinate when present in every point.
    pub cap_ordinate_row_frame: Option<f64>,
    /// Number of points participating in validation.
    pub point_count: usize,
    /// Maximum absolute radial residual.
    pub max_residual: f64,
    /// Whether stored parameters match angular deltas around the circle.
    pub angle_parameter_consistent: bool,
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// Two or more topology-bound `fc 05` cap circles that establish one native
/// cylinder's radius and row-frame axis line, but not its model-space frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Fc05CylinderCapPair {
    /// Cylinder surface identifier shared by every cap edge.
    pub surface_id: u32,
    /// Curve identifiers of the agreeing cap circles in source order.
    pub curve_ids: Vec<u32>,
    /// Shared center in the owning feature's row frame.
    pub center_row_frame: [f64; 2],
    /// Shared exact radius in mm.
    pub radius_mm: f64,
    /// At least two distinct cap ordinates in the owning feature's row frame.
    pub cap_ordinates_row_frame: Vec<f64>,
    /// Byte offset of the first participating curve row.
    pub offset: usize,
}

/// Complete eight-slot pcurve endpoints from a labeled curve prototype.
#[derive(Debug, Clone, PartialEq)]
pub struct PrototypePcurveEndpoints {
    /// Prototype curve identifier.
    pub curve_id: u32,
    /// Endpoint A then B in schema face frame zero.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in schema face frame one.
    pub face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the `crv_pnt_arr` label in the original stream.
    pub offset: usize,
}

/// Four labeled topology references of a curve prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurvePrototypeTopology {
    /// Prototype curve identifier.
    pub curve_id: u32,
    /// Adjacent surface identifiers from `crv_hdr_geom_ptr[0/1]`.
    pub faces: [u32; 2],
    /// Per-face successor curve identifiers from `next_crv_hdr_ptr[0/1]`.
    pub next_edges: [u32; 2],
    /// Byte offset of the prototype namespace.
    pub offset: usize,
}

/// Prototype pcurve endpoints bound to their two labeled adjacent faces.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundPrototypePcurve {
    /// Prototype curve identifier.
    pub curve_id: u32,
    /// Adjacent face identifiers corresponding to UV frames zero and one.
    pub faces: [u32; 2],
    /// Endpoint A then B in the first face's UV frame.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in the second face's UV frame.
    pub face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source prototype pcurve.
    pub offset: usize,
}

/// Discover every labeled `crv_array` prototype. A label range ends at the
/// following `crv_array` label, so DEPDB-concatenated namespaces remain
/// independent.
pub fn prototypes(payload: &[u8]) -> Vec<CurvePrototype> {
    let mut result = Vec::new();
    let mut start = 0;
    while let Some(relative) = find(payload, b"crv_array\0", start) {
        let section_start = relative;
        start = relative + b"crv_array\0".len();
        let section_end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        let Some(id_label) = find_in(payload, b"crv_id\0", start, section_end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let (id, id_end) = compact_int(payload, id_start);
        if id_end == id_start {
            continue;
        }
        let Some(type_label) = find_in(payload, b"type\0", id_end, section_end) else {
            continue;
        };
        let Some(&type_byte) = payload.get(type_label + b"type\0".len()) else {
            continue;
        };
        let feature_id = find_in(payload, b"feat_id\0", id_end, section_end).and_then(|label| {
            let value_start = label + b"feat_id\0".len();
            let (value, end) = compact_int(payload, value_start);
            (end != value_start).then_some(value)
        });
        result.push(CurvePrototype {
            id,
            type_byte,
            feature_id,
            offset: section_start,
        });
    }
    result
}

/// Decode positional `crv_array` rows whose terminal
/// `<four canonical reference IDs> 00 00 e3 e1 e3` suffix has exactly one
/// possible boundary. Rows with ambiguous or malformed suffixes are not
/// returned; callers must preserve their enclosing section as unknown data.
pub fn topology_rows(payload: &[u8]) -> Vec<CurveTopologyRow> {
    let mut rows = framed_rows(payload)
        .into_iter()
        .filter_map(|row| parse_topology_row(&payload[row.start..row.end], row.start))
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.offset);
    rows.dedup_by_key(|row| row.offset);
    rows
}

#[derive(Debug, Clone, Copy)]
struct FramedRow {
    start: usize,
    end: usize,
}

fn row_terminator(payload: &[u8], start: usize, end: usize) -> Option<(usize, usize)> {
    let short = find_in(payload, b"\xe1\xe3", start, end).map(|offset| (offset, 2));
    let long = find_in(payload, b"\xe1\xf5\x05\xf6\xe3", start, end).map(|offset| (offset, 5));
    match (short, long) {
        (Some(left), Some(right)) => Some(if left.0 < right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn framed_segment(payload: &[u8], start: usize, end: usize) -> Option<FramedRow> {
    let segment = payload.get(start..end)?;
    let mut closes = segment
        .windows(3)
        .enumerate()
        .filter(|(_, bytes)| *bytes == [0, 0, 0xe3])
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    closes.reverse();
    for close in closes {
        let row_end = close + 3;
        let mut candidates = Vec::new();
        for row_start in 0..close {
            if parse_topology_row(&segment[row_start..row_end], start + row_start).is_some() {
                candidates.push(row_start);
            }
        }
        if candidates.len() == 1 {
            return Some(FramedRow {
                start: start + candidates[0],
                end: start + row_end,
            });
        }
    }
    None
}

fn framed_rows(payload: &[u8]) -> Vec<FramedRow> {
    let mut result = Vec::new();
    let mut arrays = Vec::new();
    let mut search = 0;
    while let Some(array) = find(payload, b"crv_array\0", search) {
        arrays.push(array + b"crv_array\0".len());
        search = array + b"crv_array\0".len();
    }
    if arrays.is_empty() {
        arrays.push(0);
    }
    for (index, &namespace_start) in arrays.iter().enumerate() {
        let namespace_end = arrays
            .get(index + 1)
            .map_or(payload.len(), |next| next - b"crv_array\0".len());
        let Some(label) = find_in(payload, b"topol_ref_data\0", namespace_start, namespace_end)
        else {
            continue;
        };
        let mut cursor = label + b"topol_ref_data\0".len();
        while let Some((terminator, length)) = row_terminator(payload, cursor, namespace_end) {
            if let Some(row) = framed_segment(payload, cursor, terminator) {
                result.push(row);
            }
            cursor = terminator + length;
        }
    }
    result.sort_by_key(|row| row.start);
    result.dedup_by_key(|row| row.start);
    result
}

fn suffix_candidates(row: &[u8], body_start: usize, close: usize) -> Vec<usize> {
    let mut candidates = Vec::new();
    for length in 4..=11 {
        let Some(start) = close
            .checked_sub(length)
            .filter(|start| *start >= body_start)
        else {
            continue;
        };
        let Ok((_, p1)) = reference_id(row, start) else {
            continue;
        };
        let Ok((_, p2)) = reference_id(row, p1) else {
            continue;
        };
        let Ok((_, p3)) = reference_id(row, p2) else {
            continue;
        };
        let Ok((_, end)) = reference_id(row, p3) else {
            continue;
        };
        if end == close {
            candidates.push(start);
        }
    }
    candidates
}

fn curve_scalar_lane(
    body: &[u8],
    type_byte: u8,
    cache: &scalar::ScalarCache,
) -> (Vec<f64>, Vec<u32>) {
    let mut values = Vec::new();
    let mut references = Vec::new();
    let mut cursor = 0;
    while cursor < body.len() {
        if body[cursor] == psb::token::ENTITY_REF {
            if let Ok((reference, next)) = reference_id(body, cursor + 1) {
                references.push(reference);
                cursor = next;
                continue;
            }
        }
        if body[cursor] == 0x18
            && cursor + 1 == body.len()
            && matches!(type_byte, 0x00 | 0x01 | 0x06 | 0x08)
            && values.len() < 8
        {
            values.push(0.0);
            cursor += 1;
            continue;
        }
        if let Some((value, next)) = scalar::decode_in_lane(body, cursor, cache) {
            values.push(value);
            cursor = next;
        } else {
            cursor += 1;
        }
    }
    (values, references)
}

/// Decode analytic bodies from positional curve rows, retaining ambiguous
/// suffix boundaries without asserting topology connectivity.
pub fn parameter_records(payload: &[u8]) -> Vec<CurveParameterRecord> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut records = Vec::new();
    for framed in framed_rows(payload) {
        let row = &payload[framed.start..framed.end];
        let (curve_id, after_id) = compact_int(row, 0);
        let Some(&type_byte) = row.get(after_id) else {
            continue;
        };
        let (_, after_feature) = compact_int(row, after_id + 1);
        let body_start = after_feature + 2;
        let Some(close) = row.len().checked_sub(3) else {
            continue;
        };
        if row.get(close..) != Some(&[0, 0, 0xe3]) || body_start > close {
            continue;
        }
        let candidates = suffix_candidates(row, body_start, close);
        let Some(&suffix_start) = candidates.first() else {
            continue;
        };
        let body = row[body_start..suffix_start].to_vec();
        let (scalar_values, skipped_references) = curve_scalar_lane(&body, type_byte, &cache);
        records.push(CurveParameterRecord {
            curve_id,
            type_byte,
            body,
            scalar_values,
            skipped_references,
            suffix: if candidates.len() == 1 {
                CurveSuffixStatus::Unique
            } else {
                CurveSuffixStatus::Ambiguous {
                    candidate_count: candidates.len(),
                }
            },
            offset: framed.start,
            body_offset: framed.start + body_start,
            suffix_offset: framed.start + suffix_start,
        });
    }
    records
}

/// Interpret complete eight-scalar parameter lanes for pcurve-family rows.
pub fn pcurve_endpoints(
    parameters: &[CurveParameterRecord],
    topology: &[CurveTopologyRow],
) -> Vec<PcurveEndpoints> {
    let mut result = parameters
        .iter()
        .filter(|record| matches!(record.type_byte, 0x00 | 0x01 | 0x06 | 0x08))
        .filter(|record| record.scalar_values.len() == 8)
        .filter_map(|record| {
            let topology = topology.iter().find(|row| row.id == record.curve_id)?;
            let values = &record.scalar_values;
            Some(PcurveEndpoints {
                curve_id: record.curve_id,
                faces: topology.faces,
                face_0_endpoints: [[values[0], values[1]], [values[4], values[5]]],
                face_1_endpoints: [[values[2], values[3]], [values[6], values[7]]],
                offset: record.offset,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode exact world-coordinate tokens from FC-prefixed dense curve bodies.
pub fn fc_control_points(parameters: &[CurveParameterRecord]) -> Vec<FcCurveControlPoints> {
    let mut result = Vec::new();
    for record in parameters {
        let Some((&0xfc, tail)) = record.body.split_first() else {
            continue;
        };
        let Some((&subtype, lane)) = tail.split_first() else {
            continue;
        };
        let mut values_mm = Vec::new();
        let mut cursor = 0;
        while cursor < lane.len() {
            if matches!(lane[cursor], 0x46 | 0x2d) {
                if let Some((value, next)) = scalar::decode(lane, cursor) {
                    values_mm.push(value);
                    cursor = next;
                    continue;
                }
            }
            cursor += 1;
        }
        if values_mm.len() >= 4 {
            result.push(FcCurveControlPoints {
                curve_id: record.curve_id,
                subtype,
                values_mm,
                offset: record.offset,
            });
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

fn fc05_scalar(body: &[u8], offset: usize) -> Option<(f64, usize)> {
    let prefix = *body.get(offset)?;
    if prefix == 0x18 {
        return Some((0.0, offset + 1));
    }
    if let Some(decoded) = scalar::decode(body, offset) {
        return Some(decoded);
    }
    if matches!(prefix, 0xe0..=0xe3 | 0xf7 | 0xf8) || offset + 7 > body.len() {
        return None;
    }
    let byte_1 = prefix.wrapping_sub(0x8b);
    let mut raw = [0; 8];
    raw[0] = if byte_1 >= 0x80 { 0x3f } else { 0x40 };
    raw[1] = byte_1;
    raw[2..].copy_from_slice(&body[offset + 1..offset + 7]);
    Some((f64::from_be_bytes(raw), offset + 7))
}

/// Validate FC05 point lanes against their exact circle identity.
pub fn fc05_circles(parameters: &[CurveParameterRecord]) -> Vec<Fc05Circle> {
    let mut circles = Vec::new();
    for record in parameters {
        if record.body.get(..2) != Some(&[0xfc, 0x05]) {
            continue;
        }
        let mut points = Vec::new();
        let mut cursor = 2;
        while cursor < record.body.len() {
            if !matches!(record.body[cursor], 0x46 | 0x2d) {
                break;
            }
            let Some((x, next)) = fc05_scalar(&record.body, cursor) else {
                break;
            };
            let Some((z, next)) = fc05_scalar(&record.body, next) else {
                break;
            };
            let Some((parameter, next)) = fc05_scalar(&record.body, next) else {
                break;
            };
            let Some((ordinate, next)) = fc05_scalar(&record.body, next) else {
                break;
            };
            points.push((x, z, parameter, ordinate));
            cursor = next;
        }
        if points.len() < 4 {
            continue;
        }
        let ordinate = points[0].3;
        if points.iter().any(|point| (point.3 - ordinate).abs() > 1e-9) {
            continue;
        }
        let first = points[0];
        let middle = points[points.len() / 2];
        let last = points[points.len() - 1];
        let a11 = 2.0 * (middle.0 - first.0);
        let a12 = 2.0 * (middle.1 - first.1);
        let a21 = 2.0 * (last.0 - middle.0);
        let a22 = 2.0 * (last.1 - middle.1);
        let determinant = a11.mul_add(a22, -(a12 * a21));
        if determinant.abs() < 1e-15 {
            continue;
        }
        let bx = middle.0.mul_add(middle.0, middle.1 * middle.1)
            - first.0.mul_add(first.0, first.1 * first.1);
        let bz = last.0.mul_add(last.0, last.1 * last.1)
            - middle.0.mul_add(middle.0, middle.1 * middle.1);
        let center_x = bx.mul_add(a22, -(a12 * bz)) / determinant;
        let center_z = a11.mul_add(bz, -(bx * a21)) / determinant;
        let radius = (first.0 - center_x).hypot(first.1 - center_z);
        if radius <= 0.0 {
            continue;
        }
        let residuals = points
            .iter()
            .map(|point| ((point.0 - center_x).hypot(point.1 - center_z) - radius).abs())
            .collect::<Vec<_>>();
        let max_residual = residuals.iter().copied().fold(0.0, f64::max);
        if max_residual > 1e-9 * radius.max(1.0) {
            continue;
        }
        let angle_0 = (first.1 - center_z).atan2(first.0 - center_x);
        let parameter_0 = first.2;
        let angle_parameter_consistent = points.iter().all(|point| {
            let mut angle = (point.1 - center_z).atan2(point.0 - center_x) - angle_0;
            while angle > std::f64::consts::PI {
                angle -= std::f64::consts::TAU;
            }
            while angle < -std::f64::consts::PI {
                angle += std::f64::consts::TAU;
            }
            (angle.abs() - (point.2 - parameter_0).abs() % std::f64::consts::TAU).abs() <= 1e-6
        });
        circles.push(Fc05Circle {
            curve_id: record.curve_id,
            center_row_frame: [center_x, center_z],
            radius_mm: radius,
            cap_ordinate_row_frame: Some(ordinate),
            point_count: points.len(),
            max_residual,
            angle_parameter_consistent,
            offset: record.offset,
        });
    }
    circles.sort_by_key(|circle| circle.offset);
    circles
}

/// Bind validated `fc 05` circles to typed cylinder/plane face pairs and retain
/// only groups that agree on radius and center at two distinct cap ordinates.
pub fn fc05_cylinder_cap_pairs(
    circles: &[Fc05Circle],
    topology: &[CurveTopologyRow],
    surfaces: &[crate::surface::SurfaceRow],
) -> Vec<Fc05CylinderCapPair> {
    use std::collections::BTreeMap;

    let kinds = surfaces
        .iter()
        .map(|surface| (surface.id, surface.kind))
        .collect::<BTreeMap<_, _>>();
    let faces = topology
        .iter()
        .map(|row| (row.id, row.faces))
        .collect::<BTreeMap<_, _>>();
    let mut groups = BTreeMap::<u32, Vec<&Fc05Circle>>::new();
    for circle in circles {
        let Some(adjacent) = faces.get(&circle.curve_id) else {
            continue;
        };
        let cylinders = adjacent
            .iter()
            .filter(|face| kinds.get(face) == Some(&crate::surface::SurfaceKind::Cylinder))
            .copied()
            .collect::<Vec<_>>();
        let plane_count = adjacent
            .iter()
            .filter(|face| kinds.get(face) == Some(&crate::surface::SurfaceKind::Plane))
            .count();
        if cylinders.len() == 1 && plane_count == 1 && circle.cap_ordinate_row_frame.is_some() {
            groups.entry(cylinders[0]).or_default().push(circle);
        }
    }

    let mut result = Vec::new();
    for (surface_id, mut group) in groups {
        group.sort_by_key(|circle| circle.offset);
        let first = group[0];
        let tolerance = 1e-9 * first.radius_mm.max(1.0);
        if !group.iter().all(|circle| {
            (circle.radius_mm - first.radius_mm).abs() <= tolerance
                && (circle.center_row_frame[0] - first.center_row_frame[0]).abs() <= tolerance
                && (circle.center_row_frame[1] - first.center_row_frame[1]).abs() <= tolerance
        }) {
            continue;
        }
        let mut ordinates = Vec::new();
        for ordinate in group
            .iter()
            .filter_map(|circle| circle.cap_ordinate_row_frame)
        {
            if ordinates
                .iter()
                .all(|existing: &f64| (*existing - ordinate).abs() > tolerance)
            {
                ordinates.push(ordinate);
            }
        }
        if ordinates.len() < 2 {
            continue;
        }
        result.push(Fc05CylinderCapPair {
            surface_id,
            curve_ids: group.iter().map(|circle| circle.curve_id).collect(),
            center_row_frame: first.center_row_frame,
            radius_mm: first.radius_mm,
            cap_ordinates_row_frame: ordinates,
            offset: first.offset,
        });
    }
    result.sort_by_key(|pair| pair.offset);
    result
}

/// Decode labeled `crv_pnt_arr f9 02 04` prototype pcurve endpoints.
pub fn prototype_pcurve_endpoints(payload: &[u8]) -> Vec<PrototypePcurveEndpoints> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(namespace) = find(payload, b"crv_array\0", search) {
        let start = namespace + b"crv_array\0".len();
        let end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        search = start;
        let Some(id_label) = find_in(payload, b"crv_id\0", start, end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let (curve_id, after_id) = compact_int(payload, id_start);
        if after_id == id_start {
            continue;
        }
        let prototype_end = find_in(payload, b"topol_ref_data\0", after_id, end).unwrap_or(end);
        let Some(points_label) = find_in(payload, b"crv_pnt_arr\0", after_id, prototype_end) else {
            continue;
        };
        let search_end = (points_label + 64).min(prototype_end);
        let Some(header) = find_in(
            payload,
            &[psb::token::SCALAR_BODY, 0x02, 0x04],
            points_label,
            search_end,
        ) else {
            continue;
        };
        let mut values = Vec::with_capacity(8);
        let mut cursor = header + 3;
        while cursor < prototype_end && values.len() < 8 {
            if let Some((value, next)) = scalar::decode_in_lane(payload, cursor, &cache) {
                values.push(value);
                cursor = next;
            } else {
                cursor += 1;
            }
        }
        if values.len() == 8 {
            result.push(PrototypePcurveEndpoints {
                curve_id,
                face_0_endpoints: [[values[0], values[1]], [values[4], values[5]]],
                face_1_endpoints: [[values[2], values[3]], [values[6], values[7]]],
                offset: points_label,
            });
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode the four labeled topology pointers of each curve prototype.
pub fn prototype_topology(payload: &[u8]) -> Vec<CurvePrototypeTopology> {
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(namespace) = find(payload, b"crv_array\0", search) {
        let start = namespace + b"crv_array\0".len();
        let end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        search = start;
        let Some(id_label) = find_in(payload, b"crv_id\0", start, end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let Ok((curve_id, _)) = reference_id(payload, id_start) else {
            continue;
        };
        let prototype_end = find_in(payload, b"topol_ref_data\0", id_start, end).unwrap_or(end);
        let reference = |label: &[u8]| {
            let at = find_in(payload, label, id_start, prototype_end)? + label.len();
            reference_id(payload, at).ok().map(|(value, _)| value)
        };
        let Some(face_0) = reference(b"crv_hdr_geom_ptr[0]\0") else {
            continue;
        };
        let Some(face_1) = reference(b"crv_hdr_geom_ptr[1]\0") else {
            continue;
        };
        let Some(next_0) = reference(b"next_crv_hdr_ptr[0]\0") else {
            continue;
        };
        let Some(next_1) = reference(b"next_crv_hdr_ptr[1]\0") else {
            continue;
        };
        result.push(CurvePrototypeTopology {
            curve_id,
            faces: [face_0, face_1],
            next_edges: [next_0, next_1],
            offset: namespace,
        });
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Bind complete prototype UV endpoints to labeled prototype topology.
pub fn bind_prototype_pcurves(
    pcurves: &[PrototypePcurveEndpoints],
    topology: &[CurvePrototypeTopology],
) -> Vec<BoundPrototypePcurve> {
    let mut result = pcurves
        .iter()
        .filter_map(|pcurve| {
            let topology = topology
                .iter()
                .find(|topology| topology.curve_id == pcurve.curve_id)?;
            Some(BoundPrototypePcurve {
                curve_id: pcurve.curve_id,
                faces: topology.faces,
                face_0_endpoints: pcurve.face_0_endpoints,
                face_1_endpoints: pcurve.face_1_endpoints,
                offset: pcurve.offset,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|record| record.offset);
    result
}

fn parse_topology_row(row: &[u8], absolute_offset: usize) -> Option<CurveTopologyRow> {
    let (id, after_id) = compact_int(row, 0);
    let type_byte = *row.get(after_id)?;
    let (feature_id, after_feature) = compact_int(row, after_id + 1);
    let directions = [*row.get(after_feature)?, *row.get(after_feature + 1)?];
    directions
        .iter()
        .all(|direction| matches!(direction, 0x01 | 0xf6))
        .then_some(())?;
    let close = row.len().checked_sub(3)?;
    (row.get(close..)? == [0, 0, 0xe3]).then_some(())?;
    let mut candidates = Vec::new();
    for length in 4..=11 {
        let Some(start) = close.checked_sub(length) else {
            continue;
        };
        let Ok((f0, p1)) = reference_id(row, start) else {
            continue;
        };
        let Ok((f1, p2)) = reference_id(row, p1) else {
            continue;
        };
        let Ok((e0, p3)) = reference_id(row, p2) else {
            continue;
        };
        let Ok((e1, end)) = reference_id(row, p3) else {
            continue;
        };
        if end == close && start >= after_feature + 2 {
            candidates.push([f0, f1, e0, e1]);
        }
    }
    (candidates.len() == 1).then_some(())?;
    let [f0, f1, e0, e1] = candidates[0];
    Some(CurveTopologyRow {
        id,
        type_byte,
        feature_id,
        directions,
        faces: [f0, f1],
        next_edges: [e0, e1],
        offset: absolute_offset,
    })
}

fn find(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

fn find_in(data: &[u8], needle: &[u8], from: usize, end: usize) -> Option<usize> {
    data.get(from..end)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_labeled_prototypes_in_concatenated_namespaces() {
        let payload = b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04\
                       crv_array\0crv_id\0\x80\x80type\0\x01";
        assert_eq!(
            prototypes(payload),
            vec![
                CurvePrototype {
                    id: 7,
                    type_byte: 8,
                    feature_id: Some(4),
                    offset: 0,
                },
                CurvePrototype {
                    id: 128,
                    type_byte: 1,
                    feature_id: None,
                    offset: 33,
                },
            ]
        );
    }

    #[test]
    fn ignores_incomplete_labeled_rows() {
        assert!(prototypes(b"crv_array\0crv_id\0\x07").is_empty());
    }

    #[test]
    fn decodes_a_uniquely_delimited_topology_suffix() {
        let payload = [
            b't', b'o', b'p', b'o', b'l', b'_', b'r', b'e', b'f', b'_', b'd', b'a', b't', b'a', 0,
            7, 8, 4, 1, 0xf6, 0x29, 0x43, 0, // opaque row body
            10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3,
        ];
        assert_eq!(
            topology_rows(&payload),
            vec![CurveTopologyRow {
                id: 7,
                type_byte: 8,
                feature_id: 4,
                directions: [1, 0xf6],
                faces: [10, 11],
                next_edges: [7, 7],
                offset: 15,
            }]
        );
    }

    #[test]
    fn binds_agreeing_fc05_caps_to_one_typed_cylinder() {
        let circle = |curve_id, ordinate, offset| Fc05Circle {
            curve_id,
            center_row_frame: [3.0, 4.0],
            radius_mm: 2.0,
            cap_ordinate_row_frame: Some(ordinate),
            point_count: 8,
            max_residual: 0.0,
            angle_parameter_consistent: true,
            offset,
        };
        let topology = |curve_id, plane_id, offset| CurveTopologyRow {
            id: curve_id,
            type_byte: 5,
            feature_id: 4,
            directions: [1, 0xf6],
            faces: [10, plane_id],
            next_edges: [curve_id, curve_id],
            offset,
        };
        let surface = |id, kind| crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 4,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture id fits usize"),
        };
        let pairs = fc05_cylinder_cap_pairs(
            &[circle(20, -5.0, 100), circle(21, 7.0, 200)],
            &[topology(20, 11, 100), topology(21, 12, 200)],
            &[
                surface(10, crate::surface::SurfaceKind::Cylinder),
                surface(11, crate::surface::SurfaceKind::Plane),
                surface(12, crate::surface::SurfaceKind::Plane),
            ],
        );

        assert_eq!(
            pairs,
            vec![Fc05CylinderCapPair {
                surface_id: 10,
                curve_ids: vec![20, 21],
                center_row_frame: [3.0, 4.0],
                radius_mm: 2.0,
                cap_ordinates_row_frame: vec![-5.0, 7.0],
                offset: 100,
            }]
        );
    }

    #[test]
    fn withholds_fc05_caps_without_distinct_ordinates() {
        let circles = [Fc05Circle {
            curve_id: 20,
            center_row_frame: [3.0, 4.0],
            radius_mm: 2.0,
            cap_ordinate_row_frame: Some(5.0),
            point_count: 8,
            max_residual: 0.0,
            angle_parameter_consistent: true,
            offset: 100,
        }];
        assert!(fc05_cylinder_cap_pairs(&circles, &[], &[]).is_empty());
    }
}
