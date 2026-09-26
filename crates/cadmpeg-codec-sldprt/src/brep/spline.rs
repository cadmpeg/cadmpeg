// SPDX-License-Identifier: Apache-2.0
//! B-spline/list carrier tables.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::geometry::{
    nurbs::{knots_nondecreasing, NurbsCurve, NurbsSurface},
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;

use cadmpeg_core::decode::{DecodeContext, View};

use super::{CurveCarrier, SurfaceCarrier, LEN_TO_MM};

use crate::layout::bspline_array_header as arr_hdr;
use crate::layout::bspline_compact_array_header as compact_arr;
use crate::layout::bspline_surface_descriptor as surf_desc;

#[derive(Debug, Default)]
struct Arrays {
    f64s: HashMap<u16, Vec<f64>>,
    u16s: HashMap<u16, Vec<u16>>,
    compact: HashMap<u16, Vec<CompactArray>>,
}

#[derive(Debug, Clone, Copy)]
struct CompactArray {
    offset: usize,
    count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArraySpan {
    start: usize,
    count: usize,
}

#[derive(Debug)]
struct CurveDescriptor {
    degree: u32,
    control_count: usize,
    dimension: usize,
    control_attr: u16,
    multiplicity_attr: u16,
    knot_attr: u16,
}

#[derive(Debug, Clone, Copy)]
struct SurfaceDescriptor {
    attr: u16,
    u_periodic: bool,
    v_periodic: bool,
    u_degree: u32,
    v_degree: u32,
    u_count: usize,
    v_count: usize,
    u_knot_count: usize,
    v_knot_count: usize,
    rational: bool,
    dimension: usize,
    refs: [u16; 5],
}

// The body after the tag and optional envelope marker is fixed-width. It
// contains the attribute plus the complete NURBS_SURF definition and the
// five terminal array references.
const MAX_ARRAY_VALUES: usize = 1_000_000;

fn charge_items(
    ctx: Option<&DecodeContext<'_>>,
    count: usize,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(count as u64, operation)?;
    }
    Ok(())
}

fn logical_byte(bytes: &[u8], at: usize) -> Option<bool> {
    match bytes.get(at) {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}

fn parse_surface_descriptor(bytes: &[u8], off: usize) -> Option<SurfaceDescriptor> {
    if bytes.get(off..off + 2) != Some(&[0x00, 0x7e]) {
        return None;
    }
    let mut p = off + 2;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let end = p.checked_add(surf_desc::LEN)?;
    if end > bytes.len() {
        return None;
    }
    let attr = View::u16_be_at(bytes, p + surf_desc::ATTR)?;
    let u_periodic = logical_byte(bytes, p + surf_desc::U_PERIODIC)?;
    let v_periodic = logical_byte(bytes, p + surf_desc::V_PERIODIC)?;
    let u_degree = View::u16_be_at(bytes, p + surf_desc::U_DEGREE).map(u32::from)?;
    let v_degree = View::u16_be_at(bytes, p + surf_desc::V_DEGREE).map(u32::from)?;
    let u_degree_usize = usize::try_from(u_degree).ok()?;
    let v_degree_usize = usize::try_from(v_degree).ok()?;
    let u_count = usize::try_from(View::u32_be_at(bytes, p + surf_desc::U_POLE_COUNT)?).ok()?;
    let v_count = usize::try_from(View::u32_be_at(bytes, p + surf_desc::V_POLE_COUNT)?).ok()?;
    // Knot-type bytes, closure flags, and surface form have no IR fields, but
    // their positions are part of the fixed descriptor and are consumed here
    // so the following count and reference fields cannot slide into them.
    let _u_knot_type = *bytes.get(p + surf_desc::U_KNOT_TYPE)?;
    let _v_knot_type = *bytes.get(p + surf_desc::V_KNOT_TYPE)?;
    let u_knot_count = usize::try_from(View::u32_be_at(
        bytes,
        p + surf_desc::U_DISTINCT_KNOT_COUNT,
    )?)
    .ok()?;
    let v_knot_count = usize::try_from(View::u32_be_at(
        bytes,
        p + surf_desc::V_DISTINCT_KNOT_COUNT,
    )?)
    .ok()?;
    let rational = logical_byte(bytes, p + surf_desc::RATIONAL)?;
    let _u_closed = logical_byte(bytes, p + surf_desc::U_CLOSED)?;
    let _v_closed = logical_byte(bytes, p + surf_desc::V_CLOSED)?;
    let _surface_form = *bytes.get(p + surf_desc::SURFACE_FORM)?;
    let dimension = View::u16_be_at(bytes, p + surf_desc::VERTEX_DIM).map(usize::from)?;
    let refs = [
        View::u16_be_at(bytes, p + surf_desc::ARRAY_REFS)?,
        View::u16_be_at(bytes, p + surf_desc::ARRAY_REFS + 2)?,
        View::u16_be_at(bytes, p + surf_desc::ARRAY_REFS + 4)?,
        View::u16_be_at(bytes, p + surf_desc::ARRAY_REFS + 6)?,
        View::u16_be_at(bytes, p + surf_desc::ARRAY_REFS + 8)?,
    ];

    if attr <= 1
        || u_degree == 0
        || v_degree == 0
        || u_count <= u_degree_usize
        || v_count <= v_degree_usize
        || u_knot_count == 0
        || v_knot_count == 0
        || !matches!(dimension, 3 | 4)
        || rational != (dimension == 4)
        || refs.iter().any(|&reference| reference <= 1)
    {
        return None;
    }

    Some(SurfaceDescriptor {
        attr,
        u_periodic,
        v_periodic,
        u_degree,
        v_degree,
        u_count,
        v_count,
        u_knot_count,
        v_knot_count,
        rational,
        dimension,
        refs,
    })
}

fn array_body(bytes: &[u8], off: usize, tag: u8) -> Option<usize> {
    if bytes.get(off..off + 2) != Some(&[0x00, tag]) {
        return None;
    }
    let mut p = off + 2;
    if matches!(bytes.get(p), Some(0x2b | 0x2d)) {
        p += 1;
    }
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    Some(p)
}

fn scan_arrays(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    compact_attrs: Option<&HashSet<u16>>,
) -> Result<Arrays, cadmpeg_core::CodecError> {
    let mut arrays = Arrays::default();
    for off in 0..bytes.len().saturating_sub(9) {
        if bytes.get(off) == Some(&0) {
            let count = usize::from(bytes[off + compact_arr::COUNT]);
            if count > 0 {
                if let Some(attr) = View::u16_be_at(bytes, off + compact_arr::ATTR)
                    .filter(|attr| compact_attrs.is_some_and(|attrs| attrs.contains(attr)))
                {
                    charge_items(ctx, 1, "scan Parasolid compact arrays")?;
                    arrays
                        .compact
                        .entry(attr)
                        .or_default()
                        .push(CompactArray { offset: off, count });
                }
            }
        }
        let tag = match bytes.get(off..off + 2) {
            Some([0x00, tag @ (0x2d | 0x7f | 0x80)]) => *tag,
            _ => continue,
        };
        let Some(p) = array_body(bytes, off, tag) else {
            continue;
        };
        let Some(count) = View::u32_be_at(bytes, p + arr_hdr::COUNT).map(|v| v as usize) else {
            continue;
        };
        let Some(attr) = View::u16_be_at(bytes, p + arr_hdr::ATTR) else {
            continue;
        };
        if attr <= 1 || count > MAX_ARRAY_VALUES {
            continue;
        }
        let values_at = p + arr_hdr::LEN;
        if tag == 0x7f {
            if count
                .checked_mul(2)
                .and_then(|size| values_at.checked_add(size))
                .is_none_or(|end| end > bytes.len())
            {
                continue;
            }
            charge_items(ctx, count, "decode Parasolid integer array values")?;
            let Some(values) = (0..count)
                .map(|i| View::u16_be_at(bytes, values_at + i * 2))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            charge_items(ctx, 1, "collect Parasolid integer arrays")?;
            arrays.u16s.entry(attr).or_insert(values);
        } else {
            if count
                .checked_mul(8)
                .and_then(|size| values_at.checked_add(size))
                .is_none_or(|end| end > bytes.len())
            {
                continue;
            }
            charge_items(ctx, count, "decode Parasolid scalar array values")?;
            let Some(values) = (0..count)
                .map(|i| View::f64_be_at(bytes, values_at + i * 8))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            // The native surface format can reserve physical knot slots beyond
            // the descriptor's distinct-knot count. Their bits are not
            // semantic data when the matching multiplicities are zero, so
            // defer finite-value checks until descriptor binding.
            charge_items(ctx, 1, "collect Parasolid scalar arrays")?;
            arrays.f64s.entry(attr).or_insert(values);
        }
    }
    Ok(arrays)
}

fn compact_f64_arrays(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    arrays: &Arrays,
    attr: u16,
) -> Result<Vec<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut candidates = Vec::new();
    if let Some(values) = arrays.f64s.get(&attr) {
        charge_items(ctx, values.len(), "copy Parasolid scalar array values")?;
        charge_items(ctx, 1, "collect Parasolid scalar candidates")?;
        candidates.push(values.clone());
    }
    for compact in arrays.compact.get(&attr).into_iter().flatten() {
        if compact
            .count
            .checked_mul(8)
            .and_then(|size| compact.offset.checked_add(compact_arr::LEN + size))
            .is_none_or(|end| end > bytes.len())
        {
            continue;
        }
        charge_items(ctx, compact.count, "decode Parasolid compact scalar values")?;
        let Some(values) = (0..compact.count)
            .map(|index| View::f64_be_at(bytes, compact.offset + compact_arr::LEN + index * 8))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !candidates.contains(&values) {
            charge_items(ctx, 1, "collect Parasolid scalar candidates")?;
            candidates.push(values);
        }
    }
    Ok(candidates)
}

