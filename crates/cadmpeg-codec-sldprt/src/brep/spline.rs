// SPDX-License-Identifier: Apache-2.0
//! B-spline/list carrier tables.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface},
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::loss::LossNote;

use cadmpeg_core::decode::{index_from_u32, View};

use super::{CurveCarrier, SurfaceCarrier, LEN_TO_MM};

use crate::layout::bspline_array_header as arr_hdr;
use crate::layout::bspline_compact_array_header as compact_arr;
use crate::layout::bspline_curve_descriptor as curve_desc;
use crate::layout::bspline_surface_descriptor as surf_desc;
use crate::layout::parasolid_b_curve_short as b_curve;

#[derive(Debug, Default)]
struct Arrays {
    f64s: HashMap<u32, Vec<f64>>,
    u16s: HashMap<u32, Vec<u16>>,
    compact: HashMap<u32, Vec<CompactArray>>,
}

#[derive(Debug, Clone, Copy)]
struct CompactArray {
    values_at: usize,
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
    /// The descriptor's `periodic` logical.
    periodic: bool,
    control_count: usize,
    dimension: usize,
    /// The descriptor's `rational` logical, which tells a rational
    /// two-dimensional curve (`dimension` 3: `u·w v·w w`) from a polynomial
    /// model-space one with the same `dimension`.
    rational: bool,
    control_attr: u32,
    multiplicity_attr: u32,
    knot_attr: u32,
}

impl CurveDescriptor {
    /// A model-space curve: `x y z`, or `x·w y·w z·w w`.
    fn spatial(&self) -> bool {
        self.dimension == 4 || (self.dimension == 3 && !self.rational)
    }

    /// A parameter-space curve, the `b_curve` of an `SP_CURVE` (spec §7.6):
    /// `u v`, or `u·w v·w w`.
    fn planar(&self) -> bool {
        (self.dimension == 2 && !self.rational) || (self.dimension == 3 && self.rational)
    }
}

#[derive(Debug, Clone, Copy)]
struct SurfaceDescriptor {
    end: usize,
    attr: u32,
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
    refs: [u32; 5],
}

const MAX_ARRAY_VALUES: usize = 1_000_000;

