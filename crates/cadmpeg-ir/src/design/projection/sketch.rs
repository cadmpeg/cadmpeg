// SPDX-License-Identifier: Apache-2.0
//! The v11 curve of a decoded sketch entity, shared by each projection of
//! the decoded sketches and by the comparison of a projected entity with
//! its decoded record.

use crate::design::ops::sketch::PlanarCurve;
use crate::math::portable::Portable;
use crate::sketches::{SketchGeometry, SketchGeometryDefinition};
use crate::units::UnitVector2;

/// The v11 curve of `geometry`, or `None` for a geometry the planar curve
/// vocabulary does not hold as decoded.
#[must_use]
pub fn planar_curve(geometry: &SketchGeometry) -> Option<PlanarCurve> {
    Some(match geometry.definition() {
        SketchGeometryDefinition::Point { position } => PlanarCurve::Point(*position),
        SketchGeometryDefinition::Line { start, end } => PlanarCurve::Line {
            start: *start,
            end: *end,
        },
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            let [u, v] = [direction.get().u, direction.get().v];
            let length = u.portable_hypot(v);
            PlanarCurve::InfiniteLine {
                through: *origin,
                direction: UnitVector2::new([u / length, v / length])?,
            }
        }
        SketchGeometryDefinition::Circle { center, radius } => PlanarCurve::Circle {
            center: *center,
            radius: *radius,
        },
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => PlanarCurve::Arc {
            center: *center,
            radius: *radius,
            start: *start_angle,
            end: *end_angle,
        },
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let (sin, cos) = major_angle.get().portable_sin_cos();
            PlanarCurve::Ellipse {
                center: *center,
                major: UnitVector2::new([cos, sin])?,
                radii: [radii.major(), radii.minor()],
                range: *bounds,
            }
        }
        SketchGeometryDefinition::Nurbs { curve } => PlanarCurve::Nurbs(curve.clone()),
        SketchGeometryDefinition::Hyperbola { .. }
        | SketchGeometryDefinition::Parabola { .. }
        | SketchGeometryDefinition::Text { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => return None,
    })
}

/// The start and end of a line or an arc in sketch coordinates, or `None`
/// for another geometry.
#[must_use]
pub fn curve_ends(geometry: &SketchGeometry) -> Option<[[f64; 2]; 2]> {
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some([[start.u, start.v], [end.u, end.v]]),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let at = |angle: f64| {
                let (sin, cos) = angle.portable_sin_cos();
                [center.u + radius.get() * cos, center.v + radius.get() * sin]
            };
            Some([at(start_angle.get()), at(end_angle.get())])
        }
        _ => None,
    }
}
