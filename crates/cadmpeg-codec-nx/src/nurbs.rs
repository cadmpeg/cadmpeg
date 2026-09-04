// SPDX-License-Identifier: Apache-2.0
//! Decode NURBS curves and surfaces from Parasolid neutral-binary records.
//!
//! The decoder joins descriptor, payload, knot, and multiplicity records by
//! their stream-scoped references. Control points are converted from metres to
//! millimetres. Invalid references, dimensions, knots, control points, and
//! weights cause the affected carrier to be omitted.
#![deny(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};

use crate::framing::read_xmt_width as read_xmt;
use crate::layout::nurbs_curve_descriptor_prefix as curve_desc;
use crate::layout::nurbs_surface_descriptor_prefix as surf_desc;
use crate::topology::Graph;
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{
    knots_nondecreasing, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, SurfaceGeometry,
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
    let graph = Graph::parse(bytes);
    decode_surfaces(&graph, &arrays, &payloads, &descriptors)
}

fn decode_surfaces(
    graph: &Graph,
    arrays: &Arrays<'_>,
    payloads: &BTreeMap<u32, Payload<'_>>,
    descriptors: &BTreeMap<u32, SurfaceDescriptor>,
) -> Vec<Surface> {
    graph
        .of_kind(124)
        .filter_map(|node| {
            let refs = node.compact_tail_references(2)?;
            let descriptor = descriptors.get(&refs[0])?;
            descriptor
                .payload
                .is_none_or(|payload| payload == refs[1])
                .then_some(())?;
            let payload = payloads.get(&refs[1])?;
            let poles = descriptor.u_count.checked_mul(descriptor.v_count)?;
            let value_count = payload.value_count();
            let stride = value_count.checked_div(poles)?;
            let expected_values = poles.checked_mul(stride)?;
            if !(stride == 3 || stride == 4) || value_count != expected_values {
                return None;
            }
            let u_mult = arrays
                .u16s
                .get(&descriptor.u_mult)?
                .u16_prefix(descriptor.u_distinct)?;
            let v_mult = arrays
                .u16s
                .get(&descriptor.v_mult)?
                .u16_prefix(descriptor.v_distinct)?;
            let u_knots = arrays
                .f64s
                .get(&descriptor.u_knots)?
                .f64_prefix(descriptor.u_distinct)?;
            let v_knots = arrays
                .f64s
                .get(&descriptor.v_knots)?
                .f64_prefix(descriptor.v_distinct)?;
            let full_u = expand_knots(
                &u_knots,
                &u_mult,
                required_knot_count(descriptor.u_degree, descriptor.u_count)?,
            )?;
            let full_v = expand_knots(
                &v_knots,
                &v_mult,
                required_knot_count(descriptor.v_degree, descriptor.v_count)?,
            )?;
            valid_basis(descriptor.u_degree, descriptor.u_count, &full_u)?;
            valid_basis(descriptor.v_degree, descriptor.v_count, &full_v)?;
            let mut control_points = Vec::new();
            let mut weights = (stride == 4).then(Vec::new);
            for pole_index in 0..poles {
                let base = pole_index.checked_mul(stride)?;
                let weight = if stride == 4 {
                    payload.value_at(base.checked_add(3)?)?
                } else {
                    1.0
                };
                if !weight.is_finite() || weight == 0.0 {
                    return None;
                }
                let pole = [
                    payload.value_at(base)?,
                    payload.value_at(base.checked_add(1)?)?,
                    payload.value_at(base.checked_add(2)?)?,
                    weight,
                ];
                control_points.push(weighted_mm_point(&pole, weight)?);
                if let Some(weights) = &mut weights {
                    weights.push(weight);
                }
            }
            Some(Surface {
                pos: node.pos,
                geometry: SurfaceGeometry::Nurbs(
                    NurbsSurface::new(
                        descriptor.u_degree as u32,
                        descriptor.v_degree as u32,
                        full_u,
                        full_v,
                        descriptor.u_count as u32,
                        descriptor.v_count as u32,
                        control_points,
                        weights,
                        node.byte_at(18)? == b'-',
                        descriptor.u_periodic,
                        descriptor.v_periodic,
                    )
                    .ok()?,
                ),
            })
        })
        .collect()
}

/// Decode dimension-2 `B_CURVE` families as surface parameter-space curves.
pub fn pcurves(bytes: &[u8]) -> Vec<Pcurve> {
    let arrays = arrays(bytes);
    let controls = curve_payloads(bytes);
    let descriptors = curve_descriptors(bytes);
    let graph = Graph::parse(bytes);
    decode_pcurves(&graph, &arrays, &controls, &descriptors)
}

