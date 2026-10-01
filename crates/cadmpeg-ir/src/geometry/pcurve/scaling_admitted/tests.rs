// SPDX-License-Identifier: Apache-2.0
use super::{
    CodecError, DecodeContext, HarmonicPcurve, HyperbolicPcurve, LinePcurve, PcurveGeometry,
    Point2, Transform2,
};
use crate::geometry::pcurve::{
    CirclePcurve, EllipsePcurve, HyperbolaPcurve, OffsetPcurve, ParabolaPcurve, PcurveNurbs,
    PlacedPcurve, TrimmedPcurve,
};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

fn with_limits<T>(work: u64, depth: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_recursion_depth = depth;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    run(&ctx)
}

fn nurbs(x: f64, rational: bool) -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(x, 1.0); 2],
            rational.then(|| vec![1.0, 2.0]),
            false,
        )
        .expect("curve"),
    }
}

#[test]
fn owned_pcurve_scaling_refuses_pole_work_and_reuses_both_pole_forms() {
    for rational in [false, true] {
        for cap in [1, 2] {
            assert!(
                matches!(with_limits(cap, u64::MAX, |ctx| nurbs(1.0, rational).scaled_coordinates_owned(ctx, [2.0, 3.0])),
                Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR pcurve pole coordinate scaling work")
            );
        }
        let mut expected = nurbs(1.0, rational);
        expected
            .try_scale_coordinates([2.0, 3.0])
            .expect("reference");
        assert_eq!(
            with_limits(3, 0, |ctx| nurbs(1.0, rational)
                .scaled_coordinates_owned(ctx, [2.0, 3.0]))
            .expect("no copies"),
            Ok(expected)
        );
        assert_eq!(
            with_limits(2, 0, |ctx| nurbs(f64::MAX, rational)
                .scaled_coordinates_owned(ctx, [2.0, 3.0]))
            .expect("borrowed refusal"),
            Err("control_points contains a non-finite point")
        );
    }
}

#[test]
fn owned_pcurve_scaling_refuses_each_wrapper_depth_and_keeps_service_geometry() {
    let wrapped = [
        PcurveGeometry::Trimmed(
            TrimmedPcurve::try_new([0.0, 1.0], true, Box::new(nurbs(1.0, false))).expect("trim"),
        ),
        PcurveGeometry::Offset(
            OffsetPcurve::try_new(2.0, Box::new(nurbs(1.0, false))).expect("offset"),
        ),
        PcurveGeometry::Transformed(
            PlacedPcurve::try_new(Box::new(nurbs(1.0, false)), Transform2::identity())
                .expect("placed"),
        ),
    ];
    for geometry in wrapped {
        assert!(
            matches!(with_limits(u64::MAX, 0, |ctx| geometry.clone().scaled_coordinates_owned(ctx, [2.0, 2.0])),
            Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR pcurve coordinate scaling nesting")
        );
        assert!(
            matches!(with_limits(0, 1, |ctx| geometry.clone().scaled_coordinates_owned(ctx, [2.0, 2.0])),
            Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR pcurve coordinate scaling work")
        );
        let mut expected = geometry.clone();
        expected
            .try_scale_coordinates([2.0, 2.0])
            .expect("reference");
        assert_eq!(
            with_limits(4, 1, |ctx| geometry
                .scaled_coordinates_owned(ctx, [2.0, 2.0]))
            .expect("no copies"),
            Ok(expected)
        );
    }
}

#[test]
fn owned_pcurve_scaling_preserves_analytic_conversion_and_refusal_order() {
    let p = |u, v| Point2::new(u, v);
    let geometry = [
        PcurveGeometry::Line(LinePcurve::U_AXIS),
        PcurveGeometry::Circle(
            CirclePcurve::try_new(p(1.0, 2.0), p(1.0, 0.0), p(0.0, 1.0), 2.0).expect("circle"),
        ),
        PcurveGeometry::Ellipse(
            EllipsePcurve::try_new(p(1.0, 2.0), p(1.0, 0.0), p(0.0, 1.0), 3.0, 2.0)
                .expect("ellipse"),
        ),
        PcurveGeometry::Parabola(
            ParabolaPcurve::try_new(p(1.0, 2.0), p(1.0, 0.0), p(0.0, 1.0), 2.0).expect("parabola"),
        ),
        PcurveGeometry::Hyperbola(
            HyperbolaPcurve::try_new(p(1.0, 2.0), p(1.0, 0.0), p(0.0, 1.0), 3.0, 2.0)
                .expect("hyperbola"),
        ),
        PcurveGeometry::Harmonic(
            HarmonicPcurve::try_new(p(1.0, 2.0), p(1.0, 0.0), p(0.0, 1.0)).expect("harmonic"),
        ),
        PcurveGeometry::Hyperbolic(
            HyperbolicPcurve::try_new(p(1.0, 2.0), p(1.0, 0.0), p(0.0, 1.0)).expect("hyperbolic"),
        ),
    ];
    for geometry in geometry {
        for scales in [[2.0, 3.0], [2.0, 2.0], [f64::MAX, f64::MAX], [0.0, 0.0]] {
            let mut expected = geometry.clone();
            let expected = expected.try_scale_coordinates(scales).map(|()| expected).map_err(|error| error.to_string());
            let actual = with_limits(u64::MAX, u64::MAX, |ctx| {
                geometry
                    .clone()
                    .scaled_coordinates_owned(ctx, scales)
            })
            .expect("admitted");
            assert_eq!(actual.map_err(str::to_owned), expected);
        }
    }
    for geometry in [
        PcurveGeometry::Offset(
            OffsetPcurve::try_new(f64::MAX, Box::new(PcurveGeometry::Line(LinePcurve::U_AXIS)))
                .expect("offset"),
        ),
        PcurveGeometry::Transformed(
            PlacedPcurve::try_new(
                Box::new(PcurveGeometry::Line(LinePcurve::U_AXIS)),
                Transform2::identity(),
            )
            .expect("placed"),
        ),
    ] {
        for scales in [[2.0, 2.0], [2.0, 3.0], [0.0, 0.0]] {
            let mut expected = geometry.clone();
            let expected = expected.try_scale_coordinates(scales).map(|()| expected).map_err(|error| error.to_string());
            let actual = with_limits(u64::MAX, u64::MAX, |ctx| {
                geometry
                    .clone()
                    .scaled_coordinates_owned(ctx, scales)
            })
            .expect("admitted");
            assert_eq!(actual.map_err(str::to_owned), expected);
        }
    }
}
