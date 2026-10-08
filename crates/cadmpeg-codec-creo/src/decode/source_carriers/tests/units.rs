// SPDX-License-Identifier: Apache-2.0

use super::*;

fn source_sketch_line(x: f64) -> SketchEntity {
    SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("creo:test:sketch_entity#1")
            .expect("identity grammar"),
        cadmpeg_ir::sketches::SketchId::mint("creo:test:sketch#1").expect("identity grammar"),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: cadmpeg_ir::math::Point2::new(x, 0.0),
            end: cadmpeg_ir::math::Point2::new(2.0, 0.0),
        })
        .expect("source line"),
    )
}

#[test]
fn planar_sketch_lengths_are_in_millimeters_at_admission() {
    let mut ir = CadIr::empty();
    let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_sketch(ctx, &mut ir, source_sketch(Point3::new(1.0, 0.0, 0.0)))
    })
    .expect("sketch admission");
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_sketch_entities(ctx, &mut ir, vec![source_sketch_line(1.0)])
    })
    .expect("entity admission");
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_sketch_constraints(ctx, &mut ir, vec![source_distance_constraint(2.0)])
    })
    .expect("constraint admission");
    assert_eq!(
        ir.model.sketches[0]
            .resolved_placement()
            .expect("resolved placement")
            .0
            .get()
            .x,
        25.4
    );
    let SketchGeometryDefinition::Line { start, .. } =
        ir.model.sketch_entities[0].geometry.definition()
    else {
        panic!("sketch line changed family");
    };
    assert_eq!(start.u, 25.4);
    let SketchConstraintDefinitionInput::DistanceLociValue { distance, .. } =
        ir.model.sketch_constraints[0].definition.kind()
    else {
        panic!("distance constraint changed family");
    };
    assert_eq!(distance.get(), 50.8);
    let SketchGeometryDefinition::Line { start, .. } = carriers
        .sketch_geometry(&ir.model.sketch_entities[0])
        .definition()
    else {
        panic!("source sketch line changed family");
    };
    assert_eq!(start.u, 1.0);
}

#[test]
fn sketch_origin_overflow_refuses_before_admission() {
    let mut ir = CadIr::empty();
    let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_sketch(ctx, &mut ir, source_sketch(Point3::new(f64::MAX, 0.0, 0.0)))
    })
    .expect_err("millimeter placement cannot be represented");
    assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    assert!(ir.model.sketches.is_empty());
}

#[test]
fn sketch_entity_overflow_refuses_before_admission() {
    let mut ir = CadIr::empty();
    let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_sketch_entities(ctx, &mut ir, vec![source_sketch_line(f64::MAX)])
    })
    .expect_err("millimeter line cannot be represented");
    assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    assert!(ir.model.sketch_entities.is_empty());
}

#[test]
fn sketch_entity_admission_refuses_each_source_and_model_boundary() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    for (limit, operation) in [
        (0, "creo source sketch entity nodes"),
        (1, "creo model sketch entities"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let mut ir = CadIr::empty();
        let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = carriers
            .admit_sketch_entities(&ctx, &mut ir, vec![source_sketch_line(1.0)])
            .expect_err("one sketch entity exceeds its collection limit");
        assert!(
            matches!(error, CodecError::ResourceLimit(resource)
            if resource.operation == operation),
            "{error:?}"
        );
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let mut ir = CadIr::empty();
    let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let error = carriers
        .admit_sketch_entities(&ctx, &mut ir, vec![source_sketch_line(1.0)])
        .expect_err("source identity copy exceeds retained-byte limit");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo source sketch entity IDs"),
        "{error:?}"
    );
}

#[test]
fn sketch_constraint_overflow_refuses_before_admission() {
    let mut ir = CadIr::empty();
    let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_sketch_constraints(
            ctx,
            &mut ir,
            vec![source_distance_constraint(f64::MAX)],
        )
    })
    .expect_err("millimeter constraint cannot be represented");
    assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    assert!(ir.model.sketch_constraints.is_empty());
}

#[test]
fn datum_offset_distance_is_in_millimeters_at_feature_admission() {
    let mut ir = CadIr::empty();
    let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_feature(
            ctx,
            &mut ir,
            source_feature(FeatureDefinition::Operation(
                FeatureOperation::DatumOffsetPlane {
                    reference: None,
                    distance: cadmpeg_ir::scalar::Length::new(2.0).expect("finite distance"),
                },
            )),
        )
    })
    .expect("feature admission");
    let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { distance, .. }) =
        ir.model.features[0].evaluation.definition()
    else {
        panic!("datum offset feature changed family");
    };
    assert_eq!(distance.get(), 50.8);
}

