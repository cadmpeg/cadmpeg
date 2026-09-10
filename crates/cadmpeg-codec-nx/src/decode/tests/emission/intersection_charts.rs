use super::*;

#[test]
fn tolerant_edge_becomes_a_two_support_procedural_intersection() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge_id = ir.model.edges[0].id.clone();
    let expected_endpoints = [&ir.model.edges[0].start, &ir.model.edges[0].end].map(|vertex_id| {
        let point_id = &ir
            .model
            .vertices
            .iter()
            .find(|vertex| &vertex.id == vertex_id)
            .expect("edge vertex")
            .point;
        ir.model
            .points
            .iter()
            .find(|point| &point.id == point_id)
            .expect("vertex point")
            .position
    });
    ir.model.edges[0].set_curve(None).unwrap();
    ir.model.edges[0].set_param_range(None).unwrap();
    ir.model.edges[0].tolerance =
        Some(cadmpeg_ir::units::PositiveScalar::new(0.01).expect("positive finite tolerance"));
    let mut edges = std::collections::BTreeMap::new();
    edges.insert(8, edge_id.clone());
    let mut incident_coedges = ir
        .model
        .coedges
        .iter_mut()
        .filter(|coedge| coedge.edge == edge_id)
        .collect::<Vec<_>>();
    assert_eq!(incident_coedges.len(), 2);
    incident_coedges[0].id =
        cadmpeg_ir::ids::CoedgeId::mint("nx:s0:fin#7").expect("identity grammar");
    incident_coedges[1].id =
        cadmpeg_ir::ids::CoedgeId::mint("nx:s0:fin#22").expect("identity grammar");
    let mut stream = partnered_trimmed_topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let fin = stream
        .windows(2)
        .position(|window| window == [0, 17])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 1);
    let graph = crate::topology::Graph::parse(&stream);
    let mut off_support_ir = ir.clone();
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let stream = annotations.stream("nx:test");

    attach_tolerant_edge_intersections(
        &mut ir,
        &graph,
        &edges,
        &crate::decode::ids::IdScope::stream(0),
        &stream,
        &mut annotations,
    );

    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("tolerant edge");
    assert_eq!(edge.param_range(), None);
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve().as_ref())
        .expect("procedural carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Procedural { .. }));
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|procedural| ir.model.procedural_curve_owner(&procedural.id) == Some(&curve.id))
        .expect("intersection construction");
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        construction: intersection,
        parameterization,
    } = procedural.definition()
    else {
        panic!("tolerant intersection definition");
    };
    let supports = intersection.supports();
    let endpoints = intersection.endpoints();
    let tolerance = intersection.tolerance();

    assert_ne!(supports[0], supports[1]);
    assert_eq!(*endpoints, expected_endpoints);
    assert_eq!(tolerance, 0.01);
    assert_eq!(*parameterization, None);

    let start = off_support_ir.model.edges[0].start.clone();
    let point_id = off_support_ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == start)
        .expect("edge vertex")
        .point
        .clone();
    let point = off_support_ir
        .model
        .points
        .iter_mut()
        .find(|point| point.id == point_id)
        .expect("vertex point");
    point.position.x += 0.5;
    point.position.y += 0.5;
    point.position.z += 0.5;
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let stream = annotations.stream("nx:test");
    attach_tolerant_edge_intersections(
        &mut off_support_ir,
        &graph,
        &edges,
        &crate::decode::ids::IdScope::stream(0),
        &stream,
        &mut annotations,
    );
    assert_eq!(off_support_ir.model.edges[0].curve().as_ref(), None);
}

#[test]
fn tolerant_edge_does_not_replace_a_serialized_fin_curve() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge_id = ir.model.edges[0].id.clone();
    ir.model.edges[0].set_curve(None).unwrap();
    ir.model.edges[0].set_param_range(None).unwrap();
    ir.model.edges[0].tolerance =
        Some(cadmpeg_ir::units::PositiveScalar::new(0.01).expect("positive finite tolerance"));
    let edges = std::collections::BTreeMap::from([(8, edge_id.clone())]);
    let mut stream = partnered_trimmed_topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let graph = crate::topology::Graph::parse(&stream);
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let source_stream = annotations.stream("nx:test");

    attach_tolerant_edge_intersections(
        &mut ir,
        &graph,
        &edges,
        &crate::decode::ids::IdScope::stream(0),
        &source_stream,
        &mut annotations,
    );

    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("tolerant edge");
    assert_eq!(edge.curve().as_ref(), None);
    assert_eq!(edge.param_range(), None);
    assert!(ir.model.procedural_curves.is_empty());
}

