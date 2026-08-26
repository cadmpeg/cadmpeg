// SPDX-License-Identifier: Apache-2.0
//! B-spline/list carrier tables.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::geometry::{
    knots_nondecreasing, CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;

use cadmpeg_core::decode::View;

use super::{Carrier, CarrierGeometry, LEN_TO_MM};

use crate::layout::bspline_array_header as arr_hdr;
use crate::layout::bspline_compact_array_header as compact_arr;
use crate::layout::bspline_surface_descriptor as surf_desc;

#[derive(Default)]
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

fn scan_arrays(bytes: &[u8], compact_attrs: Option<&HashSet<u16>>) -> Arrays {
    let mut arrays = Arrays::default();
    for off in 0..bytes.len().saturating_sub(9) {
        if bytes.get(off) == Some(&0) {
            let count = usize::from(bytes[off + compact_arr::COUNT]);
            if count > 0 {
                if let Some(attr) = View::u16_be_at(bytes, off + compact_arr::ATTR)
                    .filter(|attr| compact_attrs.is_some_and(|attrs| attrs.contains(attr)))
                {
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
            let Some(values) = (0..count)
                .map(|i| View::u16_be_at(bytes, values_at + i * 2))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            arrays.u16s.entry(attr).or_insert(values);
        } else {
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
            arrays.f64s.entry(attr).or_insert(values);
        }
    }
    arrays
}

fn compact_f64_arrays(bytes: &[u8], arrays: &Arrays, attr: u16) -> Vec<Vec<f64>> {
    let mut candidates = arrays
        .f64s
        .get(&attr)
        .cloned()
        .into_iter()
        .collect::<Vec<_>>();
    for compact in arrays.compact.get(&attr).into_iter().flatten() {
        let Some(values) = (0..compact.count)
            .map(|index| View::f64_be_at(bytes, compact.offset + compact_arr::LEN + index * 8))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !candidates.contains(&values) {
            candidates.push(values);
        }
    }
    candidates
}

fn compact_u16_arrays(bytes: &[u8], arrays: &Arrays, attr: u16) -> Vec<Vec<u16>> {
    let mut candidates = arrays
        .u16s
        .get(&attr)
        .cloned()
        .into_iter()
        .collect::<Vec<_>>();
    for compact in arrays.compact.get(&attr).into_iter().flatten() {
        let Some(values) = (0..compact.count)
            .map(|index| View::u16_be_at(bytes, compact.offset + compact_arr::LEN + index * 2))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !candidates.contains(&values) {
            candidates.push(values);
        }
    }
    candidates
}

fn exact_f64_array(bytes: &[u8], arrays: &Arrays, attr: u16, count: usize) -> Option<Vec<f64>> {
    let mut candidates = compact_f64_arrays(bytes, arrays, attr)
        .into_iter()
        .filter(|values| values.len() == count);
    let selected = candidates.next()?;
    candidates
        .all(|candidate| candidate == selected)
        .then_some(selected)
}

fn scan_curve_descriptors(bytes: &[u8]) -> HashMap<u16, CurveDescriptor> {
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
        out.entry(attr).or_insert(CurveDescriptor {
            degree,
            control_count,
            dimension,
            control_attr,
            multiplicity_attr,
            knot_attr,
        });
    }
    out
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
fn expanded_knots(values: &[f64], multiplicities: &[u16], expected: usize) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    for (value, &multiplicity) in values.iter().zip(multiplicities) {
        if multiplicity == 0 {
            continue;
        }
        let next_len = out.len().checked_add(multiplicity as usize)?;
        if next_len > expected {
            return None;
        }
        out.extend(std::iter::repeat_n(*value, multiplicity as usize));
    }
    Some(out)
}

fn unique_knots(knots: &[f64]) -> (Vec<f64>, Vec<u16>) {
    let mut unique = Vec::new();
    let mut multiplicities = Vec::new();
    for &knot in knots {
        if unique.last() == Some(&knot) {
            *multiplicities.last_mut().expect("matching knot") += 1;
        } else {
            unique.push(knot);
            multiplicities.push(1);
        }
    }
    (unique, multiplicities)
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
    old_multiplicities: &[u16],
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
            if knots == old_values && multiplicities == old_multiplicities {
                pairs.push((knot_span, multiplicity_span));
            }
        }
    }
    pairs.sort_by_key(|(knots, multiplicities)| (knots.start, knots.count, multiplicities.start));
    pairs.dedup();
    (pairs.len() == 1).then_some(pairs[0].0)
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
        if !weight.is_finite() || weight.abs() <= f64::EPSILON {
            return None;
        }
        out.extend([
            point.x * scale * weight,
            point.y * scale * weight,
            point.z * scale * weight,
        ]);
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
    if old.degree != new.degree
        || old.control_points.len() != new.control_points.len()
        || old.weights.is_some() != new.weights.is_some()
        || old.periodic != new.periodic
    {
        return None;
    }
    let (old_unique, old_mult) = unique_knots(&old.knots);
    let (new_unique, new_mult) = unique_knots(&new.knots);
    if old_mult != new_mult || old_unique.len() != new_unique.len() {
        return None;
    }
    let mut p = wrapper_offset + 2;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let descriptors = scan_curve_descriptors(bytes);
    let descriptor = curve_descriptor(bytes, p, &descriptors)?;
    if descriptor.degree != old.degree
        || descriptor.control_count != old.control_points.len()
        || descriptor.dimension != if old.weights.is_some() { 4 } else { 3 }
    {
        return None;
    }
    let poles = homogeneous_poles(&new.control_points, new.weights.as_deref(), scale)?;
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
    if old.u_degree != new.u_degree
        || old.v_degree != new.v_degree
        || old.u_count != new.u_count
        || old.v_count != new.v_count
        || old.control_points.len() != new.control_points.len()
        || old.weights.is_some() != new.weights.is_some()
        || old.u_periodic != new.u_periodic
        || old.v_periodic != new.v_periodic
    {
        return None;
    }
    let (old_u, old_u_mult) = unique_knots(&old.u_knots);
    let (new_u, new_u_mult) = unique_knots(&new.u_knots);
    let (old_v, old_v_mult) = unique_knots(&old.v_knots);
    let (new_v, new_v_mult) = unique_knots(&new.v_knots);
    if old_u_mult != new_u_mult
        || old_v_mult != new_v_mult
        || old_u.len() != new_u.len()
        || old_v.len() != new_v.len()
    {
        return None;
    }
    let descriptors = scan_surface_descriptors(bytes);
    let compact_attrs = descriptors
        .values()
        .flat_map(|descriptor| descriptor.refs)
        .collect();
    let arrays = scan_arrays(bytes, Some(&compact_attrs));
    let mut p = wrapper_offset + 2;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let descriptor_attr = View::u16_be_at(bytes, p + 17)?;
    let descriptor = descriptors.get(&descriptor_attr)?;
    let [control_attr, _, _, u_knot_attr, v_knot_attr] = descriptor.refs;
    let dimension = if old.weights.is_some() { 4 } else { 3 };
    if descriptor.u_degree != old.u_degree
        || descriptor.v_degree != old.v_degree
        || descriptor.u_count != old.u_count as usize
        || descriptor.v_count != old.v_count as usize
        || descriptor.dimension != dimension
        || descriptor.u_periodic != old.u_periodic
        || descriptor.v_periodic != old.v_periodic
    {
        return None;
    }
    let old_poles = homogeneous_poles(&old.control_points, old.weights.as_deref(), scale)?;
    let poles = homogeneous_poles(&new.control_points, new.weights.as_deref(), scale)?;
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

