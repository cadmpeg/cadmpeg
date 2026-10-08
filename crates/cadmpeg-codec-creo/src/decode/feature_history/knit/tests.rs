// SPDX-License-Identifier: Apache-2.0

use super::super::selections::feature_result_edge_ids;
use super::{
    feature_result_surface_ids, feature_result_topology, generated_surface_face_refs,
    knit_class_100_operand_entity_ids, knit_operand_entity_ids, knit_operand_surface_ids,
    knit_surface_feature_definition,
};

fn draft_neutral_plane_selection_with_service(
    scan: &crate::container::ContainerScan<'_>,
    feature_id: u32,
) -> cadmpeg_ir::features::FaceSelection {
    crate::decode::with_test_decode_ctx(|ctx| {
        super::draft_neutral_plane_selection(ctx, scan, feature_id)
    })
    .expect("service profile admits draft neutral plane selection")
}

fn feature_surface_transitions_with_service(
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Option<Vec<(u32, u32)>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        super::feature_surface_transitions(
            ctx,
            feature_id,
            tables,
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.to_vec()),
        )
    })
    .expect("service profile admits surface transitions")
}

fn one_knit_scan() -> crate::container::ContainerScan<'static> {
    let entry = |entity_id, class_id, offset| crate::feature::entity::FeatureEntityTableEntry {
        payload: crate::feature::entity::entry_payload(class_id, None, None, None),
        entity_id,
        prefixed: true,
        offset,
        end_offset: offset + 1,
    };
    let table = |feature_id, table_class_id, entry, offset| {
        crate::feature::entity::FeatureEntityTable::new(
            feature_id,
            table_class_id,
            vec![entry],
            &std::collections::BTreeSet::new(),
            offset,
        )
        .with_surface_ids([])
    };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.entity_tables = vec![
        table(97, 67, entry(103, 200, 11), 10),
        table(97, 100, entry(103, 98, 21), 20),
        table(97, 67, entry(98, 0, 31), 30).with_surface_ids([98]),
        table(416, 100, entry(103, 98, 41), 40),
    ];
    scan.surfaces.rows =
        crate::surface::unique_rows::UniqueIdRows::from_rows(vec![crate::surface::SurfaceRow {
            id: 98,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 97,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        }]);
    scan.features.surface_merge_replay_affected_ids.push(
        crate::feature::rows::FeatureSurfaceMergeAffectedIds {
            feature_id: 416,
            geometry_ids: Vec::new(),
            edge_ids: Vec::new(),
            quilt_ids: vec![103],
            geometry_extent: crate::feature::rows::ReplayExtentSource::Explicit,
            edge_extent: crate::feature::rows::ReplayExtentSource::Explicit,
            quilt_extent: crate::feature::rows::ReplayExtentSource::Explicit,
            offset: 100,
        },
    );
    scan
}

fn knit_operand_collection_error(operation: &'static str, route: &str) {
    let scan = one_knit_scan();
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| match route {
            "class" => knit_class_100_operand_entity_ids(ctx, 416, &scan.features.entity_tables)
                .map(|_| ()),
            "quilt" => knit_operand_entity_ids(ctx, &scan, 416).map(|_| ()),
            "surface" => knit_operand_surface_ids(ctx, &scan, 416, &[103]).map(|_| ()),
            _ => panic!("unknown fixture route"),
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn knit_consumer_identity_nodes_refuse_collection_limit() {
    knit_operand_collection_error("creo knit consumer identity nodes", "class");
}

#[test]
fn knit_class_100_operand_ids_refuse_collection_limit() {
    knit_operand_collection_error("creo knit class 100 operand IDs", "class");
}

#[test]
fn knit_quilt_identity_nodes_refuse_collection_limit() {
    knit_operand_collection_error("creo knit quilt identity nodes", "quilt");
}

#[test]
fn knit_quilt_ids_refuse_collection_limit() {
    knit_operand_collection_error("creo knit quilt IDs", "quilt");
}

#[test]
fn knit_surface_identity_nodes_refuse_collection_limit() {
    knit_operand_collection_error("creo knit surface identity nodes", "surface");
}

#[test]
fn knit_surface_ids_refuse_collection_limit() {
    knit_operand_collection_error("creo knit surface IDs", "surface");
}

#[test]
fn knit_native_selection_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = one_knit_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo knit native selection"),
        |cap| {
            let trial_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &trial_arena,
                &trial_policy,
            )
            .expect("root");
            knit_surface_feature_definition(&trial_ctx, &scan, 416)
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = knit_surface_feature_definition(&ctx, &scan, 416)
        .expect_err("one native selection exceeds the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo knit native selection"),
        "{error:?}"
    );
}

