// SPDX-License-Identifier: Apache-2.0
//! Decode NURBS curves and surfaces from Parasolid neutral-binary records.
//!
//! The decoder joins descriptor, payload, knot, and multiplicity records by
//! their stream-scoped references. Control points are converted from metres to
//! millimetres. Invalid references, dimensions, knots, control points, and
//! weights cause the affected carrier to be omitted.
#![deny(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};

use crate::topology::Graph;
use cadmpeg_ir::be::{f64_at as be_f64, u16_at as be_u16, u32_at as be_u32};
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3};

/// A decoded NURBS surface and its source descriptor offset.
#[derive(Debug, Clone)]
pub struct Surface {
    /// Byte offset of the tag-126 descriptor record within the input stream.
    pub pos: usize,
    /// Reconstructed surface geometry.
    pub geometry: SurfaceGeometry,
}

/// A decoded NURBS curve and its source descriptor offset.
#[derive(Debug, Clone)]
pub struct Curve {
    /// Byte offset of the tag-136 descriptor record within the input stream.
    pub pos: usize,
    /// Reconstructed curve geometry.
    pub geometry: CurveGeometry,
}

/// A decoded parameter-space NURBS curve and its source wrapper offset.
#[derive(Debug, Clone)]
pub struct Pcurve {
    /// Byte offset of the tag-134 wrapper record within the input stream.
    pub pos: usize,
    /// Reconstructed parameter-space geometry.
    pub geometry: PcurveGeometry,
}

/// Decode valid NURBS surface record families in source order.
///
/// The returned geometry uses millimetre control points. Malformed references,
/// knots, dimensions, control points, and weights are skipped.
pub fn surfaces(bytes: &[u8]) -> Vec<Surface> {
    let arrays = arrays(bytes);
    let payloads = surface_payloads(bytes);
    let descriptors = surface_descriptors(bytes);
    Graph::parse(bytes)
        .of_kind(124)
        .filter_map(|node| {
            let refs = node.compact_tail_references(2)?;
            let descriptor = descriptors.get(&refs[0])?;
            descriptor
                .payload
                .is_none_or(|payload| payload == refs[1])
                .then_some(())?;
            let payload = payloads.get(&refs[1])?;
            let u_mult = arrays.u16s.get(&descriptor.u_mult)?;
            let v_mult = arrays.u16s.get(&descriptor.v_mult)?;
            let u_knots = arrays.f64s.get(&descriptor.u_knots)?;
            let v_knots = arrays.f64s.get(&descriptor.v_knots)?;
            let u_mult = u_mult.get(..descriptor.u_distinct)?;
            let v_mult = v_mult.get(..descriptor.v_distinct)?;
            let u_knots = u_knots.get(..descriptor.u_distinct)?;
            let v_knots = v_knots.get(..descriptor.v_distinct)?;
            let full_u = expand_knots(u_knots, u_mult)?;
            let full_v = expand_knots(v_knots, v_mult)?;
            valid_basis(descriptor.u_degree, descriptor.u_count, &full_u)?;
            valid_basis(descriptor.v_degree, descriptor.v_count, &full_v)?;
            let poles = descriptor.u_count.checked_mul(descriptor.v_count)?;
            let stride = payload.values.len().checked_div(poles)?;
            if !(stride == 3 || stride == 4) || payload.values.len() != poles * stride {
                return None;
            }
            let mut control_points = Vec::new();
            let mut weights = (stride == 4).then(Vec::new);
            for pole in payload.values.chunks_exact(stride) {
                let weight = if stride == 4 { pole[3] } else { 1.0 };
                if !weight.is_finite() || weight == 0.0 {
                    return None;
                }
                control_points.push(weighted_mm_point(pole, weight)?);
                if let Some(weights) = &mut weights {
                    weights.push(weight);
                }
            }
            Some(Surface {
                pos: node.pos,
                geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                    u_degree: descriptor.u_degree as u32,
                    v_degree: descriptor.v_degree as u32,
                    u_knots: full_u,
                    v_knots: full_v,
                    u_count: descriptor.u_count as u32,
                    v_count: descriptor.v_count as u32,
                    control_points,
                    weights,
                    u_periodic: descriptor.u_form == 6,
                    v_periodic: descriptor.v_form == 6,
                }),
            })
        })
        .collect()
}

