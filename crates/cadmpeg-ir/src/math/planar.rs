// SPDX-License-Identifier: Apache-2.0
//! Planar intersection arithmetic with scaled products.

use super::{sum::ExactSignedSum, Point2};
use std::cmp::Ordering;

/// Orientation of three finite points. Returns `None` for nonfinite input.
/// The exact-product fallback preserves the sign at extreme scales and at cancellation.
pub fn orientation(a: Point2, b: Point2, p: Point2) -> Option<Ordering> {
    if !a.is_finite() || !b.is_finite() || !p.is_finite() {
        return None;
    }
    let left = (b.u - a.u) * (p.v - a.v);
    let right = (b.v - a.v) * (p.u - a.u);
    let determinant = left - right;
    if determinant.is_finite()
        && determinant.abs() > 4.0 * f64::EPSILON * (left.abs() + right.abs())
    {
        return determinant.partial_cmp(&0.0);
    }
    let mut sum = ExactSignedSum::default();
    for (x, y) in [
        (b.u, p.v),
        (-b.u, a.v),
        (-a.u, p.v),
        (-b.v, p.u),
        (b.v, a.u),
        (a.v, p.u),
    ] {
        sum.add_product(x, y);
    }
    match sum.finish() {
        Some(value) => value.rescale(value.exponent())?.partial_cmp(&0.0),
        None => Some(Ordering::Equal),
    }
}

// Scale differences before subtraction only when their finite subtraction overflows.
fn scaled_displacement(start: Point2, end: Point2, minimum_scale: f64) -> (Point2, f64) {
    let delta = Point2::new(end.u - start.u, end.v - start.v);
    if delta.is_finite() {
        let scale = delta.u.abs().max(delta.v.abs()).max(minimum_scale);
        if scale == 0.0 {
            return (delta, scale);
        }
        (Point2::new(delta.u / scale, delta.v / scale), scale)
    } else {
        let scale = start
            .u
            .abs()
            .max(start.v.abs())
            .max(end.u.abs())
            .max(end.v.abs())
            .max(minimum_scale);
        (
            Point2::new(
                end.u / scale - start.u / scale,
                end.v / scale - start.v / scale,
            ),
            scale,
        )
    }
}

/// The half chord of a circle cut at a perpendicular distance, both lengths
/// divided by the larger of the two so that one of them is exactly one.
/// `None` states a cut that misses the circle.
///
/// A tangent touches at one point, so the difference is read against the
/// rounding error its own two sides carry. Round to nearest moves a value by at
/// most `f64::EPSILON / 2` of itself, and both callers form the perpendicular
/// distance through the same five stages: a displacement scaled by its own
/// largest component, the hypotenuse of that displacement, two exact sums, and
/// one quotient of those sums. Those stages hold eight such steps — three in the
/// scaled displacement, whose overflow form divides each coordinate before it
/// subtracts, one ulp for the hypotenuse, the final rounding of each of the two
/// exact sums, and the quotient. Dividing by the larger of the two lengths
/// rounds one side once more and leaves the other exactly one, which the
/// `radius` term counts. Products of those relative errors are second order
/// against the half ulp each step rounds up to, and the subtraction itself is
/// exact while the two sides stay within a factor of two, which is every case
/// the band decides.
fn half_chord(radius: f64, perpendicular: f64) -> Option<f64> {
    let difference = radius - perpendicular;
    let tangency_error = 0.5 * f64::EPSILON * (8.0 * perpendicular + radius);
    if difference.abs() <= tangency_error {
        return Some(0.0);
    }
    if difference < 0.0 {
        return None;
    }
    Some(difference.sqrt() * (radius + perpendicular).sqrt())
}

