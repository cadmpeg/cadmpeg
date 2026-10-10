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

#[test]
fn standard_pcurve_scaling_accepts_storage_beyond_empty_input_decode_allowance() {
    use crate::geometry::nurbs::StandardNurbsAdmission;
    use crate::geometry::pcurve::PcurveNurbsPoles;
    use crate::units::FinitePoint2;

    const OLD_EMPTY_INPUT_ALLOWANCE: usize = 16 * 1024 * 1024;
    const POLE_COUNT: usize = OLD_EMPTY_INPUT_ALLOWANCE / std::mem::size_of::<FinitePoint2>() + 1;
    let point = FinitePoint2::new(Point2::new(1.0, -2.0)).unwrap();
    let points: Vec<_> = (0..POLE_COUNT).map(|_| point).collect();
    assert!(points.len() * std::mem::size_of::<FinitePoint2>() > OLD_EMPTY_INPUT_ALLOWANCE);
    let knots: Vec<_> = (0..POLE_COUNT + 2).map(|index| index as f64).collect();
    let nurbs = crate::geometry::pcurve::construction::build_pcurve(
        &StandardNurbsAdmission,
        1,
        knots,
        PcurveNurbsPoles::Polynomial { points },
        true,
    ).expect("actual Standard construction");
    let mut geometry = PcurveGeometry::Nurbs { nurbs };
    geometry.try_scale_coordinates([2.0, 3.0]).expect("Standard scaling");
    let PcurveGeometry::Nurbs { nurbs } = geometry else { panic!("same NURBS carrier"); };
    assert_eq!(nurbs.degree(), 1);
    assert!(nurbs.periodic());
    assert_eq!(nurbs.knots().len(), POLE_COUNT + 2);
    assert!(nurbs.knots().iter().enumerate().all(|(index, knot)| *knot == index as f64));
    let PcurveNurbsPoles::Polynomial { points } = nurbs.poles else { panic!("same pole form"); };
    assert_eq!(points.len(), POLE_COUNT);
    assert!(points.iter().all(|point| point.get() == Point2::new(2.0, -6.0)));
}

#[test]
fn standard_pcurve_scaling_keeps_all_rows_after_later_pole_refusal() {
    use crate::geometry::pcurve::{PcurveCoordinateScaleError, PcurveNurbsPoles};
    use crate::units::FinitePoint2;

    for rational in [false, true] {
        let mut geometry = nurbs(1.0, rational);
        let PcurveGeometry::Nurbs { nurbs } = &mut geometry else { panic!("fixture NURBS"); };
        let point = FinitePoint2::new(Point2::new(f64::MAX, 1.0)).unwrap();
        match &mut nurbs.poles {
            PcurveNurbsPoles::Polynomial { points } => points[1] = point,
            PcurveNurbsPoles::Rational { points } => points[1].point = point,
        }
        let before = geometry.clone();
        assert_eq!(geometry.try_scale_coordinates([2.0, 3.0]),
            Err(PcurveCoordinateScaleError::Invalid("control_points contains a non-finite point".into())));
        assert_eq!(geometry, before);
    }
}

#[test]
fn owned_pcurve_scaling_preserves_original_fuse_before_geometry_refusal() {
    for geometry in [PcurveGeometry::Line(LinePcurve::U_AXIS), nurbs(f64::MAX, false), nurbs(f64::MAX, true)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_recursion_depth = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = ctx.charge_work(1, "original scaling fuse").unwrap_err();
        let error = geometry.scaled_coordinates_owned(&ctx, [f64::MAX, f64::MAX]).unwrap_err();
        assert_eq!(error.to_string(), original.to_string());
        assert_eq!(ctx.finish_session().unwrap_err().to_string(), original.to_string());
    }
}

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
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(x, 1.0); 2],
            rational.then(|| vec![1.0, 2.0]),
            false,
        )
        .expect("fixture pcurve construction admission")
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
            let expected = expected
                .try_scale_coordinates(scales)
                .map(|()| expected)
                .map_err(|error| error.to_string());
            let actual = with_limits(u64::MAX, u64::MAX, |ctx| {
                geometry.clone().scaled_coordinates_owned(ctx, scales)
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
            let expected = expected
                .try_scale_coordinates(scales)
                .map(|()| expected)
                .map_err(|error| error.to_string());
            let actual = with_limits(u64::MAX, u64::MAX, |ctx| {
                geometry.clone().scaled_coordinates_owned(ctx, scales)
            })
            .expect("admitted");
            assert_eq!(actual.map_err(str::to_owned), expected);
        }
    }
}