#[test]
fn post_process_fuzzy_tolerance_is_in_millimeters_at_feature_admission() {
    let mut ir = CadIr::empty();
    let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_feature(
            ctx,
            &mut ir,
            source_feature(FeatureDefinition::PostProcess {
                operation: FeatureOperation::StoredGeometry {},
                refine: false,
                fuzzy_tolerance: FuzzyTolerance::Explicit(
                    cadmpeg_ir::scalar::PositiveLength::new(0.5)
                        .expect("positive source tolerance"),
                ),
            }),
        )
    })
    .expect("feature admission");
    let FeatureDefinition::PostProcess {
        fuzzy_tolerance: FuzzyTolerance::Explicit(tolerance),
        ..
    } = ir.model.features[0].evaluation.definition()
    else {
        panic!("post process feature changed family");
    };
    assert_eq!(tolerance.get(), 12.7);
}

#[test]
fn feature_length_overflow_refuses_before_admission() {
    let mut ir = CadIr::empty();
    let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_feature(
            ctx,
            &mut ir,
            source_feature(FeatureDefinition::Operation(
                FeatureOperation::DatumOffsetPlane {
                    reference: None,
                    distance: cadmpeg_ir::scalar::Length::new(f64::MAX)
                        .expect("finite source distance"),
                },
            )),
        )
    })
    .expect_err("millimeter distance cannot be represented");
    assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    assert!(ir.model.features.is_empty());
}

#[test]
fn parameter_length_overflow_refuses_before_admission() {
    let mut ir = CadIr::empty();
    let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_parameter(ctx, &mut ir, source_length_parameter(f64::MAX))
    })
    .expect_err("millimeter parameter cannot be represented");
    assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    assert!(ir.model.parameters.is_empty());
}

#[test]
fn product_transform_translations_are_in_millimeters_at_admission() {
    let mut ir = CadIr::empty();
    let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_body(
            ctx,
            &mut ir,
            Body {
                id: cadmpeg_ir::ids::BodyId::mint("creo:test:body#0")
                    .expect("identity grammar"),
                kind: BodyKind::Solid,
                regions: Vec::new(),
                transform: Some(translated_product_transform(1.0)),
                name: None,
                color: None,
                visible: None,
            },
        )
    })
    .expect("body admission");
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_occurrence(
            ctx,
            &mut ir,
            source_occurrence(
                translated_product_transform(2.0),
                Some(translated_product_transform(3.0)),
            ),
        )
    })
    .expect("occurrence admission");
    assert_eq!(
        ir.model.bodies[0]
            .transform
            .expect("body transform")
            .affine_rows()[0][3],
        25.4
    );
    assert_eq!(ir.model.occurrences[0].transform.affine_rows()[0][3], 50.8);
    assert_eq!(
        ir.model.occurrences[0]
            .linked_prototype
            .expect("linked prototype")
            .affine_rows()[0][3],
        3.0 * 25.4
    );
}

#[test]
fn product_transform_translation_overflow_refuses_before_admission() {
    let mut ir = CadIr::empty();
    let carriers = SourceUnitCarriers::new(PositiveReal::new(1000.0));
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_occurrence(
            ctx,
            &mut ir,
            source_occurrence(translated_product_transform(f64::MAX), None),
        )
    })
    .expect_err("a non-finite translation has no transform");
    assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    assert!(
        error.to_string().contains("transform translation"),
        "{error}"
    );
    assert!(ir.model.occurrences.is_empty());
}

#[test]
fn scaled_cylinder_radius_overflow_refuses_unrepresentable_ir() {
    let geometry = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        f64::MAX,
    )
    .expect("finite source cylinder");
    let surface = Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(geometry)),
        source_object: None,
    };
    let mut ir = CadIr::empty();
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_surface(ctx, &mut ir, surface)
    })
    .expect_err("millimeter radius cannot be represented");
    assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    assert!(ir.model.surfaces.is_empty());
}

