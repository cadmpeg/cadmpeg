// SPDX-License-Identifier: Apache-2.0
//! Carrier equation types and vector/quadric/conic algebra.

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::planar::line_circle_parameters;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::decode::quadratic::{cancellation_bound, real_roots, Coefficient};
use crate::vecmath::{cross, dot, normalize};

use super::planes::point_on_carrier;

const EPS_PLANE_CIRCLE_PARALLEL: f64 = 1.0e-9;
const EPS_PLANE_RESIDUAL: f64 = 1.0e-6;
const EPS_PARAM_UNIQUE: f64 = 1.0e-7;
const EPS_AGREE: f64 = 1.0e-9;
const EPS_ORTHO: f64 = 1.0e-10;
const EPS_NEAR_ZERO: f64 = 1.0e-12;

const F64_EXPONENT_MASK: u64 = 0x7ff0_0000_0000_0000;

#[derive(Clone, Copy)]
pub(in crate::decode) struct PlaneEquation {
    pub(in crate::decode) origin: [f64; 3],
    pub(in crate::decode) normal: [f64; 3],
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct CylinderEquation {
    pub(in crate::decode) origin: [f64; 3],
    pub(in crate::decode) axis: [f64; 3],
    pub(in crate::decode) ref_direction: [f64; 3],
    pub(in crate::decode) radius: f64,
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct ConeEquation {
    origin: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    radius: f64,
    ratio: f64,
    half_angle: f64,
}

impl ConeEquation {
    /// A cone with finite radius, positive finite ratio, and half-angle in [0, pi/2).
    ///
    /// The half angle enters the equation only through its tangent, so zero is in the domain:
    /// it makes the quadric the cylinder of `radius`, which every consumer here evaluates.
    /// This is a wider domain than [`crate::surface::valid_apex_cone_half_angle`], which admits
    /// the half angle of an apex cone record, where a zero angle leaves the axis line and no
    /// surface. A consumer that needs a nonzero slope states that itself, as `plane_cone_conic`
    /// does.
    pub(in crate::decode) fn new(
        origin: [f64; 3],
        axis: [f64; 3],
        ref_direction: [f64; 3],
        radius: f64,
        ratio: f64,
        half_angle: f64,
    ) -> Option<Self> {
        (radius.is_finite()
            && ratio.is_finite()
            && ratio > 0.0
            && (0.0..std::f64::consts::FRAC_PI_2).contains(&half_angle))
        .then_some(Self {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        })
    }
    /// The cone reference origin.
    pub(in crate::decode) const fn origin(self) -> [f64; 3] {
        self.origin
    }
    /// The cone axis direction.
    pub(in crate::decode) const fn axis(self) -> [f64; 3] {
        self.axis
    }
    /// The cone reference radial direction.
    pub(in crate::decode) const fn ref_direction(self) -> [f64; 3] {
        self.ref_direction
    }
    /// The radius at the reference origin.
    pub(in crate::decode) const fn radius(self) -> f64 {
        self.radius
    }
    /// The radial aspect ratio.
    pub(in crate::decode) const fn ratio(self) -> f64 {
        self.ratio
    }
    /// The cone half-angle in radians.
    pub(in crate::decode) const fn half_angle(self) -> f64 {
        self.half_angle
    }
}

pub(in crate::decode) fn circular_cone(cone: ConeEquation) -> bool {
    (cone.ratio - 1.0).abs() <= EPS_NEAR_ZERO
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SphereEquation {
    pub(in crate::decode) center: [f64; 3],
    pub(in crate::decode) ref_direction: [f64; 3],
    pub(in crate::decode) radius: f64,
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct TorusEquation {
    pub(in crate::decode) center: [f64; 3],
    pub(in crate::decode) axis: [f64; 3],
    pub(in crate::decode) ref_direction: [f64; 3],
    pub(in crate::decode) major_radius: f64,
    pub(in crate::decode) minor_radius: f64,
}

#[derive(Clone, Copy)]
pub(in crate::decode) enum CarrierEquation {
    Plane(PlaneEquation),
    Cylinder(CylinderEquation),
    Cone(ConeEquation),
    Sphere(SphereEquation),
    Torus(TorusEquation),
}

impl CarrierEquation {
    /// Stable carrier family name for diagnostics.
    pub(super) fn kind_str(&self) -> &'static str {
        match self {
            Self::Plane(_) => "plane",
            Self::Cylinder(_) => "cylinder",
            Self::Cone(_) => "cone",
            Self::Sphere(_) => "sphere",
            Self::Torus(_) => "torus",
        }
    }
}

#[derive(Clone, Copy)]
struct QuadricEquation {
    matrix: [[f64; 3]; 3],
    linear: [f64; 3],
    constant: f64,
}

/// A conic in the chart coordinates `u` and `v`.
///
/// Every coefficient is a sum of products of the restricted surface's or
/// curve's own coefficients, so each one carries the magnitudes of those
/// products beside its value. That is what lets both constructors state zero
/// where a sum cancels inside its rounding bound, and what lets `conic_v_roots`
/// hand `real_roots` the `vv` error bar rather than the value alone.
#[derive(Clone, Copy)]
pub(super) struct PlaneConicEquation {
    pub(super) uu: Coefficient,
    pub(super) uv: Coefficient,
    pub(super) vv: Coefficient,
    pub(super) u: Coefficient,
    pub(super) v: Coefficient,
    pub(super) constant: Coefficient,
}

fn matrix_vector(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| dot(row, vector))
}

fn outer_product(left: [f64; 3], right: [f64; 3]) -> [[f64; 3]; 3] {
    left.map(|left| right.map(|right| left * right))
}

fn abs_dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| (left * right).abs())
        .sum()
}

