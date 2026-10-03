// SPDX-License-Identifier: Apache-2.0
//! curve bindings tests.

use super::{
    build_standard_edge_curve, circle_parameter_range_from_surface_branch, curve_point,
    ensure_native_edge_support_surface, standard_analytic_curve_parameter_range,
    standard_endpoint_pair_supports_topology, standard_oriented_analytic_curve_parameter_range,
    standard_pcurve_geometry, unit_square_surface, witness_arc_end, AnnotationBuilder, CadIr,
    Curve, CurveGeometry, FinitePoint3, HashMap, PcurveGeometry, Point, Point2, Point3, PointId,
    SolvedCurveGeometry, SolvedSurfaceGeometry, StandardCurveGeometry, StandardCurveSupport,
    Surface, SurfaceGeometry, SurfaceId, Vector3,
};

#[test]
fn spherical_section_endpoint_pair_survives_topology_admission_without_pcurve() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
        )
        .expect("valid SphereSurface fixture"),
    ));
    let section_radius = 21.0_f64.sqrt();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(Point3::new(0.0, 2.0, 0.0), section_radius),
    };
    let start = Point3::new(section_radius, 2.0, 0.0);
    let end = Point3::new(0.0, 2.0, section_radius);

    assert!(standard_pcurve_geometry(
        &surface,
        &support,
        start,
        end,
        None,
        None,
        &mut crate::nurbs::LaneRefusals::new()
    )
    .is_none());
    assert!(standard_endpoint_pair_supports_topology(
        &surface,
        &support,
        start,
        end,
        None,
        &mut crate::nurbs::LaneRefusals::new()
    ));
}

#[test]
fn standard_full_circle_edge_uses_vertex_seam_and_radian_domain() {
    let mut ir = CadIr::empty();
    ir.model.points.push(Point::new(
        PointId::mint("catia:test:point#point-0").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 0.0, 0.0))
            .expect("a finite position is a point"),
        None,
    ));
    let surface_id = SurfaceId::mint("catia:test:surface#surface-0").expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 0],
        geometry: super::super::checked_circle(Point3::new(0.0, 0.0, 0.0), 2.0),
    };
    let (curve, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                ir: &mut ir,
                annotations: &mut AnnotationBuilder::new(),
                bindings: &[(surface_id.clone(), false, 0)],
                surface_indices: &HashMap::from([(surface_id, 0)]),
                brep: &[],
                support: &support,
                points: [0, 0],
                native_support: None,
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    })
    .expect("valid source object identity");
    assert_eq!(range, Some([0.0, std::f64::consts::TAU]));
    let curve = curve.expect("closed circle support identifies a curve");
    assert!(matches!(ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == curve), Some(Curve {
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
            ..
        }) if {
            let axis = circle_curve.frame().axis().as_raw();
    let ref_direction = circle_curve.frame().reference().as_raw();
    let radius = circle_curve.radius().get();
            *axis == Vector3::new(0.0, 0.0, 1.0)
                && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 2.0
        }));
}

#[test]
fn standard_plane_circle_pcurve_rejects_carrier_outside_face_plane() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let center = Point3::new(0.0, 0.0, 1.0);
    let radius = 2.0_f64.sqrt();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(center, radius),
    };
    let carrier = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            center,
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            radius,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, -1.0, 0.0),
        None,
        Some(&carrier),
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .is_none());
}

#[test]
fn standard_plane_circle_pcurve_rejects_tilted_carrier() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let center = Point3::new(0.0, 0.0, 0.0);
    let radius = 2.0;
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(center, radius),
    };
    let carrier = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            center,
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            radius,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(0.0, radius, 0.0),
        Point3::new(0.0, -radius, 0.0),
        None,
        Some(&carrier),
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .is_none());
}

#[test]
fn solved_planar_spline_line_inverts_to_exact_parameter_line() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let start = Point3::new(2.0, 4.0, 3.0);
    let end = Point3::new(5.0, 8.0, 3.0);
    let carrier = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            start,
            Vector3::new(3.0, 4.0, 0.0)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    ));
    let (geometry, range) = standard_pcurve_geometry(
        &surface,
        &support,
        start,
        end,
        None,
        Some(&carrier),
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("solved spline line pcurve");

    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(
        geometry,
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(1.0, 2.0),
                Point2::new(3.0, 4.0)
            )
            .expect("valid LinePcurve fixture")
        )
    );
}

#[test]
fn standard_pcurve_rejects_endpoints_outside_the_face_carrier() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    };
    assert!(standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 2.0, 0.0),
        None,
        None,
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .is_none());
}

#[test]
fn standard_cone_apex_uses_the_other_endpoint_angular_gauge() {
    for half_angle in [0.25f64, 1e-200] {
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                0.0,
                1.0,
                half_angle,
            )
            .expect("valid ConeSurface fixture"),
        ));
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        };
        let height = 4.0;
        let radius = height * half_angle.tan();
        let (geometry, range) = standard_pcurve_geometry(
            &surface,
            &support,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, radius, height),
            None,
            None,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("cone generator through the apex");
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(
            geometry,
            PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    cadmpeg_ir::math::Point2::new(std::f64::consts::FRAC_PI_2, 0.0),
                    cadmpeg_ir::math::Point2::new(0.0, height)
                )
                .expect("valid LinePcurve fixture")
            )
        );
    }
}

