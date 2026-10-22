// SPDX-License-Identifier: Apache-2.0
//! `ON_Surface::ISO` classification of the actual written trim carrier.

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, pcurve::PcurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;

use super::model::{WritableCoedge, WritableEdgeCurve, WritableFaceSurface, WritableModel};

// Native openNURBS parameter and line-classification constants. These do not
// change the document's geometry, fitting allowance, or validation tolerance.
const NATIVE_SQRT_EPSILON: f64 = 1.490_116_119_385e-8;
const NATIVE_ZERO_TOLERANCE: f64 = 2.328_306_436_538_696_3e-10;

struct Carrier {
    points: Vec<[f64; 2]>,
    clamped: bool,
}

impl Carrier {
    fn line(from: [f64; 2], to: [f64; 2]) -> Self {
        Self {
            points: vec![from, to],
            clamped: true,
        }
    }

    fn nurbs(
        curve: &NurbsCurve,
        mut project: impl FnMut(Point3) -> [f64; 2],
    ) -> Result<Self, CodecError> {
        let degree = usize::try_from(curve.degree()).map_err(CodecError::malformed)?;
        let count = curve.pole_count();
        let knots = curve.knots();
        let clamped = knots[..=degree].iter().all(|knot| *knot == knots[degree])
            && knots[count..].iter().all(|knot| *knot == knots[count]);
        let weights = curve.weights();
        let points = curve
            .control_points()
            .iter()
            .enumerate()
            .map(|(index, point)| {
                let mut uv = project(point.get());
                if let Some(weights) = &weights {
                    let weight = weights[index].get();
                    // Match the Euclidean CV that the native reader recovers from
                    // the homogeneous coordinates written by this encoder.
                    for coordinate in &mut uv {
                        *coordinate = (*coordinate * weight) / weight;
                    }
                }
                uv
            })
            .collect();
        Ok(Self { points, clamped })
    }
}

pub(super) fn classify(
    model: &WritableModel<'_>,
    coedge: &WritableCoedge<'_>,
) -> Result<i32, CodecError> {
    let face = &model.faces[model.loops[coedge.owner_loop].face];
    let surface = model.surfaces[face.surface];
    let edge = &model.edges[coedge.edge];
    let domain = match surface {
        WritableFaceSurface::Plane { .. } => [super::PLANE_PARAMETER_DOMAIN; 2],
        WritableFaceSurface::Nurbs { surface, .. } => [
            [
                surface.u_knots()
                    [usize::try_from(surface.u_degree()).map_err(CodecError::malformed)?],
                surface.u_knots()[surface.u_count()],
            ],
            [
                surface.v_knots()
                    [usize::try_from(surface.v_degree()).map_err(CodecError::malformed)?],
                surface.v_knots()[surface.v_count()],
            ],
        ],
    };
    let carrier = if let Some(pcurve) = coedge.explicit_pcurve {
        match &pcurve.geometry {
            PcurveGeometry::Line(line) => {
                let origin = line.origin().as_raw();
                let direction = line.direction().as_raw();
                let [start, end] = FiniteReal::raw_array(edge.domain);
                Carrier::line(
                    [
                        origin.u + start * direction.u,
                        origin.v + start * direction.v,
                    ],
                    [origin.u + end * direction.u, origin.v + end * direction.v],
                )
            }
            PcurveGeometry::Nurbs { nurbs } => {
                let lifted = nurbs
                    .lift(|uv| Point3::new(uv.u, uv.v, 0.0))
                    .map_err(CodecError::malformed)?;
                Carrier::nurbs(&lifted, |point| [point.x, point.y])?
            }
            _ => {
                return Err(CodecError::malformed(
                    "admitted Rhino trim lost its line or NURBS carrier",
                ))
            }
        }
    } else {
        let WritableFaceSurface::Plane {
            origin,
            normal,
            u_axis,
        } = surface
        else {
            return Err(CodecError::malformed(
                "admitted Rhino NURBS trim lost its explicit pcurve",
            ));
        };
        let project = |point| {
            let uv = super::plane_uv(point, origin, u_axis, normal.cross(u_axis));
            [uv[0], uv[1]]
        };
        match edge.curve {
            WritableEdgeCurve::Line(_) => Carrier::line(
                project(model.vertices[edge.start].point),
                project(model.vertices[edge.end].point),
            ),
            WritableEdgeCurve::Nurbs(curve) => Carrier::nurbs(curve, project)?,
        }
    };
    Ok(classify_carrier(&carrier, domain))
}

