// SPDX-License-Identifier: Apache-2.0
//! Analytic carriers, plane reconciliation, vertices, edge parameters, and pcurves.

#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(super) struct PlaneEquation {
    pub(super) origin: [f64; 3],
    pub(super) normal: [f64; 3],
}

#[derive(Clone, Copy)]
pub(super) struct CylinderEquation {
    pub(super) origin: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    pub(super) radius: f64,
}

#[derive(Clone, Copy)]
pub(super) struct ConeEquation {
    pub(super) origin: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    pub(super) radius: f64,
    pub(super) ratio: f64,
    pub(super) half_angle: f64,
}

pub(super) fn circular_cone(cone: ConeEquation) -> bool {
    cone.ratio.is_finite() && (cone.ratio - 1.0).abs() <= 1e-12
}

#[derive(Clone, Copy)]
pub(super) struct SphereEquation {
    pub(super) center: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    pub(super) radius: f64,
}

#[derive(Clone, Copy)]
pub(super) struct TorusEquation {
    pub(super) center: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    pub(super) major_radius: f64,
    pub(super) minor_radius: f64,
}

#[derive(Clone, Copy)]
pub(super) enum CarrierEquation {
    Plane(PlaneEquation),
    Cylinder(CylinderEquation),
    Cone(ConeEquation),
    Sphere(SphereEquation),
    Torus(TorusEquation),
}

#[derive(Clone, Copy)]
pub(super) struct QuadricEquation {
    pub(super) matrix: [[f64; 3]; 3],
    pub(super) linear: [f64; 3],
    pub(super) constant: f64,
}

#[derive(Clone, Copy)]
pub(super) struct PlaneConicEquation {
    pub(super) uu: f64,
    pub(super) uv: f64,
    pub(super) vv: f64,
    pub(super) u: f64,
    pub(super) v: f64,
    pub(super) constant: f64,
}

pub(super) fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1].mul_add(right[2], -(left[2] * right[1])),
        left[2].mul_add(right[0], -(left[0] * right[2])),
        left[0].mul_add(right[1], -(left[1] * right[0])),
    ]
}

pub(super) fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0].mul_add(right[0], left[1].mul_add(right[1], left[2] * right[2]))
}

pub(super) fn matrix_vector(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| dot(row, vector))
}

pub(super) fn outer_product(left: [f64; 3], right: [f64; 3]) -> [[f64; 3]; 3] {
    left.map(|left| right.map(|right| left * right))
}

pub(super) fn carrier_quadric(carrier: CarrierEquation) -> Option<QuadricEquation> {
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    match carrier {
        CarrierEquation::Cylinder(cylinder) => {
            let axis = normalized(cylinder.axis)?;
            if !cylinder.radius.is_finite() || cylinder.radius <= 0.0 {
                return None;
            }
            let axis_projection = outer_product(axis, axis);
            let matrix = std::array::from_fn(|row| {
                std::array::from_fn(|column| identity[row][column] - axis_projection[row][column])
            });
            let matrix_origin = matrix_vector(matrix, cylinder.origin);
            Some(QuadricEquation {
                matrix,
                linear: matrix_origin.map(|value| -2.0 * value),
                constant: dot(cylinder.origin, matrix_origin) - cylinder.radius * cylinder.radius,
            })
        }
        CarrierEquation::Cone(cone) => {
            let axis = normalized(cone.axis)?;
            let x_axis = normalized(cone.ref_direction)?;
            if dot(axis, x_axis).abs() > 1e-10
                || !cone.ratio.is_finite()
                || cone.ratio <= 0.0
                || !cone.radius.is_finite()
                || !(0.0..std::f64::consts::FRAC_PI_2).contains(&cone.half_angle)
            {
                return None;
            }
            let y_axis = cross(axis, x_axis);
            let slope = cone.half_angle.tan();
            let x_projection = outer_product(x_axis, x_axis);
            let y_projection = outer_product(y_axis, y_axis);
            let axis_projection = outer_product(axis, axis);
            let ratio_squared = cone.ratio * cone.ratio;
            let matrix = std::array::from_fn(|row| {
                std::array::from_fn(|column| {
                    x_projection[row][column] + y_projection[row][column] / ratio_squared
                        - slope * slope * axis_projection[row][column]
                })
            });
            let matrix_origin = matrix_vector(matrix, cone.origin);
            let radius_slope = cone.radius * slope;
            Some(QuadricEquation {
                matrix,
                linear: std::array::from_fn(|index| {
                    -2.0 * matrix_origin[index] - 2.0 * radius_slope * axis[index]
                }),
                constant: dot(cone.origin, matrix_origin)
                    + 2.0 * radius_slope * dot(axis, cone.origin)
                    - cone.radius * cone.radius,
            })
        }
        CarrierEquation::Sphere(sphere) => {
            if !sphere.radius.is_finite() || sphere.radius <= 0.0 {
                return None;
            }
            Some(QuadricEquation {
                matrix: identity,
                linear: sphere.center.map(|value| -2.0 * value),
                constant: dot(sphere.center, sphere.center) - sphere.radius * sphere.radius,
            })
        }
        CarrierEquation::Plane(_) | CarrierEquation::Torus(_) => None,
    }
}

pub(super) fn restrict_quadric_to_plane(
    quadric: QuadricEquation,
    origin: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
) -> PlaneConicEquation {
    let matrix_origin = matrix_vector(quadric.matrix, origin);
    let matrix_u = matrix_vector(quadric.matrix, u_axis);
    let matrix_v = matrix_vector(quadric.matrix, v_axis);
    PlaneConicEquation {
        uu: dot(u_axis, matrix_u),
        uv: 2.0 * dot(u_axis, matrix_v),
        vv: dot(v_axis, matrix_v),
        u: 2.0 * dot(u_axis, matrix_origin) + dot(quadric.linear, u_axis),
        v: 2.0 * dot(v_axis, matrix_origin) + dot(quadric.linear, v_axis),
        constant: dot(origin, matrix_origin) + dot(quadric.linear, origin) + quadric.constant,
    }
}

pub(super) fn solve_planes(planes: &[PlaneEquation]) -> Option<[f64; 3]> {
    for first in 0..planes.len() {
        for second in first + 1..planes.len() {
            for third in second + 1..planes.len() {
                let a = planes[first];
                let b = planes[second];
                let c = planes[third];
                let b_cross_c = cross(b.normal, c.normal);
                let determinant = dot(a.normal, b_cross_c);
                if determinant.abs() <= 1e-9 {
                    continue;
                }
                let distances = [
                    dot(a.normal, a.origin),
                    dot(b.normal, b.origin),
                    dot(c.normal, c.origin),
                ];
                let c_cross_a = cross(c.normal, a.normal);
                let a_cross_b = cross(a.normal, b.normal);
                let point = [0, 1, 2].map(|axis| {
                    (distances[0] * b_cross_c[axis]
                        + distances[1] * c_cross_a[axis]
                        + distances[2] * a_cross_b[axis])
                        / determinant
                });
                if point.iter().all(|value| value.is_finite())
                    && planes.iter().all(|plane| {
                        (dot(plane.normal, point) - dot(plane.normal, plane.origin)).abs() <= 1e-6
                    })
                {
                    return Some(point);
                }
            }
        }
    }
    None
}

pub(super) fn plane_intersection_line(
    first: PlaneEquation,
    second: PlaneEquation,
) -> Option<([f64; 3], [f64; 3])> {
    let direction = cross(first.normal, second.normal);
    let denominator = dot(direction, direction);
    if denominator <= 1e-18 {
        return None;
    }
    let first_distance = dot(first.normal, first.origin);
    let second_distance = dot(second.normal, second.origin);
    let second_cross_direction = cross(second.normal, direction);
    let direction_cross_first = cross(direction, first.normal);
    let origin = std::array::from_fn(|index| {
        (first_distance * second_cross_direction[index]
            + second_distance * direction_cross_first[index])
            / denominator
    });
    Some((origin, normalized(direction)?))
}

pub(super) fn intersect_two_planes_with_quadric(
    first: PlaneEquation,
    second: PlaneEquation,
    carrier: CarrierEquation,
) -> Vec<[f64; 3]> {
    let Some((line_origin, direction)) = plane_intersection_line(first, second) else {
        return Vec::new();
    };
    let Some(quadric) = carrier_quadric(carrier) else {
        return Vec::new();
    };
    let matrix_origin = matrix_vector(quadric.matrix, line_origin);
    let matrix_direction = matrix_vector(quadric.matrix, direction);
    let quadratic = dot(direction, matrix_direction);
    let linear = 2.0 * dot(line_origin, matrix_direction) + dot(quadric.linear, direction);
    let constant =
        dot(line_origin, matrix_origin) + dot(quadric.linear, line_origin) + quadric.constant;
    quadratic_real_roots(quadratic, linear, constant)
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| line_origin[index] + parameter * direction[index])
        })
        .filter(|point| {
            point.iter().all(|value| value.is_finite())
                && point_on_carrier(*point, CarrierEquation::Plane(first))
                && point_on_carrier(*point, CarrierEquation::Plane(second))
                && point_on_carrier(*point, carrier)
        })
        .collect()
}

pub(super) fn polynomial_value(coefficients: &[f64], parameter: f64) -> f64 {
    coefficients.iter().rev().fold(0.0, |value, coefficient| {
        value.mul_add(parameter, *coefficient)
    })
}

pub(super) fn real_polynomial_roots(coefficients: &[f64]) -> Vec<f64> {
    let scale = coefficients
        .iter()
        .copied()
        .map(f64::abs)
        .fold(0.0, f64::max);
    if scale == 0.0 || !scale.is_finite() {
        return Vec::new();
    }
    let mut coefficients = coefficients
        .iter()
        .map(|coefficient| coefficient / scale)
        .collect::<Vec<_>>();
    while coefficients.len() > 1
        && coefficients
            .last()
            .is_some_and(|value| value.abs() <= 1e-14)
    {
        coefficients.pop();
    }
    let degree = coefficients.len() - 1;
    if degree == 0 {
        return Vec::new();
    }
    if degree == 1 {
        return vec![-coefficients[0] / coefficients[1]];
    }
    let derivative = coefficients
        .iter()
        .enumerate()
        .skip(1)
        .map(|(power, coefficient)| *coefficient * power as f64)
        .collect::<Vec<_>>();
    let leading = coefficients[degree].abs();
    let bound = 1.0
        + coefficients[..degree]
            .iter()
            .copied()
            .map(f64::abs)
            .fold(0.0, f64::max)
            / leading;
    let mut boundaries = vec![-bound];
    boundaries.extend(
        real_polynomial_roots(&derivative)
            .into_iter()
            .filter(|root| root.is_finite() && *root > -bound && *root < bound),
    );
    boundaries.push(bound);
    boundaries.sort_by(f64::total_cmp);
    let value_tolerance = 1e-11;
    let mut roots = boundaries
        .iter()
        .copied()
        .filter(|parameter| polynomial_value(&coefficients, *parameter).abs() <= value_tolerance)
        .collect::<Vec<_>>();
    for interval in boundaries.windows(2) {
        let (mut lower, mut upper) = (interval[0], interval[1]);
        let mut lower_value = polynomial_value(&coefficients, lower);
        let upper_value = polynomial_value(&coefficients, upper);
        if lower_value * upper_value >= 0.0 {
            continue;
        }
        for _ in 0..80 {
            let midpoint = 0.5 * (lower + upper);
            let midpoint_value = polynomial_value(&coefficients, midpoint);
            if lower_value * midpoint_value <= 0.0 {
                upper = midpoint;
            } else {
                lower = midpoint;
                lower_value = midpoint_value;
            }
        }
        roots.push(0.5 * (lower + upper));
    }
    roots.sort_by(f64::total_cmp);
    roots
        .into_iter()
        .fold(Vec::<f64>::new(), |mut unique, root| {
            if let Some(previous) = unique.last_mut() {
                let tolerance = 1e-7 * previous.abs().max(root.abs()).max(1.0);
                if (*previous - root).abs() <= tolerance {
                    if polynomial_value(&coefficients, root).abs()
                        < polynomial_value(&coefficients, *previous).abs()
                    {
                        *previous = root;
                    }
                    return unique;
                }
            }
            unique.push(root);
            unique
        })
}

pub(super) fn polynomial_product(first: &[f64], second: &[f64]) -> Vec<f64> {
    let mut product = vec![0.0; first.len() + second.len() - 1];
    for (first_power, first_coefficient) in first.iter().enumerate() {
        for (second_power, second_coefficient) in second.iter().enumerate() {
            product[first_power + second_power] += first_coefficient * second_coefficient;
        }
    }
    product
}

pub(super) const QUARTIC_RESULTANT_PERMUTATIONS: [([usize; 4], f64); 24] = [
    ([0, 1, 2, 3], 1.0),
    ([0, 1, 3, 2], -1.0),
    ([0, 2, 1, 3], -1.0),
    ([0, 2, 3, 1], 1.0),
    ([0, 3, 1, 2], 1.0),
    ([0, 3, 2, 1], -1.0),
    ([1, 0, 2, 3], -1.0),
    ([1, 0, 3, 2], 1.0),
    ([1, 2, 0, 3], 1.0),
    ([1, 2, 3, 0], -1.0),
    ([1, 3, 0, 2], -1.0),
    ([1, 3, 2, 0], 1.0),
    ([2, 0, 1, 3], 1.0),
    ([2, 0, 3, 1], -1.0),
    ([2, 1, 0, 3], -1.0),
    ([2, 1, 3, 0], 1.0),
    ([2, 3, 0, 1], 1.0),
    ([2, 3, 1, 0], -1.0),
    ([3, 0, 1, 2], -1.0),
    ([3, 0, 2, 1], 1.0),
    ([3, 1, 0, 2], 1.0),
    ([3, 1, 2, 0], -1.0),
    ([3, 2, 0, 1], -1.0),
    ([3, 2, 1, 0], 1.0),
];

pub(super) fn conic_resultant(first: PlaneConicEquation, second: PlaneConicEquation) -> Vec<f64> {
    let zero = vec![0.0];
    let first_y2 = vec![first.vv];
    let first_y = vec![first.v, first.uv];
    let first_constant = vec![first.constant, first.u, first.uu];
    let second_y2 = vec![second.vv];
    let second_y = vec![second.v, second.uv];
    let second_constant = vec![second.constant, second.u, second.uu];
    let matrix = [
        [
            first_y2.clone(),
            first_y.clone(),
            first_constant.clone(),
            zero.clone(),
        ],
        [zero.clone(), first_y2, first_y, first_constant],
        [
            second_y2.clone(),
            second_y.clone(),
            second_constant.clone(),
            zero.clone(),
        ],
        [zero, second_y2, second_y, second_constant],
    ];
    let mut determinant = vec![0.0; 9];
    for (permutation, sign) in QUARTIC_RESULTANT_PERMUTATIONS {
        let term = (0..4).fold(vec![1.0], |term, row| {
            polynomial_product(&term, &matrix[row][permutation[row]])
        });
        for (power, coefficient) in term.into_iter().enumerate() {
            determinant[power] += sign * coefficient;
        }
    }
    determinant
}

pub(super) fn quadratic_real_roots(quadratic: f64, linear: f64, constant: f64) -> Vec<f64> {
    let scale = quadratic
        .abs()
        .max(linear.abs())
        .max(constant.abs())
        .max(1.0);
    if quadratic.abs() <= 1e-14 * scale {
        return if linear.abs() > 1e-14 * scale {
            vec![-constant / linear]
        } else {
            Vec::new()
        };
    }
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    if discriminant < -1e-12 * scale * scale {
        return Vec::new();
    }
    let root = if discriminant.abs() <= 1e-12 * scale * scale {
        0.0
    } else {
        discriminant.sqrt()
    };
    let mut roots = vec![(-linear - root) / (2.0 * quadratic)];
    if root > 1e-12 * scale {
        roots.push((-linear + root) / (2.0 * quadratic));
    }
    roots
}

pub(super) fn plane_conic_value(conic: PlaneConicEquation, u: f64, v: f64) -> f64 {
    conic.uu * u * u
        + conic.uv * u * v
        + conic.vv * v * v
        + conic.u * u
        + conic.v * v
        + conic.constant
}

