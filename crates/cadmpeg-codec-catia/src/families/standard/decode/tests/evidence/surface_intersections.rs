// SPDX-License-Identifier: Apache-2.0

use crate::families::standard::decode::edge_geometry::build_standard_edge_curve;
use crate::families::standard::decode::edge_geometry::standard_spline_line;
use crate::families::standard::decode::StandardEdgeSupport;
use crate::families::standard::decode::CYLINDER_CYLINDER_CONIC_TOLERANCE;
use crate::families::standard::decode::CYLINDER_PLANE_CONIC_TOLERANCE;
use crate::families::standard::decode::SPHERE_SECTION_ENDPOINT_TOLERANCE;
use crate::families::standard::records::StandardCurveGeometry;
use crate::families::standard::records::StandardCurveSupport;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::blends::RollingBallJetDerivative;
use cadmpeg_ir::geometry::blends::RollingBallJetSite;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::AnnotationBuilder;
use std::collections::HashMap;

#[test]
fn standard_planar_spline_edge_solves_line_and_retains_intersection_construction() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    for (index, position) in [Point3::new(1.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.geometry.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::geometry::FinitePoint3::new(position)
                .expect("a finite position is a point"),
            None,
        ));
    }
    for index in 0..2 {
        ir.geometry.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#surface-{index}"))
                .expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    if index == 0 {
                        Vector3::new(0.0, 0.0, 1.0)
                    } else {
                        Vector3::new(0.0, 1.0, 0.0)
                    },
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
            native_ref: None,
        });
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let (id, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                ir: &mut ir,
                annotations: &mut annotations,
                bindings: &[
                    (
                        SurfaceId::mint("catia:test:surface#surface-0".to_string())
                            .expect("identity grammar"),
                        false,
                        0,
                    ),
                    (
                        SurfaceId::mint("catia:test:surface#surface-1".to_string())
                            .expect("identity grammar"),
                        false,
                        1,
                    ),
                ],
                surface_indices: &HashMap::from([
                    (
                        SurfaceId::mint("catia:test:surface#surface-0".to_string())
                            .expect("identity grammar"),
                        0,
                    ),
                    (
                        SurfaceId::mint("catia:test:surface#surface-1".to_string())
                            .expect("identity grammar"),
                        1,
                    ),
                ]),
                brep: &[],
                support: &support,
                points: [0, 1],
                face_bounds: &[],
                native_support: None,
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    })
    .expect("valid source object identity");
    let id = id.expect("spline support identifies a curve carrier");
    assert_eq!(range, Some([0.0, 3.0]));
    assert_eq!(ir.geometry.curves[0].id, id);
    assert_eq!(
        ir.geometry.curves[0].geometry.solved_cache(),
        Some(&SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(1.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0)
            )
            .expect("valid LineCurve fixture")
        ))
    );
    let [procedural] = ir.geometry.procedural_curves.as_slice() else {
        panic!("one procedural curve");
    };
    let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition() else {
        panic!("intersection construction");
    };
    assert_eq!(
        ir.geometry.procedural_curve_owner(&procedural.id),
        Some(&id)
    );
    assert!(context.sides()[0]
        .surface
        .as_ref()
        .is_some_and(|id| id.as_str() == "catia:test:surface#surface-0"));
    assert!(context.sides()[1]
        .surface
        .as_ref()
        .is_some_and(|id| id.as_str() == "catia:test:surface#surface-1"));
    assert_eq!(context.parameter_range().endpoints(), [0.0, 3.0]);
}