/// Parameters and points where an infinite line intersects a circle.
/// Points are constructed in the circle frame, independently of rounded line
/// parameters. A tangent returns the same pair twice. Invalid, degenerate,
/// disjoint or unrepresentable inputs return `None`.
pub fn line_circle_intersections(
    start: Point2,
    end: Point2,
    center: Point2,
    radius: f64,
) -> Option<[(f64, Point2); 2]> {
    if !start.is_finite()
        || !end.is_finite()
        || !center.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
    {
        return None;
    }
    let (direction, direction_scale) = scaled_displacement(start, end, 0.0);
    if direction_scale == 0.0 {
        return None;
    }
    let length = direction.u.hypot(direction.v);
    // `(end - start) . (end - start) / direction_scale`, the divisor that turns a
    // projection on `direction` into a parameter on `end - start`. It is formed
    // exactly, so a tangent, whose half chord contributes nothing, keeps every bit
    // the projection carries. The rounded hypotenuse reaches the parameter only
    // through the half chord.
    let mut denominator = ExactSignedSum::default();
    denominator.add_factors([direction.u, direction.u, direction_scale]);
    denominator.add_factors([direction.v, direction.v, direction_scale]);
    let denominator = denominator.finish()?;
    // `|end - start|`, the divisor that turns the determinant into a distance.
    let mut segment = ExactSignedSum::default();
    segment.add_product(length, direction_scale);
    let segment = segment.finish()?;
    // Form the determinant from the original coordinates. Normalizing the
    // direction first can rotate a distant, exactly incident line off a small circle.
    let mut determinant = ExactSignedSum::default();
    for (left, right) in [
        (end.u, start.v),
        (-end.u, center.v),
        (start.u, center.v),
        (-end.v, start.u),
        (end.v, center.u),
        (-start.v, center.u),
    ] {
        determinant.add_product(left, right);
    }
    let perpendicular = determinant
        .finish()
        .map_or(Some(0.0), |value| value.quotient(segment))?;
    // Only radial quantities contribute to this comparison. A distant line origin
    // must not widen the circle into a false tangent.
    let radial_scale = radius.max(perpendicular.abs());
    let normalized_radius = radius / radial_scale;
    let half_chord = half_chord(normalized_radius, perpendicular.abs() / radial_scale)?;
    let parameter = |sign: f64| {
        let mut numerator = ExactSignedSum::default();
        for (coordinate, component) in [
            (center.u, direction.u),
            (-start.u, direction.u),
            (center.v, direction.v),
            (-start.v, direction.v),
        ] {
            numerator.add_product(coordinate, component);
        }
        numerator.add_factors([sign * half_chord, radial_scale, length]);
        numerator
            .finish()
            .map_or(Some(0.0), |value| value.quotient(denominator))
    };
    let intersection = |sign| {
        let along = sign * half_chord * radial_scale;
        let mut point = Point2::new(
            super::sum::finite_dot(
                [1.0, -perpendicular, along],
                [center.u, direction.v / length, direction.u / length],
            )?,
            super::sum::finite_dot(
                [1.0, perpendicular, along],
                [center.v, direction.u / length, direction.v / length],
            )?,
        );
        // A constant line coordinate is exact input evidence. Retain it
        // instead of rounding it through the determinant-distance quotient.
        if start.u == end.u {
            point.u = start.u;
        }
        if start.v == end.v {
            point.v = start.v;
        }
        Some((parameter(sign)?, point))
    };
    Some([intersection(-1.0)?, intersection(1.0)?])
}

/// Projection parameter on a nondegenerate infinite line. Exact products retain
/// the quotient when direction differences or their squares exceed f64 range.
pub fn line_projection_parameter(start: Point2, end: Point2, point: Point2) -> Option<f64> {
    if !start.is_finite() || !end.is_finite() || !point.is_finite() {
        return None;
    }
    let delta = Point2::new(end.u - start.u, end.v - start.v);
    let relative = Point2::new(point.u - start.u, point.v - start.v);
    let denominator = delta.u * delta.u + delta.v * delta.v;
    let numerator = relative.u * delta.u + relative.v * delta.v;
    if denominator.is_finite() && denominator > 0.0 && numerator.is_finite() {
        let parameter = numerator / denominator;
        if parameter.is_finite() {
            return Some(parameter);
        }
    }
    let mut numerator = ExactSignedSum::default();
    let mut denominator = ExactSignedSum::default();
    for (start, end, point) in [(start.u, end.u, point.u), (start.v, end.v, point.v)] {
        for (a, b) in [(point, end), (-point, start), (-start, end), (start, start)] {
            numerator.add_product(a, b);
        }
        denominator.add_product(start, start);
        denominator.add_product(end, end);
        denominator.add_factors([-2.0, start, end]);
    }
    let denominator = denominator.finish()?;
    numerator
        .finish()
        .map_or(Some(0.0), |value| value.quotient(denominator))
}