#[test]
fn scaled_line_origin_overflow_refuses_unrepresentable_ir() {
    let geometry = cadmpeg_ir::geometry::analytic::LineCurve::try_new(
        Point3::new(f64::MAX, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("finite source line");
    let curve = Curve {
        id: CurveId::mint("creo:visibgeom:curve#1").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(geometry)),
        source_object: None,
    };
    let mut ir = CadIr::empty();
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_curve(ctx, &mut ir, curve)
    })
    .expect_err("millimeter origin cannot be represented");
    assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    assert!(ir.model.curves.is_empty());
}

#[test]
fn extrusion_construction_direction_is_in_millimeters_at_attachment() {
    let surface_id = SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar");
    let surface = Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid plane fixture"),
        )),
        source_object: None,
    };
    let mut ir = CadIr::empty();
    let scale = PositiveReal::new(25.4).expect("inch scale");
    let mut source_carriers = SourceUnitCarriers::new(Some(scale));
    crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_surface(ctx, &mut ir, surface)
    })
    .expect("surface admission");
    let procedural = ProceduralSurface::new(
        ProceduralSurfaceId::mint("creo:visibgeom:extrusion#1").expect("identity grammar"),
        ProceduralSurfaceDefinition::Extrusion(
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                CurveId::mint("creo:visibgeom:curve#1").expect("identity grammar"),
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .expect("valid extrusion fixture"),
        ),
        None,
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_procedural_surface(ctx, &mut ir, &surface_id, procedural)
    })
    .expect("procedural attachment");
    let ProceduralSurfaceDefinition::Extrusion(construction) =
        ir.model.procedural_surfaces[0].definition()
    else {
        panic!("procedural construction changed family");
    };
    assert_eq!(construction.direction().get(), Vector3::new(0.0, 0.0, 25.4));
    let ProceduralSurfaceDefinition::Extrusion(construction) =
        ir.model.procedural_surfaces[0].definition()
    else {
        panic!("procedural construction changed family");
    };
    assert_eq!(construction.direction().get(), Vector3::new(0.0, 0.0, 25.4));
}

#[test]
fn procedural_surface_without_owner_is_malformed_at_attachment() {
    let mut ir = CadIr::empty();
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let error = crate::decode::with_test_decode_ctx(|ctx| source_carriers
        .admit_procedural_surface(
            ctx,
            &mut ir,
            &SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar"),
            ProceduralSurface::new(
                ProceduralSurfaceId::mint("creo:visibgeom:extrusion#1")
                    .expect("identity grammar"),
                ProceduralSurfaceDefinition::Extrusion(
                    cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                        CurveId::mint("creo:visibgeom:curve#1").expect("identity grammar"),
                        None,
                        Vector3::new(0.0, 0.0, 1.0),
                        None,
                        cadmpeg_ir::geometry::CacheContract::from_form(None),
                    )
                    .expect("valid extrusion fixture"),
                ),
                None,
            ),
        ))
        .expect_err("missing owner must refuse attachment");
    assert!(matches!(error, CodecError::Malformed(_)), "{error}");
    assert!(ir.model.procedural_surfaces.is_empty());
}

#[test]
fn procedural_surface_attachment_refuses_arena_growth() {
    let owner = SurfaceId::mint("creo:visibgeom:surface#1")
        .expect("valid test setup or admitted service result");
    let procedural = ProceduralSurface::new(
        ProceduralSurfaceId::mint("creo:visibgeom:construction#1")
            .expect("valid test setup or admitted service result"),
        ProceduralSurfaceDefinition::Unknown {
            record: None,
            cache: None,
        },
        None,
    );
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: owner.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("valid test setup or admitted service result");
    let error = SourceUnitCarriers::default()
        .admit_procedural_surface(&ctx, &mut ir, &owner.clone(), procedural.clone())
        .expect_err("procedural surface arena exceeds limit");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "store procedural surface constructions"),
        "{error:?}"
    );
    assert!(ir.model.procedural_surfaces.is_empty());
    crate::decode::with_test_decode_ctx(|ctx| {
        SourceUnitCarriers::default().admit_procedural_surface(ctx, &mut ir, &owner, procedural)
    })
    .expect("valid test setup or admitted service result");
    assert_eq!(ir.model.procedural_surfaces.len(), 1);
}

