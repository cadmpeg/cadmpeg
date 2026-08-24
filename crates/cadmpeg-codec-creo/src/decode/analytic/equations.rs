// SPDX-License-Identifier: Apache-2.0
//! Carrier equation types and vector/quadric/conic algebra.

use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::vecmath::normalized;
pub(crate) use crate::vecmath::{cross, dot};

use super::planes::point_on_carrier;

const EPS_PLANE_RESIDUAL: f64 = 1.0e-6;
const EPS_CONIC_RESIDUAL: f64 = 1.0e-8;
const EPS_ROOT_CLUSTER: f64 = 1.0e-7;
const EPS_PARAM_UNIQUE: f64 = 1.0e-7;
const EPS_AGREE: f64 = 1.0e-9;
const EPS_ORTHO: f64 = 1.0e-10;
const EPS_POLY_ROOT_VALUE: f64 = 1.0e-11;
const EPS_NEAR_ZERO: f64 = 1.0e-12;

#[derive(Clone, Copy)]
pub struct PlaneEquation {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
}

#[derive(Clone, Copy)]
pub struct CylinderEquation {
    pub origin: [f64; 3],
    pub axis: [f64; 3],
    pub ref_direction: [f64; 3],
    pub radius: f64,
}

#[derive(Clone, Copy)]
pub struct ConeEquation {
    pub origin: [f64; 3],
    pub axis: [f64; 3],
    pub ref_direction: [f64; 3],
    pub radius: f64,
    pub ratio: f64,
    pub half_angle: f64,
}

pub fn circular_cone(cone: ConeEquation) -> bool {
    cone.ratio.is_finite() && (cone.ratio - 1.0).abs() <= EPS_NEAR_ZERO
}

#[derive(Clone, Copy)]
pub struct SphereEquation {
    pub center: [f64; 3],
    pub ref_direction: [f64; 3],
    pub radius: f64,
}

#[derive(Clone, Copy)]
pub struct TorusEquation {
    pub center: [f64; 3],
    pub axis: [f64; 3],
    pub ref_direction: [f64; 3],
    pub major_radius: f64,
    pub minor_radius: f64,
}

#[derive(Clone, Copy)]
pub enum CarrierEquation {
    Plane(PlaneEquation),
    Cylinder(CylinderEquation),
    Cone(ConeEquation),
    Sphere(SphereEquation),
    Torus(TorusEquation),
}

#[derive(Clone, Copy)]
pub struct QuadricEquation {
    pub matrix: [[f64; 3]; 3],
    pub linear: [f64; 3],
    pub constant: f64,
}

#[derive(Clone, Copy)]
pub struct PlaneConicEquation {
    pub uu: f64,
    pub uv: f64,
    pub vv: f64,
    pub u: f64,
    pub v: f64,
    pub constant: f64,
}

pub fn matrix_vector(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| dot(row, vector))
}

pub fn outer_product(left: [f64; 3], right: [f64; 3]) -> [[f64; 3]; 3] {
    left.map(|left| right.map(|right| left * right))
}