pub(super) fn refine_plane_conic_intersection(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
    mut u: f64,
    mut v: f64,
) -> [f64; 2] {
    for _ in 0..12 {
        let first_value = plane_conic_value(first, u, v);
        let second_value = plane_conic_value(second, u, v);
        let first_u = 2.0 * first.uu * u + first.uv * v + first.u;
        let first_v = first.uv * u + 2.0 * first.vv * v + first.v;
        let second_u = 2.0 * second.uu * u + second.uv * v + second.u;
        let second_v = second.uv * u + 2.0 * second.vv * v + second.v;
        let determinant = first_u.mul_add(second_v, -(first_v * second_u));
        let scale = first_u
            .abs()
            .max(first_v.abs())
            .max(second_u.abs())
            .max(second_v.abs())
            .max(1.0);
        if determinant.abs() <= 1e-14 * scale * scale {
            break;
        }
        let delta_u = (-first_value).mul_add(second_v, first_v * second_value) / determinant;
        let delta_v = first_value.mul_add(second_u, -(first_u * second_value)) / determinant;
        u += delta_u;
        v += delta_v;
        if delta_u.abs().max(delta_v.abs()) <= 1e-13 * u.abs().max(v.abs()).max(1.0) {
            break;
        }
    }
    [u, v]
}

pub(super) fn common_plane_conic_parameters(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
) -> Vec<[f64; 2]> {
    let resultant = conic_resultant(first, second);
    let mut parameters = Vec::<[f64; 2]>::new();
    for u in real_polynomial_roots(&resultant) {
        let first_v_roots = quadratic_real_roots(
            first.vv,
            first.uv.mul_add(u, first.v),
            first.uu * u * u + first.u * u + first.constant,
        );
        let second_v_roots = quadratic_real_roots(
            second.vv,
            second.uv.mul_add(u, second.v),
            second.uu * u * u + second.u * u + second.constant,
        );
        for v in first_v_roots.into_iter().chain(second_v_roots) {
            let candidate = refine_plane_conic_intersection(first, second, u, v);
            let scale = candidate[0].abs().max(candidate[1].abs()).max(1.0);
            let coefficient_scale = [
                first.uu,
                first.uv,
                first.vv,
                first.u,
                first.v,
                first.constant,
                second.uu,
                second.uv,
                second.vv,
                second.u,
                second.v,
                second.constant,
            ]
            .into_iter()
            .map(f64::abs)
            .fold(1.0, f64::max);
            let tolerance = 1e-8 * coefficient_scale * scale * scale;
            if plane_conic_value(first, candidate[0], candidate[1]).abs() <= tolerance
                && plane_conic_value(second, candidate[0], candidate[1]).abs() <= tolerance
                && !parameters.iter().any(|known| {
                    (known[0] - candidate[0])
                        .abs()
                        .max((known[1] - candidate[1]).abs())
                        <= 1e-7 * scale
                })
            {
                parameters.push(candidate);
            }
        }
    }
    parameters
}

pub(super) fn intersect_plane_with_two_quadrics(
    plane: PlaneEquation,
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<[f64; 3]> {
    let Some(normal) = normalized(plane.normal) else {
        return Vec::new();
    };
    let reference = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        .into_iter()
        .min_by(|left, right| {
            dot(normal, *left)
                .abs()
                .total_cmp(&dot(normal, *right).abs())
        })
        .expect("three reference axes");
    let Some(u_axis) = normalized(cross(normal, reference)) else {
        return Vec::new();
    };
    let v_axis = cross(normal, u_axis);
    let Some(first_quadric) = carrier_quadric(first) else {
        return Vec::new();
    };
    let Some(second_quadric) = carrier_quadric(second) else {
        return Vec::new();
    };
    let first_conic = restrict_quadric_to_plane(first_quadric, plane.origin, u_axis, v_axis);
    let second_conic = restrict_quadric_to_plane(second_quadric, plane.origin, u_axis, v_axis);
    common_plane_conic_parameters(first_conic, second_conic)
        .into_iter()
        .map(|[u, v]| {
            std::array::from_fn(|index| plane.origin[index] + u * u_axis[index] + v * v_axis[index])
        })
        .filter(|point| point_on_carrier(*point, first) && point_on_carrier(*point, second))
        .collect()
}

pub(super) fn intersect_two_planes_with_torus(
    first: PlaneEquation,
    second: PlaneEquation,
    torus: TorusEquation,
) -> Vec<[f64; 3]> {
    let Some((line_origin, direction)) = plane_intersection_line(first, second) else {
        return Vec::new();
    };
    let Some(axis) = normalized(torus.axis) else {
        return Vec::new();
    };
    if torus.major_radius <= 0.0 || torus.minor_radius <= 0.0 {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| line_origin[index] - torus.center[index]);
    let squared_distance = [dot(relative, relative), 2.0 * dot(relative, direction), 1.0];
    let axial = [dot(relative, axis), dot(direction, axis)];
    let axial_squared = [
        axial[0] * axial[0],
        2.0 * axial[0] * axial[1],
        axial[1] * axial[1],
    ];
    let mut shifted_distance = squared_distance;
    shifted_distance[0] +=
        torus.major_radius * torus.major_radius - torus.minor_radius * torus.minor_radius;
    let mut polynomial = [0.0; 5];
    for (left_power, left) in shifted_distance.into_iter().enumerate() {
        for (right_power, right) in shifted_distance.into_iter().enumerate() {
            polynomial[left_power + right_power] += left * right;
        }
    }
    let radial_scale = 4.0 * torus.major_radius * torus.major_radius;
    for power in 0..=2 {
        polynomial[power] -= radial_scale * (squared_distance[power] - axial_squared[power]);
    }
    let coordinate_scale = torus
        .center
        .into_iter()
        .chain(line_origin)
        .map(f64::abs)
        .fold(
            torus.major_radius.max(torus.minor_radius).max(1.0),
            f64::max,
        );
    real_polynomial_roots(&polynomial)
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| {
                let coordinate = line_origin[index] + parameter * direction[index];
                if coordinate.abs() <= 1e-14 * coordinate_scale {
                    0.0
                } else {
                    coordinate
                }
            })
        })
        .filter(|point| point_on_carrier(*point, CarrierEquation::Torus(torus)))
        .collect()
}

pub(super) fn intersect_plane_with_circle(
    plane: PlaneEquation,
    center: [f64; 3],
    circle_axis: [f64; 3],
    radius: f64,
) -> Vec<[f64; 3]> {
    let (Some(plane_normal), Some(circle_normal)) =
        (normalized(plane.normal), normalized(circle_axis))
    else {
        return Vec::new();
    };
    let line_direction = cross(plane_normal, circle_normal);
    let denominator = dot(line_direction, line_direction);
    if denominator <= 1e-18 || radius <= 0.0 {
        return Vec::new();
    }
    let plane_distance = dot(plane_normal, plane.origin);
    let circle_distance = dot(circle_normal, center);
    let weighted = std::array::from_fn(|index| {
        plane_distance * circle_normal[index] - circle_distance * plane_normal[index]
    });
    let line_origin = cross(weighted, line_direction).map(|value| value / denominator);
    let relative: [f64; 3] = std::array::from_fn(|index| line_origin[index] - center[index]);
    let parameter_at_nearest = -dot(relative, line_direction) / denominator;
    let nearest: [f64; 3] = std::array::from_fn(|index| {
        line_origin[index] + parameter_at_nearest * line_direction[index]
    });
    let center_to_nearest: [f64; 3] = std::array::from_fn(|index| nearest[index] - center[index]);
    let remaining = radius.mul_add(radius, -dot(center_to_nearest, center_to_nearest));
    let scale = radius.max(1.0);
    if remaining < -1e-12 * scale * scale {
        return Vec::new();
    }
    let parameter_delta = if remaining.abs() <= 1e-12 * scale * scale {
        0.0
    } else {
        remaining.sqrt() / denominator.sqrt()
    };
    let mut points = vec![std::array::from_fn(|index| {
        nearest[index] - parameter_delta * line_direction[index]
    })];
    if parameter_delta > 1e-12 * scale {
        points.push(std::array::from_fn(|index| {
            nearest[index] + parameter_delta * line_direction[index]
        }));
    }
    points
}

pub(super) fn circle_parameters(geometry: &CurveGeometry) -> Option<([f64; 3], [f64; 3], f64)> {
    let CurveGeometry::Circle {
        center,
        axis,
        radius,
        ..
    } = geometry
    else {
        return None;
    };
    Some((
        [center.x, center.y, center.z],
        [axis.x, axis.y, axis.z],
        *radius,
    ))
}

pub(super) fn plane_cone_conic(
    plane: PlaneEquation,
    cone: ConeEquation,
) -> Option<(CurveGeometry, &'static str)> {
    let normal = normalized(plane.normal)?;
    let axis = normalized(cone.axis)?;
    let x_axis = normalized(cone.ref_direction)?;
    let slope = cone.half_angle.tan();
    if slope <= 1e-12
        || !slope.is_finite()
        || cone.radius < 0.0
        || cone.ratio <= 0.0
        || !cone.ratio.is_finite()
        || dot(axis, x_axis).abs() > 1e-10
    {
        return None;
    }
    let y_axis = cross(axis, x_axis);
    let alignment = dot(normal, axis);
    let plane_u = normalized(std::array::from_fn(|index| {
        axis[index] - alignment * normal[index]
    }))?;
    let plane_v = normalized(cross(normal, plane_u))?;
    let relative: [f64; 3] = std::array::from_fn(|index| plane.origin[index] - cone.origin[index]);
    let coordinates = |vector: [f64; 3]| {
        [
            dot(vector, x_axis),
            dot(vector, y_axis) / cone.ratio,
            dot(vector, axis),
        ]
    };
    let origin = coordinates(relative);
    let u_coordinates = coordinates(plane_u);
    let v_coordinates = coordinates(plane_v);
    let origin_radius = cone.radius + slope * origin[2];
    let quadratic = |first: [f64; 3], second: [f64; 3]| {
        first[0].mul_add(
            second[0],
            first[1] * second[1] - slope * slope * first[2] * second[2],
        )
    };
    let linear = |direction: [f64; 3]| {
        2.0 * (origin[0].mul_add(
            direction[0],
            origin[1] * direction[1] - origin_radius * slope * direction[2],
        ))
    };
    let quadratic_uu = quadratic(u_coordinates, u_coordinates);
    let quadratic_uv = quadratic(u_coordinates, v_coordinates);
    let quadratic_vv = quadratic(v_coordinates, v_coordinates);
    let linear_u_source = linear(u_coordinates);
    let linear_v_source = linear(v_coordinates);
    let constant = origin[0].mul_add(
        origin[0],
        origin[1] * origin[1] - origin_radius * origin_radius,
    );
    let angle = 0.5 * (2.0 * quadratic_uv).atan2(quadratic_uu - quadratic_vv);
    let (sine, cosine) = angle.sin_cos();
    let first_direction =
        std::array::from_fn::<_, 3, _>(|index| cosine * plane_u[index] + sine * plane_v[index]);
    let second_direction =
        std::array::from_fn::<_, 3, _>(|index| -sine * plane_u[index] + cosine * plane_v[index]);
    let first_quadratic = quadratic_uu * cosine * cosine
        + 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * sine * sine;
    let second_quadratic = quadratic_uu * sine * sine - 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * cosine * cosine;
    let first_linear = linear_u_source * cosine + linear_v_source * sine;
    let second_linear = -linear_u_source * sine + linear_v_source * cosine;
    let opposite_signs = first_quadratic.is_sign_negative() != second_quadratic.is_sign_negative();
    let keep_first = if opposite_signs {
        first_quadratic.is_sign_negative()
    } else {
        first_quadratic.abs() <= second_quadratic.abs()
    };
    let (quadratic_u, quadratic_v, linear_u, linear_v, principal_u, principal_v) = if keep_first {
        (
            first_quadratic,
            second_quadratic,
            first_linear,
            second_linear,
            first_direction,
            second_direction,
        )
    } else {
        (
            second_quadratic,
            first_quadratic,
            second_linear,
            first_linear,
            second_direction,
            first_direction,
        )
    };
    let coefficient_scale = quadratic_u
        .abs()
        .max(quadratic_v.abs())
        .max(linear_u.abs())
        .max(linear_v.abs())
        .max(constant.abs())
        .max(1.0);
    let point = |u_parameter: f64, v_parameter: f64| {
        Point3::new(
            plane.origin[0] + u_parameter * principal_u[0] + v_parameter * principal_v[0],
            plane.origin[1] + u_parameter * principal_u[1] + v_parameter * principal_v[1],
            plane.origin[2] + u_parameter * principal_u[2] + v_parameter * principal_v[2],
        )
    };
    let axis_vector = Vector3::new(normal[0], normal[1], normal[2]);
    if quadratic_u.abs() <= 1e-12 * coefficient_scale {
        if linear_u.abs() <= 1e-12 * coefficient_scale {
            return None;
        }
        let vertex_v = -linear_v / (2.0 * quadratic_v);
        let shifted_constant = constant - linear_v * linear_v / (4.0 * quadratic_v);
        let vertex_u = -shifted_constant / linear_u;
        let opening = -linear_u / quadratic_v;
        if opening.abs() <= 1e-12 || !opening.is_finite() {
            return None;
        }
        let direction = principal_u.map(|value| value * opening.signum());
        return Some((
            CurveGeometry::Parabola {
                vertex: point(vertex_u, vertex_v),
                axis: axis_vector,
                major_direction: Vector3::new(direction[0], direction[1], direction[2]),
                focal_distance: opening.abs() / 4.0,
            },
            "plane_cone_parabola",
        ));
    }
    let center_u = -linear_u / (2.0 * quadratic_u);
    let center_v = -linear_v / (2.0 * quadratic_v);
    let shifted_constant = constant
        - linear_u * linear_u / (4.0 * quadratic_u)
        - linear_v * linear_v / (4.0 * quadratic_v);
    let value_scale = shifted_constant.abs().max(coefficient_scale).max(1.0);
    if shifted_constant.abs() <= 1e-12 * value_scale {
        return None;
    }
    let center = point(center_u, center_v);
    if quadratic_u > 0.0 {
        if shifted_constant >= 0.0 {
            return None;
        }
        let u_radius = (-shifted_constant / quadratic_u).sqrt();
        let v_radius = (-shifted_constant / quadratic_v).sqrt();
        let (major_direction, major_radius, minor_radius) = if u_radius >= v_radius {
            (principal_u, u_radius, v_radius)
        } else {
            (principal_v, v_radius, u_radius)
        };
        return Some((
            CurveGeometry::Ellipse {
                center,
                axis: axis_vector,
                major_direction: Vector3::new(
                    major_direction[0],
                    major_direction[1],
                    major_direction[2],
                ),
                major_radius,
                minor_radius,
            },
            "plane_cone_ellipse",
        ));
    }
    let (major_direction, major_radius, minor_radius) = if shifted_constant > 0.0 {
        (
            principal_u,
            (shifted_constant / -quadratic_u).sqrt(),
            (shifted_constant / quadratic_v).sqrt(),
        )
    } else {
        (
            principal_v,
            (-shifted_constant / quadratic_v).sqrt(),
            (-shifted_constant / -quadratic_u).sqrt(),
        )
    };
    Some((
        CurveGeometry::Hyperbola {
            center,
            axis: axis_vector,
            major_direction: Vector3::new(
                major_direction[0],
                major_direction[1],
                major_direction[2],
            ),
            major_radius,
            minor_radius,
        },
        "plane_cone_hyperbola",
    ))
}