/// Decode dimension-2 `B_CURVE` families as surface parameter-space curves.
pub fn pcurves(bytes: &[u8]) -> Vec<Pcurve> {
    let arrays = arrays(bytes);
    let controls = curve_payloads(bytes);
    let descriptors = curve_descriptors(bytes);
    Graph::parse(bytes)
        .of_kind(134)
        .filter_map(|node| {
            let refs = node.compact_tail_references(2)?;
            let descriptor = descriptors.get(&refs[0])?;
            (descriptor.dimension == 2).then_some(())?;
            let control = controls.get(&refs[1])?;
            let mult = arrays
                .u16s
                .get(&descriptor.mult)?
                .get(..descriptor.distinct)?;
            let distinct = arrays
                .f64s
                .get(&descriptor.knots)?
                .get(..descriptor.distinct)?;
            let knots = expand_knots(distinct, mult)?;
            valid_basis(descriptor.degree, descriptor.poles, &knots)?;
            let stride = control.values.len().checked_div(descriptor.poles)?;
            if !(stride == 2 || stride == 3) || control.values.len() != descriptor.poles * stride {
                return None;
            }
            let mut control_points = Vec::new();
            let mut weights = (stride == 3).then(Vec::new);
            for pole in control.values.chunks_exact(stride) {
                let weight = if stride == 3 { pole[2] } else { 1.0 };
                if !weight.is_finite() || weight == 0.0 {
                    return None;
                }
                control_points.push(weighted_point2(pole, weight)?);
                if let Some(weights) = &mut weights {
                    weights.push(weight);
                }
            }
            Some(Pcurve {
                pos: node.pos,
                geometry: PcurveGeometry::Nurbs {
                    degree: descriptor.degree as u32,
                    knots,
                    control_points,
                    weights,
                    periodic: descriptor.form == 6,
                },
            })
        })
        .collect()
}

/// Decode valid NURBS curve record families in source order.
///
/// The returned geometry uses millimetre control points. Malformed references,
/// knots, dimensions, control points, and weights are skipped.
pub fn curves(bytes: &[u8]) -> Vec<Curve> {
    let arrays = arrays(bytes);
    let controls = curve_payloads(bytes);
    let descriptors = curve_descriptors(bytes);
    Graph::parse(bytes)
        .of_kind(134)
        .filter_map(|node| {
            let refs = node.compact_tail_references(2)?;
            let descriptor = descriptors.get(&refs[0])?;
            matches!(descriptor.dimension, 3 | 4).then_some(())?;
            let control = controls.get(&refs[1])?;
            let mult = arrays
                .u16s
                .get(&descriptor.mult)?
                .get(..descriptor.distinct)?;
            let distinct = arrays
                .f64s
                .get(&descriptor.knots)?
                .get(..descriptor.distinct)?;
            let knots = expand_knots(distinct, mult)?;
            valid_basis(descriptor.degree, descriptor.poles, &knots)?;
            let stride = control.values.len().checked_div(descriptor.poles)?;
            if !matches!((descriptor.dimension, stride), (3, 3 | 4) | (4, 4))
                || control.values.len() != descriptor.poles * stride
            {
                return None;
            }
            let mut control_points = Vec::new();
            let mut weights = (stride == 4).then(Vec::new);
            for pole in control.values.chunks_exact(stride) {
                let weight = if stride == 4 { pole[3] } else { 1.0 };
                if !weight.is_finite() || weight == 0.0 {
                    return None;
                }
                control_points.push(weighted_mm_point(pole, weight)?);
                if let Some(weights) = &mut weights {
                    weights.push(weight);
                }
            }
            Some(Curve {
                pos: node.pos,
                geometry: CurveGeometry::Nurbs(NurbsCurve {
                    degree: descriptor.degree as u32,
                    knots,
                    control_points,
                    weights,
                    periodic: descriptor.form == 6,
                }),
            })
        })
        .collect()
}