#[test]
fn standard_cone_latitude_inverts_to_isoparametric_line() {
    let half_angle = 0.25f64;
    let radius = 3.0 + 2.0 * half_angle.tan();
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            1.0,
            half_angle,
        )
        .expect("valid ConeSurface fixture"),
    ));
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(Point3::new(0.0, 0.0, 2.0), radius),
    };
    let (geometry, range) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(radius, 0.0, 2.0),
        Point3::new(0.0, radius, 2.0),
        None,
        None,
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("cone latitude pcurve");
    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(
        geometry,
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(0.0, 2.0),
                cadmpeg_ir::math::Point2::new(std::f64::consts::FRAC_PI_2, 0.0)
            )
            .expect("valid LinePcurve fixture")
        )
    );
}

#[test]
fn standard_cylinder_witness_selects_complementary_arc() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(Point3::new(0.0, 0.0, 3.0), 2.0),
    };
    let (geometry, _) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(2.0, 0.0, 3.0),
        Point3::new(0.0, 2.0, 3.0),
        Some(FinitePoint3::new(Point3::new(-2.0, 0.0, 3.0)).expect("finite witness")),
        None,
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("witnessed cylinder section");
    assert_eq!(
        geometry,
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(0.0, 3.0),
                cadmpeg_ir::math::Point2::new(-3.0 * std::f64::consts::FRAC_PI_2, 0.0,)
            )
            .expect("valid LinePcurve fixture")
        )
    );
}

#[test]
fn standard_cylinder_endpoint_witness_preserves_geometric_arc() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(Point3::new(0.0, 0.0, 3.0), 2.0),
    };
    let (geometry, _) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(-2.0, 0.0, 3.0),
        Point3::new(0.0, -2.0, 3.0),
        Some(FinitePoint3::new(Point3::new(-1.0, 0.0, 4.0)).expect("finite witness")),
        None,
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("endpoint-aligned witness does not reject the arc");
    assert_eq!(
        geometry,
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(std::f64::consts::PI, 3.0),
                cadmpeg_ir::math::Point2::new(std::f64::consts::FRAC_PI_2, 0.0)
            )
            .expect("valid LinePcurve fixture")
        )
    );
}