fn decode_pcurves(
    graph: &Graph,
    arrays: &Arrays<'_>,
    controls: &BTreeMap<u32, Payload<'_>>,
    descriptors: &BTreeMap<u32, CurveDescriptor>,
) -> Vec<Pcurve> {
    graph
        .of_kind(134)
        .filter_map(|node| {
            let refs = node.compact_tail_references(2)?;
            let descriptor = descriptors.get(&refs[0])?;
            (descriptor.dimension == 2).then_some(())?;
            let control = controls.get(&refs[1])?;
            let value_count = control.value_count();
            let stride = value_count.checked_div(descriptor.poles)?;
            let expected_values = descriptor.poles.checked_mul(stride)?;
            if !(stride == 2 || stride == 3) || value_count != expected_values {
                return None;
            }
            let mult = arrays
                .u16s
                .get(&descriptor.mult)?
                .u16_prefix(descriptor.distinct)?;
            let distinct = arrays
                .f64s
                .get(&descriptor.knots)?
                .f64_prefix(descriptor.distinct)?;
            let knots = expand_knots(
                &distinct,
                &mult,
                required_knot_count(descriptor.degree, descriptor.poles)?,
            )?;
            valid_basis(descriptor.degree, descriptor.poles, &knots)?;
            let mut control_points = Vec::new();
            let mut weights = (stride == 3).then(Vec::new);
            for pole_index in 0..descriptor.poles {
                let base = pole_index.checked_mul(stride)?;
                let weight = if stride == 3 {
                    control.value_at(base.checked_add(2)?)?
                } else {
                    1.0
                };
                if !weight.is_finite() || weight == 0.0 {
                    return None;
                }
                let pole = [
                    control.value_at(base)?,
                    control.value_at(base.checked_add(1)?)?,
                    weight,
                ];
                control_points.push(weighted_point2(&pole, weight)?);
                if let Some(weights) = &mut weights {
                    weights.push(weight);
                }
            }
            Some(Pcurve {
                pos: node.pos,
                geometry: PcurveGeometry::Nurbs {
                    nurbs: cadmpeg_ir::geometry::PcurveNurbs::new(
                        descriptor.degree as u32,
                        knots,
                        control_points,
                        weights,
                        descriptor.periodic,
                    )
                    .ok()?,
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
    let graph = Graph::parse(bytes);
    decode_curves(&graph, &arrays, &controls, &descriptors)
}

fn decode_curves(
    graph: &Graph,
    arrays: &Arrays<'_>,
    controls: &BTreeMap<u32, Payload<'_>>,
    descriptors: &BTreeMap<u32, CurveDescriptor>,
) -> Vec<Curve> {
    graph
        .of_kind(134)
        .filter_map(|node| {
            let refs = node.compact_tail_references(2)?;
            let descriptor = descriptors.get(&refs[0])?;
            matches!(descriptor.dimension, 3 | 4).then_some(())?;
            let control = controls.get(&refs[1])?;
            let value_count = control.value_count();
            let stride = value_count.checked_div(descriptor.poles)?;
            let expected_values = descriptor.poles.checked_mul(stride)?;
            if !matches!((descriptor.dimension, stride), (3, 3 | 4) | (4, 4))
                || value_count != expected_values
            {
                return None;
            }
            let mult = arrays
                .u16s
                .get(&descriptor.mult)?
                .u16_prefix(descriptor.distinct)?;
            let distinct = arrays
                .f64s
                .get(&descriptor.knots)?
                .f64_prefix(descriptor.distinct)?;
            let knots = expand_knots(
                &distinct,
                &mult,
                required_knot_count(descriptor.degree, descriptor.poles)?,
            )?;
            valid_basis(descriptor.degree, descriptor.poles, &knots)?;
            let mut control_points = Vec::new();
            let mut weights = (stride == 4).then(Vec::new);
            for pole_index in 0..descriptor.poles {
                let base = pole_index.checked_mul(stride)?;
                let weight = if stride == 4 {
                    control.value_at(base.checked_add(3)?)?
                } else {
                    1.0
                };
                if !weight.is_finite() || weight == 0.0 {
                    return None;
                }
                let pole = [
                    control.value_at(base)?,
                    control.value_at(base.checked_add(1)?)?,
                    control.value_at(base.checked_add(2)?)?,
                    weight,
                ];
                control_points.push(weighted_mm_point(&pole, weight)?);
                if let Some(weights) = &mut weights {
                    weights.push(weight);
                }
            }
            Some(Curve {
                pos: node.pos,
                geometry: CurveGeometry::Nurbs(
                    NurbsCurve::new(
                        descriptor.degree as u32,
                        knots,
                        control_points,
                        weights,
                        descriptor.periodic,
                    )
                    .ok()?,
                ),
            })
        })
        .collect()
}

/// All NURBS geometry families decoded from one graph and one byte view.
///
/// The descriptor, payload, and array lanes are shared across the three
/// family decoders. Callers that already own the parsed topology graph use
/// this entry point to avoid rescanning the same byte view for each family.
#[derive(Debug, Default)]
pub(crate) struct Parsed {
    pub(crate) surfaces: Vec<Surface>,
    pub(crate) curves: Vec<Curve>,
    pub(crate) pcurves: Vec<Pcurve>,
}

pub(crate) fn parse_with_graph(bytes: &[u8], graph: &Graph) -> Parsed {
    let arrays = arrays(bytes);
    let surface_payloads = surface_payloads(bytes);
    let curve_payloads = curve_payloads(bytes);
    let surface_descriptors = surface_descriptors(bytes);
    let curve_descriptors = curve_descriptors(bytes);
    Parsed {
        surfaces: decode_surfaces(graph, &arrays, &surface_payloads, &surface_descriptors),
        curves: decode_curves(graph, &arrays, &curve_payloads, &curve_descriptors),
        pcurves: decode_pcurves(graph, &arrays, &curve_payloads, &curve_descriptors),
    }
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
struct Arrays<'a> {
    u16s: BTreeMap<u32, ArrayValues<'a>>,
    f64s: BTreeMap<u32, ArrayValues<'a>>,
}

#[derive(Clone, Copy)]
enum ArrayValues<'a> {
    U16(&'a [u8]),
    F64(&'a [u8]),
}

struct ArrayRecord<'a> {
    reference: u32,
    end: usize,
    values: ArrayValues<'a>,
}

impl ArrayValues<'_> {
    fn u16_prefix(&self, count: usize) -> Option<Vec<u16>> {
        let ArrayValues::U16(raw) = self else {
            return None;
        };
        let end = count.checked_mul(2)?;
        let raw = raw.get(..end)?;
        (0..count)
            .map(|index| View::u16_be_at(raw, index.checked_mul(2)?))
            .collect()
    }

    fn f64_prefix(&self, count: usize) -> Option<Vec<f64>> {
        let ArrayValues::F64(raw) = self else {
            return None;
        };
        let end = count.checked_mul(8)?;
        let raw = raw.get(..end)?;
        (0..count)
            .map(|index| {
                let value = View::f64_be_at(raw, index.checked_mul(8)?)?;
                value.is_finite().then_some(value)
            })
            .collect()
    }
}

fn arrays(bytes: &[u8]) -> Arrays<'_> {
    let mut out = Arrays::default();
    let mut duplicate_u16s = BTreeSet::new();
    let mut duplicate_f64s = BTreeSet::new();
    for pos in 0..bytes.len().saturating_sub(7) {
        if let Some(record) = array_record_at(bytes, pos) {
            match record.values {
                ArrayValues::U16(values) => {
                    insert_unique(
                        &mut out.u16s,
                        &mut duplicate_u16s,
                        record.reference,
                        ArrayValues::U16(values),
                    );
                }
                ArrayValues::F64(values) => {
                    insert_unique(
                        &mut out.f64s,
                        &mut duplicate_f64s,
                        record.reference,
                        ArrayValues::F64(values),
                    );
                }
            }
        }
    }
    out
}

fn array_record_at(bytes: &[u8], pos: usize) -> Option<ArrayRecord<'_>> {
    let tag = *bytes.get(pos + 1)?;
    let width = match bytes.get(pos..pos + 2)? {
        [0, 127] => 2,
        [0, 128] => 8,
        _ => return None,
    };
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    (bytes.get(pos + 2 + escape..pos + 4 + escape) == Some(&[0, 0])).then_some(())?;
    let count = View::u16_be_at(bytes, pos + 4 + escape).map(usize::from)?;
    (count > 0).then_some(())?;
    let (reference, reference_len) = read_xmt(bytes, pos + 6 + escape)?;
    (reference > 5).then_some(())?;
    let data = pos + 6 + escape + reference_len;
    let end = data.checked_add(count.checked_mul(width)?)?;
    let raw = bytes.get(data..end)?;
    let values = if tag == 127 {
        ArrayValues::U16(raw)
    } else {
        finite_f64_bytes(raw)?;
        ArrayValues::F64(raw)
    };
    Some(ArrayRecord {
        reference,
        end,
        values,
    })
}

#[derive(Clone, Copy)]
struct Payload<'a> {
    raw: &'a [u8],
}

impl Payload<'_> {
    fn value_count(self) -> usize {
        self.raw.len() / 8
    }

    fn value_at(self, index: usize) -> Option<f64> {
        View::f64_be_at(self.raw, index.checked_mul(8)?)
    }
}