fn compact_u16_arrays(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    arrays: &Arrays,
    attr: u16,
) -> Result<Vec<Vec<u16>>, cadmpeg_core::CodecError> {
    let mut candidates = Vec::new();
    if let Some(values) = arrays.u16s.get(&attr) {
        charge_items(ctx, values.len(), "copy Parasolid integer array values")?;
        charge_items(ctx, 1, "collect Parasolid integer candidates")?;
        candidates.push(values.clone());
    }
    for compact in arrays.compact.get(&attr).into_iter().flatten() {
        if compact
            .count
            .checked_mul(2)
            .and_then(|size| compact.offset.checked_add(compact_arr::LEN + size))
            .is_none_or(|end| end > bytes.len())
        {
            continue;
        }
        charge_items(
            ctx,
            compact.count,
            "decode Parasolid compact integer values",
        )?;
        let Some(values) = (0..compact.count)
            .map(|index| View::u16_be_at(bytes, compact.offset + compact_arr::LEN + index * 2))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !candidates.contains(&values) {
            charge_items(ctx, 1, "collect Parasolid integer candidates")?;
            candidates.push(values);
        }
    }
    Ok(candidates)
}

fn exact_f64_array(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    arrays: &Arrays,
    attr: u16,
    count: usize,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut candidates = compact_f64_arrays(ctx, bytes, arrays, attr)?
        .into_iter()
        .filter(|values| values.len() == count);
    let Some(selected) = candidates.next() else {
        return Ok(None);
    };
    Ok(candidates
        .all(|candidate| candidate == selected)
        .then_some(selected))
}

