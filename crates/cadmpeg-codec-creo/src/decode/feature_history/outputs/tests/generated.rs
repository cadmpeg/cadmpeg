// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureOperation, GeneratedEdgeRef, GeneratedFaceRef,
};

use super::{
    feature_output_bodies, generated_edge_output_bodies, generated_input_output_bodies, BTreeMap,
    Body, BodyId, BodyKind, CadIr, DecodeArena, DecodeContext, DecodePolicy, Face, FaceId, LoopId,
    Region, RegionId, ResourceDimension, Sense, Shell, ShellId, SurfaceId,
};

#[test]
fn generated_input_lookup_refuses_before_scoped_text() {
    let ir = CadIr::empty();
    let scan = crate::test_support::empty_container_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        Some("creo generated input feature lookup"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut history = super::super::FeatureOutputHistory::new(&ctx, &ir)?;
            generated_input_output_bodies(&ctx, &scan, &ir, 40, &mut history).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = generated_input_output_bodies(
        &ctx,
        &scan,
        &ir,
        40,
        &mut super::super::FeatureOutputHistory::new(&ctx, &ir).expect("history storage"),
    )
    .map(|_| ())
    .expect_err("lookup needs scoped text");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo generated input feature lookup")
    );
}

#[test]
fn generated_surface_body_refuses_before_feature_output_row() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 10,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let shell_id = ShellId::mint("creo:test:shell#1").expect("identity grammar");
    let region_id = RegionId::mint("creo:test:region#1").expect("identity grammar");
    let face_id = FaceId::mint("creo:test:face#1").expect("identity grammar");
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: BodyId::mint("creo:test:body#1").expect("identity grammar"),
        shells: vec![shell_id.clone()],
    });
    ir.model.shells.push(Shell::with_face(
        shell_id.clone(),
        region_id,
        face_id.clone(),
    ));
    ir.model.faces.push(Face {
        id: face_id,
        shell: shell_id,
        surface: SurfaceId::mint("creo:visibgeom:surface#7").expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo feature output bodies"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            feature_output_bodies(&ctx, &scan, &ir, 10).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_output_bodies(&ctx, &scan, &ir, 10)
        .expect_err("visited node and body row need two collection items");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature output bodies")
    );
}

#[test]
fn generated_edge_body_refuses_before_merge_row() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#50:body").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let edges = [GeneratedEdgeRef::new(
        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50").expect("identity grammar"),
        "curve#7".to_string(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection reference admission")
    .expect("valid generated edge")];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo generated edge output bodies"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut history = super::super::FeatureOutputHistory::new(&ctx, &ir)?;
            generated_edge_output_bodies(&ctx, &scan, &ir, &edges, &mut history).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = generated_edge_output_bodies(
        &ctx,
        &scan,
        &ir,
        &edges,
        &mut super::super::FeatureOutputHistory::new(&ctx, &ir).expect("history storage"),
    )
    .map(|_| ())
    .expect_err("visited producer and its body use the two admitted rows");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo generated edge output bodies")
    );
}

#[test]
fn generated_input_body_refuses_before_merge_row() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#50:body").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#10")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces: FaceSelection::generated(
                    vec![GeneratedFaceRef::new(
                        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50")
                            .expect("identity grammar"),
                        "surface#7".to_string(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("selection reference admission")
                    .expect("valid generated face")],
                    "creo:test:face#7".to_string(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .expect("valid face selection"),
                thickness: None,
                side: None,
            }),
        ),
        native_ref: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo generated input output bodies"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut history = super::super::FeatureOutputHistory::new(&ctx, &ir)?;
            generated_input_output_bodies(&ctx, &scan, &ir, 10, &mut history).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = generated_input_output_bodies(
        &ctx,
        &scan,
        &ir,
        10,
        &mut super::super::FeatureOutputHistory::new(&ctx, &ir).expect("history storage"),
    )
    .map(|_| ())
    .expect_err("generated dependency, visited producer, and its body use three rows");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo generated input output bodies")
    );
}

#[test]
fn reconciled_output_refuses_before_update_row() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#40")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo reconciled output update rows"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let mut ir = ir.clone();
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            super::super::super::dependencies::reconcile_feature_links(
                &ctx,
                &scan,
                &mut ir,
                &BTreeMap::new(),
            )
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = super::super::super::dependencies::reconcile_feature_links(
        &ctx,
        &scan,
        &mut ir,
        &BTreeMap::new(),
    )
    .expect_err("visiting node and update row need two collection items");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo reconciled output update rows")
    );
}