fn surface_payloads(bytes: &[u8]) -> BTreeMap<u32, Payload<'_>> {
    let records = (0..bytes.len().saturating_sub(96))
        .filter_map(|pos| surface_payload_at(bytes, pos).map(|(xmt, payload, _)| (xmt, payload)));
    unique_records(records)
}

fn surface_payload_at(bytes: &[u8], pos: usize) -> Option<(u32, Payload<'_>, usize)> {
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
    let count = usize::try_from(View::u32_be_at(bytes, count_at)?).ok()?;
    (count > 0).then_some(())?;
    let (_, first_len) = read_xmt(bytes, count_at + 4)?;
    let data = count_at + 4 + first_len;
    let end = data.checked_add(count.checked_mul(8)?)?;
    let raw = bytes.get(data..end)?;
    finite_f64_bytes(raw)?;
    Some((xmt, Payload { raw }, end))
}

fn surface_data_header_at(bytes: &[u8], pos: usize) -> Option<(u32, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 125])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    (xmt > 10).then_some(())?;
    let mut at = pos.checked_add(2 + escape + xmt_len)?;
    for _ in 0..8 {
        View::f64_be_at(bytes, at)?.is_finite().then_some(())?;
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

fn curve_payloads(bytes: &[u8]) -> BTreeMap<u32, Payload<'_>> {
    let records = (0..bytes.len().saturating_sub(14))
        .filter_map(|pos| curve_payload_at(bytes, pos).map(|(xmt, payload, _)| (xmt, payload)));
    unique_records(records)
}

