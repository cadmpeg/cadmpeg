// SPDX-License-Identifier: Apache-2.0
//! Serialized intersection regressions.

use super::*;

#[test]
fn serialized_surface_curves_select_a_terminal_intersection_branch() {
    let mut ir = CadIr::empty();
    let surfaces = [
        SurfaceId::mint("nx:test:surface#0").expect("identity grammar"),
        SurfaceId::mint("nx:test:surface#1").expect("identity grammar"),
    ];
    for surface in &surfaces {
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
    }
    let curve = CurveId::mint("test:model:entity#nx:test:curve").expect("identity grammar");
    let procedural = ProceduralCurveId::mint("test:model:entity#nx:test:intersection")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: procedural.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve::new(
        procedural,
        ProceduralCurveDefinition::TolerantIntersection {
            construction: cadmpeg_ir::geometry::TolerantIntersectionConstruction::try_new(
                surfaces.clone(),
                [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
                0.01,
            )
            .unwrap(),
            parameterization: None,
            cache: None,
        },
    ));
    let points = [
        PointId::mint("nx:test:point#0").expect("identity grammar"),
        PointId::mint("nx:test:point#1").expect("identity grammar"),
    ];
    let vertices = [
        VertexId::mint("nx:test:vertex#0").expect("identity grammar"),
        VertexId::mint("nx:test:vertex#1").expect("identity grammar"),
    ];
    for index in 0..2 {
        ir.model.points.push(
            Point::new(
                points[index].clone(),
                Point3::new(0.005 + 9.99 * index as f64, 0.0, 0.0),
                None,
            )
            .expect("a finite position is a point"),
        );
        ir.model.vertices.push(Vertex {
            id: vertices[index].clone(),
            point: points[index].clone(),
            tolerance: None,
        });
    }
    let edge = EdgeId::mint("test:model:entity#nx:test:edge").expect("identity grammar");
    ir.model.edges.push(Edge {
        id: edge.clone(),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve)),
        start: vertices[0].clone(),
        end: vertices[1].clone(),
        tolerance: Some(
            cadmpeg_ir::scalar::PositiveReal::new(0.03).expect("positive finite tolerance"),
        ),
    });
    let pcurves = [
        PcurveId::mint("nx:test:pcurve#0").expect("identity grammar"),
        PcurveId::mint("nx:test:pcurve#1").expect("identity grammar"),
    ];
    let faces = [
        FaceId::mint("nx:test:face#0").expect("identity grammar"),
        FaceId::mint("nx:test:face#1").expect("identity grammar"),
    ];
    let loops = [
        LoopId::mint("nx:test:loop#0").expect("identity grammar"),
        LoopId::mint("nx:test:loop#1").expect("identity grammar"),
    ];
    let coedges = [
        CoedgeId::mint("nx:test:coedge#0").expect("identity grammar"),
        CoedgeId::mint("nx:test:coedge#1").expect("identity grammar"),
    ];
    for index in 0..2 {
        ir.model.pcurves.push(Pcurve {
            id: pcurves[index].clone(),
            geometry: PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                )
                .unwrap(),
            ),
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::try_general(
                None,
                Some([0.0, 10.0]),
                Some(0.02),
            )
            .unwrap(),
        });
        ir.model.faces.push(Face {
            id: faces[index].clone(),
            shell: ShellId::mint("test:model:entity#nx:test:shell").expect("identity grammar"),
            surface: surfaces[index].clone(),
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loops[index].clone()]),
            name: None,
            color: None,
            tolerance: Some(
                cadmpeg_ir::scalar::PositiveReal::new(0.03).expect("positive finite tolerance"),
            ),
        });
        ir.model.loops.push(Loop {
            id: loops[index].clone(),
            face: faces[index].clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(vec![coedges[index].clone()], Vec::new())
                    .expect("valid loop ring"),
            ),
        });
        ir.model.coedges.push(Coedge {
            id: coedges[index].clone(),
            owner_loop: loops[index].clone(),
            edge: edge.clone(),
            radial_next: coedges[1 - index].clone(),
            sense: Sense::Forward,
            pcurves: vec![PcurveUse {
                pcurve: pcurves[index].clone(),
                isoparametric: None,
                parameter_range: Some(
                    cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 10.0]).unwrap(),
                ),
            }],
            use_curve: None,
        });
    }
    let serialized = [0, 1]
        .map(|index| {
            (
                ir.model
                    .procedural_curve_owner(&ir.model.procedural_curves[0].id)
                    .expect("intersection owner")
                    .clone(),
                surfaces[index].clone(),
                pcurves[index].clone(),
            )
        })
        .into_iter()
        .collect();
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    assert!(matches!(
        ir.model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::TolerantIntersection {
            parameterization: None,
            ..
        }
    ));
    for pcurve in &mut ir.model.pcurves {
        let cadmpeg_ir::geometry::pcurve::PcurveMetadata::General { form: metadata } =
            &mut pcurve.metadata
        else {
            panic!("fixture uses general pcurve metadata")
        };
        metadata.set_fit_tolerance(Some(0.01)).unwrap();
    }
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );

    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization: Some(parameterization),
        ..
    } = ir.model.procedural_curves[0].definition()
    else {
        panic!("serialized branch transferred");
    };
    assert_eq!(parameterization.parameter_range(), [0.0, 10.0]);
    assert_eq!(ir.model.edges[0].param_range(), Some([0.0, 10.0]));
    assert_eq!(
        cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &surfaces[0],
            5.0,
            0.0
        ),
        cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &surfaces[1],
            5.0,
            0.0
        )
    );

    ir.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::TolerantIntersection {
            parameterization, ..
        } = definition
        else {
            unreachable!();
        };
        *parameterization = None;
    });
    let edge = &mut ir.model.edges[0];
    edge.set_param_range(None).unwrap();
    std::mem::swap(&mut edge.start, &mut edge.end);
    for pcurve in &mut ir.model.pcurves {
        pcurve.geometry = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(10.0, 0.0),
                Point2::new(-1.0, 0.0),
            )
            .unwrap(),
        );
    }
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization: Some(parameterization),
        ..
    } = ir.model.procedural_curves[0].definition()
    else {
        panic!("reversed serialized branch transferred");
    };
    assert!(parameterization.pcurves.iter().all(
        |pcurve| matches!(pcurve, PcurveGeometry::Line(line_pcurve)
                if {
                    let origin = line_pcurve.origin();
        let direction = line_pcurve.direction();
                    origin.u == 0.0 && direction.u == 1.0
                })
    ));
    assert_eq!(ir.model.edges[0].start, vertices[0]);
    assert_eq!(ir.model.edges[0].end, vertices[1]);

    let range = [-1.5, 1.5];
    let canonical = PcurveGeometry::Ellipse(
        cadmpeg_ir::geometry::pcurve::EllipsePcurve::try_new(
            Point2::new(5.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            4.0,
            2.0,
        )
        .unwrap(),
    );
    let endpoints = range.map(|parameter| {
        let uv = cadmpeg_ir::eval::pcurve_uv(&canonical, parameter).unwrap();
        Point3::new(uv.u, uv.v, 0.0)
    });
    for (point, position) in ir.model.points.iter_mut().zip(endpoints) {
        point
            .set_position(position)
            .expect("a finite position is a point");
    }
    ir.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::TolerantIntersection {
            construction: intersection,
            parameterization,
            ..
        } = definition
        else {
            unreachable!();
        };
        let supports = intersection.supports();
        let tolerance = intersection.tolerance();
        *intersection = cadmpeg_ir::geometry::TolerantIntersectionConstruction::try_new(
            supports.clone(),
            endpoints,
            tolerance,
        )
        .unwrap();
        *parameterization = None;
    });
    ir.model.edges[0].set_param_range(None).unwrap();
    for coedge in &mut ir.model.coedges {
        coedge.pcurves[0].parameter_range =
            Some(cadmpeg_ir::geometry::DirectedParameterRange::new(range).unwrap());
    }
    for pcurve in &mut ir.model.pcurves {
        let cadmpeg_ir::geometry::pcurve::PcurveMetadata::General { form: metadata } =
            &mut pcurve.metadata
        else {
            panic!("fixture uses general pcurve metadata")
        };
        {
            let replacement = Some(range);
            edit::replace(metadata, |previous| {
                cadmpeg_ir::geometry::pcurve::PcurveGeneralForm::try_new(
                    previous.wrapper_reversed,
                    replacement,
                    previous.fit_tolerance(),
                )
            })
        }
        .unwrap();
        pcurve.geometry = PcurveGeometry::Ellipse(
            cadmpeg_ir::geometry::pcurve::EllipsePcurve::try_new(
                Point2::new(5.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, -1.0),
                4.0,
                2.0,
            )
            .unwrap(),
        );
    }
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization: Some(parameterization),
        ..
    } = ir.model.procedural_curves[0].definition()
    else {
        panic!("reversed symmetric conic branches transferred");
    };
    assert_eq!(parameterization.parameter_range(), range);
    assert!(parameterization.pcurves.iter().all(
        |pcurve| matches!(pcurve, PcurveGeometry::Ellipse(ellipse_pcurve)
        if {
            let y_axis = ellipse_pcurve.y_axis();
            y_axis.v == 1.0
        })
    ));

    ir.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::TolerantIntersection {
            construction: intersection,
            parameterization,
            ..
        } = definition
        else {
            unreachable!();
        };
        let supports = intersection.supports();
        let endpoints = intersection.endpoints();
        *intersection = cadmpeg_ir::geometry::TolerantIntersectionConstruction::try_new(
            supports.clone(),
            *endpoints,
            10.0,
        )
        .unwrap();
        *parameterization = None;
    });
    ir.model.edges[0].set_param_range(None).unwrap();
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    assert!(matches!(
        ir.model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::TolerantIntersection {
            parameterization: Some(_),
            ..
        }
    ));
}

#[test]
fn closed_serialized_pcurve_uses_carrier_tangent_for_orientation() {
    let mut ir = CadIr::empty();
    let curve = CurveId::mint("test:model:entity#nx:test:closed-orientation-curve")
        .expect("identity grammar");
    let support = SurfaceId::mint("test:model:entity#nx:test:closed-orientation-support")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: support.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let pcurve = PcurveGeometry::Circle(
        cadmpeg_ir::geometry::pcurve::CirclePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            2.0,
        )
        .unwrap(),
    );
    let endpoint = Point3::new(2.0, 0.0, 0.0);

    let oriented = orient_tolerant_intersection_pcurve(
        &ir,
        &curve,
        &support,
        &pcurve,
        [0.0, std::f64::consts::TAU],
        [endpoint, endpoint],
        1.0e-12,
    )
    .expect("reversed lanes pair")
    .expect("carrier tangent selects one closed-branch orientation");
    let uv = cadmpeg_ir::eval::pcurve_uv(&oriented, std::f64::consts::FRAC_PI_2).unwrap();
    assert!((uv.u - 0.0).abs() < 1.0e-12);
    assert!((uv.v - 2.0).abs() < 1.0e-12);
}