#[test]
fn standard_sphere_plane_spline_edge_derives_unbounded_circle_carrier() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let section_radius = 3.0_f64.sqrt();
    ir.geometry.points.extend(
        [
            Point3::new(1.0 + section_radius, 3.0, 3.0),
            Point3::new(1.0 - section_radius, 3.0, 3.0),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, position)| {
            Point::new(
                PointId::mint(format!("catia:test:point#point-{index}")).expect("identity grammar"),
                cadmpeg_ir::features::geometry::FinitePoint3::new(position)
                    .expect("a finite position is a point"),
                None,
            )
        }),
    );
    let sphere_id =
        SurfaceId::mint("catia:test:surface#sphere".to_string()).expect("identity grammar");
    let plane_id =
        SurfaceId::mint("catia:test:surface#plane".to_string()).expect("identity grammar");
    ir.geometry.surfaces.extend([
        Surface {
            id: sphere_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .expect("valid SphereSurface fixture"),
            )),
            source_object: None,
            native_ref: None,
        },
        Surface {
            id: plane_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(1.0, 3.0, 3.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
            native_ref: None,
        },
    ]);
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let (id, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                ir: &mut ir,
                annotations: &mut annotations,
                bindings: &[(sphere_id.clone(), false, 0), (plane_id.clone(), false, 1)],
                surface_indices: &HashMap::from([(sphere_id, 0), (plane_id, 1)]),
                brep: &[],
                support: &support,
                points: [0, 1],
                face_bounds: &[],
                native_support: None,
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    })
    .expect("valid source object identity");
    let id = id.expect("spline support identifies a curve carrier");
    assert_eq!(range, None);
    let Some(SolvedCurveGeometry::Circle(circle_curve)) = ir.geometry.curves[0].geometry.solved()
    else {
        panic!("sphere-plane spline did not derive a circle");
    };
    let center = circle_curve.center().get();
    let axis = circle_curve.frame().axis().as_raw();
    let radius = circle_curve.radius().get();
    assert!(center.distance(Point3::new(1.0, 3.0, 3.0)) <= SPHERE_SECTION_ENDPOINT_TOLERANCE);
    assert!(axis.cross(Vector3::new(0.0, 1.0, 0.0)).norm() <= SPHERE_SECTION_ENDPOINT_TOLERANCE);
    assert!((radius - section_radius).abs() <= SPHERE_SECTION_ENDPOINT_TOLERANCE);
    assert_eq!(ir.geometry.curves[0].id, id);
    assert!(ir.geometry.procedural_curves.is_empty());
}

#[test]
fn standard_cylinder_plane_spline_edge_derives_ellipse_carrier() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let sqrt_three = 3.0_f64.sqrt();
    ir.geometry.points.extend(
        [
            Point3::new(0.0, -2.0 / sqrt_three, -2.0),
            Point3::new(0.0, 2.0 / sqrt_three, 2.0),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, position)| {
            Point::new(
                PointId::mint(format!("catia:test:point#point-{index}")).expect("identity grammar"),
                cadmpeg_ir::features::geometry::FinitePoint3::new(position)
                    .expect("a finite position is a point"),
                None,
            )
        }),
    );
    let cylinder_id =
        SurfaceId::mint("catia:test:surface#cylinder".to_string()).expect("identity grammar");
    let plane_id =
        SurfaceId::mint("catia:test:surface#plane".to_string()).expect("identity grammar");
    ir.geometry.surfaces.extend([
        Surface {
            id: cylinder_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .expect("valid CylinderSurface fixture"),
            )),
            source_object: None,
            native_ref: None,
        },
        Surface {
            id: plane_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, sqrt_three / 2.0, -0.5),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
            native_ref: None,
        },
    ]);
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let (id, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                ir: &mut ir,
                annotations: &mut annotations,
                bindings: &[
                    (cylinder_id.clone(), false, 0),
                    (plane_id.clone(), false, 1),
                ],
                surface_indices: &HashMap::from([(cylinder_id, 0), (plane_id, 1)]),
                brep: &[],
                support: &support,
                points: [0, 1],
                face_bounds: &[],
                native_support: None,
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    })
    .expect("valid source object identity");
    let id = id.expect("spline support identifies a curve carrier");
    assert_eq!(range, None);
    let Some(SolvedCurveGeometry::Ellipse(ellipse_curve)) = ir.geometry.curves[0].geometry.solved()
    else {
        panic!("cylinder-plane spline did not derive an ellipse");
    };
    let center = ellipse_curve.center().get();
    let axis = ellipse_curve.frame().axis().as_raw();
    let major_direction = ellipse_curve.frame().reference().as_raw();
    let major_radius = ellipse_curve.major_radius().get();
    let minor_radius = ellipse_curve.minor_radius().get();
    assert!(center.distance(Point3::new(0.0, 0.0, 0.0)) <= CYLINDER_PLANE_CONIC_TOLERANCE);
    assert!(
        axis.cross(Vector3::new(0.0, sqrt_three / 2.0, -0.5)).norm()
            <= CYLINDER_PLANE_CONIC_TOLERANCE
    );
    assert!(
        major_direction
            .dot(Vector3::new(0.0, -0.5, -sqrt_three / 2.0))
            .abs()
            >= 1.0 - CYLINDER_PLANE_CONIC_TOLERANCE
    );
    assert!((major_radius - 4.0 / sqrt_three).abs() <= CYLINDER_PLANE_CONIC_TOLERANCE);
    assert!((minor_radius - 2.0).abs() <= CYLINDER_PLANE_CONIC_TOLERANCE);
    assert_eq!(ir.geometry.curves[0].id, id);
    assert!(ir.geometry.procedural_curves.is_empty());
}