fn logical_byte(bytes: &[u8], at: usize) -> Option<bool> {
    match bytes.get(at) {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}

fn parse_surface_descriptor_at(bytes: &[u8], payload: usize) -> Option<SurfaceDescriptor> {
    let p = payload;
    let mut fields = p + surf_desc::ATTR;
    let attr = cadmpeg_parasolid::pointer::read_at(bytes, &mut fields)?;
    let p = fields.checked_sub(2)?;
    let end = p.checked_add(surf_desc::LEN)?;
    if end > bytes.len() {
        return None;
    }
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
    let mut refs_end = p + surf_desc::ARRAY_REFS;
    let refs = super::native_refs::<5>(bytes, &mut refs_end)?;

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
        end: refs_end,
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

fn parse_surface_descriptor(bytes: &[u8], off: usize) -> Option<SurfaceDescriptor> {
    let mut candidates = super::record_starts(bytes, off, 0x7e)
        .filter_map(|p| parse_surface_descriptor_at(bytes, p));
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

pub(super) fn surface_descriptor_prefix_end(bytes: &[u8], offset: usize) -> Option<usize> {
    Some(parse_surface_descriptor(bytes, offset)?.end)
}

pub(super) fn curve_prefix_end(bytes: &[u8], off: usize) -> Option<usize> {
    match *bytes.get(off + 1)? {
        tag @ (0x7c | 0x86) => super::record_starts(bytes, off, tag)
            .filter_map(|p| {
                let attr = native_attr(bytes, p)?;
                let (descriptor, end) = if tag == 0x86 {
                    tripled_b_curve_nurbs(bytes, p).or_else(|| bare_b_curve_nurbs(bytes, p))?
                } else {
                    bare_b_curve_nurbs(bytes, p)?
                };
                (attr > 1 && descriptor > 1).then_some(end)
            })
            .min(),
        0x88 => super::record_starts(bytes, off, 0x88)
            .filter_map(|p| parse_curve_descriptor(bytes, p).map(|(_, _, end)| end))
            .min(),
        _ => None,
    }
}

pub(super) fn referenced_array_attrs(bytes: &[u8]) -> HashSet<u32> {
    let mut attrs = HashSet::new();
    for descriptor in scan_curve_descriptors(bytes, |_| true).into_values() {
        attrs.extend([
            descriptor.control_attr,
            descriptor.multiplicity_attr,
            descriptor.knot_attr,
        ]);
    }
    for descriptor in scan_surface_descriptors(bytes).into_values() {
        attrs.extend(descriptor.refs);
    }
    attrs
}

pub(super) fn array_record_end(bytes: &[u8], off: usize, attrs: &HashSet<u32>) -> Option<usize> {
    let tag = *bytes.get(off + 1)?;
    let width = match tag {
        0x2d | 0x80 => 8,
        0x7f => 2,
        _ => return None,
    };
    let p = array_body(bytes, off, tag)?;
    let (count, attr, data) = array_header(bytes, p)?;
    (count <= MAX_ARRAY_VALUES && attrs.contains(&attr) && attr > 1).then_some(())?;
    let count = cadmpeg_core::decode::bounded_len(
        cadmpeg_core::decode::u64_from_index(count),
        width,
        bytes.len().checked_sub(data)?,
    )?;
    data.checked_add(count.checked_mul(width)?)
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

fn array_header(bytes: &[u8], p: usize) -> Option<(usize, u32, usize)> {
    let count = View::u32_be_at(bytes, p + arr_hdr::COUNT).map(index_from_u32)?;
    let mut at = p + arr_hdr::ATTR;
    let attr = cadmpeg_parasolid::pointer::read_at(bytes, &mut at)?;
    Some((count, attr, at))
}

fn native_attr(bytes: &[u8], mut at: usize) -> Option<u32> {
    cadmpeg_parasolid::pointer::read_at(bytes, &mut at)
}

fn wrapper_fields(bytes: &[u8], p: usize) -> Option<(u32, u32, usize)> {
    let mut at = p;
    let attr = cadmpeg_parasolid::pointer::read_at(bytes, &mut at)?;
    View::u32_be_at(bytes, at)?;
    at += 4;
    super::native_refs::<5>(bytes, &mut at)?;
    matches!(bytes.get(at), Some(0x2b | 0x2d)).then_some(())?;
    at += b_curve::NURBS - b_curve::SENSE;
    let descriptor = cadmpeg_parasolid::pointer::read_at(bytes, &mut at)?;
    Some((attr, descriptor, at))
}

fn wrapper_start<T>(
    bytes: &[u8],
    off: usize,
    tag: u8,
    descriptors: &HashMap<u32, T>,
) -> Option<usize> {
    let mut candidates = super::record_starts(bytes, off, tag).filter(|p| {
        wrapper_fields(bytes, *p)
            .is_some_and(|(_, descriptor, _)| descriptors.contains_key(&descriptor))
    });
    if let Some(first) = candidates.next() {
        return candidates.next().is_none().then_some(first);
    }
    let mut p = off.checked_add(2)?;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    (super::word_at(bytes, p)? <= 0x7fff).then_some(p)
}

fn scan_arrays(
    bytes: &[u8],
    referenced_attrs: Option<&HashSet<u32>>,
    compact_attrs: Option<&HashSet<u32>>,
) -> Arrays {
    let mut arrays = Arrays::default();
    for off in 0..bytes.len().checked_sub(9).map_or(0, |end| end) {
        if bytes.get(off) == Some(&0) {
            let count = usize::from(bytes[off + compact_arr::COUNT]);
            if count > 0 {
                let mut values_at = off + compact_arr::ATTR;
                if let Some(attr) = cadmpeg_parasolid::pointer::read_at(bytes, &mut values_at)
                    .filter(|attr| compact_attrs.is_some_and(|attrs| attrs.contains(attr)))
                {
                    arrays.compact.entry(attr).or_default().push(CompactArray {
                        offset: off,
                        count,
                        values_at,
                    });
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
        let Some((count, attr, values_at)) = array_header(bytes, p) else {
            continue;
        };
        if attr <= 1
            || count > MAX_ARRAY_VALUES
            || referenced_attrs.is_some_and(|attrs| !attrs.contains(&attr))
        {
            continue;
        }
        if tag == 0x7f {
            if count
                .checked_mul(2)
                .and_then(|size| values_at.checked_add(size))
                .is_none_or(|end| end > bytes.len())
            {
                continue;
            }
            if arrays.u16s.contains_key(&attr) {
                continue;
            }
            let Some(values) = (0..count)
                .map(|i| View::u16_be_at(bytes, values_at + i * 2))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            arrays.u16s.entry(attr).or_insert(values);
        } else {
            if count
                .checked_mul(8)
                .and_then(|size| values_at.checked_add(size))
                .is_none_or(|end| end > bytes.len())
            {
                continue;
            }
            if arrays.f64s.contains_key(&attr) {
                continue;
            }
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

/// The f64 arrays that `attr` can name, in scan order, repeats included.
fn f64_array_candidates<'b>(
    bytes: &'b [u8],
    arrays: &'b Arrays,
    attr: u32,
) -> impl Iterator<Item = Vec<f64>> + 'b {
    arrays.f64s.get(&attr).cloned().into_iter().chain(
        arrays
            .compact
            .get(&attr)
            .into_iter()
            .flatten()
            .filter_map(move |compact| {
                compact
                    .count
                    .checked_mul(8)
                    .and_then(|size| compact.values_at.checked_add(size))
                    .filter(|&end| end <= bytes.len())?;
                (0..compact.count)
                    .map(|index| View::f64_be_at(bytes, compact.values_at + index * 8))
                    .collect::<Option<Vec<_>>>()
            }),
    )
}

/// The u16 arrays that `attr` can name, in scan order, repeats included.
fn u16_array_candidates<'b>(
    bytes: &'b [u8],
    arrays: &'b Arrays,
    attr: u32,
) -> impl Iterator<Item = Vec<u16>> + 'b {
    arrays.u16s.get(&attr).cloned().into_iter().chain(
        arrays
            .compact
            .get(&attr)
            .into_iter()
            .flatten()
            .filter_map(move |compact| {
                compact
                    .count
                    .checked_mul(2)
                    .and_then(|size| compact.values_at.checked_add(size))
                    .filter(|&end| end <= bytes.len())?;
                (0..compact.count)
                    .map(|index| View::u16_be_at(bytes, compact.values_at + index * 2))
                    .collect::<Option<Vec<_>>>()
            }),
    )
}

fn exact_f64_array(bytes: &[u8], arrays: &Arrays, attr: u32, count: usize) -> Option<Vec<f64>> {
    let mut candidates =
        f64_array_candidates(bytes, arrays, attr).filter(|values| values.len() == count);
    let selected = candidates.next()?;
    candidates
        .all(|candidate| candidate == selected)
        .then_some(selected)
}

fn descriptor_refs<const N: usize>(
    bytes: &[u8],
    base: usize,
    offsets: [usize; N],
) -> Option<([u32; N], usize)> {
    let mut references = [0; N];
    let mut extra = 0;
    let mut at = base;
    for (reference, offset) in references.iter_mut().zip(offsets) {
        at = base.checked_add(offset)?.checked_add(extra)?;
        let start = at;
        *reference = cadmpeg_parasolid::pointer::read_at(bytes, &mut at)?;
        extra = extra.checked_add(at.checked_sub(start)?.checked_sub(2)?)?;
    }
    Some((references, at))
}

fn parse_curve_descriptor(bytes: &[u8], p: usize) -> Option<(u32, CurveDescriptor, usize)> {
    let mut at = p + curve_desc::ATTR;
    let attr = cadmpeg_parasolid::pointer::read_at(bytes, &mut at)?;
    let p = at.checked_sub(2)?;
    let degree = View::u16_be_at(bytes, p + curve_desc::DEGREE).map(u32::from)?;
    let control_count =
        View::u32_be_at(bytes, p + curve_desc::CONTROL_COUNT).map(index_from_u32)?;
    let dimension = View::u16_be_at(bytes, p + curve_desc::DIMENSION).map(usize::from)?;
    let ([control_attr, multiplicity_attr, knot_attr], at) = descriptor_refs(
        bytes,
        p,
        [
            curve_desc::CONTROL_ATTR,
            curve_desc::MULTIPLICITY_ATTR,
            curve_desc::KNOT_ATTR,
        ],
    )?;
    (attr > 1 && (2..=4).contains(&dimension) && control_count > 0).then_some(())?;
    Some((
        attr,
        CurveDescriptor {
            degree,
            periodic: logical_byte(bytes, p + curve_desc::PERIODIC)?,
            control_count,
            dimension,
            rational: logical_byte(bytes, p + curve_desc::RATIONAL)?,
            control_attr,
            multiplicity_attr,
            knot_attr,
        },
        at,
    ))
}

fn scan_curve_descriptors(
    bytes: &[u8],
    kind: fn(&CurveDescriptor) -> bool,
) -> HashMap<u32, CurveDescriptor> {
    let mut out = HashMap::new();
    for (off, _) in bytes.windows(2).enumerate() {
        for p in super::record_starts(bytes, off, 0x88) {
            if let Some((attr, descriptor, _)) = parse_curve_descriptor(bytes, p) {
                if kind(&descriptor) {
                    out.entry(attr).or_insert(descriptor);
                }
            }
        }
    }
    out
}

fn curve_descriptor<'a>(
    bytes: &[u8],
    attr_at: usize,
    descriptors: &'a HashMap<u32, CurveDescriptor>,
) -> Option<&'a CurveDescriptor> {
    curve_descriptor_link(bytes, attr_at, descriptors)
        .and_then(|(reference, _)| descriptors.get(&reference))
}

/// The `B_CURVE` pointers between its node identifier and its sense byte.
const B_CURVE_POINTERS: usize = (b_curve::SENSE - b_curve::ATTRIBUTES) / std::mem::size_of::<u16>();
/// Bytes a tripled pointer cell adds to a short pointer (`sldprt.md` §4.3).
const TRIPLED_CELL_EXTRA: usize = 1;
/// The flag byte that ends a tripled pointer cell (`sldprt.md` §4.3).
const TRIPLED_CELL_FLAG: u8 = 1;

fn bare_b_curve_nurbs(bytes: &[u8], attr_at: usize) -> Option<(u32, usize)> {
    wrapper_fields(bytes, attr_at).map(|(_, descriptor, end)| (descriptor, end))
}

/// The `nurbs` pointer of the tripled `B_CURVE` whose attribute is at
/// `attr_at` and the end of that pointer, when the record frames: five
/// `[hi][lo][01]` cells, then the sense byte and the `nurbs` cell.
fn tripled_b_curve_nurbs(bytes: &[u8], attr_at: usize) -> Option<(u32, usize)> {
    let mut first = attr_at;
    cadmpeg_parasolid::pointer::read_at(bytes, &mut first)?;
    View::u32_be_at(bytes, first)?;
    first = first.checked_add(4)?;
    let cell = 2 + TRIPLED_CELL_EXTRA;
    for index in 0..B_CURVE_POINTERS {
        let flag_at = first.checked_add(index * cell + 2)?;
        if bytes.get(flag_at) != Some(&TRIPLED_CELL_FLAG) {
            return None;
        }
    }
    let sense_at = first.checked_add(B_CURVE_POINTERS * cell)?;
    if !matches!(bytes.get(sense_at), Some(b'+' | b'-')) {
        return None;
    }
    let nurbs_at = sense_at.checked_add(1)?;
    let nurbs = View::u16_be_at(bytes, nurbs_at)?;
    Some((u32::from(nurbs), nurbs_at.checked_add(2)?))
}

/// Expand a compressed knot vector by its per-value multiplicities, refusing
/// the expansion when the running length exceeds `expected`.
///
/// A NURBS knot vector must have exactly `control_count + degree + 1` entries,
/// so `expected` is a hard upper bound. Checking the multiplicities against it
/// incrementally stops an untrusted `u16` multiplicity array (each entry up to
/// `65535`, over a million-entry table) from growing a multi-hundred-gigabyte
/// `Vec` before the post-hoc length check would discard it. Returns `None` the
/// moment the accumulated length would exceed `expected`.
fn expanded_knots(values: &[f64], multiplicities: &[u16], expected: usize) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    for (value, &multiplicity) in values.iter().zip(multiplicities) {
        if multiplicity == 0 {
            continue;
        }
        if out
            .len()
            .checked_add(usize::from(multiplicity))
            .is_none_or(|next_len| next_len > expected)
        {
            return None;
        }
        out.extend(std::iter::repeat_n(*value, usize::from(multiplicity)));
    }
    Some(out)
}

fn unique_knots(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut values = Vec::new();
    let mut multiplicities = Vec::<usize>::new();
    for &knot in knots {
        if values.last() == Some(&knot) {
            if let Some(multiplicity) = multiplicities.last_mut() {
                *multiplicity += 1;
            }
        } else {
            values.push(knot);
            multiplicities.push(1);
        }
    }
    (values, multiplicities)
}

fn multiplicity_sum(values: &[u16]) -> Option<usize> {
    values
        .iter()
        .try_fold(0usize, |sum, &value| sum.checked_add(usize::from(value)))
}

fn array_span(bytes: &[u8], tag: u8, attr: u32) -> Option<(usize, usize)> {
    for off in 0..bytes.len().checked_sub(9).map_or(0, |end| end) {
        let Some(p) = array_body(bytes, off, tag) else {
            continue;
        };
        let Some(count) = View::u32_be_at(bytes, p + arr_hdr::COUNT).map(index_from_u32) else {
            continue;
        };
        if array_header(bytes, p).map(|(_, attr, _)| attr) == Some(attr) {
            return Some((array_header(bytes, p)?.2, count));
        }
    }
    None
}

fn array_spans(bytes: &[u8], arrays: &Arrays, tag: u8, attr: u32) -> Vec<ArraySpan> {
    let mut spans = Vec::new();
    for off in 0..bytes.len().checked_sub(9).map_or(0, |end| end) {
        let Some(p) = array_body(bytes, off, tag) else {
            continue;
        };
        let Some(count) = View::u32_be_at(bytes, p + arr_hdr::COUNT)
            .and_then(|value| usize::try_from(value).ok())
        else {
            continue;
        };
        if count <= MAX_ARRAY_VALUES
            && array_header(bytes, p).map(|(_, attr, _)| attr) == Some(attr)
        {
            spans.push(ArraySpan {
                start: array_header(bytes, p).map_or(p + arr_hdr::LEN, |(_, _, at)| at),
                count,
            });
        }
    }
    for array in arrays.compact.get(&attr).into_iter().flatten() {
        spans.push(ArraySpan {
            start: array.values_at,
            count: array.count,
        });
    }
    spans.sort_by_key(|span| (span.start, span.count));
    spans.dedup();
    spans
}

fn f64_values(bytes: &[u8], span: ArraySpan) -> Option<Vec<f64>> {
    (0..span.count)
        .map(|index| {
            span.start
                .checked_add(index.checked_mul(8)?)
                .and_then(|offset| View::f64_be_at(bytes, offset))
        })
        .collect()
}

fn u16_values(bytes: &[u8], span: ArraySpan) -> Option<Vec<u16>> {
    (0..span.count)
        .map(|index| {
            span.start
                .checked_add(index.checked_mul(2)?)
                .and_then(|offset| View::u16_be_at(bytes, offset))
        })
        .collect()
}

fn unique_control_span(
    bytes: &[u8],
    arrays: &Arrays,
    attr: u32,
    old_values: &[f64],
) -> Option<ArraySpan> {
    let mut spans = array_spans(bytes, arrays, 0x2d, attr);
    spans.retain(|span| span.count == old_values.len());
    if let [span] = spans.as_slice() {
        return Some(*span);
    }
    let mut selected = None;
    for span in spans {
        let Some(values) = f64_values(bytes, span) else {
            continue;
        };
        if values.iter().zip(old_values).all(|(native, expected)| {
            let scale = native.abs().max(expected.abs()).max(1.0);
            (native - expected).abs() <= 16.0 * f64::EPSILON * scale
        }) && selected.replace(span).is_some()
        {
            return None;
        }
    }
    selected
}

fn unique_surface_knot_span(
    bytes: &[u8],
    arrays: &Arrays,
    (knot_attr, multiplicity_attr): (u32, u32),
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
    let end = span.start.checked_add(values.len().checked_mul(8)?)?;
    let slots = bytes.get_mut(span.start..end)?;
    for (slot, value) in slots.chunks_exact_mut(8).zip(values) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    Some(())
}

fn patch_f64_array(bytes: &mut [u8], tag: u8, attr: u32, values: &[f64]) -> Option<()> {
    let (start, count) = array_span(bytes, tag, attr)?;
    if count != values.len() {
        return None;
    }
    patch_f64_span(bytes, ArraySpan { start, count }, values)
}

fn append_homogeneous_pole(
    out: &mut Vec<f64>,
    point: FinitePoint3,
    weight: Option<f64>,
    scale: f64,
) -> Option<()> {
    let point = point.get();
    let factor = weight.unwrap_or(1.0);
    if factor.abs() <= f64::EPSILON {
        return None;
    }
    let homogeneous = [point.x, point.y, point.z].map(|value| value * scale * factor);
    if !homogeneous.iter().all(|value| value.is_finite()) {
        return None;
    }
    out.extend(homogeneous);
    if let Some(weight) = weight {
        out.push(weight);
    }
    Some(())
}

fn homogeneous_poles(poles: &NurbsPoles3<FinitePoint3>, scale: f64) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    match poles {
        NurbsPoles3::Polynomial { points } => {
            for point in points {
                append_homogeneous_pole(&mut out, *point, None, scale)?;
            }
        }
        NurbsPoles3::Rational { points } => {
            for pole in points {
                append_homogeneous_pole(&mut out, pole.point, Some(pole.weight.get()), scale)?;
            }
        }
    }
    Some(out)
}

fn homogeneous_grid_poles(poles: &NurbsPoleGrid<FinitePoint3>, scale: f64) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    match poles {
        NurbsPoleGrid::Polynomial { rows } => {
            for point in rows.iter().flatten() {
                append_homogeneous_pole(&mut out, *point, None, scale)?;
            }
        }
        NurbsPoleGrid::Rational { rows } => {
            for pole in rows.iter().flatten() {
                append_homogeneous_pole(&mut out, pole.point, Some(pole.weight.get()), scale)?;
            }
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
) -> Result<Option<()>, cadmpeg_core::CodecError> {
    if old.degree() != new.degree()
        || old.control_points().len() != new.control_points().len()
        || old.weights().is_some() != new.weights().is_some()
        || old.periodic() != new.periodic()
    {
        return Ok(None);
    }
    let (old_unique, old_mult) = unique_knots(old.knots());
    let (new_unique, new_mult) = unique_knots(new.knots());
    if old_mult != new_mult || old_unique.len() != new_unique.len() {
        return Ok(None);
    }
    let mut p = wrapper_offset.checked_add(2).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("Parasolid patch wrapper offset overflow")
    })?;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let descriptors = scan_curve_descriptors(bytes, CurveDescriptor::spatial);
    let Some(descriptor) = curve_descriptor(bytes, p, &descriptors) else {
        return Ok(None);
    };
    if descriptor.degree != old.degree()
        || descriptor.control_count != old.control_points().len()
        || descriptor.dimension != if old.weights().is_some() { 4 } else { 3 }
    {
        return Ok(None);
    }
    let Some(poles) = homogeneous_poles(new.pole_rows(), scale) else {
        return Ok(None);
    };
    if patch_f64_array(bytes, 0x2d, descriptor.control_attr, &poles).is_none() {
        return Ok(None);
    }
    Ok(patch_f64_array(
        bytes,
        0x80,
        descriptor.knot_attr,
        &new_unique,
    ))
}

/// Patch retained surface pole and knot arrays without changing their storage shape.
pub(crate) fn patch_nurbs_surface(
    bytes: &mut [u8],
    wrapper_offset: usize,
    old: &NurbsSurface,
    new: &NurbsSurface,
    scale: f64,
) -> Result<Option<()>, cadmpeg_core::CodecError> {
    if old.u_degree() != new.u_degree()
        || old.v_degree() != new.v_degree()
        || old.u_count() != new.u_count()
        || old.v_count() != new.v_count()
        || old.poles().len() != new.poles().len()
        || old.weights().is_some() != new.weights().is_some()
        || old.u_periodic() != new.u_periodic()
        || old.v_periodic() != new.v_periodic()
    {
        return Ok(None);
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
        return Ok(None);
    }
    let descriptors = scan_surface_descriptors(bytes);
    let compact_attrs = descriptors
        .values()
        .flat_map(|descriptor| descriptor.refs)
        .collect::<HashSet<_>>();
    let arrays = scan_arrays(bytes, Some(&compact_attrs), Some(&compact_attrs));
    let mut p = wrapper_offset.checked_add(2).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("Parasolid patch wrapper offset overflow")
    })?;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let Some(descriptor_at) = p.checked_add(17) else {
        return Ok(None);
    };
    let Some(descriptor_attr) = wrapper_fields(bytes, p)
        .map(|(_, descriptor, _)| descriptor)
        .or_else(|| super::word_at(bytes, descriptor_at))
    else {
        return Ok(None);
    };
    let Some(descriptor) = descriptors.get(&descriptor_attr) else {
        return Ok(None);
    };
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
        return Ok(None);
    }
    let Some(old_poles) = homogeneous_grid_poles(old.pole_grid(), scale) else {
        return Ok(None);
    };
    let Some(poles) = homogeneous_grid_poles(new.pole_grid(), scale) else {
        return Ok(None);
    };
    let Some(control_span) = unique_control_span(bytes, &arrays, control_attr, &old_poles) else {
        return Ok(None);
    };
    let Some(u_knot_span) = unique_surface_knot_span(
        bytes,
        &arrays,
        (u_knot_attr, descriptor.refs[1]),
        descriptor.u_knot_count,
        &old_u,
        &old_u_mult,
    ) else {
        return Ok(None);
    };
    let Some(v_knot_span) = unique_surface_knot_span(
        bytes,
        &arrays,
        (v_knot_attr, descriptor.refs[2]),
        descriptor.v_knot_count,
        &old_v,
        &old_v_mult,
    ) else {
        return Ok(None);
    };
    if patch_f64_span(bytes, control_span, &poles).is_none()
        || patch_f64_span(bytes, u_knot_span, &new_u).is_none()
    {
        return Ok(None);
    }
    Ok(patch_f64_span(bytes, v_knot_span, &new_v))
}

