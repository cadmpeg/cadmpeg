// SPDX-License-Identifier: Apache-2.0
//! Native NURBS patch spans and homogeneous write-back.

use cadmpeg_core::decode::{index_from_u32, DecodeContext, View};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface};

use crate::layout::bspline_array_header as arr_hdr;
use crate::layout::bspline_compact_array_header as compact_arr;

use super::{
    array_body, curve_descriptor, read_array, scan_arrays, scan_curve_descriptors,
    scan_surface_descriptors, surface_knot_arrays, ArraySpan, Arrays, MAX_ARRAY_VALUES,
};

pub(super) fn unique_knots(
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

fn array_span(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    tag: u8,
    attr: u16,
) -> Result<Option<(usize, usize)>, cadmpeg_core::CodecError> {
    ctx.find_map(
        0..bytes.len().checked_sub(9).map_or(0, |end| end),
        |off| {
            let Some(p) = array_body(bytes, off, tag) else {
                return Ok(None);
            };
            let Some(count) = View::u32_be_at(bytes, p + arr_hdr::COUNT).map(index_from_u32) else {
                return Ok(None);
            };
            Ok((View::u16_be_at(bytes, p + arr_hdr::ATTR) == Some(attr))
                .then_some((p + arr_hdr::LEN, count)))
        },
        "find Parasolid patch scalar array",
    )
}

fn array_spans(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    arrays: &Arrays<'_>,
    tag: u8,
    attr: u16,
) -> Result<Vec<ArraySpan>, cadmpeg_core::CodecError> {
    let mut spans = Vec::new();
    let starts = 0..bytes.len().checked_sub(9).map_or(0, |end| end);
    for off in ctx.admit_iter(starts, "scan Parasolid patch array spans")? {
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
    for array in ctx
        .get_btree_map(
            &arrays.compact,
            &attr,
            "find Parasolid patch compact arrays",
        )?
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
    arrays: &Arrays<'_>,
    attr: u16,
    old_values: &[f64],
) -> Result<Option<ArraySpan>, cadmpeg_core::CodecError> {
    let (result, _storage) = ctx.with_scoped_storage(
        "hold Parasolid patch control span search",
        || -> Result<_, cadmpeg_core::CodecError> {
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
            let mut spans = spans.into_iter();
            while let Some(span) =
                ctx.next_charged(&mut spans, "scan Parasolid patch control spans")?
            {
                let _value_storage;
                let values;
                (values, _value_storage) = ctx
                    .with_scoped_storage("hold Parasolid patch control values", || {
                        f64_values(ctx, bytes, span)
                    })?;
                let Some(values) = values else {
                    continue;
                };
                if ctx.all_by(
                    values.iter().zip(old_values),
                    |(native, expected)| {
                        let scale = native.abs().max(expected.abs()).max(1.0);
                        Ok((native - expected).abs() <= 16.0 * f64::EPSILON * scale)
                    },
                    "match Parasolid patch control span",
                )? && selected.replace(span).is_some()
                {
                    return Ok(None);
                }
            }
            Ok(selected)
        },
    )?;
    Ok(result)
}

pub(super) fn unique_surface_knot_span(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    arrays: &Arrays<'_>,
    (knot_attr, multiplicity_attr): (u16, u16),
    declared_count: usize,
    old_values: &[f64],
    old_multiplicities: &[usize],
) -> Result<Option<ArraySpan>, cadmpeg_core::CodecError> {
    let (result, _storage) = ctx.with_scoped_storage(
        "hold Parasolid patch knot span search",
        || -> Result<_, cadmpeg_core::CodecError> {
            if old_values.len() != declared_count || old_multiplicities.len() != declared_count {
                return Ok(None);
            }
            let knot_spans = array_spans(ctx, bytes, arrays, 0x80, knot_attr)?;
            let multiplicity_spans = array_spans(ctx, bytes, arrays, 0x7f, multiplicity_attr)?;
            let mut selected = None;
            let ambiguous = ctx.find_map(
                knot_spans,
                |knot_span| {
                    if declared_count == 0 || knot_span.count < declared_count {
                        return Ok(None);
                    }
                    if !ctx.any_by(
                        &multiplicity_spans,
                        |span| Ok(span.count == knot_span.count),
                        "find Parasolid paired patch multiplicity count",
                    )? {
                        return Ok(None);
                    }
                    let _knot_storage;
                    let knots;
                    (knots, _knot_storage) = ctx
                        .with_scoped_storage("hold Parasolid patch knot values", || {
                            f64_values(ctx, bytes, knot_span)
                        })?;
                    let Some(knots) = knots else {
                        return Ok(None);
                    };
                    ctx.find_map(
                        &multiplicity_spans,
                        |&multiplicity_span| {
                            if multiplicity_span.count != knot_span.count {
                                return Ok(None);
                            }
                            let _multiplicity_storage;
                            let multiplicities;
                            (multiplicities, _multiplicity_storage) = ctx
                                .with_scoped_storage("hold Parasolid patch multiplicity values", || {
                                    u16_values(ctx, bytes, multiplicity_span)
                                })?;
                            let Some(multiplicities) = multiplicities else {
                                return Ok(None);
                            };
                            let Some((knots, multiplicities)) =
                                surface_knot_arrays(ctx, &knots, &multiplicities, declared_count)?
                            else {
                                return Ok(None);
                            };
                            if ctx.all_by(
                                knots.iter().zip(old_values),
                                |(native, expected)| Ok(native == expected),
                                "match Parasolid patch knot spans",
                            )? && ctx.all_by(
                                multiplicities.iter().zip(old_multiplicities),
                                |(&native, &expected)| Ok(usize::from(native) == expected),
                                "match Parasolid patch multiplicity values",
                            )? {
                                if selected.replace(knot_span).is_some() {
                                    return Ok(Some(()));
                                }
                            }
                            Ok(None)
                        },
                        "scan Parasolid patch multiplicity spans",
                    )
                },
                "scan Parasolid patch knot spans",
            )?;
            Ok(if ambiguous.is_some() {
                None
            } else {
                selected
            })
        },
    )?;
    Ok(result)
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
    let Some((start, count)) = array_span(ctx, bytes, tag, attr)? else {
        return Ok(None);
    };
    if count != values.len() {
        return Ok(None);
    }
    patch_f64_span(ctx, bytes, ArraySpan { start, count }, values)
}

pub(super) fn append_homogeneous_pole(
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

pub(super) fn homogeneous_poles(
    ctx: &DecodeContext<'_>,
    poles: &NurbsPoles3<FinitePoint3>,
    scale: f64,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "collect Parasolid homogeneous patch poles")?;
    let mut out = Vec::new();
    match poles {
        NurbsPoles3::Polynomial { points } => {
            let mut points = points.iter();
            while !points.as_slice().is_empty() {
                let Some(point) =
                    ctx.next_charged(&mut points, "form Parasolid homogeneous patch pole")?
                else {
                    break;
                };
                if storage
                    .with_storage(|| append_homogeneous_pole(ctx, &mut out, *point, None, scale))?
                    .is_none()
                {
                    return Ok(None);
                }
            }
        }
        NurbsPoles3::Rational { points } => {
            let mut points = points.iter();
            while !points.as_slice().is_empty() {
                let Some(pole) =
                    ctx.next_charged(&mut points, "form Parasolid homogeneous patch pole")?
                else {
                    break;
                };
                if storage
                    .with_storage(|| {
                        append_homogeneous_pole(
                            ctx,
                            &mut out,
                            pole.point,
                            Some(pole.weight.get()),
                            scale,
                        )
                    })?
                    .is_none()
                {
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(storage.commit_value(out)?))
}

pub(super) fn homogeneous_grid_poles(
    ctx: &DecodeContext<'_>,
    poles: &NurbsPoleGrid<FinitePoint3>,
    scale: f64,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "collect Parasolid homogeneous patch poles")?;
    let mut out = Vec::new();
    match poles {
        NurbsPoleGrid::Polynomial { rows } => {
            let mut rows = rows.iter();
            while !rows.as_slice().is_empty() {
                let Some(row) =
                    ctx.next_charged(&mut rows, "visit Parasolid homogeneous patch row")?
                else {
                    break;
                };
                let mut points = row.iter();
                while !points.as_slice().is_empty() {
                    let Some(point) =
                        ctx.next_charged(&mut points, "form Parasolid homogeneous patch pole")?
                    else {
                        break;
                    };
                    if storage
                        .with_storage(|| {
                            append_homogeneous_pole(ctx, &mut out, *point, None, scale)
                        })?
                        .is_none()
                    {
                        return Ok(None);
                    }
                }
            }
        }
        NurbsPoleGrid::Rational { rows } => {
            let mut rows = rows.iter();
            while !rows.as_slice().is_empty() {
                let Some(row) =
                    ctx.next_charged(&mut rows, "visit Parasolid homogeneous patch row")?
                else {
                    break;
                };
                let mut points = row.iter();
                while !points.as_slice().is_empty() {
                    let Some(pole) =
                        ctx.next_charged(&mut points, "form Parasolid homogeneous patch pole")?
                    else {
                        break;
                    };
                    if storage
                        .with_storage(|| {
                            append_homogeneous_pole(
                                ctx,
                                &mut out,
                                pole.point,
                                Some(pole.weight.get()),
                                scale,
                            )
                        })?
                        .is_none()
                    {
                        return Ok(None);
                    }
                }
            }
        }
    }
    Ok(Some(storage.commit_value(out)?))
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
    let (result, _storage) = ctx.with_scoped_storage(
        "hold Parasolid curve patch scratch",
        || -> Result<_, cadmpeg_core::CodecError> {
            if old.degree() != new.degree()
                || old.pole_count() != new.pole_count()
                || matches!(old.pole_rows(), NurbsPoles3::Rational { .. })
                    != matches!(new.pole_rows(), NurbsPoles3::Rational { .. })
                || old.periodic() != new.periodic()
            {
                return Ok(None);
            }
            let (old_unique, old_mult) = unique_knots(ctx, old.knots())?;
            let (new_unique, new_mult) = unique_knots(ctx, new.knots())?;
            if old_unique.len() != new_unique.len()
                || old_mult.len() != new_mult.len()
                || !ctx.equal(
                    &old_mult,
                    &new_mult,
                    "compare Parasolid patch curve multiplicities",
                )?
            {
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
                || descriptor.control_count != old.pole_count()
                || descriptor.dimension
                    != if matches!(old.pole_rows(), NurbsPoles3::Rational { .. }) {
                        4
                    } else {
                        3
                    }
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
        },
    )?;
    Ok(result)
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
    let (result, _storage) = ctx.with_scoped_storage(
        "hold Parasolid surface patch scratch",
        || -> Result<_, cadmpeg_core::CodecError> {
            if old.u_degree() != new.u_degree()
                || old.v_degree() != new.v_degree()
                || old.u_count() != new.u_count()
                || old.v_count() != new.v_count()
                || matches!(old.pole_grid(), NurbsPoleGrid::Rational { .. })
                    != matches!(new.pole_grid(), NurbsPoleGrid::Rational { .. })
                || old.u_periodic() != new.u_periodic()
                || old.v_periodic() != new.v_periodic()
            {
                return Ok(None);
            }
            let (old_u, old_u_mult) = unique_knots(ctx, old.u_knots())?;
            let (new_u, new_u_mult) = unique_knots(ctx, new.u_knots())?;
            let (old_v, old_v_mult) = unique_knots(ctx, old.v_knots())?;
            let (new_v, new_v_mult) = unique_knots(ctx, new.v_knots())?;
            if old_u.len() != new_u.len()
                || old_v.len() != new_v.len()
                || old_u_mult.len() != new_u_mult.len()
                || old_v_mult.len() != new_v_mult.len()
                || !ctx.equal(
                    &old_u_mult,
                    &new_u_mult,
                    "compare Parasolid patch U multiplicities",
                )?
                || !ctx.equal(
                    &old_v_mult,
                    &new_v_mult,
                    "compare Parasolid patch V multiplicities",
                )?
            {
                return Ok(None);
            }
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
            let descriptors = scan_surface_descriptors(ctx, bytes)?;
            let Some(descriptor) = ctx.get_btree_map(
                &descriptors,
                &descriptor_attr,
                "find Parasolid patch surface descriptor",
            )?
            else {
                return Ok(None);
            };
            let [control_attr, _, _, u_knot_attr, v_knot_attr] = descriptor.refs;
            let dimension = if matches!(old.pole_grid(), NurbsPoleGrid::Rational { .. }) {
                4
            } else {
                3
            };
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
            let compact_attrs =
                ctx.collect_btree_set(descriptor.refs, "index Parasolid patch compact attributes")?;
            let framed_attrs = std::collections::BTreeSet::new();
            let arrays = scan_arrays(ctx, bytes, Some(&compact_attrs), Some(&framed_attrs))?;
            let Some(old_poles) = homogeneous_grid_poles(ctx, old.pole_grid(), scale)? else {
                return Ok(None);
            };
            let Some(control_span) =
                unique_control_span(ctx, bytes, &arrays, control_attr, &old_poles)?
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
            let Some(poles) = homogeneous_grid_poles(ctx, new.pole_grid(), scale)? else {
                return Ok(None);
            };
            if patch_f64_span(ctx, bytes, control_span, &poles)?.is_none() {
                return Ok(None);
            }
            if patch_f64_span(ctx, bytes, u_knot_span, &new_u)?.is_none() {
                return Ok(None);
            }
            patch_f64_span(ctx, bytes, v_knot_span, &new_v)
        },
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests;