#[test]
fn standard_equal_perpendicular_cylinders_select_one_ellipse_branch() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    ir.geometry.points.extend(
        [Point3::new(2.0, 0.0, 2.0), Point3::new(-2.0, 0.0, -2.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                Point::new(
                    PointId::mint(format!("catia:test:point#point-{index}"))
                        .expect("identity grammar"),
                    cadmpeg_ir::features::geometry::FinitePoint3::new(position)
                        .expect("a finite position is a point"),
                    None,
                )
            }),
    );
    let first_id =
        SurfaceId::mint("catia:test:surface#first-cylinder".to_string()).expect("identity grammar");
    let second_id = SurfaceId::mint("catia:test:surface#second-cylinder".to_string())
        .expect("identity grammar");
    ir.geometry.surfaces.extend([
        Surface {
            id: first_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .expect("valid CylinderSurface fixture"),
            )),
            source_object: None,
            native_ref: None,
        },
        Surface {
            id: second_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    2.0,
                )
                .expect("valid CylinderSurface fixture"),
            )),
            source_object: None,
            native_ref: None,
        },
    ]);
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let (id, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                ir: &mut ir,
                annotations: &mut annotations,
                bindings: &[(first_id.clone(), false, 0), (second_id.clone(), false, 1)],
                surface_indices: &HashMap::from([(first_id, 0), (second_id, 1)]),
                brep: &[],
                support: &support,
                points: [0, 1],
                face_bounds: &[],
                native_support: None,
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    })
    .expect("valid source object identity");
    let id = id.expect("spline support identifies a curve carrier");
    assert_eq!(range, None);
    let Some(SolvedCurveGeometry::Ellipse(ellipse_curve)) = ir.geometry.curves[0].geometry.solved()
    else {
        panic!("perpendicular cylinders did not select an ellipse branch");
    };
    let center = ellipse_curve.center().get();
    let axis = ellipse_curve.frame().axis().as_raw();
    let major_direction = ellipse_curve.frame().reference().as_raw();
    let major_radius = ellipse_curve.major_radius().get();
    let minor_radius = ellipse_curve.minor_radius().get();
    assert!(center.distance(Point3::new(0.0, 0.0, 0.0)) <= CYLINDER_CYLINDER_CONIC_TOLERANCE);
    assert!(
        axis.dot(Vector3::new(-1.0, 0.0, 1.0).scale(1.0 / 2.0_f64.sqrt()))
            .abs()
            >= 1.0 - CYLINDER_CYLINDER_CONIC_TOLERANCE
    );
    assert!(
        major_direction
            .dot(Vector3::new(1.0, 0.0, 1.0).scale(1.0 / 2.0_f64.sqrt()))
            .abs()
            >= 1.0 - CYLINDER_CYLINDER_CONIC_TOLERANCE
    );
    assert!((major_radius - 2.0 * 2.0_f64.sqrt()).abs() <= CYLINDER_CYLINDER_CONIC_TOLERANCE);
    assert!((minor_radius - 2.0).abs() <= CYLINDER_CYLINDER_CONIC_TOLERANCE);
    assert_eq!(ir.geometry.curves[0].id, id);
    assert!(ir.geometry.procedural_curves.is_empty());
}

