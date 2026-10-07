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
    ) -> Result<[ScopedRows<'ctx, [f64; 4]>; 2], ResourceLimit> {
        ctx.charge_work_limit(2, "IR Bezier split final points")?;
        self.left.push(self.point);
        self.right_reversed.push(self.point);
        ctx.charge_work_limit(
            u64_from_index(self.right_reversed.len() / 2) * 2,
            "IR Bezier split reverse",
        )?;
        self.right_reversed.reverse();
        Ok([
            ScopedRows::new(self.left, self.left_storage),
            ScopedRows::new(self.right_reversed, self.right_storage),
        ])
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
    let (left_storage, mut left) = {
        let mut values = Vec::new();
        let reservation =
            ctx.reserve_temporary_vec(&mut values, controls.len(), "IR Bezier split left")?;
        (reservation, values)
    };
    let (right_storage, mut right) = {
        let mut values = Vec::new();
        let reservation =
            ctx.reserve_temporary_vec(&mut values, controls.len(), "IR Bezier split right")?;
        (reservation, values)
    };
    let mut remaining = rest.len();
    while remaining > 0 {
        ctx.charge_work_limit(3, "IR Bezier split boundary copies and blend")?;
        let second = rest[0];
        left.push(first);
        right.push(rest[remaining - 1]);
        first = blend(first, second);
        for index in ctx.admit_iter(0..remaining - 1, "IR Bezier split interior blend")? {
            rest[index] = blend(rest[index], rest[index + 1]);
        }
        remaining -= 1;
    }
    Ok(Some(HomogeneousBezierSplit {
        left,
        point: first,
        right_reversed: right,
        left_storage,
        right_storage,
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
            let (storage, mut collapsed) = {
                let mut values = Vec::new();
                let reservation = ctx.reserve_temporary_vec(
                    &mut values,
                    controls.len(),
                    "ir_bezier_collapsed_controls",
                )?;
                (reservation, values)
            };
            collapsed.extend(
                ctx.admit_iter(0..controls.len(), "IR Bezier collapsed control fill")?
                    .map(|_| split.point),
            );
            return Ok(Some(ScopedRows::new(collapsed, storage)));
        }
        let Some(split) = split_homogeneous_bezier(ctx, controls, end)? else {
            return Ok(None);
        };
        let [left, _] = split.into_polygons(ctx)?;
        if start == 0.0 {
            return Ok(Some(left));
        }
        let relative_start = start / end;
        let Some(split) = split_homogeneous_bezier(ctx, &left, relative_start)? else {
            return Ok(None);
        };
        let [_, right] = split.into_polygons(ctx)?;
        Ok(Some(right))
    })()?;
    let Some(mut result) = result else {
        return Ok(None);
    };
    if reverse {
        result.reverse(ctx, "IR Bezier restriction reverse")?;
    }
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
        let Some(numerator) = f64_from_index(degree - index + factor) else {
            return Ok(None);
        };
        let Some(denominator) = f64_from_index(factor) else {
            return Ok(None);
        };
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
    let Some(degree) = controls.len().checked_sub(1) else {
        return Ok(None);
    };
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
            (f64_from_index(index), f64_from_index(elevated_degree))
        else {
            return Ok(None);
        };
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
        for operation in [
            "IR Bezier split controls",
            "IR Bezier split boundary copies and blend",
            "IR Bezier split final points",
            "IR Bezier split reverse",
        ] {
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                operation,
                |cap| {
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    let arena = DecodeArena::new();
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    let result = (|| -> Result<(), cadmpeg_core::decode::ResourceLimit> {
                        let split = split_homogeneous_bezier_midpoint(&ctx, &controls)?
                            .expect("nonempty polygon");
                        let polygons = split.into_polygons(&ctx)?;
                        drop(polygons);
                        Ok(())
                    })()
                    .map_err(CodecError::from);
                    let Err(CodecError::ResourceLimit(limit)) = &result else {
                        panic!("each named pass needs work");
                    };
                    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(limit.operation, operation);
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit)
                    );
                    result
                },
            );
        }
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
        ] {
            cadmpeg_test_support::refusal::resource_limit_at(
                dimension,
                "IR Bezier split controls",
                |cap| {
                    let mut policy = DecodePolicy::service();
                    match dimension {
                        ResourceDimension::MaterializedBytes => {
                            policy.limits.max_materialized_bytes = cap;
                        }
                        ResourceDimension::CollectionItems => {
                            policy.limits.max_collection_items = cap;
                        }
                        _ => unreachable!("tested allocation dimensions"),
                    }
                    let arena = DecodeArena::new();
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    let result = split_homogeneous_bezier_midpoint(&ctx, &controls)
                        .map(drop)
                        .map_err(CodecError::from);
                    let Err(CodecError::ResourceLimit(limit)) = &result else {
                        panic!("allocation admission");
                    };
                    assert_eq!(limit.dimension, dimension);
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit)
                    );
                    result
                },
            );
        }
        let mut policy = DecodePolicy::service();
        // One copied row, three boundary copies/blends, two final points and
        // two reversed rows use eight units.
        policy.limits.max_work_units = 8;
        policy.limits.max_materialized_bytes = 192;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 6;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let split = split_homogeneous_bezier_midpoint(&ctx, &controls)
            .expect("exact split work")
            .expect("nonempty");
        let [left, right] = split.into_polygons(&ctx).expect("exact completion work");
        assert_eq!(&*left, &[[0.0, 0.0, 0.0, 1.0], [0.5, 0.0, 0.0, 1.0]]);
        assert_eq!(&*right, &[[0.5, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]]);
        let spare = ctx
            .reserve_scoped_limit(64, "test split working rows released")
            .expect("only two output polygons remain");
        drop(spare);
        drop(left);
        drop(right);
        let reuse = ctx
            .reserve_scoped_limit(192, "test split polygons released")
            .expect("all bytes reusable");
        drop(reuse);
        ctx.finish_session()
            .expect("temporary polygons retain no bytes");
    }

    #[test]
    fn quadratic_split_updates_rows_in_place_and_leaves_the_middle_reverse_row() {
        let controls = [
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [2.0, 0.0, 0.0, 1.0],
        ];
        let run = |cap| {
            let mut policy = DecodePolicy::service();
            // Two copied rows, six boundary copies/blends, one interior blend,
            // two final points and two reversed rows use thirteen units.
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 256;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 8;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = (|| -> Result<(), cadmpeg_core::decode::ResourceLimit> {
                let split = split_homogeneous_bezier_midpoint(&ctx, &controls)?.expect("nonempty");
                let [left, right] = split.into_polygons(&ctx)?;
                assert_eq!(
                    &*left,
                    &[
                        [0.0, 0.0, 0.0, 1.0],
                        [0.5, 0.0, 0.0, 1.0],
                        [1.0, 0.0, 0.0, 1.0]
                    ]
                );
                assert_eq!(
                    &*right,
                    &[
                        [1.0, 0.0, 0.0, 1.0],
                        [1.5, 0.0, 0.0, 1.0],
                        [2.0, 0.0, 0.0, 1.0]
                    ]
                );
                drop(left);
                drop(right);
                Ok(())
            })()
            .map_err(CodecError::from);
            match &result {
                Ok(()) => ctx.finish_session().expect("no retained bytes"),
                Err(CodecError::ResourceLimit(limit)) => {
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit)
                    );
                }
                Err(error) => panic!("unexpected split error: {error}"),
            }
            result
        };
        run(13).expect("exact split work");
        for operation in ["IR Bezier split interior blend", "IR Bezier split reverse"] {
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                operation,
                run,
            );
        }
    }

    #[test]
    fn homogeneous_restriction_preserves_forward_reverse_and_collapsed_polygons() {
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        for (start, end) in [(0.25, 0.75), (0.75, 0.25), (0.5, 0.5), (0.0, 1.0)] {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = restrict_homogeneous_bezier(&ctx, &controls, start, end)
                .expect("admission")
                .expect("valid interval");
            assert_eq!(&*result, &[[start, 0.0, 0.0, 1.0], [end, 0.0, 0.0, 1.0]]);
            drop(result);
            ctx.finish_session().expect("successful restriction");
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = restrict_homogeneous_bezier(&ctx, &controls, start, end)
                .expect_err("restriction visits need caller work");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
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
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        let chord = [
            crate::math::Point3::new(0.0, 0.0, 0.0),
            crate::math::Point3::new(1.0, 0.0, 0.0),
        ];
        for cap in 0..3 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = super::rational_curve_chord_bound(&ctx, &controls, chord)
                .expect_err("three elevated coefficients");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "IR rational Bezier chord coefficient");
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
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
        assert_eq!(
            super::rational_curve_chord_bound(&ctx, &controls, chord),
            Ok(Some(256.0 * f64::EPSILON))
        );
        ctx.finish_session()
            .expect("five visits without allocations");
    }
}
