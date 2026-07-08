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
    let marker = *bytes.get(off + 2)?;
    if marker != 0x2b && marker != 0x2d {
        return None;
    }
    let mut p = off + 3;
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

fn expanded_knots(values: &[f64], multiplicities: &[u16]) -> Vec<f64> {
    values
        .iter()
        .zip(multiplicities)
        .filter(|(_, multiplicity)| **multiplicity > 0)
        .flat_map(|(value, multiplicity)| std::iter::repeat_n(*value, *multiplicity as usize))
        .collect()
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
        let descriptor = (p + 2..(p + 18).min(bytes.len()))
            .step_by(2)
            .filter_map(|at| u16_be(bytes, at))
            .find_map(|reference| descriptors.get(&reference));
        let Some(descriptor) = descriptor else {
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
        let knots = expanded_knots(unique_knots, multiplicities);
        if knots.len() != points.len() + descriptor.degree as usize + 1 {
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
            single_sample: false,
            frame: None,
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

fn surface_shape(
    control_len: usize,
    u_mult: &[u16],
    v_mult: &[u16],
) -> Option<(usize, usize, u32, u32, usize)> {
    let u_sum: usize = u_mult.iter().map(|v| *v as usize).sum();
    let v_sum: usize = v_mult.iter().map(|v| *v as usize).sum();
    for dimension in [4usize, 3] {
        if control_len % dimension != 0 {
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
                if u_count > 0 && v_count > 0 && u_count * v_count == poles {
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
            surface_shape(control.len(), u_mult, v_mult)
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
        let u_knots = expanded_knots(u_unique, u_mult);
        let v_knots = expanded_knots(v_unique, v_mult);
        if u_knots.len() != u_count + u_degree as usize + 1
            || v_knots.len() != v_count + v_degree as usize + 1
        {
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
            single_sample: false,
            frame: None,
        });
    }
    out
}
