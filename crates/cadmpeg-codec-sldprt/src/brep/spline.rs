// SPDX-License-Identifier: Apache-2.0
//! B-spline/list carrier tables.

use std::collections::HashMap;

use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

use super::{f64_be, u16_be, u32_be, Carrier, CarrierGeometry, LEN_TO_MM};

#[derive(Default)]
struct Arrays {
    f64s: HashMap<u16, Vec<f64>>,
    u16s: HashMap<u16, Vec<u16>>,
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
const SURFACE_DESCRIPTOR_BODY_LEN: usize = 42;
const SURFACE_DESCRIPTOR_REFS_OFFSET: usize = 32;

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
    let end = p.checked_add(SURFACE_DESCRIPTOR_BODY_LEN)?;
    if end > bytes.len() {
        return None;
    }
    let attr = u16_be(bytes, p)?;
    let u_periodic = logical_byte(bytes, p + 2)?;
    let v_periodic = logical_byte(bytes, p + 3)?;
    let u_degree = u16_be(bytes, p + 4).map(u32::from)?;
    let v_degree = u16_be(bytes, p + 6).map(u32::from)?;
    let u_degree_usize = usize::try_from(u_degree).ok()?;
    let v_degree_usize = usize::try_from(v_degree).ok()?;
    let u_count = usize::try_from(u32_be(bytes, p + 8)?).ok()?;
    let v_count = usize::try_from(u32_be(bytes, p + 12)?).ok()?;
    // Knot-type bytes, closure flags, and surface form have no IR fields, but
    // their positions are part of the fixed descriptor and are consumed here
    // so the following count and reference fields cannot slide into them.
    let _u_knot_type = *bytes.get(p + 16)?;
    let _v_knot_type = *bytes.get(p + 17)?;
    let u_knot_count = usize::try_from(u32_be(bytes, p + 18)?).ok()?;
    let v_knot_count = usize::try_from(u32_be(bytes, p + 22)?).ok()?;
    let rational = logical_byte(bytes, p + 26)?;
    let _u_closed = logical_byte(bytes, p + 27)?;
    let _v_closed = logical_byte(bytes, p + 28)?;
    let _surface_form = *bytes.get(p + 29)?;
    let dimension = u16_be(bytes, p + 30).map(usize::from)?;
    let refs = [
        u16_be(bytes, p + SURFACE_DESCRIPTOR_REFS_OFFSET)?,
        u16_be(bytes, p + SURFACE_DESCRIPTOR_REFS_OFFSET + 2)?,
        u16_be(bytes, p + SURFACE_DESCRIPTOR_REFS_OFFSET + 4)?,
        u16_be(bytes, p + SURFACE_DESCRIPTOR_REFS_OFFSET + 6)?,
        u16_be(bytes, p + SURFACE_DESCRIPTOR_REFS_OFFSET + 8)?,
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

fn scan_arrays(bytes: &[u8]) -> Arrays {
    let mut arrays = Arrays::default();
    for off in 0..bytes.len().saturating_sub(9) {
        let tag = match bytes.get(off..off + 2) {
            Some([0x00, tag @ (0x2d | 0x7f | 0x80)]) => *tag,
            _ => continue,
        };
        let Some(p) = array_body(bytes, off, tag) else {
            continue;
        };
        let Some(count) = u32_be(bytes, p).map(|v| v as usize) else {
            continue;
        };
        let Some(attr) = u16_be(bytes, p + 4) else {
            continue;
        };
        if attr <= 1 || count > 1_000_000 {
            continue;
        }
        let values_at = p + 6;
        if tag == 0x7f {
            let Some(values) = (0..count)
                .map(|i| u16_be(bytes, values_at + i * 2))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            arrays.u16s.entry(attr).or_insert(values);
        } else {
            let Some(values) = (0..count)
                .map(|i| f64_be(bytes, values_at + i * 8))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            // The native surface format can reserve one physical knot slot
            // beyond the descriptor's distinct-knot count. Its bits are not
            // semantic data, so defer finite-value checks to the typed
            // carrier reader after it applies the descriptor and
            // multiplicity bounds.
            arrays.f64s.entry(attr).or_insert(values);
        }
    }
    arrays
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
        let Some(attr) = u16_be(bytes, p) else {
            continue;
        };
        let Some(degree) = u16_be(bytes, p + 2).map(u32::from) else {
            continue;
        };
        let Some(control_count) = u32_be(bytes, p + 4).map(|v| v as usize) else {
            continue;
        };
        let Some(dimension) = u16_be(bytes, p + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(control_attr) = u16_be(bytes, p + 19) else {
            continue;
        };
        let Some(multiplicity_attr) = u16_be(bytes, p + 21) else {
            continue;
        };
        let Some(knot_attr) = u16_be(bytes, p + 23) else {
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
        .filter_map(|at| u16_be(bytes, at))
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
        let Some(count) = u32_be(bytes, p).map(|value| value as usize) else {
            continue;
        };
        if u16_be(bytes, p + 4) == Some(attr) {
            return Some((p + 6, count));
        }
    }
    None
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

/// Patch a surface knot array while retaining its optional terminal
/// zero-multiplicity slot. The descriptor count names the real entries; the
/// f64 bits in the extra physical slot are not semantic data.
fn patch_surface_knot_array(
    bytes: &mut [u8],
    tag: u8,
    attr: u16,
    values: &[f64],
    multiplicities: &[u16],
) -> Option<()> {
    let (start, count) = array_span(bytes, tag, attr)?;
    if multiplicities.len() != count {
        return None;
    }
    let exact = count == values.len() && multiplicities.iter().all(|&value| value != 0);
    let terminal = count == values.len().checked_add(1)?
        && multiplicities.last() == Some(&0)
        && multiplicities[..values.len()]
            .iter()
            .all(|&value| value != 0);
    if !exact && !terminal {
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
    let arrays = scan_arrays(bytes);
    let descriptors = scan_surface_descriptors(bytes);
    let mut p = wrapper_offset + 2;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let descriptor_attr = u16_be(bytes, p + 17)?;
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
    let u_mult_native = arrays.u16s.get(&descriptor.refs[1])?;
    let v_mult_native = arrays.u16s.get(&descriptor.refs[2])?;
    let u_unique_native = arrays.f64s.get(&descriptor.refs[3])?;
    let v_unique_native = arrays.f64s.get(&descriptor.refs[4])?;
    let (u_unique_native, u_mult) =
        surface_knot_arrays(u_unique_native, u_mult_native, descriptor.u_knot_count)?;
    let (v_unique_native, v_mult) =
        surface_knot_arrays(v_unique_native, v_mult_native, descriptor.v_knot_count)?;
    if u_mult != old_u_mult
        || v_mult != old_v_mult
        || u_unique_native.len() != old_u.len()
        || v_unique_native.len() != old_v.len()
        || u_unique_native != old_u.as_slice()
        || v_unique_native != old_v.as_slice()
    {
        return None;
    }
    let poles = homogeneous_poles(&new.control_points, new.weights.as_deref(), scale)?;
    patch_f64_array(bytes, 0x2d, control_attr, &poles)?;
    patch_surface_knot_array(bytes, 0x80, u_knot_attr, &new_u, u_mult_native)?;
    patch_surface_knot_array(bytes, 0x80, v_knot_attr, &new_v, v_mult_native)
}

pub fn scan_curve_carriers(bytes: &[u8]) -> HashMap<u16, Carrier> {
    let arrays = scan_arrays(bytes);
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
        let Some(attr) = u16_be(bytes, p) else {
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
        if !unique_knots.iter().all(|value| value.is_finite())
            || !unique_knots.windows(2).all(|window| window[0] <= window[1])
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
    if declared_count == 0 || unique.len() != multiplicities.len() {
        return None;
    }
    if unique.len() == declared_count {
        return multiplicities
            .iter()
            .all(|&value| value != 0)
            .then_some((unique, multiplicities));
    }
    if unique.len() == declared_count.checked_add(1)?
        && multiplicities.last() == Some(&0)
        && multiplicities[..declared_count]
            .iter()
            .all(|&value| value != 0)
    {
        // The final f64 slot is physically present but semantically excluded
        // by the zero multiplicity. Native writers do not have to initialize
        // that slot; only the descriptor count and multiplicity roster bind
        // the usable knot values.
        return Some((&unique[..declared_count], &multiplicities[..declared_count]));
    }
    None
}

pub fn scan_surface_carriers(bytes: &[u8]) -> HashMap<u16, Carrier> {
    let arrays = scan_arrays(bytes);
    let descriptors = scan_surface_descriptors(bytes);
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(1) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x7c]) {
            continue;
        }
        let mut p = off + 2;
        if bytes.get(p) == Some(&0xff) {
            p += 1;
        }
        let Some(attr) = u16_be(bytes, p) else {
            continue;
        };
        let Some(descriptor_attr) = u16_be(bytes, p + 17) else {
            continue;
        };
        let Some(descriptor) = descriptors.get(&descriptor_attr) else {
            continue;
        };
        let Some(control) = arrays.f64s.get(&descriptor.refs[0]) else {
            continue;
        };
        let Some(u_mult) = arrays.u16s.get(&descriptor.refs[1]) else {
            continue;
        };
        let Some(v_mult) = arrays.u16s.get(&descriptor.refs[2]) else {
            continue;
        };
        let Some(u_unique) = arrays.f64s.get(&descriptor.refs[3]) else {
            continue;
        };
        let Some(v_unique) = arrays.f64s.get(&descriptor.refs[4]) else {
            continue;
        };
        let Some((u_unique, u_mult)) =
            surface_knot_arrays(u_unique, u_mult, descriptor.u_knot_count)
        else {
            continue;
        };
        let Some((v_unique, v_mult)) =
            surface_knot_arrays(v_unique, v_mult, descriptor.v_knot_count)
        else {
            continue;
        };
        let Some(expected_poles) = descriptor.u_count.checked_mul(descriptor.v_count) else {
            continue;
        };
        let Some(expected_control_values) = expected_poles.checked_mul(descriptor.dimension) else {
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
        let Some(u_multiplicity_sum) = multiplicity_sum(u_mult) else {
            continue;
        };
        let Some(v_multiplicity_sum) = multiplicity_sum(v_mult) else {
            continue;
        };
        if u_multiplicity_sum != u_expected || v_multiplicity_sum != v_expected {
            continue;
        }
        if !u_unique.iter().all(|value| value.is_finite())
            || !v_unique.iter().all(|value| value.is_finite())
            || !u_unique.windows(2).all(|window| window[0] <= window[1])
            || !v_unique.windows(2).all(|window| window[0] <= window[1])
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
            expanded_knots(u_unique, u_mult, u_expected),
            expanded_knots(v_unique, v_mult, v_expected),
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