fn weighted_mm_point(pole: &[f64], weight: f64) -> Option<Point3> {
    let coordinates = [pole[0], pole[1], pole[2]].map(|value| value / weight * 1000.0);
    coordinates
        .into_iter()
        .all(f64::is_finite)
        .then(|| Point3::new(coordinates[0], coordinates[1], coordinates[2]))
}

fn weighted_point2(pole: &[f64], weight: f64) -> Option<Point2> {
    let coordinates = [pole[0] / weight, pole[1] / weight];
    coordinates
        .into_iter()
        .all(f64::is_finite)
        .then(|| Point2::new(coordinates[0], coordinates[1]))
}

#[derive(Default)]
struct Arrays {
    u16s: BTreeMap<u32, Vec<u16>>,
    f64s: BTreeMap<u32, Vec<f64>>,
}

enum ArrayValues {
    U16(Vec<u16>),
    F64(Vec<f64>),
}

struct ArrayRecord {
    reference: u32,
    end: usize,
    values: ArrayValues,
}

fn arrays(bytes: &[u8]) -> Arrays {
    let mut out = Arrays::default();
    let mut duplicate_u16s = BTreeSet::new();
    let mut duplicate_f64s = BTreeSet::new();
    for pos in 0..bytes.len().saturating_sub(7) {
        if let Some(record) = array_record_at(bytes, pos) {
            match record.values {
                ArrayValues::U16(values) => {
                    insert_unique(&mut out.u16s, &mut duplicate_u16s, record.reference, values);
                }
                ArrayValues::F64(values) => {
                    insert_unique(&mut out.f64s, &mut duplicate_f64s, record.reference, values);
                }
            }
        }
    }
    out
}

fn array_record_at(bytes: &[u8], pos: usize) -> Option<ArrayRecord> {
    let tag = *bytes.get(pos + 1)?;
    let width = match bytes.get(pos..pos + 2)? {
        [0, 127] => 2,
        [0, 128] => 8,
        _ => return None,
    };
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    (bytes.get(pos + 2 + escape..pos + 4 + escape) == Some(&[0, 0])).then_some(())?;
    let count = be_u16(bytes, pos + 4 + escape).map(usize::from)?;
    (1..4096).contains(&count).then_some(())?;
    let (reference, reference_len) = read_xmt(bytes, pos + 6 + escape)?;
    (reference > 5).then_some(())?;
    let data = pos + 6 + escape + reference_len;
    let end = data.checked_add(count.checked_mul(width)?)?;
    let raw = bytes.get(data..end)?;
    let values = if tag == 127 {
        ArrayValues::U16(
            raw.chunks_exact(2)
                .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
                .collect(),
        )
    } else {
        let values = raw
            .chunks_exact(8)
            .map(|bytes| {
                f64::from_be_bytes(
                    bytes
                        .try_into()
                        .expect("chunks_exact(8) yields eight-byte slices"),
                )
            })
            .collect::<Vec<_>>();
        values.iter().all(|value| value.is_finite()).then_some(())?;
        ArrayValues::F64(values)
    };
    Some(ArrayRecord {
        reference,
        end,
        values,
    })
}

#[derive(Clone)]
struct Payload {
    values: Vec<f64>,
}

fn surface_payloads(bytes: &[u8]) -> BTreeMap<u32, Payload> {
    let records = (0..bytes.len().saturating_sub(96))
        .filter_map(|pos| surface_payload_at(bytes, pos).map(|(xmt, payload, _)| (xmt, payload)));
    unique_records(records)
}

fn surface_payload_at(bytes: &[u8], pos: usize) -> Option<(u32, Payload, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 125])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    (xmt > 10).then_some(())?;
    let shift = escape + xmt_len - 2;
    let count_escape = usize::from(bytes.get(pos + 91 + shift) == Some(&0xff));
    let count_at = pos + 91 + shift + count_escape;
    let nested_same_record = (pos + 2..count_at).any(|candidate| {
        if bytes.get(candidate..candidate + 2) != Some(&[0, 125]) {
            return false;
        }
        let escape = usize::from(bytes.get(candidate + 2) == Some(&0xff));
        read_xmt(bytes, candidate + 2 + escape)
            .is_some_and(|(candidate_xmt, _)| candidate_xmt == xmt)
    });
    (!nested_same_record).then_some(())?;
    let count = be_u32(bytes, count_at)? as usize;
    (count > 0 && count <= 0x40000).then_some(())?;
    let (_, first_len) = read_xmt(bytes, count_at + 4)?;
    let data = count_at + 4 + first_len;
    let end = data.checked_add(count.checked_mul(8)?)?;
    let values = finite_f64_values(bytes.get(data..end)?)?;
    Some((xmt, Payload { values }, end))
}

