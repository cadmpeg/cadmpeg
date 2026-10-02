// SPDX-License-Identifier: Apache-2.0
//! B-spline/list carrier tables.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{knots_nondecreasing, NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface},
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;

use cadmpeg_core::decode::{index_from_u32, DecodeContext, View};

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
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_collection_items(
        u64::try_from(count)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )
}

fn read_array<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
    mut read: impl FnMut(usize) -> Option<T>,
) -> Result<Option<Vec<T>>, cadmpeg_core::CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(count), operation)?;
    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, count, operation)?;
    for index in 0..count {
        let Some(value) = read(index) else {
            return Ok(None);
        };
        values.push(value);
    }
    Ok(Some(values))
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    compact_attrs: Option<&HashSet<u16>>,
) -> Result<Arrays, cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan Parasolid spline arrays",
    )?;
    let mut arrays = Arrays::default();
    for off in 0..bytes.len().checked_sub(9).map_or(0, |end| end) {
        if bytes.get(off) == Some(&0) {
            let count = usize::from(bytes[off + compact_arr::COUNT]);
            if count > 0 {
                if let Some(attr) = View::u16_be_at(bytes, off + compact_arr::ATTR)
                    .filter(|attr| compact_attrs.is_some_and(|attrs| attrs.contains(attr)))
                {
                    ctx.push_hash_group(
                        &mut arrays.compact,
                        attr,
                        CompactArray { offset: off, count },
                        "index Parasolid compact arrays",
                        "scan Parasolid compact arrays",
                    )?;
                }
            }
        }
        let tag = match bytes.get(off..off + 2) {
            Some([0x00, tag @ (0x2d | 0x7f | 0x80)]) => *tag,
            _ => continue,
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(arr_hdr::LEN),
            "probe Parasolid spline array",
        )?;
        let Some(p) = array_body(bytes, off, tag) else {
            continue;
        };
        let Some(count) = View::u32_be_at(bytes, p + arr_hdr::COUNT).map(index_from_u32) else {
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
            let Some(values) =
                read_array(ctx, count, "decode Parasolid integer array values", |i| {
                    View::u16_be_at(bytes, values_at + i * 2)
                })?
            else {
                continue;
            };
            ctx.admit_hash_map_entry(&mut arrays.u16s, &attr, "collect Parasolid integer arrays")?;
            arrays.u16s.entry(attr).or_insert(values);
        } else {
            if count
                .checked_mul(8)
                .and_then(|size| values_at.checked_add(size))
                .is_none_or(|end| end > bytes.len())
            {
                continue;
            }
            let Some(values) =
                read_array(ctx, count, "decode Parasolid scalar array values", |i| {
                    View::f64_be_at(bytes, values_at + i * 8)
                })?
            else {
                continue;
            };
            // The native surface format can reserve physical knot slots beyond
            // the descriptor's distinct-knot count. Their bits are not
            // semantic data when the matching multiplicities are zero, so
            // defer finite-value checks until descriptor binding.
            ctx.admit_hash_map_entry(&mut arrays.f64s, &attr, "collect Parasolid scalar arrays")?;
            arrays.f64s.entry(attr).or_insert(values);
        }
    }
    Ok(arrays)
}

fn compact_f64_arrays(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    arrays: &Arrays,
    attr: u16,
) -> Result<Vec<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut candidates = Vec::new();
    if let Some(values) = arrays.f64s.get(&attr) {
        let mut copy = Vec::new();
        ctx.reserve_vec(
            &mut copy,
            values.len(),
            "copy Parasolid scalar array values",
        )?;
        copy.extend_from_slice(values);
        ctx.reserve_vec(&mut candidates, 1, "collect Parasolid scalar candidates")?;
        candidates.push(copy);
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
        let Some(values) = read_array(
            ctx,
            compact.count,
            "decode Parasolid compact scalar values",
            |index| View::f64_be_at(bytes, compact.offset + compact_arr::LEN + index * 8),
        )?
        else {
            continue;
        };
        if !candidates.contains(&values) {
            ctx.reserve_vec(&mut candidates, 1, "collect Parasolid scalar candidates")?;
            candidates.push(values);
        }
    }
    Ok(candidates)
}