fn parameter_tolerance([start, end]: [f64; 2], at: f64) -> [f64; 2] {
    let span = end - start;
    let delta = span * 8.0 * NATIVE_SQRT_EPSILON + (start.abs() + end.abs()) * f64::EPSILON;
    let delta = if delta >= span { span * 0.5 } else { delta };
    let at = at.clamp(start, end);
    [at - delta, at + delta]
}

fn classify_carrier(carrier: &Carrier, domain: [[f64; 2]; 2]) -> i32 {
    let min = carrier
        .points
        .iter()
        .fold([f64::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
    let max = carrier.points.iter().fold([f64::NEG_INFINITY; 2], |a, p| {
        [a[0].max(p[0]), a[1].max(p[1])]
    });
    let span = [domain[0][1] - domain[0][0], domain[1][1] - domain[1][0]];
    let width = [max[0] - min[0], max[1] - min[1]];
    if span.iter().any(|value| *value <= 0.0)
        || (width[0] > span[0] / 32.0 && width[1] > span[1] / 32.0)
    {
        return 0;
    }
    let axis = usize::from(width[0] * span[1] > width[1] * span[0]);
    let [start, end] = domain[axis];
    let band = span[axis] / 32.0;
    let within = |at| {
        let [lower, upper] = parameter_tolerance(domain[axis], at);
        (if axis == 0 {
            lower <= min[axis]
        } else {
            lower < min[axis]
        }) && max[axis] <= upper
    };
    let iso = if max[axis] <= start + band && within(start) {
        if axis == 0 {
            3
        } else {
            4
        }
    } else if min[axis] >= end - band && within(end) {
        if axis == 0 {
            5
        } else {
            6
        }
    } else {
        // Preserve the native surface classifier's component test, including
        // its x-component test in the horizontal case.
        let intersects = start < max[0] || min[0] < end;
        if intersects && within(min[axis].midpoint(max[axis])) {
            if axis == 0 {
                1
            } else {
                2
            }
        } else {
            0
        }
    };
    let mut tolerance = width[axis];
    if tolerance < NATIVE_ZERO_TOLERANCE && NATIVE_ZERO_TOLERANCE * 1024.0 <= width[1 - axis] {
        tolerance = NATIVE_ZERO_TOLERANCE;
    }
    if iso != 0
        && is_linear(
            carrier,
            if tolerance > 0.0 {
                tolerance
            } else {
                NATIVE_ZERO_TOLERANCE
            },
        )
    {
        iso
    } else {
        0
    }
}

fn is_linear(carrier: &Carrier, tolerance: f64) -> bool {
    if !carrier.clamped {
        return false;
    }
    let (Some(&from), Some(&to)) = (carrier.points.first(), carrier.points.last()) else {
        return false;
    };
    let direction = [to[0] - from[0], to[1] - from[1]];
    let length_squared = direction[0] * direction[0] + direction[1] * direction[1];
    if !(length_squared.is_finite() && length_squared > 0.0) {
        return false;
    }
    let mut previous = 0.0_f64;
    for point in &carrier.points[1..carrier.points.len() - 1] {
        let t = ((point[0] - from[0]) * direction[0] + (point[1] - from[1]) * direction[1])
            / length_squared;
        let nearest = [
            (1.0 - t) * from[0] + t * to[0],
            (1.0 - t) * from[1] + t * to[1],
        ];
        let distance = (point[0] - nearest[0]).hypot(point[1] - nearest[1]);
        if distance > tolerance {
            return false;
        }
        if t > previous && previous < 1.0 {
            previous = t.min(1.0);
        }
        if !(previous..=1.0).contains(&t)
            && (t - previous).abs() * length_squared.sqrt() > tolerance
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests;