/// Every compact NURBS curve carrier in `bytes`, keyed by attribute id.
///
/// A candidate offset whose bytes are not a spline record is not a refusal: the
/// scan sweeps every offset. A record whose poles, knots and weights do parse
/// but do not pair is one, and it is recorded in `refusals` as a loss naming
/// the attribute id it belongs to, so the decode reports it rather than
/// deleting the carrier in silence.
pub(crate) fn scan_curve_carriers(
    bytes: &[u8],
    refusals: &mut Vec<LossNote>,
) -> HashMap<u32, CurveCarrier> {
    let descriptors = scan_curve_descriptors(bytes, CurveDescriptor::spatial);
    let referenced_attrs = descriptors
        .values()
        .flat_map(|descriptor| {
            [
                descriptor.control_attr,
                descriptor.multiplicity_attr,
                descriptor.knot_attr,
            ]
        })
        .collect::<HashSet<_>>();
    let arrays = scan_arrays(bytes, Some(&referenced_attrs), None);
    let mut out = HashMap::new();
    for off in 0..bytes.len().checked_sub(6).map_or(0, |end| end) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x86]) {
            continue;
        }
        let Some(p) = wrapper_start(bytes, off, 134, &descriptors) else {
            continue;
        };
        let Some(attr) = native_attr(bytes, p) else {
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
        if !unique_knots.iter().all(|value| value.is_finite()) || !unique_knots.is_sorted() {
            continue;
        }
        let mut points = Vec::new();
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
        let expected = points.len() + index_from_u32(descriptor.degree) + 1;
        let Some(knots) = expanded_knots(unique_knots, multiplicities, expected) else {
            continue;
        };
        if knots.len() != expected {
            continue;
        }
        let nurbs = match cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            descriptor.degree,
            knots,
            points,
            weights,
            false,
        ) {
            Ok(nurbs) => nurbs,
            Err(error) => {
                let note = crate::loss::spline_lane_refusal(format_args!(
                    "curve carrier attribute {attr}: {error}"
                ));
                refusals.push(note);
                continue;
            }
        };
        out.entry(attr).or_insert(CurveCarrier {
            marker_reversed: None,
            attr,
            offset: off,
            end: off + 2,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
            parameter_range: None,
        });
    }
    out
}