pub(super) fn point_on_carrier(point: [f64; 3], carrier: CarrierEquation) -> bool {
    match carrier {
        CarrierEquation::Plane(plane) => {
            let residual = dot(plane.normal, point) - dot(plane.normal, plane.origin);
            residual.abs() <= 1e-7
        }
        CarrierEquation::Cylinder(cylinder) => {
            let Some(axis) = normalized(cylinder.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            (dot(radial, radial).sqrt() - cylinder.radius).abs() <= 1e-7 * cylinder.radius.max(1.0)
        }
        CarrierEquation::Cone(cone) => {
            let (Some(axis), Some(x_axis)) =
                (normalized(cone.axis), normalized(cone.ref_direction))
            else {
                return false;
            };
            if cone.ratio <= 0.0 || !cone.ratio.is_finite() || dot(axis, x_axis).abs() > 1e-10 {
                return false;
            }
            let y_axis = cross(axis, x_axis);
            let relative = std::array::from_fn(|index| point[index] - cone.origin[index]);
            let axial = dot(relative, axis);
            let radius = cone.radius + axial * cone.half_angle.tan();
            let radial_x = dot(relative, x_axis);
            let radial_y = dot(relative, y_axis) / cone.ratio;
            (radial_x.hypot(radial_y) - radius.abs()).abs() <= 1e-7 * radius.abs().max(1.0)
        }
        CarrierEquation::Sphere(sphere) => {
            let relative = std::array::from_fn(|index| point[index] - sphere.center[index]);
            (dot(relative, relative).sqrt() - sphere.radius).abs() <= 1e-7 * sphere.radius.max(1.0)
        }
        CarrierEquation::Torus(torus) => {
            let Some(axis) = normalized(torus.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - torus.center[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let tube_distance = (dot(radial, radial).sqrt() - torus.major_radius).hypot(axial);
            (tube_distance - torus.minor_radius).abs()
                <= 1e-7 * torus.minor_radius.max(torus.major_radius).max(1.0)
        }
    }
}

pub(super) fn tangent_sphere_point(
    first: SphereEquation,
    second: SphereEquation,
) -> Option<[f64; 3]> {
    let delta: [f64; 3] = std::array::from_fn(|index| second.center[index] - first.center[index]);
    let distance = dot(delta, delta).sqrt();
    if distance <= 1e-12 || first.radius <= 0.0 || second.radius <= 0.0 {
        return None;
    }
    let external = first.radius + second.radius;
    let internal = (first.radius - second.radius).abs();
    let scale = external.max(distance).max(1.0);
    if (distance - external).abs() > 1e-9 * scale && (distance - internal).abs() > 1e-9 * scale {
        return None;
    }
    let axial = (distance * distance + first.radius * first.radius - second.radius * second.radius)
        / (2.0 * distance);
    Some(std::array::from_fn(|index| {
        first.center[index] + axial * delta[index] / distance
    }))
}

pub(super) fn tangent_plane_sphere_point(
    plane: PlaneEquation,
    sphere: SphereEquation,
) -> Option<[f64; 3]> {
    let normal = normalized(plane.normal)?;
    let signed_distance = dot(
        normal,
        std::array::from_fn(|index| sphere.center[index] - plane.origin[index]),
    );
    let scale = sphere.radius.max(1.0);
    if sphere.radius <= 0.0 || (signed_distance.abs() - sphere.radius).abs() > 1e-9 * scale {
        return None;
    }
    Some(std::array::from_fn(|index| {
        sphere.center[index] - signed_distance * normal[index]
    }))
}

pub(super) fn solve_carriers(carriers: &[CarrierEquation]) -> Option<[f64; 3]> {
    let mut candidates = Vec::new();
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            match (carriers[first], carriers[second]) {
                (CarrierEquation::Plane(plane), CarrierEquation::Sphere(sphere))
                | (CarrierEquation::Sphere(sphere), CarrierEquation::Plane(plane)) => {
                    if let Some(point) = tangent_plane_sphere_point(plane, sphere) {
                        candidates.push(point);
                    }
                }
                (CarrierEquation::Sphere(first), CarrierEquation::Sphere(second)) => {
                    if let Some(point) = tangent_sphere_point(first, second) {
                        candidates.push(point);
                    }
                }
                _ => {}
            }
        }
    }
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            for third in second + 1..carriers.len() {
                let triple = [carriers[first], carriers[second], carriers[third]];
                let mut planes = Vec::new();
                let mut cylinders = Vec::new();
                let mut cones = Vec::new();
                let mut spheres = Vec::new();
                let mut tori = Vec::new();
                for carrier in triple {
                    match carrier {
                        CarrierEquation::Plane(plane) => planes.push(plane),
                        CarrierEquation::Cylinder(cylinder) => cylinders.push(cylinder),
                        CarrierEquation::Cone(cone) => cones.push(cone),
                        CarrierEquation::Sphere(sphere) => spheres.push(sphere),
                        CarrierEquation::Torus(torus) => tori.push(torus),
                    }
                }
                if planes.len() == 3 {
                    if let Some(point) = solve_planes(&planes) {
                        candidates.push(point);
                    }
                } else if planes.len() == 1
                    && tori.is_empty()
                    && cylinders.len() + cones.len() + spheres.len() == 2
                {
                    let reduced = if let [first, second] = cones.as_slice() {
                        intersect_plane_with_carrier_components(
                            planes[0],
                            CarrierEquation::Cone(*first),
                            CarrierEquation::Cone(*second),
                        )
                    } else {
                        Vec::new()
                    };
                    if reduced.is_empty() {
                        let quadrics = cylinders
                            .iter()
                            .copied()
                            .map(CarrierEquation::Cylinder)
                            .chain(cones.iter().copied().map(CarrierEquation::Cone))
                            .chain(spheres.iter().copied().map(CarrierEquation::Sphere))
                            .collect::<Vec<_>>();
                        candidates.extend(intersect_plane_with_two_quadrics(
                            planes[0],
                            quadrics[0],
                            quadrics[1],
                        ));
                    } else {
                        candidates.extend(reduced);
                    }
                } else if planes.len() == 2
                    && tori.is_empty()
                    && cylinders.len() + cones.len() + spheres.len() == 1
                {
                    let quadric = cylinders
                        .first()
                        .copied()
                        .map(CarrierEquation::Cylinder)
                        .or_else(|| cones.first().copied().map(CarrierEquation::Cone))
                        .or_else(|| spheres.first().copied().map(CarrierEquation::Sphere))
                        .expect("one quadric carrier");
                    candidates.extend(intersect_two_planes_with_quadric(
                        planes[0], planes[1], quadric,
                    ));
                } else if let ([first, second], [torus]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_two_planes_with_torus(*first, *second, *torus));
                    }
                } else if let ([plane], [cylinder], [torus]) =
                    (planes.as_slice(), cylinders.as_slice(), tori.as_slice())
                {
                    if cones.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Cylinder(*cylinder),
                            CarrierEquation::Torus(*torus),
                        ));
                    }
                } else if let ([plane], [cone], [sphere]) =
                    (planes.as_slice(), cones.as_slice(), spheres.as_slice())
                {
                    if cylinders.is_empty() && tori.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Sphere(*sphere),
                        ));
                    }
                } else if let ([plane], [cone], [torus]) =
                    (planes.as_slice(), cones.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Torus(*torus),
                        ));
                    }
                } else if let ([plane], [sphere], [torus]) =
                    (planes.as_slice(), spheres.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && cones.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Sphere(*sphere),
                            CarrierEquation::Torus(*torus),
                        ));
                    }
                } else if let ([plane], [first, second]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Torus(*first),
                            CarrierEquation::Torus(*second),
                        ));
                    }
                }
            }
        }
    }
    candidates.retain(|point| {
        carriers
            .iter()
            .all(|carrier| point_on_carrier(*point, *carrier))
    });
    let mut unique = Vec::<[f64; 3]>::new();
    for candidate in candidates {
        if !unique.iter().any(|known| {
            known
                .iter()
                .zip(candidate)
                .all(|(left, right)| (left - right).abs() <= 1e-7)
        }) {
            unique.push(candidate);
        }
    }
    let [point] = unique.as_slice() else {
        return None;
    };
    Some(*point)
}

pub(super) fn is_axis_aligned(vector: [f64; 3]) -> bool {
    vector.iter().filter(|value| value.abs() > 1e-9).count() == 1
}

pub(super) fn canonical_plane(plane: PlaneEquation) -> Option<PlaneEquation> {
    let mut normal = normalized(plane.normal)?;
    let mut distance = dot(normal, plane.origin);
    if !distance.is_finite() {
        return None;
    }
    let sign = normal
        .iter()
        .find(|coordinate| coordinate.abs() > 1e-12)?
        .signum();
    if sign < 0.0 {
        normal = normal.map(|coordinate| -coordinate);
        distance = -distance;
    }
    Some(PlaneEquation {
        origin: normal.map(|coordinate| coordinate * distance),
        normal,
    })
}

pub(super) fn agreed_plane(candidates: &[PlaneEquation]) -> Option<PlaneEquation> {
    let planes = candidates
        .iter()
        .copied()
        .map(canonical_plane)
        .collect::<Option<Vec<_>>>()?;
    let first = *planes.first()?;
    let first_distance = dot(first.normal, first.origin);
    planes
        .iter()
        .all(|plane| {
            let distance = dot(plane.normal, plane.origin);
            let scale = first_distance.abs().max(distance.abs()).max(1.0);
            first
                .normal
                .iter()
                .zip(plane.normal)
                .all(|(left, right)| (left - right).abs() <= 1e-9)
                && (first_distance - distance).abs() <= 1e-9 * scale
        })
        .then_some(first)
}

#[derive(Clone, Copy)]
pub(super) struct PlaneCandidate {
    pub(super) equation: PlaneEquation,
    pub(super) chart: Option<PlaneChart>,
    pub(super) offset: usize,
}

#[derive(Clone, Copy)]
pub(super) struct PlaneChart {
    pub(super) origin: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) u_axis: [f64; 3],
}