/// Parameters at the intersection of two finite nonparallel infinite lines.
/// Parallel, degenerate or unrepresentable intersections return `None`.
pub fn line_line_parameters(a: Point2, b: Point2, c: Point2, d: Point2) -> Option<[f64; 2]> {
    if [a, b, c, d].iter().any(|point| !point.is_finite()) {
        return None;
    }
    let ab = Point2::new(b.u - a.u, b.v - a.v);
    let cd = Point2::new(d.u - c.u, d.v - c.v);
    let ac = Point2::new(c.u - a.u, c.v - a.v);
    let positive = ab.u * cd.v;
    let negative = ab.v * cd.u;
    let denominator = positive - negative;
    if denominator.is_finite()
        && denominator.abs() > 4.0 * f64::EPSILON * (positive.abs() + negative.abs())
    {
        let parameters = [
            (ac.u * cd.v - ac.v * cd.u) / denominator,
            (ac.u * ab.v - ac.v * ab.u) / denominator,
        ];
        if parameters.iter().all(|parameter| parameter.is_finite()) {
            return Some(parameters);
        }
    }
    let cross = |a: Point2, b: Point2, c: Point2, d: Point2| {
        let mut sum = ExactSignedSum::default();
        for (x, y) in [
            (b.u, d.v),
            (-b.u, c.v),
            (-a.u, d.v),
            (a.u, c.v),
            (-b.v, d.u),
            (b.v, c.u),
            (a.v, d.u),
            (-a.v, c.u),
        ] {
            sum.add_product(x, y);
        }
        sum.finish()
    };
    let denominator = cross(a, b, c, d)?;
    Some([
        cross(a, c, c, d).map_or(Some(0.0), |value| value.quotient(denominator))?,
        cross(a, c, a, b).map_or(Some(0.0), |value| value.quotient(denominator))?,
    ])
}