/// Two-dimensional `00 86` B-curves by `attr`, in stored support parameters
/// (spec §7.6).
pub(super) fn scan_pcurve_carriers(bytes: &[u8]) -> HashMap<u32, PcurveNurbs> {
    let mut out = HashMap::new();
    let descriptors = scan_curve_descriptors(bytes, CurveDescriptor::planar);
    if descriptors.is_empty() {
        return out;
    }
    let referenced_attrs = descriptors
        .values()
        .flat_map(|descriptor| {
            [
                descriptor.control_attr,
                descriptor.multiplicity_attr,
                descriptor.knot_attr,
            ]
        })
        .collect::<HashSet<_>>();
    let arrays = scan_arrays(bytes, Some(&referenced_attrs), None);
    for off in 0..bytes.len().checked_sub(6).map_or(0, |end| end) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x86]) {
            continue;
        }
        let Some(p) = wrapper_start(bytes, off, 134, &descriptors) else {
            continue;
        };
        let Some(attr) = native_attr(bytes, p) else {
            continue;
        };
        let Some(descriptor) = curve_descriptor(bytes, p, &descriptors) else {
            continue;
        };
        let (Some(control), Some(multiplicities), Some(unique_knots)) = (
            arrays.f64s.get(&descriptor.control_attr),
            arrays.u16s.get(&descriptor.multiplicity_attr),
            arrays.f64s.get(&descriptor.knot_attr),
        ) else {
            continue;
        };
        if Some(control.len()) != descriptor.control_count.checked_mul(descriptor.dimension)
            || !unique_knots.iter().all(|value| value.is_finite())
            || !unique_knots.is_sorted()
        {
            continue;
        }
        let mut points = Vec::new();
        let mut weights = descriptor.rational.then(Vec::new);
        for pole in control.chunks_exact(descriptor.dimension) {
            let weight = if descriptor.rational { pole[2] } else { 1.0 };
            if pole.iter().any(|value| !value.is_finite()) || weight.abs() <= f64::EPSILON {
                break;
            }
            points.push(Point2::new(pole[0] / weight, pole[1] / weight));
            if let Some(values) = &mut weights {
                values.push(weight);
            }
        }
        if points.len() != descriptor.control_count {
            continue;
        }
        let expected = points.len() + index_from_u32(descriptor.degree) + 1;
        let Some(knots) = expanded_knots(unique_knots, multiplicities, expected)
            .filter(|knots| knots.len() == expected)
        else {
            continue;
        };
        if let Ok(nurbs) = PcurveNurbs::from_lanes(
            descriptor.degree,
            knots,
            points,
            weights,
            descriptor.periodic,
        ) {
            out.entry(attr).or_insert(nurbs);
        }
    }
    out
}

fn scan_surface_descriptors(bytes: &[u8]) -> HashMap<u32, SurfaceDescriptor> {
    let mut out = HashMap::new();
    for off in 0..bytes.len().checked_sub(1).map_or(0, |end| end) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x7e]) {
            continue;
        }
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
    if unique.len() != multiplicities.len() {
        return None;
    }
    let declared = declared_multiplicities(multiplicities, declared_count)?;
    Some((unique.get(..declared_count)?, declared))
}

/// The first `declared_count` multiplicities, when each is non-zero and every
/// later one is zero.
fn declared_multiplicities(multiplicities: &[u16], declared_count: usize) -> Option<&[u16]> {
    if declared_count == 0 {
        return None;
    }
    let (declared, rest) = multiplicities.split_at_checked(declared_count)?;
    (declared.iter().all(|&value| value != 0) && rest.iter().all(|&value| value == 0))
        .then_some(declared)
}

#[derive(PartialEq)]
struct SurfaceKnotValues {
    unique: Vec<f64>,
    multiplicities: Vec<u16>,
}

/// The valid multiplicities of one array length: none, one, or more than one.
enum Multiplicities {
    One(Vec<u16>),
    Many,
}

fn surface_knot_values(
    bytes: &[u8],
    arrays: &Arrays,
    knot_attr: u32,
    multiplicity_attr: u32,
    declared_count: usize,
) -> Option<SurfaceKnotValues> {
    // The knots resolve only when every candidate pair agrees, so a second
    // distinct pair ends the search. The valid multiplicities of a length do
    // not depend on the knot values.
    let mut by_count = HashMap::<usize, Multiplicities>::new();
    for multiplicities in u16_array_candidates(bytes, arrays, multiplicity_attr) {
        let Some(valid) = declared_multiplicities(&multiplicities, declared_count) else {
            continue;
        };
        let valid = valid.to_vec();
        match by_count.get(&multiplicities.len()) {
            None => {
                by_count.insert(multiplicities.len(), Multiplicities::One(valid));
            }
            Some(Multiplicities::One(first)) if *first == valid => {}
            Some(_) => {
                by_count.insert(multiplicities.len(), Multiplicities::Many);
            }
        }
    }
    let mut resolved = None::<SurfaceKnotValues>;
    for unique in f64_array_candidates(bytes, arrays, knot_attr) {
        let multiplicities = match by_count.get(&unique.len()) {
            None => continue,
            Some(Multiplicities::Many) => return None,
            Some(Multiplicities::One(multiplicities)) => multiplicities,
        };
        let Some(unique) = unique.get(..declared_count) else {
            continue;
        };
        let candidate = SurfaceKnotValues {
            unique: unique.to_vec(),
            multiplicities: multiplicities.clone(),
        };
        match &resolved {
            None => resolved = Some(candidate),
            Some(first) if *first == candidate => {}
            Some(_) => return None,
        }
    }
    resolved
}