pub(super) fn agreed_plane_surface(
    candidates: &[PlaneCandidate],
) -> Option<(PlaneEquation, [f64; 3], usize)> {
    agreed_plane(
        &candidates
            .iter()
            .map(|candidate| candidate.equation)
            .collect::<Vec<_>>(),
    )?;
    let charts = candidates
        .iter()
        .filter_map(|candidate| {
            let chart = candidate.chart?;
            let normal = normalized(chart.normal)?;
            let u_axis = normalized(chart.u_axis)?;
            (dot(normal, u_axis).abs() <= 1e-9).then_some((
                chart.origin,
                normal,
                u_axis,
                candidate.offset,
            ))
        })
        .collect::<Vec<_>>();
    let representative = charts.iter().min_by_key(|(_, _, _, offset)| *offset)?;
    charts
        .iter()
        .all(|(origin, normal, u_axis, _)| {
            representative.0.iter().zip(origin).all(|(left, right)| {
                (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
            }) && representative
                .1
                .iter()
                .zip(normal)
                .all(|(left, right)| (left - right).abs() <= 1e-9)
                && representative
                    .2
                    .iter()
                    .zip(u_axis)
                    .all(|(left, right)| (left - right).abs() <= 1e-9)
        })
        .then_some((
            PlaneEquation {
                origin: representative.0,
                normal: representative.1,
            },
            representative.2,
            representative.3,
        ))
}

pub(super) fn plane_candidates(scan: &ContainerScan) -> BTreeMap<u32, Vec<PlaneCandidate>> {
    let held_planes = scan
        .planes
        .envelopes
        .iter()
        .filter_map(|envelope| Some((envelope.surface_id, held_coordinate_plane(envelope)?)))
        .fold(
            BTreeMap::<u32, Vec<PlaneEquation>>::new(),
            |mut planes, (surface_id, plane)| {
                planes.entry(surface_id).or_default().push(plane);
                planes
            },
        )
        .into_iter()
        .filter_map(|(surface_id, planes)| agreed_plane(&planes).map(|plane| (surface_id, plane)))
        .collect::<BTreeMap<_, _>>();
    let frame_bound_outlines = crate::surface::frame_bound_outline_planes(
        &scan.planes.envelopes,
        &scan.planes.local_systems,
    )
    .into_iter()
    .fold(
        BTreeMap::<u32, Vec<crate::surface::OutlinePlane>>::new(),
        |mut outlines, outline| {
            outlines
                .entry(outline.surface_id)
                .or_default()
                .push(outline);
            outlines
        },
    );
    let mut candidates = BTreeMap::<u32, Vec<PlaneCandidate>>::new();
    for frame in &scan.planes.local_systems {
        let (Some(origin), Some(normal)) = (frame.origin, frame.normal) else {
            continue;
        };
        let Some(u_axis) = frame.u_axis else {
            continue;
        };
        let frame_candidate = PlaneCandidate {
            equation: PlaneEquation { origin, normal },
            chart: Some(PlaneChart {
                origin,
                normal,
                u_axis,
            }),
            offset: frame.offset,
        };
        let candidate = frame_bound_outlines
            .get(&frame.surface_id)
            .and_then(|outlines| {
                let [outline] = outlines.as_slice() else {
                    return None;
                };
                frame_bound_outline_plane_candidate(frame, outline)
            })
            .or_else(|| {
                held_planes
                    .get(&frame.surface_id)
                    .filter(|held| agreed_plane(&[frame_candidate.equation, **held]).is_none())
                    .and_then(|held| envelope_reconciled_plane_candidate(frame, *held))
            })
            .unwrap_or(frame_candidate);
        candidates
            .entry(frame.surface_id)
            .or_default()
            .push(candidate);
    }
    let local_chart_ids = scan
        .planes
        .local_systems
        .iter()
        .filter(|frame| frame.origin.is_some() && frame.normal.is_some() && frame.u_axis.is_some())
        .map(|frame| frame.surface_id)
        .collect::<BTreeSet<_>>();
    for outline in &scan.planes.outlines {
        candidates
            .entry(outline.surface_id)
            .or_default()
            .push(PlaneCandidate {
                equation: PlaneEquation {
                    origin: outline.origin,
                    normal: outline.normal,
                },
                chart: (!local_chart_ids.contains(&outline.surface_id)).then_some(PlaneChart {
                    origin: outline.origin,
                    normal: outline.normal,
                    u_axis: outline.u_axis,
                }),
                offset: outline.offset,
            });
    }
    for envelope in &scan.planes.envelopes {
        let Some(equation) = held_coordinate_plane(envelope) else {
            continue;
        };
        candidates
            .entry(envelope.surface_id)
            .or_default()
            .push(PlaneCandidate {
                equation,
                chart: None,
                offset: envelope.offset,
            });
    }
    for plane in &scan.planes.positional_frames {
        if candidates.contains_key(&plane.surface_id) {
            continue;
        }
        candidates.insert(
            plane.surface_id,
            vec![PlaneCandidate {
                equation: PlaneEquation {
                    origin: plane.origin,
                    normal: plane.normal,
                },
                chart: Some(PlaneChart {
                    origin: plane.origin,
                    normal: plane.normal,
                    u_axis: plane.u_axis,
                }),
                offset: plane.offset,
            }],
        );
    }
    candidates
        .into_iter()
        .filter(|(id, _)| {
            scan.surfaces
                .rows
                .iter()
                .filter(|row| row.id == *id)
                .take(2)
                .count()
                < 2
        })
        .collect()
}

pub(super) fn frame_bound_outline_plane_candidate(
    frame: &crate::surface::PlaneLocalSystem,
    outline: &crate::surface::OutlinePlane,
) -> Option<PlaneCandidate> {
    (frame.surface_id == outline.surface_id).then_some(())?;
    let frame_normal = normalized(frame.normal?)?;
    let frame_u_axis = normalized(frame.u_axis?)?;
    let outline_normal = normalized(outline.normal)?;
    let outline_u_axis = normalized(outline.u_axis)?;
    (dot(frame_normal, outline_normal) >= 1.0 - 1e-9).then_some(())?;
    (dot(frame_u_axis, outline_u_axis) >= 1.0 - 1e-9).then_some(())?;
    let frame_origin = frame.origin?;
    let displacement = dot(outline_normal, outline.origin) - dot(outline_normal, frame_origin);
    let chart_origin =
        std::array::from_fn(|axis| displacement.mul_add(outline_normal[axis], frame_origin[axis]));
    Some(PlaneCandidate {
        equation: PlaneEquation {
            origin: outline.origin,
            normal: outline.normal,
        },
        chart: Some(PlaneChart {
            origin: chart_origin,
            normal: frame.normal?,
            u_axis: frame.u_axis?,
        }),
        offset: frame.offset,
    })
}

pub(super) fn envelope_reconciled_plane_candidate(
    frame: &crate::surface::PlaneLocalSystem,
    equation: PlaneEquation,
) -> Option<PlaneCandidate> {
    let origin = frame.origin?;
    let normal = normalized(equation.normal)?;
    let origin_scale = origin
        .iter()
        .chain(equation.origin.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    ((dot(normal, origin) - dot(normal, equation.origin)).abs() <= 1e-9 * origin_scale)
        .then_some(())?;
    let slots: [f64; 12] = frame
        .slots
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let supports = [
        <[f64; 3]>::try_from(&slots[0..3]).ok()?,
        <[f64; 3]>::try_from(&slots[3..6]).ok()?,
        <[f64; 3]>::try_from(&slots[6..9]).ok()?,
    ];
    let support_scale = supports
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let nonzero = supports
        .into_iter()
        .filter_map(|support| {
            let magnitude = dot(support, support).sqrt();
            (magnitude > 1e-9 * support_scale).then_some((support, magnitude))
        })
        .collect::<Vec<_>>();
    let [first, second] = nonzero.as_slice() else {
        return None;
    };
    let role = |(support, magnitude): &([f64; 3], f64)| {
        let alignment = dot(*support, normal).abs() / *magnitude;
        if alignment <= 1e-9 {
            Some((false, support.map(|value| value / *magnitude)))
        } else if (alignment - 1.0).abs() <= 1e-9 {
            Some((true, support.map(|value| value / *magnitude)))
        } else {
            None
        }
    };
    let (first_parallel, first_direction) = role(first)?;
    let (second_parallel, second_direction) = role(second)?;
    (first_parallel != second_parallel).then_some(())?;
    let u_axis = if first_parallel {
        second_direction
    } else {
        first_direction
    };
    Some(PlaneCandidate {
        equation,
        chart: Some(PlaneChart {
            origin,
            normal,
            u_axis,
        }),
        offset: frame.offset,
    })
}

pub(super) fn held_coordinate_plane(
    envelope: &crate::surface::PlaneEnvelopeRecord,
) -> Option<PlaneEquation> {
    let corners = plane_envelope_corners(&envelope.envelope)?;
    let held = envelope
        .corner_coordinate_equal
        .iter()
        .enumerate()
        .filter_map(|(axis, equal)| (*equal == Some(true)).then_some(axis))
        .collect::<Vec<_>>();
    let [axis] = held.as_slice() else {
        return None;
    };
    envelope
        .corner_coordinate_equal
        .iter()
        .enumerate()
        .all(|(candidate, equal)| candidate == *axis || *equal == Some(false))
        .then_some(())?;
    let mut normal = [0.0; 3];
    normal[*axis] = 1.0;
    Some(PlaneEquation {
        origin: corners[0],
        normal,
    })
}

pub(super) fn placed_planes(scan: &ContainerScan) -> BTreeMap<u32, PlaneEquation> {
    plane_candidates(scan)
        .into_iter()
        .filter_map(|(id, candidates)| {
            agreed_plane(
                &candidates
                    .iter()
                    .map(|candidate| candidate.equation)
                    .collect::<Vec<_>>(),
            )
            .map(|plane| (id, plane))
        })
        .collect()
}

pub(super) fn placed_plane_surfaces(
    scan: &ContainerScan,
) -> BTreeMap<u32, (PlaneEquation, [f64; 3], usize)> {
    plane_candidates(scan)
        .into_iter()
        .filter_map(|(id, candidates)| {
            agreed_plane_surface(&candidates).map(|surface| (id, surface))
        })
        .collect()
}

pub(super) fn topology_bound_plane(
    points: impl IntoIterator<Item = [f64; 3]>,
) -> Option<PlaneEquation> {
    let mut points = points.into_iter().collect::<Vec<_>>();
    points.sort_by(|left, right| {
        left.iter()
            .zip(right)
            .find_map(|(left, right)| {
                let ordering = left.total_cmp(right);
                (ordering != std::cmp::Ordering::Equal).then_some(ordering)
            })
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    points.dedup_by(|left, right| model_points_agree(*left, *right));
    let origin = *points.first()?;
    let scale = points
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let mut normal = None;
    'candidate: for first in 1..points.len() {
        for second in first + 1..points.len() {
            let first_direction = std::array::from_fn(|axis| points[first][axis] - origin[axis]);
            let second_direction = std::array::from_fn(|axis| points[second][axis] - origin[axis]);
            let Some(candidate) = normalized(cross(first_direction, second_direction)) else {
                continue;
            };
            normal = Some(candidate);
            break 'candidate;
        }
    }
    let mut normal = normal?;
    let leading = normal.iter().find(|coordinate| coordinate.abs() > 1e-12)?;
    if *leading < 0.0 {
        normal = normal.map(|coordinate| -coordinate);
    }
    points
        .iter()
        .all(|point| {
            let displacement = std::array::from_fn(|axis| point[axis] - origin[axis]);
            dot(displacement, normal).abs() <= 1e-9 * scale
        })
        .then_some(PlaneEquation { origin, normal })
}

pub(super) fn analytic_curve_plane(geometry: &CurveGeometry) -> Option<PlaneEquation> {
    let (origin, normal) = match geometry {
        CurveGeometry::Circle { center, axis, .. }
        | CurveGeometry::Ellipse { center, axis, .. } => (
            [center.x, center.y, center.z],
            normalized([axis.x, axis.y, axis.z])?,
        ),
        CurveGeometry::Nurbs(nurbs) => {
            valid_positive_nurbs_curve(nurbs)?;
            let plane = topology_bound_plane(
                nurbs
                    .control_points
                    .iter()
                    .map(|point| [point.x, point.y, point.z]),
            )?;
            (plane.origin, plane.normal)
        }
        _ => return None,
    };
    Some(PlaneEquation { origin, normal })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BoundaryLine {
    pub(super) origin: [f64; 3],
    pub(super) direction: [f64; 3],
}

pub(super) fn analytic_boundary_line(geometry: &CurveGeometry) -> Option<BoundaryLine> {
    let (origin, direction) = match geometry {
        CurveGeometry::Line { origin, direction } => (
            [origin.x, origin.y, origin.z],
            normalized([direction.x, direction.y, direction.z])?,
        ),
        CurveGeometry::Nurbs(nurbs) => {
            (nurbs.degree == 1 && !nurbs.periodic).then_some(())?;
            valid_positive_nurbs_curve(nurbs)?;
            let first = *nurbs.control_points.first()?;
            let last = *nurbs.control_points.last()?;
            let origin = [first.x, first.y, first.z];
            let direction = normalized([last.x - first.x, last.y - first.y, last.z - first.z])?;
            let scale = nurbs
                .control_points
                .iter()
                .flat_map(|point| [point.x, point.y, point.z])
                .map(f64::abs)
                .fold(1.0, f64::max);
            nurbs
                .control_points
                .iter()
                .map(|point| {
                    let relative = [
                        point.x - origin[0],
                        point.y - origin[1],
                        point.z - origin[2],
                    ];
                    let residual = cross(relative, direction);
                    dot(residual, residual).sqrt()
                })
                .all(|residual| residual <= 1e-9 * scale)
                .then_some(())?;
            (origin, direction)
        }
        _ => return None,
    };
    Some(BoundaryLine { origin, direction })
}

pub(super) fn valid_positive_nurbs_curve(nurbs: &NurbsCurve) -> Option<()> {
    nurbs_intrinsic_parameter_range(nurbs)?;
    nurbs
        .weights
        .as_ref()
        .is_none_or(|weights| {
            weights
                .iter()
                .all(|weight| weight.is_finite() && *weight > 0.0)
        })
        .then_some(())
}

pub(super) fn topology_bound_line_plane(lines: &[BoundaryLine]) -> Option<PlaneEquation> {
    let mut candidate = None;
    'pairs: for first in 0..lines.len() {
        for second in first + 1..lines.len() {
            let direction_cross = cross(lines[first].direction, lines[second].direction);
            let displacement =
                std::array::from_fn(|axis| lines[second].origin[axis] - lines[first].origin[axis]);
            let normal = normalized(direction_cross)
                .or_else(|| normalized(cross(lines[first].direction, displacement)));
            if let Some(normal) = normal {
                candidate = Some(PlaneEquation {
                    origin: lines[first].origin,
                    normal,
                });
                break 'pairs;
            }
        }
    }
    let candidate = candidate?;
    let canonical = agreed_plane(&[candidate])?;
    lines
        .iter()
        .all(|line| {
            point_on_carrier(line.origin, CarrierEquation::Plane(canonical))
                && dot(line.direction, canonical.normal).abs() <= 1e-9
        })
        .then_some(canonical)
}

pub(super) fn agreed_topology_bound_plane(
    points: impl IntoIterator<Item = [f64; 3]>,
    curve_planes: impl IntoIterator<Item = PlaneEquation>,
    lines: impl IntoIterator<Item = BoundaryLine>,
) -> Option<PlaneEquation> {
    let points = points.into_iter().collect::<Vec<_>>();
    let lines = lines.into_iter().collect::<Vec<_>>();
    let candidates = topology_bound_plane(points.iter().copied())
        .into_iter()
        .chain(curve_planes)
        .chain(topology_bound_line_plane(&lines))
        .collect::<Vec<_>>();
    let plane = agreed_plane(&candidates)?;
    let points_agree = points
        .iter()
        .all(|point| point_on_carrier(*point, CarrierEquation::Plane(plane)));
    let lines_agree = lines.iter().all(|line| {
        point_on_carrier(line.origin, CarrierEquation::Plane(plane))
            && dot(line.direction, plane.normal).abs() <= 1e-9
    });
    (points_agree && lines_agree).then_some(plane)
}

pub(super) fn transfer_topology_bound_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> usize {
    let carriers = placed_carriers(scan, ir);
    let solved_vertices =
        solved_topological_vertices(scan, ir, &carriers, nurbs_endpoint_witnesses);
    let vertex_faces =
        crate::topology::vertex_incident_faces(&scan.topology.vertices, &scan.topology.half_edges);
    let unique_rows = crate::surface::uniquely_identified_rows(&scan.surfaces.rows);
    let unique_curve_ids = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .map(|row| row.id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for row in unique_rows
        .into_iter()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)
    {
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let points = solved_vertices
            .iter()
            .filter_map(|(vertex_id, point)| {
                vertex_faces
                    .get(vertex_id)
                    .is_some_and(|faces| faces.contains(&row.id))
                    .then_some(*point)
            })
            .collect::<Vec<_>>();
        let boundary_curves = scan
            .topology
            .loops
            .iter()
            .filter(|lp| lp.face_id == row.id)
            .flat_map(|lp| lp.half_edges.iter())
            .filter_map(|half_edge| {
                unique_curve_ids
                    .contains(&half_edge.curve_id)
                    .then_some(())?;
                let id = CurveId(format!("creo:visibgeom:curve#{}", half_edge.curve_id));
                let curve = ir.model.curves.iter().find(|curve| curve.id == id)?;
                Some(&curve.geometry)
            })
            .collect::<Vec<_>>();
        let curve_planes = boundary_curves
            .iter()
            .filter_map(|geometry| analytic_curve_plane(geometry));
        let lines = boundary_curves
            .iter()
            .filter_map(|geometry| analytic_boundary_line(geometry));
        let Some(plane) = agreed_topology_bound_plane(points, curve_planes, lines) else {
            continue;
        };
        let normal = Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]);
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            "plane_topology_boundary",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal,
                u_axis: cadmpeg_ir::geometry::derive_reference_direction(normal),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn retain_unresolved_visible_carriers(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for row in crate::surface::uniquely_identified_rows(&scan.surfaces.rows) {
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            "unresolved_visible_surface_carrier",
            Exactness::Unknown,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Unknown {
                record: geometry_section_record(scan, row.offset),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            "unresolved_visible_curve_carrier",
            Exactness::Unknown,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Unknown {
                record: geometry_section_record(scan, row.offset),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
}

pub(super) fn placed_carriers(scan: &ContainerScan, ir: &CadIr) -> BTreeMap<u32, CarrierEquation> {
    let mut carriers = placed_planes(scan)
        .into_iter()
        .map(|(id, plane)| (id, CarrierEquation::Plane(plane)))
        .collect::<BTreeMap<_, _>>();
    for row in crate::surface::uniquely_identified_rows(&scan.surfaces.rows) {
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        let Some(surface) = ir.model.surfaces.iter().find(|surface| surface.id == id) else {
            continue;
        };
        if let SurfaceGeometry::Plane { origin, normal, .. } = &surface.geometry {
            let plane = PlaneEquation {
                origin: [origin.x, origin.y, origin.z],
                normal: [normal.x, normal.y, normal.z],
            };
            let agreed = match carriers.get(&row.id) {
                Some(CarrierEquation::Plane(existing)) => agreed_plane(&[*existing, plane]),
                Some(_) => None,
                None => Some(plane),
            };
            if let Some(plane) = agreed {
                carriers.insert(row.id, CarrierEquation::Plane(plane));
            } else {
                carriers.remove(&row.id);
            }
        } else if let SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } = &surface.geometry
        {
            carriers.insert(
                row.id,
                CarrierEquation::Cylinder(CylinderEquation {
                    origin: [origin.x, origin.y, origin.z],
                    axis: [axis.x, axis.y, axis.z],
                    ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    radius: *radius,
                }),
            );
        } else if let SurfaceGeometry::Sphere {
            center,
            axis: _,
            ref_direction,
            radius,
        } = &surface.geometry
        {
            carriers.insert(
                row.id,
                CarrierEquation::Sphere(SphereEquation {
                    center: [center.x, center.y, center.z],
                    ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    radius: *radius,
                }),
            );
        } else if let SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } = &surface.geometry
        {
            if ratio.is_finite() && *ratio > 0.0 {
                carriers.insert(
                    row.id,
                    CarrierEquation::Cone(ConeEquation {
                        origin: [origin.x, origin.y, origin.z],
                        axis: [axis.x, axis.y, axis.z],
                        ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                        radius: *radius,
                        ratio: *ratio,
                        half_angle: *half_angle,
                    }),
                );
            }
        } else if let SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } = &surface.geometry
        {
            carriers.insert(
                row.id,
                CarrierEquation::Torus(TorusEquation {
                    center: [center.x, center.y, center.z],
                    axis: [axis.x, axis.y, axis.z],
                    ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    major_radius: *major_radius,
                    minor_radius: *minor_radius,
                }),
            );
        }
    }
    carriers
}

pub(super) fn geometry_section_record(scan: &ContainerScan, offset: usize) -> Option<UnknownId> {
    scan.framing
        .sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY)
        .find(|section| {
            offset >= section.offset && offset < section.offset.saturating_add(section.length)
        })
        .map(|section| UnknownId(format!("creo:{}:section#{}", section.name, section.offset)))
}

pub(super) fn projected_loop_polygon(
    lp: &crate::topology::Loop,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<[f64; 2]>> {
    let dropped_axis = (0..3).max_by(|left, right| {
        plane.normal[*left]
            .abs()
            .total_cmp(&plane.normal[*right].abs())
    })?;
    let polygon = lp
        .half_edges
        .iter()
        .map(|half_edge| {
            let vertex = incidence.get(half_edge)?.start_vertex_id;
            let point = solved_vertices.get(&vertex)?;
            Some(match dropped_axis {
                0 => [point[1], point[2]],
                1 => [point[0], point[2]],
                _ => [point[0], point[1]],
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let area_twice = (0..polygon.len())
        .map(|index| {
            let first = polygon[index];
            let second = polygon[(index + 1) % polygon.len()];
            first[0].mul_add(second[1], -(first[1] * second[0]))
        })
        .sum::<f64>();
    let scale = polygon
        .iter()
        .flat_map(|point| point.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (polygon.len() >= 3 && area_twice.abs() > 1e-12 * scale * scale).then_some(polygon)
}

pub(super) fn polygon_strictly_contains(polygon: &[[f64; 2]], point: [f64; 2]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let edge = [second[0] - first[0], second[1] - first[1]];
        let relative = [point[0] - first[0], point[1] - first[1]];
        let cross = edge[0].mul_add(relative[1], -(edge[1] * relative[0]));
        let scale = edge[0].abs().max(edge[1].abs()).max(1.0);
        if cross.abs() <= 1e-9 * scale
            && point[0] >= first[0].min(second[0]) - 1e-9 * scale
            && point[0] <= first[0].max(second[0]) + 1e-9 * scale
            && point[1] >= first[1].min(second[1]) - 1e-9 * scale
            && point[1] <= first[1].max(second[1]) + 1e-9 * scale
        {
            return false;
        }
        if (first[1] > point[1]) != (second[1] > point[1]) {
            let intersection = edge[0].mul_add((point[1] - first[1]) / edge[1], first[0]);
            if point[0] < intersection {
                inside = !inside;
            }
        }
    }
    inside
}

pub(super) fn ordered_planar_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() == 1 {
        return Some(loops);
    }
    let polygons = loops
        .iter()
        .map(|lp| projected_loop_polygon(lp, plane, incidence, solved_vertices))
        .collect::<Option<Vec<_>>>()?;
    let outer = polygons
        .iter()
        .enumerate()
        .filter(|(candidate, polygon)| {
            polygons.iter().enumerate().all(|(index, inner)| {
                index == *candidate
                    || inner
                        .iter()
                        .all(|point| polygon_strictly_contains(polygon, *point))
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [outer] = outer.as_slice() else {
        return None;
    };
    let mut ordered = Vec::with_capacity(loops.len());
    ordered.push(loops[*outer]);
    ordered.extend(
        loops
            .into_iter()
            .enumerate()
            .filter_map(|(index, lp)| (index != *outer).then_some(lp)),
    );
    Some(ordered)
}

pub(super) fn face_boundary_plane(
    loops: &[&crate::topology::Loop],
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<PlaneEquation> {
    topology_bound_plane(loops.iter().flat_map(|lp| {
        lp.half_edges
            .iter()
            .filter_map(|half_edge| incidence.get(half_edge))
            .filter_map(|binding| solved_vertices.get(&binding.start_vertex_id).copied())
    }))
}

pub(super) fn ordered_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    plane: Option<PlaneEquation>,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    let plane = plane.or_else(|| face_boundary_plane(&loops, incidence, solved_vertices));
    if let Some(plane) = plane {
        ordered_planar_face_loops(loops, plane, incidence, solved_vertices)
    } else {
        let [single] = loops.as_slice() else {
            return None;
        };
        Some(vec![*single])
    }
}

pub(super) fn rowless_round_face_orientations(
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
    available_surfaces: &BTreeSet<u32>,
) -> BTreeMap<u32, bool> {
    let mut orientations = BTreeMap::new();
    for (rowless_id, sibling_id, _) in rowless_round_cylinder_pairs(round_feature_ids, tables, rows)
    {
        if !available_surfaces.contains(&rowless_id) {
            continue;
        }
        let Some(reversed) =
            crate::surface::unique_surface_row(rows, sibling_id).map(|row| row.reversed)
        else {
            continue;
        };
        orientations.insert(rowless_id, reversed);
    }
    orientations
}

pub(super) fn native_face_orientations(scan: &ContainerScan, ir: &CadIr) -> BTreeMap<u32, bool> {
    let mut orientations = scan
        .surfaces
        .rows
        .iter()
        .map(|row| row.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, id)
                .map(|row| (id, row.reversed))
        })
        .collect::<BTreeMap<_, _>>();
    let round_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let available_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            surface
                .id
                .0
                .strip_prefix("creo:visibgeom:surface#")?
                .parse()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    orientations.extend(rowless_round_face_orientations(
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
        &available_surfaces,
    ));
    orientations
}

pub(super) fn model_points_agree(first: [f64; 3], second: [f64; 3]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(first, second)| (first - second).abs() <= 1e-9 * scale)
}

pub(super) fn line_line_intersection(
    first: &CurveGeometry,
    second: &CurveGeometry,
) -> Option<[f64; 3]> {
    let (
        CurveGeometry::Line {
            origin: first_origin,
            direction: first_direction,
        },
        CurveGeometry::Line {
            origin: second_origin,
            direction: second_direction,
        },
    ) = (first, second)
    else {
        return None;
    };
    let first_origin = [first_origin.x, first_origin.y, first_origin.z];
    let second_origin = [second_origin.x, second_origin.y, second_origin.z];
    let first_direction = [first_direction.x, first_direction.y, first_direction.z];
    let second_direction = [second_direction.x, second_direction.y, second_direction.z];
    let relative = std::array::from_fn(|axis| first_origin[axis] - second_origin[axis]);
    let first_squared = dot(first_direction, first_direction);
    let second_squared = dot(second_direction, second_direction);
    let product = dot(first_direction, second_direction);
    let first_relative = dot(first_direction, relative);
    let second_relative = dot(second_direction, relative);
    let denominator = first_squared.mul_add(second_squared, -(product * product));
    if !denominator.is_finite()
        || denominator <= 1e-12 * first_squared * second_squared
        || first_squared <= 0.0
        || second_squared <= 0.0
    {
        return None;
    }
    let first_parameter =
        product.mul_add(second_relative, -(second_squared * first_relative)) / denominator;
    let second_parameter =
        first_squared.mul_add(second_relative, -(product * first_relative)) / denominator;
    let first_point = std::array::from_fn(|axis| {
        first_direction[axis].mul_add(first_parameter, first_origin[axis])
    });
    let second_point = std::array::from_fn(|axis| {
        second_direction[axis].mul_add(second_parameter, second_origin[axis])
    });
    (first_point
        .iter()
        .chain(second_point.iter())
        .all(|value| value.is_finite())
        && model_points_agree(first_point, second_point))
    .then(|| std::array::from_fn(|axis| f64::midpoint(first_point[axis], second_point[axis])))
}

pub(super) fn line_conic_intersections(
    line: &CurveGeometry,
    conic: &CurveGeometry,
) -> Vec<[f64; 3]> {
    let CurveGeometry::Line { origin, direction } = line else {
        return Vec::new();
    };
    let Some(PlanarConicEquation {
        origin: conic_origin,
        normal,
        x_axis,
        y_axis,
        quadratic,
        linear,
        constant,
        scale: conic_scale,
    }) = planar_conic_equation(conic)
    else {
        return Vec::new();
    };
    let origin = [origin.x, origin.y, origin.z];
    let Some(direction) = normalized([direction.x, direction.y, direction.z]) else {
        return Vec::new();
    };
    let relative = std::array::from_fn(|coordinate| origin[coordinate] - conic_origin[coordinate]);
    let direction_plane = dot(direction, normal);
    let origin_plane = dot(relative, normal);
    let model_scale = origin
        .into_iter()
        .chain(conic_origin)
        .map(f64::abs)
        .fold(conic_scale.max(1.0), f64::max);
    if direction_plane.abs() > 1e-12 {
        let parameter = -origin_plane / direction_plane;
        let point = std::array::from_fn(|coordinate| {
            direction[coordinate].mul_add(parameter, origin[coordinate])
        });
        return (point.iter().all(|value| value.is_finite())
            && curve_contains_points(conic, [point, point]))
        .then_some(point)
        .into_iter()
        .collect();
    }
    if origin_plane.abs() > 1e-9 * model_scale {
        return Vec::new();
    }
    let local_origin = [dot(relative, x_axis), dot(relative, y_axis)];
    let local_direction = [dot(direction, x_axis), dot(direction, y_axis)];
    let line_quadratic = quadratic[0].mul_add(
        local_direction[0].powi(2),
        quadratic[1] * local_direction[1].powi(2),
    );
    let line_linear =
        2.0 * quadratic[0].mul_add(
            local_origin[0] * local_direction[0],
            quadratic[1] * local_origin[1] * local_direction[1],
        ) + linear[0].mul_add(local_direction[0], linear[1] * local_direction[1]);
    let line_constant = quadratic[0].mul_add(
        local_origin[0].powi(2),
        quadratic[1] * local_origin[1].powi(2),
    ) + linear[0].mul_add(local_origin[0], linear[1] * local_origin[1])
        + constant;
    let coefficient_scale = line_linear
        .abs()
        .max((line_quadratic * line_constant).abs().sqrt())
        .max(1.0);
    let coefficient_tolerance = 1e-14 * coefficient_scale;
    if !line_quadratic.is_finite() || !line_linear.is_finite() || !line_constant.is_finite() {
        return Vec::new();
    }
    if line_quadratic.abs() <= coefficient_tolerance {
        if line_linear.abs() <= coefficient_tolerance {
            return Vec::new();
        }
        let parameter = -line_constant / line_linear;
        let point = std::array::from_fn(|coordinate| {
            direction[coordinate].mul_add(parameter, origin[coordinate])
        });
        return curve_contains_points(conic, [point, point])
            .then_some(point)
            .into_iter()
            .collect();
    }
    let discriminant = line_linear.mul_add(line_linear, -4.0 * line_quadratic * line_constant);
    let tolerance = 1e-12 * coefficient_scale * coefficient_scale;
    if !discriminant.is_finite() || discriminant < -tolerance {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    let first_parameter = -line_linear / (2.0 * line_quadratic);
    let first = std::array::from_fn(|coordinate| {
        direction[coordinate].mul_add(first_parameter, origin[coordinate])
    });
    if root <= 1e-9 * coefficient_scale {
        return curve_contains_points(conic, [first, first])
            .then_some(first)
            .into_iter()
            .collect();
    }
    let root_product = -0.5 * (line_linear + root.copysign(line_linear));
    let first_parameter = root_product / line_quadratic;
    let second_parameter = line_constant / root_product;
    let first = std::array::from_fn(|coordinate| {
        direction[coordinate].mul_add(first_parameter, origin[coordinate])
    });
    let second = std::array::from_fn(|coordinate| {
        direction[coordinate].mul_add(second_parameter, origin[coordinate])
    });
    [first, second]
        .into_iter()
        .filter(|point| curve_contains_points(conic, [*point, *point]))
        .collect()
}

pub(super) fn restrict_planar_conic_to_chart(
    conic: PlanarConicEquation,
    origin: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
) -> PlaneConicEquation {
    let offset: [f64; 3] =
        std::array::from_fn(|coordinate| origin[coordinate] - conic.origin[coordinate]);
    let x = [
        dot(offset, conic.x_axis),
        dot(u_axis, conic.x_axis),
        dot(v_axis, conic.x_axis),
    ];
    let y = [
        dot(offset, conic.y_axis),
        dot(u_axis, conic.y_axis),
        dot(v_axis, conic.y_axis),
    ];
    PlaneConicEquation {
        uu: conic.quadratic[0].mul_add(x[1].powi(2), conic.quadratic[1] * y[1].powi(2)),
        uv: 2.0 * conic.quadratic[0].mul_add(x[1] * x[2], conic.quadratic[1] * y[1] * y[2]),
        vv: conic.quadratic[0].mul_add(x[2].powi(2), conic.quadratic[1] * y[2].powi(2)),
        u: 2.0 * conic.quadratic[0].mul_add(x[0] * x[1], conic.quadratic[1] * y[0] * y[1])
            + conic.linear[0].mul_add(x[1], conic.linear[1] * y[1]),
        v: 2.0 * conic.quadratic[0].mul_add(x[0] * x[2], conic.quadratic[1] * y[0] * y[2])
            + conic.linear[0].mul_add(x[2], conic.linear[1] * y[2]),
        constant: conic.quadratic[0].mul_add(x[0].powi(2), conic.quadratic[1] * y[0].powi(2))
            + conic.linear[0].mul_add(x[0], conic.linear[1] * y[0])
            + conic.constant,
    }
}

pub(super) fn conic_conic_intersections(
    first: &CurveGeometry,
    second: &CurveGeometry,
) -> Vec<[f64; 3]> {
    let Some(first_equation) = planar_conic_equation(first) else {
        return Vec::new();
    };
    let Some(second_equation) = planar_conic_equation(second) else {
        return Vec::new();
    };
    let normal_cross = cross(first_equation.normal, second_equation.normal);
    if dot(normal_cross, normal_cross) > 1e-18 {
        let Some((origin, direction)) = plane_intersection_line(
            PlaneEquation {
                origin: first_equation.origin,
                normal: first_equation.normal,
            },
            PlaneEquation {
                origin: second_equation.origin,
                normal: second_equation.normal,
            },
        ) else {
            return Vec::new();
        };
        let line = CurveGeometry::Line {
            origin: Point3::new(origin[0], origin[1], origin[2]),
            direction: Vector3::new(direction[0], direction[1], direction[2]),
        };
        let mut points = line_conic_intersections(&line, first);
        points.retain(|point| curve_contains_points(second, [*point, *point]));
        return points;
    }
    let delta: [f64; 3] = std::array::from_fn(|coordinate| {
        second_equation.origin[coordinate] - first_equation.origin[coordinate]
    });
    let scale = first_equation
        .origin
        .into_iter()
        .chain(second_equation.origin)
        .map(f64::abs)
        .fold(
            first_equation.scale.max(second_equation.scale).max(1.0),
            f64::max,
        );
    if dot(delta, first_equation.normal).abs() > 1e-9 * scale {
        return Vec::new();
    }
    let first_chart = restrict_planar_conic_to_chart(
        first_equation,
        first_equation.origin,
        first_equation.x_axis,
        first_equation.y_axis,
    );
    let second_chart = restrict_planar_conic_to_chart(
        second_equation,
        first_equation.origin,
        first_equation.x_axis,
        first_equation.y_axis,
    );
    common_plane_conic_parameters(first_chart, second_chart)
        .into_iter()
        .map(|[u, v]| {
            std::array::from_fn(|coordinate| {
                first_equation.origin[coordinate]
                    + u * first_equation.x_axis[coordinate]
                    + v * first_equation.y_axis[coordinate]
            })
        })
        .filter(|point| {
            curve_contains_points(first, [*point, *point])
                && curve_contains_points(second, [*point, *point])
        })
        .collect()
}

pub(super) fn incident_analytic_vertex_domain(curves: &[&CurveGeometry]) -> Vec<[f64; 3]> {
    let mut candidates = Vec::new();
    for first in 0..curves.len() {
        for second in first + 1..curves.len() {
            candidates.extend(
                line_line_intersection(curves[first], curves[second])
                    .into_iter()
                    .chain(line_conic_intersections(curves[first], curves[second]))
                    .chain(line_conic_intersections(curves[second], curves[first]))
                    .chain(conic_conic_intersections(curves[first], curves[second])),
            );
        }
    }
    candidates.retain(|point| {
        curves
            .iter()
            .all(|curve| curve_contains_points(curve, [*point, *point]))
    });
    candidates
        .into_iter()
        .fold(Vec::new(), |mut unique, point| {
            if !unique
                .iter()
                .any(|candidate| model_points_agree(*candidate, point))
            {
                unique.push(point);
            }
            unique
        })
}

pub(super) fn mapped_pcurve_endpoints(
    ir: &CadIr,
    faces: [u32; 2],
    endpoint_sets: [[[f64; 2]; 2]; 2],
) -> Option<[[f64; 3]; 2]> {
    let mapped = faces
        .into_iter()
        .zip(endpoint_sets)
        .filter_map(|(face_id, endpoints)| {
            let surface = ir.model.surfaces.iter().find(|surface| {
                surface.id == SurfaceId(format!("creo:visibgeom:surface#{face_id}"))
            })?;
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::surface_point(&surface.geometry, uv[0], uv[1])
                    .map(|point| [point.x, point.y, point.z])
            });
            Some([first?, second?])
        })
        .collect::<Vec<[[f64; 3]; 2]>>();
    let first = *mapped.first()?;
    mapped
        .iter()
        .all(|candidate| {
            model_points_agree(first[0], candidate[0]) && model_points_agree(first[1], candidate[1])
        })
        .then_some(first)
}

pub(super) fn pcurve_edge_endpoints(
    scan: &ContainerScan,
    ir: &CadIr,
) -> BTreeMap<u32, [[f64; 3]; 2]> {
    let mut candidates = BTreeMap::<u32, Vec<[[f64; 3]; 2]>>::new();
    for (curve_id, faces, first, second) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )
        }))
    {
        if let Some(points) = mapped_pcurve_endpoints(ir, faces, [first, second]) {
            candidates.entry(curve_id).or_default().push(points);
        }
    }
    candidates
        .into_iter()
        .filter_map(|(curve_id, candidates)| {
            let first = *candidates.first()?;
            candidates
                .iter()
                .all(|candidate| {
                    model_points_agree(first[0], candidate[0])
                        && model_points_agree(first[1], candidate[1])
                })
                .then_some((curve_id, first))
        })
        .collect()
}

pub(super) fn linear_pcurve_carrier(
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
) -> Option<CurveGeometry> {
    let scaled_vector = |vector: Vector3, scale: f64| {
        Vector3::new(vector.x * scale, vector.y * scale, vector.z * scale)
    };
    let offset_point = |point: Point3, vector: Vector3, scale: f64| {
        Point3::new(
            point.x + vector.x * scale,
            point.y + vector.y * scale,
            point.z + vector.z * scale,
        )
    };
    let [start, end] = endpoints;
    if start == end {
        return None;
    }
    match surface {
        SurfaceGeometry::Plane { .. } => {
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
                    .map(|point| [point.x, point.y, point.z])
            });
            let [first, second] = [first?, second?];
            let direction = normalized(std::array::from_fn(|axis| second[axis] - first[axis]))?;
            Some(CurveGeometry::Line {
                origin: Point3::new(first[0], first[1], first[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } if start[0] == end[0] => {
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let reference = [ref_direction.x, ref_direction.y, ref_direction.z];
            let radial: [f64; 3] = std::array::from_fn(|coordinate| {
                start[0].cos() * reference[coordinate] + start[0].sin() * transverse[coordinate]
            });
            let point = [
                origin.x + radius * radial[0] + start[1] * axis.x,
                origin.y + radius * radial[1] + start[1] * axis.y,
                origin.z + radius * radial[2] + start[1] * axis.z,
            ];
            let direction = normalized([axis.x, axis.y, axis.z])?;
            Some(CurveGeometry::Line {
                origin: Point3::new(point[0], point[1], point[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } if start[1] == end[1] && radius.is_finite() && *radius > 0.0 => {
            Some(CurveGeometry::Circle {
                center: offset_point(*origin, *axis, start[1]),
                axis: *axis,
                ref_direction: *ref_direction,
                radius: *radius,
            })
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ratio,
            half_angle,
            ..
        } if start[0] == end[0] => {
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
                    .map(|point| [point.x, point.y, point.z])
            });
            let [first, second] = [first?, second?];
            let direction = normalized(std::array::from_fn(|axis| second[axis] - first[axis]))?;
            (ratio.is_finite() && *ratio > 0.0 && half_angle.is_finite()).then_some(())?;
            Some(CurveGeometry::Line {
                origin: Point3::new(first[0], first[1], first[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if start[1] == end[1] && ratio.is_finite() && *ratio > 0.0 => {
            let local_radius = radius + start[1] * half_angle.tan();
            let first_radius = local_radius.abs();
            let second_radius = (local_radius * ratio).abs();
            if !first_radius.is_finite() || !second_radius.is_finite() {
                return None;
            }
            let center = offset_point(*origin, *axis, start[1]);
            if (first_radius - second_radius).abs()
                <= 1e-12 * first_radius.max(second_radius).max(1.0)
            {
                (first_radius > 0.0).then_some(CurveGeometry::Circle {
                    center,
                    axis: *axis,
                    ref_direction: scaled_vector(*ref_direction, local_radius.signum()),
                    radius: first_radius,
                })
            } else {
                let transverse = cross(
                    [axis.x, axis.y, axis.z],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                let transverse = Vector3::new(transverse[0], transverse[1], transverse[2]);
                let (major_direction, major_radius, minor_radius) = if first_radius > second_radius
                {
                    (
                        scaled_vector(*ref_direction, local_radius.signum()),
                        first_radius,
                        second_radius,
                    )
                } else {
                    (
                        scaled_vector(transverse, (local_radius * ratio).signum()),
                        second_radius,
                        first_radius,
                    )
                };
                (minor_radius > 0.0).then_some(CurveGeometry::Ellipse {
                    center,
                    axis: *axis,
                    major_direction,
                    major_radius,
                    minor_radius,
                })
            }
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if start[1] == end[1] && radius.is_finite() && *radius > 0.0 => {
            let ring = radius * start[1].cos();
            (ring.abs() > 0.0).then_some(CurveGeometry::Circle {
                center: offset_point(*center, *axis, radius * start[1].sin()),
                axis: *axis,
                ref_direction: scaled_vector(*ref_direction, ring.signum()),
                radius: ring.abs(),
            })
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if start[0] == end[0] && radius.is_finite() && *radius > 0.0 => {
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let radial = Vector3::new(
                start[0].cos() * ref_direction.x + start[0].sin() * transverse[0],
                start[0].cos() * ref_direction.y + start[0].sin() * transverse[1],
                start[0].cos() * ref_direction.z + start[0].sin() * transverse[2],
            );
            let normal = cross([radial.x, radial.y, radial.z], [axis.x, axis.y, axis.z]);
            Some(CurveGeometry::Circle {
                center: *center,
                axis: Vector3::new(normal[0], normal[1], normal[2]),
                ref_direction: radial,
                radius: *radius,
            })
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if start[1] == end[1]
            && major_radius.is_finite()
            && minor_radius.is_finite()
            && *minor_radius > 0.0 =>
        {
            let ring = major_radius + minor_radius * start[1].cos();
            (ring.abs() > 0.0).then_some(CurveGeometry::Circle {
                center: offset_point(*center, *axis, minor_radius * start[1].sin()),
                axis: *axis,
                ref_direction: scaled_vector(*ref_direction, ring.signum()),
                radius: ring.abs(),
            })
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if start[0] == end[0]
            && major_radius.is_finite()
            && minor_radius.is_finite()
            && *minor_radius > 0.0 =>
        {
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let radial = Vector3::new(
                start[0].cos() * ref_direction.x + start[0].sin() * transverse[0],
                start[0].cos() * ref_direction.y + start[0].sin() * transverse[1],
                start[0].cos() * ref_direction.z + start[0].sin() * transverse[2],
            );
            let normal = cross([radial.x, radial.y, radial.z], [axis.x, axis.y, axis.z]);
            Some(CurveGeometry::Circle {
                center: offset_point(*center, radial, *major_radius),
                axis: Vector3::new(normal[0], normal[1], normal[2]),
                ref_direction: radial,
                radius: *minor_radius,
            })
        }
        _ => None,
    }
}

pub(super) fn transfer_analytic_pcurve_carriers(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> BTreeSet<CurveId> {
    let reconciled_endpoints = pcurve_edge_endpoints(scan, ir);
    let mut candidates = BTreeMap::<u32, Vec<(CurveGeometry, usize)>>::new();
    let mut evaluable_path_counts = BTreeMap::<u32, usize>::new();
    for (curve_id, faces, endpoint_sets, offset) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                [pcurve.face_0_endpoints, pcurve.face_1_endpoints],
                pcurve.offset,
            )
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                [pcurve.face_0_endpoints, pcurve.face_1_endpoints],
                pcurve.offset,
            )
        }))
    {
        for (face_id, endpoints) in faces.into_iter().zip(endpoint_sets) {
            let Some(surface) = ir.model.surfaces.iter().find(|surface| {
                surface.id == SurfaceId(format!("creo:visibgeom:surface#{face_id}"))
            }) else {
                continue;
            };
            if endpoints.iter().all(|uv| {
                cadmpeg_ir::eval::surface_point(&surface.geometry, uv[0], uv[1]).is_some()
            }) {
                *evaluable_path_counts.entry(curve_id).or_default() += 1;
            }
            if let Some(carrier) = linear_pcurve_carrier(&surface.geometry, endpoints) {
                candidates
                    .entry(curve_id)
                    .or_default()
                    .push((carrier, offset));
            }
        }
    }
    let mut transferred = BTreeSet::new();
    for (curve_id, candidates) in candidates {
        if evaluable_path_counts.get(&curve_id).copied() != Some(candidates.len()) {
            continue;
        }
        let Some(points) = reconciled_endpoints.get(&curve_id).copied() else {
            continue;
        };
        let Some((geometry, offset)) = candidates.first() else {
            continue;
        };
        if !curve_contains_points(geometry, points)
            || !candidates.iter().all(|(candidate, _)| {
                curve_contains_points(candidate, points)
                    && [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().all(|parameter| {
                        let point = cadmpeg_ir::eval::curve_point(candidate, parameter);
                        point.is_some_and(|point| {
                            curve_contains_points(geometry, [[point.x, point.y, point.z]; 2])
                        })
                    })
            })
        {
            continue;
        }
        let offset = candidates
            .iter()
            .map(|(_, offset)| *offset)
            .min()
            .unwrap_or(*offset);
        let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            offset as u64,
            "analytic_pcurve_carrier",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: geometry.clone(),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{curve_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred.insert(id);
    }
    transferred
}

pub(super) type PcurveVertexConstraint = ([u32; 2], [[f64; 3]; 2]);

pub(super) fn directed_pcurve_points(
    directions: [u8; 2],
    points: [[f64; 3]; 2],
) -> Option<[[f64; 3]; 2]> {
    match directions {
        [0x01, 0xf6] => Some(points),
        [0xf6, 0x01] => Some([points[1], points[0]]),
        _ => None,
    }
}

pub(super) fn solve_pcurve_vertex_domains(
    constraints: &[PcurveVertexConstraint],
    fixed_points: &BTreeMap<u32, Option<[f64; 3]>>,
    analytic_domains: &BTreeMap<u32, Vec<[f64; 3]>>,
    incident_curves: &BTreeMap<u32, Vec<&CurveGeometry>>,
) -> BTreeMap<u32, [f64; 3]> {
    let mut domains = BTreeMap::<u32, Vec<[f64; 3]>>::new();
    for (vertices, points) in constraints {
        if vertices[0] == vertices[1] {
            match domains.entry(vertices[0]) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(if model_points_agree(points[0], points[1]) {
                        vec![points[0]]
                    } else {
                        Vec::new()
                    });
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let domain = entry.get_mut();
                    if model_points_agree(points[0], points[1]) {
                        domain.retain(|candidate| model_points_agree(*candidate, points[0]));
                    } else {
                        domain.clear();
                    }
                }
            }
            continue;
        }
        for vertex in vertices {
            let domain = domains.entry(*vertex).or_insert_with(|| points.to_vec());
            domain.retain(|candidate| {
                points
                    .iter()
                    .any(|point| model_points_agree(*candidate, *point))
            });
        }
    }
    for (vertex, candidates) in analytic_domains {
        match domains.entry(*vertex) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(candidates.clone());
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().retain(|point| {
                    candidates
                        .iter()
                        .any(|candidate| model_points_agree(*point, *candidate))
                });
            }
        }
    }
    for (vertex, point) in fixed_points {
        match domains.entry(*vertex) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(point.iter().copied().collect());
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if let Some(point) = point {
                    entry
                        .get_mut()
                        .retain(|candidate| model_points_agree(*candidate, *point));
                } else {
                    entry.get_mut().clear();
                }
            }
        }
    }
    for (vertex, curves) in incident_curves {
        if let Some(domain) = domains.get_mut(vertex) {
            domain.retain(|candidate| {
                curves
                    .iter()
                    .all(|curve| curve_contains_points(curve, [*candidate, *candidate]))
            });
        }
    }
    let compatible = |first: [f64; 3], second: [f64; 3], points: [[f64; 3]; 2]| {
        (model_points_agree(first, points[0]) && model_points_agree(second, points[1]))
            || (model_points_agree(first, points[1]) && model_points_agree(second, points[0]))
    };
    loop {
        let mut changed = false;
        for (vertices, points) in constraints {
            if vertices[0] == vertices[1] {
                continue;
            }
            let first = domains.get(&vertices[0]).cloned().unwrap_or_default();
            let second = domains.get(&vertices[1]).cloned().unwrap_or_default();
            let retained_first = first
                .iter()
                .copied()
                .filter(|first| {
                    second
                        .iter()
                        .any(|second| compatible(*first, *second, *points))
                })
                .collect::<Vec<_>>();
            let retained_second = second
                .iter()
                .copied()
                .filter(|second| {
                    first
                        .iter()
                        .any(|first| compatible(*first, *second, *points))
                })
                .collect::<Vec<_>>();
            changed |= retained_first.len() != first.len() || retained_second.len() != second.len();
            domains.insert(vertices[0], retained_first);
            domains.insert(vertices[1], retained_second);
        }
        if !changed {
            break;
        }
    }
    domains
        .into_iter()
        .filter_map(|(vertex, mut domain)| {
            domain.dedup_by(|first, second| model_points_agree(*first, *second));
            let [point] = domain.as_slice() else {
                return None;
            };
            Some((vertex, *point))
        })
        .collect()
}

pub(super) fn solved_topological_vertices(
    scan: &ContainerScan,
    ir: &CadIr,
    carriers: &BTreeMap<u32, CarrierEquation>,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> BTreeMap<u32, [f64; 3]> {
    let vertex_faces =
        crate::topology::vertex_incident_faces(&scan.topology.vertices, &scan.topology.half_edges);
    let carrier_points = scan
        .topology
        .vertices
        .iter()
        .filter_map(|vertex| {
            let incident_carriers = vertex_faces
                .get(&vertex.id)?
                .iter()
                .filter_map(|face_id| carriers.get(face_id))
                .copied()
                .collect::<Vec<_>>();
            solve_carriers(&incident_carriers).map(|point| (vertex.id, point))
        })
        .collect::<BTreeMap<_, _>>();
    let edge_endpoints = pcurve_edge_endpoints(scan, ir);
    let edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let mut fixed_points = carrier_points
        .into_iter()
        .map(|(vertex, point)| (vertex, Some(point)))
        .collect::<BTreeMap<_, _>>();
    let mut constraints = Vec::new();
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let Some(points) = edge_endpoints.get(&row.id).copied() else {
            continue;
        };
        let Some(vertices) = edge_vertices.get(&row.id).copied() else {
            continue;
        };
        constraints.push((vertices, points));
        if let Some(ordered) = directed_pcurve_points(row.directions, points) {
            for (vertex, point) in vertices.into_iter().zip(ordered) {
                match fixed_points.entry(vertex) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(Some(point));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        if entry
                            .get()
                            .is_none_or(|known| !model_points_agree(known, point))
                        {
                            entry.insert(None);
                        }
                    }
                }
            }
        }
    }
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let Some(vertices) = edge_vertices.get(&row.id).copied() else {
            continue;
        };
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        if !nurbs_endpoint_witnesses.contains(&id) {
            continue;
        }
        let Some(geometry) = ir.model.curves.iter().find(|curve| curve.id == id) else {
            continue;
        };
        let Some(points) = nonperiodic_nurbs_endpoint_points(&geometry.geometry) else {
            continue;
        };
        constraints.push((vertices, points));
    }
    let analytic_curves = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .filter_map(|row| {
            let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
            let geometry = &ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == id)?
                .geometry;
            let evaluable = match geometry {
                CurveGeometry::Line { .. }
                | CurveGeometry::Circle { .. }
                | CurveGeometry::Ellipse { .. }
                | CurveGeometry::Parabola { .. }
                | CurveGeometry::Hyperbola { .. } => true,
                CurveGeometry::Nurbs(nurbs) => valid_positive_nurbs_curve(nurbs).is_some(),
                _ => false,
            };
            evaluable.then_some((row.id, geometry))
        })
        .collect::<BTreeMap<_, _>>();
    let incident_curves = scan
        .topology
        .vertices
        .iter()
        .filter_map(|vertex| {
            let curves = vertex
                .half_edges
                .iter()
                .filter_map(|half_edge| analytic_curves.get(&half_edge.curve_id).copied())
                .collect::<Vec<_>>();
            (!curves.is_empty()).then_some((vertex.id, curves))
        })
        .collect::<BTreeMap<_, _>>();
    let analytic_domains = incident_curves
        .iter()
        .filter_map(|(vertex, curves)| {
            let candidates = incident_analytic_vertex_domain(curves);
            (!candidates.is_empty()).then_some((*vertex, candidates))
        })
        .collect::<BTreeMap<_, _>>();
    solve_pcurve_vertex_domains(
        &constraints,
        &fixed_points,
        &analytic_domains,
        &incident_curves,
    )
}

pub(super) fn orient_line_edge_carrier(
    geometry: &mut CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points) {
        return None;
    }
    let CurveGeometry::Line { origin, direction } = geometry else {
        return None;
    };
    let delta: [f64; 3] = std::array::from_fn(|index| points[1][index] - points[0][index]);
    let length = dot(delta, delta).sqrt();
    let oriented = normalized(delta)?;
    *origin = Point3::new(points[0][0], points[0][1], points[0][2]);
    *direction = Vector3::new(oriented[0], oriented[1], oriented[2]);
    Some([0.0, length])
}

pub(super) fn exact_line_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points) {
        return None;
    }
    let CurveGeometry::Line { origin, direction } = geometry else {
        return None;
    };
    let direction = [direction.x, direction.y, direction.z];
    let denominator = dot(direction, direction);
    if !denominator.is_finite() || denominator <= 0.0 {
        return None;
    }
    let origin = [origin.x, origin.y, origin.z];
    let parameters = points.map(|point| {
        dot(
            std::array::from_fn(|index| point[index] - origin[index]),
            direction,
        ) / denominator
    });
    parameters
        .into_iter()
        .all(f64::is_finite)
        .then_some(if parameters[0] <= parameters[1] {
            parameters
        } else {
            [parameters[1], parameters[0]]
        })
}

pub(super) fn point_pair_alignments(mapped: [[f64; 3]; 2], target: [[f64; 3]; 2]) -> [bool; 2] {
    let mismatch = |left: [f64; 3], right: [f64; 3]| {
        dot(
            std::array::from_fn(|index| left[index] - right[index]),
            std::array::from_fn(|index| left[index] - right[index]),
        )
        .sqrt()
    };
    let scale = mapped
        .into_iter()
        .flatten()
        .chain(target.into_iter().flatten())
        .map(f64::abs)
        .fold(1.0, f64::max);
    let tolerance = 1e-9 * scale;
    [
        mismatch(mapped[0], target[0]).max(mismatch(mapped[1], target[1])) <= tolerance,
        mismatch(mapped[0], target[1]).max(mismatch(mapped[1], target[0])) <= tolerance,
    ]
}

pub(super) fn nurbs_control_extent(nurbs: &NurbsCurve) -> Option<f64> {
    let bounds = nurbs.control_points.iter().try_fold(
        [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
        |mut bounds, point| {
            for (index, coordinate) in [point.x, point.y, point.z].into_iter().enumerate() {
                coordinate.is_finite().then_some(())?;
                bounds[0][index] = bounds[0][index].min(coordinate);
                bounds[1][index] = bounds[1][index].max(coordinate);
            }
            Some(bounds)
        },
    )?;
    Some(
        (0..3)
            .map(|index| bounds[1][index] - bounds[0][index])
            .fold(1.0, f64::max),
    )
}

pub(super) fn nurbs_intrinsic_parameter_range(nurbs: &NurbsCurve) -> Option<[f64; 2]> {
    let degree = usize::try_from(nurbs.degree).ok()?;
    (degree > 0
        && nurbs.control_points.len() > degree
        && nurbs.knots.len() == nurbs.control_points.len().checked_add(degree + 1)?
        && nurbs_control_extent(nurbs).is_some()
        && nurbs.knots.iter().all(|knot| knot.is_finite())
        && nurbs.knots.windows(2).all(|pair| pair[0] <= pair[1])
        && nurbs
            .weights
            .as_ref()
            .is_none_or(|weights| weights.len() == nurbs.control_points.len()))
    .then_some(())?;
    let range = [
        *nurbs.knots.get(degree)?,
        *nurbs.knots.get(nurbs.control_points.len())?,
    ];
    (range[0] < range[1]).then_some(range)
}

pub(super) fn nonperiodic_nurbs_endpoint_points(geometry: &CurveGeometry) -> Option<[[f64; 3]; 2]> {
    let CurveGeometry::Nurbs(nurbs) = geometry else {
        return None;
    };
    (!nurbs.periodic).then_some(())?;
    valid_positive_nurbs_curve(nurbs)?;
    let range = nurbs_intrinsic_parameter_range(nurbs)?;
    let points = range.map(|parameter| {
        cadmpeg_ir::eval::curve_point(geometry, parameter).map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = points else {
        return None;
    };
    first
        .into_iter()
        .chain(second)
        .all(f64::is_finite)
        .then_some([first, second])
}

pub(super) fn nonperiodic_nurbs_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let CurveGeometry::Nurbs(nurbs) = geometry else {
        return None;
    };
    if nurbs.periodic {
        return None;
    }
    let degree = usize::try_from(nurbs.degree).ok()?;
    let range = nurbs_intrinsic_parameter_range(nurbs)?;

    if degree == 1 {
        nurbs
            .weights
            .as_ref()
            .is_none_or(|weights| {
                weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
            })
            .then_some(())?;
        let scale = nurbs_control_extent(nurbs)?;
        let tolerance = 1e-9 * scale;
        let first = degree_one_nurbs_point_parameter(geometry, nurbs, points[0], range, tolerance)?;
        let second =
            degree_one_nurbs_point_parameter(geometry, nurbs, points[1], range, tolerance)?;
        let parameters = if first <= second {
            [first, second]
        } else {
            [second, first]
        };
        return (parameters[1] - parameters[0] > 1e-12 * (range[1] - range[0]).max(1.0))
            .then_some(parameters);
    }

    let mapped = range.map(|parameter| {
        cadmpeg_ir::eval::curve_point(geometry, parameter).map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    match point_pair_alignments([first, second], points) {
        [true, false] | [false, true] => Some(range),
        _ => None,
    }
}

pub(super) fn full_periodic_nurbs_edge_parameter_range(
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Option<[f64; 2]> {
    let CurveGeometry::Nurbs(nurbs) = geometry else {
        return None;
    };
    nurbs.periodic.then_some(())?;
    nurbs
        .weights
        .as_ref()
        .is_none_or(|weights| {
            weights
                .iter()
                .all(|weight| weight.is_finite() && *weight > 0.0)
        })
        .then_some(())?;
    let range = nurbs_intrinsic_parameter_range(nurbs)?;
    let mapped = range.map(|parameter| {
        cadmpeg_ir::eval::curve_point(geometry, parameter).map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    let tolerance = 1e-9 * nurbs_control_extent(nurbs)?;
    [first, second]
        .into_iter()
        .all(|mapped| {
            let delta: [f64; 3] = std::array::from_fn(|index| mapped[index] - point[index]);
            dot(delta, delta).sqrt() <= tolerance
        })
        .then_some(range)
}

pub(super) fn degree_one_nurbs_point_parameter(
    geometry: &CurveGeometry,
    nurbs: &NurbsCurve,
    point: [f64; 3],
    range: [f64; 2],
    tolerance: f64,
) -> Option<f64> {
    let parameter_tolerance = 1e-9 * (range[1] - range[0]).max(1.0);
    let mut candidates = Vec::<f64>::new();
    for span in 1..nurbs.control_points.len() {
        let lower = nurbs.knots[span];
        let upper = nurbs.knots[span + 1];
        if !lower.is_finite() || !upper.is_finite() || upper <= lower {
            continue;
        }
        let first = nurbs.control_points[span - 1];
        let second = nurbs.control_points[span];
        let delta = [second.x - first.x, second.y - first.y, second.z - first.z];
        let denominator = dot(delta, delta);
        if !denominator.is_finite() {
            continue;
        }
        let relative = [point[0] - first.x, point[1] - first.y, point[2] - first.z];
        if denominator <= tolerance * tolerance {
            if dot(relative, relative).sqrt() <= tolerance {
                return None;
            }
            continue;
        }
        let fraction = dot(relative, delta) / denominator;
        if !(-1e-9..=1.0 + 1e-9).contains(&fraction) {
            continue;
        }
        let fraction = fraction.clamp(0.0, 1.0);
        let projected = [
            first.x + fraction * delta[0],
            first.y + fraction * delta[1],
            first.z + fraction * delta[2],
        ];
        let mismatch: [f64; 3] = std::array::from_fn(|index| projected[index] - point[index]);
        if dot(mismatch, mismatch).sqrt() > tolerance {
            continue;
        }
        let first_weight = nurbs
            .weights
            .as_ref()
            .map_or(1.0, |weights| weights[span - 1]);
        let second_weight = nurbs.weights.as_ref().map_or(1.0, |weights| weights[span]);
        let rational_denominator = second_weight * (1.0 - fraction) + fraction * first_weight;
        if rational_denominator <= 0.0 || !rational_denominator.is_finite() {
            continue;
        }
        let local = fraction * first_weight / rational_denominator;
        let parameter = lower + local * (upper - lower);
        let Some(mapped) = cadmpeg_ir::eval::curve_point(geometry, parameter) else {
            continue;
        };
        let mismatch = [
            mapped.x - point[0],
            mapped.y - point[1],
            mapped.z - point[2],
        ];
        if dot(mismatch, mismatch).sqrt() <= tolerance
            && !candidates
                .iter()
                .any(|known| (parameter - known).abs() <= parameter_tolerance)
        {
            candidates.push(parameter);
        }
    }
    let [parameter] = candidates.as_slice() else {
        return None;
    };
    Some(*parameter)
}

#[derive(Clone, Copy)]
pub(super) struct PeriodicConicFrame {
    pub(super) center: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) x_axis: [f64; 3],
    pub(super) y_axis: [f64; 3],
    pub(super) radii: [f64; 2],
}

#[derive(Clone, Copy)]
pub(super) struct PlanarConicEquation {
    pub(super) origin: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) x_axis: [f64; 3],
    pub(super) y_axis: [f64; 3],
    pub(super) quadratic: [f64; 2],
    pub(super) linear: [f64; 2],
    pub(super) constant: f64,
    pub(super) scale: f64,
}

#[derive(Clone, Copy)]
pub(super) enum NonperiodicConicFamily {
    Parabola,
    Hyperbola,
}

#[derive(Clone, Copy)]
pub(super) struct NonperiodicConicFrame {
    pub(super) origin: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) x_axis: [f64; 3],
    pub(super) y_axis: [f64; 3],
    pub(super) x_scale: f64,
    pub(super) y_scale: f64,
    pub(super) family: NonperiodicConicFamily,
}

pub(super) fn planar_conic_equation(geometry: &CurveGeometry) -> Option<PlanarConicEquation> {
    if let Some(frame) = periodic_conic_frame(geometry) {
        return Some(PlanarConicEquation {
            origin: frame.center,
            normal: frame.normal,
            x_axis: frame.x_axis,
            y_axis: frame.y_axis,
            quadratic: [1.0 / frame.radii[0].powi(2), 1.0 / frame.radii[1].powi(2)],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: frame.radii.into_iter().fold(1.0, f64::max),
        });
    }
    let NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    } = nonperiodic_conic_frame(geometry)?;
    let (quadratic, linear, constant) = match family {
        NonperiodicConicFamily::Parabola => ([0.0, -1.0 / (2.0 * y_scale)], [1.0, 0.0], 0.0),
        NonperiodicConicFamily::Hyperbola => (
            [1.0 / x_scale.powi(2), -1.0 / y_scale.powi(2)],
            [0.0, 0.0],
            -1.0,
        ),
    };
    Some(PlanarConicEquation {
        origin,
        normal,
        x_axis,
        y_axis,
        quadratic,
        linear,
        constant,
        scale: x_scale.max(y_scale),
    })
}

pub(super) fn nonperiodic_conic_frame(geometry: &CurveGeometry) -> Option<NonperiodicConicFrame> {
    let (origin, normal, x_axis, x_scale, y_scale, family) = match geometry {
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => (
            [vertex.x, vertex.y, vertex.z],
            [axis.x, axis.y, axis.z],
            [major_direction.x, major_direction.y, major_direction.z],
            *focal_distance,
            2.0 * *focal_distance,
            NonperiodicConicFamily::Parabola,
        ),
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            [center.x, center.y, center.z],
            [axis.x, axis.y, axis.z],
            [major_direction.x, major_direction.y, major_direction.z],
            *major_radius,
            *minor_radius,
            NonperiodicConicFamily::Hyperbola,
        ),
        _ => return None,
    };
    let normal = normalized(normal)?;
    let x_axis = normalized(x_axis)?;
    (dot(normal, x_axis).abs() <= 1e-9).then_some(())?;
    let y_axis = normalized(cross(normal, x_axis))?;
    (origin.into_iter().all(f64::is_finite)
        && x_scale > 0.0
        && x_scale.is_finite()
        && y_scale > 0.0
        && y_scale.is_finite())
    .then_some(())?;
    Some(NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    })
}

pub(super) fn periodic_conic_frame(geometry: &CurveGeometry) -> Option<PeriodicConicFrame> {
    let (center, axis, x_axis, radii) = match geometry {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => (
            [center.x, center.y, center.z],
            [axis.x, axis.y, axis.z],
            [ref_direction.x, ref_direction.y, ref_direction.z],
            [*radius, *radius],
        ),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            [center.x, center.y, center.z],
            [axis.x, axis.y, axis.z],
            [major_direction.x, major_direction.y, major_direction.z],
            [*major_radius, *minor_radius],
        ),
        _ => return None,
    };
    let axis = normalized(axis)?;
    let x_axis = normalized(x_axis)?;
    (dot(axis, x_axis).abs() <= 1e-9).then_some(())?;
    let y_axis = normalized(cross(axis, x_axis))?;
    (center.into_iter().all(f64::is_finite)
        && radii
            .into_iter()
            .all(|radius| radius > 0.0 && radius.is_finite()))
    .then_some(PeriodicConicFrame {
        center,
        normal: axis,
        x_axis,
        y_axis,
        radii,
    })
}

pub(super) fn nonperiodic_conic_parameter(
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Option<f64> {
    let NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    } = nonperiodic_conic_frame(geometry)?;
    let relative = std::array::from_fn(|index| point[index] - origin[index]);
    let scale = dot(relative, relative)
        .sqrt()
        .max(x_scale)
        .max(y_scale)
        .max(1.0);
    (dot(relative, normal).abs() <= 1e-7 * scale).then_some(())?;
    let x = dot(relative, x_axis);
    let y = dot(relative, y_axis);
    let parameter = match family {
        NonperiodicConicFamily::Parabola => y / y_scale,
        NonperiodicConicFamily::Hyperbola => (y / y_scale).asinh(),
    };
    let expected_x = match family {
        NonperiodicConicFamily::Parabola => x_scale * parameter * parameter,
        NonperiodicConicFamily::Hyperbola => x_scale * parameter.cosh(),
    };
    (parameter.is_finite() && (x - expected_x).abs() <= 1e-7 * scale).then_some(parameter)
}

pub(super) fn nonperiodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let [Some(first), Some(second)] =
        points.map(|point| nonperiodic_conic_parameter(geometry, point))
    else {
        return None;
    };
    let parameters = if first <= second {
        [first, second]
    } else {
        [second, first]
    };
    (parameters[1] - parameters[0] > 1e-12).then_some(parameters)
}

