// SPDX-License-Identifier: Apache-2.0
//! Homogeneous Bezier control-polygon splitting, restriction, and chord bounds.

use crate::geometry::nurbs::scoped::ScopedRows;
use crate::math::Point3;
use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit, ScopedReservation};

pub(super) struct HomogeneousBezierSplit<'ctx> {
    left: Vec<[f64; 4]>,
    point: [f64; 4],
    right_reversed: Vec<[f64; 4]>,
    left_storage: ScopedReservation<'ctx>,
    right_storage: ScopedReservation<'ctx>,
}

impl<'ctx> HomogeneousBezierSplit<'ctx> {
    pub(super) fn into_polygons(
        mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<(ScopedRows<'ctx, [f64; 4]>, ScopedRows<'ctx, [f64; 4]>), ResourceLimit> {
        ctx.charge_work_limit(2, "IR Bezier split final points")?;
        self.left.push(self.point);
        self.right_reversed.push(self.point);
        ctx.charge_work_limit(u64_from_index(self.right_reversed.len()), "IR Bezier split reverse")?;
        self.right_reversed.reverse();
        Ok((ScopedRows::new(self.left, self.left_storage),
            ScopedRows::new(self.right_reversed, self.right_storage)))
    }
}

fn split_homogeneous_bezier<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    controls: &[[f64; 4]],
    parameter: f64,
) -> Result<Option<HomogeneousBezierSplit<'ctx>>, ResourceLimit> {
    if !parameter.is_finite() || !(0.0..=1.0).contains(&parameter) {
        return Ok(None);
    }
    split_homogeneous_bezier_with(ctx, controls, |left, right| {
        std::array::from_fn(|axis| (1.0 - parameter) * left[axis] + parameter * right[axis])
    })
}

pub(super) fn split_homogeneous_bezier_midpoint<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    controls: &[[f64; 4]],
) -> Result<Option<HomogeneousBezierSplit<'ctx>>, ResourceLimit> {
    split_homogeneous_bezier_with(ctx, controls, |left, right| {
        std::array::from_fn(|axis| 0.5 * (left[axis] + right[axis]))
    })
}

fn split_homogeneous_bezier_with<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    controls: &[[f64; 4]],
    blend: impl Fn([f64; 4], [f64; 4]) -> [f64; 4],
) -> Result<Option<HomogeneousBezierSplit<'ctx>>, ResourceLimit> {
    let Some((&first, input_rest)) = controls.split_first() else {
        return Ok(None);
    };
    let mut first = first;
    let _rest_storage;
    let mut rest;
    (rest, _rest_storage) = ctx.copy_temporary_slice(input_rest, "IR Bezier split controls")?;
    let _next_storage;
    let mut next = Vec::new();
    _next_storage = ctx.reserve_temporary_vec(&mut next, rest.len(), "IR Bezier split level")?;
    ctx.charge_work_limit(u64_from_index(rest.len()), "IR Bezier split level fill")?;
    next.resize(rest.len(), [0.0; 4]);
    let left_storage;
    let mut left = Vec::new();
    left_storage = ctx.reserve_temporary_vec(&mut left, controls.len(), "IR Bezier split left")?;
    ctx.charge_work_limit(u64_from_index(rest.len()), "IR Bezier split left fill")?;
    left.resize(rest.len(), [0.0; 4]);
    let right_storage;
    let mut right = Vec::new();
    right_storage = ctx.reserve_temporary_vec(&mut right, controls.len(), "IR Bezier split right")?;
    ctx.charge_work_limit(u64_from_index(rest.len()), "IR Bezier split right fill")?;
    right.resize(rest.len(), [0.0; 4]);
    let mut remaining = rest.len();
    let mut level = 0;
    while remaining > 0 {
        ctx.charge_work_limit(3, "IR Bezier split boundary copies and blend")?;
        let second = rest[0];
        left[level] = first;
        right[level] = rest[remaining - 1];
        first = blend(first, second);
        for index in 0..remaining - 1 {
            ctx.charge_work_limit(1, "IR Bezier split interior blend")?;
            next[index] = blend(rest[index], rest[index + 1]);
        }
        std::mem::swap(&mut rest, &mut next);
        remaining -= 1;
        level += 1;
    }
    Ok(Some(HomogeneousBezierSplit {
        left, point: first, right_reversed: right, left_storage, right_storage,
    }))
}