/// Every compact NURBS surface carrier in `bytes`, keyed by attribute id.
///
/// `refusals` carries the same meaning as in [`scan_curve_carriers`].
pub(crate) fn scan_surface_carriers(
    bytes: &[u8],
    refusals: &mut Vec<LossNote>,
) -> HashMap<u32, SurfaceCarrier> {
    let descriptors = scan_surface_descriptors(bytes);
    let compact_attrs = descriptors
        .values()
        .flat_map(|descriptor| descriptor.refs)
        .collect::<HashSet<_>>();
    let arrays = scan_arrays(bytes, Some(&compact_attrs), Some(&compact_attrs));
    let mut out = HashMap::new();
    for off in 0..bytes.len().checked_sub(1).map_or(0, |end| end) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x7c]) {
            continue;
        }
        let Some(p) = wrapper_start(bytes, off, 124, &descriptors) else {
            continue;
        };
        let Some(attr) = native_attr(bytes, p) else {
            continue;
        };
        let Some(descriptor_at) = p.checked_add(17) else {
            continue;
        };
        let Some(descriptor_attr) = wrapper_fields(bytes, p)
            .map(|(_, descriptor, _)| descriptor)
            .or_else(|| super::word_at(bytes, descriptor_at))
        else {
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
        let Some(SurfaceKnotValues {
            unique: u_unique,
            multiplicities: u_mult,
        }) = surface_knot_values(
            bytes,
            &arrays,
            descriptor.refs[3],
            descriptor.refs[1],
            descriptor.u_knot_count,
        )
        else {
            continue;
        };
        let Some(SurfaceKnotValues {
            unique: v_unique,
            multiplicities: v_mult,
        }) = surface_knot_values(
            bytes,
            &arrays,
            descriptor.refs[4],
            descriptor.refs[2],
            descriptor.v_knot_count,
        )
        else {
            continue;
        };
        if control.len() != expected_control_values {
            continue;
        }
        let Some(u_expected) = descriptor
            .u_count
            .checked_add(index_from_u32(descriptor.u_degree))
            .and_then(|value| value.checked_add(1))
        else {
            continue;
        };
        let Some(v_expected) = descriptor
            .v_count
            .checked_add(index_from_u32(descriptor.v_degree))
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
            || !u_unique.is_sorted()
            || !v_unique.is_sorted()
        {
            continue;
        }
        let mut points = Vec::new();
        let dimension = if descriptor.rational {
            descriptor.dimension
        } else {
            3
        };
        let mut weights = descriptor.rational.then(Vec::new);
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
        let pole_rows = points
            .chunks(descriptor.v_count)
            .map(<[_]>::to_vec)
            .collect::<Vec<_>>();
        let weight_rows = weights.map(|values| {
            values
                .chunks(descriptor.v_count)
                .map(<[_]>::to_vec)
                .collect::<Vec<_>>()
        });
        let nurbs = match cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
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
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(pole_rows, weight_rows),
            false,
        ) {
            Ok(nurbs) => nurbs,
            Err(error) => {
                let note = crate::loss::spline_lane_refusal(format_args!(
                    "surface carrier attribute {attr}: {error}"
                ));
                refusals.push(note);
                continue;
            }
        };
        out.entry(attr).or_insert(SurfaceCarrier {
            attr,
            offset: off,
            end: off + 2,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
            orientation_reversed: false,
            marker_reversed: false,
        });
    }
    out
}

/// Exact-read spans of the admitted NURBS carriers of one stream body.
///
/// Each carrier names its wrapper span, its descriptor span, and the semantic
/// value spans of the array lanes its admission consumed. Wrapper, descriptor,
/// and array spans are separate entries. Array value spans cover the semantic
/// prefix only: physical slots past the descriptor knot counts (surfaces) or
/// past knot-vector completion (curves) are not recorded. Compact array
/// records carry no tag byte; their spans use tag zero.
pub(super) fn spline_spans(body: &[u8]) -> Vec<super::read_ledger::RawSpan> {
    let mut spans = Vec::new();
    curve_spans(body, &mut spans);
    surface_spans(body, &mut spans);
    spans
}

/// One array record in sweep order: plain records name their tag byte,
/// compact records carry no tag byte and use tag zero.
#[derive(Debug, Clone, Copy)]
struct ArrayRecord {
    record_start: usize,
    values_at: usize,
    count: usize,
    tag: u8,
}

fn push_raw_span(
    spans: &mut Vec<super::read_ledger::RawSpan>,
    body: &[u8],
    offset: usize,
    end: usize,
    tag: u8,
    record: u32,
) {
    if let Some(length) = end.checked_sub(offset) {
        if body.get(offset..end).is_some() {
            if let Some(span) = super::read_ledger::RawSpan::at(offset, length, tag, record) {
                spans.push(span);
            }
        }
    }
}

fn push_values_span(
    spans: &mut Vec<super::read_ledger::RawSpan>,
    body: &[u8],
    record: &ArrayRecord,
    semantic_count: usize,
    element_size: usize,
    record_id: u32,
) {
    if let Some(values_end) = semantic_count
        .checked_mul(element_size)
        .and_then(|size| record.values_at.checked_add(size))
    {
        push_raw_span(
            spans,
            body,
            record.record_start,
            values_end,
            record.tag,
            record_id,
        );
    }
}

/// Plain array records with one attribute, in body order. `f64_lane` selects
/// the `0x2d`/`0x80` value tags; otherwise the `0x7f` multiplicity tag.
fn plain_array_records(bytes: &[u8], attr: u32, f64_lane: bool) -> Vec<ArrayRecord> {
    let mut out = Vec::new();
    for off in 0..bytes.len().checked_sub(9).map_or(0, |end| end) {
        let tag = match bytes.get(off..off + 2) {
            Some([0x00, tag @ (0x2d | 0x7f | 0x80)]) => *tag,
            _ => continue,
        };
        if (tag == 0x7f) == f64_lane {
            continue;
        }
        let Some(p) = array_body(bytes, off, tag) else {
            continue;
        };
        let Some(count) = View::u32_be_at(bytes, p + arr_hdr::COUNT).map(index_from_u32) else {
            continue;
        };
        if array_header(bytes, p).map(|(_, attr, _)| attr) != Some(attr) {
            continue;
        }
        if attr <= 1 || count > MAX_ARRAY_VALUES {
            continue;
        }
        let Some(values_at) = array_header(bytes, p).map(|(_, _, at)| at) else {
            continue;
        };
        let element_size = if f64_lane { 8 } else { 2 };
        let Some(values_end) = count
            .checked_mul(element_size)
            .and_then(|size| values_at.checked_add(size))
        else {
            continue;
        };
        if values_end > bytes.len() {
            continue;
        }
        out.push(ArrayRecord {
            record_start: off,
            values_at,
            count,
            tag,
        });
    }
    out
}

/// Compact array records with one attribute, in body order. Compact records
/// carry no tag byte, so their spans use tag zero.
fn compact_array_records(
    bytes: &[u8],
    arrays: &Arrays,
    attr: u32,
    f64_lane: bool,
) -> Vec<ArrayRecord> {
    let mut out = Vec::new();
    for array in arrays.compact.get(&attr).into_iter().flatten() {
        let values_at = array.values_at;
        let element_size = if f64_lane { 8 } else { 2 };
        let Some(values_end) = array
            .count
            .checked_mul(element_size)
            .and_then(|size| values_at.checked_add(size))
        else {
            continue;
        };
        if values_end > bytes.len() {
            continue;
        }
        out.push(ArrayRecord {
            record_start: array.offset,
            values_at,
            count: array.count,
            tag: 0,
        });
    }
    out
}

fn read_record_f64(bytes: &[u8], record: &ArrayRecord) -> Option<Vec<f64>> {
    f64_values(
        bytes,
        ArraySpan {
            start: record.values_at,
            count: record.count,
        },
    )
}

fn read_record_u16(bytes: &[u8], record: &ArrayRecord) -> Option<Vec<u16>> {
    u16_values(
        bytes,
        ArraySpan {
            start: record.values_at,
            count: record.count,
        },
    )
}

/// The attribute position of a curve or surface wrapper record.
fn wrapper_attr_pos(bytes: &[u8], off: usize) -> Option<usize> {
    let mut p = off.checked_add(2)?;
    if bytes.get(p) == Some(&0xff) {
        p = p.checked_add(1)?;
    }
    super::word_at(bytes, p)?;
    Some(p)
}

/// Curve descriptor records keyed by descriptor attribute, each with the
/// body-relative span of its fixed record extent.
fn curve_descriptor_offsets(bytes: &[u8]) -> HashMap<u32, (usize, usize)> {
    let mut out = HashMap::new();
    for (off, _) in bytes.windows(2).enumerate() {
        for p in super::record_starts(bytes, off, 0x88) {
            if let Some((attr, descriptor, end)) = parse_curve_descriptor(bytes, p) {
                if descriptor.spatial() {
                    out.entry(attr).or_insert((off, end));
                }
            }
        }
    }
    out
}

