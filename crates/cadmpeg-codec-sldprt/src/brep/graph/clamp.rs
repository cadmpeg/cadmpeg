// SPDX-License-Identifier: Apache-2.0
//! Insert homogeneous knots and clamp NURBS curve lanes.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::eval::nurbs_curve_parameter_domain;
use cadmpeg_ir::geometry::nurbs::knots_nondecreasing;
use super::nurbs_pcurves::NurbsPcurveFailure;

fn nurbs_homogeneous_controls(
    ctx: &DecodeContext<'_>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<Option<Vec<[f64; 4]>>, cadmpeg_core::CodecError> {
    let mut controls = Vec::new();
    ctx.reserve_capacity(
        &mut controls,
        curve.pole_count(),
        "collect homogeneous NURBS controls",
    )?;
    for index in ctx.admit_iter(0..curve.pole_count(), "collect homogeneous NURBS controls")? {
        let Some(point) = curve.pole_rows().point_at(index) else {
            return Ok(None);
        };
        let weight = curve.pole_rows().weight_at(index).unwrap_or(1.0);
        if weight <= 0.0 {
            return Ok(None);
        }
        ctx.push_vec(
            &mut (controls),
            [point.x * weight, point.y * weight, point.z * weight, weight],
            "collect homogeneous NURBS controls",
        )?;
    }
    Ok(Some(controls))
}

pub(super) struct HomogeneousCurveLanes<'ctx> {
    pub(super) knots: Vec<f64>,
    pub(super) controls: Vec<[f64; 4]>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn insert_nurbs_homogeneous_knot<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    degree: usize,
    knots: &[f64],
    controls: &[[f64; 4]],
    value: f64,
) -> Result<Option<HomogeneousCurveLanes<'ctx>>, cadmpeg_core::CodecError> {
    if let Some(limit) = ctx.resource_refusal() { return Err(limit.into()); }
    let Some(expected_knots) = controls
        .len()
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    if degree == 0
        || controls.is_empty()
        || knots.len() != expected_knots
        || !value.is_finite()
        || ctx.any_by(
            knots,
            |knot| Ok(!knot.is_finite()),
            "check homogeneous NURBS knots",
        )?
        || !knots_nondecreasing(knots, |count| ctx.charge_work(count, "IR NURBS knot order"))?
    {
        return Ok(None);
    }
    let n = controls.len() - 1;
    let (Some(&start), Some(&end)) = (knots.get(degree), knots.get(n + 1)) else {
        return Ok(None);
    };
    let domain = [start, end];
    if value < domain[0] || value > domain[1] {
        return Ok(None);
    }
    let span = if value == domain[1] {
        n
    } else {
        let Some(span) = ctx.find_by(
            degree..=n,
            |index| Ok(knots[*index] <= value && value < knots[*index + 1]),
            "find homogeneous NURBS knot span",
        )?
        else {
            return Ok(None);
        };
        span
    };
    let multiplicity = ctx
        .admit_iter(knots, "count homogeneous NURBS knot multiplicity")?
        .filter(|knot| **knot == value)
        .count();
    if multiplicity > degree {
        return Ok(None);
    }
    let (generation, storage) = ctx.with_scoped_storage("hold inserted homogeneous NURBS lanes", || {
    let mut inserted_knots = Vec::new();
    ctx.extend_from_slice(
        &mut inserted_knots,
        &knots[..=span],
        "insert homogeneous NURBS knot lane",
    )?;
    ctx.push_vec(
        &mut inserted_knots,
        value,
        "insert homogeneous NURBS knot lane",
    )?;
    ctx.extend_from_slice(
        &mut inserted_knots,
        &knots[span + 1..],
        "insert homogeneous NURBS knot lane",
    )?;

    // Controls before `prefix_end` and from `suffix_start` keep their values
    // (the suffix shifts by one); the controls between are blended. The knot
    // span never precedes the multiplicity of the inserted value: a refused
    // subtraction states that, where a saturating one would alias an
    // impossible span with span 0 and copy the wrong control run.
    let (Some(prefix_end), Some(suffix_start)) = (
        (span + 1).checked_sub(degree),
        span.checked_sub(multiplicity),
    ) else {
        return Ok(None);
    };
    let mut inserted_controls = Vec::new();
    ctx.extend_from_slice(
        &mut inserted_controls,
        &controls[..prefix_end],
        "insert homogeneous NURBS control lane",
    )?;
    for index in ctx.admit_iter(
        prefix_end..=suffix_start,
        "blend homogeneous NURBS controls",
    )? {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return Ok(None);
        }
        let alpha = (value - knots[index]) / denominator;
        ctx.push_vec(
            &mut inserted_controls,
            std::array::from_fn(|axis| {
                alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
            }),
            "insert homogeneous NURBS control lane",
        )?;
    }
    ctx.extend_from_slice(
        &mut inserted_controls,
        &controls[suffix_start..],
        "insert homogeneous NURBS control lane",
    )?;
    Ok::<_, cadmpeg_core::CodecError>(Some((inserted_knots, inserted_controls)))
    })?;
    Ok(generation.map(|(knots, controls)| HomogeneousCurveLanes { knots, controls, _storage: storage }))
}