#[test]
fn procedural_curve_attachment_refuses_arena_growth() {
    let owner = CurveId::mint("creo:visibgeom:curve#1")
        .expect("valid test setup or admitted service result");
    let procedural = ProceduralCurve::new(
        ProceduralCurveId::mint("creo:visibgeom:construction#1")
            .expect("valid test setup or admitted service result"),
        ProceduralCurveDefinition::Exact { cache: None },
    );
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: owner.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("valid test setup or admitted service result");
    let error = SourceUnitCarriers::default()
        .admit_procedural_curve(&ctx, &mut ir, &owner.clone(), procedural.clone())
        .expect_err("procedural curve arena exceeds limit");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "store procedural curve constructions"),
        "{error:?}"
    );
    assert!(ir.model.procedural_curves.is_empty());
    crate::decode::with_test_decode_ctx(|ctx| {
        SourceUnitCarriers::default().admit_procedural_curve(ctx, &mut ir, &owner, procedural)
    })
    .expect("valid test setup or admitted service result");
    assert_eq!(ir.model.procedural_curves.len(), 1);
}

#[test]
fn helix_construction_lengths_are_in_millimeters_at_attachment() {
    let curve_id = CurveId::mint("creo:depdb:curve#1").expect("identity grammar");
    let mut ir = CadIr::empty();
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_curve(
            ctx,
            &mut ir,
            Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: None,
            },
        )
    })
    .expect("curve admission");
    crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_procedural_curve(
            ctx,
            &mut ir,
            &curve_id,
            ProceduralCurve::new(
                ProceduralCurveId::mint("creo:depdb:helix#1").expect("identity grammar"),
                ProceduralCurveDefinition::Helix(
                    HelixCurveConstruction::try_new(
                        [0.0, 1.0],
                        HelixFrame {
                            center: Point3::new(1.0, 0.0, 0.0),
                            major: Vector3::new(1.0, 0.0, 0.0),
                            minor: Vector3::new(0.0, 1.0, 0.0),
                            pitch: Vector3::new(0.0, 0.0, 1.0),
                            axis: Vector3::new(0.0, 0.0, 1.0),
                        },
                        0.0,
                        None,
                    )
                    .expect("valid helix"),
                ),
            ),
        )
    })
    .expect("helix attachment");
    let ProceduralCurveDefinition::Helix(helix) = ir.model.procedural_curves[0].definition()
    else {
        panic!("helix construction changed family");
    };
    assert_eq!(helix.center().get(), Point3::new(25.4, 0.0, 0.0));
    assert_eq!(helix.pitch().get(), Vector3::new(0.0, 0.0, 25.4));
}

#[test]
fn topological_point_is_in_millimeters_at_admission() {
    let mut ir = CadIr::empty();
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let point = Point::new(
        cadmpeg_ir::ids::PointId::mint("creo:visibgeom:point#1").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
            .expect("finite source point"),
        None,
    );
    crate::decode::with_test_decode_ctx(|ctx| source_carriers.admit_point(ctx, &mut ir, point))
        .expect("point admission");
    assert_eq!(
        ir.model.points[0].position().get(),
        Point3::new(25.4, 0.0, 0.0)
    );
}

#[test]
fn bounded_line_edge_range_is_in_millimeters_at_admission() {
    let mut ir = CadIr::empty();
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let curve_id = CurveId::mint("creo:visibgeom:curve#1").expect("identity grammar");
    crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_curve(
            ctx,
            &mut ir,
            Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("source line"),
                )),
                source_object: None,
            },
        )
    })
    .expect("curve admission");
    let vertex =
        cadmpeg_ir::ids::VertexId::mint("creo:visibgeom:vertex#1").expect("identity grammar");
    let edge = Edge {
        id: cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#1").expect("identity grammar"),
        carrier: EdgeCarrier::new(Some(curve_id), Some([0.0, 2.0])).expect("bounded line"),
        start: vertex.clone(),
        end: vertex,
        tolerance: PositiveReal::new(0.1),
    };
    crate::decode::with_test_decode_ctx(|ctx| source_carriers.admit_edge(ctx, &mut ir, edge))
        .expect("edge admission");
    assert_eq!(
        ir.model.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, 50.8])
    );
    assert_eq!(
        source_carriers.source_edge_parameter_range(&ir.model.edges[0]),
        Some([0.0, 2.0])
    );
    assert_eq!(
        ir.model.edges[0].tolerance.map(PositiveReal::get),
        Some(2.54)
    );
}