#[test]
fn standard_torus_witness_selects_complementary_latitude_arc() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
            2.0,
        )
        .expect("valid TorusSurface fixture"),
    ));
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(Point3::new(0.0, 0.0, 0.0), 7.0),
    };
    let (geometry, _) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(7.0, 0.0, 0.0),
        Point3::new(0.0, 7.0, 0.0),
        Some(FinitePoint3::new(Point3::new(-7.0, 0.0, 0.0)).expect("finite witness")),
        None,
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("witnessed torus latitude");
    let PcurveGeometry::Line(line_pcurve) = geometry else {
        panic!("expected torus chart line");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert_eq!(*origin, cadmpeg_ir::math::Point2::new(0.0, 0.0));
    assert_eq!(
        *direction,
        cadmpeg_ir::math::Point2::new(-3.0 * std::f64::consts::FRAC_PI_2, 0.0)
    );
    let range = crate::test_support::with_service_context(|ctx| {
        circle_parameter_range_from_surface_branch(
            ctx,
            crate::assemble::CircleParameterRangeFromSurfaceBranchInputs {
                surface: &surface,
                center: Point3::new(0.0, 0.0, 0.0),
                radius: 7.0,
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                start: Point3::new(7.0, 0.0, 0.0),
                end: Point3::new(0.0, 7.0, 0.0),
                pcurve_origin: *line_pcurve.origin(),
                pcurve_direction: (*line_pcurve.direction()).into(),
            },
        )
    })
    .expect("circle evaluation resources")
    .expect("torus circle range");
    assert!(((range[1] - range[0]).abs() - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
}

#[test]
fn standard_torus_witness_selects_complementary_meridian_arc() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
            2.0,
        )
        .expect("valid TorusSurface fixture"),
    ));
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(Point3::new(5.0, 0.0, 0.0), 2.0),
    };
    let start = Point3::new(7.0, 0.0, 0.0);
    let end = Point3::new(5.0, 0.0, 2.0);
    let witness = Point3::new(5.0, 0.0, -2.0);
    let (geometry, _) = standard_pcurve_geometry(
        &surface,
        &support,
        start,
        end,
        Some(FinitePoint3::new(witness).expect("finite witness")),
        None,
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("witnessed torus meridian");
    let PcurveGeometry::Line(line_pcurve) = geometry else {
        panic!("expected torus meridian chart line");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    let long_sweep = std::f64::consts::FRAC_PI_2 - std::f64::consts::TAU;
    assert_eq!(*origin, cadmpeg_ir::math::Point2::new(0.0, 0.0));
    assert_eq!(*direction, cadmpeg_ir::math::Point2::new(0.0, long_sweep));

    let range = crate::test_support::with_service_context(|ctx| {
        circle_parameter_range_from_surface_branch(
            ctx,
            crate::assemble::CircleParameterRangeFromSurfaceBranchInputs {
                surface: &surface,
                center: Point3::new(5.0, 0.0, 0.0),
                radius: 2.0,
                axis: Vector3::new(0.0, -1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                start,
                end,
                pcurve_origin: *line_pcurve.origin(),
                pcurve_direction: (*line_pcurve.direction()).into(),
            },
        )
    })
    .expect("circle evaluation resources")
    .expect("torus meridian circle range");
    assert_eq!(range, [0.0, long_sweep]);
}

#[test]
fn arc_witness_selects_tiny_nonzero_sweep() {
    let sweep = 1e-200;
    assert_eq!(witness_arc_end(0.0, sweep, sweep * 0.5), Some(sweep));
}

#[test]
fn standard_sphere_latitude_inverts_to_isoparametric_line() {
    let latitude = 0.4f64;
    let radius = 5.0;
    let ring = radius * latitude.cos();
    let height = radius * latitude.sin();
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
        )
        .expect("valid SphereSurface fixture"),
    ));
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(Point3::new(0.0, 0.0, height), ring),
    };
    let (geometry, _) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(ring, 0.0, height),
        Point3::new(0.0, ring, height),
        None,
        None,
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("sphere latitude pcurve");
    let PcurveGeometry::Line(line_pcurve) = geometry else {
        panic!("expected line pcurve");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert!(origin.u.abs() < 1.0e-12);
    assert!((origin.v - latitude).abs() < 1.0e-12);
    assert!((direction.u - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert!(direction.v.abs() < 1.0e-12);
}

#[test]
fn generated_analytic_curve_ranges_use_angular_parameters() {
    const ANGLE_TOLERANCE: f64 = 1e-12;

    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
            2.0,
        )
        .expect("valid EllipseCurve fixture"),
    ));
    let start = curve_point(&geometry, 0.0).expect("ellipse start");
    let end = curve_point(&geometry, std::f64::consts::FRAC_PI_2).expect("ellipse end");
    let witness = curve_point(&geometry, 0.75 * std::f64::consts::PI).expect("ellipse witness");
    let short = standard_analytic_curve_parameter_range(&geometry, start.get(), end.get(), None)
        .expect("short angular range");
    let mut oriented = geometry.clone();
    let long = standard_oriented_analytic_curve_parameter_range(
        &mut oriented,
        start.get(),
        end.get(),
        witness.get(),
    )
    .expect("witnessed angular range");
    assert!((short[0] - 0.0).abs() < ANGLE_TOLERANCE);
    assert!((short[1] - std::f64::consts::FRAC_PI_2).abs() < ANGLE_TOLERANCE);
    assert!((long[1] - 1.5 * std::f64::consts::PI).abs() < ANGLE_TOLERANCE);
}

#[test]
fn native_edge_support_match_refuses_work_limit() {
    let source = crate::test_support::with_service_context(|ctx| {
        crate::assemble::cgm_source(ctx, "surface", 42)
    })
    .expect("service profile admits source identity");
    let id = SurfaceId::mint("catia:test:surface#matching".to_owned()).expect("surface identity");
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: id.clone(),
        geometry: geometry.clone(),
        source_object: Some(source),
    });
    let carrier = crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(geometry);
    let refused =
        crate::test_support::with_work_refusal("catia_native_edge_support_source_scan", |ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            ensure_native_edge_support_surface(
                &mut ir,
                &mut AnnotationBuilder::new(),
                42,
                &carrier,
                &mut admission,
            )
        });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_edge_support_source_scan")
    );
    let matched = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        ensure_native_edge_support_surface(
            &mut ir,
            &mut AnnotationBuilder::new(),
            42,
            &carrier,
            &mut admission,
        )
    })
    .expect("service profile admits surface match");
    assert_eq!(matched, id);
    assert_eq!(ir.model.surfaces.len(), 1);
}

#[test]
fn native_edge_support_nurbs_copy_refuses_collection_limit() {
    let carrier = crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(unit_square_surface())),
    );
    let refused = crate::test_support::with_collection_limit(1, |ctx| {
        let mut ir = CadIr::empty();
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        ensure_native_edge_support_surface(
            &mut ir,
            &mut AnnotationBuilder::new(),
            42,
            &carrier,
            &mut admission,
        )
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_edge_support_geometry")
    );
    let admitted = crate::test_support::with_service_context(|ctx| {
        let mut ir = CadIr::empty();
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let id = ensure_native_edge_support_surface(
            &mut ir,
            &mut AnnotationBuilder::new(),
            42,
            &carrier,
            &mut admission,
        )?;
        Ok::<_, cadmpeg_core::CodecError>((id, ir.model.surfaces))
    })
    .expect("service budget admits native edge support surface");
    assert_eq!(admitted.1.len(), 1);
    assert_eq!(admitted.0, admitted.1[0].id);
}