#[test]
fn knit_generated_native_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = one_knit_scan();

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo knit generated native selection"),
        |cap| {
            let trial_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &trial_arena,
                &trial_policy,
            )
            .expect("root");
            knit_surface_feature_definition(&trial_ctx, &scan, 416)
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = knit_surface_feature_definition(&ctx, &scan, 416)
        .expect_err("generated native copy exceeds the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo knit generated native selection"),
        "{error:?}"
    );
}

#[test]
fn knit_operand_fixture_generates_a_surface_face_under_service_policy() {
    let scan = one_knit_scan();
    let definition =
        crate::decode::with_test_decode_ctx(|ctx| knit_surface_feature_definition(ctx, &scan, 416))
            .expect("service profile admits the knit surface selection");
    assert!(matches!(
        definition,
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::KnitSurface {
                faces: cadmpeg_ir::features::FaceSelection::Generated { .. },
                ..
            }
        )
    ));
}

fn generated_face_reference_error(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) {
    let row = crate::surface::SurfaceRow {
        id: 201,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let available = std::collections::BTreeSet::from([cadmpeg_ir::features::FeatureId::mint(
        "creo:model:feature#17",
    )
    .expect("fixture feature ID")]);
    let results = std::collections::BTreeMap::from([(17, vec![201])]);
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        generated_surface_face_refs(
            ctx,
            &[201],
            &crate::surface::unique_rows::UniqueIdRows::from_rows(
                std::slice::from_ref(&row).to_vec(),
            ),
            &results,
            &available,
        )
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn generated_surface_feature_id_refuses_retained_limit() {
    generated_face_reference_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo generated surface feature IDs",
    );
}

#[test]
fn generated_surface_local_id_refuses_retained_limit() {
    generated_face_reference_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo generated surface local IDs",
    );
}

#[test]
fn generated_surface_face_references_refuse_collection_limit() {
    generated_face_reference_error(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo generated surface face references",
    );
}