fn scan_curve_descriptors(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
) -> Result<HashMap<u16, CurveDescriptor>, cadmpeg_core::CodecError> {
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(29) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x88]) {
            continue;
        }
        let mut p = off + 2;
        if bytes.get(p) == Some(&0xff) {
            p += 1;
        }
        let Some(attr) = View::u16_be_at(bytes, p) else {
            continue;
        };
        let Some(degree) = View::u16_be_at(bytes, p + 2).map(u32::from) else {
            continue;
        };
        let Some(control_count) = View::u32_be_at(bytes, p + 4).map(|v| v as usize) else {
            continue;
        };
        let Some(dimension) = View::u16_be_at(bytes, p + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(control_attr) = View::u16_be_at(bytes, p + 19) else {
            continue;
        };
        let Some(multiplicity_attr) = View::u16_be_at(bytes, p + 21) else {
            continue;
        };
        let Some(knot_attr) = View::u16_be_at(bytes, p + 23) else {
            continue;
        };
        if attr <= 1 || !(dimension == 3 || dimension == 4) || control_count == 0 {
            continue;
        }
        charge_items(ctx, 1, "collect Parasolid curve descriptors")?;
        out.entry(attr).or_insert(CurveDescriptor {
            degree,
            control_count,
            dimension,
            control_attr,
            multiplicity_attr,
            knot_attr,
        });
    }
    Ok(out)
}

fn curve_descriptor<'a>(
    bytes: &[u8],
    attr_at: usize,
    descriptors: &'a HashMap<u16, CurveDescriptor>,
) -> Option<&'a CurveDescriptor> {
    (attr_at + 2..(attr_at + 24).min(bytes.len().saturating_sub(1)))
        .filter_map(|at| View::u16_be_at(bytes, at))
        .find_map(|reference| descriptors.get(&reference))
}

/// Expand a compressed knot vector by its per-value multiplicities, refusing
/// the expansion when the running length exceeds `expected`.
///
/// A NURBS knot vector must have exactly `control_count + degree + 1` entries,
/// so `expected` is a hard upper bound. Charging the multiplicities against it
/// incrementally stops an untrusted `u16` multiplicity array (each entry up to
/// `65535`, over a million-entry table) from reserving a multi-hundred-gigabyte
/// `Vec` before the post-hoc length check would discard it. Returns `None` the
/// moment the accumulated length would exceed `expected`.
fn expanded_knots(
    ctx: Option<&DecodeContext<'_>>,
    values: &[f64],
    multiplicities: &[u16],
    expected: usize,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut out = Vec::new();
    for (value, &multiplicity) in values.iter().zip(multiplicities) {
        if multiplicity == 0 {
            continue;
        }
        let Some(next_len) = out.len().checked_add(multiplicity as usize) else {
            return Ok(None);
        };
        if next_len > expected {
            return Ok(None);
        }
        charge_items(ctx, usize::from(multiplicity), "expand Parasolid knots")?;
        out.extend(std::iter::repeat_n(*value, multiplicity as usize));
    }
    Ok(Some(out))
}

fn unique_knots(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut runs = Vec::<(f64, usize)>::new();
    for &knot in knots {
        if let Some((last, multiplicity)) = runs.last_mut() {
            if *last == knot {
                *multiplicity += 1;
                continue;
            }
        }
        runs.push((knot, 1));
    }
    runs.into_iter().unzip()
}

fn multiplicity_sum(values: &[u16]) -> Option<usize> {
    values
        .iter()
        .try_fold(0usize, |sum, &value| sum.checked_add(usize::from(value)))
}

fn array_span(bytes: &[u8], tag: u8, attr: u16) -> Option<(usize, usize)> {
    for off in 0..bytes.len().saturating_sub(9) {
        let Some(p) = array_body(bytes, off, tag) else {
            continue;
        };
        let Some(count) = View::u32_be_at(bytes, p + arr_hdr::COUNT).map(|value| value as usize)
        else {
            continue;
        };
        if View::u16_be_at(bytes, p + arr_hdr::ATTR) == Some(attr) {
            return Some((p + arr_hdr::LEN, count));
        }
    }
    None
}

fn array_spans(bytes: &[u8], arrays: &Arrays, tag: u8, attr: u16) -> Vec<ArraySpan> {
    let mut spans = Vec::new();
    for off in 0..bytes.len().saturating_sub(9) {
        let Some(p) = array_body(bytes, off, tag) else {
            continue;
        };
        let Some(count) = View::u32_be_at(bytes, p + arr_hdr::COUNT)
            .and_then(|value| usize::try_from(value).ok())
        else {
            continue;
        };
        if count <= MAX_ARRAY_VALUES && View::u16_be_at(bytes, p + arr_hdr::ATTR) == Some(attr) {
            spans.push(ArraySpan {
                start: p + arr_hdr::LEN,
                count,
            });
        }
    }
    spans.extend(
        arrays
            .compact
            .get(&attr)
            .into_iter()
            .flatten()
            .map(|array| ArraySpan {
                start: array.offset + compact_arr::LEN,
                count: array.count,
            }),
    );
    spans.sort_by_key(|span| (span.start, span.count));
    spans.dedup();
    spans
}

fn f64_values(bytes: &[u8], span: ArraySpan) -> Option<Vec<f64>> {
    (0..span.count)
        .map(|index| View::f64_be_at(bytes, span.start + index * 8))
        .collect()
}

fn u16_values(bytes: &[u8], span: ArraySpan) -> Option<Vec<u16>> {
    (0..span.count)
        .map(|index| View::u16_be_at(bytes, span.start + index * 2))
        .collect()
}

fn unique_control_span(
    bytes: &[u8],
    arrays: &Arrays,
    attr: u16,
    old_values: &[f64],
) -> Option<ArraySpan> {
    let spans = array_spans(bytes, arrays, 0x2d, attr)
        .into_iter()
        .filter(|span| span.count == old_values.len())
        .collect::<Vec<_>>();
    if spans.len() == 1 {
        return Some(spans[0]);
    }
    let mut matching = spans.into_iter().filter(|&span| {
        f64_values(bytes, span).is_some_and(|values| {
            values.iter().zip(old_values).all(|(native, expected)| {
                let scale = native.abs().max(expected.abs()).max(1.0);
                (native - expected).abs() <= 16.0 * f64::EPSILON * scale
            })
        })
    });
    let selected = matching.next()?;
    matching.next().is_none().then_some(selected)
}