#[test]
fn opposite_intersection_chart_transfers_adaptively_within_edge_tolerance() {
    let mut ir = cylinder_plane_transfer_fixture(std::f64::consts::TAU, 0.01);

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        unreachable!()
    };
    let pcurve = context.sides()[1].pcurve.as_ref().unwrap();
    let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
        unreachable!()
    };
    assert!(nurbs.control_points().len() > 2);
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let uv = cadmpeg_ir::eval::pcurve_uv(&pcurve.geometry, parameter).unwrap();
        let point =
            cadmpeg_ir::eval::surface_point(&ir.model.surfaces[1].geometry, uv.u, uv.v).unwrap();
        let angle = std::f64::consts::TAU * parameter;
        assert!((point.x - 10.0 * angle.cos()).abs() < 0.01);
        assert!((point.y - 10.0 * angle.sin()).abs() < 0.01);
        assert!(point.z.abs() < 0.01);
    }
}

#[test]
fn opposite_intersection_chart_transfer_fails_closed_at_sample_budget() {
    const TIGHT_EDGE_TOLERANCE: f64 = 0.0001;

    let mut ir =
        cylinder_plane_transfer_fixture(std::f64::consts::TAU * 10_000.0, TIGHT_EDGE_TOLERANCE);
    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        unreachable!()
    };
    assert!(context.sides()[1].pcurve.is_none());
}

#[test]
fn opposite_intersection_blend_contact_transfers_many_candidates_within_budget() {
    const CONTACT_FIT_TOLERANCE: f64 = 1.0e-8;
    const CANDIDATE_COUNT: usize = 300;

    let source_pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
            .unwrap(),
    );
    let mut ir = blend_contact_transfer_fixture(
        CANDIDATE_COUNT,
        &source_pcurve,
        CONTACT_FIT_TOLERANCE,
        true,
    );

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    assert!(ir.model.procedural_curves[1..].iter().all(|procedural| {
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            return false;
        };
        context.sides()[1].pcurve.is_some()
    }));
}

#[test]
fn opposite_intersection_blend_contact_keeps_adaptive_fit_certification() {
    const CONTACT_FIT_TOLERANCE: f64 = 1.0e-2;

    let source_pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.2, 0.0),
                Point2::new(1.0, 0.0),
            ],
            None,
            false,
        )
        .expect("valid blend-contact pcurve"),
    };
    let mut ir = blend_contact_transfer_fixture(1, &source_pcurve, CONTACT_FIT_TOLERANCE, true);

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[1].definition()
    else {
        unreachable!()
    };
    let Some(support) = context.sides()[1].pcurve.as_ref() else {
        panic!("adaptive blend-contact transfer did not produce a pcurve")
    };
    let PcurveGeometry::Nurbs { nurbs } = &support.geometry else {
        panic!("adaptive blend-contact transfer did not produce a NURBS pcurve")
    };
    let source_pcurve = context.sides()[0].pcurve.as_ref().unwrap();
    let target_pcurve = context.sides()[1].pcurve.as_ref().unwrap();
    assert!(nurbs.control_points().len() > 2);
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let source_uv = cadmpeg_ir::eval::pcurve_uv(&source_pcurve.geometry, parameter).unwrap();
        let target_uv = cadmpeg_ir::eval::pcurve_uv(&target_pcurve.geometry, parameter).unwrap();
        assert!((source_uv.u - target_uv.u).abs() <= CONTACT_FIT_TOLERANCE);
        assert_eq!(source_uv.v, target_uv.v);
    }
}

#[test]
fn opposite_intersection_complete_blend_boundary_transfers_many_candidates_without_contact_chart() {
    const BLEND_BOUNDARY_FIT_TOLERANCE: f64 = 1.0e-8;
    const CANDIDATE_COUNT: usize = 300;

    let source_pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
            .unwrap(),
    );
    let mut ir = blend_contact_transfer_fixture(
        CANDIDATE_COUNT,
        &source_pcurve,
        BLEND_BOUNDARY_FIT_TOLERANCE,
        false,
    );

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    assert!(ir.model.procedural_curves[1..].iter().all(|procedural| {
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            return false;
        };
        context.sides()[1].pcurve.is_some()
    }));
}

