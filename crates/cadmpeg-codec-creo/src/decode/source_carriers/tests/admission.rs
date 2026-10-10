// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn source_sketch_geometry_refuses_nurbs_copy_limit() {
    let geometry = SketchGeometry::nurbs(
        cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 1.0),
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
            ],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .expect("source NURBS"),
    );
    let copy = crate::test_support::assert_refusal_order(ResourceDimension::CollectionItems,
        &["creo source sketch geometry copy", "creo source sketch geometry copy"], |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            geometry.try_clone_for_decode(&ctx, "creo source sketch geometry copy")
        });
    assert_eq!(copy, geometry);
}

#[test]
fn source_sketch_geometry_refuses_text_and_native_retained_limits() {
    let text = SketchGeometry::try_from(SketchGeometryDefinition::Text {
        text: cadmpeg_core::text::NonBlankString::try_from("cadmpeg").expect("text"),
        font_family: cadmpeg_core::text::NonBlankString::try_from("sans").expect("font"),
        font_weight: cadmpeg_ir::sketches::SketchFontWeight::Regular,
        height: cadmpeg_ir::scalar::Length::new(4.0).expect("height"),
        width_factor: None,
        placement: None,
        horizontal_alignment: None,
        vertical_alignment: None,
    })
    .expect("source text");
    let native = SketchGeometry::native(
        cadmpeg_core::text::NonBlankString::try_from("native").expect("native kind"),
    );
    for (geometry, operations) in [
        (&text, ["creo source sketch geometry copy", "creo source sketch geometry copy"].as_slice()),
        (&native, ["creo source sketch geometry copy"].as_slice()),
    ] {
        let copy = crate::test_support::assert_refusal_order(ResourceDimension::RetainedBytes,
            operations, |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                geometry.try_clone_for_decode(&ctx, "creo source sketch geometry copy")
            });
        assert_eq!(&copy, geometry);
    }
}

#[test]
fn source_sketch_geometry_refuses_external_reference_copies() {
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::ExternalReference {
        document: Some("doc".to_owned()),
        object: cadmpeg_core::text::NonBlankString::try_from("part").expect("object"),
        subelements: vec!["face".to_owned(), "edge".to_owned()],
    })
    .expect("external source geometry");
    for (dimension, operations) in [
        (ResourceDimension::CollectionItems, ["creo source sketch geometry copy"].as_slice()),
        (ResourceDimension::RetainedBytes, ["creo source sketch geometry copy"; 5].as_slice()),
    ] {
        let copy = crate::test_support::assert_refusal_order(dimension, operations, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            geometry.try_clone_for_decode(&ctx, "creo source sketch geometry copy")
        });
        assert_eq!(copy, geometry);
    }
}

#[test]
fn replacement_curve_refuses_source_node_and_id_copy_limits() {
    let mut curve = Curve {
        id: CurveId::mint("creo:test:replacement-curve#1").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    };
    let geometry = curve.geometry.clone();
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "creo replacement source curve nodes",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "creo replacement source curve IDs",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            SourceUnitCarriers::for_decode(ctx, None).replace_curve_geometry(
                ctx,
                &mut curve.clone(),
                geometry.clone(),
            )
        });
        assert!(
            matches!(error, CodecError::ResourceLimit(resource) if resource.operation == operation),
            "{error:?}"
        );
    }
    let mut carriers = SourceUnitCarriers::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.replace_curve_geometry(ctx, &mut curve, geometry.clone())
    })
    .expect("service replacement");
    assert_eq!(
        carriers
            .curve_geometry(&curve)
            .expect("source carrier lookup"),
        &geometry
    );
}

#[test]
fn replacement_curve_refuses_materialized_geometry_copy() {
    let id = CurveId::mint("creo:test:replacement-curve#1")
        .expect("valid test setup or admitted service result");
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
        record: Some(
            cadmpeg_ir::ids::UnknownId::mint("creo:test:unknown#1")
                .expect("valid test setup or admitted service result"),
        ),
    });
    let mut curve = Curve {
        id: id.clone(),
        geometry: geometry.clone(),
        source_object: None,
    };
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "creo replacement source curve geometry",
        |ctx| {
            SourceUnitCarriers::for_decode(ctx, None).replace_curve_geometry(
                ctx,
                &mut curve.clone(),
                geometry.clone(),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(resource) if resource.operation == "creo replacement source curve geometry"),
        "{error:?}"
    );
    let mut carriers = SourceUnitCarriers::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.replace_curve_geometry(ctx, &mut curve, geometry.clone())
    })
    .expect("valid test setup or admitted service result");
    assert_eq!(
        carriers
            .curve_geometry(&curve)
            .expect("source carrier lookup"),
        &geometry
    );
}

