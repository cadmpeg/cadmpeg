//! Helix polyline fitting and the linear solvers it uses.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{power_of_two_bound, scale_power_of_two, Point3, Vector3};

// Mesh coordinates are an approximation of the analytic helix. This fixed
// relative bound is the decoder's promotion policy, not a value inferred from
// the mesh spacing.
const HELIX_MAX_RELATIVE_RESIDUAL: f64 = 5.0e-4;

pub(super) fn fit_helix_polyline(
    ctx: &DecodeContext<'_>,
    points: &[Point3],
    revolutions: cadmpeg_ir::scalar::PositiveReal,
    clockwise: bool,
) -> Result<Option<(Point3, Vector3, f64, f64)>, CodecError> {
    if points.len() < 6 {
        return Ok(None);
    }
    let revolutions = revolutions.get();
    let mut parameters = ctx.alloc_filled(points.len(), 0.0, "fit SLDPRT helix parameters")?;
    for (index, point) in ctx
        .admit_iter(&points[1..], "fit SLDPRT helix work")?
        .enumerate()
    {
        let previous = points[index];
        let delta = Vector3::new(
            point.x - previous.x,
            point.y - previous.y,
            point.z - previous.z,
        );
        parameters[index + 1] = parameters[index] + delta.norm();
    }
    (|| -> Result<Option<(Point3, Vector3, f64, f64)>, CodecError> {
        let Some(total) = parameters.last().copied() else {
            return Ok(None);
        };
        if !total.is_finite() || total <= 0.0 {
            return Ok(None);
        }
        let angle = std::f64::consts::TAU * revolutions * if clockwise { -1.0 } else { 1.0 };
        let mut normal = [[0.0; 4]; 4];
        let mut rhs = [[0.0; 3]; 4];
        for (point, distance) in ctx
            .admit_iter(points, "fit SLDPRT helix normal equations")?
            .zip(ctx.admit_iter(&parameters, "fit SLDPRT helix normal equations")?.copied())
        {
            let t = distance / total;
            let row = [1.0, t, (angle * t).cos(), (angle * t).sin()];
            for i in 0..4 {
                for j in 0..4 {
                    normal[i][j] += row[i] * row[j];
                }
                rhs[i][0] += row[i] * point.x;
                rhs[i][1] += row[i] * point.y;
                rhs[i][2] += row[i] * point.z;
            }
        }
        let Some(x) = solve_four(ctx, normal, rhs)? else {
            return Ok(None);
        };
        let cosine = Vector3::new(x[2][0], x[2][1], x[2][2]);
        let sine = Vector3::new(x[3][0], x[3][1], x[3][2]);
        let mut axis = cosine.cross(sine);
        let axis_length = axis.norm();
        if !axis_length.is_finite() || axis_length <= 0.0 {
            return Ok(None);
        }
        axis = Vector3::new(
            axis.x / axis_length,
            axis.y / axis_length,
            axis.z / axis_length,
        );
        let radial_cosine = subtract_axis(cosine, axis);
        let radial_sine = subtract_axis(sine, axis);
        let radius_estimate = (radial_cosine.norm() + radial_sine.norm()) * 0.5;
        if !radius_estimate.is_finite() || radius_estimate <= 0.0 {
            return Ok(None);
        }
        let mut max_error = 0.0f64;
        for (point, distance) in ctx
            .admit_iter(points, "fit SLDPRT helix residual")?
            .zip(ctx.admit_iter(&parameters, "fit SLDPRT helix residual")?.copied())
        {
            let t = distance / total;
            let row = [1.0, t, (angle * t).cos(), (angle * t).sin()];
            for (coordinate, actual) in [point.x, point.y, point.z].into_iter().enumerate() {
                let fitted = (0..4).map(|i| row[i] * x[i][coordinate]).sum::<f64>();
                max_error = max_error.max((fitted - actual).abs());
            }
        }
        if max_error > radius_estimate * HELIX_MAX_RELATIVE_RESIDUAL {
            return Ok(None);
        }
        let Some((origin, radius)) = fit_circle_on_axis(ctx, points, axis)? else {
            return Ok(None);
        };
        let Some(last) = points.last() else {
            return Ok(None);
        };
        let displacement = Vector3::new(
            last.x - points[0].x,
            last.y - points[0].y,
            last.z - points[0].z,
        );
        Ok(Some((origin, axis, radius, displacement.dot(axis))))
    })()
}

