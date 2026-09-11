use crate::decode::analytic::edges::nonperiodic_nurbs_endpoint_points;
use crate::decode::analytic::pcurve_geometry::{
    meridian_circle_pcurve, ruled_generator_line_pcurve, surface_of_revolution_parallel_pcurve,
};
use crate::decode::analytic::pcurves::{
    directed_pcurve_points, linear_pcurve_carrier, mapped_pcurve_endpoints,
    oriented_native_pcurve_endpoints, planar_curve_pcurve, solve_pcurve_vertex_domains,
    solve_pcurve_vertex_domains_with_authoritative_points, unique_oriented_native_pcurve,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, PcurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::BTreeMap;

const EPS_LATITUDE_RADIUS: f64 = 1.0e-12;

#[test]
fn reconciles_pcurve_endpoints_across_evaluable_face_charts() {
    let mut ir = CadIr::empty();
    for (id, normal, u_axis) in [
        (1, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        (2, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(normal[0], normal[1], normal[2]),
                    Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }
    assert_eq!(
        mapped_pcurve_endpoints(
            &ir,
            [1, 2],
            [[[1.0, 2.0], [3.0, 4.0]], [[2.0, 1.0], [4.0, 3.0]]],
        ),
        Some([[1.0, 2.0, 0.0], [3.0, 4.0, 0.0]])
    );
    assert!(mapped_pcurve_endpoints(
        &ir,
        [1, 2],
        [[[1.0, 2.0], [3.0, 4.0]], [[2.0, 1.0], [5.0, 3.0]]],
    )
    .is_none());
}

#[test]
fn maps_linear_pcurves_to_exact_analytic_carriers() {
    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            0.5,
            0.25,
        )
        .expect("valid ConeSurface fixture"),
    ));
    let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid SphereSurface fixture"),
    ));

    assert!(matches!(
        linear_pcurve_carrier(&plane, [[1.0, 2.0], [3.0, 4.0]]),
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(_)))
    ));
    assert!(matches!(
        linear_pcurve_carrier(&cylinder, [[1.0, 2.0], [1.0, 4.0]]),
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(_)))
    ));
    assert!(
        matches!(linear_pcurve_carrier(&cylinder, [[1.0, 2.0], [2.0, 2.0]]), Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)))
        if {
            let radius = circle_curve.radius();
            radius == 2.0
        })
    );
    assert!(matches!(
        linear_pcurve_carrier(&cone, [[1.0, 2.0], [1.0, 4.0]]),
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(_)))
    ));
    assert!(
        matches!(linear_pcurve_carrier(&cone, [[1.0, 2.0], [2.0, 2.0]]), Some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)))
                if {
                    let major_radius = ellipse_curve.major_radius();
        let minor_radius = ellipse_curve.minor_radius();
                    major_radius > minor_radius
                })
    );
    assert!(linear_pcurve_carrier(&cylinder, [[1.0, 2.0], [2.0, 4.0]]).is_none());
    assert!(
        matches!(linear_pcurve_carrier(&sphere, [[1.0, 2.0], [1.0, 4.0]]), Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)))
        if {
            let radius = circle_curve.radius();
            radius == 2.0
        })
    );
    assert!(
        matches!(linear_pcurve_carrier(&sphere, [[1.0, 0.25], [2.0, 0.25]]), Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)))
        if {
            let radius = circle_curve.radius();
            (radius - 2.0 * 0.25_f64.cos()).abs() <= EPS_LATITUDE_RADIUS
        })
    );

    let torus = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            1.0,
        )
        .expect("valid TorusSurface fixture"),
    ));
    assert!(
        matches!(linear_pcurve_carrier(&torus, [[0.5, 0.0], [0.5, 1.0]]), Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)))
        if {
            let radius = circle_curve.radius();
            radius == 1.0
        })
    );
    assert!(
        matches!(linear_pcurve_carrier(&torus, [[0.5, 0.0], [1.0, 0.0]]), Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)))
        if {
            let radius = circle_curve.radius();
            radius == 4.0
        })
    );
}