fn one_result_surface() -> (
    Vec<crate::feature::entity::FeatureEntityTable>,
    Vec<crate::surface::SurfaceRow>,
) {
    let table = crate::feature::entity::FeatureEntityTable::new(
        17,
        29,
        vec![crate::feature::entity::dummy_table_entry(201)],
        &std::collections::BTreeSet::from([201]),
        0,
    );
    let row = crate::surface::SurfaceRow {
        id: 201,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    (vec![table], vec![row])
}

fn one_result_edge() -> Vec<crate::curve::CurveTopologyRow> {
    vec![crate::curve::CurveTopologyRow {
        id: 77,
        type_byte: 8,
        feature_id: 17,
        directions: [1, 0xf6],
        faces: [
            std::num::NonZeroU32::new(201),
            std::num::NonZeroU32::new(202),
        ],
        next_edges: [77, 77],
        offset: 0,
    }]
}

fn topology_limit_error(
    face: bool,
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) {
    let (tables, rows) = if face {
        one_result_surface()
    } else {
        (Vec::new(), Vec::new())
    };
    let curve_rows = if face { Vec::new() } else { one_result_edge() };
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        super::feature_result_topology(
            ctx,
            &tables,
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
            &curve_rows,
            17,
        )
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn feature_result_face_local_id_refuses_retained_limit() {
    topology_limit_error(
        true,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo feature result face local IDs",
    );
}

#[test]
fn feature_result_face_members_refuse_collection_limit() {
    topology_limit_error(
        true,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo feature result face members",
    );
}

#[test]
fn feature_result_edge_local_id_refuses_retained_limit() {
    topology_limit_error(
        false,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo feature result edge local IDs",
    );
}

#[test]
fn feature_result_edge_members_refuse_collection_limit() {
    topology_limit_error(
        false,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo feature result edge members",
    );
}

#[test]
fn feature_result_topology_id_refuses_retained_limit() {
    topology_limit_error(
        true,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo feature result topology ID",
    );
}

#[test]
fn feature_result_owner_id_refuses_retained_limit() {
    topology_limit_error(
        true,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo feature result owner ID",
    );
}

#[test]
fn feature_result_distinctness_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (mut tables, mut rows) = one_result_surface();
    tables[0]
        .entries
        .push(crate::feature::entity::dummy_table_entry(202));
    tables[0].mark_surface_id(202);
    rows.push(crate::surface::SurfaceRow {
        id: 202,
        ..rows[0].clone()
    });
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "creo feature result member distinctness",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            super::feature_result_topology(
                &ctx,
                &tables,
                &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
                &[],
                17,
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo feature result member distinctness"),
        "{error:?}"
    );
}

#[test]
fn feature_result_topology_arena_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };

    let (tables, rows) = one_result_surface();
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.entity_tables = tables;
    scan.surfaces.rows = crate::surface::unique_rows::UniqueIdRows::from_rows(rows);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("creo:model:feature#17").expect("fixture identity"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        )),
        native_ref: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo feature result member distinctness"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let mut ir = ir.clone();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root is admitted");
            super::emit_feature_result_topologies(&ctx, &scan, &mut ir).map(|_| ())
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = super::emit_feature_result_topologies(&ctx, &scan, &mut ir)
        .expect_err("one topology arena row exceeds the collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature result member distinctness"),
        "{error:?}"
    );
    assert!(ir.model.feature_result_topologies.is_empty());
    let cadmpeg_core::CodecError::ResourceLimit(first) = error else {
        unreachable!()
    };
    assert!(matches!(ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo model feature result topologies",
        |ctx| {
            let mut candidate = ir.clone();
            let result = super::emit_feature_result_topologies(ctx, &scan, &mut candidate);
            if result.is_err() {
                assert!(candidate.model.feature_result_topologies.is_empty());
            }
            result
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model feature result topologies")
    );
}

fn result_surface_limit_error(by_feature: bool, operation: &'static str) {
    use cadmpeg_core::decode::ResourceDimension;
    let (tables, rows) = one_result_surface();
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| {
            if by_feature {
                super::feature_result_surface_ids_by_feature(
                    ctx,
                    &tables,
                    &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
                )
                .map(|_| ())
            } else {
                super::feature_result_surface_ids(
                    ctx,
                    &tables,
                    &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
                    17,
                )
                .map(|_| ())
            }
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn feature_result_surface_identity_nodes_refuse_collection_limit() {
    result_surface_limit_error(false, "creo feature result surface identity nodes");
}

#[test]
fn feature_result_surface_ids_refuse_collection_limit() {
    result_surface_limit_error(false, "creo feature result surface IDs");
}

#[test]
fn feature_result_surface_map_nodes_refuse_collection_limit() {
    result_surface_limit_error(true, "creo feature result surface map nodes");
}

#[test]
fn feature_result_surface_roster_preserves_order() {
    let (tables, rows) = one_result_surface();
    crate::decode::with_test_decode_ctx(|ctx| {
        let ids = super::feature_result_surface_ids(
            ctx,
            &tables,
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
            17,
        )?;
        assert_eq!(ids, Some(vec![201]));
        let by_feature = super::feature_result_surface_ids_by_feature(
            ctx,
            &tables,
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
        )?;
        assert_eq!(by_feature.get(&17), Some(&vec![201]));
        Ok::<(), cadmpeg_core::CodecError>(())
    })
    .expect("service profile admits the result surface");
}

#[test]
fn draft_neutral_plane_rejects_duplicate_materialized_roster_entry() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            225,
            29,
            vec![crate::feature::entity::FeatureEntityTableEntry {
                entity_id: 226,
                payload: crate::feature::entity::entry_payload(209, None, None, None),
                prefixed: true,
                offset: 0,
                end_offset: 0,
            }],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([226]),
    );
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 226,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 225,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });

    assert_eq!(
        draft_neutral_plane_selection_with_service(&scan, 225),
        cadmpeg_ir::features::FaceSelection::Native("creo:visibgeom:surface#226".to_string())
    );

    scan.features.entity_tables[0]
        .entries
        .push(crate::feature::entity::dummy_table_entry(226));
    assert_eq!(
        draft_neutral_plane_selection_with_service(&scan, 225),
        cadmpeg_ir::features::FaceSelection::Unresolved
    );
}