/// The descriptor attribute the record at `attr_at` links, with the end of
/// the link. A tripled or bare `B_CURVE` links the descriptor its `nurbs`
/// pointer names, or none. A record that neither form frames links the first
/// descriptor pointer between the node identifier and the end of the short
/// record.
fn curve_descriptor_link(
    bytes: &[u8],
    attr_at: usize,
    descriptors: &HashMap<u32, CurveDescriptor>,
) -> Option<(u32, usize)> {
    // The tripled frame checks five cell flags, so it goes first: a tripled
    // record can also read as a bare one.
    if let Some((nurbs, end)) =
        tripled_b_curve_nurbs(bytes, attr_at).or_else(|| bare_b_curve_nurbs(bytes, attr_at))
    {
        return descriptors.contains_key(&nurbs).then_some((nurbs, end));
    }
    let mut start = attr_at;
    cadmpeg_parasolid::pointer::read_at(bytes, &mut start)?;
    start = start.checked_add(4)?;
    let limit = attr_at
        .checked_add(b_curve::LEN - 1)?
        .min(bytes.len().checked_sub(1).map_or(0, |end| end));
    (start..limit)
        .filter_map(|at| super::word_at(bytes, at).map(|reference| (at, reference)))
        .find_map(|(at, reference)| {
            descriptors
                .contains_key(&reference)
                .then_some(reference)
                .zip(at.checked_add(2))
        })
}

/// Slots of the knot lanes the admission expansion consumed: the count of
/// leading slots whose multiplicities complete the knot vector. Reserved
/// slots past completion are not semantic.
fn knot_consumed(unique: &[f64], multiplicities: &[u16], expected: usize) -> Option<usize> {
    let mut total = 0usize;
    for (index, multiplicity) in multiplicities.iter().enumerate() {
        if index >= unique.len() {
            break;
        }
        if *multiplicity == 0 {
            continue;
        }
        total = total.checked_add(usize::from(*multiplicity))?;
        if total > expected {
            return None;
        }
        if total == expected {
            return Some(index + 1);
        }
    }
    (total == expected).then_some(multiplicities.len().min(unique.len()))
}

fn curve_spans(body: &[u8], spans: &mut Vec<super::read_ledger::RawSpan>) {
    let mut refusals = Vec::new();
    let carriers = scan_curve_carriers(body, &mut refusals);
    if carriers.is_empty() {
        return;
    }
    let descriptors = scan_curve_descriptors(body, CurveDescriptor::spatial);
    let desc_offsets = curve_descriptor_offsets(body);
    for (wrapper_attr, carrier) in &carriers {
        let off = carrier.offset;
        let Some(attr_at) = wrapper_attr_pos(body, off) else {
            continue;
        };
        let Some((desc_attr, link_end)) = curve_descriptor_link(body, attr_at, &descriptors) else {
            continue;
        };
        push_raw_span(spans, body, off, link_end, 0x86, *wrapper_attr);
        let Some(&(desc_off, desc_end)) = desc_offsets.get(&desc_attr) else {
            continue;
        };
        push_raw_span(spans, body, desc_off, desc_end, 0x88, desc_attr);
        let Some(descriptor) = descriptors.get(&desc_attr) else {
            continue;
        };
        let Some(expected_control) = descriptor.control_count.checked_mul(descriptor.dimension)
        else {
            continue;
        };
        if let Some(record) = plain_array_records(body, descriptor.control_attr, true)
            .into_iter()
            .next()
        {
            if record.count == expected_control {
                push_values_span(
                    spans,
                    body,
                    &record,
                    record.count,
                    8,
                    descriptor.control_attr,
                );
            }
        }
        let knot_record = plain_array_records(body, descriptor.knot_attr, true)
            .into_iter()
            .next();
        let mult_record = plain_array_records(body, descriptor.multiplicity_attr, false)
            .into_iter()
            .next();
        if let (Some(knot_record), Some(mult_record)) = (knot_record, mult_record) {
            let knots = read_record_f64(body, &knot_record);
            let mults = read_record_u16(body, &mult_record);
            if let (Some(knots), Some(mults)) = (knots, mults) {
                let Some(expected) = descriptor
                    .control_count
                    .checked_add(index_from_u32(descriptor.degree))
                    .and_then(|count| count.checked_add(1))
                else {
                    continue;
                };
                if let Some(consumed) = knot_consumed(&knots, &mults, expected) {
                    push_values_span(spans, body, &knot_record, consumed, 8, descriptor.knot_attr);
                    push_values_span(
                        spans,
                        body,
                        &mult_record,
                        consumed,
                        2,
                        descriptor.multiplicity_attr,
                    );
                }
            }
        }
    }
}

/// Surface descriptor records keyed by descriptor attribute, each with the
/// body-relative span of its fixed record extent.
fn surface_descriptor_offsets(bytes: &[u8]) -> HashMap<u32, (usize, usize)> {
    let mut out = HashMap::new();
    for off in 0..bytes.len().checked_sub(1).map_or(0, |end| end) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x7e]) {
            continue;
        }
        let Some(descriptor) = parse_surface_descriptor(bytes, off) else {
            continue;
        };
        let Some(end) = surface_descriptor_prefix_end(bytes, off) else {
            continue;
        };
        out.entry(descriptor.attr).or_insert((off, end));
    }
    out
}

/// The ordered float candidates of one surface control lane: the plain record
/// first, then the compact records, each with its values.
fn surface_control_candidates(
    bytes: &[u8],
    arrays: &Arrays,
    attr: u32,
) -> Vec<(Vec<f64>, ArrayRecord)> {
    let mut out = Vec::new();
    if let Some(record) = plain_array_records(bytes, attr, true).into_iter().next() {
        if let Some(values) = read_record_f64(bytes, &record) {
            out.push((values, record));
        }
    }
    for record in compact_array_records(bytes, arrays, attr, true) {
        if let Some(values) = read_record_f64(bytes, &record) {
            if !out.iter().any(|(candidate, _)| *candidate == values) {
                out.push((values, record));
            }
        }
    }
    out
}

/// The ordered float candidates of one surface knot lane, each with its values.
fn surface_knot_candidates(
    bytes: &[u8],
    arrays: &Arrays,
    attr: u32,
) -> Vec<(Vec<f64>, ArrayRecord)> {
    surface_control_candidates(bytes, arrays, attr)
}

/// The ordered multiplicity candidates of one surface knot lane, grouped by
/// stored length, each with its values. Mirrors the grouping in
/// [`surface_knot_values`]: the plain record first, then the compact records.
fn surface_multiplicity_candidates(
    bytes: &[u8],
    arrays: &Arrays,
    attr: u32,
) -> HashMap<usize, Vec<(Vec<u16>, ArrayRecord)>> {
    let mut out: HashMap<usize, Vec<(Vec<u16>, ArrayRecord)>> = HashMap::new();
    if let Some(record) = plain_array_records(bytes, attr, false).into_iter().next() {
        if let Some(values) = read_record_u16(bytes, &record) {
            out.entry(values.len()).or_default().push((values, record));
        }
    }
    for record in compact_array_records(bytes, arrays, attr, false) {
        if let Some(values) = read_record_u16(bytes, &record) {
            out.entry(values.len()).or_default().push((values, record));
        }
    }
    out
}

fn surface_spans(body: &[u8], spans: &mut Vec<super::read_ledger::RawSpan>) {
    let mut refusals = Vec::new();
    let carriers = scan_surface_carriers(body, &mut refusals);
    if carriers.is_empty() {
        return;
    }
    let descriptors = scan_surface_descriptors(body);
    let desc_offsets = surface_descriptor_offsets(body);
    let compact_attrs = descriptors
        .values()
        .flat_map(|descriptor| descriptor.refs)
        .collect::<HashSet<_>>();
    let arrays = scan_arrays(body, Some(&compact_attrs), Some(&compact_attrs));
    for (wrapper_attr, carrier) in &carriers {
        let off = carrier.offset;
        let Some(attr_at) = wrapper_attr_pos(body, off) else {
            continue;
        };
        let Some(descriptor_at) = attr_at.checked_add(17) else {
            continue;
        };
        let Some(desc_attr) = wrapper_fields(body, attr_at)
            .map(|(_, descriptor, _)| descriptor)
            .or_else(|| super::word_at(body, descriptor_at))
        else {
            continue;
        };
        let Some(link_end) = descriptor_at.checked_add(2) else {
            continue;
        };
        push_raw_span(spans, body, off, link_end, 0x7c, *wrapper_attr);
        let Some(descriptor) = descriptors.get(&desc_attr) else {
            continue;
        };
        let Some(&(desc_off, desc_end)) = desc_offsets.get(&desc_attr) else {
            continue;
        };
        push_raw_span(spans, body, desc_off, desc_end, 0x7e, desc_attr);
        let Some(expected_poles) = descriptor.u_count.checked_mul(descriptor.v_count) else {
            continue;
        };
        let Some(expected_control) = expected_poles.checked_mul(descriptor.dimension) else {
            continue;
        };
        let control_candidates: Vec<(Vec<f64>, ArrayRecord)> =
            surface_control_candidates(body, &arrays, descriptor.refs[0])
                .into_iter()
                .filter(|(values, _)| values.len() == expected_control)
                .collect();
        if let Some((first_values, _)) = control_candidates.first() {
            if control_candidates
                .iter()
                .all(|(values, _)| values == first_values)
            {
                let record = control_candidates
                    .iter()
                    .find(|(values, _)| values == first_values);
                if let Some((_, record)) = record {
                    push_values_span(spans, body, record, expected_control, 8, descriptor.refs[0]);
                }
            }
        }
        for (knot_attr, mult_attr, declared) in [
            (
                descriptor.refs[3],
                descriptor.refs[1],
                descriptor.u_knot_count,
            ),
            (
                descriptor.refs[4],
                descriptor.refs[2],
                descriptor.v_knot_count,
            ),
        ] {
            if let Some((knot_record, mult_record)) =
                surface_knot_records(body, &arrays, knot_attr, mult_attr, declared)
            {
                push_values_span(spans, body, &knot_record, declared, 8, knot_attr);
                push_values_span(spans, body, &mult_record, declared, 2, mult_attr);
            }
        }
    }
}