/// The knots, control points and weights of one clamped NURBS segment, before
/// the carrier mints them.
type ClampedCurveLanes = (Vec<f64>, Vec<cadmpeg_ir::math::Point3>, Option<Vec<f64>>);

/// The clamped segment lanes of `curve` over `domain`. `None` when the curve
/// does not clamp to the domain at all; the lanes themselves are minted by the
/// caller.
pub(super) fn clamp_nurbs_curve_to_domain_lanes(
    ctx: &DecodeContext<'_>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    domain: [f64; 2],
) -> Result<Option<ClampedCurveLanes>, cadmpeg_core::CodecError> {
    if let Some(limit) = ctx.resource_refusal() { return Err(limit.into()); }
    if curve.periodic()
        || !domain[0].is_finite()
        || !domain[1].is_finite()
        || domain[0] >= domain[1]
    {
        return Ok(None);
    }
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(None);
    };
    let Some(original_domain) = nurbs_curve_parameter_domain(curve) else {
        return Ok(None);
    };
    let original_domain = original_domain.endpoints();
    if domain[0] < original_domain[0] || domain[1] > original_domain[1] {
        return Ok(None);
    }
    let (generation, storage) = ctx.with_scoped_storage("hold initial homogeneous NURBS lanes", || {
        let knots = ctx.copy_slice(curve.knots(), "copy NURBS knots for clamping")?;
        let Some(controls) = nurbs_homogeneous_controls(ctx, curve)? else { return Ok(None); };
        Ok::<_, cadmpeg_core::CodecError>(Some((knots, controls)))
    })?;
    let Some((knots, controls)) = generation else { return Ok(None); };
    let mut current = HomogeneousCurveLanes { knots, controls, _storage: storage };
    let Some(full_multiplicity) = degree.checked_add(1) else {
        return Ok(None);
    };
    for value in domain {
        let multiplicity = ctx
            .admit_iter(&current.knots, "count clamped NURBS knot multiplicity")?
            .filter(|knot| **knot == value)
            .count();
        if multiplicity > full_multiplicity {
            return Ok(None);
        }
        for _ in ctx.admit_iter(
            multiplicity..full_multiplicity,
            "insert clamped NURBS knots",
        )? {
            let Some(inserted) =
                insert_nurbs_homogeneous_knot(ctx, degree, &current.knots, &current.controls, value)?
            else {
                return Ok(None);
            };
            current = inserted;
        }
    }
    let (Some(start), Some(end), Some(end_last)) = (
        ctx.position_by(
            &current.knots,
            |knot| Ok(*knot == domain[0]),
            "find clamped NURBS domain start",
        )?,
        ctx.position_by(
            &current.knots,
            |knot| Ok(*knot == domain[1]),
            "find clamped NURBS domain end",
        )?,
        ctx.rposition_by(
            &current.knots,
            |knot| Ok(*knot == domain[1]),
            "find clamped NURBS domain end",
        )?,
    ) else {
        return Ok(None);
    };
    if end <= start || end_last < end || end - start == 0 {
        return Ok(None);
    }
    let (Some(control_slice), Some(knot_slice)) =
        (current.controls.get(start..end), current.knots.get(start..=end_last))
    else {
        return Ok(None);
    };
    let Some(expected_knots) = control_slice
        .len()
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    if knot_slice.len() != expected_knots {
        return Ok(None);
    }
    let segment_knots = ctx.copy_slice(knot_slice, "copy clamped NURBS knots")?;
    let rational = matches!(
        curve.pole_rows(),
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { .. }
    );
    let mut control_points = Vec::new();
    ctx.reserve_capacity(
        &mut control_points,
        control_slice.len(),
        "collect clamped NURBS controls",
    )?;
    let mut weights = if rational {
        let mut weights = Vec::new();
        ctx.reserve_capacity(
            &mut weights,
            control_slice.len(),
            "collect clamped NURBS weights",
        )?;
        Some(weights)
    } else {
        None
    };
    for &[x, y, z, weight] in ctx.admit_iter(control_slice, "collect clamped NURBS controls")? {
        if !weight.is_finite() || weight <= 0.0 {
            return Ok(None);
        }
        ctx.push_vec(
            &mut (control_points),
            cadmpeg_ir::math::Point3::new(x / weight, y / weight, z / weight),
            "collect clamped NURBS controls",
        )?;
        if let Some(weights) = &mut weights {
            ctx.push_vec(&mut *weights, weight, "collect clamped NURBS weights")?;
        }
    }
    Ok(Some((segment_knots, control_points, weights)))
}

pub(super) fn clamp_nurbs_curve_to_domain(
    ctx: &DecodeContext<'_>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    domain: [f64; 2],
) -> Result<Option<cadmpeg_ir::geometry::nurbs::NurbsCurve>, NurbsPcurveFailure> {
    let Some((segment_knots, control_points, weights)) =
        clamp_nurbs_curve_to_domain_lanes(ctx, curve, domain)?
    else {
        return Ok(None);
    };
    Ok(Some(cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        ctx,
        curve.degree(),
        segment_knots,
        control_points,
        weights,
        false,
    )??))
}