fn compact_u16_arrays(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    arrays: &Arrays,
    attr: u16,
) -> Result<Vec<Vec<u16>>, cadmpeg_core::CodecError> {
    let mut candidates = Vec::new();
    if let Some(values) = arrays.u16s.get(&attr) {
        let mut copy = Vec::new();
        ctx.reserve_vec(
            &mut copy,
            values.len(),
            "copy Parasolid integer array values",
        )?;
        copy.extend_from_slice(values);
        ctx.reserve_vec(&mut candidates, 1, "collect Parasolid integer candidates")?;
        candidates.push(copy);
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
        let Some(values) = read_array(
            ctx,
            compact.count,
            "decode Parasolid compact integer values",
            |index| View::u16_be_at(bytes, compact.offset + compact_arr::LEN + index * 2),
        )?
        else {
            continue;
        };
        if !candidates.contains(&values) {
            ctx.reserve_vec(&mut candidates, 1, "collect Parasolid integer candidates")?;
            candidates.push(values);
        }
    }
    Ok(candidates)
}

fn exact_f64_array(
    ctx: &DecodeContext<'_>,
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<HashMap<u16, CurveDescriptor>, cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan Parasolid curve descriptors",
    )?;
    let mut out = HashMap::new();
    for off in 0..bytes.len().checked_sub(29).map_or(0, |end| end) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x88]) {
            continue;
        }
        ctx.charge_work(29, "probe Parasolid curve descriptor")?;
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
        let Some(control_count) = View::u32_be_at(bytes, p + 4).map(index_from_u32) else {
            continue;
        };
        let Some(dimension) = View::u16_be_at(bytes, p + 8).map(usize::from) else {
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
        ctx.admit_hash_map_entry(&mut out, &attr, "collect Parasolid curve descriptors")?;
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
    (attr_at + 2..(attr_at + 24).min(bytes.len().checked_sub(1).map_or(0, |end| end)))
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
    ctx: &DecodeContext<'_>,
    values: &[f64],
    multiplicities: &[u16],
    expected: usize,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let scan_count = if values.len() < multiplicities.len() {
        values.len()
    } else {
        multiplicities.len()
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan_count),
        "scan Parasolid knot multiplicities",
    )?;
    let mut out = Vec::new();
    for (value, &multiplicity) in values.iter().zip(multiplicities) {
        if multiplicity == 0 {
            continue;
        }
        let next_len = out
            .len()
            .checked_add(usize::from(multiplicity))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("expand Parasolid knots", u64::MAX - 1, u64::MAX)
            })?;
        if next_len > expected {
            return Ok(None);
        }
        ctx.charge_work(u64::from(multiplicity), "emit Parasolid expanded knots")?;
        ctx.reserve_vec(
            &mut out,
            usize::from(multiplicity),
            "expand Parasolid knots",
        )?;
        out.extend(std::iter::repeat_n(*value, usize::from(multiplicity)));
    }
    Ok(Some(out))
}

fn unique_knots(
    ctx: &DecodeContext<'_>,
    knots: &[f64],
) -> Result<(Vec<f64>, Vec<usize>), cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(knots.len()),
        "compress Parasolid patch knots",
    )?;
    let mut values = Vec::new();
    let mut multiplicities = Vec::<usize>::new();
    for &knot in knots {
        if values.last() == Some(&knot) {
            if let Some(multiplicity) = multiplicities.last_mut() {
                *multiplicity = multiplicity.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("compress Parasolid patch knots", u64::MAX, u64::MAX)
                })?;
            }
        } else {
            ctx.push_vec(&mut values, knot, "collect Parasolid patch knot values")?;
            ctx.push_vec(
                &mut multiplicities,
                1,
                "collect Parasolid patch knot multiplicities",
            )?;
        }
    }
    Ok((values, multiplicities))
}

fn multiplicity_sum(values: &[u16]) -> Option<usize> {
    values
        .iter()
        .try_fold(0usize, |sum, &value| sum.checked_add(usize::from(value)))
}

fn array_span(bytes: &[u8], tag: u8, attr: u16) -> Option<(usize, usize)> {
    for off in 0..bytes.len().checked_sub(9).map_or(0, |end| end) {
        let Some(p) = array_body(bytes, off, tag) else {
            continue;
        };
        let Some(count) = View::u32_be_at(bytes, p + arr_hdr::COUNT).map(index_from_u32) else {
            continue;
        };
        if View::u16_be_at(bytes, p + arr_hdr::ATTR) == Some(attr) {
            return Some((p + arr_hdr::LEN, count));
        }
    }
    None
}