pub fn scan_curve_carriers(bytes: &[u8]) -> HashMap<u16, Carrier> {
    let arrays = scan_arrays(bytes, None);
    let descriptors = scan_curve_descriptors(bytes);
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
        let mut points = Vec::with_capacity(descriptor.control_count);
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
        let Some(knots) = expanded_knots(unique_knots, multiplicities, expected) else {
            continue;
        };
        if knots.len() != expected {
            continue;
        }
        out.entry(attr).or_insert(Carrier {
            attr,
            offset: off,
            end: off + 2,
            geometry: CarrierGeometry::Curve(CurveGeometry::Nurbs(NurbsCurve {
                degree: descriptor.degree,
                knots,
                control_points: points,
                weights,
                periodic: false,
            })),
            frame: None,
            parameter_range: None,
            orientation_reversed: false,
        });
    }
    out
}

fn scan_surface_descriptors(bytes: &[u8]) -> HashMap<u16, SurfaceDescriptor> {
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(1) {
        let Some(descriptor) = parse_surface_descriptor(bytes, off) else {
            continue;
        };
        out.entry(descriptor.attr).or_insert(descriptor);
    }
    out
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
    bytes: &[u8],
    arrays: &Arrays,
    knot_attr: u16,
    multiplicity_attr: u16,
    declared_count: usize,
) -> Option<(Vec<f64>, Vec<u16>)> {
    let mut resolved = Vec::<(Vec<f64>, Vec<u16>)>::new();
    let mut multiplicities_by_count = HashMap::<usize, Vec<Vec<u16>>>::new();
    for multiplicities in compact_u16_arrays(bytes, arrays, multiplicity_attr) {
        multiplicities_by_count
            .entry(multiplicities.len())
            .or_default()
            .push(multiplicities);
    }
    for unique in compact_f64_arrays(bytes, arrays, knot_attr) {
        let Some(multiplicity_candidates) = multiplicities_by_count.get(&unique.len()) else {
            continue;
        };
        for multiplicities in multiplicity_candidates {
            let Some((unique, multiplicities)) =
                surface_knot_arrays(&unique, multiplicities, declared_count)
            else {
                continue;
            };
            let candidate = (unique.to_vec(), multiplicities.to_vec());
            if !resolved.contains(&candidate) {
                resolved.push(candidate);
            }
        }
    }
    (resolved.len() == 1).then(|| resolved.pop()).flatten()
}