#[test]
fn opposite_intersection_chart_transfer_scopes_to_new_procedural_curves() {
    let mut ir = cylinder_plane_transfer_fixture(std::f64::consts::TAU, 0.01);
    let mut later = ir.model.procedural_curves[0].clone();
    later.id =
        cadmpeg_ir::ids::ProceduralCurveId::mint("test:model:entity#synthetic:later-intersection")
            .expect("identity grammar");
    let original_owner = ir
        .model
        .procedural_curve_owner(&ir.model.procedural_curves[0].id)
        .unwrap();
    let mut carrier = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *original_owner)
        .unwrap()
        .clone();
    carrier.id = cadmpeg_ir::ids::CurveId::mint("test:model:curve#later-intersection").unwrap();
    let CurveGeometry::Procedural { construction, .. } = &mut carrier.geometry else {
        panic!("procedural carrier");
    };
    *construction = later.id.clone();
    let mut edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve().as_ref() == Some(original_owner))
        .unwrap()
        .clone();
    edge.id = cadmpeg_ir::ids::EdgeId::mint("test:model:edge#later-intersection").unwrap();
    edge.set_curve(Some(carrier.id.clone())).unwrap();
    ir.model.edges.push(edge);
    ir.model.curves.push(carrier);
    ir.model.procedural_curves.push(later);

    let transfer_budget = cadmpeg_core::decode::WorkBudget::new(
        crate::decode::pcurves::MAX_COMPLETION_TRANSFER_SAMPLES,
    );
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(
        crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    );
    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts_with_budget(
        &mut ir,
        1,
        &transfer_budget,
        &geometry_budget,
    );

    let ProceduralCurveDefinition::Intersection { context: first, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        unreachable!()
    };
    assert!(first.sides()[1].pcurve.is_none());
    let ProceduralCurveDefinition::Intersection { context: later, .. } =
        ir.model.procedural_curves[1].definition()
    else {
        unreachable!()
    };
    assert!(later.sides()[1].pcurve.is_some());
}

fn cylinder_plane_transfer_fixture(
    source_pcurve_angle: f64,
    edge_tolerance: f64,
) -> cadmpeg_ir::document::CadIr {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let source =
        SurfaceId::mint("test:model:entity#synthetic:source-cylinder").expect("identity grammar");
    let target =
        SurfaceId::mint("test:model:entity#synthetic:target-plane").expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    10.0,
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let curve =
        CurveId::mint("test:model:entity#synthetic:intersection-curve").expect("identity grammar");
    let construction = ProceduralCurveId::mint("test:model:entity#synthetic:intersection")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(
        ProceduralCurve::new(
            construction,
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(source),
                            pcurve: Some(
                                PcurveGeometry::Line(
                                    cadmpeg_ir::geometry::LinePcurve::try_new(
                                        Point2::new(0.0, 0.0),
                                        Point2::new(source_pcurve_angle, 0.0),
                                    )
                                    .unwrap(),
                                )
                                .into(),
                            ),
                        },
                        IntcurveSupportSide {
                            surface: Some(target.clone()),
                            pcurve: None,
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
        )
        .unwrap(),
    );
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:entity#synthetic:edge").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve), Some([0.0, 1.0])).unwrap(),
        start: VertexId::mint("test:model:entity#synthetic:start").expect("identity grammar"),
        end: VertexId::mint("test:model:entity#synthetic:end").expect("identity grammar"),
        tolerance: Some(
            cadmpeg_ir::units::PositiveScalar::new(edge_tolerance)
                .expect("positive finite tolerance"),
        ),
    });
    ir
}

