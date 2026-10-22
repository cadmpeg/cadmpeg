// SPDX-License-Identifier: Apache-2.0
//! OCCT parameter-space curve rows from neutral carriers.

use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::units::FinitePoint2;

const RIGID_CHART_TOLERANCE: f64 = 1.0e-12;

pub(super) fn serialize(
    value: &PcurveGeometry,
    domain: Option<[f64; 2]>,
    chart: cadmpeg_ir::transform::Transform2,
) -> Option<String> {
    transformed(value, domain, chart)
}

fn transformed(
    value: &PcurveGeometry,
    domain: Option<[f64; 2]>,
    transform: cadmpeg_ir::transform::Transform2,
) -> Option<String> {
    if let PcurveGeometry::Transformed(placed) = value {
        return transformed(
            placed.basis(),
            domain,
            compose(transform, *placed.transform())?,
        );
    }
    let vector_row = |point: &FinitePoint2| {
        let point = transform.apply_vector(point.get());
        (point.u.is_finite() && point.v.is_finite()).then(|| format!("{} {}", point.u, point.v))
    };
    let rigid = {
        let rows = transform.affine_rows();
        let x = [rows[0][0], rows[1][0]];
        let y = [rows[0][1], rows[1][1]];
        (x[0].hypot(x[1]) - 1.0).abs() <= RIGID_CHART_TOLERANCE
            && (y[0].hypot(y[1]) - 1.0).abs() <= RIGID_CHART_TOLERANCE
            && (x[0] * y[0] + x[1] * y[1]).abs() <= RIGID_CHART_TOLERANCE
    };
    let point_row = |point: cadmpeg_ir::math::Point2| {
        let point = transform.apply_point(point);
        (point.u.is_finite() && point.v.is_finite()).then(|| format!("{} {}", point.u, point.v))
    };

    Some(match value {
        PcurveGeometry::Line(value) => {
            let origin = value.origin().get();
            let direction = value.direction().as_raw();
            let transformed_direction = transform.apply_vector(*direction);
            let norm = transformed_direction.u.hypot(transformed_direction.v);
            if !norm.is_finite() || norm == 0.0 {
                return None;
            }
            if (norm - 1.0).abs() <= RIGID_CHART_TOLERANCE {
                format!(
                    "1 {} {} {}",
                    point_row(origin)?,
                    transformed_direction.u,
                    transformed_direction.v
                )
            } else {
                let [start, end] = domain?;
                if start >= end {
                    return None;
                }
                let at = |t: f64| {
                    cadmpeg_ir::math::Point2::new(
                        origin.u + t * direction.u,
                        origin.v + t * direction.v,
                    )
                };
                format!(
                    "7 0 0 1 2 2 {} {} {start} 2 {end} 2",
                    point_row(at(start))?,
                    point_row(at(end))?
                )
            }
        }
        PcurveGeometry::Circle(value) if rigid => format!(
            "2 {} {} {} {}",
            point_row(value.center().get())?,
            vector_row(value.x_axis())?,
            vector_row(value.y_axis())?,
            value.radius().get()
        ),
        PcurveGeometry::Ellipse(value) if rigid => format!(
            "3 {} {} {} {} {}",
            point_row(value.center().get())?,
            vector_row(value.x_axis())?,
            vector_row(value.y_axis())?,
            value.major_radius().get(),
            value.minor_radius().get()
        ),
        PcurveGeometry::Parabola(value) if rigid => format!(
            "4 {} {} {} {}",
            point_row(value.vertex().get())?,
            vector_row(value.x_axis())?,
            vector_row(value.y_axis())?,
            value.focal_distance().get()
        ),
        PcurveGeometry::Hyperbola(value) if rigid => format!(
            "5 {} {} {} {} {}",
            point_row(value.center().get())?,
            vector_row(value.x_axis())?,
            vector_row(value.y_axis())?,
            value.major_radius().get(),
            value.minor_radius().get()
        ),
        PcurveGeometry::Nurbs { nurbs } => {
            let poles = nurbs.control_points();
            let axis = super::geometry::nurbs::axis(
                nurbs.knots(),
                nurbs.degree(),
                poles.len(),
                nurbs.periodic(),
            )?;
            let weights = nurbs.weights();
            if !super::geometry::nurbs::cyclic_tail(&poles, &axis)
                || weights.as_ref().is_some_and(|weights| {
                    !super::geometry::nurbs::cyclic_tail(weights, &axis)
                        || weights.iter().any(|weight| weight.get() <= 0.0)
                })
            {
                return None;
            }
            let mut output = format!(
                "7 {} {} {} {} {}",
                u8::from(weights.is_some()),
                u8::from(nurbs.periodic()),
                nurbs.degree(),
                axis.count,
                axis.rows.len()
            );
            for (index, pole) in poles.iter().take(axis.count).enumerate() {
                output.push(' ');
                output.push_str(&point_row(pole.get())?);
                if let Some(weights) = &weights {
                    output.push(' ');
                    output.push_str(&weights[index].get().to_string());
                }
            }
            for (knot, multiplicity) in axis.rows {
                output.push(' ');
                output.push_str(&knot.to_string());
                output.push(' ');
                output.push_str(&multiplicity.to_string());
            }
            output
        }
        PcurveGeometry::Trimmed(value) if value.same_sense() => {
            let range = value.parameter_range().endpoints();
            format!(
                "8 {} {} {}",
                range[0],
                range[1],
                transformed(value.basis(), Some(range), transform)?
            )
        }
        PcurveGeometry::Offset(value)
            if transform == cadmpeg_ir::transform::Transform2::identity() =>
        {
            format!(
                "9 {} {}",
                value.distance().get(),
                transformed(value.basis(), domain, transform)?
            )
        }
        _ => return None,
    })
}

fn compose(
    left: cadmpeg_ir::transform::Transform2,
    right: cadmpeg_ir::transform::Transform2,
) -> Option<cadmpeg_ir::transform::Transform2> {
    let a = left.affine_rows();
    let b = right.rows();
    let rows = [0, 1]
        .map(|i| [0, 1, 2].map(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j]));
    cadmpeg_ir::transform::Transform2::affine(rows)
}
