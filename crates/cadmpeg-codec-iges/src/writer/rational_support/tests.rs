// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::FRAC_PI_2;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::eval::decode::pcurve_uv;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::math::Point2;

use super::{rational_line, Slope, UnitArc};

/// Bound on the distance of an image angle from the parameter line.
const ON_LINE: f64 = 1.0e-9;

/// The angle of the point at arc parameter `parameter`.
fn angle(arc: &UnitArc, parameter: f64) -> f64 {
    let [start, end] = arc.span(parameter).expect("a parameter on the arc");
    let sweep = end - start;
    let fraction = (parameter - start) / sweep;
    let half = (2.0 * fraction - 1.0) * (0.25 * sweep).tan();
    2.0f64.mul_add(half.atan(), start + 0.5 * sweep)
}

#[test]
fn the_image_of_a_parameter_line_of_whole_slope_stays_on_the_line() {
    let u_arc = UnitArc::new([0.0, 2.5]).expect("a u arc");
    let v_arc = UnitArc::new([-FRAC_PI_2, FRAC_PI_2]).expect("a v arc");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let range = [3.0, 5.0];
    for (p, q, start, end) in [
        (1, 1, [0.2, -1.2], [1.2, -0.2]),
        (-2, 1, [2.3, -1.0], [0.3, 0.0]),
        (1, 3, [0.5, -1.4], [1.3, 1.0]),
    ] {
        let (start, end) = (Point2::new(start[0], start[1]), Point2::new(end[0], end[1]));
        let nurbs = rational_line(&u_arc, &v_arc, start, end, Slope { p, q }, range)
            .expect("the line has an exact image");
        let geometry = PcurveGeometry::Nurbs { nurbs };
        for step in 0..=32 {
            let parameter = (f64::from(step) / 32.0).mul_add(range[1] - range[0], range[0]);
            let point = pcurve_uv(&ctx, &geometry, parameter)
                .expect("a parameter point")
                .get();
            let (u, v) = (angle(&u_arc, point.u), angle(&v_arc, point.v));
            let off_line =
                (u - start.u).mul_add(end.v - start.v, -(v - start.v) * (end.u - start.u));
            assert!(
                off_line.abs() <= ON_LINE,
                "slope {p}/{q}: angles ({u}, {v}) at {parameter} lie off the line"
            );
            for (label, at, expected) in [("start", 0, start), ("end", 32, end)] {
                if step == at {
                    assert!(
                        (u - expected.u).abs() <= ON_LINE && (v - expected.v).abs() <= ON_LINE,
                        "slope {p}/{q}: the image {label} ({u}, {v}) differs from {expected:?}"
                    );
                }
            }
        }
    }
}