fn blend_contact_transfer_fixture(
    candidate_count: usize,
    source_pcurve: &PcurveGeometry,
    tolerance: f64,
    contact_on_source_support: bool,
) -> cadmpeg_ir::document::CadIr {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let support = SurfaceId::mint("test:model:entity#synthetic:blend-contact-support")
        .expect("identity grammar");
    let other_support = SurfaceId::mint("test:model:entity#synthetic:blend-contact-other-support")
        .expect("identity grammar");
    let offset = SurfaceId::mint("test:model:entity#synthetic:blend-contact-offset")
        .expect("identity grammar");
    let target = SurfaceId::mint("test:model:entity#synthetic:blend-contact-target")
        .expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: other_support.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: offset.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 2.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: ProceduralSurfaceId::mint(
                    "test:model:entity#synthetic:blend-contact-construction",
                )
                .expect("identity grammar"),
                cache: None,
            },
            source_object: None,
        },
    ]);

    let spine =
        CurveId::mint("test:model:entity#synthetic:blend-contact-spine").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
                Point3::new(0.0, 0.0, 2.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let contact_pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            None,
            false,
        )
        .expect("valid contact pcurve"),
    };
    let contact_surface = if contact_on_source_support {
        offset
    } else {
        other_support.clone()
    };
    let _attached = ir.model.add_procedural_curve(
        spine.clone(),
        ProceduralCurve::new(
            ProceduralCurveId::mint("test:model:entity#synthetic:blend-contact-spine-construction")
                .expect("identity grammar"),
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(contact_surface),
                            pcurve: Some(contact_pcurve.into()),
                        },
                        IntcurveSupportSide {
                            surface: Some(other_support.clone()),
                            pcurve: None,
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
        )
        .unwrap(),
    );
    ir.model.procedural_surfaces.push(
        ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:entity#synthetic:blend-contact-construction")
                .expect("identity grammar"),
            ProceduralSurfaceDefinition::Blend(
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    [
                        Some(BlendSupport {
                            surface: support.clone(),
                            reversed: false,
                        }),
                        Some(BlendSupport {
                            surface: other_support,
                            reversed: false,
                        }),
                    ],
                    Some(spine),
                    BlendRadiusLaw::Constant { signed_radius: 2.0 },
                    BlendCrossSection::Circular,
                    None,
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap(),
    );

    for index in 0..candidate_count {
        let curve = CurveId::mint(format!(
            "test:model:entity#synthetic:blend-contact-curve-{index}"
        ))
        .expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Line(
                cadmpeg_ir::geometry::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        });
        let procedural = ProceduralCurve::try_new(
            ProceduralCurveId::mint(format!(
                "test:model:entity#synthetic:blend-contact-intersection-{index}"
            ))
            .expect("identity grammar"),
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(support.clone()),
                            pcurve: Some(source_pcurve.clone().into()),
                        },
                        IntcurveSupportSide {
                            surface: Some(target.clone()),
                            pcurve: None,
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
            Some(tolerance),
        )
        .unwrap();
        ir.model.add_procedural_curve(curve, procedural).unwrap();
    }
    ir
}

#[test]
fn blend_boundary_chart_uses_the_solved_curve_when_the_source_blend_is_unevaluable() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let source = SurfaceId::mint("test:model:entity#synthetic:unevaluable-source-blend")
        .expect("identity grammar");
    let other_support =
        SurfaceId::mint("test:model:entity#synthetic:other-support").expect("identity grammar");
    let target =
        SurfaceId::mint("test:model:entity#synthetic:target-blend").expect("identity grammar");
    let target_construction =
        ProceduralSurfaceId::mint("test:model:entity#synthetic:target-blend-construction")
            .expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        Surface {
            id: other_support.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: target_construction.clone(),
                cache: None,
            },
            source_object: None,
        },
    ]);
    let spine =
        CurveId::mint("test:model:entity#synthetic:target-spine").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    ir.model.procedural_surfaces.push(
        ProceduralSurface::new(
            target_construction,
            ProceduralSurfaceDefinition::Blend(
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    [
                        Some(BlendSupport {
                            surface: source.clone(),
                            reversed: false,
                        }),
                        Some(BlendSupport {
                            surface: other_support,
                            reversed: false,
                        }),
                    ],
                    Some(spine),
                    BlendRadiusLaw::Constant { signed_radius: 2.0 },
                    BlendCrossSection::Circular,
                    None,
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap(),
    );

    let curve =
        CurveId::mint("test:model:entity#synthetic:solved-boundary").expect("identity grammar");
    let construction = ProceduralCurveId::mint("test:model:entity#synthetic:boundary-intersection")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
                Point3::new(2.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        curve.clone(),
        ProceduralCurve::new(
            construction,
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(source),
                            pcurve: Some(
                                PcurveGeometry::Line(
                                    cadmpeg_ir::geometry::LinePcurve::try_new(
                                        Point2::new(0.0, 0.0),
                                        Point2::new(1.0, 0.0),
                                    )
                                    .unwrap(),
                                )
                                .into(),
                            ),
                        },
                        IntcurveSupportSide {
                            surface: Some(target),
                            pcurve: None,
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
        )
        .unwrap(),
    );
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:entity#synthetic:boundary-edge").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve), Some([0.0, 1.0])).unwrap(),
        start: VertexId::mint("test:model:entity#synthetic:boundary-start")
            .expect("identity grammar"),
        end: VertexId::mint("test:model:entity#synthetic:boundary-end").expect("identity grammar"),
        tolerance: Some(
            cadmpeg_ir::units::PositiveScalar::new(EPS_TOPOLOGY_TOLERANCE)
                .expect("positive finite tolerance"),
        ),
    });

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { nurbs } = &context.sides()[1].pcurve.as_ref().unwrap().geometry
    else {
        unreachable!()
    };
    assert_eq!(nurbs.control_points().first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(nurbs.control_points().last(), Some(&Point2::new(1.0, 0.0)));
}