/// One resolved surface knot lane: the semantic unique and multiplicity
/// vectors with the array records that hold them.
type ResolvedKnotLane = ((Vec<f64>, Vec<u16>), (ArrayRecord, ArrayRecord));

/// The first resolving knot/multiplicity record pair of one surface knot lane,
/// in scan order. The semantic vectors must resolve uniquely, matching
/// [`surface_knot_values`]; the pair returned is the first pair that resolves
/// to them.
fn surface_knot_records(
    bytes: &[u8],
    arrays: &Arrays,
    knot_attr: u32,
    mult_attr: u32,
    declared: usize,
) -> Option<(ArrayRecord, ArrayRecord)> {
    let unique = surface_knot_candidates(bytes, arrays, knot_attr);
    let by_count = surface_multiplicity_candidates(bytes, arrays, mult_attr);
    let mut resolved: Vec<ResolvedKnotLane> = Vec::new();
    for (unique_values, knot_record) in &unique {
        let Some(candidates) = by_count.get(&unique_values.len()) else {
            continue;
        };
        for (mult_values, mult_record) in candidates {
            let Some((knots, mults)) = surface_knot_arrays(unique_values, mult_values, declared)
            else {
                continue;
            };
            let entry = (knots.to_vec(), mults.to_vec());
            if !resolved.iter().any(|(candidate, _)| *candidate == entry) {
                resolved.push((entry, (*knot_record, *mult_record)));
            }
        }
    }
    if resolved.len() != 1 {
        return None;
    }
    resolved.pop().map(|(_, records)| records)
}

#[cfg(test)]
mod tests {
    use super::{
        curve_descriptor, curve_descriptor_link, expanded_knots, scan_arrays, scan_curve_carriers,
        scan_curve_descriptors, scan_surface_carriers, surface_knot_values, unique_knots,
        unique_surface_knot_span, Arrays, CurveDescriptor,
    };
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::nurbs::NurbsPoles3;
    use cadmpeg_ir::geometry::SolvedCurveGeometry;