fn surface_data_header_at(bytes: &[u8], pos: usize) -> Option<(u32, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 125])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    (xmt > 10).then_some(())?;
    let mut at = pos.checked_add(2 + escape + xmt_len)?;
    for _ in 0..8 {
        be_f64(bytes, at)?.is_finite().then_some(())?;
        at += 8;
    }
    let marker = usize::from(*bytes.get(at)?);
    matches!(marker, 1 | 2).then_some(())?;
    at += 1;
    let marker_lane = bytes.get(at..at.checked_add(12)?)?;
    let canonical_b_count = marker * 4;
    let canonical = marker_lane[..canonical_b_count]
        .iter()
        .all(|byte| *byte == b'B')
        && marker_lane[canonical_b_count..]
            .iter()
            .all(|byte| *byte == b'?');
    let extended_marker_one = marker == 1
        && marker_lane[..8].iter().all(|byte| *byte == b'B')
        && marker_lane[8..].iter().all(|byte| *byte == b'?');
    (canonical || extended_marker_one).then_some(())?;
    at += marker_lane.len();
    for _ in 0..4 {
        let (_, reference_len) = read_xmt(bytes, at)?;
        at += reference_len;
        (bytes.get(at) == Some(&1)).then_some(())?;
        at += 1;
    }
    Some((xmt, at))
}

fn curve_payloads(bytes: &[u8]) -> BTreeMap<u32, Payload> {
    let records = (0..bytes.len().saturating_sub(14))
        .filter_map(|pos| curve_payload_at(bytes, pos).map(|(xmt, payload, _)| (xmt, payload)));
    unique_records(records)
}

fn curve_payload_at(bytes: &[u8], pos: usize) -> Option<(u32, Payload, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 135])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    (xmt > 10).then_some(())?;
    let shift = escape + xmt_len - 2;
    let count_escape = usize::from(bytes.get(pos + 9 + shift) == Some(&0xff));
    let count_at = pos + 9 + shift + count_escape;
    let count = be_u32(bytes, count_at)? as usize;
    (count > 0 && count <= 0x40000).then_some(())?;
    let (_, control_ref_len) = read_xmt(bytes, count_at + 4)?;
    let data = count_at + 4 + control_ref_len;
    let end = data.checked_add(count.checked_mul(8)?)?;
    let values = finite_f64_values(bytes.get(data..end)?)?;
    Some((xmt, Payload { values }, end))
}

fn curve_data_header_at(bytes: &[u8], pos: usize) -> Option<(u32, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 135])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    (xmt > 10).then_some(())?;
    let mut at = pos.checked_add(2 + escape + xmt_len)?;
    matches!(bytes.get(at), Some(1 | 2)).then_some(())?;
    at += 1;
    let (_, reference_len) = read_xmt(bytes, at)?;
    at += reference_len;
    (bytes.get(at) == Some(&1)).then_some(())?;
    Some((xmt, at + 1))
}

fn finite_f64_values(raw: &[u8]) -> Option<Vec<f64>> {
    let values = raw
        .chunks_exact(8)
        .map(|bytes| {
            f64::from_be_bytes(
                bytes
                    .try_into()
                    .expect("chunks_exact(8) yields eight-byte slices"),
            )
        })
        .collect::<Vec<_>>();
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
}

#[derive(Clone, PartialEq, Eq)]
struct SurfaceDescriptor {
    u_degree: u16,
    v_degree: u16,
    u_count: usize,
    v_count: usize,
    u_form: u8,
    v_form: u8,
    u_distinct: usize,
    v_distinct: usize,
    u_mult: u32,
    v_mult: u32,
    u_knots: u32,
    v_knots: u32,
    payload: Option<u32>,
}