#[test]
fn tolerant_nurbs_boundary_establishes_both_intersection_charts() {
    use cadmpeg_ir::geometry::{Curve, NurbsSurface, ProceduralCurve, Surface};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, Point, Vertex};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let nurbs =
        SurfaceId::mint("test:model:entity#synthetic:nurbs-boundary").expect("identity grammar");
    let plane =
        SurfaceId::mint("test:model:entity#synthetic:boundary-plane").expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(
                NurbsSurface::new(
                    1,
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(0.0, 5.0, 0.0),
                        Point3::new(10.0, 0.0, 0.0),
                        Point3::new(10.0, 5.0, 0.0),
                    ],
                    None,
                    false,
                    false,
                    false,
                )
                .expect("valid boundary surface"),
            ),
            source_object: None,
        },
        Surface {
            id: plane.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let curve =
        CurveId::mint("test:model:entity#synthetic:boundary-curve").expect("identity grammar");
    let construction = ProceduralCurveId::mint("test:model:entity#synthetic:boundary-intersection")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(
            cadmpeg_ir::geometry::NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        curve.clone(),
        ProceduralCurve::new(
            construction,
            ProceduralCurveDefinition::TolerantIntersection {
                construction: cadmpeg_ir::geometry::TolerantIntersectionConstruction::try_new(
                    [nurbs, plane],
                    [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
                    TOLERANT_INTERSECTION_FIT,
                )
                .unwrap(),
                parameterization: None,
            },
        )
        .unwrap(),
    );
    let point_ids = [
        PointId::mint("test:model:entity#synthetic:p0").expect("identity grammar"),
        PointId::mint("test:model:entity#synthetic:p1").expect("identity grammar"),
    ];
    let vertex_ids = [
        VertexId::mint("test:model:entity#synthetic:v0").expect("identity grammar"),
        VertexId::mint("test:model:entity#synthetic:v1").expect("identity grammar"),
    ];
    ir.model.points.extend([
        Point {
            id: point_ids[0].clone(),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: point_ids[1].clone(),
            position: Point3::new(10.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: vertex_ids[0].clone(),
            point: point_ids[0].clone(),
            tolerance: Some(
                cadmpeg_ir::units::PositiveScalar::new(EPS_TOPOLOGY_TOLERANCE)
                    .expect("positive finite tolerance"),
            ),
        },
        Vertex {
            id: vertex_ids[1].clone(),
            point: point_ids[1].clone(),
            tolerance: Some(
                cadmpeg_ir::units::PositiveScalar::new(EPS_TOPOLOGY_TOLERANCE)
                    .expect("positive finite tolerance"),
            ),
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:entity#synthetic:boundary-edge").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve)),
        start: vertex_ids[0].clone(),
        end: vertex_ids[1].clone(),
        tolerance: Some(
            cadmpeg_ir::units::PositiveScalar::new(EPS_TOPOLOGY_TOLERANCE)
                .expect("positive finite tolerance"),
        ),
    });

    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::pcurves::complete_exact_boundary_intersection_pcurves(&mut ir, &mut annotations);

    let ProceduralCurveDefinition::TolerantIntersection {
        construction: intersection,
        parameterization: Some(parameterization),
        ..
    } = ir.model.procedural_curves[0].definition()
    else {
        unreachable!()
    };
    let supports = intersection.supports();

    assert_eq!(
        ir.model.procedural_curves[0].cache_fit_tolerance(),
        Some(1.0e-8)
    );
    assert_eq!(parameterization.parameter_range(), [0.0, 1.0]);
    assert_eq!(ir.model.edges[0].param_range(), Some([0.0, 1.0]));
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let owner = ir
            .model
            .procedural_curve_owner(&ir.model.procedural_curves[0].id)
            .expect("tolerant intersection owner");
        let evaluated = cadmpeg_ir::eval::model_curve_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            owner,
            parameter,
        )
        .expect("charted tolerant intersection evaluates");
        let inverted =
            cadmpeg_ir::eval::model_curve_parameter_near_point(&ir, owner, evaluated, parameter)
                .expect("charted tolerant intersection inverts");
        assert!((inverted - parameter).abs() < 1.0e-8);
        let points: [Point3; 2] = std::array::from_fn(|side| {
            let uv =
                cadmpeg_ir::eval::pcurve_uv(&parameterization.pcurves[side], parameter).unwrap();
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == supports[side])
                .unwrap();
            cadmpeg_ir::eval::surface_point(&surface.geometry, uv.u, uv.v).unwrap()
        });
        assert!((points[0].x - 10.0 * parameter).abs() < 1.0e-8);
        assert_eq!(evaluated, points[0]);
        assert!(
            (points[0].x - points[1].x)
                .hypot(points[0].y - points[1].y)
                .hypot(points[0].z - points[1].z)
                < 1.0e-8
        );
    }
}