#[test]
fn draft_neutral_plane_native_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            225,
            29,
            vec![crate::feature::entity::FeatureEntityTableEntry {
                entity_id: 226,
                payload: crate::feature::entity::entry_payload(209, None, None, None),
                prefixed: true,
                offset: 0,
                end_offset: 0,
            }],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([226]),
    );
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 226,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 225,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo draft neutral plane native"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root is admitted");
            super::draft_neutral_plane_selection(&ctx, &scan, 225).map(|_| ())
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = super::draft_neutral_plane_selection(&ctx, &scan, 225)
        .expect_err("draft neutral plane native exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo draft neutral plane native"),
        "{error:?}"
    );
}

#[test]
fn feature_surface_transitions_reject_duplicate_output_roster_entry() {
    let entry =
        |entity_id, class_id, related_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(
                class_id,
                None,
                related_entity_id,
                related_entity_id.map(|_| 0),
            ),

            entity_id,
            prefixed: true,
            offset: usize::try_from(entity_id).expect("fixture index fits usize"),
            end_offset: usize::try_from(entity_id).expect("fixture index fits usize"),
        };
    let mut table = crate::feature::entity::FeatureEntityTable::new(
        17,
        80,
        vec![entry(101, 214, Some(11)), entry(201, 210, Some(101))],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([201]);
    let rows = vec![
        crate::surface::SurfaceRow {
            id: 11,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 3,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 11,
        },
        crate::surface::SurfaceRow {
            id: 201,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 17,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 201,
        },
    ];

    assert_eq!(
        feature_surface_transitions_with_service(17, std::slice::from_ref(&table), &rows),
        Some(vec![(11, 201)])
    );

    table.entries.push(entry(201, 210, Some(101)));
    table.mark_surface_id(201);
    assert_eq!(
        feature_surface_transitions_with_service(17, std::slice::from_ref(&table), &rows),
        None
    );
}

fn transition_limit_error(operation: &'static str, dependency_route: bool) {
    let entry =
        |entity_id, class_id, related_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(
                class_id,
                None,
                related_entity_id,
                related_entity_id.map(|_| 0),
            ),
            entity_id,
            prefixed: true,
            offset: usize::try_from(entity_id).expect("fixture index fits usize"),
            end_offset: usize::try_from(entity_id).expect("fixture index fits usize"),
        };
    let table = crate::feature::entity::FeatureEntityTable::new(
        17,
        80,
        vec![entry(101, 214, Some(11)), entry(201, 210, Some(101))],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([201]);
    let row = |id, feature_id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture index fits usize"),
    };
    let rows = [row(11, 3), row(201, 17)];
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| {
            if dependency_route {
                super::surface_transition_dependencies(
                    ctx,
                    17,
                    std::slice::from_ref(&table),
                    &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.to_vec()),
                )
                .map(|_| ())
            } else {
                super::feature_surface_transitions(
                    ctx,
                    17,
                    std::slice::from_ref(&table),
                    &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.to_vec()),
                )
                .map(|_| ())
            }
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn transition_output_identity_nodes_refuse_collection_limit() {
    transition_limit_error("creo transition output identity nodes", false);
}