fn unique_surface_knot_span(
    bytes: &[u8],
    arrays: &Arrays,
    knot_attr: u16,
    multiplicity_attr: u16,
    declared_count: usize,
    old_values: &[f64],
    old_multiplicities: &[usize],
) -> Option<ArraySpan> {
    if old_values.len() != declared_count || old_multiplicities.len() != declared_count {
        return None;
    }
    let knot_spans = array_spans(bytes, arrays, 0x80, knot_attr);
    let multiplicity_spans = array_spans(bytes, arrays, 0x7f, multiplicity_attr);
    let mut pairs = Vec::new();
    for knot_span in knot_spans {
        let Some(knots) = f64_values(bytes, knot_span) else {
            continue;
        };
        for multiplicity_span in multiplicity_spans
            .iter()
            .copied()
            .filter(|span| span.count == knot_span.count)
        {
            let Some(multiplicities) = u16_values(bytes, multiplicity_span) else {
                continue;
            };
            let Some((knots, multiplicities)) =
                surface_knot_arrays(&knots, &multiplicities, declared_count)
            else {
                continue;
            };
            if knots == old_values
                && multiplicities
                    .iter()
                    .copied()
                    .map(usize::from)
                    .eq(old_multiplicities.iter().copied())
            {
                pairs.push((knot_span, multiplicity_span));
            }
        }
    }
    pairs.sort_by_key(|(knots, multiplicities)| (knots.start, knots.count, multiplicities.start));
    pairs.dedup();
    match pairs.as_slice() {
        [(knots, _)] => Some(*knots),
        _ => None,
    }
}

fn patch_f64_span(bytes: &mut [u8], span: ArraySpan, values: &[f64]) -> Option<()> {
    if values.len() > span.count {
        return None;
    }
    for (index, value) in values.iter().enumerate() {
        bytes
            .get_mut(span.start + index * 8..span.start + (index + 1) * 8)?
            .copy_from_slice(&value.to_be_bytes());
    }
    Some(())
}

fn patch_f64_array(bytes: &mut [u8], tag: u8, attr: u16, values: &[f64]) -> Option<()> {
    let (start, count) = array_span(bytes, tag, attr)?;
    if count != values.len() {
        return None;
    }
    for (index, value) in values.iter().enumerate() {
        bytes
            .get_mut(start + index * 8..start + (index + 1) * 8)?
            .copy_from_slice(&value.to_be_bytes());
    }
    Some(())
}

fn homogeneous_poles(points: &[Point3], weights: Option<&[f64]>, scale: f64) -> Option<Vec<f64>> {
    if weights.is_some_and(|values| values.len() != points.len()) {
        return None;
    }
    let mut out = Vec::with_capacity(points.len() * if weights.is_some() { 4 } else { 3 });
    for (index, point) in points.iter().enumerate() {
        let weight = weights.map_or(1.0, |values| values[index]);
        if weight.abs() <= f64::EPSILON {
            return None;
        }
        let homogeneous = [point.x, point.y, point.z].map(|value| value * scale * weight);
        if !homogeneous.iter().all(|value| value.is_finite()) {
            return None;
        }
        out.extend(homogeneous);
        if weights.is_some() {
            out.push(weight);
        }
    }
    Some(out)
}

/// Patch retained curve pole and knot arrays without changing their storage shape.
pub(crate) fn patch_nurbs_curve(
    bytes: &mut [u8],
    wrapper_offset: usize,
    old: &NurbsCurve,
    new: &NurbsCurve,
    scale: f64,
) -> Option<()> {
    if old.degree() != new.degree()
        || old.control_points().len() != new.control_points().len()
        || old.weights().is_some() != new.weights().is_some()
        || old.periodic() != new.periodic()
    {
        return None;
    }
    let (old_unique, old_mult) = unique_knots(old.knots());
    let (new_unique, new_mult) = unique_knots(new.knots());
    if old_mult != new_mult || old_unique.len() != new_unique.len() {
        return None;
    }
    let mut p = wrapper_offset + 2;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let descriptors = scan_curve_descriptors(None, bytes).ok()?;
    let descriptor = curve_descriptor(bytes, p, &descriptors)?;
    if descriptor.degree != old.degree()
        || descriptor.control_count != old.control_points().len()
        || descriptor.dimension != if old.weights().is_some() { 4 } else { 3 }
    {
        return None;
    }
    let control_points = new.pole_rows().raw_points();
    let weights = new.pole_rows().weights();
    let poles = homogeneous_poles(&control_points, weights.as_deref(), scale)?;
    patch_f64_array(bytes, 0x2d, descriptor.control_attr, &poles)?;
    patch_f64_array(bytes, 0x80, descriptor.knot_attr, &new_unique)
}