/// The finite intersections of two positive-radius circles.
/// Coincident circles and invalid or unrepresentable results return `None`.
/// Disjoint circles return an empty vector; a tangent returns one point.
pub fn circle_intersections(
    first: Point2,
    first_radius: f64,
    second: Point2,
    second_radius: f64,
) -> Option<Vec<Point2>> {
    if !first.is_finite()
        || !second.is_finite()
        || !first_radius.is_finite()
        || !second_radius.is_finite()
        || first_radius <= 0.0
        || second_radius <= 0.0
    {
        return None;
    }
    // Work from the smaller circle so its radius survives independently of the
    // separation and the other radius. The returned point set is unordered.
    if first_radius > second_radius {
        return circle_intersections(second, second_radius, first, first_radius);
    }
    let (delta, scale) = scaled_displacement(first, second, 0.0);
    let distance = delta.u.hypot(delta.v);
    if distance == 0.0 {
        return (first_radius != second_radius).then(Vec::new);
    }
    let mut denominator = ExactSignedSum::default();
    denominator.add_factors([2.0, distance, scale]);
    let denominator = denominator.finish()?;
    let mut numerator = ExactSignedSum::default();
    for (start, end) in [(first.u, second.u), (first.v, second.v)] {
        numerator.add_product(start, start);
        numerator.add_product(end, end);
        numerator.add_factors([-2.0, start, end]);
    }
    numerator.add_product(first_radius, first_radius);
    numerator.add_product(-second_radius, second_radius);
    let Some(along) = numerator
        .finish()
        .map_or(Some(0.0), |value| value.quotient(denominator))
    else {
        return Some(Vec::new());
    };
    // `along` is the signed perpendicular distance from the smaller circle's
    // center to the radical line, so the chord it cuts answers the same question
    // a line cutting that circle does. Only radial quantities contribute to the
    // comparison, and the larger of the two normalizes both, which keeps a
    // radius below the normal range from carrying the offset out of range.
    let perpendicular = along.abs();
    let radial_scale = first_radius.max(perpendicular);
    let Some(half_chord) = half_chord(first_radius / radial_scale, perpendicular / radial_scale)
    else {
        return Some(Vec::new());
    };
    let height = radial_scale * half_chord;
    let unit = Point2::new(delta.u / distance, delta.v / distance);
    let mut points = Vec::new();
    for height in [height, -height] {
        let point = Point2::new(
            super::sum::finite_dot([1.0, along, -height], [first.u, unit.u, unit.v])?,
            super::sum::finite_dot([1.0, along, height], [first.v, unit.v, unit.u])?,
        );
        if !points.contains(&point) {
            points.push(point);
        }
    }
    Some(points)
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::{circle_intersections, line_circle_intersections, orientation, Point2};

    fn line_circle_parameters(
        start: Point2,
        end: Point2,
        center: Point2,
        radius: f64,
    ) -> Option<[f64; 2]> {
        line_circle_intersections(start, end, center, radius)
            .map(|hits| hits.map(|(parameter, _)| parameter))
    }
    #[test]
    fn distant_diagonal_line_preserves_exact_circle_incidence() {
        const RADIUS: f64 = 1e-10;
        for exponent in [-600, 0, 600] {
            let scale = 2.0_f64.powi(exponent);
            let roots = line_circle_parameters(
                Point2::new(1e8 * scale, 3e8 * scale),
                Point2::new((1e8 + 1.0) * scale, (3e8 + 3.0) * scale),
                Point2::new(0.0, 0.0),
                RADIUS * scale,
            )
            .unwrap();
            for root in roots {
                assert!((root + 1e8).abs() <= 2.0 * f64::EPSILON * 1e8);
            }
        }
    }

    #[test]
    fn distant_line_origin_does_not_change_circle_intersections() {
        let center = Point2::new(0.0, 0.0);
        assert!(line_circle_parameters(
            Point2::new(1e8, 2.0),
            Point2::new(1e8 + 1.0, 2.0),
            center,
            1.0
        )
        .is_none());
        let mut roots = line_circle_parameters(
            Point2::new(1e8, 0.0),
            Point2::new(1e8 + 1.0, 0.0),
            center,
            1.0,
        )
        .unwrap();
        roots.sort_by(f64::total_cmp);
        assert_eq!(roots, [-1e8 - 1.0, -1e8 + 1.0]);
        assert_eq!(
            line_circle_parameters(
                Point2::new(1e8, 1.0),
                Point2::new(1e8 + 1.0, 1.0),
                center,
                1.0
            )
            .unwrap(),
            [-1e8; 2]
        );
    }

    #[test]
    fn intersections_preserve_independent_direction_and_position_scales() {
        let mut roots = line_circle_parameters(
            Point2::new(0.0, 0.0),
            Point2::new(1e-200, 0.0),
            Point2::new(0.0, 0.0),
            1.0,
        )
        .unwrap();
        roots.sort_by(f64::total_cmp);
        assert_eq!(roots, [-1e200, 1e200]);
        let points = circle_intersections(
            Point2::new(-1e308, 0.0),
            1e308,
            Point2::new(1e308, 0.0),
            1e308,
        )
        .unwrap();
        assert_eq!(points, vec![Point2::new(0.0, 0.0)]);
    }

    #[test]
    fn intersections_and_orientation_preserve_scale() {
        for r in [1e-200, 1., 1e200] {
            let a = Point2::new(0., 0.);
            let b = Point2::new(r, 0.);
            let points = circle_intersections(a, r, b, r).unwrap();
            assert_eq!(points.len(), 2);
            for p in points {
                assert!((p.u / r - 0.5).abs() <= 4. * f64::EPSILON);
                assert!((p.v.abs() / r - 0.75_f64.sqrt()).abs() <= 4. * f64::EPSILON);
            }
            let mut parameters =
                line_circle_parameters(Point2::new(-2. * r, 0.), Point2::new(2. * r, 0.), a, r)
                    .unwrap();
            parameters.sort_by(f64::total_cmp);
            assert_eq!(parameters, [0.25, 0.75]);
            assert_eq!(
                orientation(a, b, Point2::new(0., r)),
                Some(Ordering::Greater)
            );
            assert_eq!(orientation(a, b, Point2::new(0., -r)), Some(Ordering::Less));
        }
    }

    #[test]
    fn a_tangent_line_states_one_repeated_root_at_every_scale() {
        let center = Point2::new(0.0, 0.0);
        for radius in [1e-150, 1e-4, 1.0, 1e150] {
            assert_eq!(
                line_circle_parameters(
                    Point2::new(-radius, radius),
                    Point2::new(radius, radius),
                    center,
                    radius
                )
                .unwrap(),
                [0.5; 2]
            );
            assert!(line_circle_parameters(
                Point2::new(-radius, 2.0 * radius),
                Point2::new(radius, 2.0 * radius),
                center,
                radius
            )
            .is_none());
        }
        // This diagonal's perpendicular distance is `sqrt(2) / 2`, which no f64
        // holds, so the two sides of the tangency comparison differ in their last
        // bit. The touch parameter stays exact: it is the projection alone.
        assert_eq!(
            line_circle_parameters(
                Point2::new(1.0, 0.0),
                Point2::new(2.0, 1.0),
                center,
                0.5 * 2.0_f64.sqrt()
            )
            .unwrap(),
            [-0.5; 2]
        );
    }

    // Two unit circles a distance `d` apart put the radical line `d / 2` from
    // the first center, and `d * d` and `2 * d` both reach the accumulator
    // exactly, so one last bit of `d` moves that distance by half a last bit of
    // the radius. The outcome then reads the offset against the band alone.
    fn unit_circles_cut(distance: f64) -> Vec<Point2> {
        circle_intersections(Point2::new(0.0, 0.0), 1.0, Point2::new(distance, 0.0), 1.0).unwrap()
    }

    #[test]
    fn a_radical_line_inside_the_tangency_band_states_one_point() {
        assert_eq!(unit_circles_cut(2.0), vec![Point2::new(1.0, 0.0)]);
        assert_eq!(
            unit_circles_cut(2.0 + 2.0 * f64::EPSILON),
            vec![Point2::new(1.0 + f64::EPSILON, 0.0)]
        );
        assert_eq!(
            unit_circles_cut(2.0 - 2.0 * f64::EPSILON),
            vec![Point2::new(1.0 - f64::EPSILON, 0.0)]
        );
        let secant = unit_circles_cut(2.0 - 64.0 * f64::EPSILON);
        assert_eq!(secant.len(), 2);
        for point in secant {
            assert_eq!(point.u, 1.0 - 32.0 * f64::EPSILON);
            assert!((point.v.abs() - (64.0 * f64::EPSILON).sqrt()).abs() <= 8.0 * f64::EPSILON);
        }
    }

    #[test]
    fn a_radical_line_past_the_tangency_band_states_no_point() {
        assert_eq!(
            unit_circles_cut(2.0 + 32.0 * f64::EPSILON),
            Vec::<Point2>::new()
        );
    }

    #[test]
    fn numerical_seventh_unequal_circles_preserve_the_small_circle() {
        for large in [1.0e20, 1.0e200, 1.0e300] {
            for reversed in [false, true] {
                let small_center = Point2::new(0.0, 0.0);
                let large_center = Point2::new(large, 0.0);
                let points = if reversed {
                    circle_intersections(large_center, large, small_center, 1.0)
                } else {
                    circle_intersections(small_center, 1.0, large_center, large)
                }
                .unwrap();
                assert_eq!(points.len(), 2);
                for point in points {
                    assert!((point.u * large - 0.5).abs() <= 8.0 * f64::EPSILON);
                    assert!((point.v.abs() - 1.0).abs() <= 8.0 * f64::EPSILON);
                }
            }
        }
    }

    #[test]
    fn numerical_0922b_line_circle_points_preserve_distant_origin() {
        for extent in [1.0, 1e4, 1e8, 1e200] {
            let hits = line_circle_intersections(
                Point2::new(-extent, 0.),
                Point2::new(extent, 0.),
                Point2::new(0., 0.),
                0.001,
            )
            .unwrap();
            assert_eq!(
                hits.map(|(_, point)| point),
                [Point2::new(-0.001, 0.), Point2::new(0.001, 0.)]
            );
            assert!(line_circle_intersections(
                Point2::new(-extent, 0.002),
                Point2::new(extent, 0.002),
                Point2::new(0., 0.),
                0.001
            )
            .is_none());
        }
        for radius in [0.0001, 0.001, 1.0, 1e200] {
            for (start, end, expected) in [
                (
                    Point2::new(-radius, radius),
                    Point2::new(radius, radius),
                    Point2::new(0., radius),
                ),
                (
                    Point2::new(radius, -radius),
                    Point2::new(radius, radius),
                    Point2::new(radius, 0.),
                ),
            ] {
                let hits =
                    line_circle_intersections(start, end, Point2::new(0., 0.), radius).unwrap();
                assert_eq!(hits.map(|(_, point)| point), [expected; 2]);
            }
        }
    }
}