#[test]
fn propagates_unique_pcurve_endpoints_through_a_vertex_component() {
    let a = [1.0, 0.0, 0.0];
    let b = [2.0, 0.0, 0.0];
    let c = [3.0, 0.0, 0.0];
    let constraints = [([1, 2], [a, b]), ([2, 3], [c, b])];
    assert_eq!(
        solve_pcurve_vertex_domains(
            &constraints,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        ),
        BTreeMap::from([(1, a), (2, b), (3, c)])
    );
    assert!(solve_pcurve_vertex_domains(
        &constraints[..1],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .is_empty());
    assert!(solve_pcurve_vertex_domains(
        &constraints,
        &BTreeMap::from([(2, [9.0, 0.0, 0.0])]),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .is_empty());

    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::LineCurve::try_new(
            Point3::new(a[0], a[1], a[2]),
            Vector3::new(0.0, 1.0, 0.0),
        )
        .expect("valid LineCurve fixture"),
    ));
    assert_eq!(
        solve_pcurve_vertex_domains(
            &constraints[..1],
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::from([(1, vec![&line])]),
        ),
        BTreeMap::from([(1, a), (2, b)])
    );

    let analytic_domains = BTreeMap::from([(1, vec![a, c])]);
    assert!(solve_pcurve_vertex_domains(
        &[],
        &BTreeMap::new(),
        &analytic_domains,
        &BTreeMap::new(),
    )
    .is_empty());
    assert_eq!(
        solve_pcurve_vertex_domains(
            &constraints[..1],
            &BTreeMap::new(),
            &analytic_domains,
            &BTreeMap::new(),
        ),
        BTreeMap::from([(1, a), (2, b)])
    );
}

#[test]
fn authoritative_native_endpoint_survives_conflicting_inferred_domain() {
    let witness = [1.0, 0.0, 0.0];
    let adjacent = [2.0, 0.0, 0.0];
    let inferred = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::LineCurve::try_new(
            Point3::new(9.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid LineCurve fixture"),
    ));
    let constraints = [([1, 2], [witness, adjacent])];
    let analytic_domains = BTreeMap::from([(1, vec![[9.0, 0.0, 0.0]])]);
    let incident_curves = BTreeMap::from([(1, vec![&inferred])]);

    assert!(solve_pcurve_vertex_domains(
        &constraints,
        &BTreeMap::new(),
        &analytic_domains,
        &incident_curves,
    )
    .is_empty());
    assert_eq!(
        solve_pcurve_vertex_domains_with_authoritative_points(
            &constraints,
            &BTreeMap::from([(1, witness), (2, adjacent)]),
            &analytic_domains,
            &incident_curves,
            &BTreeMap::from([(1, witness)]),
        ),
        BTreeMap::from([(1, witness), (2, adjacent)])
    );
}

#[test]
fn boundary_nurbs_endpoint_witnesses_use_the_intrinsic_domain() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
            ],
            Some(vec![1.0, 2.0, 1.0]),
            false,
        )
        .expect("valid boundary NURBS"),
    ));
    assert_eq!(
        nonperiodic_nurbs_endpoint_points(&geometry),
        Some([[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]])
    );

    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(mut periodic)) = geometry else {
        unreachable!("test geometry is NURBS");
    };
    periodic.set_periodic(true);
    assert!(
        nonperiodic_nurbs_endpoint_points(&CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            periodic
        )))
        .is_none()
    );
}

#[test]
fn boundary_nurbs_endpoints_propagate_as_unordered_edge_constraints() {
    let first = [0.0, 0.0, 0.0];
    let middle = [1.0, 0.0, 0.0];
    let last = [2.0, 0.0, 0.0];
    let constraints = [([1, 2], [first, middle]), ([2, 3], [last, middle])];
    assert_eq!(
        solve_pcurve_vertex_domains(
            &constraints,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        ),
        BTreeMap::from([(1, first), (2, middle), (3, last)])
    );
}

