// SPDX-License-Identifier: Apache-2.0
//! Bounded-curve wrappers.

use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

use super::{f64_be, u16_be, Carrier, CarrierGeometry, CarrierIndex, LEN_TO_MM};

const TAG: u8 = 0x85;
const PAYLOAD_LEN: usize = 2 + 8 * 8;
const POINT_TOLERANCE_MM: f64 = 1.0e-7;

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

fn nurbs_point(curve: &cadmpeg_ir::geometry::NurbsCurve, parameter: f64) -> Option<Point3> {
    let degree = usize::try_from(curve.degree).ok()?;
    let last_control = curve.control_points.len().checked_sub(1)?;
    if degree > last_control
        || curve.knots.len() != curve.control_points.len() + degree + 1
        || curve
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != curve.control_points.len())
    {
        return None;
    }
    let domain_start = *curve.knots.get(degree)?;
    let domain_end = *curve.knots.get(last_control + 1)?;
    if parameter < domain_start || parameter > domain_end {
        return None;
    }
    let span = if parameter == domain_end {
        last_control
    } else {
        (degree..=last_control)
            .find(|index| curve.knots[*index] <= parameter && parameter < curve.knots[*index + 1])?
    };
    let mut poles = (span - degree..=span)
        .map(|index| {
            let point = curve.control_points[index];
            let weight = curve.weights.as_ref().map_or(1.0, |weights| weights[index]);
            [point.x * weight, point.y * weight, point.z * weight, weight]
        })
        .collect::<Vec<_>>();
    for level in 1..=degree {
        for local in (level..=degree).rev() {
            let knot = span - degree + local;
            let denominator = curve.knots[knot + degree - level + 1] - curve.knots[knot];
            let alpha = if denominator.abs() <= f64::EPSILON {
                0.0
            } else {
                (parameter - curve.knots[knot]) / denominator
            };
            let previous = poles[local - 1];
            let current = poles[local];
            poles[local] = std::array::from_fn(|coordinate| {
                (1.0 - alpha) * previous[coordinate] + alpha * current[coordinate]
            });
        }
    }
    let result = poles[degree];
    (result[3].is_finite() && result[3].abs() > f64::EPSILON).then(|| {
        Point3::new(
            result[0] / result[3],
            result[1] / result[3],
            result[2] / result[3],
        )
    })
}

fn point_at(curve: &CurveGeometry, parameter: f64) -> Option<Point3> {
    match curve {
        CurveGeometry::Line { origin, direction } => Some(Point3::new(
            origin.x + parameter * direction.x * LEN_TO_MM,
            origin.y + parameter * direction.y * LEN_TO_MM,
            origin.z + parameter * direction.z * LEN_TO_MM,
        )),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let tangent = cross(*axis, *ref_direction);
            Some(Point3::new(
                center.x
                    + radius * (parameter.cos() * ref_direction.x + parameter.sin() * tangent.x),
                center.y
                    + radius * (parameter.cos() * ref_direction.y + parameter.sin() * tangent.y),
                center.z
                    + radius * (parameter.cos() * ref_direction.z + parameter.sin() * tangent.z),
            ))
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let minor_direction = cross(*axis, *major_direction);
            Some(Point3::new(
                center.x
                    + major_radius * parameter.cos() * major_direction.x
                    + minor_radius * parameter.sin() * minor_direction.x,
                center.y
                    + major_radius * parameter.cos() * major_direction.y
                    + minor_radius * parameter.sin() * minor_direction.y,
                center.z
                    + major_radius * parameter.cos() * major_direction.z
                    + minor_radius * parameter.sin() * minor_direction.z,
            ))
        }
        CurveGeometry::Nurbs(curve) => nurbs_point(curve, parameter),
        _ => None,
    }
}

fn close(left: Point3, right: Point3) -> bool {
    (left.x - right.x).abs() <= POINT_TOLERANCE_MM
        && (left.y - right.y).abs() <= POINT_TOLERANCE_MM
        && (left.z - right.z).abs() <= POINT_TOLERANCE_MM
}

