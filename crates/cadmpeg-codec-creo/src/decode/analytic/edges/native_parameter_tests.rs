use crate::decode::analytic::edges::{
    exact_line_edge_parameter_range, full_periodic_conic_edge_parameter_range,
    full_periodic_nurbs_edge_parameter_range, nonperiodic_conic_edge_parameter_range,
    nonperiodic_conic_parameter, nonperiodic_nurbs_edge_parameter_range,
    orient_nonperiodic_nurbs_edge_carrier, periodic_conic_edge_parameter_range,
};
use crate::decode::analytic::pcurves::{
    native_pcurve_midpoint, pcurve_backed_periodic_conic_parameter_range, NativePcurveCandidates,
};
use crate::decode::surfaces::intersection_resolve::{
    curve_contains_points, select_unique_curve_candidate,
};
use crate::decode::surfaces::transfer_curves::analytic_curve_branches;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

fn circle() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CircleCurve fixture"),
    ))
}

fn ellipse() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
            2.0,
        )
        .expect("valid EllipseCurve fixture"),
    ))
}

fn evaluated(geometry: &CurveGeometry, parameter: f64) -> [f64; 3] {
    let point = cadmpeg_ir::eval::decode::curve_point(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        geometry,
        parameter,
    )
    .expect("conic point");
    [point.x, point.y, point.z]
}

fn nurbs_curve(
    degree: u32,
    knots: Vec<f64>,
    control_points: Vec<Point3>,
    weights: Option<Vec<f64>>,
    periodic: bool,
) -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            degree,
            knots,
            control_points,
            weights,
            periodic,
        )
        .expect("fixture constructor admission")
        .expect("cardinality-valid test curve"),
    ))
}

#[test]
fn preserves_unit_line_parameterization_and_orders_the_interval() {
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(2.0, 0.0, 0.0)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    ));
    assert_eq!(
        exact_line_edge_parameter_range(&line, [[7.0, 2.0, 3.0], [-3.0, 2.0, 3.0]]),
        Some([-4.0, 6.0])
    );
}

#[test]
fn withholds_parameters_for_points_off_the_line() {
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(2.0, 0.0, 0.0)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    ));
    assert_eq!(
        exact_line_edge_parameter_range(&line, [[7.0, 2.0, 3.0], [-3.0, 2.1, 3.0]]),
        None
    );
}

#[test]
fn full_nonperiodic_nurbs_recovers_its_intrinsic_domain() {
    let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
    let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
    let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &evaluation_arena,
        &evaluation_policy,
    )
    .expect("evaluation root");

    let nurbs = nurbs_curve(
        2,
        vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
        vec![
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(4.0, 7.0, 3.0),
            Point3::new(9.0, 8.0, 3.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &evaluation_ctx,
            &nurbs,
            [[9.0, 8.0, 3.0], [1.0, 2.0, 3.0]],
        )
        .expect("evaluation resources"),
        Some([2.0, 5.0])
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &evaluation_ctx,
            &nurbs,
            [[9.0, 8.0, 3.0], [1.0, 2.1, 3.0]],
        )
        .expect("evaluation resources"),
        None
    );
}

#[test]
fn orients_reversed_nonperiodic_nurbs_edges_with_increasing_ranges() {
    let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
    let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
    let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &evaluation_arena,
        &evaluation_policy,
    )
    .expect("evaluation root");

    let mut nurbs = nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)],
        Some(vec![2.0, 3.0]),
        false,
    );

    assert_eq!(
        orient_nonperiodic_nurbs_edge_carrier(
            &evaluation_ctx,
            &mut nurbs,
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        )
        .expect("evaluation resources"),
        Some([0.0, 1.0])
    );
    assert_eq!(evaluated(&nurbs, 0.0), [0.0, 0.0, 0.0]);
    assert_eq!(evaluated(&nurbs, 1.0), [1.0, 0.0, 0.0]);
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = nurbs else {
        panic!("NURBS carrier");
    };
    assert_eq!(nurbs.pole_rows().weights(), Some(vec![3.0, 2.0]));
}

#[test]
fn degree_one_nurbs_recovers_unique_bounded_parameters() {
    let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
    let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
    let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &evaluation_arena,
        &evaluation_policy,
    )
    .expect("evaluation root");

    let nurbs = nurbs_curve(
        1,
        vec![2.0, 2.0, 5.0, 9.0, 9.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(3.0, 4.0, 0.0),
        ],
        Some(vec![2.0, 1.0, 3.0]),
        false,
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &evaluation_ctx,
            &nurbs,
            [[3.0, 3.0, 0.0], [1.0, 0.0, 0.0]],
        )
        .expect("evaluation resources"),
        Some([3.5, 7.0])
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &evaluation_ctx,
            &nurbs,
            [[3.0, 3.0, 0.0], [1.0, 0.1, 0.0]],
        )
        .expect("evaluation resources"),
        None
    );

    let translated = nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            Point3::new(100_000_000.0, 0.0, 0.0),
            Point3::new(100_000_004.0, 0.0, 0.0),
        ],
        None,
        false,
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &evaluation_ctx,
            &translated,
            [[100_000_001.0, 0.01, 0.0], [100_000_003.0, 0.0, 0.0]],
        )
        .expect("evaluation resources"),
        None
    );

    let self_intersecting = nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 2.0, 2.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        None,
        false,
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &evaluation_ctx,
            &self_intersecting,
            [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        )
        .expect("evaluation resources"),
        None
    );

    let constant_span = nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        None,
        false,
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &evaluation_ctx,
            &constant_span,
            [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        )
        .expect("evaluation resources"),
        None
    );
}