fn source_line_for_range_tests() -> (CadIr, SourceUnitCarriers, CurveId) {
    let mut ir = CadIr::empty();
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    let curve_id = CurveId::mint("creo:visibgeom:curve#1").expect("identity grammar");
    crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_curve(
            ctx,
            &mut ir,
            Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("source line"),
                )),
                source_object: None,
            },
        )
    })
    .expect("curve admission");
    (ir, source_carriers, curve_id)
}

#[test]
fn bounded_line_edge_range_overflow_refuses_at_admission() {
    let (mut ir, mut source_carriers, curve_id) = source_line_for_range_tests();
    let vertex =
        cadmpeg_ir::ids::VertexId::mint("creo:visibgeom:vertex#1").expect("identity grammar");
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_edge(
            ctx,
            &mut ir,
            Edge {
                id: cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#1")
                    .expect("identity grammar"),
                carrier: EdgeCarrier::new(Some(curve_id), Some([0.0, f64::MAX]))
                    .expect("finite source range"),
                start: vertex.clone(),
                end: vertex,
                tolerance: None,
            },
        )
    })
    .expect_err("millimeter range overflows");
    assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    assert!(ir.model.edges.is_empty());
}

#[test]
fn coedge_line_use_range_overflow_refuses_at_admission() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let (mut ir, source_carriers, curve_id) = source_line_for_range_tests();
        let coedge_id = cadmpeg_ir::ids::CoedgeId::mint("creo:visibgeom:coedge#1")
            .expect("identity grammar");
        let error = source_carriers
            .admit_coedge(
                ctx,
                &mut ir,
                Coedge {
                    id: coedge_id.clone(),
                    owner_loop: cadmpeg_ir::ids::LoopId::mint("creo:visibgeom:loop#1")
                        .expect("identity grammar"),
                    edge: cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#1")
                        .expect("identity grammar"),
                    radial_next: coedge_id,
                    sense: Sense::Forward,
                    pcurves: Vec::new(),
                    use_curve: Some(CoedgeUseCurve {
                        curve: curve_id,
                        parameter_range: ParameterInterval::try_from([0.0, f64::MAX])
                            .expect("finite source interval"),
                    }),
                },
            )
            .expect_err("millimeter use range overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.coedges.is_empty());
    });
}

#[test]
fn vertex_face_tolerances_and_coedge_line_range_are_in_millimeters_at_admission() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let curve_id = CurveId::mint("creo:visibgeom:curve#1").expect("identity grammar");
        crate::decode::with_test_decode_ctx(|ctx| {
            source_carriers.admit_curve(
                ctx,
                &mut ir,
                Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(1.0, 0.0, 0.0),
                        )
                        .expect("source line"),
                    )),
                    source_object: None,
                },
            )
        })
        .expect("curve admission");
        source_carriers
            .admit_vertex(
                ctx,
                &mut ir,
                Vertex {
                    id: cadmpeg_ir::ids::VertexId::mint("creo:visibgeom:vertex#1")
                        .expect("identity grammar"),
                    point: cadmpeg_ir::ids::PointId::mint("creo:visibgeom:point#1")
                        .expect("identity grammar"),
                    tolerance: PositiveReal::new(0.5),
                },
            )
            .expect("vertex admission");
        source_carriers
            .admit_face(
                ctx,
                &mut ir,
                Face {
                    id: cadmpeg_ir::ids::FaceId::mint("creo:visibgeom:face#1")
                        .expect("identity grammar"),
                    shell: cadmpeg_ir::ids::ShellId::mint("creo:visibgeom:shell#1")
                        .expect("identity grammar"),
                    surface: SurfaceId::mint("creo:visibgeom:surface#1")
                        .expect("identity grammar"),
                    sense: Sense::Forward,
                    loops: FaceLoops::unspecified(Vec::new()),
                    name: None,
                    color: None,
                    tolerance: PositiveReal::new(0.25),
                },
            )
            .expect("face admission");
        let coedge_id = cadmpeg_ir::ids::CoedgeId::mint("creo:visibgeom:coedge#1")
            .expect("identity grammar");
        source_carriers
            .admit_coedge(
                ctx,
                &mut ir,
                Coedge {
                    id: coedge_id.clone(),
                    owner_loop: cadmpeg_ir::ids::LoopId::mint("creo:visibgeom:loop#1")
                        .expect("identity grammar"),
                    edge: cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#1")
                        .expect("identity grammar"),
                    radial_next: coedge_id,
                    sense: Sense::Forward,
                    pcurves: Vec::new(),
                    use_curve: Some(CoedgeUseCurve {
                        curve: curve_id,
                        parameter_range: ParameterInterval::try_from([1.0, 2.0])
                            .expect("bounded source interval"),
                    }),
                },
            )
            .expect("coedge admission");
        assert_eq!(
            ir.model.vertices[0].tolerance.map(PositiveReal::get),
            Some(12.7)
        );
        assert_eq!(
            ir.model.faces[0].tolerance.map(PositiveReal::get),
            Some(6.35)
        );
        assert_eq!(
            ir.model.coedges[0]
                .use_curve
                .as_ref()
                .map(|use_curve| use_curve.parameter_range.endpoints()),
            Some([25.4, 50.8])
        );
    });
}

