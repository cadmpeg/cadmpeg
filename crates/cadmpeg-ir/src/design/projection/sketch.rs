// SPDX-License-Identifier: Apache-2.0
//! The v11 curve of a decoded sketch entity, shared by each projection of
//! the decoded sketches and by the comparison of a projected entity with
//! its decoded record.

use crate::design::ops::sketch::PlanarCurve;
use crate::math::portable::Portable;
use crate::sketches::{SketchGeometry, SketchGeometryDefinition as Geometry};
use crate::units::UnitVector2;

/// The v11 curve of `geometry`, or `None` for a geometry the planar curve
/// vocabulary does not hold as decoded.
#[must_use]
pub fn planar_curve(geometry: &SketchGeometry) -> Option<PlanarCurve> {
    Some(match geometry.definition() {
        Geometry::Point { position } => PlanarCurve::Point(*position),
        Geometry::Line { start, end } => PlanarCurve::Line {
            start: *start,
            end: *end,
        },
        Geometry::ReferenceLine { origin, direction } => {
            let [u, v] = [direction.get().u, direction.get().v];
            let length = u.portable_hypot(v);
            PlanarCurve::InfiniteLine {
                through: *origin,
                direction: UnitVector2::new([u / length, v / length])?,
            }
        }
        Geometry::Circle { center, radius } => PlanarCurve::Circle {
            center: *center,
            radius: *radius,
        },
        Geometry::Arc {
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
        Geometry::Ellipse {
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
        Geometry::Nurbs { curve } => PlanarCurve::Nurbs(curve.clone()),
        Geometry::Hyperbola { .. }
        | Geometry::Parabola { .. }
        | Geometry::Text { .. }
        | Geometry::ExternalReference { .. }
        | Geometry::Native { .. } => return None,
    })
}