fn curve_payload_at(bytes: &[u8], pos: usize) -> Option<(u32, Payload<'_>, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 135])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    (xmt > 10).then_some(())?;
    let shift = escape + xmt_len - 2;
    let count_escape = usize::from(bytes.get(pos + 9 + shift) == Some(&0xff));
    let count_at = pos + 9 + shift + count_escape;
    let count = usize::try_from(View::u32_be_at(bytes, count_at)?).ok()?;
    (count > 0).then_some(())?;
    let (_, control_ref_len) = read_xmt(bytes, count_at + 4)?;
    let data = count_at + 4 + control_ref_len;
    let end = data.checked_add(count.checked_mul(8)?)?;
    let raw = bytes.get(data..end)?;
    finite_f64_bytes(raw)?;
    Some((xmt, Payload { raw }, end))
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

fn finite_f64_bytes(raw: &[u8]) -> Option<()> {
    raw.len().is_multiple_of(8).then_some(())?;
    for index in 0..raw.len() / 8 {
        let value = View::f64_be_at(raw, index.checked_mul(8)?)?;
        value.is_finite().then_some(())?;
    }
    Some(())
}

#[derive(Clone, PartialEq, Eq)]
struct SurfaceDescriptor {
    u_degree: u16,
    v_degree: u16,
    u_count: usize,
    v_count: usize,
    u_periodic: bool,
    v_periodic: bool,
    u_knot_type: u8,
    v_knot_type: u8,
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
    let u_periodic = logical_at(bytes, pos + surf_desc::U_PERIODIC + shift)?;
    let v_periodic = logical_at(bytes, pos + surf_desc::V_PERIODIC + shift)?;
    let u_degree = View::u16_be_at(bytes, pos + surf_desc::U_DEGREE + shift)?;
    let v_degree = View::u16_be_at(bytes, pos + surf_desc::V_DEGREE + shift)?;
    let u_count = usize::try_from(View::u32_be_at(
        bytes,
        pos + surf_desc::U_POLE_COUNT + shift,
    )?)
    .ok()?;
    let v_count = usize::try_from(View::u32_be_at(
        bytes,
        pos + surf_desc::V_POLE_COUNT + shift,
    )?)
    .ok()?;
    let u_knot_type = *bytes.get(pos + surf_desc::U_KNOT_TYPE + shift)?;
    let v_knot_type = *bytes.get(pos + surf_desc::V_KNOT_TYPE + shift)?;
    let u_distinct = usize::try_from(View::u32_be_at(
        bytes,
        pos + surf_desc::U_DISTINCT_KNOT_COUNT + shift,
    )?)
    .ok()?;
    let v_distinct = usize::try_from(View::u32_be_at(
        bytes,
        pos + surf_desc::V_DISTINCT_KNOT_COUNT + shift,
    )?)
    .ok()?;
    ((u_count > 0)
        && (v_count > 0)
        && valid_knot_type(u_knot_type)
        && valid_knot_type(v_knot_type)
        && (u_distinct > 0)
        && (v_distinct > 0))
        .then_some(())?;
    let short = View::u16_be_at(bytes, pos + 44 + shift) == Some(125);
    let (u_mult, v_mult, u_knots, v_knots, payload, end) = if short {
        let payload_at = pos + 46 + shift;
        let (payload, payload_len) = read_enveloped_xmt(bytes, payload_at)?;
        (payload > 1).then_some(())?;
        (
            u32::from(View::u16_be_at(bytes, pos + 36 + shift)?),
            u32::from(View::u16_be_at(bytes, pos + 38 + shift)?),
            u32::from(View::u16_be_at(bytes, pos + 40 + shift)?),
            u32::from(View::u16_be_at(bytes, pos + 42 + shift)?),
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
        if at == pos + 54 + shift && View::u16_be_at(bytes, at) == Some(125) {
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
            u_periodic,
            v_periodic,
            u_knot_type,
            v_knot_type,
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
    knot_type: u8,
    periodic: bool,
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
    let degree = View::u16_be_at(bytes, pos + curve_desc::DEGREE + shift)?;
    let poles = usize::try_from(View::u32_be_at(
        bytes,
        pos + curve_desc::POLE_COUNT + shift,
    )?)
    .ok()?;
    let dimension = View::u16_be_at(bytes, pos + curve_desc::DIMENSION + shift)?;
    let distinct = usize::try_from(View::u32_be_at(
        bytes,
        pos + curve_desc::DISTINCT_KNOT_COUNT + shift,
    )?)
    .ok()?;
    let knot_type = *bytes.get(pos + curve_desc::KNOT_TYPE + shift)?;
    let periodic = logical_at(bytes, pos + curve_desc::PERIODIC + shift)?;
    ((poles > 0) && matches!(dimension, 2..=4) && (distinct > 0) && valid_knot_type(knot_type))
        .then_some(())?;
    if matches!(
        bytes.get(pos + curve_desc::PERIODIC + shift..pos + curve_desc::LEN + shift),
        Some([0, 0, 0, 1] | [0, 0, 1, 4])
    ) {
        let status_references = (|| {
            let mut at = pos + curve_desc::LEN + shift;
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
                    knot_type,
                    periodic,
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
            knot_type,
            periodic,
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
    let kind = View::u16_be_at(bytes, pos)?;
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

fn read_enveloped_xmt(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let escape = usize::from(bytes.get(at) == Some(&0xff));
    let (value, len) = read_xmt(bytes, at + escape)?;
    Some((value, escape + len))
}

fn logical_at(bytes: &[u8], at: usize) -> Option<bool> {
    match bytes.get(at)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn valid_knot_type(value: u8) -> bool {
    (1..=6).contains(&value)
}

fn required_knot_count(degree: u16, control_count: usize) -> Option<usize> {
    control_count
        .checked_add(usize::from(degree))?
        .checked_add(1)
}

fn expand_knots(
    distinct: &[f64],
    multiplicities: &[u16],
    required_count: usize,
) -> Option<Vec<f64>> {
    if distinct.len() != multiplicities.len() || !knots_nondecreasing(distinct) {
        return None;
    }
    let expanded_count = multiplicities.iter().try_fold(0usize, |total, &count| {
        (count > 0).then_some(())?;
        total.checked_add(usize::from(count))
    })?;
    (expanded_count == required_count).then_some(())?;
    let mut out = Vec::new();
    for (&value, &count) in distinct.iter().zip(multiplicities) {
        for _ in 0..usize::from(count) {
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

#[cfg(test)]
mod tests;
