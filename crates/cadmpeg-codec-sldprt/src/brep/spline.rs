// SPDX-License-Identifier: Apache-2.0
//! B-spline/list carrier tables.

pub(crate) mod patch;

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::geometry::{
    nurbs::knots_nondecreasing,
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;

use cadmpeg_core::decode::{index_from_u32, DecodeContext, ScopedReservation, View};

use super::{CurveCarrier, SurfaceCarrier, LEN_TO_MM};

use crate::layout::bspline_array_header as arr_hdr;
use crate::layout::bspline_compact_array_header as compact_arr;
use crate::layout::bspline_surface_descriptor as surf_desc;

#[derive(Debug, Default)]
struct Arrays<'ctx> {
    f64s: BTreeMap<u16, FramedArray<'ctx, f64>>,
    u16s: BTreeMap<u16, FramedArray<'ctx, u16>>,
    compact: BTreeMap<u16, Vec<CompactArray>>,
}

#[derive(Debug, Clone, Copy)]
struct CompactArray {
    offset: usize,
    count: usize,
}

enum ArrayCandidate<'a, 'ctx, T> {
    Borrowed(&'a [T]),
    Owned {
        values: Vec<T>,
        _storage: ScopedReservation<'ctx>,
    },
}

impl<T> ArrayCandidate<'_, '_, T> {
    fn as_slice(&self) -> &[T] {
        match self {
            Self::Borrowed(values) => values,
            Self::Owned { values, .. } => values,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArraySpan {
    start: usize,
    count: usize,
}

/// A bounded framed payload, decoded once when a consumer first reads it.
/// Its cached values and their reservation have the same lifetime.
#[derive(Debug)]
struct FramedArray<'ctx, T> {
    span: ArraySpan,
    operation: &'static str,
    values: OnceCell<(Vec<T>, ScopedReservation<'ctx>)>,
}

impl<'ctx, T> FramedArray<'ctx, T> {
    fn read(
        &self,
        ctx: &'ctx DecodeContext<'_>,
        read: impl Fn(usize) -> Option<T>,
    ) -> Result<&[T], cadmpeg_core::CodecError> {
        if self.values.get().is_none() {
            let (values, storage) = ctx.with_scoped_storage(self.operation, || {
                read_array(ctx, self.span.count, self.operation, |index| {
                    read(self.span.start + index * std::mem::size_of::<T>())
                })
            })?;
            let Some(values) = values else {
                return Err(cadmpeg_core::CodecError::malformed(
                    "bounded Parasolid array lost a cell",
                ));
            };
            if self.values.set((values, storage)).is_err() {
                return Err(cadmpeg_core::CodecError::malformed(
                    "Parasolid array initialized twice",
                ));
            }
        }
        let Some((values, _storage)) = self.values.get() else {
            return Err(cadmpeg_core::CodecError::malformed(
                "Parasolid array cache is absent",
            ));
        };
        Ok(values.as_slice())
    }
}

#[derive(Debug, Clone, Copy)]
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

fn read_array<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
    mut read: impl FnMut(usize) -> Option<T>,
) -> Result<Option<Vec<T>>, cadmpeg_core::CodecError> {
    let mut values = Vec::new();
    let mut indices = 0..count;
    while let Some(index) = ctx.next_charged(&mut indices, operation)? {
        let Some(value) = read(index) else {
            return Ok(None);
        };
        ctx.push_vec(&mut values, value, operation)?;
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

fn scan_arrays<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    compact_attrs: Option<&BTreeSet<u16>>,
    required_attrs: Option<&BTreeSet<u16>>,
) -> Result<Arrays<'ctx>, cadmpeg_core::CodecError> {
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
        if let Some(required) = required_attrs {
            if !ctx.contains_btree_set(required, &attr, "select Parasolid spline arrays")? {
                continue;
            }
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
            ctx.insert_btree_map(
                &mut arrays.u16s,
                attr,
                FramedArray {
                    span: ArraySpan {
                        start: values_at,
                        count,
                    },
                    operation: "decode Parasolid integer array values",
                    values: OnceCell::new(),
                },
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
            // The native surface format can reserve physical knot slots beyond
            // the descriptor's distinct-knot count. Their bits are not
            // semantic data when the matching multiplicities are zero, so
            // defer finite-value checks until descriptor binding.
            ctx.insert_btree_map(
                &mut arrays.f64s,
                attr,
                FramedArray {
                    span: ArraySpan {
                        start: values_at,
                        count,
                    },
                    operation: "decode Parasolid scalar array values",
                    values: OnceCell::new(),
                },
                "collect Parasolid scalar arrays",
            )?;
        }
    }
    Ok(arrays)
}

/// The distinct value arrays an attribute names: its framed array, borrowed
/// from the table, then each compact array that differs from those before it.
/// Framed reads are cached with their source reservation. Each compact read
/// keeps a reservation only while its values remain a candidate. A value
/// occupies `size_of::<T>()` bytes on the wire.
fn array_candidates<'a, 'ctx, T>(
    ctx: &'ctx DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    bytes: &[u8],
    framed: &'a BTreeMap<u16, FramedArray<'ctx, T>>,
    compact: &BTreeMap<u16, Vec<CompactArray>>,
    attr: u16,
    count_matches: impl Fn(usize) -> Result<bool, cadmpeg_core::CodecError>,
    read: impl Fn(usize) -> Option<T>,
) -> Result<Vec<ArrayCandidate<'a, 'ctx, T>>, cadmpeg_core::CodecError>
where
    T: PartialEq + cadmpeg_core::decode::cost::DecodeCost,
{
    const OPERATION: &str = "collect Parasolid array candidates";
    let width = std::mem::size_of::<T>();
    let mut candidates = Vec::new();
    if let Some(array) = ctx.get_btree_map(framed, &attr, OPERATION)? {
        if count_matches(array.span.count)? {
            let values = array.read(ctx, &read)?;
            ctx.push_scoped_vec(
                storage,
                &mut candidates,
                ArrayCandidate::Borrowed(values),
                OPERATION,
            )?;
        }
    }
    let Some(arrays) = ctx.get_btree_map(compact, &attr, OPERATION)? else {
        return Ok(candidates);
    };
    for array in ctx.admit_iter(arrays, "scan Parasolid compact attribute arrays")? {
        if !count_matches(array.count)? {
            continue;
        }
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
        let mut candidate_storage =
            ctx.reserve_scoped(0, "hold Parasolid compact array candidate")?;
        let Some(values) = candidate_storage.with_storage(|| {
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
            |candidate| {
                Ok(candidate.as_slice().len() == values.len()
                    && ctx.equal(candidate.as_slice(), values.as_slice(), OPERATION)?)
            },
            OPERATION,
        )? {
            ctx.push_scoped_vec(
                storage,
                &mut candidates,
                ArrayCandidate::Owned {
                    values,
                    _storage: candidate_storage,
                },
                OPERATION,
            )?;
        }
    }
    Ok(candidates)
}

/// The one scalar array of `count` values an attribute names, when its
/// candidates of that length agree.
fn exact_f64_array<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    bytes: &[u8],
    arrays: &'a Arrays<'ctx>,
    attr: u16,
    count: usize,
) -> Result<Option<ArrayCandidate<'a, 'ctx, f64>>, cadmpeg_core::CodecError> {
    let candidates = array_candidates(
        ctx,
        storage,
        bytes,
        &arrays.f64s,
        &arrays.compact,
        attr,
        |candidate_count| Ok(candidate_count == count),
        |at| View::f64_be_at(bytes, at),
    )?;
    let mut selected: Option<ArrayCandidate<'a, 'ctx, f64>> = None;
    for candidate in ctx.admit_iter(candidates, "scan Parasolid exact scalar array candidates")? {
        if candidate.as_slice().len() != count {
            continue;
        }
        match &selected {
            Some(first) => {
                if !ctx.equal(
                    first.as_slice(),
                    candidate.as_slice(),
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

    let mut storage = ctx.reserve_scoped(0, "expand Parasolid knots")?;
    let mut out = Vec::new();
    let mut pairs = values[..scan_count].iter().zip(&multiplicities[..scan_count]);
    while pairs.len() != 0 {
        let Some((value, &multiplicity)) =
            ctx.next_charged(&mut pairs, "scan Parasolid knot values")?
        else {
            break;
        };
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
        for _ in ctx.admit_iter(
            0..usize::from(multiplicity),
            "emit Parasolid expanded knots",
        )? {
            storage.with_storage(|| {
                ctx.push_vec(&mut out, *value, "expand Parasolid knots")
            })?;
        }
    }
    Ok(Some(storage.commit_value(out)?))
}

fn multiplicity_sum(
    ctx: &DecodeContext<'_>,
    values: &[u16],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(values, "sum Parasolid knot multiplicities")?
        .try_fold(0usize, |sum, &value| sum.checked_add(usize::from(value))))
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
    let mut scratch = ctx.reserve_scoped(0, "hold Parasolid curve source tables")?;
    let mut wrappers = Vec::new();
    let mut required_attrs = BTreeSet::new();
    {
        let mut descriptor_storage = ctx.reserve_scoped(0, "hold Parasolid wrapper descriptors")?;
        let mut descriptors = None;
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
            if descriptors.is_none() {
                descriptors =
                    Some(descriptor_storage.with_storage(|| scan_curve_descriptors(ctx, bytes))?);
            }
            let Some(descriptors) = &descriptors else {
                continue;
            };
            let Some(descriptor) = curve_descriptor(ctx, bytes, p, descriptors)? else {
                continue;
            };
            for reference in [
                descriptor.control_attr,
                descriptor.multiplicity_attr,
                descriptor.knot_attr,
            ] {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut required_attrs,
                        reference,
                        "collect Parasolid curve array references",
                    )
                })?;
            }
            ctx.push_scoped_vec(
                &mut scratch,
                &mut wrappers,
                (off, attr, *descriptor),
                "collect Parasolid curve wrappers",
            )?;
        }
    }
    if wrappers.is_empty() {
        return Ok(BTreeMap::new());
    }
    let arrays = scratch.with_storage(|| scan_arrays(ctx, bytes, None, Some(&required_attrs)))?;
    let mut out = BTreeMap::new();
    for (off, attr, descriptor) in ctx.admit_iter(wrappers, "decode Parasolid curve wrappers")? {
        // The first carrier of an attribute is kept; a later wrapper is not built.
        if ctx.contains_key_btree_map(&out, &attr, "collect Parasolid curve carriers")? {
            continue;
        }
        let Some(control_array) =
            ctx.get_btree_map(&arrays.f64s, &descriptor.control_attr, CURVE_ARRAYS)?
        else {
            continue;
        };
        let Some(expected_control_values) =
            descriptor.control_count.checked_mul(descriptor.dimension)
        else {
            continue;
        };
        if control_array.span.count != expected_control_values {
            continue;
        }
        let Some(multiplicity_array) =
            ctx.get_btree_map(&arrays.u16s, &descriptor.multiplicity_attr, CURVE_ARRAYS)?
        else {
            continue;
        };
        let Some(knot_array) =
            ctx.get_btree_map(&arrays.f64s, &descriptor.knot_attr, CURVE_ARRAYS)?
        else {
            continue;
        };
        let control = control_array.read(ctx, |at| View::f64_be_at(bytes, at))?;
        let multiplicities = multiplicity_array.read(ctx, |at| View::u16_be_at(bytes, at))?;
        let unique_knots = knot_array.read(ctx, |at| View::f64_be_at(bytes, at))?;
        if !ctx.all_by(
            unique_knots,
            |value| Ok(value.is_finite()),
            "check Parasolid curve knots",
        )? || !knots_nondecreasing(unique_knots, |count| {
            ctx.charge_work(count, "IR NURBS knot order")
        })? {
            continue;
        }
        let Some(dimension) = std::num::NonZeroUsize::new(descriptor.dimension) else {
            continue;
        };
        let mut pole_storage = ctx.reserve_scoped(0, "hold Parasolid curve pole lanes")?;
        let (points, weights, valid) = pole_storage.with_storage(|| {
            let mut points =
                ctx.vector_storage(descriptor.control_count, "decode Parasolid curve poles")?;
            let mut weights = if descriptor.dimension == 4 {
                Some(ctx.vector_storage(descriptor.control_count, "decode Parasolid curve weights")?)
            } else {
                None
            };
            let valid = ctx.all_by(
                control.chunks(dimension.get()),
                |pole| {
                    // A pole holds three or four values.
                    if pole.iter().any(|value| !value.is_finite()) {
                        return Ok(false);
                    }
                    let weight = if descriptor.dimension == 4 {
                        pole[3]
                    } else {
                        1.0
                    };
                    if !weight.is_finite() || weight.abs() <= f64::EPSILON {
                        return Ok(false);
                    }
                    ctx.push_vec(
                        &mut points,
                        Point3::new(
                            pole[0] / weight * LEN_TO_MM,
                            pole[1] / weight * LEN_TO_MM,
                            pole[2] / weight * LEN_TO_MM,
                        ),
                        "decode Parasolid curve poles",
                    )?;
                    if let Some(values) = &mut weights {
                        ctx.push_vec(values, weight, "decode Parasolid curve weights")?;
                    }
                    Ok(true)
                },
                "scan Parasolid curve poles",
            )?;
            Ok::<_, cadmpeg_core::CodecError>((points, weights, valid))
        })?;
        if !valid {
            continue;
        }
        if points.len() != descriptor.control_count {
            continue;
        }
        let expected = points.len() + index_from_u32(descriptor.degree) + 1;
        let mut carrier_storage = ctx.reserve_scoped(0, "retain Parasolid curve candidate")?;
        let Some(nurbs) = carrier_storage.with_storage(|| {
            let Some(knots) = expanded_knots(ctx, unique_knots, multiplicities, expected)? else {
                return Ok(None);
            };
            if knots.len() != expected {
                return Ok(None);
            }
            Ok::<_, cadmpeg_core::CodecError>(Some(
                cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                    ctx,
                    descriptor.degree,
                    knots,
                    points,
                    weights,
                    false,
                )?,
            ))
        })? else {
            continue;
        };
        drop(pole_storage);
        let nurbs = match nurbs {
            Ok(nurbs) => carrier_storage.commit_value(nurbs)?,
            Err(cadmpeg_ir::geometry::nurbs::NurbsError::ResourceLimit(limit)) => {
                return Err(limit.into())
            }
            Err(error) => {
                let note = crate::loss::spline_lane_refusal(
                    ctx,
                    format_args!("curve carrier attribute {attr}: {error}"),
                )?;
                drop(error);
                drop(carrier_storage);
                ctx.push_vec(refusals, note, "collect Parasolid spline refusals")?;
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
fn surface_knot_values<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    bytes: &[u8],
    arrays: &Arrays<'ctx>,
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
        |count| Ok(declared_count != 0 && count >= declared_count),
        |at| View::u16_be_at(bytes, at),
    )?;
    let mut multiplicities_by_count = BTreeMap::<usize, Vec<&[u16]>>::new();
    for multiplicities in ctx.admit_iter(
        &multiplicity_candidates,
        "index Parasolid knot multiplicity counts",
    )? {
        let values = multiplicities.as_slice();
        storage.with_storage(|| {
            ctx.push_btree_group(
                &mut multiplicities_by_count,
                values.len(),
                values,
                "index Parasolid knot multiplicity counts",
                "group Parasolid knot multiplicities",
            )
        })?;
    }
    if multiplicities_by_count.is_empty() {
        return Ok(None);
    }
    let unique_candidates = array_candidates(
        ctx,
        storage,
        bytes,
        &arrays.f64s,
        &arrays.compact,
        knot_attr,
        |count| ctx.contains_key_btree_map(&multiplicities_by_count, &count, OPERATION),
        |at| View::f64_be_at(bytes, at),
    )?;
    let mut resolved = Vec::<(&[f64], &[u16])>::new();
    for unique in ctx.admit_iter(&unique_candidates, "scan Parasolid surface knot candidates")? {
        let unique_values = unique.as_slice();
        let Some(group) =
            ctx.get_btree_map(&multiplicities_by_count, &unique_values.len(), OPERATION)?
        else {
            continue;
        };
        for multiplicities in
            ctx.admit_iter(group, "scan Parasolid knot multiplicity candidates")?
        {
            let Some(candidate) =
                surface_knot_arrays(ctx, unique_values, multiplicities, declared_count)?
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
    let mut scratch = ctx.reserve_scoped(0, "hold Parasolid surface source tables")?;
    let mut required_attrs = BTreeSet::new();
    let mut wrappers = Vec::new();
    {
        let mut descriptor_storage = ctx.reserve_scoped(0, "hold Parasolid wrapper descriptors")?;
        let mut descriptors = None;
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
            let Some(descriptor_at) = p.checked_add(17) else {
                continue;
            };
            let Some(descriptor_attr) = View::u16_be_at(bytes, descriptor_at) else {
                continue;
            };
            if descriptors.is_none() {
                descriptors =
                    Some(descriptor_storage.with_storage(|| scan_surface_descriptors(ctx, bytes))?);
            }
            let Some(descriptors) = &descriptors else {
                continue;
            };
            let Some(descriptor) = ctx.get_btree_map(
                descriptors,
                &descriptor_attr,
                "find Parasolid surface descriptors",
            )?
            else {
                continue;
            };
            for &reference in &descriptor.refs {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut required_attrs,
                        reference,
                        "collect Parasolid surface array references",
                    )
                })?;
            }
            ctx.push_scoped_vec(
                &mut scratch,
                &mut wrappers,
                (off, attr, *descriptor),
                "collect Parasolid surface wrappers",
            )?;
        }
    }
    if wrappers.is_empty() {
        return Ok(BTreeMap::new());
    }
    let arrays = scratch
        .with_storage(|| scan_arrays(ctx, bytes, Some(&required_attrs), Some(&required_attrs)))?;
    let mut out = BTreeMap::new();
    for (off, attr, descriptor) in ctx.admit_iter(wrappers, "decode Parasolid surface wrappers")? {
        // The first carrier of an attribute is kept; a later wrapper is not built.
        if ctx.contains_key_btree_map(&out, &attr, "collect Parasolid surface carriers")? {
            continue;
        }
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
        if control.as_slice().len() != expected_control_values {
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
        let Some(pole_width) = std::num::NonZeroUsize::new(dimension) else {
            continue;
        };
        let mut pole_storage = ctx.reserve_scoped(0, "hold Parasolid surface pole lanes")?;
        let (pole_rows, weight_rows, valid) = pole_storage.with_storage(|| {
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
            let valid = ctx.all_by(
                control.as_slice().chunks(row_width.get()),
                |row| {
                    let mut points =
                        ctx.vector_storage(descriptor.v_count, "decode Parasolid surface poles")?;
                    let mut weights = if descriptor.rational {
                        Some(ctx.vector_storage(
                            descriptor.v_count,
                            "decode Parasolid surface weights",
                        )?)
                    } else {
                        None
                    };
                    if !ctx.all_by(
                        row[..row.len() / dimension * dimension].chunks(pole_width.get()),
                        |pole| {
                            let weight = if descriptor.rational { pole[3] } else { 1.0 };
                            // A pole holds three or four values.
                            if pole.iter().any(|value| !value.is_finite())
                                || weight.abs() <= f64::EPSILON
                            {
                                return Ok(false);
                            }
                            ctx.push_vec(
                                &mut points,
                                Point3::new(
                                    pole[0] / weight * LEN_TO_MM,
                                    pole[1] / weight * LEN_TO_MM,
                                    pole[2] / weight * LEN_TO_MM,
                                ),
                                "decode Parasolid surface poles",
                            )?;
                            if let Some(values) = &mut weights {
                                ctx.push_vec(values, weight, "decode Parasolid surface weights")?;
                            }
                            Ok(true)
                        },
                        "scan Parasolid surface row poles",
                    )? {
                        return Ok(false);
                    }
                    ctx.push_vec(
                        &mut pole_rows,
                        points,
                        "partition Parasolid surface pole rows",
                    )?;
                    if let (Some(rows), Some(weights)) = (&mut weight_rows, weights) {
                        ctx.push_vec(rows, weights, "partition Parasolid surface weight rows")?;
                    }
                    Ok(true)
                },
                "scan Parasolid surface poles",
            )?;
            Ok::<_, cadmpeg_core::CodecError>((pole_rows, weight_rows, valid))
        })?;
        if !valid || pole_rows.len() != descriptor.u_count {
            continue;
        }
        let mut carrier_storage = ctx.reserve_scoped(0, "retain Parasolid surface candidate")?;
        let Some(nurbs) = carrier_storage.with_storage(|| {
            let (Some(u_knots), Some(v_knots)) = (
                expanded_knots(ctx, &u_unique, &u_mult, u_expected)?,
                expanded_knots(ctx, &v_unique, &v_mult, v_expected)?,
            ) else {
                return Ok(None);
            };
            if u_knots.len() != u_expected || v_knots.len() != v_expected {
                return Ok(None);
            }
            Ok::<_, cadmpeg_core::CodecError>(Some(
                cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
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
                )?,
            ))
        })? else {
            continue;
        };
        drop(pole_storage);
        let nurbs = match nurbs {
            Ok(nurbs) => carrier_storage.commit_value(nurbs)?,
            Err(cadmpeg_ir::geometry::nurbs::NurbsError::ResourceLimit(limit)) => {
                return Err(limit.into())
            }
            Err(error) => {
                let note = crate::loss::spline_lane_refusal(
                    ctx,
                    format_args!("surface carrier attribute {attr}: {error}"),
                )?;
                drop(error);
                drop(carrier_storage);
                ctx.push_vec(refusals, note, "collect Parasolid spline refusals")?;
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
mod tests;

#[cfg(test)]
mod patch_tests;

#[cfg(test)]
mod candidate_tests;

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
        let mut policy = DecodePolicy::service();
        // Two paired visits and four knot emissions.
        policy.limits.max_work_units = 6;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert_eq!(
            super::expanded_knots(&ctx, &[0.0, 1.0], &[2, 2], 4).expect("scan and emission"),
            Some(vec![0.0, 0.0, 1.0, 1.0])
        );
    }
}