#[test]
fn generated_edge_outputs_follow_producer_history_before_ir_feature_insertion() {
    let feature_row = |feature_id| crate::feature::rows::FeatureRow {
        feature_id,
        root_schema_class: None,
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    };
    let curve_row = |id, feature_id| crate::curve::CurveTopologyRow {
        id,
        type_byte: 8,
        feature_id,
        directions: [1, 0xf6],
        faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
        next_edges: [id, id],
        offset: 0,
    };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .rows
        .extend([feature_row(50), feature_row(70)]);
    scan.features.affected_ids.extend([
        crate::feature::rows::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::rows::AffectedIdKind::Edges,
            ids: vec![45],
            offset: 0,
        },
        crate::feature::rows::FeatureAffectedIds {
            feature_id: 50,
            kind: crate::feature::rows::AffectedIdKind::Edges,
            ids: vec![60],
            offset: 0,
        },
    ]);
    scan.curves
        .topology_rows
        .extend([curve_row(45, 50), curve_row(60, 70)]);

    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#70:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(ctx, &scan, &ir, 10))
            .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#70:body".to_string()).expect("identity grammar")]
    );
}

#[test]
fn duplicate_generated_output_candidates_copy_one_final_body_id() {
    let feature_row = crate::feature::rows::FeatureRow {
        feature_id: 50,
        root_schema_class: None,
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    };
    let curve_row = crate::curve::CurveTopologyRow {
        id: 45,
        type_byte: 8,
        feature_id: 50,
        directions: [1, 0xf6],
        faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
        next_edges: [45, 45],
        offset: 0,
    };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(feature_row);
    scan.features.affected_ids.push(crate::feature::rows::FeatureAffectedIds {
        feature_id: 10,
        kind: crate::feature::rows::AffectedIdKind::Edges,
        ids: vec![45],
        offset: 0,
    });
    scan.curves.topology_rows.push(curve_row);

    let body_id = BodyId::mint("creo:feature:extrusion#50:body")
        .expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#10")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces: FaceSelection::generated(
                    vec![GeneratedFaceRef::new(
                        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50")
                            .expect("identity grammar"),
                        "surface#7".to_string(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("selection reference admission")
                    .expect("valid generated face")],
                    "creo:generated-face#7".to_string(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .expect("valid generated face selection"),
                thickness: None,
                side: None,
            }),
        ),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50")
            .expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    });

    crate::decode::with_test_decode_ctx(|ctx| {
        let Some(cadmpeg_ir::features::EdgeSelection::Generated { edges, .. }) =
            super::super::feature_edge_selection(ctx, &scan, &ir, 10)?
        else {
            panic!("the edge row must resolve to a generated producer reference");
        };
        let mut history = super::super::FeatureOutputHistory::new(ctx, &ir)?;
        let edge_outputs = generated_edge_output_bodies(ctx, &scan, &ir, &edges, &mut history)?;
        assert_eq!(edge_outputs.bodies, vec![&body_id]);
        let input_outputs = generated_input_output_bodies(ctx, &scan, &ir, 10, &mut history)?;
        assert_eq!(input_outputs.bodies, vec![&body_id]);
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("both generated paths select the same producer body");

    let cap_below_output_copy = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo feature output body IDs"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root is admitted");
            feature_output_bodies(&ctx, &scan, &ir, 10).map(|_| ())
        },
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = cap_below_output_copy
        .checked_add(1)
        .expect("retained-byte boundary");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    assert_eq!(
        feature_output_bodies(&ctx, &scan, &ir, 10)
            .expect("duplicate candidates need one final body ID copy"),
        vec![body_id]
    );
}

#[test]
fn generated_face_outputs_follow_producer_history_after_feature_insertion() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#10")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces: FaceSelection::generated(
                    vec![GeneratedFaceRef::new(
                        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50")
                            .expect("identity grammar"),
                        "surface#7".to_string(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("selection reference admission")
                    .expect("valid test fixture")],
                    "creo:generated-face#7".to_string(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .expect("valid test fixture"),
                thickness: None,
                side: None,
            }),
        ),
        native_ref: None,
    });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(ctx, &scan, &ir, 10))
            .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );

    crate::decode::with_test_decode_ctx(|ctx| {
        super::super::super::dependencies::reconcile_feature_links(
            ctx,
            &scan,
            &mut ir,
            &BTreeMap::new(),
        )
    })
    .expect("the fixture feature links reconcile");
    assert_eq!(
        *ir.model.features[0].evaluation.outputs(),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );
}