pub(super) fn periodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
    interior: [f64; 3],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points)
        || !curve_contains_points(geometry, [interior, interior])
    {
        return None;
    }
    let PeriodicConicFrame {
        center,
        x_axis,
        y_axis,
        radii,
        ..
    } = periodic_conic_frame(geometry)?;
    let parameter = |point: [f64; 3]| {
        let relative = std::array::from_fn(|index| point[index] - center[index]);
        (dot(relative, y_axis) / radii[1])
            .atan2(dot(relative, x_axis) / radii[0])
            .rem_euclid(std::f64::consts::TAU)
    };
    let [first, second] = points.map(parameter);
    let increasing = |start: f64, end: f64| {
        [
            start,
            if end < start {
                end + std::f64::consts::TAU
            } else {
                end
            },
        ]
    };
    let first_arc = increasing(first, second);
    let second_arc = if (first - second).abs() <= 1e-12 {
        [first, first + std::f64::consts::TAU]
    } else {
        increasing(second, first)
    };
    let scale = radii.into_iter().fold(1.0, f64::max);
    let matches_interior = |range: [f64; 2]| {
        cadmpeg_ir::eval::curve_point(geometry, f64::midpoint(range[0], range[1])).is_some_and(
            |point| {
                let point = [point.x, point.y, point.z];
                dot(
                    std::array::from_fn(|index| point[index] - interior[index]),
                    std::array::from_fn(|index| point[index] - interior[index]),
                )
                .sqrt()
                    <= 1e-9 * scale
            },
        )
    };
    let selected = match (matches_interior(first_arc), matches_interior(second_arc)) {
        (true, false) => first_arc,
        (false, true) => second_arc,
        _ => return None,
    };
    (selected[1] - selected[0] > 1e-12).then_some(selected)
}