    /// A `00 86` record with attribute 10, node identifier 90 and `fields`
    /// after it; then descriptors 40, 90 and 256 (each with its own carrier).
    fn b_curve_with_descriptors(fields: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x00, 0x86];
        bytes.extend_from_slice(&10_u16.to_be_bytes());
        bytes.extend_from_slice(&90_u32.to_be_bytes());
        bytes.extend_from_slice(fields);
        for (wrapper, descriptor) in [(100, 40), (120, 90), (140, 256)] {
            bytes.extend(crate::test_support::parasolid::nurbs_curve_carrier(
                wrapper, descriptor,
            ));
        }
        bytes
    }

    fn be16s(values: &[u16]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect()
    }

    fn link(fields: &[u8]) -> Option<(u32, usize)> {
        let bytes = b_curve_with_descriptors(fields);
        let descriptors = scan_curve_descriptors(&bytes, CurveDescriptor::spatial);
        curve_descriptor_link(&bytes, 2, &descriptors)
    }

    /// `copies` compact knot arrays `[0, 1]` under attribute 0x1234 and as
    /// many compact multiplicity arrays `[2, 2]` under attribute 0x1235.
    fn repeated_compact_knots(copies: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for _ in 0..copies {
            bytes.extend_from_slice(&[0, 2, 0x12, 0x34]);
            bytes.extend_from_slice(&0.0_f64.to_be_bytes());
            bytes.extend_from_slice(&1.0_f64.to_be_bytes());
            bytes.extend_from_slice(&[0, 2, 0x12, 0x35]);
            bytes.extend(be16s(&[2, 2]));
        }
        bytes
    }

    #[test]
    fn repeated_compact_knot_arrays_resolve_once() {
        let bytes = repeated_compact_knots(20_000);
        let attrs = [0x1234, 0x1235].into_iter().collect();
        let arrays = scan_arrays(&bytes, None, Some(&attrs));
        assert_eq!(arrays.compact.get(&0x1234).map(Vec::len), Some(20_000));
        let resolved =
            surface_knot_values(&bytes, &arrays, 0x1234, 0x1235, 2).expect("one knot pair");
        assert_eq!(resolved.unique, [0.0, 1.0]);
        assert_eq!(resolved.multiplicities, [2, 2]);
    }

    #[test]
    fn two_distinct_multiplicities_of_one_length_resolve_no_knots() {
        let mut bytes = vec![0, 2, 0x12, 0x35];
        bytes.extend(be16s(&[1, 3]));
        bytes.extend(repeated_compact_knots(3));
        let attrs = [0x1234, 0x1235].into_iter().collect();
        let arrays = scan_arrays(&bytes, None, Some(&attrs));
        assert!(surface_knot_values(&bytes, &arrays, 0x1234, 0x1235, 2).is_none());
    }

    #[test]
    fn wide_compact_knot_candidates_preserve_repeats_and_ambiguity() {
        fn pair(knot: u32, multiplicity: u32, values: &[u16]) -> Vec<u8> {
            let mut bytes = 2_u16.to_be_bytes().to_vec();
            pointer(&mut bytes, knot);
            for value in [0.0_f64, 1.0] {
                bytes.extend(value.to_be_bytes());
            }
            bytes.extend(2_u16.to_be_bytes());
            pointer(&mut bytes, multiplicity);
            bytes.extend(be16s(values));
            bytes
        }
        for knot in [0x7fff, 0x8000, 0x1_0000] {
            let multiplicity = knot + 1;
            let mut bytes = Vec::new();
            for _ in 0..3 {
                bytes.extend(pair(knot, multiplicity, &[2, 2]));
            }
            let attrs = [knot, multiplicity].into_iter().collect();
            let arrays = scan_arrays(&bytes, None, Some(&attrs));
            assert_eq!(arrays.compact.get(&knot).map(Vec::len), Some(3));
            let resolved = surface_knot_values(&bytes, &arrays, knot, multiplicity, 2)
                .expect("one wide knot pair");
            assert_eq!(resolved.unique, [0.0, 1.0]);
            assert_eq!(resolved.multiplicities, [2, 2]);
            bytes.extend(pair(knot, multiplicity, &[1, 3]));
            let arrays = scan_arrays(&bytes, None, Some(&attrs));
            assert!(surface_knot_values(&bytes, &arrays, knot, multiplicity, 2).is_none());
        }
    }

    #[test]
    fn a_b_curve_takes_its_descriptor_from_its_nurbs_pointer() {
        // The node identifier's low half and the owner both name descriptor 90.
        let mut fields = be16s(&[1, 90, 12, 13, 1]);
        fields.push(b'+');
        fields.extend(be16s(&[40, 1]));
        assert_eq!(link(&fields), Some((40, 2 + 19)));
        let bytes = b_curve_with_descriptors(&fields);
        let descriptors = scan_curve_descriptors(&bytes, CurveDescriptor::spatial);
        assert_eq!(
            curve_descriptor(&bytes, 2, &descriptors).map(|descriptor| descriptor.control_attr),
            Some(41)
        );
    }

    #[test]
    fn a_wide_pointer_shifts_the_sense_byte_and_the_nurbs_pointer() {
        // The owner names descriptor 90, which the search would take.
        let mut fields = be16s(&[1, 90, 0xfffe, 2, 12, 1]);
        fields.push(b'-');
        fields.extend(be16s(&[40, 1]));
        assert_eq!(link(&fields), Some((40, 2 + 21)));
    }

    #[test]
    fn a_tripled_b_curve_takes_its_nurbs_cell_after_its_sense_byte() {
        // The fourth cell's low byte reads as a sense byte at +16, and the
        // two bytes at +17 name descriptor 256.
        let mut fields = Vec::new();
        for pointer in [1_u16, 12, 13, 0x2b, 1] {
            fields.extend(pointer.to_be_bytes());
            fields.push(1);
        }
        fields.push(b'+');
        fields.extend(be16s(&[40, 1]));
        assert_eq!(link(&fields), Some((40, 2 + 24)));
    }

    #[test]
    fn a_framed_b_curve_whose_nurbs_pointer_names_no_descriptor_links_none() {
        // The pointers `00 01 00 12` hold `01 00`, descriptor 256, across two
        // fields; the NURBS pointer 77 names no descriptor.
        let mut fields = be16s(&[1, 18, 13, 14, 1]);
        fields.push(b'+');
        fields.extend(be16s(&[77, 1]));
        assert_eq!(link(&fields), None);
    }

    #[test]
    fn a_b_curve_without_a_sense_byte_never_reads_its_node_identifier() {
        let mut fields = be16s(&[2, 40, 12, 13, 1]);
        fields.extend(be16s(&[0, 1]));
        assert_eq!(link(&fields).map(|(attr, _)| attr), Some(40));
    }

    #[test]
    fn spline_arrays_materialize_each_identity_once() {
        let mut bytes = Vec::new();
        for index in 0..128 {
            let values = if index == 0 { [1.0; 8] } else { [2.0; 8] };
            bytes.extend(crate::test_support::parasolid::f64_array(0x2d, 12, &values));
            bytes.extend(crate::test_support::parasolid::u16_array(13, &[3; 8]));
        }
        let arrays = scan_arrays(&bytes, None, None);
        assert_eq!(arrays.f64s[&12], [1.0; 8]);
        assert_eq!(arrays.u16s[&13], [3; 8]);
    }

    #[test]
    fn spline_arrays_materialize_only_referenced_attributes() {
        let mut bytes = crate::test_support::parasolid::f64_array(0x2d, 12, &[1.0, 2.0, 3.0]);
        let unrelated_values = vec![4.0; 4096];
        bytes.extend(crate::test_support::parasolid::f64_array(
            0x2d,
            13,
            &unrelated_values,
        ));
        let attributes = std::collections::HashSet::from([12]);
        let arrays = scan_arrays(&bytes, Some(&attributes), None);
        assert_eq!(arrays.f64s[&12], [1.0, 2.0, 3.0]);
        assert!(!arrays.f64s.contains_key(&13));
    }

    #[test]
    fn parasolid_scalar_array_values_decode() {
        let bytes = crate::test_support::parasolid::f64_array(0x2d, 12, &[0.0, 1.0, 2.0]);
        assert_eq!(
            scan_arrays(&bytes, None, None).f64s.get(&12).map(Vec::len),
            Some(3)
        );
    }

    #[test]
    fn parasolid_knot_expansion_repeats_each_value_by_its_multiplicity() {
        assert_eq!(
            expanded_knots(&[0.0, 1.0], &[2, 2], 4),
            Some(vec![0.0, 0.0, 1.0, 1.0])
        );
    }

    #[test]
    fn parasolid_curve_carrier_decodes() {
        let bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        assert!(scan_curve_carriers(&bytes, &mut Vec::new()).contains_key(&170));
    }

    #[test]
    fn parasolid_surface_carrier_decodes() {
        let bytes = crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10);
        assert!(scan_surface_carriers(&bytes, &mut Vec::new()).contains_key(&180));
    }

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
        let point = FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0e300, 0.0, 0.0)).unwrap();
        assert_eq!(
            super::homogeneous_poles(
                &NurbsPoles3::Rational {
                    points: vec![cadmpeg_ir::geometry::nurbs::WeightedPole3 {
                        point,
                        weight: cadmpeg_ir::scalar::NonZeroReal::new(1.0e300).unwrap()
                    }]
                },
                0.001
            ),
            None
        );
        assert_eq!(
            super::homogeneous_poles(
                &NurbsPoles3::Rational {
                    points: vec![cadmpeg_ir::geometry::nurbs::WeightedPole3 {
                        point,
                        weight: cadmpeg_ir::scalar::NonZeroReal::new(2.0).unwrap()
                    }]
                },
                0.001
            ),
            Some(vec![1.0e300 * 0.001 * 2.0, 0.0, 0.0, 2.0])
        );
    }

    #[test]
    fn missing_surface_knot_arrays_decline_the_patch() {
        assert_eq!(
            unique_surface_knot_span(&[], &Arrays::default(), (2, 3), 1, &[0.0], &[1]),
            None
        );
    }

    fn pointer(out: &mut Vec<u8>, value: u32) {
        if value <= 0x7fff {
            out.extend(u16::try_from(value).unwrap().to_be_bytes());
        } else {
            let low = u16::try_from(value & 0x7fff).unwrap();
            out.extend((0x8000 | low).to_be_bytes());
            out.extend(u16::try_from(value >> 15).unwrap().to_be_bytes());
        }
    }

    #[test]
    fn wide_b_curve_resolves_its_nurbs_pointer_without_selecting_other_fields() {
        let mut descriptors =
            scan_curve_descriptors(&b_curve_with_descriptors(&[]), CurveDescriptor::spatial);
        let descriptor = descriptors.remove(&90).unwrap();
        descriptors.insert(0x1_0000, descriptor);
        for own in [0x7fff, 0x8000, 0xffff, 0x1_0000] {
            for nurbs in [0x1_0000, 0x1_0001] {
                let mut bytes = Vec::new();
                pointer(&mut bytes, own);
                bytes.extend(0x0028_0028_u32.to_be_bytes());
                for reference in [1, 40, 1, 1, 1] {
                    pointer(&mut bytes, reference);
                }
                bytes.push(b'+');
                pointer(&mut bytes, nurbs);
                let expected = (nurbs == 0x1_0000).then_some((nurbs, bytes.len()));
                assert_eq!(curve_descriptor_link(&bytes, 0, &descriptors), expected);
            }
        }
    }

    #[test]
    fn tripled_b_curve_with_wide_identity_reads_its_nurbs_cell() {
        let descriptors =
            scan_curve_descriptors(&b_curve_with_descriptors(&[]), CurveDescriptor::spatial);
        for own in [0x7fff, 0x8000, 0x1_0000] {
            let mut bytes = Vec::new();
            pointer(&mut bytes, own);
            bytes.extend(0x005a_005a_u32.to_be_bytes());
            for reference in [1_u16, 12, 13, 0x2b, 1] {
                bytes.extend(reference.to_be_bytes());
                bytes.push(1);
            }
            bytes.push(b'+');
            bytes.extend(40_u16.to_be_bytes());
            assert_eq!(
                curve_descriptor_link(&bytes, 0, &descriptors),
                Some((40, bytes.len()))
            );
        }
    }

    #[test]
    fn wide_spline_references_select_the_control_and_knot_arrays() {
        let mut bytes = vec![0, 0x86, 0xff];
        pointer(&mut bytes, 0xffff);
        bytes.extend(1_u32.to_be_bytes());
        for _ in 0..5 {
            pointer(&mut bytes, 1);
        }
        bytes.push(b'+');
        pointer(&mut bytes, 0x1_0000);
        pointer(&mut bytes, 1);
        bytes.extend([0, 0x88, 0xff]);
        pointer(&mut bytes, 0x1_0000);
        bytes.extend(1_u16.to_be_bytes());
        bytes.extend(2_u32.to_be_bytes());
        bytes.extend(3_u16.to_be_bytes());
        bytes.extend(2_u32.to_be_bytes());
        bytes.extend([0, 0, 0, 0, 1]);
        for reference in [0x1_0001, 0x1_0002, 0x1_0003] {
            pointer(&mut bytes, reference);
        }
        for (tag, attr, values) in [
            (0x2d, 0x1_0001, &[0.0_f64, 0.0, 0.0, 0.001, 0.0, 0.0][..]),
            (0x80, 0x1_0003, &[0.0, 1.0][..]),
        ] {
            bytes.extend([0, tag]);
            bytes.extend(u32::try_from(values.len()).unwrap().to_be_bytes());
            pointer(&mut bytes, attr);
            for value in values {
                bytes.extend(value.to_be_bytes());
            }
        }
        bytes.extend([0, 0x7f]);
        bytes.extend(2_u32.to_be_bytes());
        pointer(&mut bytes, 0x1_0002);
        bytes.extend([0, 2, 0, 2]);
        let mut refusals = Vec::new();
        let curves = super::scan_curve_carriers(&bytes, &mut refusals);
        assert!(refusals.is_empty());
        let Some(SolvedCurveGeometry::Nurbs(curve)) = curves[&0xffff].geometry.solved() else {
            panic!("NURBS carrier");
        };
        assert_eq!(curve.degree(), 1);
        let NurbsPoles3::Polynomial { points } = curve.pole_rows() else {
            panic!("polynomial poles");
        };
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].get().x, 0.0);
        assert_eq!(points[1].get().x, 1.0);
    }
}