#[test]
fn replacement_surface_refuses_source_node_and_id_copy_limits() {
    let mut surface = Surface {
        id: SurfaceId::mint("creo:test:replacement-surface#1").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    };
    let geometry = surface.geometry.clone();
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "creo replacement source surface nodes",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "creo replacement source surface IDs",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            SourceUnitCarriers::for_decode(ctx, None).replace_surface_geometry(
                ctx,
                &mut surface.clone(),
                geometry.clone(),
            )
        });
        assert!(
            matches!(error, CodecError::ResourceLimit(resource) if resource.operation == operation),
            "{error:?}"
        );
    }
    let mut carriers = SourceUnitCarriers::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.replace_surface_geometry(ctx, &mut surface, geometry.clone())
    })
    .expect("service replacement");
    assert_eq!(
        carriers
            .surface_geometry(&surface)
            .expect("source carrier lookup"),
        &geometry
    );
}

#[test]
fn replacement_surface_refuses_materialized_geometry_copy() {
    let id = SurfaceId::mint("creo:test:replacement-surface#1")
        .expect("valid test setup or admitted service result");
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
        record: Some(
            cadmpeg_ir::ids::UnknownId::mint("creo:test:unknown#1")
                .expect("valid test setup or admitted service result"),
        ),
    });
    let mut surface = Surface {
        id: id.clone(),
        geometry: geometry.clone(),
        source_object: None,
    };
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "creo replacement source surface geometry",
        |ctx| {
            SourceUnitCarriers::for_decode(ctx, None).replace_surface_geometry(
                ctx,
                &mut surface.clone(),
                geometry.clone(),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(resource) if resource.operation == "creo replacement source surface geometry"),
        "{error:?}"
    );
    let mut carriers = SourceUnitCarriers::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.replace_surface_geometry(ctx, &mut surface, geometry.clone())
    })
    .expect("valid test setup or admitted service result");
    assert_eq!(
        carriers
            .surface_geometry(&surface)
            .expect("source carrier lookup"),
        &geometry
    );
}

#[test]
fn source_curve_admission_refuses_each_outer_boundary() {
    let curve = Curve {
        id: CurveId::mint("creo:test:source-curve#1").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    };
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "creo source curve nodes",
        ),
        (ResourceDimension::CollectionItems, "creo model curves"),
        (
            ResourceDimension::MaterializedBytes,
            "creo source curve IDs",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            SourceUnitCarriers::for_decode(ctx, None).admit_curve(
                ctx,
                &mut CadIr::empty(),
                curve.clone(),
            )
        });
        assert!(
            matches!(error, CodecError::ResourceLimit(resource) if resource.operation == operation),
            "{error:?}"
        );
    }
    let mut ir = CadIr::empty();
    let mut carriers = SourceUnitCarriers::default();
    crate::decode::with_test_decode_ctx(|ctx| carriers.admit_curve(ctx, &mut ir, curve.clone()))
        .expect("service curve admission");
    assert_eq!(ir.model.curves, vec![curve]);
    assert_eq!(
        carriers
            .curve_geometry(&ir.model.curves[0])
            .expect("source carrier lookup"),
        &ir.model.curves[0].geometry
    );
}

#[test]
fn source_curve_admission_refuses_materialized_geometry_copy() {
    let id = CurveId::mint("creo:test:source-curve#1")
        .expect("valid test setup or admitted service result");
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
        record: Some(
            cadmpeg_ir::ids::UnknownId::mint("creo:test:unknown#1")
                .expect("valid test setup or admitted service result"),
        ),
    });
    let curve = Curve {
        id: id.clone(),
        geometry: geometry.clone(),
        source_object: None,
    };
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "creo source curve geometry",
        |ctx| {
            SourceUnitCarriers::for_decode(ctx, None).admit_curve(
                ctx,
                &mut CadIr::empty(),
                curve.clone(),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(resource) if resource.operation == "creo source curve geometry"),
        "{error:?}"
    );
    let mut carriers = SourceUnitCarriers::default();
    let mut ir = CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| carriers.admit_curve(ctx, &mut ir, curve.clone()))
        .expect("valid test setup or admitted service result");
    assert_eq!(
        carriers
            .curve_geometry(&ir.model.curves[0])
            .expect("source carrier lookup"),
        &geometry
    );
}

