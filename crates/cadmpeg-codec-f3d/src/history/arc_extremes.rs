// SPDX-License-Identifier: Apache-2.0

/// The points of a circular arc that are extreme along a model axis. The arc
/// runs from `start` through `interior` to `end` on the circle of `center`,
/// unit `normal` and `radius`; a closed arc is the whole circle. An open arc
/// without an interior point gives none, because its side is unknown.
pub(super) fn circle_arc_extremes(
    center: cadmpeg_ir::math::Point3,
    normal: cadmpeg_ir::math::Vector3,
    radius: f64,
    start: cadmpeg_ir::math::Point3,
    interior: Option<cadmpeg_ir::math::Point3>,
    end: cadmpeg_ir::math::Point3,
    closed: bool,
) -> Vec<cadmpeg_ir::math::Point3> {
    use cadmpeg_ir::math::Vector3;
    use std::f64::consts::TAU;
    let first = start.vector_from(center).scale(radius.recip());
    let second = normal.cross(first);
    let angle = |point: cadmpeg_ir::math::Point3| {
        let offset = point.vector_from(center);
        offset.dot(second).atan2(offset.dot(first)).rem_euclid(TAU)
    };
    let span = if closed {
        None
    } else {
        let Some(interior) = interior else {
            return Vec::new();
        };
        let (middle, last) = (angle(interior), angle(end));
        // The arc runs counterclockwise about `normal` when the interior
        // point lies before the end in that sense.
        Some(if middle < last {
            (true, last)
        } else {
            (false, last)
        })
    };
    let mut out = Vec::new();
    for axis in [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ] {
        let along = axis - normal.scale(axis.dot(normal));
        let length = along.norm();
        if length <= super::EPS_HISTORY_CIRCLE_EDGE_E7 {
            continue;
        }
        for sign in [1.0, -1.0] {
            let point = center.translated(along, sign * radius / length);
            let keep = match span {
                None => true,
                Some((true, last)) => angle(point) <= last,
                Some((false, last)) => angle(point) >= last || angle(point) == 0.0,
            };
            if keep {
                out.push(point);
            }
        }
    }
    out
}