fn fit_circle_on_axis(
    ctx: &DecodeContext<'_>,
    points: &[Point3],
    axis: Vector3,
) -> Result<Option<(Point3, f64)>, CodecError> {
    let helper = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if axis.y.abs() <= axis.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let mut u = axis.cross(helper);
    let u_length = u.norm();
    u = Vector3::new(u.x / u_length, u.y / u_length, u.z / u_length);
    let v = axis.cross(u);
    let reference = points[0];
    let extent = ctx
        .admit_iter(points, "fit SLDPRT circle extent")?
        .map(|point| point.vector_from(reference).norm())
        .fold(0.0_f64, f64::max);
    // The normal equations mix the quadratic terms of the row with a constant
    // one, so the fitted coordinates are normalised before the solve. The scale
    // is the binade bound of the largest offset: it states an exponent alone, so
    // every significand reaches the solve unchanged and the normalisation cannot
    // move the answer. `power_of_two_bound` answers `None` for a zero or
    // non-finite extent, which is the degenerate span this refuses.
    let Some(exponent) = power_of_two_bound(extent) else {
        return Ok(None);
    };
    let mut normal = [[0.0; 3]; 3];
    let mut rhs = [0.0; 3];
    for point in ctx.admit_iter(points, "fit SLDPRT circle normal equations")? {
        let delta = Vector3::new(
            point.x - reference.x,
            point.y - reference.y,
            point.z - reference.z,
        );
        let Some(x) = scale_power_of_two(delta.dot(u), -exponent) else {
            return Ok(None);
        };
        let Some(y) = scale_power_of_two(delta.dot(v), -exponent) else {
            return Ok(None);
        };
        let x = x.get();
        let y = y.get();
        let row = [x, y, 1.0];
        let target = -(x * x + y * y);
        for i in 0..3 {
            rhs[i] += row[i] * target;
            for j in 0..3 {
                normal[i][j] += row[i] * row[j];
            }
        }
    }
    let Some(solution) = solve_three(ctx, normal, rhs)? else {
        return Ok(None);
    };
    let center_u = -solution[0] * 0.5;
    let center_v = -solution[1] * 0.5;
    let radius_squared = center_u * center_u + center_v * center_v - solution[2];
    if !radius_squared.is_finite() || radius_squared <= 0.0 {
        return Ok(None);
    }
    let Some(center_u) = scale_power_of_two(center_u, exponent) else {
        return Ok(None);
    };
    let Some(center_v) = scale_power_of_two(center_v, exponent) else {
        return Ok(None);
    };
    let Some(radius) = scale_power_of_two(radius_squared.sqrt(), exponent) else {
        return Ok(None);
    };
    let center_u = center_u.get();
    let center_v = center_v.get();
    let radius = radius.get();
    let origin = Point3::new(
        reference.x + center_u * u.x + center_v * v.x,
        reference.y + center_u * u.y + center_v * v.y,
        reference.z + center_u * u.z + center_v * v.z,
    );
    Ok(origin.is_finite().then_some((origin, radius)))
}