#[test]
fn pcurve_direction_flags_assign_endpoint_order() {
    let points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    assert_eq!(directed_pcurve_points([0x01, 0xf6], points), Some(points));
    assert_eq!(
        directed_pcurve_points([0xf6, 0x01], points),
        Some([points[1], points[0]])
    );
    assert_eq!(directed_pcurve_points([0x01, 0x01], points), None);
}

fn plane() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ))
}

fn assert_pcurve_matches_curve(
    surface: &SurfaceGeometry,
    curve: &CurveGeometry,
    pcurve: &PcurveGeometry,
    parameters: &[f64],
) {
    for parameter in parameters {
        let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, *parameter).expect("pcurve point");
        let mapped = cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v).expect("surface point");
        let expected = cadmpeg_ir::eval::curve_point(curve, *parameter).expect("curve point");
        assert!((mapped.x - expected.x).abs() <= 1.0e-10);
        assert!((mapped.y - expected.y).abs() <= 1.0e-10);
        assert!((mapped.z - expected.z).abs() <= 1.0e-10);
    }
}

#[test]
fn orients_uv_endpoints_by_the_coedge_traversal() {
    let endpoints = [[2.0, 4.0], [5.0, 7.0]];
    assert_eq!(
        oriented_native_pcurve_endpoints(&plane(), endpoints, [[5.0, 7.0, 3.0], [2.0, 4.0, 3.0]],),
        Some([endpoints[1], endpoints[0]])
    );
}

#[test]
fn withholds_uv_endpoints_that_do_not_map_to_the_edge() {
    assert_eq!(
        oriented_native_pcurve_endpoints(
            &plane(),
            [[2.0, 4.0], [5.0, 7.0]],
            [[2.0, 4.0, 3.0], [9.0, 7.0, 3.0]],
        ),
        None
    );
}

#[test]
fn reconciles_agreeing_source_forms_and_rejects_competing_paths() {
    let traversal = [[2.0, 4.0, 3.0], [5.0, 7.0, 3.0]];
    let endpoints = [[2.0, 4.0], [5.0, 7.0]];
    assert_eq!(
        unique_oriented_native_pcurve(
            &plane(),
            &[(endpoints, 20), ([endpoints[1], endpoints[0]], 10)],
            traversal,
        ),
        Some((endpoints, 10))
    );
    assert_eq!(
        unique_oriented_native_pcurve(
            &plane(),
            &[(endpoints, 20), ([[2.0, 4.0], [5.0, 8.0]], 10)],
            traversal,
        ),
        Some((endpoints, 20))
    );

    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    assert_eq!(
        unique_oriented_native_pcurve(
            &cylinder,
            &[
                ([[0.0, 0.0], [std::f64::consts::FRAC_PI_2, 0.0]], 10),
                (
                    [
                        [std::f64::consts::TAU, 0.0],
                        [std::f64::consts::TAU + std::f64::consts::FRAC_PI_2, 0.0],
                    ],
                    20,
                ),
            ],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        ),
        None
    );
}

#[test]
fn projects_exact_planar_carriers_without_changing_parameters() {
    let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(2.0, 4.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
            2.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(
        matches!(planar_curve_pcurve(&plane(), &circle), Some(PcurveGeometry::Circle(circle_pcurve))
                if {
                    let center = circle_pcurve.center();
        let x_axis = circle_pcurve.x_axis();
        let y_axis = circle_pcurve.y_axis();
        let radius = circle_pcurve.radius();
                    *center == Point2::new(2.0, 4.0)
                        && *x_axis == Point2::new(0.0, 1.0)
                        && *y_axis == Point2::new(-1.0, 0.0)
                        && radius == 2.0
                })
    );

    let nurbs = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::new(
            1,
            vec![2.0, 2.0, 5.0, 5.0],
            vec![Point3::new(2.0, 4.0, 3.0), Point3::new(5.0, 7.0, 3.0)],
            Some(vec![2.0, 1.0]),
            false,
        )
        .expect("valid planar NURBS"),
    ));
    assert!(matches!(
        planar_curve_pcurve(&plane(), &nurbs),
        Some(PcurveGeometry::Nurbs { nurbs })
            if nurbs.degree() == 1
                && nurbs.knots() == [2.0, 2.0, 5.0, 5.0]
                && nurbs.control_points()
                    == [Point2::new(2.0, 4.0), Point2::new(5.0, 7.0)]
                && nurbs.weights() == Some(&[2.0, 1.0][..])
                && !nurbs.periodic()
    ));

    let off_plane = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::LineCurve::try_new(
            Point3::new(0.0, 0.0, 3.1),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid LineCurve fixture"),
    ));
    assert!(planar_curve_pcurve(&plane(), &off_plane).is_none());
}