#[test]
fn source_surface_admission_refuses_each_outer_boundary() {
    let surface = Surface {
        id: SurfaceId::mint("creo:test:source-surface#1").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    };
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "creo source surface nodes",
        ),
        (ResourceDimension::CollectionItems, "creo model surfaces"),
        (
            ResourceDimension::MaterializedBytes,
            "creo source surface IDs",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            SourceUnitCarriers::for_decode(ctx, None).admit_surface(
                ctx,
                &mut CadIr::empty(),
                surface.clone(),
            )
        });
        assert!(
            matches!(error, CodecError::ResourceLimit(resource) if resource.operation == operation),
            "{error:?}"
        );
    }
    let mut ir = CadIr::empty();
    let mut carriers = SourceUnitCarriers::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_surface(ctx, &mut ir, surface.clone())
    })
    .expect("service surface admission");
    assert_eq!(ir.model.surfaces, vec![surface]);
    assert_eq!(
        carriers
            .surface_geometry(&ir.model.surfaces[0])
            .expect("source carrier lookup"),
        &ir.model.surfaces[0].geometry
    );
}

#[test]
fn source_surface_admission_refuses_materialized_geometry_copy() {
    let id = SurfaceId::mint("creo:test:source-surface#1")
        .expect("valid test setup or admitted service result");
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
        record: Some(
            cadmpeg_ir::ids::UnknownId::mint("creo:test:unknown#1")
                .expect("valid test setup or admitted service result"),
        ),
    });
    let surface = Surface {
        id: id.clone(),
        geometry: geometry.clone(),
        source_object: None,
    };
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "creo source surface geometry",
        |ctx| {
            SourceUnitCarriers::for_decode(ctx, None).admit_surface(
                ctx,
                &mut CadIr::empty(),
                surface.clone(),
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(resource) if resource.operation == "creo source surface geometry"),
        "{error:?}"
    );
    let mut carriers = SourceUnitCarriers::default();
    let mut ir = CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| {
        carriers.admit_surface(ctx, &mut ir, surface.clone())
    })
    .expect("valid test setup or admitted service result");
    assert_eq!(
        carriers
            .surface_geometry(&ir.model.surfaces[0])
            .expect("source carrier lookup"),
        &geometry
    );
}

#[test]
fn feature_admission_refuses_before_model_vector_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut ir = CadIr::empty();
    let error = SourceUnitCarriers::default()
        .admit_feature(
            &ctx,
            &mut ir,
            source_feature(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            )),
        )
        .expect_err("one feature needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model features"));
    assert!(ir.model.features.is_empty());
}

#[test]
fn parameter_admission_refuses_before_model_vector_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut ir = CadIr::empty();
    let error = SourceUnitCarriers::default()
        .admit_parameter(&ctx, &mut ir, source_length_parameter(2.0))
        .expect_err("one parameter needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model parameters"));
    assert!(ir.model.parameters.is_empty());
}

#[test]
fn point_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let point = Point::new(
        cadmpeg_ir::ids::PointId::mint("creo:test:point#0").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite point"),
        None,
    );
    let error =
        zero_collection_ctx(|ctx| SourceUnitCarriers::default().admit_point(ctx, &mut ir, point))
            .expect_err("one point needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model points"));
    assert!(ir.model.points.is_empty());
}

#[test]
fn vertex_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let vertex = Vertex {
        id: cadmpeg_ir::ids::VertexId::mint("creo:test:vertex#0").expect("identity grammar"),
        point: cadmpeg_ir::ids::PointId::mint("creo:test:point#0").expect("identity grammar"),
        tolerance: None,
    };
    let error =
        zero_collection_ctx(|ctx| SourceUnitCarriers::default().admit_vertex(ctx, &mut ir, vertex))
            .expect_err("one vertex needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model vertices"));
    assert!(ir.model.vertices.is_empty());
}

#[test]
fn face_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let face = Face {
        id: cadmpeg_ir::ids::FaceId::mint("creo:test:face#0").expect("identity grammar"),
        shell: cadmpeg_ir::ids::ShellId::mint("creo:test:shell#0").expect("identity grammar"),
        surface: SurfaceId::mint("creo:test:surface#0").expect("identity grammar"),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };
    let error =
        zero_collection_ctx(|ctx| SourceUnitCarriers::default().admit_face(ctx, &mut ir, face))
            .expect_err("one face needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model faces"));
    assert!(ir.model.faces.is_empty());
}

