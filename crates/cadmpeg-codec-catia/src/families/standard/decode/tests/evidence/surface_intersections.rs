// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn standard_planar_spline_edge_solves_line_and_retains_intersection_construction() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    for (index, position) in [Point3::new(1.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.points.push(Point {
            id: PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            position,
            source_object: None,
        });
    }
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#surface-{index}"))
                .expect("identity grammar"),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: if index == 0 {
                    Vector3::new(0.0, 0.0, 1.0)
                } else {
                    Vector3::new(0.0, 1.0, 0.0)
                },
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let (id, range) = build_standard_edge_curve(
        &mut ir,
        &mut annotations,
        &[
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
        &HashMap::from([
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
        &[],
        &support,
        [0, 1],
        None,
        None,
    )
    .expect("valid source object identity");
    let id = id.expect("spline support identifies a curve carrier");
    assert_eq!(range, Some([0.0, 3.0]));
    assert_eq!(ir.model.curves[0].id, id);
    assert_eq!(
        ir.model.curves[0].geometry.solved_cache(),
        Some(&CurveGeometry::Line {
            origin: Point3::new(1.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
    let [procedural] = ir.model.procedural_curves.as_slice() else {
        panic!("one procedural curve");
    };
    let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition() else {
        panic!("intersection construction");
    };
    assert_eq!(ir.model.procedural_curve_owner(&procedural.id), Some(&id));
    assert!(context.sides[0]
        .surface
        .as_ref()
        .is_some_and(|id| id.as_str() == "catia:test:surface#surface-0"));
    assert!(context.sides[1]
        .surface
        .as_ref()
        .is_some_and(|id| id.as_str() == "catia:test:surface#surface-1"));
    assert_eq!(context.parameter_range, [0.0, 3.0]);
}

#[test]
fn standard_sphere_plane_spline_edge_derives_unbounded_circle_carrier() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let section_radius = 3.0_f64.sqrt();
    ir.model.points.extend(
        [
            Point3::new(1.0 + section_radius, 3.0, 3.0),
            Point3::new(1.0 - section_radius, 3.0, 3.0),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, position)| Point {
            id: PointId::mint(format!("catia:test:point#point-{index}")).expect("identity grammar"),
            position,
            source_object: None,
        }),
    );
    let sphere_id =
        SurfaceId::mint("catia:test:surface#sphere".to_string()).expect("identity grammar");
    let plane_id =
        SurfaceId::mint("catia:test:surface#plane".to_string()).expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: sphere_id.clone(),
            geometry: SurfaceGeometry::Sphere {
                center: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: plane_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(1.0, 3.0, 3.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let (id, range) = build_standard_edge_curve(
        &mut ir,
        &mut annotations,
        &[(sphere_id.clone(), false, 0), (plane_id.clone(), false, 1)],
        &HashMap::from([(sphere_id, 0), (plane_id, 1)]),
        &[],
        &support,
        [0, 1],
        None,
        None,
    )
    .expect("valid source object identity");
    let id = id.expect("spline support identifies a curve carrier");
    assert_eq!(range, None);
    let CurveGeometry::Circle {
        center,
        axis,
        radius,
        ..
    } = &ir.model.curves[0].geometry
    else {
        panic!("sphere-plane spline did not derive a circle");
    };
    assert!(center.distance(Point3::new(1.0, 3.0, 3.0)) <= SPHERE_SECTION_ENDPOINT_TOLERANCE);
    assert!(axis.cross(Vector3::new(0.0, 1.0, 0.0)).norm() <= SPHERE_SECTION_ENDPOINT_TOLERANCE);
    assert!((*radius - section_radius).abs() <= SPHERE_SECTION_ENDPOINT_TOLERANCE);
    assert_eq!(ir.model.curves[0].id, id);
    assert!(ir.model.procedural_curves.is_empty());
}

#[test]
fn standard_cylinder_plane_spline_edge_derives_ellipse_carrier() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let sqrt_three = 3.0_f64.sqrt();
    ir.model.points.extend(
        [
            Point3::new(0.0, -2.0 / sqrt_three, -2.0),
            Point3::new(0.0, 2.0 / sqrt_three, 2.0),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, position)| Point {
            id: PointId::mint(format!("catia:test:point#point-{index}")).expect("identity grammar"),
            position,
            source_object: None,
        }),
    );
    let cylinder_id =
        SurfaceId::mint("catia:test:surface#cylinder".to_string()).expect("identity grammar");
    let plane_id =
        SurfaceId::mint("catia:test:surface#plane".to_string()).expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: cylinder_id.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: plane_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, sqrt_three / 2.0, -0.5),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let (id, range) = build_standard_edge_curve(
        &mut ir,
        &mut annotations,
        &[
            (cylinder_id.clone(), false, 0),
            (plane_id.clone(), false, 1),
        ],
        &HashMap::from([(cylinder_id, 0), (plane_id, 1)]),
        &[],
        &support,
        [0, 1],
        None,
        None,
    )
    .expect("valid source object identity");
    let id = id.expect("spline support identifies a curve carrier");
    assert_eq!(range, None);
    let CurveGeometry::Ellipse {
        center,
        axis,
        major_direction,
        major_radius,
        minor_radius,
    } = &ir.model.curves[0].geometry
    else {
        panic!("cylinder-plane spline did not derive an ellipse");
    };
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
    assert!((*major_radius - 4.0 / sqrt_three).abs() <= CYLINDER_PLANE_CONIC_TOLERANCE);
    assert!((*minor_radius - 2.0).abs() <= CYLINDER_PLANE_CONIC_TOLERANCE);
    assert_eq!(ir.model.curves[0].id, id);
    assert!(ir.model.procedural_curves.is_empty());
}

#[test]
fn standard_equal_perpendicular_cylinders_select_one_ellipse_branch() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    ir.model.points.extend(
        [Point3::new(2.0, 0.0, 2.0), Point3::new(-2.0, 0.0, -2.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| Point {
                id: PointId::mint(format!("catia:test:point#point-{index}"))
                    .expect("identity grammar"),
                position,
                source_object: None,
            }),
    );
    let first_id =
        SurfaceId::mint("catia:test:surface#first-cylinder".to_string()).expect("identity grammar");
    let second_id = SurfaceId::mint("catia:test:surface#second-cylinder".to_string())
        .expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: first_id.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: second_id.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(1.0, 0.0, 0.0),
                ref_direction: Vector3::new(0.0, 0.0, 1.0),
                radius: 2.0,
            },
            source_object: None,
        },
    ]);
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let (id, range) = build_standard_edge_curve(
        &mut ir,
        &mut annotations,
        &[(first_id.clone(), false, 0), (second_id.clone(), false, 1)],
        &HashMap::from([(first_id, 0), (second_id, 1)]),
        &[],
        &support,
        [0, 1],
        None,
        None,
    )
    .expect("valid source object identity");
    let id = id.expect("spline support identifies a curve carrier");
    assert_eq!(range, None);
    let CurveGeometry::Ellipse {
        center,
        axis,
        major_direction,
        major_radius,
        minor_radius,
    } = &ir.model.curves[0].geometry
    else {
        panic!("perpendicular cylinders did not select an ellipse branch");
    };
    assert!(center.distance(Point3::new(0.0, 0.0, 0.0)) <= PERPENDICULAR_CYLINDER_CONIC_TOLERANCE);
    assert!(
        axis.dot(Vector3::new(-1.0, 0.0, 1.0).scale(1.0 / 2.0_f64.sqrt()))
            .abs()
            >= 1.0 - PERPENDICULAR_CYLINDER_CONIC_TOLERANCE
    );
    assert!(
        major_direction
            .dot(Vector3::new(1.0, 0.0, 1.0).scale(1.0 / 2.0_f64.sqrt()))
            .abs()
            >= 1.0 - PERPENDICULAR_CYLINDER_CONIC_TOLERANCE
    );
    assert!((*major_radius - 2.0 * 2.0_f64.sqrt()).abs() <= PERPENDICULAR_CYLINDER_CONIC_TOLERANCE);
    assert!((*minor_radius - 2.0).abs() <= PERPENDICULAR_CYLINDER_CONIC_TOLERANCE);
    assert_eq!(ir.model.curves[0].id, id);
    assert!(ir.model.procedural_curves.is_empty());
}