#[test]
fn projects_a_coaxial_cylinder_circle_with_its_native_angle() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::CylinderSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 8.0),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(0.0, 1.0, 0.0),
            2.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    let pcurve = surface_of_revolution_parallel_pcurve(&surface, &circle).expect("cylinder pcurve");
    let PcurveGeometry::Line(line_pcurve) = &pcurve else {
        panic!("cylinder-circle pcurve: {pcurve:#?}");
    };
    let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
    assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1.0e-12);
    assert!((origin.v - 5.0).abs() <= 1.0e-12);
    assert_eq!(*direction, Point2::new(-1.0, 0.0));
    assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-2.0, 0.0, 1.25, 4.0]);

    let off_axis = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(1.1, 2.0, 8.0),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(0.0, 1.0, 0.0),
            2.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(surface_of_revolution_parallel_pcurve(&surface, &off_axis).is_none());
}

#[test]
fn projects_cone_parallel_conics_on_either_side_of_the_apex() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("valid ConeSurface fixture"),
    ));
    for (height, radius, expected_phase) in [(3.0, 5.0, 0.0), (-3.0, 1.0, std::f64::consts::PI)] {
        let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::CircleCurve::try_new(
                Point3::new(0.0, 0.0, height),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .expect("valid CircleCurve fixture"),
        ));
        let pcurve =
            surface_of_revolution_parallel_pcurve(&surface, &circle).expect("cone section pcurve");
        let PcurveGeometry::Line(line_pcurve) = &pcurve else {
            panic!("cone-circle pcurve: {pcurve:#?}");
        };
        let origin = line_pcurve.origin();
        let direction = line_pcurve.direction();
        assert!((origin.u - expected_phase).sin().abs() <= 1.0e-12);
        assert!(((origin.u - expected_phase).cos() - 1.0).abs() <= 1.0e-12);
        assert!((origin.v - height).abs() <= 1.0e-12);
        assert_eq!(*direction, Point2::new(1.0, 0.0));
        assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-1.0, 0.0, 2.0]);
    }

    let elliptical = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            0.5,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("valid ConeSurface fixture"),
    ));
    let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(surface_of_revolution_parallel_pcurve(&elliptical, &circle).is_none());
    for (height, major_radius, minor_radius, expected_phase) in
        [(3.0, 5.0, 2.5, 0.0), (-3.0, 1.0, 0.5, std::f64::consts::PI)]
    {
        let ellipse = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
            cadmpeg_ir::geometry::EllipseCurve::try_new(
                Point3::new(0.0, 0.0, height),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                major_radius,
                minor_radius,
            )
            .expect("valid EllipseCurve fixture"),
        ));
        let pcurve = surface_of_revolution_parallel_pcurve(&elliptical, &ellipse)
            .expect("elliptical cone parallel pcurve");
        let PcurveGeometry::Line(line_pcurve) = &pcurve else {
            panic!("cone-ellipse pcurve: {pcurve:#?}");
        };
        let origin = line_pcurve.origin();
        let direction = line_pcurve.direction();
        assert!((origin.u - expected_phase).sin().abs() <= 1.0e-12);
        assert!(((origin.u - expected_phase).cos() - 1.0).abs() <= 1.0e-12);
        assert!((origin.v - height).abs() <= 1.0e-12);
        assert_eq!(*direction, Point2::new(1.0, 0.0));
        assert_pcurve_matches_curve(&elliptical, &ellipse, &pcurve, &[-1.0, 0.0, 2.0]);
    }
}