#[test]
fn topology_tolerance_overflow_refuses_at_admission() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut ir = CadIr::empty();
        let source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = source_carriers
            .admit_vertex(
                ctx,
                &mut ir,
                Vertex {
                    id: cadmpeg_ir::ids::VertexId::mint("creo:visibgeom:vertex#1")
                        .expect("identity grammar"),
                    point: cadmpeg_ir::ids::PointId::mint("creo:visibgeom:point#1")
                        .expect("identity grammar"),
                    tolerance: PositiveReal::new(f64::MAX),
                },
            )
            .expect_err("millimeter tolerance cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.vertices.is_empty());
    });
}

#[test]
fn plane_pcurve_coordinates_are_in_millimeters_at_admission() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let surface_id = SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar");
        crate::decode::with_test_decode_ctx(|ctx| {
            source_carriers.admit_surface(
                ctx,
                &mut ir,
                Surface {
                    id: surface_id.clone(),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(1.0, 0.0, 0.0),
                        )
                        .expect("source plane"),
                    )),
                    source_object: None,
                },
            )
        })
        .expect("surface admission");
        source_carriers
            .admit_pcurve(
                ctx,
                &mut ir,
                cadmpeg_ir::geometry::pcurve::Pcurve {
                    id: cadmpeg_ir::ids::PcurveId::mint("creo:visibgeom:pcurve#1")
                        .expect("identity grammar"),
                    geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                            cadmpeg_ir::math::Point2::new(1.0, 2.0),
                            cadmpeg_ir::math::Point2::new(1.0, 0.0),
                        )
                        .expect("source pcurve"),
                    ),
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                        None, None, None,
                    ),
                },
                &surface_id,
            )
            .expect("pcurve admission");
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line) =
            &ir.model.pcurves[0].geometry
        else {
            panic!("pcurve changed family");
        };
        assert_eq!(
            line.origin().get(),
            cadmpeg_ir::math::Point2::new(25.4, 50.8)
        );
    });
}

#[test]
fn pcurve_without_owning_surface_is_malformed_at_admission() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut ir = CadIr::empty();
        let source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = source_carriers
            .admit_pcurve(
                ctx,
                &mut ir,
                cadmpeg_ir::geometry::pcurve::Pcurve {
                    id: cadmpeg_ir::ids::PcurveId::mint("creo:visibgeom:pcurve#1")
                        .expect("identity grammar"),
                    geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                            cadmpeg_ir::math::Point2::new(1.0, 2.0),
                            cadmpeg_ir::math::Point2::new(1.0, 0.0),
                        )
                        .expect("source pcurve"),
                    ),
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                        None, None, None,
                    ),
                },
                &SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar"),
            )
            .expect_err("pcurve has no owning surface");
        assert!(matches!(error, CodecError::Malformed(_)), "{error}");
        assert!(ir.model.pcurves.is_empty());
    });
}

#[test]
fn plane_pcurve_coordinate_overflow_refuses_at_admission() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let surface_id = SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar");
        crate::decode::with_test_decode_ctx(|ctx| {
            source_carriers.admit_surface(
                ctx,
                &mut ir,
                Surface {
                    id: surface_id.clone(),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(1.0, 0.0, 0.0),
                        )
                        .expect("source plane"),
                    )),
                    source_object: None,
                },
            )
        })
        .expect("surface admission");
        let error = source_carriers
            .admit_pcurve(
                ctx,
                &mut ir,
                cadmpeg_ir::geometry::pcurve::Pcurve {
                    id: cadmpeg_ir::ids::PcurveId::mint("creo:visibgeom:pcurve#1")
                        .expect("identity grammar"),
                    geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                            cadmpeg_ir::math::Point2::new(f64::MAX, 0.0),
                            cadmpeg_ir::math::Point2::new(0.0, 1.0),
                        )
                        .expect("finite source pcurve"),
                    ),
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                        None, None, None,
                    ),
                },
                &surface_id,
            )
            .expect_err("scaled pcurve coordinate overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.pcurves.is_empty());
    });
}

