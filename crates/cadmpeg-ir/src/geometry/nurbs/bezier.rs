// SPDX-License-Identifier: Apache-2.0
//! Homogeneous Bezier extraction and rational boundary bounds.

use super::PoleValue;
use crate::features::FinitePoint3;
use crate::math::sum::ExactSignedSum;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit, ScopedReservation};

/// One nonempty knot span and its homogeneous Bezier control polygon.
#[derive(Clone, Debug)]
pub struct HomogeneousBezierSpan<const DIMENSION: usize = 4> {
    /// The span's original parameter interval.
    pub domain: [f64; 2],
    /// Weighted coordinates followed by their weight, such as `(w*x, w*y, w*z, w)`.
    pub controls: Vec<[f64; DIMENSION]>,
}

/// A homogeneous polygon with its live temporary-storage reservation.
#[derive(Debug)]
pub struct HomogeneousControls<'ctx> {
    /// Positive-weight coordinates followed by the homogeneous weight.
    pub controls: Vec<[f64; 4]>,
    _storage: ScopedReservation<'ctx>,
}

/// Form a positive-weight homogeneous polygon without common-scale overflow.
/// Refuse a raw pole with a non-finite coordinate, and a relative weight or
/// coordinate product that would disappear. Absent weights read as 1.0.
pub fn positive_controls<'ctx, P: PoleValue<FinitePoint3>>(
    ctx: &'ctx DecodeContext<'_>,
    points: &[P],
    weights: Option<&[f64]>,
    operation: &'static str,
) -> Result<Option<HomogeneousControls<'ctx>>, ResourceLimit> {
    if weights.is_some_and(|weights| points.len() != weights.len()) || points.is_empty() {
        return Ok(None);
    }
    let mut scale = 1.0;
    if let Some(weights) = weights {
        for weight in weights {
            ctx.charge_work_limit(1, "Bezier positive weight validation")?;
            if !weight.is_finite() || *weight <= 0.0 {
                return Ok(None);
            }
        }
        scale = 0.0_f64;
        for weight in weights {
            ctx.charge_work_limit(1, "Bezier positive weight scale")?;
            scale = scale.max(*weight);
        }
    }
    let storage;
    let mut output = Vec::new();
    storage = ctx.reserve_temporary_vec(&mut output, points.len(), operation)?;
    for (index, point) in points.iter().enumerate() {
        ctx.charge_work_limit(1, "Bezier positive control conversion")?;
        let weight = weights.map_or(1.0, |weights| weights[index]) / scale;
        let Some(point) = point.admit() else {
            return Ok(None);
        };
        let point = point.get();
        if weight == 0.0 {
            return Ok(None);
        }
        let result = [weight * point.x, weight * point.y, weight * point.z, weight];
        if result.iter().any(|value| !value.is_finite())
            || [point.x, point.y, point.z].iter().zip(result)
                .any(|(raw, product)| *raw != 0.0 && product == 0.0)
        {
            return Ok(None);
        }
        output.push(result);
    }
    Ok(Some(HomogeneousControls { controls: output, _storage: storage }))
}

/// Extracted spans with their live header and control-storage reservation.
#[derive(Debug)]
pub struct HomogeneousBezierSpans<'ctx, const DIMENSION: usize = 4> {
    /// Nonempty active spans in parameter order.
    pub spans: Vec<HomogeneousBezierSpan<DIMENSION>>,
    _storage: ScopedReservation<'ctx>,
}

struct BezierWorkingSpline<'ctx, const DIMENSION: usize> {
    knots: Vec<f64>,
    controls: Vec<[f64; DIMENSION]>,
    knot_storage: ScopedReservation<'ctx>,
    control_storage: ScopedReservation<'ctx>,
}

fn knot_multiplicity(
    ctx: &DecodeContext<'_>,
    knots: &[f64],
    value: f64,
) -> Result<usize, ResourceLimit> {
    ctx.charge_work_limit(u64_from_index(knots.len()), "Bezier knot multiplicity scan")?;
    Ok(knots.iter().filter(|knot| **knot == value).count())
}