fn carrier_quadric(carrier: CarrierEquation) -> Option<QuadricEquation> {
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    match carrier {
        CarrierEquation::Cylinder(cylinder) => {
            let axis = normalize(cylinder.axis)?;
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
            let axis = normalize(cone.axis)?;
            let x_axis = normalize(cone.ref_direction)?;
            if dot(axis, x_axis).abs() > EPS_ORTHO {
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

fn restrict_quadric_to_plane(
    quadric: QuadricEquation,
    origin: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
) -> PlaneConicEquation {
    let matrix_origin = matrix_vector(quadric.matrix, origin);
    let matrix_u = matrix_vector(quadric.matrix, u_axis);
    let matrix_v = matrix_vector(quadric.matrix, v_axis);
    PlaneConicEquation {
        uu: Coefficient::summed(dot(u_axis, matrix_u), abs_dot(u_axis, matrix_u)),
        uv: Coefficient::summed(2.0 * dot(u_axis, matrix_v), 2.0 * abs_dot(u_axis, matrix_v)),
        vv: Coefficient::summed(dot(v_axis, matrix_v), abs_dot(v_axis, matrix_v)),
        u: Coefficient::summed(
            2.0 * dot(u_axis, matrix_origin) + dot(quadric.linear, u_axis),
            2.0 * abs_dot(u_axis, matrix_origin) + abs_dot(quadric.linear, u_axis),
        ),
        v: Coefficient::summed(
            2.0 * dot(v_axis, matrix_origin) + dot(quadric.linear, v_axis),
            2.0 * abs_dot(v_axis, matrix_origin) + abs_dot(quadric.linear, v_axis),
        ),
        constant: Coefficient::summed(
            dot(origin, matrix_origin) + dot(quadric.linear, origin) + quadric.constant,
            abs_dot(origin, matrix_origin)
                + abs_dot(quadric.linear, origin)
                + quadric.constant.abs(),
        ),
    }
}

pub(in crate::decode) fn solve_planes(planes: &[PlaneEquation]) -> Option<[f64; 3]> {
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

pub(in crate::decode) fn plane_intersection_line(
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
    Some((origin, normalize(direction)?))
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
    let quadratic = Coefficient::summed(
        dot(direction, matrix_direction),
        abs_dot(direction, matrix_direction),
    );
    let linear = Coefficient::summed(
        2.0 * dot(line_origin, matrix_direction) + dot(quadric.linear, direction),
        2.0 * abs_dot(line_origin, matrix_direction) + abs_dot(quadric.linear, direction),
    );
    let constant = Coefficient::summed(
        dot(line_origin, matrix_origin) + dot(quadric.linear, line_origin) + quadric.constant,
        abs_dot(line_origin, matrix_origin)
            + abs_dot(quadric.linear, line_origin)
            + quadric.constant.abs(),
    );
    real_roots(quadratic, linear, constant)
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

fn polynomial_value(coefficients: &[f64], parameter: f64) -> f64 {
    coefficients.iter().rev().fold(0.0, |value, coefficient| {
        value.mul_add(parameter, *coefficient)
    })
}

fn operation_rounding_bound(value: f64) -> f64 {
    f64::EPSILON * value.abs() + f64::from_bits(1)
}

fn inflate_positive_bound(value: f64) -> f64 {
    value + cancellation_bound(value) + f64::from_bits(1)
}

fn polynomial_value_and_bound(coefficients: &[BoundedCoefficient], parameter: f64) -> (f64, f64) {
    coefficients
        .iter()
        .rev()
        .fold((0.0, 0.0), |(value, bound), coefficient| {
            let next_value = value.mul_add(parameter, coefficient.value);
            let propagated = bound.mul_add(parameter.abs(), coefficient.bound);
            (
                next_value,
                inflate_positive_bound(propagated + operation_rounding_bound(next_value)),
            )
        })
}

fn polynomial_value_bound(coefficients: &[BoundedCoefficient], parameter: f64) -> f64 {
    let magnitude = parameter.abs();
    let (coefficient_error, evaluation_terms, _) = coefficients.iter().fold(
        (0.0, 0.0, 1.0),
        |(coefficient_error, evaluation_terms, power), coefficient| {
            (
                coefficient.bound.mul_add(power, coefficient_error),
                coefficient.value.abs().mul_add(power, evaluation_terms),
                power * magnitude,
            )
        },
    );
    coefficient_error + cancellation_bound(evaluation_terms)
}

fn polynomial_interval_value_bound(
    coefficients: &[BoundedCoefficient],
    parameter: f64,
    parameter_error: f64,
) -> f64 {
    let (_, mut bound) = polynomial_value_and_bound(coefficients, parameter);
    let mut derivative = coefficients.to_vec();
    let mut parameter_error_power = 1.0;
    let mut factorial = 1.0;
    for order in 1..coefficients.len() {
        derivative = derivative
            .iter()
            .enumerate()
            .skip(1)
            .map(|(power, coefficient)| {
                let Ok(power) = u32::try_from(power) else {
                    return BoundedCoefficient {
                        value: f64::INFINITY,
                        bound: f64::INFINITY,
                    };
                };
                let factor = f64::from(power);
                let value = coefficient.value * factor;
                BoundedCoefficient {
                    value,
                    bound: inflate_positive_bound(
                        coefficient.bound * factor + operation_rounding_bound(value),
                    ),
                }
            })
            .collect();
        let Ok(order) = u32::try_from(order) else {
            return f64::INFINITY;
        };
        parameter_error_power *= parameter_error;
        factorial *= f64::from(order);
        let (derivative_value, derivative_bound) =
            polynomial_value_and_bound(&derivative, parameter);
        let term = inflate_positive_bound(
            (derivative_value.abs() + derivative_bound) * parameter_error_power / factorial,
        );
        bound = inflate_positive_bound(bound + term);
    }
    bound
}

fn polynomial_sign(coefficients: &[BoundedCoefficient], parameter: f64) -> Option<bool> {
    let (value, bound) = polynomial_value_and_bound(coefficients, parameter);
    (value.is_finite() && bound.is_finite() && value.abs() > bound)
        .then_some(value.is_sign_positive())
}

/// A polynomial coefficient beside the bound on its distance from the exact
/// coefficient of the exact polynomial.
///
/// The two travel together because the degree of a polynomial formed by
/// cancelling sums is stated by its leading coefficient against that bound.
#[derive(Clone, Copy)]
struct BoundedCoefficient {
    value: f64,
    bound: f64,
}

/// The factor on a coefficient's term magnitudes that bounds its distance from
/// the exact coefficient.
///
/// A product in either polynomial built for `real_polynomial_roots` — the
/// conic resultant and the torus line polynomial — has at most four factors,
/// and each factor is within one `cancellation_bound` of its own exact value,
/// so the bars contribute at most four times that bound against the product of
/// the term magnitudes. The construction's own rounding is three
/// multiplications and at most four additions on each product's path, which is
/// `7 u` against the bound's `128 u`; products of two bars are smaller again by
/// that same ratio. The fifth multiple covers both.
const POLYNOMIAL_ERROR_FACTOR: f64 = 5.0;

/// A real root beside the distance from it inside which the exact root lies,
/// and whether the derivative states zero there as well.
///
/// The three producers of a root state different accuracies, and a caller that
/// reads a coordinate off the root cannot tell them apart from the value. A
/// bisected root carries the polynomial value's bound divided by the lower
/// magnitude of its derivative, as well as the final bracket width. `multiple`
/// marks a root the polynomial shares with its derivative, which is a root of
/// even order: two intersections that coincide rather than two that are apart.
#[derive(Clone, Copy)]
struct PolynomialRoot {
    value: f64,
    error: f64,
    multiple: bool,
}

/// Return the finite real roots in ascending order.
///
/// The degree is stated by each leading coefficient against its own bound: a
/// coefficient inside that bound is the rounding residue of a cancellation and
/// the polynomial is of lower degree. A residue kept as a leading coefficient
/// states roots of order `1 / residue` that no exact polynomial has; a real
/// leading coefficient dropped loses the roots it carries.
fn real_polynomial_roots(coefficients: &[BoundedCoefficient]) -> Vec<PolynomialRoot> {
    let largest = coefficients
        .iter()
        .map(|coefficient| coefficient.value.abs())
        .fold(0.0, f64::max);
    if largest == 0.0 || !largest.is_finite() {
        return Vec::new();
    }
    // Division by the power of two at the largest coefficient is exact. The
    // largest quotient remains below two for a normal coefficient and below
    // one when every coefficient is subnormal, so the normalization cannot
    // overflow and does not discard a coefficient's significand.
    let normal_scale = f64::from_bits(largest.to_bits() & F64_EXPONENT_MASK);
    let scale = if normal_scale == 0.0 {
        f64::MIN_POSITIVE
    } else {
        normal_scale
    };
    let mut scaled = coefficients
        .iter()
        .map(|coefficient| BoundedCoefficient {
            value: coefficient.value / scale,
            bound: coefficient.bound / scale,
        })
        .collect::<Vec<_>>();
    while scaled.len() > 1
        && scaled
            .last()
            .is_some_and(|coefficient| coefficient.value.abs() <= coefficient.bound)
    {
        scaled.pop();
    }
    let degree = scaled.len() - 1;
    if degree == 0 {
        return Vec::new();
    }
    let coefficients = scaled
        .iter()
        .map(|coefficient| coefficient.value)
        .collect::<Vec<_>>();
    if degree == 1 {
        // The pop loop stopped because the leading coefficient is outside its
        // own bound, so the difference below is positive. The exact root is
        // `-(c0 + e0)/(c1 + e1)` for some `|e| <= bound`, which is within
        // `(b0 + |root| b1)/(|c1| - b1)` of the stated one, and the division
        // itself rounds once.
        let value = -coefficients[0] / coefficients[1];
        let error = (scaled[0].bound + value.abs() * scaled[1].bound)
            / (coefficients[1].abs() - scaled[1].bound)
            + cancellation_bound(value);
        return vec![PolynomialRoot {
            value,
            error,
            multiple: false,
        }];
    }
    let derivative = scaled
        .iter()
        .enumerate()
        .skip(1)
        .map(|(power, coefficient)| BoundedCoefficient {
            value: coefficient.value * power as f64,
            bound: coefficient.bound * power as f64,
        })
        .collect::<Vec<_>>();
    let leading = coefficients[degree].abs();
    let bound = 1.0
        + coefficients[..degree]
            .iter()
            .copied()
            .map(f64::abs)
            .fold(0.0, f64::max)
            / leading;
    // Each derivative root partitions the polynomial into monotone intervals.
    // Its reported location interval, rather than its approximate value, is
    // removed from those intervals before their endpoint signs are compared.
    let derivative_roots = real_polynomial_roots(&derivative)
        .into_iter()
        .filter(|root| root.value.is_finite() && root.value > -bound && root.value < bound)
        .map(|root| PolynomialRoot {
            multiple: true,
            ..root
        })
        .collect::<Vec<_>>();
    let derivative_values = derivative
        .iter()
        .map(|coefficient| coefficient.value)
        .collect::<Vec<_>>();
    let mut roots = Vec::new();
    let mut gap_lower = -bound;
    let mut gap_lower_sign = polynomial_sign(&scaled, gap_lower);
    let mut gaps = Vec::new();
    for station in derivative_roots {
        let station_lower = station.value - station.error;
        let station_upper = station.value + station.error;
        if gap_lower < station_lower {
            gaps.push((
                gap_lower,
                gap_lower_sign,
                station_lower,
                polynomial_sign(&scaled, station_lower),
            ));
        }
        let (station_value, _) = polynomial_value_and_bound(&scaled, station.value);
        let station_value_bound =
            polynomial_interval_value_bound(&scaled, station.value, station.error);
        if station_value.is_finite()
            && station_value_bound.is_finite()
            && station_value.abs() <= station_value_bound
        {
            roots.push(station);
        }
        gap_lower = gap_lower.max(station_upper);
        gap_lower_sign = polynomial_sign(&scaled, gap_lower);
    }
    gaps.push((
        gap_lower,
        gap_lower_sign,
        bound,
        polynomial_sign(&scaled, bound),
    ));
    for (mut lower, lower_sign, mut upper, upper_sign) in gaps {
        let (Some(mut lower_sign), Some(upper_sign)) = (lower_sign, upper_sign) else {
            continue;
        };
        if lower_sign == upper_sign {
            continue;
        }
        let complete_lower = lower;
        let complete_upper = upper;
        for _ in 0..80 {
            let midpoint = 0.5 * (lower + upper);
            let midpoint_sign = polynomial_sign(&scaled, midpoint)
                .unwrap_or_else(|| polynomial_value(&coefficients, midpoint).is_sign_positive());
            if lower_sign == midpoint_sign {
                lower = midpoint;
                lower_sign = midpoint_sign;
            } else {
                upper = midpoint;
            }
        }
        let value = 0.5 * (lower + upper);
        let derivative_value = polynomial_value(&derivative_values, value).abs();
        let derivative_error = polynomial_value_bound(&derivative, value);
        // The value bound divided by the derivative magnitude outside its own
        // bound is the displacement that coefficient and evaluation error can
        // give this simple root. If the derivative does not state a nonzero
        // slope, the complete gap between derivative-station intervals is the
        // location statement.
        let coefficient_error = if derivative_value > derivative_error {
            polynomial_value_bound(&scaled, value) / (derivative_value - derivative_error)
        } else {
            (value - complete_lower).max(complete_upper - value)
        };
        roots.push(PolynomialRoot {
            value,
            error: (0.5 * (upper - lower).abs())
                .max(coefficient_error)
                .max(cancellation_bound(value)),
            multiple: false,
        });
    }
    roots.sort_by(|left, right| left.value.total_cmp(&right.value));
    roots
}

fn polynomial_product(first: &[f64], second: &[f64]) -> Vec<f64> {
    let Some(count) = first
        .len()
        .checked_add(second.len())
        .and_then(|len| len.checked_sub(1))
    else {
        return Vec::new();
    };
    let Ok(mut product) = alloc_filled(count, 0.0, "creo polynomial product") else {
        return Vec::new();
    };
    for (first_power, first_coefficient) in first.iter().enumerate() {
        for (second_power, second_coefficient) in second.iter().enumerate() {
            product[first_power + second_power] += first_coefficient * second_coefficient;
        }
    }
    product
}

const QUARTIC_RESULTANT_PERMUTATIONS: [([usize; 4], f64); 24] = [
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

/// The Sylvester matrix of the two conics read as quadratics in v, with each
/// entry taken from its coefficient by `entry`.
///
/// Four entries are the shape's own zeros rather than a polynomial that
/// happens to vanish, so they are `None` and the permutations that select them
/// contribute nothing. That is what bounds the determinant's degree: a
/// permutation with no `None` takes column 3 from row 1 or row 3 and column 0
/// from row 0 or row 2, which leaves eight permutations, each of total degree
/// four. The permutations that do select a `None` reach total degree five.
fn sylvester_matrix(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
    entry: impl Fn(Coefficient) -> f64,
) -> [[Option<Vec<f64>>; 4]; 4] {
    let zero = None;
    let first_y2 = vec![entry(first.vv)];
    let first_y = vec![entry(first.v), entry(first.uv)];
    let first_constant = vec![entry(first.constant), entry(first.u), entry(first.uu)];
    let second_y2 = vec![entry(second.vv)];
    let second_y = vec![entry(second.v), entry(second.uv)];
    let second_constant = vec![entry(second.constant), entry(second.u), entry(second.uu)];
    [
        [
            Some(first_y2.clone()),
            Some(first_y.clone()),
            Some(first_constant.clone()),
            zero.clone(),
        ],
        [
            zero.clone(),
            Some(first_y2),
            Some(first_y),
            Some(first_constant),
        ],
        [
            Some(second_y2.clone()),
            Some(second_y.clone()),
            Some(second_constant.clone()),
            zero.clone(),
        ],
        [zero, Some(second_y2), Some(second_y), Some(second_constant)],
    ]
}

/// The determinant of the Sylvester matrix as a polynomial in u.
///
/// `sign` is the identity for the determinant itself and `f64::abs` for the
/// sum of the magnitudes of the same products, which is what a matrix of term
/// magnitudes folds to.
///
/// The length of the returned vector is the degree the construction reaches
/// plus one. It is stated by the fold rather than allocated ahead of it: each
/// contributing permutation's product extends the determinant to its own
/// length. For two plane conics that length is five, which
/// `conic_resultant_is_a_quartic` pins.
fn sylvester_polynomial(
    matrix: &[[Option<Vec<f64>>; 4]; 4],
    sign: impl Fn(f64) -> f64,
) -> Vec<f64> {
    let mut determinant = Vec::new();
    for (permutation, permutation_sign) in QUARTIC_RESULTANT_PERMUTATIONS {
        let Some(term) = (0..4).try_fold(vec![1.0], |term, row| {
            Some(polynomial_product(
                &term,
                matrix[row][permutation[row]].as_deref()?,
            ))
        }) else {
            continue;
        };
        if determinant.len() < term.len() {
            determinant.resize(term.len(), 0.0);
        }
        for (entry, coefficient) in determinant.iter_mut().zip(term) {
            *entry += sign(permutation_sign) * coefficient;
        }
    }
    determinant
}

fn conic_resultant(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
) -> Vec<BoundedCoefficient> {
    let values = sylvester_polynomial(
        &sylvester_matrix(first, second, Coefficient::stated),
        |sign| sign,
    );
    let terms = sylvester_polynomial(
        &sylvester_matrix(first, second, Coefficient::terms),
        f64::abs,
    );
    values
        .into_iter()
        .zip(terms)
        .map(|(value, terms)| BoundedCoefficient {
            value,
            bound: POLYNOMIAL_ERROR_FACTOR * cancellation_bound(terms),
        })
        .collect()
}

fn plane_conic_value(conic: PlaneConicEquation, u: f64, v: f64) -> f64 {
    conic.uu.stated() * u * u
        + conic.uv.stated() * u * v
        + conic.vv.stated() * v * v
        + conic.u.stated() * u
        + conic.v.stated() * v
        + conic.constant.stated()
}

/// A refined chart parameter pair beside the last correction the refinement
/// applied to it.
///
/// A Newton step is the distance from the pair to the root of the linearised
/// system, so once the step is applied and met the refinement's convergence
/// rule what remains is of that step's own order. That is what the pair is
/// known to. A pair the refinement left without a converged step — a singular
/// Jacobian, or twelve steps that never settled — carries `None`, and its only
/// accuracy is the arithmetic that produced the coefficients.
#[derive(Clone, Copy)]
struct RefinedParameters {
    point: [f64; 2],
    correction: Option<[f64; 2]>,
}

/// The term magnitudes `plane_conic_value` sums at a parameter pair: each
/// coefficient's own terms scaled by the monomial it multiplies.
fn plane_conic_terms(conic: PlaneConicEquation, u: f64, v: f64) -> f64 {
    conic.uu.terms() * u * u
        + conic.uv.terms() * (u * v).abs()
        + conic.vv.terms() * v * v
        + conic.u.terms() * u.abs()
        + conic.v.terms() * v.abs()
        + conic.constant.terms()
}

/// The bound on the distance between `plane_conic_value` and the exact conic's
/// value at the same pair.
///
/// Each coefficient's own bound reaches the value scaled by the monomial it
/// multiplies, which over the six is one `cancellation_bound` of the weighted
/// term magnitudes. The evaluation is six products and five additions over
/// values no larger than those same weighted terms, which is `17 u` against
/// the bound's `128 u`, so a second `cancellation_bound` covers it.
fn plane_conic_value_bound(conic: PlaneConicEquation, u: f64, v: f64) -> f64 {
    2.0 * cancellation_bound(plane_conic_terms(conic, u, v))
}

/// The gradient of a chart conic at a parameter pair, beside the term
/// magnitudes each of its two entries was summed from.
fn plane_conic_gradient(conic: PlaneConicEquation, u: f64, v: f64) -> ([f64; 2], [f64; 2]) {
    (
        [
            2.0 * conic.uu.stated() * u + conic.uv.stated() * v + conic.u.stated(),
            conic.uv.stated() * u + 2.0 * conic.vv.stated() * v + conic.v.stated(),
        ],
        [
            2.0 * conic.uu.terms() * u.abs() + conic.uv.terms() * v.abs() + conic.u.terms(),
            conic.uv.terms() * u.abs() + 2.0 * conic.vv.terms() * v.abs() + conic.v.terms(),
        ],
    )
}

/// The bound on `values[0] * values[1] - values[2] * values[3]` where each
/// factor is within its own entry of `bounds` of the exact one.
///
/// The bars reach the difference through the other factor of their product and
/// through each other, and the arithmetic itself is two multiplications and one
/// fused difference, which is `3 u` against the `cancellation_bound`'s `128 u`.
fn product_difference_bound(values: [f64; 4], bounds: [f64; 4]) -> f64 {
    let [first, second, third, fourth] = values;
    let [first_bound, second_bound, third_bound, fourth_bound] = bounds;
    first.abs() * second_bound
        + second.abs() * first_bound
        + first_bound * second_bound
        + third.abs() * fourth_bound
        + fourth.abs() * third_bound
        + third_bound * fourth_bound
        + cancellation_bound((first * second).abs() + (third * fourth).abs())
}

/// A pair of equations in the chart parameters, each quantity beside the bound
/// on its distance from the exact one.
struct NewtonSystem {
    values: [f64; 2],
    value_bounds: [f64; 2],
    jacobian: [[f64; 2]; 2],
    jacobian_bounds: [[f64; 2]; 2],
}

/// The steps a refinement takes before it states that the iteration has not
/// settled.
const REFINEMENT_STEPS: usize = 12;

/// Newton's method on a pair of equations that state their own accuracy.
///
/// Two decisions are taken each step and both are read against the arithmetic
/// that formed the quantity they judge.
///
/// The determinant of the Jacobian is a difference of two products of entries
/// that are themselves three-term sums, so it states singular inside
/// `product_difference_bound`: past that the linearised system has no stated
/// solution and the iteration stops.
///
/// The step is that difference of products over the determinant, so the step
/// states zero exactly when its numerator does. The numerator's bound comes
/// from the two residuals' own bounds, which is the floor the iteration can
/// reach: past it a further step is the rounding of the residuals rather than
/// a correction. That is convergence, and the step that reached it is what the
/// pair is known to.
fn newton_refine(
    system: impl Fn(f64, f64) -> NewtonSystem,
    mut u: f64,
    mut v: f64,
) -> RefinedParameters {
    let mut correction = None;
    for _ in 0..REFINEMENT_STEPS {
        let NewtonSystem {
            values: [first_value, second_value],
            value_bounds: [first_bound, second_bound],
            jacobian: [[first_u, first_v], [second_u, second_v]],
            jacobian_bounds: [[bound_first_u, bound_first_v], [bound_second_u, bound_second_v]],
        } = system(u, v);
        let determinant = first_u.mul_add(second_v, -(first_v * second_u));
        let determinant_bound = product_difference_bound(
            [first_u, second_v, first_v, second_u],
            [bound_first_u, bound_second_v, bound_first_v, bound_second_u],
        );
        if determinant.abs() <= determinant_bound {
            break;
        }
        let numerator_u = first_v.mul_add(second_value, -(first_value * second_v));
        let numerator_v = first_value.mul_add(second_u, -(first_u * second_value));
        let numerator_u_bound = product_difference_bound(
            [first_v, second_value, first_value, second_v],
            [bound_first_v, second_bound, first_bound, bound_second_v],
        );
        let numerator_v_bound = product_difference_bound(
            [first_value, second_u, first_u, second_value],
            [first_bound, bound_second_u, bound_first_u, second_bound],
        );
        let delta_u = numerator_u / determinant;
        let delta_v = numerator_v / determinant;
        u += delta_u;
        v += delta_v;
        if numerator_u.abs() <= numerator_u_bound && numerator_v.abs() <= numerator_v_bound {
            correction = Some([delta_u.abs(), delta_v.abs()]);
            break;
        }
    }
    RefinedParameters {
        point: [u, v],
        correction,
    }
}

/// The bound on `plane_conic_value` at a refined pair whose exact conic passes
/// through the pair's own uncertainty.
///
/// Three quantities reach the residual and nothing else does. The first two are
/// the coefficients' own bounds and the evaluation's rounding, which is
/// `plane_conic_value_bound`. The third is the pair's own uncertainty: the
/// exact zero of the conic can sit anywhere inside the refinement's correction,
/// and the conic over that displacement is its gradient times the correction
/// plus the exact quadratic remainder, both read against the coefficients'
/// term magnitudes, which bound their values. A pair with no converged
/// correction carries the first two alone.
fn plane_conic_residual_bound(conic: PlaneConicEquation, refined: RefinedParameters) -> f64 {
    let [u, v] = refined.point;
    let [correction_u, correction_v] = refined.correction.unwrap_or([0.0, 0.0]);
    let (_, [gradient_u, gradient_v]) = plane_conic_gradient(conic, u, v);
    plane_conic_value_bound(conic, u, v)
        + gradient_u * correction_u
        + gradient_v * correction_v
        + conic.uu.terms() * correction_u * correction_u
        + conic.uv.terms() * correction_u * correction_v
        + conic.vv.terms() * correction_v * correction_v
}

/// The pair `first = 0, second = 0`, whose root is a transversal intersection
/// of the two conics.
fn plane_conic_pair_system(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
    u: f64,
    v: f64,
) -> NewtonSystem {
    let (first_gradient, first_terms) = plane_conic_gradient(first, u, v);
    let (second_gradient, second_terms) = plane_conic_gradient(second, u, v);
    NewtonSystem {
        values: [
            plane_conic_value(first, u, v),
            plane_conic_value(second, u, v),
        ],
        value_bounds: [
            plane_conic_value_bound(first, u, v),
            plane_conic_value_bound(second, u, v),
        ],
        jacobian: [first_gradient, second_gradient],
        jacobian_bounds: [
            first_terms.map(cancellation_bound),
            second_terms.map(cancellation_bound),
        ],
    }
}

fn refine_plane_conic_intersection(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
    u: f64,
    v: f64,
) -> RefinedParameters {
    newton_refine(|u, v| plane_conic_pair_system(first, second, u, v), u, v)
}

/// The pair `first = 0` and `first_u second_v - first_v second_u = 0`, whose
/// root is a contact of the two conics.
///
/// Where the two conics touch they share a point and their gradients are
/// parallel, so the second equation holds there and the pair states the contact
/// as a simple root — which the pair of conics itself does not, because a
/// contact is a double root of both the resultant and the Jacobian. The second
/// equation's derivatives are exact in the conics' coefficients: the second
/// derivatives of a conic are its quadratic coefficients.
fn plane_conic_tangency_system(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
    u: f64,
    v: f64,
) -> NewtonSystem {
    let (first_gradient, first_terms) = plane_conic_gradient(first, u, v);
    let (second_gradient, second_terms) = plane_conic_gradient(second, u, v);
    let [first_u, first_v] = first_gradient;
    let [second_u, second_v] = second_gradient;
    let [first_u_bound, first_v_bound] = first_terms.map(cancellation_bound);
    let [second_u_bound, second_v_bound] = second_terms.map(cancellation_bound);
    let tangency = first_u.mul_add(second_v, -(first_v * second_u));
    let tangency_bound = product_difference_bound(
        [first_u, second_v, first_v, second_u],
        [first_u_bound, second_v_bound, first_v_bound, second_u_bound],
    );
    let tangency_u = 2.0 * first.uu.stated() * second_v + first_u * second.uv.stated()
        - first.uv.stated() * second_u
        - 2.0 * second.uu.stated() * first_v;
    let tangency_v = first.uv.stated() * second_v + 2.0 * second.vv.stated() * first_u
        - 2.0 * first.vv.stated() * second_u
        - second.uv.stated() * first_v;
    let tangency_u_terms = 2.0 * first.uu.terms() * second_v.abs()
        + first_u.abs() * second.uv.terms()
        + first.uv.terms() * second_u.abs()
        + 2.0 * second.uu.terms() * first_v.abs();
    let tangency_v_terms = first.uv.terms() * second_v.abs()
        + 2.0 * second.vv.terms() * first_u.abs()
        + 2.0 * first.vv.terms() * second_u.abs()
        + second.uv.terms() * first_v.abs();
    NewtonSystem {
        values: [plane_conic_value(first, u, v), tangency],
        value_bounds: [plane_conic_value_bound(first, u, v), tangency_bound],
        jacobian: [first_gradient, [tangency_u, tangency_v]],
        jacobian_bounds: [
            [first_u_bound, first_v_bound],
            [
                cancellation_bound(tangency_u_terms),
                cancellation_bound(tangency_v_terms),
            ],
        ],
    }
}

fn refine_plane_conic_tangency(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
    u: f64,
    v: f64,
) -> RefinedParameters {
    newton_refine(
        |u, v| plane_conic_tangency_system(first, second, u, v),
        u,
        v,
    )
}

/// The conic parameters v that satisfy the conic at the given u.
///
/// `conic.vv` is the quadratic coefficient of the problem and reaches
/// `real_roots` with the error bar its own constructor gave it, so the degree
/// decision there is the one that constructor made. The two sums formed here
/// scale that same bar by the powers of `u` they multiply it with, which bounds
/// each sum against the exact coefficients rather than against the stated ones.
fn conic_v_roots(conic: PlaneConicEquation, u: f64) -> Vec<f64> {
    real_roots(
        conic.vv,
        Coefficient::summed(
            conic.uv.stated().mul_add(u, conic.v.stated()),
            u.abs() * conic.uv.terms() + conic.v.terms(),
        ),
        Coefficient::summed(
            conic.uu.stated() * u * u + conic.u.stated() * u + conic.constant.stated(),
            u * u * conic.uu.terms() + u.abs() * conic.u.terms() + conic.constant.terms(),
        ),
    )
}

pub(super) fn common_plane_conic_parameters(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
) -> Vec<[f64; 2]> {
    let resultant = conic_resultant(first, second);
    let mut parameters = Vec::<[f64; 2]>::new();
    for root in real_polynomial_roots(&resultant) {
        let u = root.value;
        let first_v_roots = conic_v_roots(first, u);
        let second_v_roots = conic_v_roots(second, u);
        for v in first_v_roots.into_iter().chain(second_v_roots) {
            // A root the resultant shares with its derivative is two
            // intersections that coincide in u. They are two distinct points on
            // a chord across the chart u axis, or one point where the conics
            // touch. The refinement separates the two: a chord has a regular
            // Jacobian at each of its points and converges, and a contact has
            // the gradients parallel, so the pair of conics states no converged
            // step there however near the start is. Only then is the contact
            // solved for as such.
            let mut refined = refine_plane_conic_intersection(first, second, u, v);
            if refined.correction.is_none() && root.multiple {
                refined = refine_plane_conic_tangency(first, second, u, v);
            }
            let candidate = refined.point;
            let scale = candidate[0].abs().max(candidate[1].abs()).max(1.0);
            if plane_conic_value(first, candidate[0], candidate[1]).abs()
                <= plane_conic_residual_bound(first, refined)
                && plane_conic_value(second, candidate[0], candidate[1]).abs()
                    <= plane_conic_residual_bound(second, refined)
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

pub(in crate::decode) fn intersect_plane_with_two_quadrics(
    plane: PlaneEquation,
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<[f64; 3]> {
    let Some(normal) = normalize(plane.normal) else {
        return Vec::new();
    };
    let reference_axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let magnitudes = reference_axes.map(|axis| dot(normal, axis).abs());
    let index = if magnitudes[0] <= magnitudes[1] && magnitudes[0] <= magnitudes[2] {
        0
    } else if magnitudes[1] <= magnitudes[2] {
        1
    } else {
        2
    };
    let reference = reference_axes[index];
    let Some(u_axis) = normalize(cross(normal, reference)) else {
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

pub(in crate::decode) fn intersect_two_planes_with_torus(
    first: PlaneEquation,
    second: PlaneEquation,
    torus: TorusEquation,
) -> Vec<[f64; 3]> {
    let Some((line_origin, direction)) = plane_intersection_line(first, second) else {
        return Vec::new();
    };
    let Some(axis) = normalize(torus.axis) else {
        return Vec::new();
    };
    if torus.major_radius <= 0.0 || torus.minor_radius <= 0.0 {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| line_origin[index] - torus.center[index]);
    let squared_distance = [dot(relative, relative), 2.0 * dot(relative, direction), 1.0];
    let squared_distance_terms = [
        abs_dot(relative, relative),
        2.0 * abs_dot(relative, direction),
        1.0,
    ];
    let axial = [dot(relative, axis), dot(direction, axis)];
    let axial_terms = [abs_dot(relative, axis), abs_dot(direction, axis)];
    let axial_squared = [
        axial[0] * axial[0],
        2.0 * axial[0] * axial[1],
        axial[1] * axial[1],
    ];
    let axial_squared_terms = [
        axial_terms[0] * axial_terms[0],
        2.0 * axial_terms[0] * axial_terms[1],
        axial_terms[1] * axial_terms[1],
    ];
    let mut shifted_distance = squared_distance;
    shifted_distance[0] +=
        torus.major_radius * torus.major_radius - torus.minor_radius * torus.minor_radius;
    let mut shifted_distance_terms = squared_distance_terms;
    shifted_distance_terms[0] +=
        torus.major_radius * torus.major_radius + torus.minor_radius * torus.minor_radius;
    let mut polynomial = [0.0; 5];
    let mut polynomial_terms = [0.0; 5];
    for (left_power, left) in shifted_distance.into_iter().enumerate() {
        for (right_power, right) in shifted_distance.into_iter().enumerate() {
            polynomial[left_power + right_power] += left * right;
            polynomial_terms[left_power + right_power] +=
                shifted_distance_terms[left_power] * shifted_distance_terms[right_power];
        }
    }
    let radial_scale = 4.0 * torus.major_radius * torus.major_radius;
    for power in 0..=2 {
        polynomial[power] -= radial_scale * (squared_distance[power] - axial_squared[power]);
        polynomial_terms[power] +=
            radial_scale * (squared_distance_terms[power] + axial_squared_terms[power]);
    }
    let polynomial = std::array::from_fn::<_, 5, _>(|power| BoundedCoefficient {
        value: polynomial[power],
        bound: POLYNOMIAL_ERROR_FACTOR * cancellation_bound(polynomial_terms[power]),
    });
    real_polynomial_roots(&polynomial)
        .into_iter()
        .map(|root| {
            std::array::from_fn(|index| {
                // The coordinate is the two-term sum `origin + parameter *
                // direction`. Its distance from the coordinate at the exact
                // root is the root's own error scaled by the direction cosine,
                // plus the rounding of the product and the sum over the two
                // terms' magnitudes. A coordinate inside that distance states
                // the zero the exact root gives it; one outside states its own
                // value. The torus is a surface of revolution about its axis
                // and its intersection with a line carries no coordinate
                // exactly, so nothing here rounds a coordinate to a tidier
                // value that the arithmetic does not already hold.
                let offset = root.value * direction[index];
                let coordinate = line_origin[index] + offset;
                let coordinate_bound = direction[index].abs() * root.error
                    + cancellation_bound(line_origin[index].abs() + offset.abs());
                if coordinate.abs() <= coordinate_bound {
                    return 0.0;
                }
                coordinate
            })
        })
        .filter(|point| point_on_carrier(*point, CarrierEquation::Torus(torus)))
        .collect()
}

pub(in crate::decode) fn intersect_plane_with_circle(
    plane: PlaneEquation,
    center: [f64; 3],
    circle_axis: [f64; 3],
    radius: f64,
) -> Vec<[f64; 3]> {
    let (Some(plane_normal), Some(circle_normal)) =
        (normalize(plane.normal), normalize(circle_axis))
    else {
        return Vec::new();
    };
    let direction = cross(plane_normal, circle_normal);
    let sine = direction[0].hypot(direction[1]).hypot(direction[2]);
    if sine <= EPS_PLANE_CIRCLE_PARALLEL || !radius.is_finite() || radius <= 0.0 {
        return Vec::new();
    }
    let direction = direction.map(|value| value / sine);
    let radial_direction = cross(circle_normal, direction);
    let relative = std::array::from_fn(|index| plane.origin[index] - center[index]);
    // `radial_direction` and `direction` are an orthonormal frame of the circle
    // plane, and `plane_normal` projects on `radial_direction` with length
    // `sine`, so the cut line is `radial_direction = distance` in that frame.
    let distance = dot(plane_normal, relative) / sine;
    // The chord, and with it the tangency decision, belongs to the owner of the
    // line-circle error band. Its parameter runs from the foot of the radial
    // offset over one radius along `direction`, so a tangent states the same
    // parameter twice and collapses below.
    let Some(parameters) = line_circle_parameters(
        Point2::new(distance, 0.0),
        Point2::new(distance, radius),
        Point2::new(0.0, 0.0),
        radius,
    ) else {
        return Vec::new();
    };
    let nearest: [f64; 3] =
        std::array::from_fn(|index| center[index] + distance * radial_direction[index]);
    let mut points = Vec::new();
    for parameter in parameters {
        let signed_chord = parameter * radius;
        let point = std::array::from_fn(|index| nearest[index] + signed_chord * direction[index]);
        if point.iter().all(|value| value.is_finite()) && !points.contains(&point) {
            points.push(point);
        }
    }
    points
}

pub(in crate::decode) fn circle_parameters(
    geometry: &CurveGeometry,
) -> Option<([f64; 3], [f64; 3], f64)> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = geometry else {
        return None;
    };
    let center = circle_curve.center().get();
    let axis = circle_curve.axis();
    let radius = circle_curve.radius().get();
    Some((
        [center.x, center.y, center.z],
        [axis.x, axis.y, axis.z],
        radius,
    ))
}

pub(in crate::decode) fn plane_cone_conic(
    plane: PlaneEquation,
    cone: ConeEquation,
) -> Option<(CurveGeometry, &'static str)> {
    let normal = normalize(plane.normal)?;
    let axis = normalize(cone.axis)?;
    let x_axis = normalize(cone.ref_direction)?;
    let slope = cone.half_angle.tan();
    if slope <= EPS_NEAR_ZERO || cone.radius < 0.0 || dot(axis, x_axis).abs() > EPS_ORTHO {
        return None;
    }
    let y_axis = cross(axis, x_axis);
    let alignment = dot(normal, axis);
    let plane_u = normalize(std::array::from_fn(|index| {
        axis[index] - alignment * normal[index]
    }))?;
    let plane_v = normalize(cross(normal, plane_u))?;
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
    let axis_vector = Vector3::from(normal);
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
            CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
                cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
                    point(vertex_u, vertex_v),
                    axis_vector,
                    Vector3::from(direction),
                    opening.abs() / 4.0,
                )
                .ok()?,
            )),
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
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                    center,
                    axis_vector,
                    Vector3::from(major_direction),
                    major_radius,
                    minor_radius,
                )
                .ok()?,
            )),
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
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
            cadmpeg_ir::geometry::analytic::HyperbolaCurve::try_new(
                center,
                axis_vector,
                Vector3::from(major_direction),
                major_radius,
                minor_radius,
            )
            .ok()?,
        )),
        "plane_cone_hyperbola",
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        BoundedCoefficient, ConeEquation, PlaneConicEquation, PlaneEquation, TorusEquation,
    };
    use crate::decode::quadratic::Coefficient;
    use std::f64::consts::FRAC_PI_2;

    const ORIGIN: [f64; 3] = [1.0, 2.0, 3.0];
    const AXIS: [f64; 3] = [0.0, 0.0, 1.0];
    const REF_DIRECTION: [f64; 3] = [1.0, 0.0, 0.0];

    fn cone(radius: f64, ratio: f64, half_angle: f64) -> Option<ConeEquation> {
        ConeEquation::new(ORIGIN, AXIS, REF_DIRECTION, radius, ratio, half_angle)
    }

    /// A conic whose six coefficients are all nonzero, so no permutation of the
    /// Sylvester matrix drops out through a coefficient that happens to vanish.
    fn dense_conic(coefficients: [f64; 6]) -> PlaneConicEquation {
        let [uu, uv, vv, u, v, constant] = coefficients.map(Coefficient::single);
        PlaneConicEquation {
            uu,
            uv,
            vv,
            u,
            v,
            constant,
        }
    }

    #[test]
    fn polynomial_roots_keep_exact_coefficients_during_normalization() {
        let roots = super::real_polynomial_roots(&[
            BoundedCoefficient {
                value: 6.0,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: -5.0,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: 1.0,
                bound: 0.0,
            },
        ]);

        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].value, 2.0);
        assert_eq!(roots[1].value, 3.0);
    }

    #[test]
    fn polynomial_roots_admit_only_even_roots_stated_by_the_arithmetic() {
        let no_roots = super::real_polynomial_roots(&[
            BoundedCoefficient {
                value: 5.0e-12,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: 0.0,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: 1.0,
                bound: 0.0,
            },
        ]);
        assert!(no_roots.is_empty());

        let roots = super::real_polynomial_roots(&[
            BoundedCoefficient {
                value: 1.0,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: -2.0,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: 1.0,
                bound: 0.0,
            },
        ]);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].value, 1.0);
        assert!(roots[0].error <= super::cancellation_bound(1.0));
    }

    #[test]
    fn polynomial_roots_keep_distinct_roots_closer_than_a_fixed_relative_band() {
        const SECOND_ROOT: f64 = 5.0e-8;
        let roots = super::real_polynomial_roots(&[
            BoundedCoefficient {
                value: 0.0,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: -SECOND_ROOT,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: 1.0,
                bound: 0.0,
            },
        ]);

        assert_eq!(roots.len(), 2);
        for (root, expected) in roots.iter().zip([0.0, SECOND_ROOT]) {
            assert!((root.value - expected).abs() <= root.error);
        }
    }

    #[test]
    fn polynomial_root_intervals_cover_exactly_the_clustered_quartic_roots() {
        let roots = super::real_polynomial_roots(&[
            BoundedCoefficient {
                value: 156.982_737_423_565_65,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: -177.398_141_926_765_25,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: 75.175_686_368_480_33,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: -14.158_691_457_556_788,
                bound: 0.0,
            },
            BoundedCoefficient {
                value: 1.0,
                bound: 0.0,
            },
        ]);
        let expected = [
            3.537_110_093_263_118,
            3.538_086_303_440_86,
            3.538_796_911_152_453,
            3.544_698_149_700_357,
        ];

        assert_eq!(roots.len(), expected.len());
        assert!(roots.iter().all(|root| !root.multiple));
        assert!(expected
            .iter()
            .all(|expected_root| roots.iter().any(|root| {
                (root.value - root.error..=root.value + root.error).contains(expected_root)
            })));
        assert!(roots
            .iter()
            .all(|root| expected.iter().any(|expected_root| {
                (root.value - root.error..=root.value + root.error).contains(expected_root)
            })));
    }

    #[test]
    fn conic_resultant_is_a_quartic() {
        // The Sylvester matrix of two plane conics read as quadratics in v
        // carries four structural zeros. Every permutation that avoids them has
        // total degree four, so the resultant has five coefficients however
        // dense the conics are.
        let resultant = super::conic_resultant(
            dense_conic([1.0, 2.0, 3.0, 5.0, 7.0, 11.0]),
            dense_conic([13.0, -3.0, 2.0, -17.0, 4.0, -6.0]),
        );

        assert_eq!(resultant.len(), 5);
        assert!(resultant[4].value != 0.0);
    }

    #[test]
    fn numerical_followup_torus_line_keeps_a_coordinate_the_plane_pair_states() {
        // The plane x = 1e-15 meets the equatorial plane in a line along y
        // whose x coordinate is that offset exactly: the direction has no x
        // component, so no error of the polynomial root reaches it and the only
        // distance it carries is the rounding of a one-term sum, which is
        // 1e-31. A rule that states zero below a fixed fraction of the torus
        // radii replaces the offset the plane pair carries by a tidier value.
        const OFFSET: f64 = 1.0e-15;
        const EPS_TEST_TORUS_POINT: f64 = 1.0e-9;
        let offset_plane = PlaneEquation {
            origin: [OFFSET, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        };
        let equatorial_plane = PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        };
        let torus = TorusEquation {
            center: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            major_radius: 3.0,
            minor_radius: 1.0,
        };

        let mut points =
            super::intersect_two_planes_with_torus(offset_plane, equatorial_plane, torus);
        points.sort_by(|left, right| left[1].total_cmp(&right[1]));

        assert_eq!(points.len(), 4);
        for (point, expected) in points.iter().zip([-4.0, -2.0, 2.0, 4.0]) {
            assert_eq!(point[0], OFFSET);
            assert_eq!(point[2], 0.0);
            assert!((point[1] - expected).abs() <= EPS_TEST_TORUS_POINT);
        }
    }

    #[test]
    fn admitted_cone_keeps_its_stored_parameters() {
        let admitted =
            cone(4.0, 1.5, 0.25).expect("finite radius, positive ratio, acute half-angle");

        assert_eq!(admitted.origin(), ORIGIN);
        assert_eq!(admitted.axis(), AXIS);
        assert_eq!(admitted.ref_direction(), REF_DIRECTION);
        assert_eq!(admitted.radius(), 4.0);
        assert_eq!(admitted.ratio(), 1.5);
        assert_eq!(admitted.half_angle(), 0.25);
        assert!(cone(4.0, 1.5, 0.0).is_some());
    }

    #[test]
    fn non_finite_radius_is_rejected() {
        assert!(cone(f64::INFINITY, 1.5, 0.25).is_none());
        assert!(cone(f64::NEG_INFINITY, 1.5, 0.25).is_none());
        assert!(cone(f64::NAN, 1.5, 0.25).is_none());
    }

    #[test]
    fn non_finite_or_non_positive_ratio_is_rejected() {
        assert!(cone(4.0, f64::INFINITY, 0.25).is_none());
        assert!(cone(4.0, f64::NAN, 0.25).is_none());
        assert!(cone(4.0, 0.0, 0.25).is_none());
        assert!(cone(4.0, -1.5, 0.25).is_none());
    }

    #[test]
    fn half_angle_outside_the_acute_range_is_rejected() {
        assert!(cone(4.0, 1.5, FRAC_PI_2).is_none());
        assert!(cone(4.0, 1.5, FRAC_PI_2 + 0.25).is_none());
        assert!(cone(4.0, 1.5, -0.25).is_none());
        assert!(cone(4.0, 1.5, f64::NAN).is_none());
    }

    const SMALL_SECTION_CIRCLE_RADIUS: f64 = 1.0e-7;
    #[test]
    fn numerical_seventh_plane_circle_preserves_intersection_multiplicity() {
        for radius in [SMALL_SECTION_CIRCLE_RADIUS, 1.0, 1.0e200] {
            let cut = |x| {
                super::intersect_plane_with_circle(
                    PlaneEquation {
                        origin: [x, 0.0, 0.0],
                        normal: [1.0, 0.0, 0.0],
                    },
                    [0.0; 3],
                    [0.0, 0.0, 1.0],
                    radius,
                )
            };
            let points = cut(0.0);
            assert_eq!(points.len(), 2);
            assert!(points.contains(&[0.0, radius, 0.0]));
            assert!(points.contains(&[0.0, -radius, 0.0]));
            assert_eq!(cut(radius).len(), 1);
            assert!(cut(2.0 * radius).is_empty());
        }
    }

    const TANGENT_CIRCLE_RADIUS: f64 = 4.0;
    #[test]
    fn a_plane_inside_the_tangency_band_states_one_point() {
        // The offsets are exact multiples of the last bit of the radius, and the
        // plane normal reaches the radial offset without rounding it.
        let cut = |offset: f64| {
            super::intersect_plane_with_circle(
                PlaneEquation {
                    origin: [offset, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                },
                [0.0; 3],
                [0.0, 0.0, 1.0],
                TANGENT_CIRCLE_RADIUS,
            )
        };
        let last_bit = f64::EPSILON * TANGENT_CIRCLE_RADIUS;
        assert_eq!(cut(TANGENT_CIRCLE_RADIUS), vec![[4.0, 0.0, 0.0]]);
        assert_eq!(
            cut(TANGENT_CIRCLE_RADIUS - last_bit),
            vec![[4.0 - last_bit, 0.0, 0.0]]
        );
        assert_eq!(cut(TANGENT_CIRCLE_RADIUS - 16.0 * last_bit).len(), 2);
        assert!(cut(TANGENT_CIRCLE_RADIUS + 16.0 * last_bit).is_empty());
    }
}