/// Patch retained surface pole and knot arrays without changing their storage shape.
pub(crate) fn patch_nurbs_surface(
    bytes: &mut [u8],
    wrapper_offset: usize,
    old: &NurbsSurface,
    new: &NurbsSurface,
    scale: f64,
) -> Option<()> {
    if old.u_degree() != new.u_degree()
        || old.v_degree() != new.v_degree()
        || old.u_count() != new.u_count()
        || old.v_count() != new.v_count()
        || old.poles().len() != new.poles().len()
        || old.weights().is_some() != new.weights().is_some()
        || old.u_periodic() != new.u_periodic()
        || old.v_periodic() != new.v_periodic()
    {
        return None;
    }
    let (old_u, old_u_mult) = unique_knots(old.u_knots());
    let (new_u, new_u_mult) = unique_knots(new.u_knots());
    let (old_v, old_v_mult) = unique_knots(old.v_knots());
    let (new_v, new_v_mult) = unique_knots(new.v_knots());
    if old_u_mult != new_u_mult
        || old_v_mult != new_v_mult
        || old_u.len() != new_u.len()
        || old_v.len() != new_v.len()
    {
        return None;
    }
    let descriptors = scan_surface_descriptors(None, bytes).ok()?;
    let compact_attrs = descriptors
        .values()
        .flat_map(|descriptor| descriptor.refs)
        .collect();
    let arrays = scan_arrays(None, bytes, Some(&compact_attrs)).ok()?;
    let mut p = wrapper_offset + 2;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let descriptor_attr = View::u16_be_at(bytes, p + 17)?;
    let descriptor = descriptors.get(&descriptor_attr)?;
    let [control_attr, _, _, u_knot_attr, v_knot_attr] = descriptor.refs;
    let dimension = if old.weights().is_some() { 4 } else { 3 };
    if descriptor.u_degree != old.u_degree()
        || descriptor.v_degree != old.v_degree()
        || descriptor.u_count != old.u_count()
        || descriptor.v_count != old.v_count()
        || descriptor.dimension != dimension
        || descriptor.u_periodic != old.u_periodic()
        || descriptor.v_periodic != old.v_periodic()
    {
        return None;
    }
    let old_points = old.pole_grid().raw_points().concat();
    let old_weights = old.pole_grid().weights().map(|rows| rows.concat());
    let old_poles = homogeneous_poles(&old_points, old_weights.as_deref(), scale)?;
    let points = new.pole_grid().raw_points().concat();
    let weights = new.pole_grid().weights().map(|rows| rows.concat());
    let poles = homogeneous_poles(&points, weights.as_deref(), scale)?;
    let control_span = unique_control_span(bytes, &arrays, control_attr, &old_poles)?;
    let u_knot_span = unique_surface_knot_span(
        bytes,
        &arrays,
        u_knot_attr,
        descriptor.refs[1],
        descriptor.u_knot_count,
        &old_u,
        &old_u_mult,
    )?;
    let v_knot_span = unique_surface_knot_span(
        bytes,
        &arrays,
        v_knot_attr,
        descriptor.refs[2],
        descriptor.v_knot_count,
        &old_v,
        &old_v_mult,
    )?;
    patch_f64_span(bytes, control_span, &poles)?;
    patch_f64_span(bytes, u_knot_span, &new_u)?;
    patch_f64_span(bytes, v_knot_span, &new_v)
}

/// Every compact NURBS curve carrier in `bytes`, keyed by attribute id.
///
/// A candidate offset whose bytes are not a spline record is not a refusal: the
/// scan sweeps every offset. A record whose poles, knots and weights do parse
/// but do not pair is one, and it is recorded in `refusals` as a loss naming
/// the attribute id it belongs to, so the decode reports it rather than
/// deleting the carrier in silence.
pub(crate) fn scan_curve_carriers(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    refusals: &mut Vec<LossNote>,
) -> Result<HashMap<u16, CurveCarrier>, cadmpeg_core::CodecError> {
    let arrays = scan_arrays(ctx, bytes, None)?;
    let descriptors = scan_curve_descriptors(ctx, bytes)?;
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(6) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x86]) {
            continue;
        }
        let mut p = off + 2;
        if bytes.get(p) == Some(&0xff) {
            p += 1;
        }
        let Some(attr) = View::u16_be_at(bytes, p) else {
            continue;
        };
        let Some(descriptor) = curve_descriptor(bytes, p, &descriptors) else {
            continue;
        };
        let Some(control) = arrays.f64s.get(&descriptor.control_attr) else {
            continue;
        };
        let Some(multiplicities) = arrays.u16s.get(&descriptor.multiplicity_attr) else {
            continue;
        };
        let Some(unique_knots) = arrays.f64s.get(&descriptor.knot_attr) else {
            continue;
        };
        let Some(expected_control_values) =
            descriptor.control_count.checked_mul(descriptor.dimension)
        else {
            continue;
        };
        if control.len() != expected_control_values {
            continue;
        }
        if !unique_knots.iter().all(|value| value.is_finite()) || !knots_nondecreasing(unique_knots)
        {
            continue;
        }
        charge_items(
            ctx,
            descriptor.control_count,
            "decode Parasolid curve poles",
        )?;
        let mut points = Vec::with_capacity(descriptor.control_count);
        if descriptor.dimension == 4 {
            charge_items(
                ctx,
                descriptor.control_count,
                "decode Parasolid curve weights",
            )?;
        }
        let mut weights = (descriptor.dimension == 4).then(Vec::new);
        for pole in control.chunks_exact(descriptor.dimension) {
            if pole.iter().any(|value| !value.is_finite()) {
                points.clear();
                break;
            }
            let weight = if descriptor.dimension == 4 {
                pole[3]
            } else {
                1.0
            };
            if !weight.is_finite() || weight.abs() <= f64::EPSILON {
                points.clear();
                break;
            }
            points.push(Point3::new(
                pole[0] / weight * LEN_TO_MM,
                pole[1] / weight * LEN_TO_MM,
                pole[2] / weight * LEN_TO_MM,
            ));
            if let Some(values) = &mut weights {
                values.push(weight);
            }
        }
        if points.len() != descriptor.control_count {
            continue;
        }
        let expected = points.len() + descriptor.degree as usize + 1;
        let Some(knots) = expanded_knots(ctx, unique_knots, multiplicities, expected)? else {
            continue;
        };
        if knots.len() != expected {
            continue;
        }
        if weights.is_some() {
            charge_items(ctx, points.len(), "pair Parasolid curve weighted poles")?;
        }
        charge_items(ctx, points.len(), "admit Parasolid curve poles")?;
        let nurbs = match NurbsCurve::from_lanes(descriptor.degree, knots, points, weights, false) {
            Ok(nurbs) => nurbs,
            Err(error) => {
                charge_items(ctx, 1, "collect Parasolid spline refusals")?;
                refusals.push(crate::loss::spline_lane_refusal(&format!(
                    "curve carrier attribute {attr}: {error}"
                )));
                continue;
            }
        };
        charge_items(ctx, 1, "collect Parasolid curve carriers")?;
        out.entry(attr).or_insert(CurveCarrier {
            attr,
            offset: off,
            end: off + 2,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
            parameter_range: None,
        });
    }
    Ok(out)
}