#[test]
fn periodic_nurbs_does_not_imply_a_full_edge_trim() {
    let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
    let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
    let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &evaluation_arena,
        &evaluation_policy,
    )
    .expect("evaluation root");

    let nurbs = nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        true,
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &evaluation_ctx,
            &nurbs,
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        )
        .expect("evaluation resources"),
        None
    );

    let closed = nurbs_curve(
        1,
        vec![2.0, 2.0, 4.0, 7.0, 9.0, 9.0],
        vec![
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        true,
    );
    assert_eq!(
        full_periodic_nurbs_edge_parameter_range(&evaluation_ctx, &closed, [1.0, 0.0, 0.0])
            .expect("evaluation resources"),
        Some([2.0, 9.0])
    );
    assert_eq!(
        full_periodic_nurbs_edge_parameter_range(&evaluation_ctx, &closed, [0.0, 1.0, 0.0])
            .expect("evaluation resources"),
        None
    );
}

#[test]
fn pcurve_midpoint_selects_minor_major_and_full_circle_intervals() {
    let circle = circle();
    let points = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let root_two = std::f64::consts::SQRT_2;
    assert_eq!(
        ({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            periodic_conic_edge_parameter_range(&ctx, &circle, points, [root_two, root_two, 0.0])
        })
        .expect("evaluation resources"),
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );
    assert_eq!(
        ({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            periodic_conic_edge_parameter_range(&ctx, &circle, points, [-root_two, -root_two, 0.0])
        })
        .expect("evaluation resources"),
        Some([std::f64::consts::FRAC_PI_2, std::f64::consts::TAU])
    );
    assert_eq!(
        ({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            periodic_conic_edge_parameter_range(
                &ctx,
                &circle,
                [points[0], points[0]],
                [-2.0, 0.0, 0.0],
            )
        })
        .expect("evaluation resources"),
        Some([0.0, std::f64::consts::TAU])
    );
    assert_eq!(
        ({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            periodic_conic_edge_parameter_range(&ctx, &circle, [points[0], points[0]], points[0])
        })
        .expect("evaluation resources"),
        None
    );
}

#[test]
fn conic_parameters_preserve_ellipse_axis_scales() {
    let ellipse = ellipse();
    let points = [[4.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let root_two = std::f64::consts::SQRT_2;
    assert!(curve_contains_points(&ellipse, points));
    assert!(!curve_contains_points(
        &ellipse,
        [points[0], [0.0, 4.0, 0.0]],
    ));
    assert_eq!(
        ({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            periodic_conic_edge_parameter_range(
                &ctx,
                &ellipse,
                points,
                [2.0 * root_two, root_two, 0.0],
            )
        })
        .expect("evaluation resources"),
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );
    assert_eq!(
        ({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            periodic_conic_edge_parameter_range(
                &ctx,
                &ellipse,
                points,
                [-2.0 * root_two, -root_two, 0.0],
            )
        })
        .expect("evaluation resources"),
        Some([std::f64::consts::FRAC_PI_2, std::f64::consts::TAU])
    );
    assert_eq!(
        ({
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("root");
            periodic_conic_edge_parameter_range(
                &ctx,
                &ellipse,
                [points[0], points[0]],
                [-4.0, 0.0, 0.0],
            )
        })
        .expect("evaluation resources"),
        Some([0.0, std::f64::consts::TAU])
    );
}

#[test]
fn closed_periodic_conic_uses_one_full_period_from_its_seam() {
    let circle = circle();
    let range = full_periodic_conic_edge_parameter_range(&circle, [0.0, 2.0, 0.0])
        .expect("full circle range");
    assert!((range[0] - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert!((range[1] - (std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU)).abs() < 1.0e-12);

    let ellipse = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
            2.0,
        )
        .expect("valid EllipseCurve fixture"),
    ));
    assert_eq!(
        full_periodic_conic_edge_parameter_range(&ellipse, [4.0, 0.0, 0.0]),
        Some([0.0, std::f64::consts::TAU])
    );
    assert_eq!(
        full_periodic_conic_edge_parameter_range(&ellipse, [4.0, 0.1, 0.0]),
        None
    );
}

#[test]
fn nonperiodic_conics_recover_their_native_parameters() {
    let parabola = CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
        cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid ParabolaCurve fixture"),
    ));
    let parabola_points = [evaluated(&parabola, 3.0), evaluated(&parabola, -2.0)];
    assert_eq!(
        nonperiodic_conic_edge_parameter_range(&parabola, parabola_points),
        Some([-2.0, 3.0])
    );
    assert_eq!(
        nonperiodic_conic_parameter(&parabola, [1.0, 6.0, 3.0]),
        None
    );

    let hyperbola = CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
        cadmpeg_ir::geometry::analytic::HyperbolaCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            2.0,
        )
        .expect("valid HyperbolaCurve fixture"),
    ));
    let hyperbola_points = [evaluated(&hyperbola, 2.0), evaluated(&hyperbola, -1.0)];
    let range = nonperiodic_conic_edge_parameter_range(&hyperbola, hyperbola_points)
        .expect("hyperbola range");
    assert!((range[0] + 1.0).abs() <= 1.0e-12);
    assert!((range[1] - 2.0).abs() <= 1.0e-12);
    assert_eq!(
        nonperiodic_conic_parameter(&hyperbola, [-2.0, 2.0, 3.0]),
        None
    );
}