pub(super) fn full_periodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Option<[f64; 2]> {
    curve_contains_points(geometry, [point, point]).then_some(())?;
    let PeriodicConicFrame {
        center,
        x_axis,
        y_axis,
        radii,
        ..
    } = periodic_conic_frame(geometry)?;
    let relative = std::array::from_fn(|index| point[index] - center[index]);
    let start = (dot(relative, y_axis) / radii[1])
        .atan2(dot(relative, x_axis) / radii[0])
        .rem_euclid(std::f64::consts::TAU);
    Some([start, start + std::f64::consts::TAU])
}

pub(super) fn native_pcurve_midpoint(
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
    edge_points: [[f64; 3]; 2],
) -> Option<[f64; 3]> {
    let mapped = endpoints.map(|uv| {
        cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
            .map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    point_pair_alignments([first, second], edge_points)
        .into_iter()
        .any(|matches| matches)
        .then_some(())?;
    let uv = [
        f64::midpoint(endpoints[0][0], endpoints[1][0]),
        f64::midpoint(endpoints[0][1], endpoints[1][1]),
    ];
    cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1]).map(|point| [point.x, point.y, point.z])
}

pub(super) type NativePcurveCandidates = BTreeMap<(u32, u32), Vec<([[f64; 2]; 2], usize)>>;