/// Decode `00 85` wrappers whose stored bounds agree with their source curve.
pub(super) fn scan(bytes: &[u8], carriers: &CarrierIndex) -> Vec<Carrier> {
    let mut out = Vec::new();
    for off in 0..bytes.len().saturating_sub(2) {
        if bytes.get(off..off + 2) != Some(&[0x00, TAG]) {
            continue;
        }
        let header = off + 2 + usize::from(bytes.get(off + 2) == Some(&0xff));
        let marker_at = header + 16;
        if !matches!(bytes.get(marker_at), Some(0x2b | 0x2d)) {
            continue;
        }
        let Some(attr) = u16_be(bytes, header) else {
            continue;
        };
        let Some(source_attr) = u16_be(bytes, marker_at + 1) else {
            continue;
        };
        let Some(source) = carriers.curve(source_attr) else {
            continue;
        };
        let CarrierGeometry::Curve(geometry) = &source.geometry else {
            continue;
        };
        let values = (0..8)
            .map(|index| f64_be(bytes, marker_at + 3 + index * 8))
            .collect::<Option<Vec<_>>>();
        let Some(values) = values.filter(|values| values.iter().all(|value| value.is_finite()))
        else {
            continue;
        };
        let start = Point3::new(
            values[0] * LEN_TO_MM,
            values[1] * LEN_TO_MM,
            values[2] * LEN_TO_MM,
        );
        let end = Point3::new(
            values[3] * LEN_TO_MM,
            values[4] * LEN_TO_MM,
            values[5] * LEN_TO_MM,
        );
        let Some(evaluated_start) = point_at(geometry, values[6]) else {
            continue;
        };
        let Some(evaluated_end) = point_at(geometry, values[7]) else {
            continue;
        };
        if !close(start, evaluated_start) || !close(end, evaluated_end) {
            continue;
        }
        out.push(Carrier {
            attr,
            offset: off,
            end: marker_at + 1 + PAYLOAD_LEN,
            geometry: CarrierGeometry::Curve(geometry.clone()),
            frame: source.frame,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::geometry::NurbsCurve;
    use cadmpeg_ir::math::Vector3;

    use super::*;

    fn wrapper(end_y: f64, has_ff: bool) -> Vec<u8> {
        let mut bytes = vec![0x00, TAG];
        if has_ff {
            bytes.push(0xff);
        }
        bytes.extend_from_slice(&20u16.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        for reference in [1u16, 2, 3, 4, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(0x2b);
        bytes.extend_from_slice(&10u16.to_be_bytes());
        for value in [0.0, 0.0, 0.0, 0.0, end_y, 0.0, 0.0, end_y] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    fn carriers() -> CarrierIndex {
        let mut carriers = CarrierIndex::default();
        carriers.insert(Carrier {
            attr: 10,
            offset: 100,
            end: 120,
            geometry: CarrierGeometry::Curve(CurveGeometry::Line {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 1.0, 0.0),
            }),
            frame: None,
        });
        carriers
    }

    #[test]
    fn decodes_bounds_that_evaluate_on_the_source_curve() {
        let decoded = scan(&wrapper(0.005, false), &carriers());
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].attr, 20);
        assert!(matches!(
            decoded[0].geometry,
            CarrierGeometry::Curve(CurveGeometry::Line { .. })
        ));
    }

    #[test]
    fn decodes_optional_ff_header() {
        assert_eq!(scan(&wrapper(0.005, true), &carriers()).len(), 1);
    }

    #[test]
    fn rejects_bounds_that_do_not_evaluate_on_the_source_curve() {
        let mut bytes = wrapper(0.005, false);
        bytes[21 + 3 * 8..21 + 4 * 8].copy_from_slice(&0.001f64.to_be_bytes());
        assert!(scan(&bytes, &carriers()).is_empty());
    }

    #[test]
    fn evaluates_rational_nurbs_in_homogeneous_coordinates() {
        let curve = NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            weights: Some(vec![1.0, 2.0]),
            periodic: false,
        };
        let point = nurbs_point(&curve, 0.5).expect("valid NURBS parameter");
        assert!((point.x - 20.0 / 3.0).abs() < 1.0e-12);
    }
}