fn surface_descriptors(bytes: &[u8]) -> BTreeMap<u32, SurfaceDescriptor> {
    let records = (0..bytes.len().saturating_sub(47)).filter_map(|pos| {
        surface_descriptor_at(bytes, pos).map(|(xmt, descriptor, _)| (xmt, descriptor))
    });
    let mut descriptors = BTreeMap::<u32, SurfaceDescriptor>::new();
    let mut conflicts = BTreeSet::new();
    for (xmt, descriptor) in records {
        if conflicts.contains(&xmt) {
            continue;
        }
        let Some(current) = descriptors.remove(&xmt) else {
            descriptors.insert(xmt, descriptor);
            continue;
        };
        if current == descriptor {
            conflicts.insert(xmt);
            continue;
        }
        let mut current_basis = current.clone();
        let mut descriptor_basis = descriptor.clone();
        current_basis.payload = None;
        descriptor_basis.payload = None;
        if current_basis != descriptor_basis {
            conflicts.insert(xmt);
            continue;
        }
        let payload = match (current.payload, descriptor.payload) {
            (Some(left), Some(right)) if left != right => {
                conflicts.insert(xmt);
                continue;
            }
            (Some(payload), _) | (_, Some(payload)) => Some(payload),
            (None, None) => None,
        };
        descriptors.insert(xmt, SurfaceDescriptor { payload, ..current });
    }
    descriptors
}

fn surface_descriptor_at(bytes: &[u8], pos: usize) -> Option<(u32, SurfaceDescriptor, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 126])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    (xmt > 10).then_some(())?;
    let shift = escape + xmt_len - 2;
    let u_degree = be_u16(bytes, pos + 6 + shift)?;
    let v_degree = be_u16(bytes, pos + 8 + shift)?;
    let u_count = be_u16(bytes, pos + 12 + shift)? as usize;
    let v_count = be_u16(bytes, pos + 16 + shift)? as usize;
    let u_form = *bytes.get(pos + 18 + shift)?;
    let v_form = *bytes.get(pos + 19 + shift)?;
    let u_distinct = be_u32(bytes, pos + 20 + shift)? as usize;
    let v_distinct = be_u32(bytes, pos + 24 + shift)? as usize;
    ((1..=10).contains(&u_degree)
        && (1..=10).contains(&v_degree)
        && (2..=2000).contains(&u_count)
        && (2..=2000).contains(&v_count)
        && [1, 4, 5, 6].contains(&u_form)
        && [1, 4, 5, 6].contains(&v_form)
        && (2..2000).contains(&u_distinct)
        && (2..2000).contains(&v_distinct))
    .then_some(())?;
    let short = be_u16(bytes, pos + 44 + shift) == Some(125);
    let (u_mult, v_mult, u_knots, v_knots, payload, end) = if short {
        let payload_at = pos + 46 + shift;
        let (payload, payload_len) = read_enveloped_xmt(bytes, payload_at)?;
        (payload > 1).then_some(())?;
        (
            u32::from(be_u16(bytes, pos + 36 + shift)?),
            u32::from(be_u16(bytes, pos + 38 + shift)?),
            u32::from(be_u16(bytes, pos + 40 + shift)?),
            u32::from(be_u16(bytes, pos + 42 + shift)?),
            Some(payload),
            payload_at + payload_len,
        )
    } else {
        let mut at = pos + 34 + shift;
        let mut refs = [0u32; 5];
        for reference in &mut refs {
            let (value, len) = read_xmt(bytes, at)?;
            *reference = value;
            at += len;
        }
        if at == pos + 54 + shift && be_u16(bytes, at) == Some(125) {
            let payload_at = at + 2;
            let (payload, payload_len) = read_enveloped_xmt(bytes, payload_at)?;
            (payload > 1).then_some(())?;
            (
                refs[1],
                refs[2],
                refs[3],
                refs[4],
                Some(payload),
                payload_at + payload_len,
            )
        } else {
            at = pos + 34 + shift;
            for reference in &mut refs {
                let (value, len) = read_xmt(bytes, at)?;
                (value > 1).then_some(())?;
                *reference = value;
                at += len;
                (bytes.get(at) == Some(&0)).then_some(())?;
                at += 1;
            }
            (refs[1], refs[2], refs[3], refs[4], None, at)
        }
    };
    Some((
        xmt,
        SurfaceDescriptor {
            u_degree,
            v_degree,
            u_count,
            v_count,
            u_form,
            v_form,
            u_distinct,
            v_distinct,
            u_mult,
            v_mult,
            u_knots,
            v_knots,
            payload,
        },
        end,
    ))
}