#[test]
fn transition_intermediate_identity_nodes_refuse_collection_limit() {
    transition_limit_error("creo transition intermediate identity nodes", false);
}

#[test]
fn transition_source_identity_nodes_refuse_collection_limit() {
    transition_limit_error("creo transition source identity nodes", false);
}

#[test]
fn surface_transitions_refuse_collection_limit() {
    transition_limit_error("creo surface transitions", false);
}

#[test]
fn transition_dependencies_refuse_collection_limit() {
    transition_limit_error("creo transition dependencies", true);
}

#[test]
fn feature_result_faces_require_unique_owned_materialized_table_surfaces() {
    let row = |id, feature_id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),

            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
    let table = crate::feature::entity::FeatureEntityTable::new(
        97,
        29,
        vec![entry(98, 200, Some(1)), entry(145, 203, None)],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([98, 145]);
    let rows = [row(98, 97), row(145, 97)];
    let curve_rows = [crate::curve::CurveTopologyRow {
        id: 77,
        type_byte: 8,
        feature_id: 97,
        directions: [1, 0xf6],
        faces: [
            std::num::NonZeroU32::new(98),
            std::num::NonZeroU32::new(145),
        ],
        next_edges: [77, 77],
        offset: 0,
    }];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_edge_ids(ctx, &curve_rows, 97))
            .expect("service profile admits result edge IDs"),
        Some(vec![77])
    );
    let duplicate_curve_rows = [
        curve_rows[0].clone(),
        crate::curve::CurveTopologyRow {
            offset: 1,
            ..curve_rows[0].clone()
        },
    ];
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_edge_ids(
            ctx,
            &duplicate_curve_rows,
            97
        ))
        .expect("service profile admits duplicate check")
        .is_none()
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_surface_ids(
            ctx,
            std::slice::from_ref(&table),
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.to_vec()),
            97
        ))
        .expect("service profile admits the result surfaces"),
        Some(vec![98, 145])
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_topology(
            ctx,
            std::slice::from_ref(&table),
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.to_vec()),
            &curve_rows,
            97
        ))
        .expect("service profile admits the result topology")
        .expect("complete result topology")
        .faces()
        .iter()
        .map(cadmpeg_core::text::NonBlankString::as_str)
        .collect::<Vec<_>>(),
        vec!["surface#98", "surface#145"]
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_topology(
            ctx,
            std::slice::from_ref(&table),
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.to_vec()),
            &curve_rows,
            97
        ))
        .expect("service profile admits the result topology")
        .expect("complete result topology")
        .edges()
        .iter()
        .map(cadmpeg_core::text::NonBlankString::as_str)
        .collect::<Vec<_>>(),
        vec!["curve#77"]
    );

    let mut duplicate = table.clone();
    let extra = entry(98, 204, None);
    duplicate.entries.push(extra);
    duplicate.mark_surface_id(98);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_surface_ids(
            ctx,
            &[duplicate],
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.to_vec()),
            97
        ))
        .expect("service profile admits the duplicate check")
        .is_none()
    );

    let mut missing = table;
    missing
        .entries
        .edit(1, |slot| *slot = entry(146, 203, None));
    missing.mark_surface_id(146);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_surface_ids(
            ctx,
            &[missing],
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.to_vec()),
            97
        ))
        .expect("service profile admits the missing-row check")
        .is_none()
    );

    let foreign = crate::feature::entity::FeatureEntityTable::new(
        97,
        29,
        vec![entry(145, 203, None)],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([145]);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_surface_ids(
            ctx,
            &[foreign],
            &crate::surface::unique_rows::UniqueIdRows::from_rows([row(145, 144)].to_vec()),
            97
        ))
        .expect("service profile admits the foreign-row check")
        .is_none()
    );
}