#[test]
fn coedge_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let id = cadmpeg_ir::ids::CoedgeId::mint("creo:test:coedge#0").expect("identity grammar");
    let coedge = Coedge {
        id: id.clone(),
        owner_loop: cadmpeg_ir::ids::LoopId::mint("creo:test:loop#0").expect("identity grammar"),
        edge: cadmpeg_ir::ids::EdgeId::mint("creo:test:edge#0").expect("identity grammar"),
        radial_next: id,
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    };
    let error =
        zero_collection_ctx(|ctx| SourceUnitCarriers::default().admit_coedge(ctx, &mut ir, coedge))
            .expect_err("one coedge needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model coedges"));
    assert!(ir.model.coedges.is_empty());
}

#[test]
fn pcurve_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let surface_id = SurfaceId::mint("creo:test:surface#0").expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: admission_plane(),
        source_object: None,
    });
    let error = zero_collection_ctx(|ctx| {
        SourceUnitCarriers::default().admit_pcurve(ctx, &mut ir, admission_pcurve(), &surface_id)
    })
    .expect_err("one pcurve needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model pcurves"));
    assert!(ir.model.pcurves.is_empty());
}

#[test]
fn source_surface_pcurve_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let error = zero_collection_ctx(|ctx| {
        SourceUnitCarriers::default().admit_pcurve_with_source_surface(
            ctx,
            &mut ir,
            admission_pcurve(),
            &admission_plane(),
        )
    })
    .expect_err("one source-surface pcurve needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model pcurves"));
    assert!(ir.model.pcurves.is_empty());
}

#[test]
fn edge_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let error = zero_collection_ctx(|ctx| {
        SourceUnitCarriers::default().admit_edge(ctx, &mut ir, admission_edge(None))
    })
    .expect_err("one edge needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model edges"));
    assert!(ir.model.edges.is_empty());
}

#[test]
fn bounded_edge_admission_refuses_before_source_range_node() {
    let mut ir = CadIr::empty();
    let error = zero_collection_ctx(|ctx| {
        SourceUnitCarriers::default().admit_edge(ctx, &mut ir, admission_edge(Some([0.0, 1.0])))
    })
    .expect_err("one bounded edge needs one source range node");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo source edge range nodes"));
    assert!(ir.model.edges.is_empty());
}

#[test]
fn bounded_edge_admission_refuses_before_source_range_id_copy() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "creo source edge range IDs",
        |ctx| {
            let mut ir = CadIr::empty();
            let result = SourceUnitCarriers::for_decode(ctx, None).admit_edge(
                ctx,
                &mut ir,
                admission_edge(Some([0.0, 1.0])),
            );
            if result.is_err() {
                assert!(ir.model.edges.is_empty());
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo source edge range IDs"));
}

#[test]
fn body_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let error = zero_collection_ctx(|ctx| {
        SourceUnitCarriers::default().admit_body(
            ctx,
            &mut ir,
            Body {
                id: cadmpeg_ir::ids::BodyId::mint("creo:test:body#0").expect("identity grammar"),
                kind: BodyKind::Solid,
                regions: Vec::new(),
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        )
    })
    .expect_err("one body needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model bodies"));
    assert!(ir.model.bodies.is_empty());
}

#[test]
fn occurrence_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let error = zero_collection_ctx(|ctx| {
        SourceUnitCarriers::default().admit_occurrence(
            ctx,
            &mut ir,
            source_occurrence(translated_product_transform(0.0), None),
        )
    })
    .expect_err("one occurrence needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model occurrences"));
    assert!(ir.model.occurrences.is_empty());
}

#[test]
fn sketch_admission_refuses_before_model_vector_growth() {
    let mut ir = CadIr::empty();
    let error = zero_collection_ctx(|ctx| {
        SourceUnitCarriers::default().admit_sketch(
            ctx,
            &mut ir,
            source_sketch(Point3::new(0.0, 0.0, 0.0)),
        )
    })
    .expect_err("one sketch needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model sketches"));
    assert!(ir.model.sketches.is_empty());
}

#[test]
fn sketch_constraint_admission_refuses_before_counted_model_rows() {
    let mut ir = CadIr::empty();
    let error = zero_collection_ctx(|ctx| {
        SourceUnitCarriers::default().admit_sketch_constraints(
            ctx,
            &mut ir,
            vec![source_distance_constraint(2.0)],
        )
    })
    .expect_err("one constraint needs one model vector row");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model sketch constraints"));
    assert!(ir.model.sketch_constraints.is_empty());
}