pub fn scan_surface_carriers(bytes: &[u8]) -> HashMap<u16, Carrier> {
    let descriptors = scan_surface_descriptors(bytes);
    let compact_attrs = descriptors
        .values()
        .flat_map(|descriptor| descriptor.refs)
        .collect();
    let arrays = scan_arrays(bytes, Some(&compact_attrs));
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
        let Some(control) =
            exact_f64_array(bytes, &arrays, descriptor.refs[0], expected_control_values)
        else {
            continue;
        };
        let Some((u_unique, u_mult)) = surface_knot_values(
            bytes,
            &arrays,
            descriptor.refs[3],
            descriptor.refs[1],
            descriptor.u_knot_count,
        ) else {
            continue;
        };
        let Some((v_unique, v_mult)) = surface_knot_values(
            bytes,
            &arrays,
            descriptor.refs[4],
            descriptor.refs[2],
            descriptor.v_knot_count,
        ) else {
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
        let mut points = Vec::with_capacity(expected_poles);
        let dimension = if descriptor.rational {
            descriptor.dimension
        } else {
            3
        };
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
            expanded_knots(&u_unique, &u_mult, u_expected),
            expanded_knots(&v_unique, &v_mult, v_expected),
        ) else {
            continue;
        };
        if u_knots.len() != u_expected || v_knots.len() != v_expected {
            continue;
        }
        out.entry(attr).or_insert(Carrier {
            attr,
            offset: off,
            end: off + 2,
            geometry: CarrierGeometry::Surface(SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: descriptor.u_degree,
                v_degree: descriptor.v_degree,
                u_knots,
                v_knots,
                u_count: descriptor.u_count as u32,
                v_count: descriptor.v_count as u32,
                control_points: points,
                weights,
                normal_reversed: false,
                u_periodic: descriptor.u_periodic,
                v_periodic: descriptor.v_periodic,
            })),
            frame: None,
            parameter_range: None,
            orientation_reversed: false,
        });
    }
    out
}