fn insert_knot<const DIMENSION: usize>(
    ctx: &DecodeContext<'_>,
    degree: usize,
    working: &mut BezierWorkingSpline<'_, DIMENSION>,
    value: f64,
) -> Result<Option<()>, ResourceLimit> {
    let Some(last) = working.controls.len().checked_sub(1) else {
        return Ok(None);
    };
    let mut span = None;
    for (index, knot) in working.knots.iter().enumerate().rev() {
        ctx.charge_work_limit(1, "Bezier insertion span search")?;
        if *knot <= value {
            span = Some(index);
            break;
        }
    }
    let Some(span) = span else {
        return Ok(None);
    };
    let multiplicity = knot_multiplicity(ctx, &working.knots, value)?;
    let Some(first) = span.checked_sub(degree) else {
        return Ok(None);
    };
    let Some(tail) = span.checked_sub(multiplicity) else {
        return Ok(None);
    };
    if multiplicity > degree || first > last || tail > last {
        return Ok(None);
    }
    let Some(_) = working.controls.len().checked_add(1) else {
        return Ok(None);
    };
    ctx.reserve_scoped_vec_limit(&mut working.control_storage, &mut working.controls,
        1, "Bezier knot insertion")?;
    ctx.reserve_scoped_vec_limit(&mut working.knot_storage, &mut working.knots,
        1, "Bezier inserted knot")?;
    ctx.charge_work_limit(u64_from_index(last - tail + 2), "Bezier insertion control shift")?;
    working.controls.push([0.0; DIMENSION]);
    working.controls.copy_within(tail..=last, tail + 1);
    for index in (first + 1..=tail).rev() {
        ctx.charge_work_limit(1, "Bezier insertion control interpolation")?;
        let Some(alpha) = crate::math::parameter_fraction(value,
            working.knots[index], working.knots[index + degree])
            .map(crate::scalar::FiniteReal::get)
        else {
            return Ok(None);
        };
        working.controls[index] = std::array::from_fn(|axis| {
            (1.0 - alpha) * working.controls[index - 1][axis]
                + alpha * working.controls[index][axis]
        });
    }
    ctx.charge_work_limit(u64_from_index(working.knots.len() - span), "Bezier inserted knot shift")?;
    working.knots.insert(span + 1, value);
    Ok(Some(()))
}

/// Extract the active spans, including unclamped endpoints and discontinuities.
/// Knot multiplicity controls the control index of each span. Full internal
/// multiplicity starts a new polygon without a shared endpoint.
pub fn homogeneous_spans<'ctx, const DIMENSION: usize>(
    ctx: &'ctx DecodeContext<'_>,
    degree: usize,
    knots: &[f64],
    controls: &[[f64; DIMENSION]],
) -> Result<Option<HomogeneousBezierSpans<'ctx, DIMENSION>>, ResourceLimit> {
    let count = controls.len();
    let Some(expected_knots) = count.checked_add(degree).and_then(|value| value.checked_add(1)) else {
        return Ok(None);
    };
    if degree >= count || knots.len() != expected_knots {
        return Ok(None);
    }
    for knot in knots {
        ctx.charge_work_limit(1, "Bezier finite knot scan")?;
        if !knot.is_finite() { return Ok(None); }
    }
    for pair in knots.windows(2) {
        ctx.charge_work_limit(1, "Bezier knot order scan")?;
        if pair[0] > pair[1] { return Ok(None); }
    }
    for value in controls.iter().flatten() {
        ctx.charge_work_limit(1, "Bezier finite control scan")?;
        if !value.is_finite() { return Ok(None); }
    }
    let domain = [knots[degree], knots[count]];
    if domain[0] >= domain[1] {
        return Ok(None);
    }
    let knot_storage = ctx.reserve_scoped_limit(0, "Bezier knot copy")?;
    let control_storage = ctx.reserve_scoped_limit(0, "Bezier working controls")?;
    let mut working = BezierWorkingSpline {
        knots: Vec::new(), controls: Vec::new(), knot_storage, control_storage,
    };
    ctx.reserve_scoped_vec_limit(&mut working.knot_storage, &mut working.knots,
        knots.len(), "Bezier knot copy")?;
    ctx.charge_work_limit(u64_from_index(knots.len()), "Bezier knot copy")?;
    working.knots.extend_from_slice(knots);
    ctx.reserve_scoped_vec_limit(&mut working.control_storage, &mut working.controls,
        count, "Bezier working controls")?;
    ctx.charge_work_limit(u64_from_index(count), "Bezier working control copy")?;
    working.controls.extend_from_slice(controls);
    for endpoint in domain {
        while knot_multiplicity(ctx, &working.knots, endpoint)? < degree + 1 {
            if insert_knot(ctx, degree, &mut working, endpoint)?.is_none() { return Ok(None); }
        }
    }
    let _internal_storage;
    let mut internal = Vec::new();
    _internal_storage = ctx.reserve_temporary_vec(&mut internal, working.knots.len(), "Bezier internal knots")?;
    for knot in &working.knots {
        ctx.charge_work_limit(1, "Bezier internal knot scan")?;
        if domain[0] < *knot && *knot < domain[1] {
            ctx.charge_work_limit(1, "Bezier internal knot copy")?;
            internal.push(*knot);
        }
    }
    if !internal.is_empty() {
        ctx.charge_work_limit(u64_from_index(internal.len() - 1), "Bezier internal knot deduplication")?;
        internal.dedup();
    }
    for knot in &internal {
        ctx.charge_work_limit(1, "Bezier internal knot visit")?;
        while knot_multiplicity(ctx, &working.knots, *knot)? < degree {
            if insert_knot(ctx, degree, &mut working, *knot)?.is_none() { return Ok(None); }
        }
    }
    let mut storage = ctx.reserve_scoped_limit(0, "Bezier spans")?;
    let mut spans = Vec::new();
    ctx.reserve_scoped_vec_limit(&mut storage, &mut spans,
        working.controls.len() - degree, "Bezier spans")?;
    for span in degree..working.controls.len() {
        ctx.charge_work_limit(1, "Bezier active span visit")?;
        let interval = [working.knots[span], working.knots[span + 1]];
        if interval[0] < interval[1] && interval[0] >= domain[0] && interval[1] <= domain[1] {
            let mut span_controls = Vec::new();
            ctx.reserve_scoped_vec_limit(&mut storage, &mut span_controls,
                degree + 1, "Bezier span controls")?;
            ctx.charge_work_limit(u64_from_index(degree + 1), "Bezier span control copy")?;
            span_controls.extend_from_slice(&working.controls[span - degree..=span]);
            spans.push(HomogeneousBezierSpan { domain: interval, controls: span_controls });
        }
    }
    if spans.is_empty() { Ok(None) }
    else { Ok(Some(HomogeneousBezierSpans { spans, _storage: storage })) }
}