pub(super) fn pcurve_backed_periodic_conic_parameter_range(
    geometry: &CurveGeometry,
    curve_id: u32,
    faces: [u32; 2],
    candidates: &NativePcurveCandidates,
    surfaces: &[Surface],
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let mut selected = None;
    for face_id in faces {
        let Some(surface) = surfaces
            .iter()
            .find(|surface| surface.id == SurfaceId(format!("creo:visibgeom:surface#{face_id}")))
            .map(|surface| &surface.geometry)
        else {
            continue;
        };
        for (endpoints, _) in candidates.get(&(curve_id, face_id)).into_iter().flatten() {
            let Some(interior) = native_pcurve_midpoint(surface, *endpoints, points) else {
                continue;
            };
            let candidate = periodic_conic_edge_parameter_range(geometry, points, interior)?;
            if selected.is_some_and(|selected: [f64; 2]| {
                candidate
                    .into_iter()
                    .zip(selected)
                    .any(|(candidate, selected)| (candidate - selected).abs() > 1e-9)
            }) {
                return None;
            }
            selected = Some(candidate);
        }
    }
    selected
}

pub(super) fn oriented_native_pcurve_endpoints(
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
    traversal: [[f64; 3]; 2],
) -> Option<[[f64; 2]; 2]> {
    let mapped = endpoints.map(|uv| {
        cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
            .map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    match point_pair_alignments([first, second], traversal) {
        [true, false] => Some(endpoints),
        [false, true] => Some([endpoints[1], endpoints[0]]),
        _ => None,
    }
}

pub(super) fn unique_oriented_native_pcurve(
    surface: &SurfaceGeometry,
    candidates: &[([[f64; 2]; 2], usize)],
    traversal: [[f64; 3]; 2],
) -> Option<([[f64; 2]; 2], usize)> {
    let mut matching = candidates.iter().filter_map(|(endpoints, offset)| {
        oriented_native_pcurve_endpoints(surface, *endpoints, traversal)
            .map(|oriented| (oriented, *offset))
    });
    let mut selected = matching.next()?;
    for candidate in matching {
        if candidate.0 != selected.0 {
            return None;
        }
        selected.1 = selected.1.min(candidate.1);
    }
    Some(selected)
}

pub(super) fn planar_curve_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = surface
    else {
        return None;
    };
    let origin = [origin.x, origin.y, origin.z];
    let normal = normalized([normal.x, normal.y, normal.z])?;
    let u_axis = normalized([u_axis.x, u_axis.y, u_axis.z])?;
    (dot(normal, u_axis).abs() <= 1e-10).then_some(())?;
    let v_axis = normalized(cross(normal, u_axis))?;
    let project_point = |point: [f64; 3], tolerance: f64| {
        let relative: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
        (dot(relative, normal).abs() <= tolerance)
            .then_some(Point2::new(dot(relative, u_axis), dot(relative, v_axis)))
    };
    let project_direction = |direction: [f64; 3]| {
        let length = dot(direction, direction).sqrt();
        (length.is_finite() && length > 0.0 && dot(direction, normal).abs() <= 1e-10 * length)
            .then_some(Point2::new(dot(direction, u_axis), dot(direction, v_axis)))
    };
    let conic_frame = |center: [f64; 3], axis: [f64; 3], x_axis: [f64; 3], scale: f64| {
        let axis = normalized(axis)?;
        let x_axis = normalized(x_axis)?;
        ((dot(axis, normal).abs() - 1.0).abs() <= 1e-10 && dot(axis, x_axis).abs() <= 1e-10)
            .then_some(())?;
        let y_axis = normalized(cross(axis, x_axis))?;
        Some((
            project_point(center, 1e-9 * scale.max(1.0))?,
            project_direction(x_axis)?,
            project_direction(y_axis)?,
        ))
    };

    match geometry {
        CurveGeometry::Line { origin, direction } => {
            let direction = [direction.x, direction.y, direction.z];
            Some(PcurveGeometry::Line {
                origin: project_point([origin.x, origin.y, origin.z], 1e-9)?,
                direction: project_direction(direction)?,
            })
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => {
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
                *radius,
            )?;
            Some(PcurveGeometry::Circle {
                center,
                x_axis,
                y_axis,
                radius: *radius,
            })
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                [axis.x, axis.y, axis.z],
                [major_direction.x, major_direction.y, major_direction.z],
                major_radius.max(*minor_radius),
            )?;
            Some(PcurveGeometry::Ellipse {
                center,
                x_axis,
                y_axis,
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            })
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } if focal_distance.is_finite() && *focal_distance > 0.0 => {
            let (vertex, x_axis, y_axis) = conic_frame(
                [vertex.x, vertex.y, vertex.z],
                [axis.x, axis.y, axis.z],
                [major_direction.x, major_direction.y, major_direction.z],
                *focal_distance,
            )?;
            Some(PcurveGeometry::Parabola {
                vertex,
                x_axis,
                y_axis,
                focal_distance: *focal_distance,
            })
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                [axis.x, axis.y, axis.z],
                [major_direction.x, major_direction.y, major_direction.z],
                major_radius.max(*minor_radius),
            )?;
            Some(PcurveGeometry::Hyperbola {
                center,
                x_axis,
                y_axis,
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            })
        }
        CurveGeometry::Nurbs(nurbs) => {
            nurbs_intrinsic_parameter_range(nurbs)?;
            nurbs
                .weights
                .as_ref()
                .is_none_or(|weights| weights.iter().all(|weight| weight.is_finite()))
                .then_some(())?;
            let tolerance = 1e-9 * nurbs_control_extent(nurbs)?;
            let control_points = nurbs
                .control_points
                .iter()
                .map(|point| project_point([point.x, point.y, point.z], tolerance))
                .collect::<Option<Vec<_>>>()?;
            Some(PcurveGeometry::Nurbs {
                degree: nurbs.degree,
                knots: nurbs.knots.clone(),
                control_points,
                weights: nurbs.weights.clone(),
                periodic: nurbs.periodic,
            })
        }
        _ => None,
    }
}

