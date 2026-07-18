// SPDX-License-Identifier: Apache-2.0
//! Decode NURBS curves and surfaces from Parasolid neutral-binary records.
//!
//! The decoder joins descriptor, payload, knot, and multiplicity records by
//! their stream-scoped references. Control points are converted from metres to
//! millimetres. Invalid references, dimensions, knots, control points, and
//! weights cause the affected carrier to be omitted.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use crate::topology::Graph;
use cadmpeg_ir::be::{u16_at as be_u16, u32_at as be_u32};
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

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
            let descriptor = u16::try_from(refs[0]).ok()?;
            let payload = u16::try_from(refs[1]).ok()?;
            let descriptor = descriptors.get(&descriptor)?;
            let payload = payloads.get(&payload)?;
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
                control_points.push(Point3::new(
                    pole[0] * 1000.0 / weight,
                    pole[1] * 1000.0 / weight,
                    pole[2] * 1000.0 / weight,
                ));
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
            let descriptor = u16::try_from(refs[0]).ok()?;
            let control = u16::try_from(refs[1]).ok()?;
            let descriptor = descriptors.get(&descriptor)?;
            let control = controls.get(&control)?;
            let mult = arrays
                .u16s
                .get(&descriptor.mult)?
                .get(..descriptor.distinct)?;
            let distinct = arrays
                .f64s
                .get(&descriptor.knots)?
                .get(..descriptor.distinct)?;
            let knots = expand_knots(distinct, mult)?;
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
                control_points.push(Point3::new(
                    pole[0] * 1000.0 / weight,
                    pole[1] * 1000.0 / weight,
                    pole[2] * 1000.0 / weight,
                ));
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

#[derive(Default)]
struct Arrays {
    u16s: BTreeMap<u16, Vec<u16>>,
    f64s: BTreeMap<u16, Vec<f64>>,
}

fn arrays(bytes: &[u8]) -> Arrays {
    let mut out = Arrays::default();
    for (tag, width) in [(127, 2usize), (128, 8)] {
        for (pos, _) in records(bytes, tag, 8) {
            let Some(count) = be_u16(bytes, pos + 4).map(usize::from) else {
                continue;
            };
            let Some(reference) = be_u16(bytes, pos + 6) else {
                continue;
            };
            let Some(raw) = bytes.get(pos + 8..pos + 8 + count * width) else {
                continue;
            };
            if tag == 127 {
                out.u16s.insert(
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
                    out.f64s.insert(reference, values);
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

fn surface_payloads(bytes: &[u8]) -> BTreeMap<u16, Payload> {
    records(bytes, 125, 97)
        .into_iter()
        .filter_map(|(pos, xmt)| {
            let count = be_u32(bytes, pos + 91)? as usize;
            let raw = bytes.get(pos + 97..pos + 97 + count * 8)?;
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
        })
        .collect()
}

fn curve_payloads(bytes: &[u8]) -> BTreeMap<u16, Payload> {
    records(bytes, 135, 15)
        .into_iter()
        .filter_map(|(pos, xmt)| {
            let count = be_u32(bytes, pos + 9)? as usize;
            let raw = bytes.get(pos + 15..pos + 15 + count * 8)?;
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
        })
        .collect()
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
    u_mult: u16,
    v_mult: u16,
    u_knots: u16,
    v_knots: u16,
}

fn surface_descriptors(bytes: &[u8]) -> BTreeMap<u16, SurfaceDescriptor> {
    records(bytes, 126, 48)
        .into_iter()
        .filter_map(|(pos, xmt)| {
            (be_u16(bytes, pos + 44) == Some(125))
                .then(|| {
                    Some(SurfaceDescriptor {
                        u_degree: be_u16(bytes, pos + 6)?,
                        v_degree: be_u16(bytes, pos + 8)?,
                        u_count: be_u16(bytes, pos + 12)? as usize,
                        v_count: be_u16(bytes, pos + 16)? as usize,
                        u_form: *bytes.get(pos + 18)?,
                        v_form: *bytes.get(pos + 19)?,
                        u_distinct: be_u32(bytes, pos + 20)? as usize,
                        v_distinct: be_u32(bytes, pos + 24)? as usize,
                        u_mult: be_u16(bytes, pos + 36)?,
                        v_mult: be_u16(bytes, pos + 38)?,
                        u_knots: be_u16(bytes, pos + 40)?,
                        v_knots: be_u16(bytes, pos + 42)?,
                    })
                })
                .flatten()
                .map(|descriptor| (xmt, descriptor))
        })
        .collect()
}

struct CurveDescriptor {
    degree: u16,
    poles: usize,
    distinct: usize,
    form: u8,
    mult: u16,
    knots: u16,
}

fn curve_descriptors(bytes: &[u8]) -> BTreeMap<u16, CurveDescriptor> {
    records(bytes, 136, 27)
        .into_iter()
        .filter_map(|(pos, xmt)| {
            Some((
                xmt,
                CurveDescriptor {
                    degree: be_u16(bytes, pos + 4)?,
                    poles: be_u16(bytes, pos + 8)? as usize,
                    distinct: be_u16(bytes, pos + 14)? as usize,
                    form: *bytes.get(pos + 16)?,
                    mult: be_u16(bytes, pos + 23)?,
                    knots: be_u16(bytes, pos + 25)?,
                },
            ))
        })
        .collect()
}

fn records(bytes: &[u8], tag: u8, min_len: usize) -> Vec<(usize, u16)> {
    (0..bytes.len().saturating_sub(min_len - 1))
        .filter_map(|pos| {
            (bytes.get(pos..pos + 2) == Some(&[0, tag]))
                .then(|| be_u16(bytes, pos + 2).map(|xmt| (pos, xmt)))
                .flatten()
                .filter(|(_, xmt)| *xmt > 1 || tag == 127 || tag == 128)
        })
        .collect()
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