fn scan_surface_descriptors(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
) -> Result<HashMap<u16, SurfaceDescriptor>, cadmpeg_core::CodecError> {
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(1) {
        let Some(descriptor) = parse_surface_descriptor(bytes, off) else {
            continue;
        };
        charge_items(ctx, 1, "collect Parasolid surface descriptors")?;
        out.entry(descriptor.attr).or_insert(descriptor);
    }
    Ok(out)
}

fn surface_knot_arrays<'a>(
    unique: &'a [f64],
    multiplicities: &'a [u16],
    declared_count: usize,
) -> Option<(&'a [f64], &'a [u16])> {
    if declared_count == 0 || unique.len() != multiplicities.len() || unique.len() < declared_count
    {
        return None;
    }
    (multiplicities[..declared_count]
        .iter()
        .all(|&value| value != 0)
        && multiplicities[declared_count..]
            .iter()
            .all(|&value| value == 0))
    .then_some((&unique[..declared_count], &multiplicities[..declared_count]))
}

fn surface_knot_values(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    arrays: &Arrays,
    knot_attr: u16,
    multiplicity_attr: u16,
    declared_count: usize,
) -> Result<Option<(Vec<f64>, Vec<u16>)>, cadmpeg_core::CodecError> {
    let mut resolved = Vec::<(Vec<f64>, Vec<u16>)>::new();
    let mut multiplicities_by_count = HashMap::<usize, Vec<Vec<u16>>>::new();
    for multiplicities in compact_u16_arrays(ctx, bytes, arrays, multiplicity_attr)? {
        charge_items(ctx, 1, "group Parasolid knot multiplicities")?;
        multiplicities_by_count
            .entry(multiplicities.len())
            .or_default()
            .push(multiplicities);
    }
    for unique in compact_f64_arrays(ctx, bytes, arrays, knot_attr)? {
        let Some(multiplicity_candidates) = multiplicities_by_count.get(&unique.len()) else {
            continue;
        };
        for multiplicities in multiplicity_candidates {
            let Some((unique, multiplicities)) =
                surface_knot_arrays(&unique, multiplicities, declared_count)
            else {
                continue;
            };
            charge_items(ctx, unique.len(), "copy Parasolid distinct knots")?;
            charge_items(
                ctx,
                multiplicities.len(),
                "copy Parasolid knot multiplicities",
            )?;
            let candidate = (unique.to_vec(), multiplicities.to_vec());
            if !resolved.contains(&candidate) {
                charge_items(ctx, 1, "collect Parasolid knot candidates")?;
                resolved.push(candidate);
            }
        }
    }
    Ok((resolved.len() == 1).then(|| resolved.pop()).flatten())
}