/// Prove equal-degree positive-weight rational Bezier boundaries agree.
///
/// Bernstein cross-products are compared before converting their scale back
/// to `f64`, so a nonzero difference cannot disappear through underflow.
pub fn boundaries_within_resolution(
    ctx: &DecodeContext<'_>,
    first: &[[f64; 4]],
    second: &[[f64; 4]],
    resolution: f64,
) -> Result<Option<bool>, ResourceLimit> {
    macro_rules! value {
        ($expression:expr) => {
            match $expression {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    if first.is_empty()
        || first.len() != second.len()
        || !resolution.is_finite()
        || resolution < 0.0
    {
        return Ok(None);
    }
    for control in first.iter().chain(second) {
        ctx.charge_work_limit(1, "IR Bezier boundary finite control visit")?;
        if control.iter().any(|value| !value.is_finite()) || control[3] <= 0.0 {
            return Ok(None);
        }
    }
    let degree = first.len() - 1;
    let product_degree = value!(degree.checked_mul(2));
    let binomial = |n: usize, k: usize| -> Result<Option<f64>, ResourceLimit> {
        let k = k.min(n - k);
        ctx.charge_work_limit(u64_from_index(k), "IR Bezier boundary binomial factors")?;
        Ok((1..=k).try_fold(1.0, |value, factor| {
            Some(
                value * cadmpeg_core::convert::f64_from_index(n - k + factor)?
                    / cadmpeg_core::convert::f64_from_index(factor)?,
            )
        }))
    };
    let mut first_weight = f64::INFINITY;
    for control in first {
        ctx.charge_work_limit(1, "IR Bezier boundary first weight visit")?;
        first_weight = first_weight.min(control[3]);
    }
    let mut second_weight = f64::INFINITY;
    for control in second {
        ctx.charge_work_limit(1, "IR Bezier boundary second weight visit")?;
        second_weight = second_weight.min(control[3]);
    }
    let mut threshold = ExactSignedSum::default();
    threshold.add_factors([
        resolution,
        first_weight,
        second_weight,
        1.0 / 3.0_f64.sqrt(),
    ]);
    let threshold = threshold.finish();
    for index in 0..=product_degree {
        ctx.charge_work_limit(1, "IR Bezier boundary product coefficient")?;
        let mut cross = [ExactSignedSum::default(); 3];
        // A control pair contributes when its indices sum to the product index.
        for (first_index, first_control) in first.iter().enumerate() {
            ctx.charge_work_limit(1, "IR Bezier boundary control pair")?;
            let Some(second_index) = index.checked_sub(first_index) else {
                break;
            };
            let Some(second_control) = second.get(second_index) else {
                continue;
            };
            let coefficient = value!(binomial(degree, first_index)?)
                * value!(binomial(degree, second_index)?)
                / value!(binomial(product_degree, index)?);
            if !coefficient.is_finite() {
                return Ok(None);
            }
            for (axis, component) in cross.iter_mut().enumerate() {
                component.add_factors([coefficient, first_control[axis], second_control[3]]);
                component.add_factors([-coefficient, second_control[axis], first_control[3]]);
            }
        }
        for component in cross {
            if let Some(value) = component.finish() {
                let Some(limit) = threshold else {
                    return Ok(Some(false));
                };
                let exponent = value.exponent().max(limit.exponent());
                if value!(value.rescale(exponent)).abs() > value!(limit.rescale(exponent)).abs() {
                    return Ok(Some(false));
                }
            }
        }
    }
    Ok(Some(true))
}

#[cfg(test)]
mod tests {
    use super::{boundaries_within_resolution, homogeneous_spans};
    #[test]
    fn bezier_extraction_admits_each_pass_and_holds_output_storage() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let knots = [0.0, 0.0, 1.0, 1.0];
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        // Four finite knots, three order comparisons, eight finite coordinates,
        // six copied rows, eight multiplicity comparisons, four internal-knot
        // visits, one active-span visit and two output-row copies.
        const WORK: u64 = 36;
        for cap in 0..WORK {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = homogeneous_spans(&ctx, 1, &knots, &controls).expect_err("every pass needs work");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.limit, cap);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                _ => unreachable!("tested storage dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = homogeneous_spans(&ctx, 1, &knots, &controls).expect_err("scratch admission");
            assert_eq!(limit.dimension, dimension);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = WORK;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4096;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let output = homogeneous_spans(&ctx, 1, &knots, &controls).expect("exact work")
            .expect("one active span");
        assert_eq!(output.spans.len(), 1);
        assert_eq!(output.spans[0].domain, [0.0, 1.0]);
        assert_eq!(output.spans[0].controls, controls);
        let bytes = output.spans.capacity() * std::mem::size_of::<super::HomogeneousBezierSpan>()
            + output.spans.iter().map(|span| span.controls.capacity()
                * std::mem::size_of::<[f64; 4]>()).sum::<usize>();
        let bytes = u64::try_from(bytes).expect("test allocation bytes fit");
        assert!(bytes > 0);
        let spare = ctx.reserve_scoped_limit(4096 - bytes, "test live Bezier output")
            .expect("working scratch released while output remains live");
        drop(spare);
        drop(output);
        let reuse = ctx.reserve_scoped_limit(4096, "test Bezier output released")
            .expect("complete scratch allowance reusable");
        drop(reuse);
        ctx.finish_session().expect("no retained temporary storage");
    }

    #[test]
    fn bezier_knot_insertion_preserves_every_caller_work_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let knots = [-1.0, 0.0, 1.0, 2.0];
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        // Initial scans and copies use 21 units. Endpoint multiplicities,
        // searches and shifts use 44. Six internal-knot visits and three
        // active-span visits plus two output-row copies complete extraction.
        const WORK: u64 = 76;
        for cap in 0..WORK {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = homogeneous_spans(&ctx, 1, &knots, &controls)
                .expect_err("unclamped endpoint insertion needs caller work");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.limit, cap);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = WORK;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4096;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let output = homogeneous_spans(&ctx, 1, &knots, &controls).expect("exact insertion work")
            .expect("one active span");
        assert_eq!(output.spans.len(), 1);
        assert_eq!(output.spans[0].domain, [0.0, 1.0]);
        assert_eq!(output.spans[0].controls, controls);
        drop(output);
        let reuse = ctx.reserve_scoped_limit(4096, "test inserted Bezier scratch released")
            .expect("all working and output scratch released");
        drop(reuse);
        ctx.finish_session().expect("exact admitted insertion");
    }

    #[test]
    fn positive_controls_preserve_refusals_and_hold_scoped_storage() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        use crate::math::Point3;
        let points = [Point3::new(0.0, 1.0, 2.0), Point3::new(3.0, 4.0, 5.0)];
        let weights = [2.0, 1.0];
        for cap in 0..6 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = super::positive_controls(&ctx, &points, Some(&weights), "Bezier positive controls")
                .expect_err("weight scans and conversion need work");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, if cap < 2 { "Bezier positive weight validation" }
                else if cap < 4 { "Bezier positive weight scale" }
                else { "Bezier positive control conversion" });
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                _ => unreachable!("tested storage dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = super::positive_controls(&ctx, &points, None, "Bezier positive controls").expect_err("scratch storage");
            assert_eq!(limit.dimension, dimension);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 6;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 64;
        policy.limits.max_collection_items = 2;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let output = super::positive_controls(&ctx, &points, Some(&weights), "Bezier positive controls")
            .expect("exact work and scoped bytes").expect("positive weights");
        assert_eq!(output.controls, [[0.0, 1.0, 2.0, 1.0], [1.5, 2.0, 2.5, 0.5]]);
        drop(output);
        let reuse = ctx.reserve_scoped_limit(64, "test positive controls scratch released")
            .expect("complete scratch allowance is reusable");
        drop(reuse);
        ctx.finish_session().expect("no retained temporary storage");
    }

    #[test]
    fn boundary_certificate_preserves_every_caller_work_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        // Four finite visits, four weight visits, three coefficients, six pair
        // visits and two binomial factors for the middle coefficient.
        const WORK: u64 = 19;
        for cap in 0..WORK {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = boundaries_within_resolution(&ctx, &controls, &controls, 0.0)
                .expect_err("each certificate visit needs caller work");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.limit, cap);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = WORK;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(boundaries_within_resolution(&ctx, &controls, &controls, 0.0), Ok(Some(true)));
        ctx.finish_session().expect("exact work without storage or nesting");
    }

    #[test]
    fn numerical_followup_bezier_spans_preserve_unclamped_and_discontinuous_curves() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let spans = homogeneous_spans(&ctx,
            2,
            &[-1., -1., 0., 1., 2., 2.],
            &[[0., 0., 0., 1.], [1., 1., 0., 1.], [2., 0., 0., 1.]],
        )
        .expect("resource allocation did not fail")
        .unwrap();
        let controls = &spans.spans[0].controls;
        let midpoint = (controls[0][1] + 2.0 * controls[1][1] + controls[2][1]) / 4.0;
        assert_eq!(midpoint, 0.75);
        let spans = homogeneous_spans(&ctx,
            2,
            &[0., 0., 0., 1., 1., 1., 2., 2., 2.],
            &(0..6).map(|i| [f64::from(i), 0., 0., 1.]).collect::<Vec<_>>(),
        )
        .expect("resource allocation did not fail")
        .unwrap();
        assert_eq!(
            spans.spans[1].controls.iter().map(|p| p[0]).collect::<Vec<_>>(),
            [3., 4., 5.]
        );
        let zero = homogeneous_spans(&ctx, 0, &[0., 1., 2.], &[[0., 0., 0., 1.], [1., 0., 0., 1.]])
            .expect("resource allocation did not fail")
            .unwrap();
        assert_eq!(zero.spans.len(), 2);
    }
    #[test]
    fn numerical_followup_boundary_certificate_keeps_small_cross_products() {
        for w in [1.0, 1e-200, 1e200] {
            let a = [[0., 0., 0., w], [w, 0., 0., w]];
            let b = [[0., 2.0 * w, 0., w], [w, 2.0 * w, 0., w]];
            assert_eq!(boundaries_within_resolution(&cadmpeg_test_support::service_decode_context(), &a, &b, 0.001), Ok(Some(false)));
            assert_eq!(boundaries_within_resolution(&cadmpeg_test_support::service_decode_context(), &a, &a, 0.0), Ok(Some(true)));
        }
    }

    #[test]
    fn numerical_0922b_bezier_insertion_preserves_wide_knot_units() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let controls = vec![
            [0., 0., 0., 1.],
            [1., 0., 0., 1.],
            [1., 1., 0., 1.],
            [2., 1., 0., 1.],
        ];
        let expected = homogeneous_spans(&ctx, 2, &[-1., -1., -1., 0., 1., 1., 1.], &controls)
            .expect("resource allocation did not fail")
            .unwrap();
        let actual = homogeneous_spans(&ctx,
            2,
            &[-1e308, -1e308, -1e308, 0., 1e308, 1e308, 1e308],
            &controls,
        )
        .expect("resource allocation did not fail")
        .unwrap();
        assert_eq!(actual.spans.len(), expected.spans.len());
        for (actual, expected) in actual.spans.iter().zip(&expected.spans) {
            assert_eq!(actual.controls, expected.controls);
            assert_eq!(actual.domain.map(|t| t / 1e308), expected.domain);
        }
    }
}
