// SPDX-License-Identifier: Apache-2.0
//! B-spline/list carrier tables.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

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
    f64s: BTreeMap<u16, Vec<f64>>,
    u16s: BTreeMap<u16, Vec<u16>>,
    compact: BTreeMap<u16, Vec<CompactArray>>,
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

/// Lookup of the arrays a curve descriptor names.
const CURVE_ARRAYS: &str = "find Parasolid curve arrays";

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
    compact_attrs: Option<&BTreeSet<u16>>,
) -> Result<Arrays, cadmpeg_core::CodecError> {
    let mut arrays = Arrays::default();
    let starts = 0..bytes.len().checked_sub(9).map_or(0, |end| end);
    for off in ctx.admit_iter(starts, "scan Parasolid spline arrays")? {
        if let Some(attrs) = compact_attrs {
            let count = bytes
                .get(off + compact_arr::COUNT)
                .copied()
                .map_or(0, usize::from);
            if bytes.get(off) == Some(&0) && count > 0 {
                if let Some(attr) = View::u16_be_at(bytes, off + compact_arr::ATTR) {
                    if ctx.contains_btree_set(attrs, &attr, "index Parasolid compact arrays")? {
                        ctx.push_btree_group(
                            &mut arrays.compact,
                            attr,
                            CompactArray { offset: off, count },
                            "index Parasolid compact arrays",
                            "scan Parasolid compact arrays",
                        )?;
                    }
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
        // The first array of an attribute is kept; a later one is not read.
        if tag == 0x7f {
            if ctx.contains_key_btree_map(
                &arrays.u16s,
                &attr,
                "collect Parasolid integer arrays",
            )? {
                continue;
            }
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
            ctx.insert_btree_map(
                &mut arrays.u16s,
                attr,
                values,
                "collect Parasolid integer arrays",
            )?;
        } else {
            if ctx.contains_key_btree_map(&arrays.f64s, &attr, "collect Parasolid scalar arrays")? {
                continue;
            }
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
            ctx.insert_btree_map(
                &mut arrays.f64s,
                attr,
                values,
                "collect Parasolid scalar arrays",
            )?;
        }
    }
    Ok(arrays)
}

/// The distinct value arrays an attribute names: its framed array, borrowed
/// from the table, then each compact array that differs from those before it.
/// Compact reads are held under the caller's scoped reservation. A value
/// occupies `size_of::<T>()` bytes on the wire.
fn array_candidates<'a, T>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    bytes: &[u8],
    framed: &'a BTreeMap<u16, Vec<T>>,
    compact: &BTreeMap<u16, Vec<CompactArray>>,
    attr: u16,
    read: impl Fn(usize) -> Option<T>,
) -> Result<Vec<Cow<'a, [T]>>, cadmpeg_core::CodecError>
where
    T: Clone + PartialEq + cadmpeg_core::decode::cost::DecodeCost,
{
    const OPERATION: &str = "collect Parasolid array candidates";
    let width = std::mem::size_of::<T>();
    let mut candidates = Vec::new();
    if let Some(values) = ctx.get_btree_map(framed, &attr, OPERATION)? {
        ctx.push_scoped_vec(
            storage,
            &mut candidates,
            Cow::Borrowed(values.as_slice()),
            OPERATION,
        )?;
    }
    let Some(arrays) = ctx.get_btree_map(compact, &attr, OPERATION)? else {
        return Ok(candidates);
    };
    for array in ctx.admit_iter(arrays, "scan Parasolid compact attribute arrays")? {
        let Some(start) = array.offset.checked_add(compact_arr::LEN) else {
            continue;
        };
        if array
            .count
            .checked_mul(width)
            .and_then(|size| start.checked_add(size))
            .is_none_or(|end| end > bytes.len())
        {
            continue;
        }
        let Some(values) = storage.with_storage(|| {
            read_array(
                ctx,
                array.count,
                "decode Parasolid compact values",
                |index| read(start + index * width),
            )
        })?
        else {
            continue;
        };
        if !ctx.any_by(
            &candidates,
            |candidate| ctx.equal(candidate.as_ref(), values.as_slice(), OPERATION),
            OPERATION,
        )? {
            ctx.push_scoped_vec(storage, &mut candidates, Cow::Owned(values), OPERATION)?;
        }
    }
    Ok(candidates)
}

/// The one scalar array of `count` values an attribute names, when its
/// candidates of that length agree.
fn exact_f64_array<'a>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    bytes: &[u8],
    arrays: &'a Arrays,
    attr: u16,
    count: usize,
) -> Result<Option<Cow<'a, [f64]>>, cadmpeg_core::CodecError> {
    let candidates = array_candidates(
        ctx,
        storage,
        bytes,
        &arrays.f64s,
        &arrays.compact,
        attr,
        |at| View::f64_be_at(bytes, at),
    )?;
    let mut selected: Option<Cow<'a, [f64]>> = None;
    for candidate in ctx.admit_iter(candidates, "scan Parasolid exact scalar array candidates")? {
        if candidate.len() != count {
            continue;
        }
        match &selected {
            Some(first) => {
                if !ctx.equal(
                    first.as_ref(),
                    candidate.as_ref(),
                    "compare Parasolid exact scalar array candidates",
                )? {
                    return Ok(None);
                }
            }
            None => selected = Some(candidate),
        }
    }
    Ok(selected)
}