#[test]
fn projects_sphere_latitude_circles_to_the_canonical_polar_chart() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::SphereSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
        )
        .expect("valid SphereSurface fixture"),
    ));
    for axial in [-3.0, 3.0] {
        let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::CircleCurve::try_new(
                Point3::new(1.0, 2.0, 3.0 + axial),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 1.0, 0.0),
                4.0,
            )
            .expect("valid CircleCurve fixture"),
        ));
        let pcurve = surface_of_revolution_parallel_pcurve(&surface, &circle)
            .expect("sphere latitude pcurve");
        let PcurveGeometry::Line(line_pcurve) = &pcurve else {
            panic!("sphere-circle pcurve: {pcurve:#?}");
        };
        let origin = line_pcurve.origin();
        let direction = line_pcurve.direction();
        assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1.0e-12);
        assert!((origin.v - axial.atan2(4.0)).abs() <= 1.0e-12);
        assert_eq!(*direction, Point2::new(1.0, 0.0));
        assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-1.0, 0.0, 2.0]);
    }

    let invalid_circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 6.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.1,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(surface_of_revolution_parallel_pcurve(&surface, &invalid_circle).is_none());
}

#[test]
fn projects_torus_parallel_circles_with_signed_ring_branches() {
    for (major_radius, minor_radius, polar, circle_radius, expected_phase) in [
        (4.0, 1.0, std::f64::consts::FRAC_PI_2, 4.0, 0.0),
        (1.0, 2.0, std::f64::consts::PI, 1.0, std::f64::consts::PI),
    ] {
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            cadmpeg_ir::geometry::TorusSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                major_radius,
                minor_radius,
            )
            .expect("valid TorusSurface fixture"),
        ));
        let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::CircleCurve::try_new(
                Point3::new(0.0, 0.0, minor_radius * polar.sin()),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                circle_radius,
            )
            .expect("valid CircleCurve fixture"),
        ));
        let pcurve = surface_of_revolution_parallel_pcurve(&surface, &circle)
            .expect("torus parallel pcurve");
        let PcurveGeometry::Line(line_pcurve) = &pcurve else {
            panic!("torus-circle pcurve: {pcurve:#?}");
        };
        let origin = line_pcurve.origin();
        let direction = line_pcurve.direction();
        assert!((origin.u - expected_phase).sin().abs() <= 1.0e-12);
        assert!(((origin.u - expected_phase).cos() - 1.0).abs() <= 1.0e-12);
        assert!((origin.v - polar).sin().abs() <= 1.0e-12);
        assert!(((origin.v - polar).cos() - 1.0).abs() <= 1.0e-12);
        assert_eq!(*direction, Point2::new(1.0, 0.0));
        assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-1.0, 0.0, 2.0]);
    }
}

#[test]
fn projects_torus_meridian_circles_with_native_angle_phase() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::TorusSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
            1.5,
        )
        .expect("valid TorusSurface fixture"),
    ));
    let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(1.0, 6.0, 3.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            1.5,
        )
        .expect("valid CircleCurve fixture"),
    ));
    let pcurve = meridian_circle_pcurve(&surface, &circle).expect("meridian pcurve");
    let PcurveGeometry::Line(line_pcurve) = &pcurve else {
        panic!("torus-meridian pcurve: {pcurve:#?}");
    };
    let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
    assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1.0e-12);
    assert!((origin.v - std::f64::consts::FRAC_PI_2).abs() <= 1.0e-12);
    assert_eq!(*direction, Point2::new(0.0, 1.0));
    assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-1.0, 0.0, 2.0]);

    let displaced = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(1.1, 6.0, 3.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            1.5,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(meridian_circle_pcurve(&surface, &displaced).is_none());
}