fn array_spans(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    arrays: &Arrays,
    tag: u8,
    attr: u16,
) -> Result<Vec<ArraySpan>, cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan Parasolid patch array spans",
    )?;
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
        if count <= MAX_ARRAY_VALUES && View::u16_be_at(bytes, p + arr_hdr::ATTR) == Some(attr) {
            ctx.push_vec(
                &mut spans,
                ArraySpan {
                    start: p + arr_hdr::LEN,
                    count,
                },
                "collect Parasolid patch array spans",
            )?;
        }
    }
    for array in arrays.compact.get(&attr).into_iter().flatten() {
        ctx.push_vec(
            &mut spans,
            ArraySpan {
                start: array.offset + compact_arr::LEN,
                count: array.count,
            },
            "collect Parasolid patch array spans",
        )?;
    }
    ctx.stable_sort_by(
        &mut spans,
        |left, right| (left.start, left.count).cmp(&(right.start, right.count)),
        |_| 0,
        "sldprt parasolid array spans sort",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(spans.len()),
        "deduplicate Parasolid patch array spans",
    )?;
    spans.dedup();
    Ok(spans)
}

fn f64_values(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    span: ArraySpan,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    read_array(
        ctx,
        span.count,
        "read Parasolid patch scalar values",
        |index| {
            span.start
                .checked_add(index.checked_mul(8)?)
                .and_then(|offset| View::f64_be_at(bytes, offset))
        },
    )
}

fn u16_values(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    span: ArraySpan,
) -> Result<Option<Vec<u16>>, cadmpeg_core::CodecError> {
    read_array(
        ctx,
        span.count,
        "read Parasolid patch multiplicities",
        |index| {
            span.start
                .checked_add(index.checked_mul(2)?)
                .and_then(|offset| View::u16_be_at(bytes, offset))
        },
    )
}

fn unique_control_span(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    arrays: &Arrays,
    attr: u16,
    old_values: &[f64],
) -> Result<Option<ArraySpan>, cadmpeg_core::CodecError> {
    let mut spans = array_spans(ctx, bytes, arrays, 0x2d, attr)?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(spans.len()),
        "filter Parasolid patch control spans",
    )?;
    spans.retain(|span| span.count == old_values.len());
    if let [span] = spans.as_slice() {
        return Ok(Some(*span));
    }
    let mut selected = None;
    for span in spans {
        let Some(values) = f64_values(ctx, bytes, span)? else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(values.len()),
            "match Parasolid patch control span",
        )?;
        if values.iter().zip(old_values).all(|(native, expected)| {
            let scale = native.abs().max(expected.abs()).max(1.0);
            (native - expected).abs() <= 16.0 * f64::EPSILON * scale
        }) && selected.replace(span).is_some()
        {
            return Ok(None);
        }
    }
    Ok(selected)
}

fn unique_surface_knot_span(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    arrays: &Arrays,
    (knot_attr, multiplicity_attr): (u16, u16),
    declared_count: usize,
    old_values: &[f64],
    old_multiplicities: &[usize],
) -> Result<Option<ArraySpan>, cadmpeg_core::CodecError> {
    if old_values.len() != declared_count || old_multiplicities.len() != declared_count {
        return Ok(None);
    }
    let knot_spans = array_spans(ctx, bytes, arrays, 0x80, knot_attr)?;
    let multiplicity_spans = array_spans(ctx, bytes, arrays, 0x7f, multiplicity_attr)?;
    let mut pairs = Vec::new();
    for knot_span in knot_spans {
        let Some(knots) = f64_values(ctx, bytes, knot_span)? else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(multiplicity_spans.len()),
            "scan Parasolid patch multiplicity spans",
        )?;
        for multiplicity_span in multiplicity_spans
            .iter()
            .copied()
            .filter(|span| span.count == knot_span.count)
        {
            let Some(multiplicities) = u16_values(ctx, bytes, multiplicity_span)? else {
                continue;
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(knots.len())
                    .checked_mul(3)
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "match Parasolid patch knot spans",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?,
                "match Parasolid patch knot spans",
            )?;
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
                ctx.push_vec(
                    &mut pairs,
                    (knot_span, multiplicity_span),
                    "collect Parasolid patch knot span pairs",
                )?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut pairs,
        |(left_knots, left_multiplicities), (right_knots, right_multiplicities)| {
            (
                left_knots.start,
                left_knots.count,
                left_multiplicities.start,
            )
                .cmp(&(
                    right_knots.start,
                    right_knots.count,
                    right_multiplicities.start,
                ))
        },
        |_| 0,
        "sldprt parasolid surface knot span pairs sort",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(pairs.len()),
        "deduplicate Parasolid patch knot pairs",
    )?;
    pairs.dedup();
    Ok(match pairs.as_slice() {
        [(knots, _)] => Some(*knots),
        _ => None,
    })
}