fn scan_curve_descriptors(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<BTreeMap<u16, CurveDescriptor>, cadmpeg_core::CodecError> {
    let mut out = BTreeMap::new();
    let starts = 0..bytes.len().checked_sub(29).map_or(0, |end| end);
    for off in ctx.admit_iter(starts, "scan Parasolid curve descriptors")? {
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
        ctx.entry_btree_map(&mut out, attr, "collect Parasolid curve descriptors")?
            .or_insert(CurveDescriptor {
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

/// The first descriptor a wrapper names among the 22 byte positions after
/// its attribute.
fn curve_descriptor<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    attr_at: usize,
    descriptors: &'a BTreeMap<u16, CurveDescriptor>,
) -> Result<Option<&'a CurveDescriptor>, cadmpeg_core::CodecError> {
    for at in attr_at + 2..(attr_at + 24).min(bytes.len().checked_sub(1).map_or(0, |end| end)) {
        let Some(reference) = View::u16_be_at(bytes, at) else {
            continue;
        };
        if let Some(descriptor) =
            ctx.get_btree_map(descriptors, &reference, "find Parasolid curve descriptors")?
        {
            return Ok(Some(descriptor));
        }
    }
    Ok(None)
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

    let mut out = Vec::new();
    for (value, &multiplicity) in ctx
        .admit_iter(&values[..scan_count], "scan Parasolid knot values")?
        .zip(&multiplicities[..scan_count])
    {
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
        for _ in ctx.admit_iter(0..usize::from(multiplicity), "emit Parasolid expanded knots")? {
            ctx.push_vec(&mut out, *value, "expand Parasolid knots")?;
        }
    }
    Ok(Some(out))
}

fn unique_knots(
    ctx: &DecodeContext<'_>,
    knots: &[f64],
) -> Result<(Vec<f64>, Vec<usize>), cadmpeg_core::CodecError> {
    let mut values = Vec::new();
    let mut multiplicities = Vec::<usize>::new();
    for &knot in ctx.admit_iter(knots, "compress Parasolid patch knots")? {
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

fn multiplicity_sum(
    ctx: &DecodeContext<'_>,
    values: &[u16],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(values, "sum Parasolid knot multiplicities")?
        .try_fold(0usize, |sum, &value| sum.checked_add(usize::from(value))))
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
    for array in arrays
        .compact
        .get(&attr)
        .map(|values| ctx.admit_iter(values, "scan Parasolid compact attribute arrays"))
        .transpose()?
        .into_iter()
        .flatten()
    {
        ctx.push_vec(
            &mut spans,
            ArraySpan {
                start: array.offset + compact_arr::LEN,
                count: array.count,
            },
            "collect Parasolid patch array spans",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut spans,
        |value| (value.start, value.count),
        Ord::cmp,
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
                    .checked_mul(2)
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
                surface_knot_arrays(ctx, &knots, &multiplicities, declared_count)?
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
    ctx.stable_sort_by_key(
        &mut pairs,
        |value| {
            let (left_knots, left_multiplicities) = value;
            (
                left_knots.start,
                left_knots.count,
                left_multiplicities.start,
            )
        },
        Ord::cmp,
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
    let Some(descriptor) = curve_descriptor(ctx, bytes, p, &descriptors)? else {
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
    let compact_attrs = ctx.collect_btree_set(
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
) -> Result<BTreeMap<u16, CurveCarrier>, cadmpeg_core::CodecError> {
    let arrays = scan_arrays(ctx, bytes, None)?;
    let descriptors = scan_curve_descriptors(ctx, bytes)?;
    let mut out = BTreeMap::new();
    let starts = 0..bytes.len().checked_sub(6).map_or(0, |end| end);
    for off in ctx.admit_iter(starts, "scan Parasolid curve wrappers")? {
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
        // The first carrier of an attribute is kept; a later wrapper is not built.
        if ctx.contains_key_btree_map(&out, &attr, "collect Parasolid curve carriers")? {
            continue;
        }
        let Some(descriptor) = curve_descriptor(ctx, bytes, p, &descriptors)? else {
            continue;
        };
        let Some(control) =
            ctx.get_btree_map(&arrays.f64s, &descriptor.control_attr, CURVE_ARRAYS)?
        else {
            continue;
        };
        let Some(multiplicities) =
            ctx.get_btree_map(&arrays.u16s, &descriptor.multiplicity_attr, CURVE_ARRAYS)?
        else {
            continue;
        };
        let Some(unique_knots) =
            ctx.get_btree_map(&arrays.f64s, &descriptor.knot_attr, CURVE_ARRAYS)?
        else {
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
        if !ctx.all_by(
            unique_knots,
            |value| Ok(value.is_finite()),
            "check Parasolid curve knots",
        )? || !knots_nondecreasing(unique_knots, |count| {
            ctx.charge_work(count, "IR NURBS knot order")
        })? {
            continue;
        }
        let mut points =
            ctx.vector_storage(descriptor.control_count, "decode Parasolid curve poles")?;
        let mut weights = if descriptor.dimension == 4 {
            Some(ctx.vector_storage(descriptor.control_count, "decode Parasolid curve weights")?)
        } else {
            None
        };
        let Some(dimension) = std::num::NonZeroUsize::new(descriptor.dimension) else {
            continue;
        };
        for pole in ctx
            .admit_iter(control.as_slice(), "scan Parasolid curve poles")?
            .chunks(dimension)
        {
            // A pole holds three or four values.
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
            ctx.push_vec(
                &mut (points),
                Point3::new(
                    pole[0] / weight * LEN_TO_MM,
                    pole[1] / weight * LEN_TO_MM,
                    pole[2] / weight * LEN_TO_MM,
                ),
                "decode Parasolid curve poles",
            )?;
            if let Some(values) = &mut weights {
                ctx.push_vec(&mut *values, weight, "decode Parasolid curve weights")?;
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
        let nurbs = match cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
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
                ctx.reserve_capacity(refusals, 1, "collect Parasolid spline refusals")?;
                refusals.push(note);
                continue;
            }
        };
        ctx.entry_btree_map(&mut out, attr, "collect Parasolid curve carriers")?
            .or_insert(CurveCarrier {
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
) -> Result<BTreeMap<u16, SurfaceDescriptor>, cadmpeg_core::CodecError> {
    let mut out = BTreeMap::new();
    let starts = 0..bytes.len().checked_sub(1).map_or(0, |end| end);
    for off in ctx.admit_iter(starts, "scan Parasolid surface descriptors")? {
        if bytes.get(off..off + 2) != Some(&[0x00, 0x7e]) {
            continue;
        }
        let Some(descriptor) = parse_surface_descriptor(bytes, off) else {
            continue;
        };
        ctx.entry_btree_map(
            &mut out,
            descriptor.attr,
            "collect Parasolid surface descriptors",
        )?
        .or_insert(descriptor);
    }
    Ok(out)
}

/// Distinct knots and their multiplicities, of one declared length.
type KnotLanes<'a> = (&'a [f64], &'a [u16]);

fn surface_knot_arrays<'a>(
    ctx: &DecodeContext<'_>,
    unique: &'a [f64],
    multiplicities: &'a [u16],
    declared_count: usize,
) -> Result<Option<KnotLanes<'a>>, cadmpeg_core::CodecError> {
    if declared_count == 0 || unique.len() != multiplicities.len() || unique.len() < declared_count
    {
        return Ok(None);
    }
    let (declared, trailing) = multiplicities.split_at(declared_count);
    Ok((ctx.all_by(
        declared,
        |&value| Ok(value != 0),
        "scan Parasolid declared knot multiplicities",
    )? && ctx.all_by(
        trailing,
        |&value| Ok(value == 0),
        "scan Parasolid trailing knot multiplicities",
    )?)
    .then(|| (unique.split_at(declared_count).0, declared)))
}

struct SurfaceKnotValues {
    unique: Vec<f64>,
    multiplicities: Vec<u16>,
}

/// The one distinct-knot and multiplicity pairing an axis names, when every
/// candidate pairing that fits the declared count agrees.
fn surface_knot_values(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    bytes: &[u8],
    arrays: &Arrays,
    knot_attr: u16,
    multiplicity_attr: u16,
    declared_count: usize,
) -> Result<Option<SurfaceKnotValues>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect Parasolid knot candidates";
    let multiplicity_candidates = array_candidates(
        ctx,
        storage,
        bytes,
        &arrays.u16s,
        &arrays.compact,
        multiplicity_attr,
        |at| View::u16_be_at(bytes, at),
    )?;
    let unique_candidates = array_candidates(
        ctx,
        storage,
        bytes,
        &arrays.f64s,
        &arrays.compact,
        knot_attr,
        |at| View::f64_be_at(bytes, at),
    )?;
    let mut multiplicities_by_count = BTreeMap::<usize, Vec<&[u16]>>::new();
    for multiplicities in ctx.admit_iter(
        &multiplicity_candidates,
        "index Parasolid knot multiplicity counts",
    )? {
        storage.with_storage(|| {
            ctx.push_btree_group(
                &mut multiplicities_by_count,
                multiplicities.len(),
                multiplicities.as_ref(),
                "index Parasolid knot multiplicity counts",
                "group Parasolid knot multiplicities",
            )
        })?;
    }
    let mut resolved = Vec::<(&[f64], &[u16])>::new();
    for unique in ctx.admit_iter(&unique_candidates, "scan Parasolid surface knot candidates")? {
        let Some(group) = ctx.get_btree_map(&multiplicities_by_count, &unique.len(), OPERATION)?
        else {
            continue;
        };
        for multiplicities in
            ctx.admit_iter(group, "scan Parasolid knot multiplicity candidates")?
        {
            let Some(candidate) =
                surface_knot_arrays(ctx, unique.as_ref(), multiplicities, declared_count)?
            else {
                continue;
            };
            if !ctx.any_by(
                &resolved,
                |(knots, multiplicities)| {
                    Ok(ctx.equal(*knots, candidate.0, OPERATION)?
                        && ctx.equal(*multiplicities, candidate.1, OPERATION)?)
                },
                OPERATION,
            )? {
                ctx.push_scoped_vec(storage, &mut resolved, candidate, OPERATION)?;
            }
        }
    }
    let [(unique, multiplicities)] = resolved.as_slice() else {
        return Ok(None);
    };
    Ok(Some(SurfaceKnotValues {
        unique: storage.with_storage(|| ctx.copy_slice(unique, "copy Parasolid distinct knots"))?,
        multiplicities: storage.with_storage(|| {
            ctx.copy_slice(multiplicities, "copy Parasolid knot multiplicities")
        })?,
    }))
}

/// Every compact NURBS surface carrier in `bytes`, keyed by attribute id.
///
/// `refusals` carries the same meaning as in [`scan_curve_carriers`].
pub(crate) fn scan_surface_carriers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    refusals: &mut Vec<LossNote>,
) -> Result<BTreeMap<u16, SurfaceCarrier>, cadmpeg_core::CodecError> {
    let descriptors = scan_surface_descriptors(ctx, bytes)?;
    let mut compact_attrs = BTreeSet::new();
    for (_, descriptor) in
        ctx.admit_iter(&descriptors, "collect Parasolid surface array references")?
    {
        for &attr in ctx.admit_iter(
            &descriptor.refs,
            "collect Parasolid surface array references",
        )? {
            ctx.insert_btree_set(
                &mut compact_attrs,
                attr,
                "collect Parasolid surface array references",
            )?;
        }
    }
    let arrays = scan_arrays(ctx, bytes, Some(&compact_attrs))?;
    let mut out = BTreeMap::new();
    let starts = 0..bytes.len().checked_sub(1).map_or(0, |end| end);
    for off in ctx.admit_iter(starts, "scan Parasolid surface wrappers")? {
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
        // The first carrier of an attribute is kept; a later wrapper is not built.
        if ctx.contains_key_btree_map(&out, &attr, "collect Parasolid surface carriers")? {
            continue;
        }
        let Some(descriptor_at) = p.checked_add(17) else {
            continue;
        };
        let Some(descriptor_attr) = View::u16_be_at(bytes, descriptor_at) else {
            continue;
        };
        let Some(descriptor) = ctx.get_btree_map(
            &descriptors,
            &descriptor_attr,
            "find Parasolid surface descriptors",
        )?
        else {
            continue;
        };
        let mut storage = ctx.reserve_scoped(0, "hold Parasolid surface array candidates")?;
        let Some(expected_poles) = descriptor.u_count.checked_mul(descriptor.v_count) else {
            continue;
        };
        let Some(expected_control_values) = expected_poles.checked_mul(descriptor.dimension) else {
            continue;
        };
        let Some(control) = exact_f64_array(
            ctx,
            &mut storage,
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
            &mut storage,
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
            &mut storage,
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
        let Some(u_multiplicity_sum) = multiplicity_sum(ctx, &u_mult)? else {
            continue;
        };
        let Some(v_multiplicity_sum) = multiplicity_sum(ctx, &v_mult)? else {
            continue;
        };
        if u_multiplicity_sum != u_expected || v_multiplicity_sum != v_expected {
            continue;
        }
        if !ctx.all_by(
            &u_unique,
            |value| Ok(value.is_finite()),
            "check Parasolid surface knots",
        )? || !ctx.all_by(
            &v_unique,
            |value| Ok(value.is_finite()),
            "check Parasolid surface knots",
        )? || !knots_nondecreasing(&u_unique, |count| {
            ctx.charge_work(count, "IR NURBS knot order")
        })? || !knots_nondecreasing(&v_unique, |count| {
            ctx.charge_work(count, "IR NURBS knot order")
        })? {
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
        // The control array holds `u_count` rows of `v_count` poles.
        let dimension = if descriptor.rational {
            descriptor.dimension
        } else {
            3
        };
        let Some(row_values) = descriptor.v_count.checked_mul(dimension) else {
            continue;
        };
        let Some(row_width) = std::num::NonZeroUsize::new(row_values) else {
            continue;
        };
        let mut pole_rows =
            ctx.vector_storage(descriptor.u_count, "partition Parasolid surface pole rows")?;
        let mut weight_rows = if descriptor.rational {
            Some(ctx.vector_storage(
                descriptor.u_count,
                "partition Parasolid surface weight rows",
            )?)
        } else {
            None
        };
        let Some(pole_width) = std::num::NonZeroUsize::new(dimension) else { continue; };
        let mut valid = true;
        for row in ctx
            .admit_iter(control.as_ref(), "scan Parasolid surface poles")?
            .chunks(row_width)
        {
            let mut points =
                ctx.vector_storage(descriptor.v_count, "decode Parasolid surface poles")?;
            let mut weights = if descriptor.rational {
                Some(ctx.vector_storage(descriptor.v_count, "decode Parasolid surface weights")?)
            } else {
                None
            };
            for pole in ctx.admit_iter(&row[..row.len() / dimension * dimension], "scan Parasolid surface row poles")?.chunks(pole_width) {
                let weight = if descriptor.rational { pole[3] } else { 1.0 };
                // A pole holds three or four values.
                if pole.iter().any(|value| !value.is_finite()) || weight.abs() <= f64::EPSILON {
                    valid = false;
                    break;
                }
                ctx.push_vec(&mut points, Point3::new(
                    pole[0] / weight * LEN_TO_MM,
                    pole[1] / weight * LEN_TO_MM,
                    pole[2] / weight * LEN_TO_MM,
                ), "decode Parasolid surface poles")?;
                if let Some(values) = &mut weights {
                    ctx.push_vec(values, weight, "decode Parasolid surface weights")?;
                }
            }
            if !valid {
                break;
            }
            ctx.push_vec(&mut pole_rows, points, "partition Parasolid surface pole rows")?;
            if let (Some(rows), Some(weights)) = (&mut weight_rows, weights) {
                ctx.push_vec(rows, weights, "partition Parasolid surface weight rows")?;
            }
        }
        if !valid || pole_rows.len() != descriptor.u_count {
            continue;
        }
        let nurbs = match cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
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
                ctx.reserve_capacity(refusals, 1, "collect Parasolid spline refusals")?;
                refusals.push(note);
                continue;
            }
        };
        ctx.entry_btree_map(&mut out, attr, "collect Parasolid surface carriers")?
            .or_insert(SurfaceCarrier {
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
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid curve poles",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
                scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
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
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid curve weights",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
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
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "IR NURBS admitted poles",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
                scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
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
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid surface poles",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
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
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid surface weights",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid surface weights"),
            "{error:?}"
        );
    }

    macro_rules! surface_collection_boundary {
        ($name:ident, $bytes:expr, $operation:literal) => {
            #[test]
            fn $name() {
                let bytes = $bytes;
                let arena = DecodeArena::new();
                let error = cadmpeg_test_support::refusal::resource_limit_at(
                    ResourceDimension::CollectionItems,
                    $operation,
                    |cap| {
                        let mut policy = DecodePolicy::service();
                        policy.limits.max_collection_items = cap;
                        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
                        let result = scan_surface_carriers(&ctx, &bytes, &mut Vec::new());
                        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                        }
                        result
                    },
                );
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
            }
        };
    }

    macro_rules! plain_surface_boundary {
        ($name:ident, $operation:literal) => {
            surface_collection_boundary!(
                $name,
                crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10),
                $operation
            );
        };
    }

    plain_surface_boundary!(
        parasolid_surface_descriptors_refuse_before_insertion,
        "collect Parasolid surface descriptors"
    );
    plain_surface_boundary!(
        parasolid_surface_array_references_refuse_before_collection,
        "collect Parasolid surface array references"
    );
    plain_surface_boundary!(
        parasolid_compact_arrays_refuse_before_insertion,
        "scan Parasolid compact arrays"
    );
    plain_surface_boundary!(
        parasolid_scalar_arrays_refuse_before_insertion,
        "collect Parasolid scalar arrays"
    );
    plain_surface_boundary!(
        parasolid_integer_arrays_refuse_before_insertion,
        "collect Parasolid integer arrays"
    );
    plain_surface_boundary!(
        parasolid_array_candidates_refuse_before_insertion,
        "collect Parasolid array candidates"
    );
    plain_surface_boundary!(
        parasolid_compact_values_refuse_before_allocation,
        "decode Parasolid compact values"
    );
    plain_surface_boundary!(
        parasolid_knot_multiplicity_groups_refuse_before_insertion,
        "group Parasolid knot multiplicities"
    );
    plain_surface_boundary!(
        parasolid_distinct_knots_refuse_before_copy,
        "copy Parasolid distinct knots"
    );
    plain_surface_boundary!(
        parasolid_knot_multiplicities_refuse_before_copy,
        "copy Parasolid knot multiplicities"
    );
    plain_surface_boundary!(
        parasolid_knot_candidates_refuse_before_insertion,
        "collect Parasolid knot candidates"
    );
    plain_surface_boundary!(
        parasolid_surface_pole_rows_refuse_before_partition,
        "partition Parasolid surface pole rows"
    );
    plain_surface_boundary!(
        parasolid_surface_pole_rows_refuse_before_admission,
        "IR NURBS admitted grid rows"
    );
    plain_surface_boundary!(
        parasolid_surface_poles_refuse_before_admission,
        "IR NURBS admitted poles"
    );
    plain_surface_boundary!(
        parasolid_surface_carriers_refuse_before_insertion,
        "collect Parasolid surface carriers"
    );
    surface_collection_boundary!(
        parasolid_surface_weight_rows_refuse_before_partition,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        "partition Parasolid surface weight rows"
    );
    surface_collection_boundary!(
        parasolid_weighted_surface_rows_refuse_before_pairing,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        "IR NURBS paired grid rows"
    );
    surface_collection_boundary!(
        parasolid_weighted_surface_poles_refuse_before_pairing,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
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
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        // One admission covers the value and multiplicity lanes walked together.
        for operation in [
            "scan Parasolid knot values",
            "emit Parasolid expanded knots",
        ] {
            crate::test_support::work_refusal_at(operation, |ctx| {
                super::expanded_knots(ctx, &[0.0, 1.0], &[2, 2], 4)
            });
        }
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert_eq!(
            super::expanded_knots(&ctx, &[0.0, 1.0], &[2, 2], 4).expect("scan and emission"),
            Some(vec![0.0, 0.0, 1.0, 1.0])
        );
    }
}