#[test]
fn exact_boundary_completion_preserves_existing_cache_fit_tolerance() {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, Point, Vertex};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let first_support =
        SurfaceId::mint("test:model:entity#nx:test:boundary-plane-a").expect("identity grammar");
    let second_support =
        SurfaceId::mint("test:model:entity#nx:test:boundary-plane-b").expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: first_support.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: second_support.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let curve = CurveId::mint("test:model:entity#nx:test:boundary-line").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(
            cadmpeg_ir::geometry::NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let points = [
        (
            PointId::mint("test:model:entity#nx:test:boundary-point-0").expect("identity grammar"),
            Point3::new(0.0, 0.0, 0.0),
        ),
        (
            PointId::mint("test:model:entity#nx:test:boundary-point-1").expect("identity grammar"),
            Point3::new(10.0, 0.0, 0.0),
        ),
    ];
    ir.model
        .points
        .extend(points.iter().map(|(id, position)| Point {
            id: id.clone(),
            position: *position,
            source_object: None,
        }));
    let vertices = [
        VertexId::mint("test:model:entity#nx:test:boundary-vertex-0").expect("identity grammar"),
        VertexId::mint("test:model:entity#nx:test:boundary-vertex-1").expect("identity grammar"),
    ];
    ir.model.vertices.extend([
        Vertex {
            id: vertices[0].clone(),
            point: points[0].0.clone(),
            tolerance: None,
        },
        Vertex {
            id: vertices[1].clone(),
            point: points[1].0.clone(),
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:entity#nx:test:boundary-edge").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve.clone())),
        start: vertices[0].clone(),
        end: vertices[1].clone(),
        tolerance: Some(
            cadmpeg_ir::units::PositiveScalar::new(EPS_TOPOLOGY_TOLERANCE)
                .expect("positive finite tolerance"),
        ),
    });
    let procedural = ProceduralCurve::try_new(
        ProceduralCurveId::mint("test:model:entity#nx:test:serialized-boundary")
            .expect("identity grammar"),
        ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext::try_new(
                [
                    IntcurveSupportSide {
                        surface: Some(first_support.clone()),
                        pcurve: None,
                    },
                    IntcurveSupportSide {
                        surface: Some(second_support.clone()),
                        pcurve: None,
                    },
                ],
                [0.0, 1.0],
                [Vec::new(), Vec::new(), Vec::new()],
            )
            .unwrap(),
            discontinuity_flag: false,
        },
        Some(0.25),
    )
    .unwrap();
    ir.model.add_procedural_curve(curve, procedural).unwrap();

    crate::decode::pcurves::complete_exact_boundary_intersection_pcurves(
        &mut ir,
        &mut cadmpeg_ir::AnnotationBuilder::new(),
    );

    let procedural = ir
        .model
        .procedural_curves
        .last()
        .expect("boundary construction");
    assert_eq!(procedural.cache_fit_tolerance(), Some(0.25));
    let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition() else {
        panic!("intersection construction");
    };
    assert!(context.sides().iter().all(|side| side.pcurve.is_some()));
}
