// SPDX-License-Identifier: Apache-2.0
//! Carrier equation types and vector/quadric/conic algebra.

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::decode::quadratic::{cancellation_bound, real_roots, Coefficient};
use crate::vecmath::{cross, dot, normalize};

use super::planes::point_on_carrier;

const EPS_PLANE_RESIDUAL: f64 = 1.0e-6;
const EPS_ROOT_CLUSTER: f64 = 1.0e-7;
const EPS_PARAM_UNIQUE: f64 = 1.0e-7;
const EPS_AGREE: f64 = 1.0e-9;
const EPS_ORTHO: f64 = 1.0e-10;
const EPS_POLY_ROOT_VALUE: f64 = 1.0e-11;
const EPS_NEAR_ZERO: f64 = 1.0e-12;

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

/// Return the finite real roots in ascending order.
///
/// The degree is stated by each leading coefficient against its own bound: a
/// coefficient inside that bound is the rounding residue of a cancellation and
/// the polynomial is of lower degree. A residue kept as a leading coefficient
/// states roots of order `1 / residue` that no exact polynomial has; a real
/// leading coefficient dropped loses the roots it carries.
fn real_polynomial_roots(coefficients: &[BoundedCoefficient]) -> Vec<f64> {
    let scale = coefficients
        .iter()
        .map(|coefficient| coefficient.value.abs())
        .fold(0.0, f64::max);
    if scale == 0.0 || !scale.is_finite() {
        return Vec::new();
    }
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
        return vec![-coefficients[0] / coefficients[1]];
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
/// Jacobian, or twelve steps that never settled — carries no correction, and
/// its only accuracy is the arithmetic that produced the coefficients.
#[derive(Clone, Copy)]
struct RefinedParameters {
    point: [f64; 2],
    correction: [f64; 2],
}

/// The bound on `plane_conic_value` at a refined pair whose exact conic passes
/// through the pair's own uncertainty.
///
/// Three quantities reach the residual and nothing else does.
///
/// Each coefficient multiplies a monomial of the point, so its own bound
/// reaches the residual scaled by that monomial; over the six that is one
/// `cancellation_bound` of the weighted term magnitudes. The evaluation formed
/// here is six products and five additions over values no larger than those
/// same weighted terms, which is `17 u` against the bound's `128 u`, so a
/// second `cancellation_bound` covers it. Last, the exact zero of the conic can
/// sit anywhere inside the refinement's own correction, and the conic over that
/// displacement is its gradient times the correction plus the exact quadratic
/// remainder; both are read against the coefficients' term magnitudes, which
/// bound their values.
fn plane_conic_residual_bound(conic: PlaneConicEquation, refined: RefinedParameters) -> f64 {
    let [u, v] = refined.point;
    let [correction_u, correction_v] = refined.correction;
    let terms = conic.uu.terms() * u * u
        + conic.uv.terms() * (u * v).abs()
        + conic.vv.terms() * v * v
        + conic.u.terms() * u.abs()
        + conic.v.terms() * v.abs()
        + conic.constant.terms();
    let gradient_u =
        2.0 * conic.uu.terms() * u.abs() + conic.uv.terms() * v.abs() + conic.u.terms();
    let gradient_v =
        conic.uv.terms() * u.abs() + 2.0 * conic.vv.terms() * v.abs() + conic.v.terms();
    2.0 * cancellation_bound(terms)
        + gradient_u * correction_u
        + gradient_v * correction_v
        + conic.uu.terms() * correction_u * correction_u
        + conic.uv.terms() * correction_u * correction_v
        + conic.vv.terms() * correction_v * correction_v
}

fn refine_plane_conic_intersection(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
    mut u: f64,
    mut v: f64,
) -> RefinedParameters {
    let mut correction = [0.0, 0.0];
    for _ in 0..12 {
        let first_value = plane_conic_value(first, u, v);
        let second_value = plane_conic_value(second, u, v);
        let first_u = 2.0 * first.uu.stated() * u + first.uv.stated() * v + first.u.stated();
        let first_v = first.uv.stated() * u + 2.0 * first.vv.stated() * v + first.v.stated();
        let second_u = 2.0 * second.uu.stated() * u + second.uv.stated() * v + second.u.stated();
        let second_v = second.uv.stated() * u + 2.0 * second.vv.stated() * v + second.v.stated();
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
            correction = [delta_u.abs(), delta_v.abs()];
            break;
        }
    }
    RefinedParameters {
        point: [u, v],
        correction,
    }
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
    for u in real_polynomial_roots(&resultant) {
        let first_v_roots = conic_v_roots(first, u);
        let second_v_roots = conic_v_roots(second, u);
        for v in first_v_roots.into_iter().chain(second_v_roots) {
            let refined = refine_plane_conic_intersection(first, second, u, v);
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

pub(in crate::decode) fn circle_parameters(
    geometry: &CurveGeometry,
) -> Option<([f64; 3], [f64; 3], f64)> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = geometry else {
        return None;
    };
    let center = circle_curve.center();
    let axis = circle_curve.axis();
    let radius = circle_curve.radius();
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
            CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
                cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
                    point(vertex_u, vertex_v),
                    axis_vector,
                    Vector3::new(direction[0], direction[1], direction[2]),
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
                    Vector3::new(major_direction[0], major_direction[1], major_direction[2]),
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
                Vector3::new(major_direction[0], major_direction[1], major_direction[2]),
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
    use super::{ConeEquation, PlaneConicEquation};
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
}