/// The restricted polygon has one control per admitted input control.
pub(super) fn restrict_homogeneous_bezier<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    controls: &[[f64; 4]],
    start: f64,
    end: f64,
) -> Result<Option<ScopedRows<'ctx, [f64; 4]>>, ResourceLimit> {
    let reverse = start > end;
    let (start, end) = if reverse { (end, start) } else { (start, end) };
    let result = (|| -> Result<Option<ScopedRows<'ctx, [f64; 4]>>, ResourceLimit> {
        if start == end {
            let Some(split) = split_homogeneous_bezier(ctx, controls, start)? else {
                return Ok(None);
            };
            let storage;
            let mut collapsed = Vec::new();
            storage = ctx.reserve_temporary_vec(&mut collapsed, controls.len(), "ir_bezier_collapsed_controls")?;
            ctx.charge_work_limit(u64_from_index(controls.len()), "IR Bezier collapsed control fill")?;
            collapsed.resize(controls.len(), split.point);
            return Ok(Some(ScopedRows::new(collapsed, storage)));
        }
        let Some(split) = split_homogeneous_bezier(ctx, controls, end)? else {
            return Ok(None);
        };
        let (left, _) = split.into_polygons(ctx)?;
        if start == 0.0 {
            return Ok(Some(left));
        }
        let relative_start = start / end;
        let Some(split) = split_homogeneous_bezier(ctx, &left, relative_start)? else {
            return Ok(None);
        };
        Ok(Some(split.into_polygons(ctx)?.1))
    })()?;
    let Some(mut result) = result else { return Ok(None); };
    if reverse { result.reverse(ctx, "IR Bezier restriction reverse")?; }
    Ok(Some(result))
}

pub(super) fn binomial_coefficient(
    ctx: &DecodeContext<'_>,
    degree: usize,
    index: usize,
) -> Result<Option<f64>, ResourceLimit> {
    let index = index.min(degree - index);
    let mut value = 1.0;
    for factor in 1..=index {
        ctx.charge_work_limit(1, "IR Bezier binomial factor")?;
        let Some(numerator) = f64_from_index(degree - index + factor) else { return Ok(None); };
        let Some(denominator) = f64_from_index(factor) else { return Ok(None); };
        value = value * numerator / denominator;
    }
    Ok(Some(value))
}

pub(super) fn point_on_chord(chord: [Point3; 2], parameter: f64) -> Point3 {
    Point3::new(
        chord[0].x + parameter * (chord[1].x - chord[0].x),
        chord[0].y + parameter * (chord[1].y - chord[0].y),
        chord[0].z + parameter * (chord[1].z - chord[0].z),
    )
}

pub(super) fn rational_curve_chord_bound(
    ctx: &DecodeContext<'_>,
    controls: &[[f64; 4]],
    chord: [Point3; 2],
) -> Result<Option<f64>, ResourceLimit> {
    let Some(degree) = controls.len().checked_sub(1) else { return Ok(None); };
    let elevated_degree = degree + 1;
    let mut bound = 0.0_f64;
    let mut coordinate_scale = chord
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .fold(1.0_f64, |scale, coordinate| scale.max(coordinate.abs()));
    for index in 0..=elevated_degree {
        ctx.charge_work_limit(1, "IR rational Bezier chord coefficient")?;
        let previous = index.checked_sub(1).and_then(|index| controls.get(index));
        let current = controls.get(index);
        let (Some(numerator), Some(denominator)) =
            (f64_from_index(index), f64_from_index(elevated_degree)) else { return Ok(None); };
        let previous_factor = numerator / denominator;
        let current_factor = 1.0 - previous_factor;
        let weight = previous_factor * previous.map_or(0.0, |control| control[3])
            + current_factor * current.map_or(0.0, |control| control[3]);
        if !weight.is_finite() || weight <= 0.0 {
            return Ok(None);
        }
        let mut residual_norm = 0.0_f64;
        for (axis, chord_coordinates) in [
            [chord[0].x, chord[1].x],
            [chord[0].y, chord[1].y],
            [chord[0].z, chord[1].z],
        ]
        .into_iter()
        .enumerate()
        {
            let coordinate = previous_factor * previous.map_or(0.0, |control| control[axis])
                + current_factor * current.map_or(0.0, |control| control[axis]);
            let weighted_chord =
                current_factor * current.map_or(0.0, |control| control[3]) * chord_coordinates[0]
                    + previous_factor
                        * previous.map_or(0.0, |control| control[3])
                        * chord_coordinates[1];
            let residual = (coordinate - weighted_chord) / weight;
            if !residual.is_finite() {
                return Ok(None);
            }
            residual_norm = residual_norm.hypot(residual);
            coordinate_scale = coordinate_scale.max((coordinate / weight).abs());
        }
        bound = bound.max(residual_norm);
    }
    let rounding_margin = 256.0 * f64::EPSILON * coordinate_scale.max(bound);
    Ok((bound.is_finite() && rounding_margin.is_finite()).then_some(bound + rounding_margin))
}