fn patch_f64_span(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    span: ArraySpan,
    values: &[f64],
) -> Result<Option<()>, cadmpeg_core::CodecError> {
    if values.len() > span.count {
        return Ok(None);
    }
    let Some(size) = values.len().checked_mul(8) else {
        return Ok(None);
    };
    let Some(end) = span.start.checked_add(size) else {
        return Ok(None);
    };
    let Some(slots) = bytes.get_mut(span.start..end) else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(size),
        "write Parasolid patch scalar span",
    )?;
    for (slot, value) in slots.chunks_exact_mut(8).zip(values) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    Ok(Some(()))
}

fn patch_f64_array(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    tag: u8,
    attr: u16,
    values: &[f64],
) -> Result<Option<()>, cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "find Parasolid patch scalar array",
    )?;
    let Some((start, count)) = array_span(bytes, tag, attr) else {
        return Ok(None);
    };
    if count != values.len() {
        return Ok(None);
    }
    patch_f64_span(ctx, bytes, ArraySpan { start, count }, values)
}

fn append_homogeneous_pole(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<f64>,
    point: FinitePoint3,
    weight: Option<f64>,
    scale: f64,
) -> Result<Option<()>, cadmpeg_core::CodecError> {
    let point = point.get();
    let factor = weight.unwrap_or(1.0);
    if factor.abs() <= f64::EPSILON {
        return Ok(None);
    }
    ctx.charge_work(4, "form Parasolid homogeneous patch pole")?;
    let homogeneous = [point.x, point.y, point.z].map(|value| value * scale * factor);
    if !homogeneous.iter().all(|value| value.is_finite()) {
        return Ok(None);
    }
    ctx.reserve_vec(
        out,
        3 + usize::from(weight.is_some()),
        "collect Parasolid homogeneous patch poles",
    )?;
    out.extend(homogeneous);
    if let Some(weight) = weight {
        out.push(weight);
    }
    Ok(Some(()))
}

