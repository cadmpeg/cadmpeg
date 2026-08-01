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
            if values.iter().all(|v| v.is_finite()) {
                arrays.f64s.entry(attr).or_insert(values);
            }
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
        if out.len() + multiplicity as usize > expected {
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
    let descriptors = surface_refs(bytes, &arrays);
    let mut p = wrapper_offset + 2;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let descriptor_attr = u16_be(bytes, p + 17)?;
    let refs = descriptors.get(&descriptor_attr)?;
    let poles = homogeneous_poles(&new.control_points, new.weights.as_deref(), scale)?;
    patch_f64_array(bytes, 0x2d, refs[0], &poles)?;
    patch_f64_array(bytes, 0x80, refs[3], &new_u)?;
    patch_f64_array(bytes, 0x80, refs[4], &new_v)
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
        if control.len() != descriptor.control_count * descriptor.dimension {
            continue;
        }
        let mut points = Vec::with_capacity(descriptor.control_count);
        let mut weights = (descriptor.dimension == 4).then(Vec::new);
        for pole in control.chunks_exact(descriptor.dimension) {
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
            orientation_reversed: false,
        });
    }
    out
}

fn surface_refs(bytes: &[u8], arrays: &Arrays) -> HashMap<u16, [u16; 5]> {
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(16) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x7e]) {
            continue;
        }
        let mut p = off + 2;
        if bytes.get(p) == Some(&0xff) {
            p += 1;
        }
        let Some(attr) = u16_be(bytes, p) else {
            continue;
        };
        for at in (p + 2..(p + 96).min(bytes.len().saturating_sub(9))).step_by(2) {
            let Some(refs) = (0..5)
                .map(|i| u16_be(bytes, at + i * 2))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if arrays.f64s.contains_key(&refs[0])
                && arrays.u16s.contains_key(&refs[1])
                && arrays.u16s.contains_key(&refs[2])
                && arrays.f64s.contains_key(&refs[3])
                && arrays.f64s.contains_key(&refs[4])
            {
                out.insert(attr, [refs[0], refs[1], refs[2], refs[3], refs[4]]);
                break;
            }
        }
    }
    out
}

#[allow(clippy::manual_is_multiple_of)] // `is_multiple_of` exceeds the workspace MSRV.
pub(crate) fn infer_surface_shape(
    control_len: usize,
    u_mult: &[u16],
    v_mult: &[u16],
) -> Option<(usize, usize, u32, u32, usize)> {
    let u_sum: usize = u_mult.iter().map(|v| *v as usize).sum();
    let v_sum: usize = v_mult.iter().map(|v| *v as usize).sum();
    for dimension in [4usize, 3] {
        if !control_len.is_multiple_of(dimension) {
            continue;
        }
        let poles = control_len / dimension;
        for u_degree in 1..=8usize {
            let Some(u_count) = u_sum.checked_sub(u_degree + 1) else {
                continue;
            };
            for v_degree in 1..=8usize {
                let Some(v_count) = v_sum.checked_sub(v_degree + 1) else {
                    continue;
                };
                // `checked_mul` bounds the pole count: `u_count`/`v_count` derive from
                // multiplicity sums and their product can exceed `usize`, so an overflowing
                // shape never matches `poles` (and never reaches the `with_capacity` below).
                if u_count > 0 && v_count > 0 && u_count.checked_mul(v_count) == Some(poles) {
                    return Some((
                        u_count,
                        v_count,
                        u_degree as u32,
                        v_degree as u32,
                        dimension,
                    ));
                }
            }
        }
    }
    None
}

pub fn scan_surface_carriers(bytes: &[u8]) -> HashMap<u16, Carrier> {
    let arrays = scan_arrays(bytes);
    let descriptors = surface_refs(bytes, &arrays);
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(23) {
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
        let Some(refs) = descriptors.get(&descriptor_attr) else {
            continue;
        };
        let Some(control) = arrays.f64s.get(&refs[0]) else {
            continue;
        };
        let Some(u_mult) = arrays.u16s.get(&refs[1]) else {
            continue;
        };
        let Some(v_mult) = arrays.u16s.get(&refs[2]) else {
            continue;
        };
        let Some(u_unique) = arrays.f64s.get(&refs[3]) else {
            continue;
        };
        let Some(v_unique) = arrays.f64s.get(&refs[4]) else {
            continue;
        };
        let Some((u_count, v_count, u_degree, v_degree, dimension)) =
            infer_surface_shape(control.len(), u_mult, v_mult)
        else {
            continue;
        };
        let mut points = Vec::with_capacity(u_count * v_count);
        let mut weights = (dimension == 4).then(Vec::new);
        for pole in control.chunks_exact(dimension) {
            let weight = if dimension == 4 { pole[3] } else { 1.0 };
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
        if points.len() != u_count * v_count {
            continue;
        }
        let u_expected = u_count + u_degree as usize + 1;
        let v_expected = v_count + v_degree as usize + 1;
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
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                u_count: u_count as u32,
                v_count: v_count as u32,
                control_points: points,
                weights,
                u_periodic: false,
                v_periodic: false,
            })),
            frame: None,
            orientation_reversed: false,
        });
    }
    out
}