#[test]
fn projects_sphere_meridians_through_both_poles() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::SphereSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
        )
        .expect("valid SphereSurface fixture"),
    ));
    let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            5.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    let pcurve = meridian_circle_pcurve(&surface, &circle).expect("sphere meridian pcurve");
    let PcurveGeometry::Line(line_pcurve) = &pcurve else {
        panic!("sphere-meridian pcurve: {pcurve:#?}");
    };
    let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
    assert!(origin.u.abs() <= 1.0e-12);
    assert!((origin.v - std::f64::consts::FRAC_PI_2).abs() <= 1.0e-12);
    assert_eq!(*direction, Point2::new(0.0, -1.0));
    assert_pcurve_matches_curve(
        &surface,
        &circle,
        &pcurve,
        &[
            -std::f64::consts::PI,
            -std::f64::consts::FRAC_PI_2,
            0.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
        ],
    );

    let small_circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(meridian_circle_pcurve(&surface, &small_circle).is_none());
}

#[test]
fn projects_cylinder_and_cone_generators_with_native_line_parameters() {
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::CylinderSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let cylinder_line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::LineCurve::try_new(
            Point3::new(1.0, 4.0, 8.0),
            Vector3::new(0.0, 0.0, -2.0)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    ));
    let pcurve =
        ruled_generator_line_pcurve(&cylinder, &cylinder_line).expect("cylinder generator pcurve");
    let PcurveGeometry::Line(line_pcurve) = &pcurve else {
        panic!("cylinder-generator pcurve: {pcurve:#?}");
    };
    let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
    assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1.0e-12);
    assert!((origin.v - 5.0).abs() <= 1.0e-12);
    assert_eq!(*direction, Point2::new(0.0, -1.0));
    assert_pcurve_matches_curve(&cylinder, &cylinder_line, &pcurve, &[-1.0, 0.0, 2.0]);
    let tiny_skew = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::LineCurve::try_new(
            Point3::new(1.0, 4.0, 8.0),
            Vector3::new(1e-13, 0.0, 1e-13)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    ));
    assert!(ruled_generator_line_pcurve(&cylinder, &tiny_skew).is_none());

    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("valid ConeSurface fixture"),
    ));
    let cone_line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::LineCurve::try_new(
            Point3::new(0.0, 5.0, 3.0),
            Vector3::new(0.0, 2.0, 2.0)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    ));
    let pcurve = ruled_generator_line_pcurve(&cone, &cone_line).expect("cone generator pcurve");
    let PcurveGeometry::Line(line_pcurve) = &pcurve else {
        panic!("cone-generator pcurve: {pcurve:#?}");
    };
    let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
    assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1.0e-12);
    assert!((origin.v - 3.0).abs() <= 1.0e-12);
    assert!(direction.u.abs() <= 1.0e-12);
    assert!((direction.v - std::f64::consts::FRAC_1_SQRT_2).abs() <= 1.0e-12);
    assert_pcurve_matches_curve(&cone, &cone_line, &pcurve, &[-1.0, 0.0, 2.0]);

    let elliptical_cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            0.5,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("valid ConeSurface fixture"),
    ));
    let root_half = std::f64::consts::FRAC_1_SQRT_2;
    let elliptical_generator = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::LineCurve::try_new(
            Point3::new(5.0 * root_half, 2.5 * root_half, 3.0),
            Vector3::new(2.0 * root_half, root_half, 2.0)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    ));
    let pcurve = ruled_generator_line_pcurve(&elliptical_cone, &elliptical_generator)
        .expect("elliptical cone generator pcurve");
    let PcurveGeometry::Line(line_pcurve) = &pcurve else {
        panic!("elliptical-cone generator pcurve: {pcurve:#?}");
    };
    let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
    assert!((origin.u - std::f64::consts::FRAC_PI_4).abs() <= 1.0e-12);
    assert!((origin.v - 3.0).abs() <= 1.0e-12);
    assert!(direction.u.abs() <= 1.0e-12);
    assert!((direction.v - 2.0 / 6.5_f64.sqrt()).abs() <= 1.0e-12);
    assert_pcurve_matches_curve(
        &elliptical_cone,
        &elliptical_generator,
        &pcurve,
        &[-3.0, 0.0, 2.0],
    );

    let skew = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::LineCurve::try_new(
            Point3::new(0.0, 5.0, 3.0),
            Vector3::new(0.1, 2.0, 2.0)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    ));
    assert!(ruled_generator_line_pcurve(&cone, &skew).is_none());
}