fn homogeneous_poles(
    ctx: &DecodeContext<'_>,
    poles: &NurbsPoles3<FinitePoint3>,
    scale: f64,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut out = Vec::new();
    match poles {
        NurbsPoles3::Polynomial { points } => {
            for point in points {
                if append_homogeneous_pole(ctx, &mut out, *point, None, scale)?.is_none() {
                    return Ok(None);
                }
            }
        }
        NurbsPoles3::Rational { points } => {
            for pole in points {
                if append_homogeneous_pole(
                    ctx,
                    &mut out,
                    pole.point,
                    Some(pole.weight.get()),
                    scale,
                )?
                .is_none()
                {
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(out))
}

fn homogeneous_grid_poles(
    ctx: &DecodeContext<'_>,
    poles: &NurbsPoleGrid<FinitePoint3>,
    scale: f64,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut out = Vec::new();
    match poles {
        NurbsPoleGrid::Polynomial { rows } => {
            for point in rows.iter().flatten() {
                if append_homogeneous_pole(ctx, &mut out, *point, None, scale)?.is_none() {
                    return Ok(None);
                }
            }
        }
        NurbsPoleGrid::Rational { rows } => {
            for pole in rows.iter().flatten() {
                if append_homogeneous_pole(
                    ctx,
                    &mut out,
                    pole.point,
                    Some(pole.weight.get()),
                    scale,
                )?
                .is_none()
                {
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(out))
}

/// Patch retained curve pole and knot arrays without changing their storage shape.
pub(crate) fn patch_nurbs_curve(
    ctx: &DecodeContext<'_>,
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
    let (old_unique, old_mult) = unique_knots(ctx, old.knots())?;
    let (new_unique, new_mult) = unique_knots(ctx, new.knots())?;
    if old_mult != new_mult || old_unique.len() != new_unique.len() {
        return Ok(None);
    }
    let mut p = wrapper_offset.checked_add(2).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("Parasolid patch wrapper offset overflow")
    })?;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let descriptors = scan_curve_descriptors(ctx, bytes)?;
    let Some(descriptor) = curve_descriptor(bytes, p, &descriptors) else {
        return Ok(None);
    };
    if descriptor.degree != old.degree()
        || descriptor.control_count != old.control_points().len()
        || descriptor.dimension != if old.weights().is_some() { 4 } else { 3 }
    {
        return Ok(None);
    }
    let Some(poles) = homogeneous_poles(ctx, new.pole_rows(), scale)? else {
        return Ok(None);
    };
    if patch_f64_array(ctx, bytes, 0x2d, descriptor.control_attr, &poles)?.is_none() {
        return Ok(None);
    }
    patch_f64_array(ctx, bytes, 0x80, descriptor.knot_attr, &new_unique)
}

/// Patch retained surface pole and knot arrays without changing their storage shape.
pub(crate) fn patch_nurbs_surface(
    ctx: &DecodeContext<'_>,
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
    let (old_u, old_u_mult) = unique_knots(ctx, old.u_knots())?;
    let (new_u, new_u_mult) = unique_knots(ctx, new.u_knots())?;
    let (old_v, old_v_mult) = unique_knots(ctx, old.v_knots())?;
    let (new_v, new_v_mult) = unique_knots(ctx, new.v_knots())?;
    if old_u_mult != new_u_mult
        || old_v_mult != new_v_mult
        || old_u.len() != new_u.len()
        || old_v.len() != new_v.len()
    {
        return Ok(None);
    }
    let descriptors = scan_surface_descriptors(ctx, bytes)?;
    let compact_attrs = ctx.collect_hash_set(
        descriptors.values().flat_map(|descriptor| descriptor.refs),
        "index Parasolid patch compact attributes",
    )?;
    let arrays = scan_arrays(ctx, bytes, Some(&compact_attrs))?;
    let mut p = wrapper_offset.checked_add(2).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("Parasolid patch wrapper offset overflow")
    })?;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let Some(descriptor_at) = p.checked_add(17) else {
        return Ok(None);
    };
    let Some(descriptor_attr) = View::u16_be_at(bytes, descriptor_at) else {
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
    let Some(old_poles) = homogeneous_grid_poles(ctx, old.pole_grid(), scale)? else {
        return Ok(None);
    };
    let Some(poles) = homogeneous_grid_poles(ctx, new.pole_grid(), scale)? else {
        return Ok(None);
    };
    let Some(control_span) = unique_control_span(ctx, bytes, &arrays, control_attr, &old_poles)?
    else {
        return Ok(None);
    };
    let Some(u_knot_span) = unique_surface_knot_span(
        ctx,
        bytes,
        &arrays,
        (u_knot_attr, descriptor.refs[1]),
        descriptor.u_knot_count,
        &old_u,
        &old_u_mult,
    )?
    else {
        return Ok(None);
    };
    let Some(v_knot_span) = unique_surface_knot_span(
        ctx,
        bytes,
        &arrays,
        (v_knot_attr, descriptor.refs[2]),
        descriptor.v_knot_count,
        &old_v,
        &old_v_mult,
    )?
    else {
        return Ok(None);
    };
    if patch_f64_span(ctx, bytes, control_span, &poles)?.is_none() {
        return Ok(None);
    }
    if patch_f64_span(ctx, bytes, u_knot_span, &new_u)?.is_none() {
        return Ok(None);
    }
    patch_f64_span(ctx, bytes, v_knot_span, &new_v)
}

/// Every compact NURBS curve carrier in `bytes`, keyed by attribute id.
///
/// A candidate offset whose bytes are not a spline record is not a refusal: the
/// scan sweeps every offset. A record whose poles, knots and weights do parse
/// but do not pair is one, and it is recorded in `refusals` as a loss naming
/// the attribute id it belongs to, so the decode reports it rather than
/// deleting the carrier in silence.
pub(crate) fn scan_curve_carriers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    refusals: &mut Vec<LossNote>,
) -> Result<HashMap<u16, CurveCarrier>, cadmpeg_core::CodecError> {
    let arrays = scan_arrays(ctx, bytes, None)?;
    let descriptors = scan_curve_descriptors(ctx, bytes)?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan Parasolid curve wrappers",
    )?;
    let mut out = HashMap::new();
    for off in 0..bytes.len().checked_sub(6).map_or(0, |end| end) {
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
        let mut points =
            ctx.collection_vec(descriptor.control_count, "decode Parasolid curve poles")?;
        let mut weights = ctx.optional_collection_vec(
            descriptor.dimension == 4,
            descriptor.control_count,
            "decode Parasolid curve weights",
        )?;
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
        let Some(knots) = expanded_knots(ctx, unique_knots, multiplicities, expected)? else {
            continue;
        };
        if knots.len() != expected {
            continue;
        }
        let nurbs = match cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes_for_decode(
            ctx,
            descriptor.degree,
            knots,
            points,
            weights,
            false,
        )? {
            Ok(nurbs) => nurbs,
            Err(cadmpeg_ir::geometry::nurbs::NurbsError::ResourceLimit(limit)) => {
                return Err(limit.into())
            }
            Err(error) => {
                charge_items(ctx, 1, "collect Parasolid spline refusals")?;
                let note = crate::loss::spline_lane_refusal(
                    ctx,
                    format_args!("curve carrier attribute {attr}: {error}"),
                )?;
                cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                    refusals,
                    1,
                    "collect Parasolid spline refusals",
                )?;
                refusals.push(note);
                continue;
            }
        };
        ctx.reserve_map(&mut out, 1, "collect Parasolid curve carriers")?;
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<HashMap<u16, SurfaceDescriptor>, cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan Parasolid surface descriptors",
    )?;
    let mut out = HashMap::new();
    for off in 0..bytes.len().checked_sub(1).map_or(0, |end| end) {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x7e]) {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(surf_desc::LEN),
            "probe Parasolid surface descriptor",
        )?;
        let Some(descriptor) = parse_surface_descriptor(bytes, off) else {
            continue;
        };
        ctx.admit_hash_map_entry(
            &mut out,
            &descriptor.attr,
            "collect Parasolid surface descriptors",
        )?;
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