pub fn carrier_quadric(carrier: CarrierEquation) -> Option<QuadricEquation> {
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
            if dot(axis, x_axis).abs() > EPS_ORTHO
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

pub fn restrict_quadric_to_plane(
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

pub fn solve_planes(planes: &[PlaneEquation]) -> Option<[f64; 3]> {
    for first in 0..planes.len() {
        for second in first + 1..planes.len() {
            for third in second + 1..planes.len() {
                let a = planes[first];
                let b = planes[second];
                let c = planes[third];
                let b_cross_c = cross(b.normal, c.normal);
                let determinant = dot(a.normal, b_cross_c);
                if determinant.abs() <= EPS_AGREE {
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
                        (dot(plane.normal, point) - dot(plane.normal, plane.origin)).abs()
                            <= EPS_PLANE_RESIDUAL
                    })
                {
                    return Some(point);
                }
            }
        }
    }
    None
}

pub fn plane_intersection_line(
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

pub fn intersect_two_planes_with_quadric(
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

pub fn polynomial_value(coefficients: &[f64], parameter: f64) -> f64 {
    coefficients.iter().rev().fold(0.0, |value, coefficient| {
        value.mul_add(parameter, *coefficient)
    })
}

pub fn real_polynomial_roots(coefficients: &[f64]) -> Vec<f64> {
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
    let value_tolerance = EPS_POLY_ROOT_VALUE;
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
                let tolerance = EPS_ROOT_CLUSTER * previous.abs().max(root.abs()).max(1.0);
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

pub fn polynomial_product(first: &[f64], second: &[f64]) -> Vec<f64> {
    let mut product = vec![0.0; first.len() + second.len() - 1];
    for (first_power, first_coefficient) in first.iter().enumerate() {
        for (second_power, second_coefficient) in second.iter().enumerate() {
            product[first_power + second_power] += first_coefficient * second_coefficient;
        }
    }
    product
}

pub const QUARTIC_RESULTANT_PERMUTATIONS: [([usize; 4], f64); 24] = [
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

pub fn conic_resultant(first: PlaneConicEquation, second: PlaneConicEquation) -> Vec<f64> {
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

pub fn quadratic_real_roots(quadratic: f64, linear: f64, constant: f64) -> Vec<f64> {
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
    if discriminant < -EPS_NEAR_ZERO * scale * scale {
        return Vec::new();
    }
    let root = if discriminant.abs() <= EPS_NEAR_ZERO * scale * scale {
        0.0
    } else {
        discriminant.sqrt()
    };
    let mut roots = vec![(-linear - root) / (2.0 * quadratic)];
    if root > EPS_NEAR_ZERO * scale {
        roots.push((-linear + root) / (2.0 * quadratic));
    }
    roots
}

pub fn plane_conic_value(conic: PlaneConicEquation, u: f64, v: f64) -> f64 {
    conic.uu * u * u
        + conic.uv * u * v
        + conic.vv * v * v
        + conic.u * u
        + conic.v * v
        + conic.constant
}

pub fn refine_plane_conic_intersection(
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

pub fn common_plane_conic_parameters(
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
            let tolerance = EPS_CONIC_RESIDUAL * coefficient_scale * scale * scale;
            if plane_conic_value(first, candidate[0], candidate[1]).abs() <= tolerance
                && plane_conic_value(second, candidate[0], candidate[1]).abs() <= tolerance
                && !parameters.iter().any(|known| {
                    (known[0] - candidate[0])
                        .abs()
                        .max((known[1] - candidate[1]).abs())
                        <= EPS_PARAM_UNIQUE * scale
                })
            {
                parameters.push(candidate);
            }
        }
    }
    parameters
}

pub fn intersect_plane_with_two_quadrics(
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

pub fn intersect_two_planes_with_torus(
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

pub fn intersect_plane_with_circle(
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
    if remaining < -EPS_NEAR_ZERO * scale * scale {
        return Vec::new();
    }
    let parameter_delta = if remaining.abs() <= EPS_NEAR_ZERO * scale * scale {
        0.0
    } else {
        remaining.sqrt() / denominator.sqrt()
    };
    let mut points = vec![std::array::from_fn(|index| {
        nearest[index] - parameter_delta * line_direction[index]
    })];
    if parameter_delta > EPS_NEAR_ZERO * scale {
        points.push(std::array::from_fn(|index| {
            nearest[index] + parameter_delta * line_direction[index]
        }));
    }
    points
}

pub fn circle_parameters(geometry: &CurveGeometry) -> Option<([f64; 3], [f64; 3], f64)> {
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

pub fn plane_cone_conic(
    plane: PlaneEquation,
    cone: ConeEquation,
) -> Option<(CurveGeometry, &'static str)> {
    let normal = normalized(plane.normal)?;
    let axis = normalized(cone.axis)?;
    let x_axis = normalized(cone.ref_direction)?;
    let slope = cone.half_angle.tan();
    if slope <= EPS_NEAR_ZERO
        || !slope.is_finite()
        || cone.radius < 0.0
        || cone.ratio <= 0.0
        || !cone.ratio.is_finite()
        || dot(axis, x_axis).abs() > EPS_ORTHO
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
    if quadratic_u.abs() <= EPS_NEAR_ZERO * coefficient_scale {
        if linear_u.abs() <= EPS_NEAR_ZERO * coefficient_scale {
            return None;
        }
        let vertex_v = -linear_v / (2.0 * quadratic_v);
        let shifted_constant = constant - linear_v * linear_v / (4.0 * quadratic_v);
        let vertex_u = -shifted_constant / linear_u;
        let opening = -linear_u / quadratic_v;
        if opening.abs() <= EPS_NEAR_ZERO || !opening.is_finite() {
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
    if shifted_constant.abs() <= EPS_NEAR_ZERO * value_scale {
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
