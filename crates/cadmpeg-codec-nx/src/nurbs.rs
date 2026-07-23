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
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::wire::be::{u16_at as be_u16, u32_at as be_u32};

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
            (descriptor.dimension == 3).then_some(())?;
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
            if !(stride == 3 || stride == 4) || control.values.len() != descriptor.poles * stride {
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

fn arrays(bytes: &[u8]) -> Arrays {
    let mut out = Arrays::default();
    let mut duplicate_u16s = BTreeSet::new();
    let mut duplicate_f64s = BTreeSet::new();
    for (tag, width) in [(127, 2usize), (128, 8)] {
        for pos in 0..bytes.len().saturating_sub(7) {
            if bytes.get(pos..pos + 2) != Some(&[0, tag]) {
                continue;
            }
            let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
            if bytes.get(pos + 2 + escape..pos + 4 + escape) != Some(&[0, 0]) {
                continue;
            }
            let Some(count) = be_u16(bytes, pos + 4 + escape).map(usize::from) else {
                continue;
            };
            if !(1..4096).contains(&count) {
                continue;
            }
            let Some((reference, reference_len)) = read_xmt(bytes, pos + 6 + escape) else {
                continue;
            };
            if reference <= 5 {
                continue;
            }
            let data = pos + 6 + escape + reference_len;
            let Some(raw) = bytes.get(data..data + count * width) else {
                continue;
            };
            if tag == 127 {
                insert_unique(
                    &mut out.u16s,
                    &mut duplicate_u16s,
                    reference,
                    raw.chunks_exact(2)
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                        .collect(),
                );
            } else {
                let values: Vec<_> = raw
                    .chunks_exact(8)
                    .map(|b| {
                        f64::from_be_bytes(
                            b.try_into()
                                .expect("invariant: chunks_exact(8) yields exactly 8-byte slices"),
                        )
                    })
                    .collect();
                if values.iter().all(|value| value.is_finite()) {
                    insert_unique(&mut out.f64s, &mut duplicate_f64s, reference, values);
                }
            }
        }
    }
    out
}

#[derive(Clone)]
struct Payload {
    values: Vec<f64>,
}

fn surface_payloads(bytes: &[u8]) -> BTreeMap<u32, Payload> {
    let records = (0..bytes.len().saturating_sub(96)).filter_map(|pos| {
        (bytes.get(pos..pos + 2) == Some(&[0, 125])).then_some(())?;
        let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
        let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
        (xmt > 10).then_some(())?;
        let shift = escape + xmt_len - 2;
        let count_escape = usize::from(bytes.get(pos + 91 + shift) == Some(&0xff));
        let count_at = pos + 91 + shift + count_escape;
        let count = be_u32(bytes, count_at)? as usize;
        (count > 0 && count <= 0x40000).then_some(())?;
        let (_, first_len) = read_xmt(bytes, count_at + 4)?;
        let data = count_at + 4 + first_len;
        let raw = bytes.get(data..data + count * 8)?;
        let values: Vec<_> = raw
            .chunks_exact(8)
            .map(|b| {
                f64::from_be_bytes(
                    b.try_into()
                        .expect("invariant: chunks_exact(8) yields exactly 8-byte slices"),
                )
            })
            .collect();
        values
            .iter()
            .all(|value| value.is_finite())
            .then_some((xmt, Payload { values }))
    });
    unique_records(records)
}

fn curve_payloads(bytes: &[u8]) -> BTreeMap<u32, Payload> {
    let records = (0..bytes.len().saturating_sub(14)).filter_map(|pos| {
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
        let raw = bytes.get(data..data + count * 8)?;
        let values: Vec<_> = raw
            .chunks_exact(8)
            .map(|b| {
                f64::from_be_bytes(
                    b.try_into()
                        .expect("invariant: chunks_exact(8) yields exactly 8-byte slices"),
                )
            })
            .collect();
        values
            .iter()
            .all(|value| value.is_finite())
            .then_some((xmt, Payload { values }))
    });
    unique_records(records)
}

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
}

fn surface_descriptors(bytes: &[u8]) -> BTreeMap<u32, SurfaceDescriptor> {
    let records = (0..bytes.len().saturating_sub(47)).filter_map(|pos| {
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
        let (u_mult, v_mult, u_knots, v_knots) = if short {
            (
                u32::from(be_u16(bytes, pos + 36 + shift)?),
                u32::from(be_u16(bytes, pos + 38 + shift)?),
                u32::from(be_u16(bytes, pos + 40 + shift)?),
                u32::from(be_u16(bytes, pos + 42 + shift)?),
            )
        } else {
            (be_u16(bytes, pos + 54 + shift) == Some(125)).then_some(())?;
            let mut at = pos + 34 + shift;
            let mut refs = [0u32; 5];
            for reference in &mut refs {
                let (value, len) = read_xmt(bytes, at)?;
                *reference = value;
                at += len;
            }
            (at == pos + 54 + shift).then_some(())?;
            (refs[1], refs[2], refs[3], refs[4])
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
            },
        ))
    });
    unique_records(records)
}

struct CurveDescriptor {
    degree: u16,
    poles: usize,
    dimension: u16,
    distinct: usize,
    form: u8,
    mult: u32,
    knots: u32,
}

fn curve_descriptors(bytes: &[u8]) -> BTreeMap<u32, CurveDescriptor> {
    let records = (0..bytes.len().saturating_sub(26)).filter_map(|pos| {
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
            && matches!(dimension, 2 | 3)
            && (2..=2000).contains(&distinct)
            && [1, 4, 5, 6].contains(&form))
        .then_some(())?;
        let (mult, mult_len) = read_xmt(bytes, pos + 23 + shift)?;
        let (knots, _) = read_xmt(bytes, pos + 23 + shift + mult_len)?;
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
            },
        ))
    });
    unique_records(records)
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
