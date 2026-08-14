//! Helix polyline fitting and the linear solvers it uses.

use super::sketch_edges::{cross, dot};
use cadmpeg_ir::math::{Point3, Vector3};

// Mesh coordinates are an approximation of the analytic helix. This fixed
// relative bound is the decoder's promotion policy, not a value inferred from
// the mesh spacing.
const HELIX_MAX_RELATIVE_RESIDUAL: f64 = 5.0e-4;

pub(crate) fn fit_helix_polyline(
    points: &[Point3],
    revolutions: f64,
    clockwise: bool,
) -> Option<(Point3, Vector3, f64, f64)> {
    if points.len() < 6 || !revolutions.is_finite() || revolutions <= 0.0 {
        return None;
    }
    let mut parameters = Vec::with_capacity(points.len());
    parameters.push(0.0);
    for pair in points.windows(2) {
        let delta = Vector3::new(
            pair[1].x - pair[0].x,
            pair[1].y - pair[0].y,
            pair[1].z - pair[0].z,
        );
        parameters.push(parameters.last().copied()? + dot(delta, delta).sqrt());
    }
    let total = *parameters.last()?;
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    let angle = std::f64::consts::TAU * revolutions * if clockwise { -1.0 } else { 1.0 };
    let mut normal = [[0.0; 4]; 4];
    let mut rhs = [[0.0; 3]; 4];
    for (point, distance) in points.iter().zip(parameters) {
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
    let x = solve_four(normal, rhs)?;
    let cosine = Vector3::new(x[2][0], x[2][1], x[2][2]);
    let sine = Vector3::new(x[3][0], x[3][1], x[3][2]);
    let mut axis = cross(cosine, sine);
    let axis_length = dot(axis, axis).sqrt();
    if !axis_length.is_finite() || axis_length <= 0.0 {
        return None;
    }
    axis = Vector3::new(
        axis.x / axis_length,
        axis.y / axis_length,
        axis.z / axis_length,
    );
    let radial_cosine = subtract_axis(cosine, axis);
    let radial_sine = subtract_axis(sine, axis);
    let radius_estimate =
        (dot(radial_cosine, radial_cosine).sqrt() + dot(radial_sine, radial_sine).sqrt()) * 0.5;
    if !radius_estimate.is_finite() || radius_estimate <= 0.0 {
        return None;
    }
    let mut max_error = 0.0f64;
    for (point, distance) in
        points.iter().zip(
            std::iter::once(0.0).chain(points.windows(2).scan(0.0, |sum, pair| {
                let delta = Vector3::new(
                    pair[1].x - pair[0].x,
                    pair[1].y - pair[0].y,
                    pair[1].z - pair[0].z,
                );
                *sum += dot(delta, delta).sqrt();
                Some(*sum)
            })),
        )
    {
        let t = distance / total;
        let row = [1.0, t, (angle * t).cos(), (angle * t).sin()];
        for (coordinate, actual) in [point.x, point.y, point.z].into_iter().enumerate() {
            let fitted = (0..4).map(|i| row[i] * x[i][coordinate]).sum::<f64>();
            max_error = max_error.max((fitted - actual).abs());
        }
    }
    if max_error > radius_estimate * HELIX_MAX_RELATIVE_RESIDUAL {
        return None;
    }
    let (origin, radius) = fit_circle_on_axis(points, axis)?;
    let displacement = Vector3::new(
        points.last()?.x - points[0].x,
        points.last()?.y - points[0].y,
        points.last()?.z - points[0].z,
    );
    Some((origin, axis, radius, dot(displacement, axis)))
}

fn fit_circle_on_axis(points: &[Point3], axis: Vector3) -> Option<(Point3, f64)> {
    let helper = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if axis.y.abs() <= axis.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let mut u = cross(axis, helper);
    let u_length = dot(u, u).sqrt();
    u = Vector3::new(u.x / u_length, u.y / u_length, u.z / u_length);
    let v = cross(axis, u);
    let reference = points[0];
    let mut normal = [[0.0; 3]; 3];
    let mut rhs = [0.0; 3];
    for point in points {
        let delta = Vector3::new(
            point.x - reference.x,
            point.y - reference.y,
            point.z - reference.z,
        );
        let x = dot(delta, u);
        let y = dot(delta, v);
        let row = [x, y, 1.0];
        let target = -(x * x + y * y);
        for i in 0..3 {
            rhs[i] += row[i] * target;
            for j in 0..3 {
                normal[i][j] += row[i] * row[j];
            }
        }
    }
    let solution = solve_three(normal, rhs)?;
    let center_u = -solution[0] * 0.5;
    let center_v = -solution[1] * 0.5;
    let radius_squared = center_u * center_u + center_v * center_v - solution[2];
    if !radius_squared.is_finite() || radius_squared <= 0.0 {
        return None;
    }
    Some((
        Point3::new(
            reference.x + center_u * u.x + center_v * v.x,
            reference.y + center_u * u.y + center_v * v.y,
            reference.z + center_u * u.z + center_v * v.z,
        ),
        radius_squared.sqrt(),
    ))
}

fn solve_three(mut matrix: [[f64; 3]; 3], mut rhs: [f64; 3]) -> Option<[f64; 3]> {
    for column in 0..3 {
        let pivot = (column..3).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        if matrix[pivot][column].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
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
    Some(rhs)
}

fn subtract_axis(vector: Vector3, axis: Vector3) -> Vector3 {
    let axial = dot(vector, axis);
    Vector3::new(
        vector.x - axial * axis.x,
        vector.y - axial * axis.y,
        vector.z - axial * axis.z,
    )
}

fn solve_four(mut matrix: [[f64; 4]; 4], mut rhs: [[f64; 3]; 4]) -> Option<[[f64; 3]; 4]> {
    for column in 0..4 {
        let pivot = (column..4).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        if matrix[pivot][column].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
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
    Some(rhs)
}

#[cfg(test)]
mod tests {
    #[test]
    fn helix_polyline_fit_recovers_axis_radius_and_rise() {
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
        let (origin, axis, radius, rise) = super::fit_helix_polyline(&points, 0.25, false).unwrap();
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
        let (_, axis, radius, _) = super::fit_helix_polyline(&points, 0.25, false).unwrap();
        assert!(axis.x > 3.0e-5 && axis.x < 5.0e-5, "{axis:?}");
        assert!(axis.y < -0.999_999_99, "{axis:?}");
        assert!(axis.z.abs() < 1.0e-6, "{axis:?}");
        assert!((radius - 3.5).abs() < 1.0e-5);
    }
}