#[derive(Clone, PartialEq, Eq)]
struct CurveDescriptor {
    degree: u16,
    poles: usize,
    dimension: u16,
    distinct: usize,
    form: u8,
    mult: u32,
    knots: u32,
    references: Vec<u32>,
}

fn curve_descriptors(bytes: &[u8]) -> BTreeMap<u32, CurveDescriptor> {
    let records = (0..bytes.len().saturating_sub(26)).filter_map(|pos| {
        curve_descriptor_at(bytes, pos, true).map(|(xmt, descriptor, _)| (xmt, descriptor))
    });
    let mut descriptors = BTreeMap::<u32, CurveDescriptor>::new();
    let mut conflicts = BTreeSet::new();
    for (xmt, descriptor) in records {
        if conflicts.contains(&xmt) {
            continue;
        }
        let Some(current) = descriptors.remove(&xmt) else {
            descriptors.insert(xmt, descriptor);
            continue;
        };
        if current == descriptor {
            conflicts.insert(xmt);
            continue;
        }
        let mut current_basis = current.clone();
        let mut descriptor_basis = descriptor.clone();
        current_basis.references.clear();
        descriptor_basis.references.clear();
        if current_basis == descriptor_basis {
            descriptors.insert(xmt, current);
        } else {
            conflicts.insert(xmt);
        }
    }
    descriptors
}

fn curve_descriptor_at(
    bytes: &[u8],
    pos: usize,
    allow_compact_fallback: bool,
) -> Option<(u32, CurveDescriptor, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 136])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    (xmt > 10).then_some(())?;
    let shift = escape + xmt_len - 2;
    let degree = be_u16(bytes, pos + 4 + shift)?;
    let poles = be_u16(bytes, pos + 8 + shift)? as usize;
    let dimension = be_u16(bytes, pos + 10 + shift)?;
    let distinct = be_u16(bytes, pos + 14 + shift)? as usize;
    let form = *bytes.get(pos + 16 + shift)?;
    ((1..=10).contains(&degree)
        && (2..=2000).contains(&poles)
        && matches!(dimension, 2..=4)
        && (2..=2000).contains(&distinct)
        && [1, 4, 5, 6].contains(&form))
    .then_some(())?;
    if matches!(
        bytes.get(pos + 17 + shift..pos + 21 + shift),
        Some([0, 0, 0, 1] | [0, 0, 1, 4])
    ) {
        let status_references = (|| {
            let mut at = pos + 21 + shift;
            let mut references = [0; 3];
            for reference in &mut references {
                let (value, consumed) = read_xmt(bytes, at)?;
                (value > 1).then_some(())?;
                *reference = value;
                at = at.checked_add(consumed)?;
                (bytes.get(at) == Some(&0)).then_some(())?;
                at = at.checked_add(1)?;
            }
            Some((references, at))
        })();
        if let Some((references, at)) = status_references {
            return Some((
                xmt,
                CurveDescriptor {
                    degree,
                    poles,
                    dimension,
                    distinct,
                    form,
                    mult: references[1],
                    knots: references[2],
                    references: references.to_vec(),
                },
                at,
            ));
        }
        allow_compact_fallback.then_some(())?;
    }
    let (mult, mult_len) = read_xmt(bytes, pos + 23 + shift)?;
    let (knots, knots_len) = read_xmt(bytes, pos + 23 + shift + mult_len)?;
    Some((
        xmt,
        CurveDescriptor {
            degree,
            poles,
            dimension,
            distinct,
            form,
            mult,
            knots,
            references: vec![mult, knots],
        },
        pos + 23 + shift + mult_len + knots_len,
    ))
}

