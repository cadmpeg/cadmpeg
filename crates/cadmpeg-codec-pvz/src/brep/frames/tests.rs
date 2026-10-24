// SPDX-License-Identifier: Apache-2.0
use std::f64::consts::{PI, TAU};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{CurveGeometry as IrCurve, SurfaceGeometry as IrSurface};

use super::{
    analytic_chart, bspline_chart, edge_curve, pcurve, CarrierRefusal, UvMap, MILLIMETRES_PER_METRE,
};
use crate::brep::eval::{curve_interval, curve_point, distance, scale, surface_point};
use crate::brep::model::{
    BsplineCurve, BsplineSurface, Conic, CurveGeometry, SurfaceGeometry, Torus,
};

/// Agreement between a native point and its neutral image, in millimetres.
const EPS_IMAGE_MM: f64 = 1.0e-9;

const SAMPLES: [[f64; 2]; 4] = [[0.0, 0.0], [0.3, 0.7], [2.0, -0.4], [-1.1, 1.3]];
/// Samples inside the v domain of a B-spline that is periodic in u only.
const SPLINE_SAMPLES: [[f64; 2]; 3] = [[0.3, 0.7], [2.25, 0.5], [-0.75, 0.2]];

fn with_context(check: impl FnOnce(&DecodeContext<'_>)) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    check(&ctx);
}

/// The neutral chart of `native` evaluates at `M·(u, v)` to the native
/// point at `(u, v)`.
fn assert_chart_matches(native: &SurfaceGeometry) {
    let (chart, samples) = match native {
        SurfaceGeometry::Bspline(surface) => (bspline_chart(surface).unwrap(), &SPLINE_SAMPLES[..]),
        other => (analytic_chart(other).unwrap(), &SAMPLES[..]),
    };
    let geometry = IrSurface::Solved(chart.geometry.clone());
    with_context(|ctx| {
        for uv in samples.iter().copied() {
            let expected = scale(surface_point(native, uv).unwrap(), MILLIMETRES_PER_METRE);
            let [u, v] = chart.map.point(uv);
            let point = cadmpeg_ir::eval::decode::surface_point(ctx, &geometry, u, v)
                .unwrap()
                .get();
            assert!(
                distance(expected, [point.x, point.y, point.z]) <= EPS_IMAGE_MM,
                "{native:?} at {uv:?}"
            );
        }
    });
}

#[test]
fn plane_chart_projects_skew_native_axes() {
    assert_chart_matches(&SurfaceGeometry::Plane {
        origin: [0.1, 0.2, 0.3],
        u_axis: [0.002, 0.001, 0.0],
        v_axis: [-0.001, 0.003, 0.0],
        normal: [0.0, 0.0, -2.0],
    });
}

#[test]
fn cylinder_chart_keeps_or_turns_the_angle() {
    for y in [[0.0, 0.02, 0.0], [0.0, -0.02, 0.0]] {
        assert_chart_matches(&SurfaceGeometry::Cylinder {
            origin: [0.0, 0.0, 0.1],
            x: [0.02, 0.0, 0.0],
            y,
            axis: [0.0, 0.0, 0.5],
        });
    }
}

#[test]
fn cone_chart_handles_both_signs_of_a_and_b() {
    for (a, b, y) in [
        (1.0, 0.5, [0.0, 0.01, 0.0]),
        (-1.0, 0.5, [0.0, 0.01, 0.0]),
        (2.0, -0.25, [0.0, -0.01, 0.0]),
        (0.0, 1.0, [0.0, 0.01, 0.0]),
    ] {
        assert_chart_matches(&SurfaceGeometry::Cone {
            origin: [0.0; 3],
            x: [0.01, 0.0, 0.0],
            y,
            axis: [0.0, 0.0, 0.02],
            a,
            b,
        });
    }
}

#[test]
fn sphere_chart_follows_the_handedness_of_its_frame() {
    for y in [[0.0, 0.05, 0.0], [0.0, -0.05, 0.0]] {
        assert_chart_matches(&SurfaceGeometry::Sphere {
            center: [0.0, 0.1, 0.0],
            x: [0.05, 0.0, 0.0],
            y,
            z: [0.0, 0.0, 0.05],
        });
    }
}

#[test]
fn torus_chart_covers_every_minor_frame_sign() {
    for side in [1.0, -1.0] {
        for lift in [1.0, -1.0] {
            for turn in [1.0, -1.0] {
                let minor = 0.01;
                assert_chart_matches(&SurfaceGeometry::Torus(Torus {
                    origin: [0.0; 3],
                    major_cos: [0.05, 0.0, 0.0],
                    major_sin: [0.0, 0.05 * turn, 0.0],
                    axis: [0.0, 0.0, 1.0],
                    minor_cos: [side * minor, 0.0, 0.0],
                    minor_axial: [0.0, 0.0, lift * minor],
                    minor_sin: [0.0, -side * turn, 0.0],
                }));
            }
        }
    }
}

#[test]
fn bspline_chart_keeps_the_native_domain() {
    assert_chart_matches(&SurfaceGeometry::Bspline(BsplineSurface {
        rational: true,
        u_order: 2,
        v_order: 2,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        poles: vec![
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.02, 0.0, 2.0],
            [0.01, 0.0, 0.0, 1.0],
            [0.02, 0.02, 0.01, 2.0],
        ],
        periodic: [true, false],
    }));
}