#[test]
fn standard_spline_retains_a_procedural_rolling_ball_support() {
    let mut ir = CadIr::empty();
    ir.geometry.points.extend(
        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                Point::new(
                    PointId::mint(format!("catia:test:point#point-{index}"))
                        .expect("identity grammar"),
                    cadmpeg_ir::features::geometry::FinitePoint3::new(position)
                        .expect("a finite position is a point"),
                    None,
                )
            }),
    );
    let support = StandardCurveSupport {
        pos: 12,
        tag: 40,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    let plane = crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
    );
    let rolling_ball_definition = ProceduralSurfaceDefinition::RollingBallJet(
        cadmpeg_ir::geometry::blends::RollingBallJetStations::try_new(
            5,
            [0.0, 1.0]
                .into_iter()
                .map(|knot| cadmpeg_ir::geometry::blends::RollingBallJetStation {
                    knot,
                    multiplicity: 6,
                    site: RollingBallJetSite {
                        first_limit: Point3::new(0.0, 0.0, 0.0),
                        second_limit: Point3::new(0.0, 1.0, 0.0),
                        center: Point3::new(0.0, 0.5, 0.0),
                        angle: std::f64::consts::PI,
                        first_derivative: RollingBallJetDerivative {
                            first_limit: Vector3::new(0.0, 0.0, 0.0),
                            second_limit: Vector3::new(0.0, 0.0, 0.0),
                            center: Vector3::new(0.0, 0.0, 0.0),
                            angle: 0.0,
                        },
                        second_derivative: RollingBallJetDerivative {
                            first_limit: Vector3::new(0.0, 0.0, 0.0),
                            second_limit: Vector3::new(0.0, 0.0, 0.0),
                            center: Vector3::new(0.0, 0.0, 0.0),
                            angle: 0.0,
                        },
                    },
                })
                .collect(),
        )
        .expect("valid RollingBallJetStations fixture"),
    );
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            plane,
            crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall {
                carrier_object_id: 22,
                definition: Box::new(rolling_ball_definition.clone()),
            },
        ],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [2.0, 5.0],
    };
    let (curve, _) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                ir: &mut ir,
                annotations: &mut AnnotationBuilder::new(),
                bindings: &[],
                surface_indices: &HashMap::new(),
                brep: &[],
                support: &support,
                points: [0, 1],
                face_bounds: &[],
                native_support: Some(&native),
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    })
    .expect("valid source object identity");
    let curve = curve.expect("procedural support identifies the curve");
    assert_eq!(ir.geometry.surfaces.len(), 2);
    let [procedural] = ir.geometry.procedural_surfaces.as_slice() else {
        panic!("one procedural surface");
    };
    assert_eq!(
        ir.geometry
            .procedural_surface_owner(&procedural.id)
            .map(cadmpeg_ir::ids::SurfaceId::as_str),
        Some("catia:standard:edge-support-surface#21")
    );
    assert_eq!(procedural.definition(), &rolling_ball_definition);
    assert!(ir
        .geometry
        .procedural_curves
        .iter()
        .any(|construction| ir.geometry.procedural_curve_owner(&construction.id) == Some(&curve)));
}

#[test]
fn standard_intersection_entity_limit_refuses_before_procedural_curve_creation() {
    let mut ir = CadIr::empty();
    ir.geometry.points.extend(
        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                Point::new(
                    PointId::mint(format!("catia:test:point#point-{index}"))
                        .expect("identity grammar"),
                    cadmpeg_ir::features::geometry::FinitePoint3::new(position)
                        .expect("a finite position is a point"),
                    None,
                )
            }),
    );
    let support = StandardCurveSupport {
        pos: 12,
        tag: 40,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    let carrier = |height| {
        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, height),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            ),
        ))
    };
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [carrier(0.0), carrier(1.0)],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [2.0, 5.0],
    };
    crate::test_support::with_entity_limit(3, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                ir: &mut ir,
                annotations: &mut AnnotationBuilder::new(),
                bindings: &[],
                surface_indices: &HashMap::new(),
                brep: &[],
                support: &support,
                points: [0, 1],
                face_bounds: &[],
                native_support: Some(&native),
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        ) else {
            panic!("intersection construction must exceed the three-entity limit");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::Entities
        );
        assert_eq!(limit.used, 3);
        assert_eq!(limit.operation, "admit CATIA family model entity");
    });
    assert_eq!(ir.geometry.curves.len(), 1);
    assert_eq!(ir.geometry.surfaces.len(), 2);
    assert!(ir.geometry.procedural_curves.is_empty());
}

#[test]
fn same_surface_spline_requires_an_exact_ruled_surface_generator() {
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let solve = |geometry, points: [Point3; 2]| {
        let mut ir = CadIr::empty();
        ir.geometry.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:surface#surface".to_string())
                .expect("identity grammar"),
            geometry,
            source_object: None,
            native_ref: None,
        });
        ir.geometry
            .points
            .extend(points.into_iter().enumerate().map(|(index, position)| {
                Point::new(
                    PointId::mint(format!("catia:test:point#point-{index}"))
                        .expect("identity grammar"),
                    cadmpeg_ir::features::geometry::FinitePoint3::new(position)
                        .expect("a finite position is a point"),
                    None,
                )
            }));
        standard_spline_line(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &ir,
            &[(
                SurfaceId::mint("catia:test:surface#surface".to_string())
                    .expect("identity grammar"),
                false,
                0,
            )],
            &HashMap::from([(
                SurfaceId::mint("catia:test:surface#surface".to_string())
                    .expect("identity grammar"),
                0,
            )]),
            &support,
            [0, 1],
        )
        .expect("surface evaluator accepts the fixture")
    };
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    assert!(solve(
        cylinder.clone(),
        [Point3::new(2.0, 0.0, -3.0), Point3::new(2.0, 0.0, 4.0)]
    )
    .is_some());
    assert!(solve(
        cylinder.clone(),
        [Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1e-200)]
    )
    .is_some());
    assert!(solve(
        cylinder.clone(),
        [Point3::new(2., 0., 0.), Point3::new(2., 0., 1e-310)]
    )
    .is_some());
    for radius in [0.0005, 2.0] {
        let small = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
                radius,
            )
            .expect("valid finite regression fixture"),
        ));
        assert!(solve(
            small,
            [Point3::new(radius, 0., 0.), Point3::new(-radius, 0., 0.)]
        )
        .is_none());
    }
    assert!(solve(
        cylinder,
        [Point3::new(2.0, 0.0, 0.0), Point3::new(0.0, 2.0, 0.0)]
    )
    .is_none());

    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(2.0, 0.0, 2.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("valid ConeSurface fixture"),
    ));
    assert!(solve(
        cone.clone(),
        [Point3::new(3.0, 0.0, 1.0), Point3::new(5.0, 0.0, 3.0)]
    )
    .is_some());
    assert!(solve(
        cone,
        [Point3::new(3.0, 0.0, 1.0), Point3::new(2.0, 2.0, 2.0)]
    )
    .is_none());
}