#[test]
fn solved_endpoints_select_one_hyperbola_branch() {
    let hyperbola = CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
        cadmpeg_ir::geometry::analytic::HyperbolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            2.0,
        )
        .expect("valid HyperbolaCurve fixture"),
    ));
    let branches = crate::decode::with_test_decode_ctx(|ctx| {
        analytic_curve_branches(ctx, &hyperbola, "hyperbola")
    })
    .expect("analytic branches admitted");
    let points = [
        evaluated(&branches[1].0, -1.0),
        evaluated(&branches[1].0, 2.0),
    ];
    let selected = select_unique_curve_candidate(branches, points).expect("one branch");
    let CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) = selected.0 else {
        panic!("hyperbola branch");
    };
    let major_direction = *hyperbola_curve.frame().reference().as_raw();
    assert_eq!(major_direction, Vector3::new(-1.0, 0.0, 0.0));
}

#[test]
fn surface_pcurve_midpoint_retains_periodic_path() {
    let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
    let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
    let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &evaluation_arena,
        &evaluation_policy,
    )
    .expect("evaluation root");

    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let midpoint = native_pcurve_midpoint(
        &evaluation_ctx,
        &cylinder,
        [[0.0, 0.0], [-3.0 * std::f64::consts::FRAC_PI_2, 0.0]],
        [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
    )
    .expect("evaluation resources")
    .expect("periodic midpoint");
    assert!((midpoint[0] + std::f64::consts::SQRT_2).abs() <= 1.0e-12);
    assert!((midpoint[1] + std::f64::consts::SQRT_2).abs() <= 1.0e-12);
}

#[test]
fn adjacent_face_pcurves_must_select_the_same_circle_arc() {
    let evaluation_arena = cadmpeg_core::decode::DecodeArena::new();
    let evaluation_policy = cadmpeg_core::decode::DecodePolicy::service();
    let (evaluation_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &evaluation_arena,
        &evaluation_policy,
    )
    .expect("evaluation root");

    let surface_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let surfaces = [10, 11]
        .map(|face| Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{face}"))
                .expect("identity grammar"),
            geometry: surface_geometry.clone(),
            source_object: None,
        })
        .to_vec();
    let points = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let mut candidates = NativePcurveCandidates::new();
    candidates.insert(
        (7, 10),
        vec![([[0.0, 0.0], [std::f64::consts::FRAC_PI_2, 0.0]], 10)],
    );
    candidates.insert(
        (7, 11),
        vec![([[0.0, 0.0], [std::f64::consts::FRAC_PI_2, 0.0]], 20)],
    );
    assert_eq!(
        pcurve_backed_periodic_conic_parameter_range(
            &evaluation_ctx,
            &circle(),
            (7, [10, 11]),
            &candidates,
            &surfaces,
            points,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
        .expect("evaluation resources"),
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );

    candidates.insert(
        (7, 11),
        vec![([[0.0, 0.0], [-3.0 * std::f64::consts::FRAC_PI_2, 0.0]], 20)],
    );
    assert_eq!(
        pcurve_backed_periodic_conic_parameter_range(
            &evaluation_ctx,
            &circle(),
            (7, [10, 11]),
            &candidates,
            &surfaces,
            points,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
        .expect("evaluation resources"),
        None
    );
}

#[test]
fn analytic_nurbs_endpoints_propagate_evaluator_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    // Degree 16 needs a heap basis; lower degrees evaluate inline.
    let mut knots = vec![0.0; 17];
    knots.extend(vec![1.0; 17]);
    let nurbs = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        16,
        knots,
        (0..17)
            .map(|pole| Point3::new(f64::from(pole) / 16.0, 0.0, 0.0))
            .collect(),
        None,
        false,
    )
    .expect("fixture constructor admission")
    .expect("degree-16 spline");
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        matches!(super::nonperiodic_nurbs_endpoint_points(&ctx, &geometry), Err(CodecError::ResourceLimit(limit)) if limit.operation == "IR B-spline basis")
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::nonperiodic_nurbs_endpoint_points(&ctx, &geometry).expect("service resources"),
        Some([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
    );
}