#[cfg(test)]
mod tests {
    use super::{restrict_homogeneous_bezier, split_homogeneous_bezier_midpoint};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn homogeneous_split_admits_each_work_pass_and_keeps_polygon_storage() {
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        // One copied row, three filled rows, three boundary copies/blends,
        // two final points and two reversed rows use eleven units.
        for cap in 0..11 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let error = (|| {
                let split = split_homogeneous_bezier_midpoint(&ctx, &controls)?.expect("nonempty polygon");
                split.into_polygons(&ctx)
            })().err().expect("every pass needs work");
            assert_eq!(error.dimension, ResourceDimension::WorkUnits);
            assert_eq!(error.limit, cap);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == error));
        }
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                _ => unreachable!("tested allocation dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let error = split_homogeneous_bezier_midpoint(&ctx, &controls).err().expect("allocation admission");
            assert_eq!(error.dimension, dimension);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == error));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 11;
        policy.limits.max_materialized_bytes = 192;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 6;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let split = split_homogeneous_bezier_midpoint(&ctx, &controls).expect("exact split work").expect("nonempty");
        let (left, right) = split.into_polygons(&ctx).expect("exact completion work");
        assert_eq!(&*left, &[[0.0, 0.0, 0.0, 1.0], [0.5, 0.0, 0.0, 1.0]]);
        assert_eq!(&*right, &[[0.5, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]]);
        let spare = ctx.reserve_scoped_limit(64, "test split working rows released").expect("only two output polygons remain");
        drop(spare);
        drop(left);
        drop(right);
        let reuse = ctx.reserve_scoped_limit(192, "test split polygons released").expect("all bytes reusable");
        drop(reuse);
        ctx.finish_session().expect("temporary polygons retain no bytes");
    }

    #[test]
    fn homogeneous_restriction_preserves_forward_reverse_and_collapsed_polygons() {
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        for (start, end) in [(0.25, 0.75), (0.75, 0.25), (0.5, 0.5), (0.0, 1.0)] {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = restrict_homogeneous_bezier(&ctx, &controls, start, end).expect("admission").expect("valid interval");
            assert_eq!(&*result, &[[start, 0.0, 0.0, 1.0], [end, 0.0, 0.0, 1.0]]);
            drop(result);
            ctx.finish_session().expect("successful restriction");
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = restrict_homogeneous_bezier(&ctx, &controls, start, end).expect_err("restriction visits need caller work");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
    }

    #[test]
    fn bezier_bounds_preserve_factor_and_coefficient_work_refusals() {
        for cap in 0..2 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = super::binomial_coefficient(&ctx, 4, 2).expect_err("two binomial factors");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "IR Bezier binomial factor");
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        let chord = [crate::math::Point3::new(0.0, 0.0, 0.0), crate::math::Point3::new(1.0, 0.0, 0.0)];
        for cap in 0..3 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = super::rational_curve_chord_bound(&ctx, &controls, chord).expect_err("three elevated coefficients");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "IR rational Bezier chord coefficient");
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 5;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(super::binomial_coefficient(&ctx, 4, 2), Ok(Some(6.0)));
        assert_eq!(super::rational_curve_chord_bound(&ctx, &controls, chord), Ok(Some(256.0 * f64::EPSILON)));
        ctx.finish_session().expect("five visits without allocations");
    }

}