#[test]
fn numerical_ranges_standard_line_rejects_cylinder_chord_mismatch() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
            0.1,
        )
        .expect("an axis-aligned cylinder of radius 0.1 is representable"),
    ));
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    };
    let start = Point3::new(0.1, 0., 0.);
    for (end, accepted) in [
        (
            Point3::new(0.1 * 1.0_f64.cos(), 0.1 * 1.0_f64.sin(), 0.),
            false,
        ),
        (Point3::new(0.1, 0., 1.), true),
    ] {
        let result = crate::test_support::with_service_context(|ctx| {
            crate::families::standard::decode::edge_geometry::standard_pcurve_geometry(
                ctx,
                &surface,
                &support,
                (start, end),
                None,
                None,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("compute the standard pcurve geometry");
        assert_eq!(result.is_some(), accepted);
    }
}

#[test]
fn equal_intersecting_cylinders_have_conic_sections_at_oblique_angles() {
    use crate::families::standard::decode::edge_geometry::standard_spline_intersecting_cylinders;
    let radius = 2.0;
    for angle in [
        std::f64::consts::FRAC_PI_3,
        2.0 * std::f64::consts::FRAC_PI_3,
    ] {
        let major = Vector3::new((angle / 2.0).sin(), 0.0, (angle / 2.0).cos());
        let expected_radius = radius / (angle / 2.0).sin();
        let mut ir = CadIr::empty();
        for (index, sign) in [1.0, -1.0].into_iter().enumerate() {
            ir.geometry.points.push(Point::new(
                PointId::mint(format!("catia:test:point#{index}")).expect("point id"),
                cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(
                    major.x * expected_radius * sign,
                    0.0,
                    major.z * expected_radius * sign,
                ))
                .expect("ellipse endpoint"),
                None,
            ));
        }
        let mut bindings = Vec::new();
        let mut indices = HashMap::new();
        for (index, axis) in [
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(angle.sin(), 0.0, angle.cos()),
        ]
        .into_iter()
        .enumerate()
        {
            let id = SurfaceId::mint(format!("catia:test:surface#{index}")).expect("surface id");
            indices.insert(id.clone(), index);
            bindings.push((id.clone(), true, 0));
            ir.geometry.surfaces.push(Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        axis,
                        Vector3::new(0.0, 1.0, 0.0),
                        radius,
                    )
                    .expect("cylinder"),
                )),
                source_object: None,
                native_ref: None,
            });
        }
        let support = StandardCurveSupport {
            pos: 0,
            tag: 7,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Bspline,
        };
        let curve = crate::test_support::with_service_context(|ctx| {
            standard_spline_intersecting_cylinders(ctx, &ir, &bindings, &indices, &support, [0, 1])
        })
        .expect("intersection resources")
        .expect("one ellipse branch");
        let Some(SolvedCurveGeometry::Ellipse(ellipse)) = curve.solved() else {
            panic!("ellipse carrier");
        };
        assert!((ellipse.major_radius().get() - expected_radius).abs() < 1e-12);
        for step in 0..32 {
            let point = cadmpeg_ir::eval::decode::curve_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &curve,
                f64::from(step) * std::f64::consts::TAU / 32.0,
            )
            .expect("ellipse point")
            .get();
            for axis in [
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(angle.sin(), 0.0, angle.cos()),
            ] {
                let offset = point.vector_from(Point3::new(0.0, 0.0, 0.0));
                assert!(((offset - axis.scale(offset.dot(axis))).norm() - radius).abs() < 1e-12);
            }
        }
    }
}