#[test]
fn pcurve_normalization_failure_text_refuses_below_retained_limit() {
    let make_pcurve = || cadmpeg_ir::geometry::pcurve::Pcurve {
        id: cadmpeg_ir::ids::PcurveId::mint("creo:visibgeom:pcurve#1")
            .expect("identity grammar"),
        geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(f64::MAX, 0.0),
                cadmpeg_ir::math::Point2::new(0.0, 1.0),
            )
            .expect("finite pcurve"),
        ),
        metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(None, None, None),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = SourceUnitCarriers::push_pcurve(
        &ctx,
        &mut CadIr::empty(),
        make_pcurve(),
        Some([25.4, 25.4]),
    )
    .expect_err("text refused before formatting");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo normalized pcurve refusal text"));

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = SourceUnitCarriers::push_pcurve(
        &ctx,
        &mut CadIr::empty(),
        make_pcurve(),
        Some([25.4, 25.4]),
    )
    .expect_err("overflow remains not implemented");
    assert_eq!(error.to_string(), "not implemented yet: Creo pcurve cannot be represented after unit normalization with scales [25.4, 25.4]");
}
#[test]
fn source_sketch_nurbs_copy_refuses_knots_and_poles_separately() {
    for rational in [false, true] {
        let geometry = SketchGeometry::nurbs(
            cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![cadmpeg_ir::math::Point2::new(0.0, 0.0); 2],
                rational.then(|| vec![1.0, 2.0]),
                false,
            )
            .expect("fixture pcurve construction admission")
            .expect("curve"),
        );
        for cap in [3, 5] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            assert!(
                matches!(geometry.try_clone_for_decode(&ctx, "creo source sketch geometry copy"),
                Err(CodecError::ResourceLimit(resource)) if resource.operation == "creo source sketch geometry copy")
            );
        }
        assert_eq!(
            crate::decode::with_test_decode_ctx(
                |ctx| geometry.try_clone_for_decode(ctx, "creo source sketch geometry copy")
            )
            .expect("service"),
            geometry
        );
    }
}


#[test]
fn source_sketch_input_traversals_refuse_work() {
    crate::test_support::assert_work_boundaries(&["creo source sketch entity traversal"], |ctx| {
        SourceUnitCarriers::default().admit_sketch_entities(ctx, &mut CadIr::empty(), vec![source_sketch_line(1.0)])
    });
    crate::test_support::assert_work_boundaries(&["creo source sketch constraint traversal"], |ctx| {
        SourceUnitCarriers::default().admit_sketch_constraints(ctx, &mut CadIr::empty(), vec![source_distance_constraint(2.0)])
    });
}

#[test]
fn source_pcurve_surface_search_stops_at_first_match() {
    let target = SurfaceId::mint("creo:test:surface#1").expect("source ID");
    let surface = Surface { id: target.clone(), geometry: admission_plane(), source_object: None };
    let short = vec![surface.clone()];
    let mut long = short.clone();
    long.extend(std::iter::repeat_n(surface, 64));
    let run = |ctx: &DecodeContext<'_>, surfaces: &[Surface]| {
        let mut ir = CadIr::empty();
        ir.model.surfaces = surfaces.to_vec();
        SourceUnitCarriers::default().admit_pcurve(ctx, &mut ir, admission_pcurve(), &target)
    };
    crate::test_support::assert_work_boundaries(&["creo source pcurve surface search", "creo source pcurve surface ID comparison"], |ctx| run(ctx, &short));
    let refusal = |surfaces: &[Surface]| crate::test_support::last_refusal_at(b"", ResourceDimension::WorkUnits,
        "creo source pcurve surface search", |ctx| run(ctx, surfaces));
    let CodecError::ResourceLimit(short_refusal) = refusal(&short) else { panic!("work refusal") };
    let CodecError::ResourceLimit(long_refusal) = refusal(&long) else { panic!("work refusal") };
    assert_eq!(short_refusal, long_refusal);
    crate::decode::with_test_decode_ctx(|ctx| run(ctx, &long)).expect("first source surface admits pcurve");
}