pub(super) fn stored_unit_vector(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(vector, vector).sqrt();
    (length.is_finite() && (length - 1.0).abs() <= 1e-10).then_some(vector)
}

pub(super) fn surface_of_revolution_parallel_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let (center, conic_axis, conic_x, conic_radii) = match geometry {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => {
            (*center, *axis, *ref_direction, [*radius, *radius])
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            (
                *center,
                *axis,
                *major_direction,
                [*major_radius, *minor_radius],
            )
        }
        _ => return None,
    };
    let (origin, axis, ref_direction) = match surface {
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => (*origin, *axis, *ref_direction),
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if radius.is_finite() && ratio.is_finite() && *ratio > 0.0 && half_angle.is_finite() => {
            half_angle.tan().is_finite().then_some(())?;
            (*origin, *axis, *ref_direction)
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => (*center, *axis, *ref_direction),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            (*center, *axis, *ref_direction)
        }
        _ => return None,
    };
    let surface_axis = stored_unit_vector([axis.x, axis.y, axis.z])?;
    let surface_x = stored_unit_vector([ref_direction.x, ref_direction.y, ref_direction.z])?;
    (dot(surface_axis, surface_x).abs() <= 1e-10).then_some(())?;
    let surface_y = cross(surface_axis, surface_x);
    let conic_axis = stored_unit_vector([conic_axis.x, conic_axis.y, conic_axis.z])?;
    let conic_x = stored_unit_vector([conic_x.x, conic_x.y, conic_x.z])?;
    (dot(conic_axis, conic_x).abs() <= 1e-10
        && (dot(conic_axis, surface_axis).abs() - 1.0).abs() <= 1e-10)
        .then_some(())?;
    let conic_y = cross(conic_axis, conic_x);
    let center_relative = [
        center.x - origin.x,
        center.y - origin.y,
        center.z - origin.z,
    ];
    let axial = dot(center_relative, surface_axis);
    let center_radial = std::array::from_fn::<_, 3, _>(|index| {
        center_relative[index] - axial * surface_axis[index]
    });
    let (v, surface_radii) = match surface {
        SurfaceGeometry::Cylinder { radius, .. } => (axial, [*radius, *radius]),
        SurfaceGeometry::Cone {
            radius,
            ratio,
            half_angle,
            ..
        } => {
            let local_radius = radius + axial * half_angle.tan();
            (axial, [local_radius, local_radius * ratio])
        }
        SurfaceGeometry::Sphere { radius, .. } => {
            ((conic_radii[0] - conic_radii[1]).abs()
                <= 1e-9 * conic_radii.into_iter().fold(1.0, f64::max))
            .then_some(())?;
            let scale = radius.abs().max(conic_radii[0]).max(1.0);
            ((axial.mul_add(axial, conic_radii[0] * conic_radii[0]) - radius * radius).abs()
                <= 1e-9 * scale * scale)
                .then_some(())?;
            let polar = axial.atan2(conic_radii[0]);
            let ring = radius * polar.cos();
            (polar, [ring, ring])
        }
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } => {
            ((conic_radii[0] - conic_radii[1]).abs()
                <= 1e-9 * conic_radii.into_iter().fold(1.0, f64::max))
            .then_some(())?;
            let candidates = [conic_radii[0], -conic_radii[0]]
                .into_iter()
                .filter_map(|ring| {
                    let sine = axial / minor_radius;
                    let cosine = (ring - major_radius) / minor_radius;
                    ((sine.mul_add(sine, cosine * cosine) - 1.0).abs() <= 1e-9)
                        .then_some((sine.atan2(cosine), ring))
                })
                .collect::<Vec<_>>();
            let [candidate] = candidates.as_slice() else {
                return None;
            };
            (candidate.0, [candidate.1, candidate.1])
        }
        _ => unreachable!(),
    };
    let scale = surface_radii
        .into_iter()
        .chain(conic_radii)
        .map(f64::abs)
        .fold(1.0, f64::max);
    (dot(center_radial, center_radial).sqrt() <= 1e-9 * scale
        && surface_radii
            .iter()
            .all(|radius| radius.abs() > 1e-12 * scale)
        && surface_radii
            .into_iter()
            .map(f64::abs)
            .zip(conic_radii)
            .all(|(surface_radius, conic_radius)| {
                (surface_radius - conic_radius).abs() <= 1e-9 * scale
            }))
    .then_some(())?;
    let radius_sign = surface_radii[0].signum();
    let phase =
        (radius_sign * dot(conic_x, surface_y)).atan2(radius_sign * dot(conic_x, surface_x));
    let surface_tangent = std::array::from_fn::<_, 3, _>(|index| {
        -phase.sin() * surface_x[index] + phase.cos() * surface_y[index]
    });
    let orientation = radius_sign * dot(conic_y, surface_tangent);
    ((orientation.abs() - 1.0).abs() <= 1e-10).then_some(())?;
    Some(PcurveGeometry::Line {
        origin: Point2::new(phase, v),
        direction: Point2::new(orientation.signum(), 0.0),
    })
}

pub(super) fn meridian_circle_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let (surface_center, surface_axis, surface_x, major_radius, meridian_radius) = match surface {
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => (*center, *axis, *ref_direction, None, *radius),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            (
                *center,
                *axis,
                *ref_direction,
                Some(*major_radius),
                *minor_radius,
            )
        }
        _ => return None,
    };
    let CurveGeometry::Circle {
        center: circle_center,
        axis: circle_axis,
        ref_direction: circle_x,
        radius: circle_radius,
    } = geometry
    else {
        return None;
    };
    (circle_radius.is_finite() && *circle_radius > 0.0).then_some(())?;
    let surface_axis = stored_unit_vector([surface_axis.x, surface_axis.y, surface_axis.z])?;
    let surface_x = stored_unit_vector([surface_x.x, surface_x.y, surface_x.z])?;
    (dot(surface_axis, surface_x).abs() <= 1e-10).then_some(())?;
    let surface_y = cross(surface_axis, surface_x);
    let circle_axis = stored_unit_vector([circle_axis.x, circle_axis.y, circle_axis.z])?;
    let circle_x = stored_unit_vector([circle_x.x, circle_x.y, circle_x.z])?;
    (dot(circle_axis, circle_x).abs() <= 1e-10).then_some(())?;
    let circle_y = cross(circle_axis, circle_x);
    let center_relative = [
        circle_center.x - surface_center.x,
        circle_center.y - surface_center.y,
        circle_center.z - surface_center.z,
    ];
    let scale = major_radius
        .unwrap_or(0.0)
        .abs()
        .max(meridian_radius.abs())
        .max(circle_radius.abs())
        .max(1.0);
    ((circle_radius - meridian_radius).abs() <= 1e-9 * scale).then_some(())?;
    let radial = if let Some(major_radius) = major_radius {
        let axial = dot(center_relative, surface_axis);
        let radial = std::array::from_fn::<_, 3, _>(|index| {
            center_relative[index] - axial * surface_axis[index]
        });
        let radial_length = dot(radial, radial).sqrt();
        (axial.abs() <= 1e-9 * scale && (radial_length - major_radius).abs() <= 1e-9 * scale)
            .then_some(())?;
        radial.map(|coordinate| coordinate / radial_length)
    } else {
        (dot(center_relative, center_relative).sqrt() <= 1e-9 * scale).then_some(())?;
        let radial = cross(circle_axis, surface_axis);
        stored_unit_vector(radial)?
    };
    let meridian_normal = cross(surface_axis, radial);
    ((dot(circle_axis, meridian_normal).abs() - 1.0).abs() <= 1e-10).then_some(())?;
    let u = dot(radial, surface_y).atan2(dot(radial, surface_x));
    let phase = dot(circle_x, surface_axis).atan2(dot(circle_x, radial));
    let surface_tangent = std::array::from_fn::<_, 3, _>(|index| {
        -phase.sin() * radial[index] + phase.cos() * surface_axis[index]
    });
    let orientation = dot(circle_y, surface_tangent);
    ((orientation.abs() - 1.0).abs() <= 1e-10).then_some(())?;
    Some(PcurveGeometry::Line {
        origin: Point2::new(u, phase),
        direction: Point2::new(0.0, orientation.signum()),
    })
}

pub(super) fn ruled_generator_line_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let CurveGeometry::Line {
        origin: line_origin,
        direction: line_direction,
    } = geometry
    else {
        return None;
    };
    let (surface_origin, surface_axis, surface_x, reference_radius, radius_ratio, radius_slope) =
        match surface {
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } if radius.is_finite() && *radius > 0.0 => {
                (*origin, *axis, *ref_direction, *radius, 1.0, 0.0)
            }
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } if radius.is_finite()
                && ratio.is_finite()
                && *ratio > 0.0
                && half_angle.is_finite() =>
            {
                let slope = half_angle.tan();
                slope.is_finite().then_some((
                    *origin,
                    *axis,
                    *ref_direction,
                    *radius,
                    *ratio,
                    slope,
                ))?
            }
            _ => return None,
        };
    let surface_axis = stored_unit_vector([surface_axis.x, surface_axis.y, surface_axis.z])?;
    let surface_x = stored_unit_vector([surface_x.x, surface_x.y, surface_x.z])?;
    (dot(surface_axis, surface_x).abs() <= 1e-10).then_some(())?;
    let surface_y = cross(surface_axis, surface_x);
    let relative = [
        line_origin.x - surface_origin.x,
        line_origin.y - surface_origin.y,
        line_origin.z - surface_origin.z,
    ];
    let v = dot(relative, surface_axis);
    let radial = std::array::from_fn::<_, 3, _>(|index| relative[index] - v * surface_axis[index]);
    let local_radius = reference_radius + v * radius_slope;
    let scale = local_radius
        .abs()
        .max((local_radius * radius_ratio).abs())
        .max(1.0);
    (local_radius.abs() > 1e-12 * scale).then_some(())?;
    let chart_x = dot(radial, surface_x) / local_radius;
    let chart_y = dot(radial, surface_y) / (local_radius * radius_ratio);
    (chart_x.is_finite()
        && chart_y.is_finite()
        && (chart_x.mul_add(chart_x, chart_y * chart_y) - 1.0).abs() <= 1e-9)
        .then_some(())?;
    let u = chart_y.atan2(chart_x);
    let chart_radial = std::array::from_fn::<_, 3, _>(|index| {
        chart_x * surface_x[index] + radius_ratio * chart_y * surface_y[index]
    });
    let surface_derivative = std::array::from_fn::<_, 3, _>(|index| {
        surface_axis[index] + radius_slope * chart_radial[index]
    });
    let line_direction = [line_direction.x, line_direction.y, line_direction.z];
    let direction_length = dot(line_direction, line_direction).sqrt();
    let derivative_norm = dot(surface_derivative, surface_derivative);
    (direction_length.is_finite()
        && direction_length > 0.0
        && derivative_norm.is_finite()
        && derivative_norm > 0.0)
        .then_some(())?;
    let parameter_scale = dot(line_direction, surface_derivative) / derivative_norm;
    let residual = std::array::from_fn::<_, 3, _>(|index| {
        line_direction[index] - parameter_scale * surface_derivative[index]
    });
    (parameter_scale.is_finite()
        && parameter_scale.abs() > 0.0
        && dot(residual, residual).sqrt() <= 1e-10 * direction_length)
        .then_some(())?;
    Some(PcurveGeometry::Line {
        origin: Point2::new(u, v),
        direction: Point2::new(0.0, parameter_scale),
    })
}