/// Exact frame of one NURBS descriptor, payload, knot, or multiplicity record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuxiliaryRecord {
    /// Parasolid record type.
    pub(crate) kind: u16,
    /// Stream-local record identity.
    pub(crate) xmt: u32,
    /// Ordered stream-local references retained by the record.
    pub(crate) references: Vec<u32>,
    /// First byte after the complete record.
    pub(crate) end: usize,
}

/// Decode one complete NURBS auxiliary record at `pos`.
pub(crate) fn auxiliary_record_at(bytes: &[u8], pos: usize) -> Option<AuxiliaryRecord> {
    let kind = be_u16(bytes, pos)?;
    let (xmt, references, end) = match kind {
        125 => surface_payload_at(bytes, pos)
            .map(|(xmt, _, end)| (xmt, end))
            .or_else(|| surface_data_header_at(bytes, pos))
            .map(|(xmt, end)| (xmt, Vec::new(), end))?,
        126 => {
            let (xmt, _, end) = surface_descriptor_at(bytes, pos)?;
            (xmt, Vec::new(), end)
        }
        127 | 128 => {
            let record = array_record_at(bytes, pos)?;
            (record.reference, Vec::new(), record.end)
        }
        135 => curve_payload_at(bytes, pos)
            .map(|(xmt, _, end)| (xmt, end))
            .or_else(|| curve_data_header_at(bytes, pos))
            .map(|(xmt, end)| (xmt, Vec::new(), end))?,
        136 => {
            let (xmt, descriptor, end) = curve_descriptor_at(bytes, pos, false)?;
            (xmt, descriptor.references, end)
        }
        _ => return None,
    };
    Some(AuxiliaryRecord {
        kind,
        xmt,
        references,
        end,
    })
}

fn unique_records<T>(records: impl IntoIterator<Item = (u32, T)>) -> BTreeMap<u32, T> {
    let mut unique = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    for (xmt, record) in records {
        insert_unique(&mut unique, &mut duplicates, xmt, record);
    }
    unique
}

fn insert_unique<T>(
    records: &mut BTreeMap<u32, T>,
    duplicates: &mut BTreeSet<u32>,
    xmt: u32,
    record: T,
) {
    if duplicates.contains(&xmt) {
        return;
    }
    if records.insert(xmt, record).is_some() {
        records.remove(&xmt);
        duplicates.insert(xmt);
    }
}

fn read_xmt(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let first = i16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]);
    if first >= 0 {
        return Some((first as u32, 2));
    }
    let remainder = first.unsigned_abs();
    let quotient = u16::from_be_bytes([*bytes.get(at + 2)?, *bytes.get(at + 3)?]);
    Some((u32::from(quotient) * 32_767 + u32::from(remainder), 4))
}

fn read_enveloped_xmt(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let escape = usize::from(bytes.get(at) == Some(&0xff));
    let (value, len) = read_xmt(bytes, at + escape)?;
    Some((value, escape + len))
}

/// Codec-local ceiling on the total expanded knot count. Multiplicities are
/// attacker-controlled `u16` values with
/// no physical input floor of their own, so a hostile record can request a knot
/// vector of `distinct.len() * 65535` entries out of a few input bytes. This cap
/// bounds the `repeat_n`-style expansion (class A) independently of input size;
/// it is an algorithm fact retained as defense in depth, not a resource policy.
const MAX_KNOT_ENTRIES: usize = 1 << 20;

fn expand_knots(distinct: &[f64], multiplicities: &[u16]) -> Option<Vec<f64>> {
    if distinct.len() != multiplicities.len() || !distinct.windows(2).all(|pair| pair[0] <= pair[1])
    {
        return None;
    }
    // The explicit running cap prevents the expansion from committing
    // memory proportional to an untrusted multiplicity sum.
    let mut out = Vec::new();
    for (&value, &count) in distinct.iter().zip(multiplicities) {
        let count = count as usize;
        if out.len().saturating_add(count) > MAX_KNOT_ENTRIES {
            return None;
        }
        for _ in 0..count {
            out.push(value);
        }
    }
    Some(out)
}

fn valid_basis(degree: u16, control_count: usize, knots: &[f64]) -> Option<()> {
    let degree = usize::from(degree);
    let required_knots = control_count.checked_add(degree)?.checked_add(1)?;
    (control_count > degree && knots.len() == required_knots).then_some(())
}