#[test]
fn feature_result_identity_validation_refuses_at_work_boundaries() {
    let (tables, rows) = one_result_surface();
    let topology = crate::test_support::assert_work_boundaries(
        &[
            "creo feature result topology identity validation",
            "creo feature result owner identity validation",
        ],
        |ctx| {
            feature_result_topology(
                ctx,
                &tables,
                &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
                &[],
                17,
            )
        },
    )
    .expect("one feature result topology");
    assert_eq!(
        topology.id.as_str(),
        "creo:model:feature-result-topology#17"
    );
    assert_eq!(topology.output_of.as_str(), "creo:model:feature#17");
    assert_eq!(
        topology
            .faces()
            .iter()
            .map(cadmpeg_core::text::NonBlankString::as_str)
            .collect::<Vec<_>>(),
        vec!["surface#201"],
    );
}

#[test]
fn generated_surface_result_id_membership_refuses_work_and_preserves_face() {
    let row = crate::surface::SurfaceRow {
        id: 201,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let available = std::collections::BTreeSet::from([cadmpeg_ir::features::FeatureId::mint(
        "creo:model:feature#17",
    )
    .expect("fixture feature ID")]);
    let results = std::collections::BTreeMap::from([(17, vec![201])]);
    let generated = crate::test_support::assert_work_boundaries(
        &["creo generated surface result ID lookup"],
        |ctx| {
            generated_surface_face_refs(
                ctx,
                &[201],
                &crate::surface::unique_rows::UniqueIdRows::from_rows(
                    std::slice::from_ref(&row).to_vec(),
                ),
                &results,
                &available,
            )
        },
    )
    .expect("the result roster contains the generated surface");
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].feature.as_str(), "creo:model:feature#17");
    assert_eq!(generated[0].local_id.as_str(), "surface#201");
}

#[test]
fn generated_surface_feature_membership_miss_preserves_result_id_laziness() {
    let row = crate::surface::SurfaceRow {
        id: 201,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let available = std::collections::BTreeSet::from([cadmpeg_ir::features::FeatureId::mint(
        "creo:model:feature#18",
    )
    .expect("nonmatching feature ID")]);
    let results = std::collections::BTreeMap::from([(17, vec![201])]);
    let refusal = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo generated surface feature lookup",
        |ctx| {
            generated_surface_face_refs(
                ctx,
                &[201],
                &crate::surface::unique_rows::UniqueIdRows::from_rows(
                    std::slice::from_ref(&row).to_vec(),
                ),
                &results,
                &available,
            )
        },
    );
    let limit = match refusal {
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "creo generated surface feature lookup" =>
        {
            limit
        }
        error => panic!("expected generated feature membership refusal, got {error:?}"),
    };
    let cap = limit.used.checked_add(limit.additional).expect("work cap");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    assert!(generated_surface_face_refs(
        &ctx,
        &[201],
        &crate::surface::unique_rows::UniqueIdRows::from_rows(std::slice::from_ref(&row).to_vec()),
        &results,
        &available,
    )
    .expect("a feature miss returns before result-ID membership")
    .is_none());
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        generated_surface_face_refs(
            ctx,
            &[201],
            &crate::surface::unique_rows::UniqueIdRows::from_rows(
                std::slice::from_ref(&row).to_vec(),
            ),
            &results,
            &available,
        )
    })
    .expect("service profile preserves the feature-miss result")
    .is_none());
}

#[test]
fn generated_surface_feature_identity_validation_refuses_at_work_boundary() {
    let row = crate::surface::SurfaceRow {
        id: 201,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let available = std::collections::BTreeSet::from([cadmpeg_ir::features::FeatureId::mint(
        "creo:model:feature#17",
    )
    .expect("fixture feature ID")]);
    let results = std::collections::BTreeMap::from([(17, vec![201])]);
    let generated = crate::test_support::assert_work_boundaries(
        &[
            "creo generated surface feature identity validation",
            "creo generated surface feature lookup",
        ],
        |ctx| {
            generated_surface_face_refs(
                ctx,
                &[201],
                &crate::surface::unique_rows::UniqueIdRows::from_rows(
                    std::slice::from_ref(&row).to_vec(),
                ),
                &results,
                &available,
            )
        },
    )
    .expect("row has one matching generated face");
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].feature.as_str(), "creo:model:feature#17");
    assert_eq!(generated[0].local_id.as_str(), "surface#201");
}