/// Every compact NURBS surface carrier in `bytes`, keyed by attribute id.
///
/// `refusals` carries the same meaning as in [`scan_curve_carriers`].
pub(crate) fn scan_surface_carriers(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    refusals: &mut Vec<LossNote>,
) -> Result<HashMap<u16, SurfaceCarrier>, cadmpeg_core::CodecError> {
    let descriptors = scan_surface_descriptors(ctx, bytes)?;
    charge_items(
        ctx,
        descriptors.len() * 5,
        "collect Parasolid surface array references",
    )?;
    let compact_attrs = descriptors
        .values()
        .flat_map(|descriptor| descriptor.refs)
        .collect();
    let arrays = scan_arrays(ctx, bytes, Some(&compact_attrs))?;
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(1) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x7c]) {
            continue;
        }
        let mut p = off + 2;
        if bytes.get(p) == Some(&0xff) {
            p += 1;
        }
        let Some(attr) = View::u16_be_at(bytes, p) else {
            continue;
        };
        let Some(descriptor_attr) = View::u16_be_at(bytes, p + 17) else {
            continue;
        };
        let Some(descriptor) = descriptors.get(&descriptor_attr) else {
            continue;
        };
        let Some(expected_poles) = descriptor.u_count.checked_mul(descriptor.v_count) else {
            continue;
        };
        let Some(expected_control_values) = expected_poles.checked_mul(descriptor.dimension) else {
            continue;
        };
        let Some(control) = exact_f64_array(
            ctx,
            bytes,
            &arrays,
            descriptor.refs[0],
            expected_control_values,
        )?
        else {
            continue;
        };
        let Some((u_unique, u_mult)) = surface_knot_values(
            ctx,
            bytes,
            &arrays,
            descriptor.refs[3],
            descriptor.refs[1],
            descriptor.u_knot_count,
        )?
        else {
            continue;
        };
        let Some((v_unique, v_mult)) = surface_knot_values(
            ctx,
            bytes,
            &arrays,
            descriptor.refs[4],
            descriptor.refs[2],
            descriptor.v_knot_count,
        )?
        else {
            continue;
        };
        if control.len() != expected_control_values {
            continue;
        }
        let Some(u_expected) = descriptor
            .u_count
            .checked_add(descriptor.u_degree as usize)
            .and_then(|value| value.checked_add(1))
        else {
            continue;
        };
        let Some(v_expected) = descriptor
            .v_count
            .checked_add(descriptor.v_degree as usize)
            .and_then(|value| value.checked_add(1))
        else {
            continue;
        };
        let Some(u_multiplicity_sum) = multiplicity_sum(&u_mult) else {
            continue;
        };
        let Some(v_multiplicity_sum) = multiplicity_sum(&v_mult) else {
            continue;
        };
        if u_multiplicity_sum != u_expected || v_multiplicity_sum != v_expected {
            continue;
        }
        if !u_unique.iter().all(|value| value.is_finite())
            || !v_unique.iter().all(|value| value.is_finite())
            || !knots_nondecreasing(&u_unique)
            || !knots_nondecreasing(&v_unique)
        {
            continue;
        }
        charge_items(ctx, expected_poles, "decode Parasolid surface poles")?;
        let mut points = Vec::with_capacity(expected_poles);
        let dimension = if descriptor.rational {
            descriptor.dimension
        } else {
            3
        };
        if descriptor.rational {
            charge_items(ctx, expected_poles, "decode Parasolid surface weights")?;
        }
        let mut weights = (descriptor.rational).then(Vec::new);
        for pole in control.chunks_exact(dimension) {
            if pole.iter().any(|value| !value.is_finite()) {
                points.clear();
                break;
            }
            let weight = if descriptor.rational { pole[3] } else { 1.0 };
            if !weight.is_finite() || weight.abs() <= f64::EPSILON {
                points.clear();
                break;
            }
            points.push(Point3::new(
                pole[0] / weight * LEN_TO_MM,
                pole[1] / weight * LEN_TO_MM,
                pole[2] / weight * LEN_TO_MM,
            ));
            if let Some(values) = &mut weights {
                values.push(weight);
            }
        }
        if points.len() != expected_poles {
            continue;
        }
        let (Some(u_knots), Some(v_knots)) = (
            expanded_knots(ctx, &u_unique, &u_mult, u_expected)?,
            expanded_knots(ctx, &v_unique, &v_mult, v_expected)?,
        ) else {
            continue;
        };
        if u_knots.len() != u_expected || v_knots.len() != v_expected {
            continue;
        }
        charge_items(
            ctx,
            descriptor.u_count,
            "partition Parasolid surface pole rows",
        )?;
        charge_items(ctx, expected_poles, "partition Parasolid surface poles")?;
        if descriptor.rational {
            charge_items(
                ctx,
                descriptor.u_count,
                "partition Parasolid surface weight rows",
            )?;
            charge_items(ctx, expected_poles, "partition Parasolid surface weights")?;
            charge_items(ctx, descriptor.u_count, "pair Parasolid weighted pole rows")?;
            charge_items(ctx, expected_poles, "pair Parasolid weighted poles")?;
        }
        charge_items(ctx, descriptor.u_count, "admit Parasolid surface pole rows")?;
        charge_items(ctx, expected_poles, "admit Parasolid surface poles")?;
        let nurbs = match NurbsSurface::from_lanes(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                descriptor.u_degree,
                u_knots,
                descriptor.u_periodic,
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                descriptor.v_degree,
                v_knots,
                descriptor.v_periodic,
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                points
                    .chunks(descriptor.v_count as u32 as usize)
                    .map(<[_]>::to_vec)
                    .collect(),
                weights.map(|values| {
                    values
                        .chunks(descriptor.v_count as u32 as usize)
                        .map(<[_]>::to_vec)
                        .collect()
                }),
            ),
            false,
        ) {
            Ok(nurbs) => nurbs,
            Err(error) => {
                charge_items(ctx, 1, "collect Parasolid spline refusals")?;
                refusals.push(crate::loss::spline_lane_refusal(&format!(
                    "surface carrier attribute {attr}: {error}"
                )));
                continue;
            }
        };
        charge_items(ctx, 1, "collect Parasolid surface carriers")?;
        out.entry(attr).or_insert(SurfaceCarrier {
            attr,
            offset: off,
            end: off + 2,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
            orientation_reversed: false,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        expanded_knots, scan_arrays, scan_curve_carriers, scan_surface_carriers, unique_knots,
        unique_surface_knot_span, Arrays,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[test]
    fn parasolid_scalar_array_values_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::f64_array(0x2d, 12, &[0.0, 1.0, 2.0]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error =
            scan_arrays(Some(&ctx), &bytes, None).expect_err("three values exceed two items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid scalar array values"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            scan_arrays(Some(&ctx), &bytes, None)
                .expect("service scan")
                .f64s
                .get(&12)
                .map(Vec::len),
            Some(3)
        );
    }

    #[test]
    fn parasolid_integer_array_values_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::u16_array(12, &[1, 2, 3]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error =
            scan_arrays(Some(&ctx), &bytes, None).expect_err("three values exceed two items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid integer array values"));
    }

    #[test]
    fn parasolid_knot_expansion_refuses_collection_limit_before_allocation() {
        let bytes = [0_u8; 1];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = expanded_knots(Some(&ctx), &[0.0, 1.0], &[2, 2], 4)
            .expect_err("four knots exceed three items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "expand Parasolid knots"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            expanded_knots(Some(&ctx), &[0.0, 1.0], &[2, 2], 4).expect("service expansion"),
            Some(vec![0.0, 0.0, 1.0, 1.0])
        );
    }

    #[test]
    fn parasolid_curve_poles_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 17;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_curve_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect_err("three poles exceed the remaining items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid curve poles"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(scan_curve_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect("service scan")
            .contains_key(&170));
    }

    #[test]
    fn parasolid_curve_weights_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::rational_linear_nurbs_curve_carrier(170, 171);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 18;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_curve_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect_err("two weights exceed the remaining items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid curve weights"));
    }

    #[test]
    fn parasolid_curve_descriptors_refuse_collection_limit_before_insertion() {
        let bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 16;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_curve_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect_err("curve descriptor insertion exceeds the limit");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "collect Parasolid curve descriptors"));
    }

    #[test]
    fn parasolid_curve_carriers_refuse_collection_limit_before_insertion() {
        let bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 29;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_curve_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect_err("curve carrier insertion exceeds the limit");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "collect Parasolid curve carriers"));
    }

    #[test]
    fn parasolid_curve_weighted_poles_refuse_collection_limit_before_pairing() {
        let bytes = crate::test_support::parasolid::rational_linear_nurbs_curve_carrier(170, 171);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 24;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_curve_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect_err("weighted pole pairing exceeds the limit");
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "pair Parasolid curve weighted poles"),
            "{error:?}"
        );
    }

    #[test]
    fn parasolid_curve_poles_refuse_collection_limit_before_admission() {
        let bytes = crate::test_support::parasolid::rational_linear_nurbs_curve_carrier(170, 171);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 26;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_curve_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect_err("pole admission exceeds the limit");
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Parasolid curve poles"),
            "{error:?}"
        );
    }

    #[test]
    fn parasolid_surface_poles_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 102;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_surface_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect_err("four surface poles exceed the remaining items");
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid surface poles"),
            "{error:?}"
        );

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(scan_surface_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect("service scan")
            .contains_key(&180));
    }

    #[test]
    fn parasolid_surface_weights_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 119;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_surface_carriers(Some(&ctx), &bytes, &mut Vec::new())
            .expect_err("four surface weights exceed the remaining items");
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid surface weights"),
            "{error:?}"
        );
    }

    macro_rules! surface_collection_boundary {
        ($name:ident, $bytes:expr, $limit:expr, $operation:literal) => {
            #[test]
            fn $name() {
                let bytes = $bytes;
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = $limit;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                let error = scan_surface_carriers(Some(&ctx), &bytes, &mut Vec::new())
                    .expect_err("surface collection exceeds the limit");
                assert!(matches!(error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::CollectionItems
                            && limit.operation == $operation), "{error:?}");
            }
        };
    }

    macro_rules! plain_surface_boundary {
        ($name:ident, $limit:expr, $operation:literal) => {
            surface_collection_boundary!(
                $name,
                crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10),
                $limit,
                $operation
            );
        };
    }

    plain_surface_boundary!(
        parasolid_surface_descriptors_refuse_before_insertion,
        0,
        "collect Parasolid surface descriptors"
    );
    plain_surface_boundary!(
        parasolid_surface_array_references_refuse_before_collection,
        1,
        "collect Parasolid surface array references"
    );
    plain_surface_boundary!(
        parasolid_compact_arrays_refuse_before_insertion,
        6,
        "scan Parasolid compact arrays"
    );
    plain_surface_boundary!(
        parasolid_scalar_arrays_refuse_before_insertion,
        23,
        "collect Parasolid scalar arrays"
    );
    plain_surface_boundary!(
        parasolid_integer_arrays_refuse_before_insertion,
        27,
        "collect Parasolid integer arrays"
    );
    plain_surface_boundary!(
        parasolid_scalar_values_refuse_before_candidate_copy,
        41,
        "copy Parasolid scalar array values"
    );
    plain_surface_boundary!(
        parasolid_scalar_candidates_refuse_before_insertion,
        53,
        "collect Parasolid scalar candidates"
    );
    plain_surface_boundary!(
        parasolid_compact_scalar_values_refuse_before_allocation,
        54,
        "decode Parasolid compact scalar values"
    );
    plain_surface_boundary!(
        parasolid_integer_values_refuse_before_candidate_copy,
        70,
        "copy Parasolid integer array values"
    );
    plain_surface_boundary!(
        parasolid_integer_candidates_refuse_before_insertion,
        72,
        "collect Parasolid integer candidates"
    );
    plain_surface_boundary!(
        parasolid_compact_integer_values_refuse_before_allocation,
        73,
        "decode Parasolid compact integer values"
    );
    plain_surface_boundary!(
        parasolid_knot_multiplicity_groups_refuse_before_insertion,
        75,
        "group Parasolid knot multiplicities"
    );
    plain_surface_boundary!(
        parasolid_distinct_knots_refuse_before_copy,
        81,
        "copy Parasolid distinct knots"
    );
    plain_surface_boundary!(
        parasolid_knot_multiplicities_refuse_before_copy,
        83,
        "copy Parasolid knot multiplicities"
    );
    plain_surface_boundary!(
        parasolid_knot_candidates_refuse_before_insertion,
        85,
        "collect Parasolid knot candidates"
    );
    plain_surface_boundary!(
        parasolid_surface_pole_rows_refuse_before_partition,
        114,
        "partition Parasolid surface pole rows"
    );
    plain_surface_boundary!(
        parasolid_surface_poles_refuse_before_partition,
        116,
        "partition Parasolid surface poles"
    );
    plain_surface_boundary!(
        parasolid_surface_pole_rows_refuse_before_admission,
        120,
        "admit Parasolid surface pole rows"
    );
    plain_surface_boundary!(
        parasolid_surface_poles_refuse_before_admission,
        122,
        "admit Parasolid surface poles"
    );
    plain_surface_boundary!(
        parasolid_surface_carriers_refuse_before_insertion,
        126,
        "collect Parasolid surface carriers"
    );
    surface_collection_boundary!(
        parasolid_surface_weight_rows_refuse_before_partition,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        137,
        "partition Parasolid surface weight rows"
    );
    surface_collection_boundary!(
        parasolid_surface_weights_refuse_before_partition,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        139,
        "partition Parasolid surface weights"
    );
    surface_collection_boundary!(
        parasolid_weighted_surface_rows_refuse_before_pairing,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        143,
        "pair Parasolid weighted pole rows"
    );
    surface_collection_boundary!(
        parasolid_weighted_surface_poles_refuse_before_pairing,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        145,
        "pair Parasolid weighted poles"
    );

    #[test]
    fn patch_shape_counts_do_not_narrow_to_the_native_multiplicity_width() {
        let count = usize::from(u16::MAX) + 1;
        let knots = std::iter::repeat_n(0.0, count)
            .chain(std::iter::once(1.0))
            .collect::<Vec<_>>();
        let (values, multiplicities) = unique_knots(&knots);
        assert_eq!(values, [0.0, 1.0]);
        assert_eq!(multiplicities, [count, 1]);
    }

    /// A rational pole whose coordinate times its weight overflows declines
    /// the patch, where its homogeneous coordinate was written as an
    /// infinity.
    #[test]
    fn a_pole_whose_weighted_coordinate_overflows_declines_the_patch() {
        let point = cadmpeg_ir::math::Point3::new(1.0e300, 0.0, 0.0);
        assert_eq!(
            super::homogeneous_poles(&[point], Some(&[1.0e300]), 0.001),
            None
        );
        assert_eq!(
            super::homogeneous_poles(&[point], Some(&[2.0]), 0.001),
            Some(vec![1.0e300 * 0.001 * 2.0, 0.0, 0.0, 2.0])
        );
    }

    #[test]
    fn missing_surface_knot_arrays_decline_the_patch() {
        assert_eq!(
            unique_surface_knot_span(&[], &Arrays::default(), 2, 3, 1, &[0.0], &[1]),
            None
        );
    }
}