#[test]
fn generated_result_faces_are_outputs_alongside_generated_input_bodies() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 10,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar"),
        kind: BodyKind::Sheet,
        regions: vec![
            RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar")
        ],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar"),
        body: BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar"),
        shells: vec![
            ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar")
        ],
    });
    ir.model.shells.push(Shell::with_face(
        ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar"),
        RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar"),
        FaceId::mint("creo:generated:face#7".to_string()).expect("identity grammar"),
    ));
    ir.model.faces.push(Face {
        id: FaceId::mint("creo:generated:face#7".to_string()).expect("identity grammar"),
        shell: ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar"),
        surface: SurfaceId::mint("creo:visibgeom:surface#7".to_string()).expect("identity grammar"),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![LoopId::mint(
            "creo:generated:loop#7".to_string(),
        )
        .expect("identity grammar")]),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#10")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces: FaceSelection::generated(
                    vec![GeneratedFaceRef::new(
                        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50")
                            .expect("identity grammar"),
                        "surface#7".to_string(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("selection reference admission")
                    .expect("valid test fixture")],
                    "creo:generated-face#7".to_string(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .expect("valid test fixture"),
                thickness: None,
                side: None,
            }),
        ),
        native_ref: None,
    });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(ctx, &scan, &ir, 10))
            .expect("service profile admits output bodies"),
        vec![
            BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar"),
            BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar"),
        ]
    );

    let mut existing_surface_output = ir.clone();
    let sweep_body = BodyId::mint("creo:feature:extrusion#10:body").expect("identity grammar");
    existing_surface_output.model.bodies[1].id = sweep_body.clone();
    existing_surface_output.model.regions[0].body = sweep_body.clone();
    let output_bodies =
        crate::test_support::assert_work_boundaries(&["creo feature output body lookup"], |ctx| {
            feature_output_bodies(ctx, &scan, &existing_surface_output, 10)
        });
    assert_eq!(
        output_bodies,
        vec![
            sweep_body,
            BodyId::mint("creo:feature:extrusion#50:body").expect("identity grammar"),
        ]
    );

    let mut duplicate_shell = ir.clone();
    duplicate_shell.model.shells.push(
        Shell::new(
            ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar"),
            RegionId::mint("creo:ambiguous:region#10".to_string()).expect("identity grammar"),
            ir.model.shells[0].faces().to_vec(),
            Vec::new(),
            Vec::new(),
        )
        .expect("valid test fixture"),
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(
            ctx,
            &scan,
            &duplicate_shell,
            10
        ))
        .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );

    let mut duplicate_region = ir.clone();
    duplicate_region.model.regions.push(Region {
        id: RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar"),
        body: BodyId::mint("creo:ambiguous:body#10".to_string()).expect("identity grammar"),
        shells: Vec::new(),
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(
            ctx,
            &scan,
            &duplicate_region,
            10
        ))
        .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );

    let mut duplicate_feature = ir.clone();
    let feature = duplicate_feature.model.features[0].clone();
    duplicate_feature.model.features.push(feature);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(
            ctx,
            &scan,
            &duplicate_feature,
            10
        ))
        .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar")]
    );
}

#[test]
fn generated_input_chain_membership_refuses_work_without_surface_route() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    // A preexisting evaluated output makes the generated-input chain search nonempty.
    let initial_output = BodyId::mint("creo:feature:extrusion#10:body").expect("identity grammar");
    ir.model.bodies.push(Body {
        id: initial_output.clone(),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let body = BodyId::mint("creo:feature:extrusion#50:body").expect("identity grammar");
    ir.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    let fixture_ctx = cadmpeg_test_support::service_decode_context();
    let producer = GeneratedFaceRef::new(
        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50").expect("identity grammar"),
        "surface#7".to_string(),
        &fixture_ctx,
    )
    .expect("selection reference admission")
    .expect("valid generated reference");
    let faces = FaceSelection::generated(
        vec![producer],
        "creo:generated-face#7".to_string(),
        &fixture_ctx,
    )
    .expect("selection reference admission")
    .expect("valid generated face selection");
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#10")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces,
                thickness: None,
                side: None,
            }),
        ),
        native_ref: None,
    });

    assert!(scan.surfaces.rows.is_empty());
    assert!(ir.model.faces.is_empty());
    let outputs =
        crate::test_support::assert_work_boundaries(&["creo feature output body lookup"], |ctx| {
            feature_output_bodies(ctx, &scan, &ir, 10)
        });
    assert_eq!(outputs, vec![initial_output, body]);
}

#[test]
fn generated_input_feature_scan_refuses_before_identity_comparison() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#40")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    });
    let outputs = crate::test_support::assert_work_boundaries(
        &["creo generated input feature lookup traversal"],
        |ctx| {
            generated_input_output_bodies(
                ctx,
                &scan,
                &ir,
                40,
                &mut super::super::FeatureOutputHistory::new(ctx, &ir)?,
            )
            .map(|outputs| outputs.bodies.is_empty())
        },
    );
    assert!(outputs);
}