#[test]
fn standard_spline_retains_a_procedural_rolling_ball_support() {
    let mut ir = CadIr::empty();
    ir.model.points.extend(
        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| Point {
                id: PointId::mint(format!("catia:test:point#point-{index}"))
                    .expect("identity grammar"),
                position,
                source_object: None,
            }),
    );
    let support = StandardCurveSupport {
        pos: 12,
        tag: 40,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let pcurve = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let plane =
        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        });
    let rolling_ball_definition = ProceduralSurfaceDefinition::RollingBallJet {
        degree: 5,
        stations: vec![cadmpeg_ir::geometry::RollingBallJetStation {
            knot: 0.0,
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
        }],
    };
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
    let (curve, _) = build_standard_edge_curve(
        &mut ir,
        &mut AnnotationBuilder::new(),
        &[],
        &HashMap::new(),
        &[],
        &support,
        [0, 1],
        Some(&native),
        None,
    )
    .expect("valid source object identity");
    let curve = curve.expect("procedural support identifies the curve");
    assert_eq!(ir.model.surfaces.len(), 2);
    let [procedural] = ir.model.procedural_surfaces.as_slice() else {
        panic!("one procedural surface");
    };
    assert_eq!(
        ir.model
            .procedural_surface_owner(&procedural.id)
            .map(cadmpeg_ir::ids::SurfaceId::as_str),
        Some("catia:standard:edge-support-surface#21")
    );
    assert_eq!(procedural.definition(), &rolling_ball_definition);
    assert!(ir
        .model
        .procedural_curves
        .iter()
        .any(|construction| ir.model.procedural_curve_owner(&construction.id) == Some(&curve)));
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
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:surface#surface".to_string())
                .expect("identity grammar"),
            geometry,
            source_object: None,
        });
        ir.model.points.extend(
            points
                .into_iter()
                .enumerate()
                .map(|(index, position)| Point {
                    id: PointId::mint(format!("catia:test:point#point-{index}"))
                        .expect("identity grammar"),
                    position,
                    source_object: None,
                }),
        );
        standard_spline_line(
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
    };
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
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
        cylinder,
        [Point3::new(2.0, 0.0, 0.0), Point3::new(0.0, 2.0, 0.0)]
    )
    .is_none());

    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(2.0, 0.0, 2.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
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