#[test]
fn frames_that_break_their_conditions_are_refused() {
    let skew = SurfaceGeometry::Cylinder {
        origin: [0.0; 3],
        x: [1.0, 0.0, 0.0],
        y: [0.5, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
    };
    assert!(matches!(
        analytic_chart(&skew),
        Err(CarrierRefusal::Frame(_))
    ));
    let flat_torus = SurfaceGeometry::Torus(Torus {
        origin: [0.0; 3],
        major_cos: [0.0; 3],
        major_sin: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        minor_cos: [1.0, 0.0, 0.0],
        minor_axial: [0.0, 0.0, 1.0],
        minor_sin: [0.0, -1.0, 0.0],
    });
    assert!(analytic_chart(&flat_torus).is_err());
}

/// The neutral curve evaluates at the mapped parameter to the native point.
fn assert_curve_matches(native: &CurveGeometry<3>) {
    let chart = edge_curve(native).unwrap();
    let geometry = IrCurve::Solved(chart.geometry.clone());
    let [low, high] = curve_interval(native).unwrap();
    with_context(|ctx| {
        for fraction in [0.0, 0.25, 0.5, 1.0] {
            let t = low + (high - low) * fraction;
            let expected = scale(curve_point(native, t).unwrap(), MILLIMETRES_PER_METRE);
            let point = cadmpeg_ir::eval::decode::curve_point(ctx, &geometry, chart.map(t))
                .unwrap()
                .get();
            assert!(
                distance(expected, [point.x, point.y, point.z]) <= EPS_IMAGE_MM,
                "{native:?} at {t}"
            );
        }
    });
}

#[test]
fn edge_curves_keep_their_points_under_the_parameter_map() {
    assert_curve_matches(&CurveGeometry::Line {
        origin: [0.0, 0.1, 0.0],
        direction: [0.02, 0.0, 0.01],
    });
    assert_curve_matches(&CurveGeometry::Circle(Conic {
        center: [0.0; 3],
        x: [0.0, 0.01, 0.0],
        y: [0.0, 0.0, 0.01],
        range: Some([0.5, 2.0]),
    }));
    for (x, y) in [
        ([0.03, 0.0, 0.0], [0.0, 0.01, 0.0]),
        ([0.01, 0.0, 0.0], [0.0, 0.03, 0.0]),
    ] {
        assert_curve_matches(&CurveGeometry::Ellipse(Conic {
            center: [0.0; 3],
            x,
            y,
            range: None,
        }));
    }
    assert_curve_matches(&CurveGeometry::Bspline(BsplineCurve {
        rational: true,
        order: 3,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        weighted: vec![[0.01, 0.0, 0.0], [0.005, 0.005, 0.0], [0.0, 0.01, 0.0]],
        weights: vec![1.0, 0.5, 1.0],
        range: None,
    }));
    assert_curve_matches(&CurveGeometry::Polyline(vec![
        [0.0, 0.0, 0.0],
        [0.01, 0.0, 0.0],
        [0.01, 0.01, 0.0],
    ]));
}

#[test]
fn conic_interval_starts_in_the_first_turn() {
    let chart = edge_curve(&CurveGeometry::Circle(Conic {
        center: [0.0; 3],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        range: None,
    }))
    .unwrap();
    let [start, end] = chart.interval([-PI, 0.5]);
    assert!((0.0..TAU).contains(&start));
    assert!((end - start - (0.5 + PI)).abs() <= f64::EPSILON * 8.0);
}

#[test]
fn pcurves_map_through_the_surface_chart() {
    let map = UvMap {
        linear: [[2.0, 0.5], [0.0, -1.0]],
        offset: [1.0, 3.0],
    };
    let curves = [
        CurveGeometry::Line {
            origin: [0.5, 0.5],
            direction: [1.0, 2.0],
        },
        CurveGeometry::Ellipse(Conic {
            center: [0.0, 1.0],
            x: [1.0, 0.0],
            y: [0.0, 0.5],
            range: None,
        }),
        CurveGeometry::Bspline(BsplineCurve {
            rational: true,
            order: 2,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            weighted: vec![[0.0, 0.0], [2.0, 4.0]],
            weights: vec![1.0, 2.0],
            range: None,
        }),
        CurveGeometry::Polyline(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]),
    ];
    with_context(|ctx| {
        for native in &curves {
            let geometry = pcurve(native, &map).unwrap();
            let [low, high] = curve_interval(native).unwrap();
            for fraction in [0.0, 0.4, 1.0] {
                let t = low + (high - low) * fraction;
                let expected = map.point(curve_point(native, t).unwrap());
                let uv = cadmpeg_ir::eval::decode::pcurve_uv(ctx, &geometry, t)
                    .unwrap()
                    .get();
                assert!(
                    distance(expected, [uv.u, uv.v]) <= EPS_IMAGE_MM,
                    "{native:?} at {t}"
                );
            }
        }
    });
}