fn solve_three(
    ctx: &DecodeContext<'_>,
    mut matrix: [[f64; 3]; 3],
    mut rhs: [f64; 3],
) -> Result<Option<[f64; 3]>, CodecError> {
    for column in 0usize..3 {
        let Some(pivot) = ctx
            .admit_iter(&(column..3), "fit SLDPRT circle pivot search")?
            .max_by(|left, right| {
                matrix[*left][column]
                    .abs()
                    .total_cmp(&matrix[*right][column].abs())
            })
        else {
            return Ok(None);
        };
        if matrix[pivot][column].abs() <= 1.0e-14 {
            return Ok(None);
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for value_index in ctx.admit_iter(
            &(column..matrix[column].len()),
            "fit SLDPRT circle pivot row",
        )? {
            matrix[column][value_index] /= scale;
        }
        rhs[column] /= scale;
        for row in 0..3 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            let pivot_row = matrix[column];
            for (target, pivot) in matrix[row].iter_mut().zip(pivot_row).skip(column) {
                *target -= factor * pivot;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    Ok(Some(rhs))
}

fn subtract_axis(vector: Vector3, axis: Vector3) -> Vector3 {
    let axial = vector.dot(axis);
    Vector3::new(
        vector.x - axial * axis.x,
        vector.y - axial * axis.y,
        vector.z - axial * axis.z,
    )
}

fn solve_four(
    ctx: &DecodeContext<'_>,
    mut matrix: [[f64; 4]; 4],
    mut rhs: [[f64; 3]; 4],
) -> Result<Option<[[f64; 3]; 4]>, CodecError> {
    for column in 0usize..4 {
        let Some(pivot) = ctx
            .admit_iter(&(column..4), "fit SLDPRT helix pivot search")?
            .max_by(|left, right| {
                matrix[*left][column]
                    .abs()
                    .total_cmp(&matrix[*right][column].abs())
            })
        else {
            return Ok(None);
        };
        if matrix[pivot][column].abs() <= 1.0e-14 {
            return Ok(None);
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for value_index in ctx.admit_iter(
            &(column..matrix[column].len()),
            "fit SLDPRT helix pivot row",
        )? {
            matrix[column][value_index] /= scale;
        }
        for value in &mut rhs[column] {
            *value /= scale;
        }
        for row in 0..4 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            let pivot_row = matrix[column];
            for (target, pivot) in matrix[row].iter_mut().zip(pivot_row).skip(column) {
                *target -= factor * pivot;
            }
            let rhs_pivot = rhs[column];
            for (target, pivot) in rhs[row].iter_mut().zip(rhs_pivot) {
                *target -= factor * pivot;
            }
        }
    }
    Ok(Some(rhs))
}

#[cfg(test)]
mod tests {
    fn revolutions(value: f64) -> cadmpeg_ir::scalar::PositiveReal {
        cadmpeg_ir::scalar::PositiveReal::new(value).expect("positive revolutions")
    }

    #[test]
    fn helix_fit_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let points = [cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 6];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 5;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let error = super::fit_helix_polyline(&ctx, &points, revolutions(1.0), false)
            .expect_err("parameter lane exceeds collection limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "fit SLDPRT helix parameters"
        ));
    }

    #[test]
    fn helix_fit_refuses_work_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let points = [cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 6];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Six parameter-fill visits precede the five admitted segment visits.
        policy.limits.max_work_units = 10;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let error = super::fit_helix_polyline(&ctx, &points, revolutions(1.0), false)
            .expect_err("fit exceeds work limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "fit SLDPRT helix work"
        ));
    }

    // Four points of the exact circle of radius 5 about `(-5, 0, 0)` in the
    // plane normal to `z`. Every coordinate, the centre and the radius are
    // representable in f64, and the frame the fit builds for this axis is the
    // exact pair `u = (0, 1, 0)`, `v = (-1, 0, 0)`, so the answer is reachable
    // bit for bit. The largest offset from `points[0]` is `sqrt(50)`: a
    // normalisation by that value divides every fitted coordinate by an
    // irrational scale and loses the last bit of each one before the solve.
    #[test]
    fn circle_fit_normalisation_keeps_an_exact_centre_and_radius() {
        use cadmpeg_ir::math::{Point3, Vector3};
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(-5.0, -5.0, 0.0),
            Point3::new(-2.0, -4.0, 0.0),
            Point3::new(-1.0, 3.0, 0.0),
        ];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let (origin, radius) =
            super::fit_circle_on_axis(&ctx, &points, Vector3::new(0.0, 0.0, 1.0))
                .unwrap()
                .unwrap();
        assert_eq!(origin.x, -5.0);
        assert_eq!(origin.y, 0.0);
        assert_eq!(origin.z, 0.0);
        assert_eq!(radius, 5.0);
    }

    #[test]
    fn helix_fit_preserves_small_model_units() {
        const RELATIVE_ERROR: f64 = 1e-10;

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        for scale in [1e-8, 1.0, 1e8] {
            let points = (0..=16)
                .map(|index| {
                    let t = f64::from(index) / 16.0;
                    let angle = std::f64::consts::TAU * t;
                    cadmpeg_ir::math::Point3::new(
                        scale * angle.cos(),
                        scale * angle.sin(),
                        scale * t,
                    )
                })
                .collect::<Vec<_>>();
            let (origin, axis, radius, rise) =
                super::fit_helix_polyline(&ctx, &points, revolutions(1.0), false)
                    .unwrap()
                    .unwrap();
            assert!(origin.x.abs() / scale <= RELATIVE_ERROR);
            assert!(origin.y.abs() / scale <= RELATIVE_ERROR);
            assert!((axis.z - 1.0).abs() <= RELATIVE_ERROR);
            assert!((radius / scale - 1.0).abs() <= RELATIVE_ERROR);
            assert!((rise / scale - 1.0).abs() <= RELATIVE_ERROR);
        }
    }

    #[test]
    fn helix_polyline_fit_recovers_axis_radius_and_rise() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let points = (0..=64)
            .map(|index| {
                let t = f64::from(index) / 64.0;
                let angle = std::f64::consts::FRAC_PI_2 * t;
                cadmpeg_ir::math::Point3::new(
                    10.0 + 3.5 * angle.cos(),
                    20.0 - 3.2 * t,
                    30.0 + 3.5 * angle.sin(),
                )
            })
            .collect::<Vec<_>>();
        let (origin, axis, radius, rise) =
            super::fit_helix_polyline(&ctx, &points, revolutions(0.25), false)
                .unwrap()
                .unwrap();
        assert!((origin.x - 10.0).abs() < 1.0e-9);
        assert!((origin.y - 20.0).abs() < 1.0e-9);
        assert!((origin.z - 30.0).abs() < 1.0e-9);
        assert!(axis.x.abs() < 1.0e-9);
        assert!((axis.y + 1.0).abs() < 1.0e-12);
        assert!(axis.z.abs() < 1.0e-9);
        assert!((radius - 3.5).abs() < 1.0e-9);
        assert!((rise - 3.2).abs() < 1.0e-9);
    }

    #[test]
    fn helix_fit_does_not_snap_axis_to_mesh_residual() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let axis_x: f64 = 4.0e-5;
        let axis_y = -(1.0 - axis_x * axis_x).sqrt();
        let points = (0..=64)
            .map(|index| {
                let t = f64::from(index) / 64.0;
                let angle = std::f64::consts::FRAC_PI_2 * t;
                let mut point = cadmpeg_ir::math::Point3::new(
                    10.0 + axis_x * 3.2 * t - 3.5 * axis_y.abs() * angle.sin(),
                    20.0 + axis_y * 3.2 * t - 3.5 * axis_x * angle.sin(),
                    30.0 + 3.5 * angle.cos(),
                );
                if index == 32 {
                    point.x += 2.0e-5;
                }
                point
            })
            .collect::<Vec<_>>();
        let (_, axis, radius, _) =
            super::fit_helix_polyline(&ctx, &points, revolutions(0.25), false)
                .unwrap()
                .unwrap();
        assert!(axis.x > 3.0e-5 && axis.x < 5.0e-5, "{axis:?}");
        assert!(axis.y < -0.999_999_99, "{axis:?}");
        assert!(axis.z.abs() < 1.0e-6, "{axis:?}");
        assert!((radius - 3.5).abs() < 1.0e-5);
    }
}