#[derive(PartialEq)]
struct SurfaceKnotValues {
    unique: Vec<f64>,
    multiplicities: Vec<u16>,
}

fn surface_knot_values(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    arrays: &Arrays,
    knot_attr: u16,
    multiplicity_attr: u16,
    declared_count: usize,
) -> Result<Option<SurfaceKnotValues>, cadmpeg_core::CodecError> {
    let mut resolved = Vec::<SurfaceKnotValues>::new();
    let mut multiplicities_by_count = HashMap::<usize, Vec<Vec<u16>>>::new();
    for multiplicities in compact_u16_arrays(ctx, bytes, arrays, multiplicity_attr)? {
        ctx.push_hash_group(
            &mut multiplicities_by_count,
            multiplicities.len(),
            multiplicities,
            "index Parasolid knot multiplicity counts",
            "group Parasolid knot multiplicities",
        )?;
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
            let mut unique_copy = Vec::new();
            ctx.reserve_vec(
                &mut unique_copy,
                unique.len(),
                "copy Parasolid distinct knots",
            )?;
            unique_copy.extend_from_slice(unique);
            let mut multiplicity_copy = Vec::new();
            ctx.reserve_vec(
                &mut multiplicity_copy,
                multiplicities.len(),
                "copy Parasolid knot multiplicities",
            )?;
            multiplicity_copy.extend_from_slice(multiplicities);
            let candidate = SurfaceKnotValues {
                unique: unique_copy,
                multiplicities: multiplicity_copy,
            };
            if !resolved.contains(&candidate) {
                ctx.reserve_vec(&mut resolved, 1, "collect Parasolid knot candidates")?;
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    refusals: &mut Vec<LossNote>,
) -> Result<HashMap<u16, SurfaceCarrier>, cadmpeg_core::CodecError> {
    let descriptors = scan_surface_descriptors(ctx, bytes)?;
    let maximum_refs = descriptors.len().checked_mul(5).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "collect Parasolid surface array references",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    let mut compact_attrs = HashSet::new();
    ctx.reserve_set(
        &mut compact_attrs,
        maximum_refs,
        "collect Parasolid surface array references",
    )?;
    for descriptor in descriptors.values() {
        compact_attrs.extend(descriptor.refs);
    }
    let arrays = scan_arrays(ctx, bytes, Some(&compact_attrs))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan Parasolid surface wrappers",
    )?;
    let mut out = HashMap::new();
    for off in 0..bytes.len().checked_sub(1).map_or(0, |end| end) {
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
        let Some(descriptor_at) = p.checked_add(17) else {
            continue;
        };
        let Some(descriptor_attr) = View::u16_be_at(bytes, descriptor_at) else {
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
        let Some(SurfaceKnotValues {
            unique: u_unique,
            multiplicities: u_mult,
        }) = surface_knot_values(
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
        let Some(SurfaceKnotValues {
            unique: v_unique,
            multiplicities: v_mult,
        }) = surface_knot_values(
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
            || !knots_nondecreasing(&u_unique)
            || !knots_nondecreasing(&v_unique)
        {
            continue;
        }
        let mut points = ctx.collection_vec(expected_poles, "decode Parasolid surface poles")?;
        let dimension = if descriptor.rational {
            descriptor.dimension
        } else {
            3
        };
        let mut weights = ctx.optional_collection_vec(
            descriptor.rational,
            expected_poles,
            "decode Parasolid surface weights",
        )?;
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
        }
        let mut pole_rows = Vec::new();
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut pole_rows,
            descriptor.u_count,
            "partition Parasolid surface pole rows",
        )?;
        for row in points.chunks(descriptor.v_count) {
            let mut copy = Vec::new();
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut copy,
                row.len(),
                "partition Parasolid surface poles",
            )?;
            copy.extend_from_slice(row);
            pole_rows.push(copy);
        }
        let weight_rows = if let Some(values) = weights {
            let mut rows = Vec::new();
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut rows,
                descriptor.u_count,
                "partition Parasolid surface weight rows",
            )?;
            for row in values.chunks(descriptor.v_count) {
                let mut copy = Vec::new();
                cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                    &mut copy,
                    row.len(),
                    "partition Parasolid surface weights",
                )?;
                copy.extend_from_slice(row);
                rows.push(copy);
            }
            Some(rows)
        } else {
            None
        };
        let nurbs = match cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes_for_decode(
            ctx,
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
        )? {
            Ok(nurbs) => nurbs,
            Err(cadmpeg_ir::geometry::nurbs::NurbsError::ResourceLimit(limit)) => {
                return Err(limit.into())
            }
            Err(error) => {
                charge_items(ctx, 1, "collect Parasolid spline refusals")?;
                let note = crate::loss::spline_lane_refusal(
                    ctx,
                    format_args!("surface carrier attribute {attr}: {error}"),
                )?;
                cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                    refusals,
                    1,
                    "collect Parasolid spline refusals",
                )?;
                refusals.push(note);
                continue;
            }
        };
        ctx.reserve_map(&mut out, 1, "collect Parasolid surface carriers")?;
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
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::nurbs::NurbsPoles3;

    #[test]
    fn parasolid_scalar_array_values_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::f64_array(0x2d, 12, &[0.0, 1.0, 2.0]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_arrays(&ctx, &bytes, None).expect_err("three values exceed two items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid scalar array values"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            scan_arrays(&ctx, &bytes, None)
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
        let error = scan_arrays(&ctx, &bytes, None).expect_err("three values exceed two items");
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
        let error = expanded_knots(&ctx, &[0.0, 1.0], &[2, 2], 4)
            .expect_err("four knots exceed three items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "expand Parasolid knots"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            expanded_knots(&ctx, &[0.0, 1.0], &[2, 2], 4).expect("service expansion"),
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
        let error = scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            .expect_err("three poles exceed the remaining items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid curve poles"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
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
        let error = scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
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
        let error = scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
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
        let error = scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
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
        let error = scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            .expect_err("weighted pole pairing exceeds the limit");
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "IR NURBS paired poles"),
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
        let error = scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            .expect_err("pole admission exceeds the limit");
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "IR NURBS admitted poles"),
            "{error:?}"
        );
    }

    #[test]
    fn parasolid_surface_poles_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 109;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
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
        assert!(scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
            .expect("service scan")
            .contains_key(&180));
    }

    #[test]
    fn parasolid_surface_weights_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 126;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
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
                let error = scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
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
        7,
        "scan Parasolid compact arrays"
    );
    plain_surface_boundary!(
        parasolid_scalar_arrays_refuse_before_insertion,
        28,
        "collect Parasolid scalar arrays"
    );
    plain_surface_boundary!(
        parasolid_integer_arrays_refuse_before_insertion,
        32,
        "collect Parasolid integer arrays"
    );
    plain_surface_boundary!(
        parasolid_scalar_values_refuse_before_candidate_copy,
        46,
        "copy Parasolid scalar array values"
    );
    plain_surface_boundary!(
        parasolid_scalar_candidates_refuse_before_insertion,
        58,
        "collect Parasolid scalar candidates"
    );
    plain_surface_boundary!(
        parasolid_compact_scalar_values_refuse_before_allocation,
        59,
        "decode Parasolid compact scalar values"
    );
    plain_surface_boundary!(
        parasolid_integer_values_refuse_before_candidate_copy,
        75,
        "copy Parasolid integer array values"
    );
    plain_surface_boundary!(
        parasolid_integer_candidates_refuse_before_insertion,
        77,
        "collect Parasolid integer candidates"
    );
    plain_surface_boundary!(
        parasolid_compact_integer_values_refuse_before_allocation,
        78,
        "decode Parasolid compact integer values"
    );
    plain_surface_boundary!(
        parasolid_knot_multiplicity_groups_refuse_before_insertion,
        81,
        "group Parasolid knot multiplicities"
    );
    plain_surface_boundary!(
        parasolid_distinct_knots_refuse_before_copy,
        87,
        "copy Parasolid distinct knots"
    );
    plain_surface_boundary!(
        parasolid_knot_multiplicities_refuse_before_copy,
        89,
        "copy Parasolid knot multiplicities"
    );
    plain_surface_boundary!(
        parasolid_knot_candidates_refuse_before_insertion,
        91,
        "collect Parasolid knot candidates"
    );
    plain_surface_boundary!(
        parasolid_surface_pole_rows_refuse_before_partition,
        121,
        "partition Parasolid surface pole rows"
    );
    plain_surface_boundary!(
        parasolid_surface_poles_refuse_before_partition,
        123,
        "partition Parasolid surface poles"
    );
    plain_surface_boundary!(
        parasolid_surface_pole_rows_refuse_before_admission,
        127,
        "IR NURBS admitted grid rows"
    );
    plain_surface_boundary!(
        parasolid_surface_poles_refuse_before_admission,
        129,
        "IR NURBS admitted poles"
    );
    plain_surface_boundary!(
        parasolid_surface_carriers_refuse_before_insertion,
        133,
        "collect Parasolid surface carriers"
    );
    surface_collection_boundary!(
        parasolid_surface_weight_rows_refuse_before_partition,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        144,
        "partition Parasolid surface weight rows"
    );
    surface_collection_boundary!(
        parasolid_surface_weights_refuse_before_partition,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        146,
        "partition Parasolid surface weights"
    );
    surface_collection_boundary!(
        parasolid_weighted_surface_rows_refuse_before_pairing,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        150,
        "IR NURBS paired grid rows"
    );
    surface_collection_boundary!(
        parasolid_weighted_surface_poles_refuse_before_pairing,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        152,
        "IR NURBS paired poles"
    );

    #[test]
    fn patch_shape_counts_do_not_narrow_to_the_native_multiplicity_width() {
        let count = usize::from(u16::MAX) + 1;
        let knots = std::iter::repeat_n(0.0, count)
            .chain(std::iter::once(1.0))
            .collect::<Vec<_>>();
        let (values, multiplicities) =
            unique_knots(&cadmpeg_test_support::service_decode_context(), &knots).unwrap();
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
                &cadmpeg_test_support::service_decode_context(),
                &NurbsPoles3::Rational {
                    points: vec![cadmpeg_ir::geometry::nurbs::WeightedPole3 {
                        point,
                        weight: cadmpeg_ir::scalar::NonZeroReal::new(1.0e300).unwrap()
                    }]
                },
                0.001
            )
            .unwrap(),
            None
        );
        assert_eq!(
            super::homogeneous_poles(
                &cadmpeg_test_support::service_decode_context(),
                &NurbsPoles3::Rational {
                    points: vec![cadmpeg_ir::geometry::nurbs::WeightedPole3 {
                        point,
                        weight: cadmpeg_ir::scalar::NonZeroReal::new(2.0).unwrap()
                    }]
                },
                0.001
            )
            .unwrap(),
            Some(vec![1.0e300 * 0.001 * 2.0, 0.0, 0.0, 2.0])
        );
    }

    #[test]
    fn missing_surface_knot_arrays_decline_the_patch() {
        assert_eq!(
            unique_surface_knot_span(
                &cadmpeg_test_support::service_decode_context(),
                &[],
                &Arrays::default(),
                (2, 3),
                1,
                &[0.0],
                &[1]
            )
            .unwrap(),
            None
        );
    }
}

#[cfg(test)]
mod patch_tests;

#[cfg(test)]
mod knot_work_tests {
    #[test]
    fn parasolid_knot_expansion_refuses_scan_and_emission_work() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        for (cap, operation) in [
            (0, "scan Parasolid knot multiplicities"),
            (2, "emit Parasolid expanded knots"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let result = super::expanded_knots(&ctx, &[0.0, 1.0], &[2, 2], 4);
            assert!(
                matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation)
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 6;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert_eq!(
            super::expanded_knots(&ctx, &[0.0, 1.0], &[2, 2], 4).expect("scan and emission"),
            Some(vec![0.0, 0.0, 1.0, 1.0])
        );
    }
}
