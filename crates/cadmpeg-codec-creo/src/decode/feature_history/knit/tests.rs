// SPDX-License-Identifier: Apache-2.0

use super::{
    feature_result_surface_ids, feature_result_topology, generated_surface_face_refs,
    knit_class_100_operand_entity_ids, knit_operand_entity_ids, knit_operand_surface_ids,
    knit_surface_feature_definition,
};
use super::super::selections::feature_result_edge_ids;

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
        super::feature_surface_transitions(ctx, feature_id, tables, rows)
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
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.entity_tables = vec![
        table(97, 67, entry(103, 200, 11), 10),
        table(97, 100, entry(103, 98, 21), 20),
        table(97, 67, entry(98, 0, 31), 30).with_surface_ids([98]),
        table(416, 100, entry(103, 98, 41), 40),
    ];
    scan.surfaces.rows = vec![crate::surface::SurfaceRow {
        id: 98,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 97,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }];
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

fn knit_operand_collection_error(
    limit: u64,
    operation: &'static str,
    route: &str,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = one_knit_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = match route {
        "class" => knit_class_100_operand_entity_ids(&ctx, 416, &scan.features.entity_tables).map(|_| ()),
        "quilt" => knit_operand_entity_ids(&ctx, &scan, 416).map(|_| ()),
        "surface" => knit_operand_surface_ids(&ctx, &scan, 416, &[103]).map(|_| ()),
        _ => panic!("unknown fixture route"),
    }
    .expect_err("one operand exceeds the resource limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn knit_consumer_identity_nodes_refuse_collection_limit() {
    knit_operand_collection_error(0, "creo knit consumer identity nodes", "class");
}

#[test]
fn knit_class_100_operand_ids_refuse_collection_limit() {
    knit_operand_collection_error(1, "creo knit class 100 operand IDs", "class");
}

#[test]
fn knit_quilt_identity_nodes_refuse_collection_limit() {
    knit_operand_collection_error(0, "creo knit quilt identity nodes", "quilt");
}

#[test]
fn knit_quilt_ids_refuse_collection_limit() {
    knit_operand_collection_error(1, "creo knit quilt IDs", "quilt");
}

#[test]
fn knit_surface_identity_nodes_refuse_collection_limit() {
    knit_operand_collection_error(0, "creo knit surface identity nodes", "surface");
}

#[test]
fn knit_surface_ids_refuse_collection_limit() {
    knit_operand_collection_error(1, "creo knit surface IDs", "surface");
}

#[test]
fn knit_native_selection_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = one_knit_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = knit_surface_feature_definition(&ctx, &scan, 416)
        .expect_err("one native selection exceeds the retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo knit native selection"), "{error:?}");
}

#[test]
fn knit_generated_native_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = one_knit_scan();
    let native = "creo:allfeatur:surface_merge_quilts#416:103";
    let producer = "creo:model:feature#97";
    let local = "surface#98";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = (native.len() + producer.len() * 2 + local.len()) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = knit_surface_feature_definition(&ctx, &scan, 416)
        .expect_err("generated native copy exceeds the retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo knit generated native selection"), "{error:?}");
}

#[test]
fn knit_operand_fixture_generates_a_surface_face_under_service_policy() {
    let scan = one_knit_scan();
    let definition = crate::decode::with_test_decode_ctx(|ctx| {
        knit_surface_feature_definition(ctx, &scan, 416)
    })
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
    collection: Option<u64>,
    retained: Option<u64>,
    operation: &'static str,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let row = crate::surface::SurfaceRow {
        id: 201,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let available = std::collections::BTreeSet::from([
        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#17")
            .expect("fixture feature ID"),
    ]);
    let results = std::collections::BTreeMap::from([(17, vec![201])]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if let Some(limit) = collection {
        policy.limits.max_collection_items = limit;
    }
    if let Some(limit) = retained {
        policy.limits.max_retained_bytes = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = generated_surface_face_refs(&ctx, &[201], &[row], &results, &available)
        .expect_err("one generated face exceeds the resource limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn generated_surface_feature_id_refuses_retained_limit() {
    generated_face_reference_error(None, Some(0), "creo generated surface feature IDs");
}

#[test]
fn generated_surface_local_id_refuses_retained_limit() {
    generated_face_reference_error(
        None,
        Some("creo:model:feature#17".len() as u64),
        "creo generated surface local IDs",
    );
}

#[test]
fn generated_surface_face_references_refuse_collection_limit() {
    generated_face_reference_error(
        Some(0),
        None,
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
        faces: [std::num::NonZeroU32::new(201), std::num::NonZeroU32::new(202)],
        next_edges: [77, 77],
        offset: 0,
    }]
}

fn topology_limit_error(
    face: bool,
    collection: Option<u64>,
    retained: Option<u64>,
    operation: &'static str,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (tables, rows) = if face { one_result_surface() } else { (Vec::new(), Vec::new()) };
    let curve_rows = if face { Vec::new() } else { one_result_edge() };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if let Some(limit) = collection {
        policy.limits.max_collection_items = limit;
    }
    if let Some(limit) = retained {
        policy.limits.max_retained_bytes = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::feature_result_topology(&ctx, &tables, &rows, &curve_rows, 17)
        .expect_err("one result member exceeds the resource limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn feature_result_face_local_id_refuses_retained_limit() {
    topology_limit_error(true, None, Some(0), "creo feature result face local IDs");
}

#[test]
fn feature_result_face_members_refuse_collection_limit() {
    topology_limit_error(true, Some(2), None, "creo feature result face members");
}

#[test]
fn feature_result_edge_local_id_refuses_retained_limit() {
    topology_limit_error(false, None, Some(0), "creo feature result edge local IDs");
}

#[test]
fn feature_result_edge_members_refuse_collection_limit() {
    topology_limit_error(false, Some(2), None, "creo feature result edge members");
}

#[test]
fn feature_result_topology_id_refuses_retained_limit() {
    topology_limit_error(true, None, Some("surface#201".len() as u64),
        "creo feature result topology ID");
}

#[test]
fn feature_result_owner_id_refuses_retained_limit() {
    topology_limit_error(true, None,
        Some(("surface#201".len() + "creo:model:feature-result-topology#17".len()) as u64),
        "creo feature result owner ID");
}

#[test]
fn feature_result_distinctness_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (mut tables, mut rows) = one_result_surface();
    tables[0].entries.push(crate::feature::entity::dummy_table_entry(202));
    tables[0].mark_surface_id(202);
    rows.push(crate::surface::SurfaceRow { id: 202, ..rows[0].clone() });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::feature_result_topology(&ctx, &tables, &rows, &[], 17)
        .expect_err("two distinctness comparisons exceed zero work units");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo feature result member distinctness"), "{error:?}");
}

#[test]
fn feature_result_topology_arena_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
        FeatureId, FeatureOperation,
    };

    let (tables, rows) = one_result_surface();
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.entity_tables = tables;
    scan.surfaces.rows = rows;
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
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::emit_feature_result_topologies(&ctx, &scan, &mut ir)
        .expect_err("one topology arena row exceeds the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo model feature result topologies"), "{error:?}");
    assert!(ir.model.feature_result_topologies.is_empty());
}

fn result_surface_limit_error(
    limit: u64,
    by_feature: bool,
    operation: &'static str,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (tables, rows) = one_result_surface();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = if by_feature {
        super::feature_result_surface_ids_by_feature(&ctx, &tables, &rows)
            .map(|_| ())
    } else {
        super::feature_result_surface_ids(&ctx, &tables, &rows, 17)
            .map(|_| ())
    }
    .expect_err("the result surface exceeds the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation), "{error:?}");
}

#[test]
fn feature_result_surface_identity_nodes_refuse_collection_limit() {
    result_surface_limit_error(0, false, "creo feature result surface identity nodes");
}

#[test]
fn feature_result_surface_ids_refuse_collection_limit() {
    result_surface_limit_error(1, false, "creo feature result surface IDs");
}

#[test]
fn feature_result_feature_identity_nodes_refuse_collection_limit() {
    result_surface_limit_error(0, true, "creo feature result feature identity nodes");
}

#[test]
fn feature_result_surface_map_nodes_refuse_collection_limit() {
    result_surface_limit_error(3, true, "creo feature result surface map nodes");
}

#[test]
fn feature_result_surface_roster_preserves_order() {
    let (tables, rows) = one_result_surface();
    crate::decode::with_test_decode_ctx(|ctx| {
        let ids = super::feature_result_surface_ids(ctx, &tables, &rows, 17)?;
        assert_eq!(ids, Some(vec![201]));
        let by_feature = super::feature_result_surface_ids_by_feature(ctx, &tables, &rows)?;
        assert_eq!(by_feature.get(&17), Some(&vec![201]));
        Ok::<(), cadmpeg_core::CodecError>(())
    }).expect("service profile admits the result surface");
}

#[test]
fn draft_neutral_plane_rejects_duplicate_materialized_roster_entry() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
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
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
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
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::draft_neutral_plane_selection(&ctx, &scan, 225)
        .expect_err("draft neutral plane native exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo draft neutral plane native"), "{error:?}");
}

fn thicken_offset_limit_error(limit: u64, operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let planes = std::collections::BTreeMap::from([
        (11, crate::decode::analytic::equations::PlaneEquation {
            origin: [0.0, 2.0, 0.0],
            normal: [0.0, -1.0, 0.0],
        }),
        (201, crate::decode::analytic::equations::PlaneEquation {
            origin: [0.0, -3.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        }),
    ]);
    let row = |id, reversed| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: if id >= 200 { 17 } else { 3 },
        reversed,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: id as usize,
    };
    let rows = [row(11, true), row(201, false)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::thicken_plane_offset(&ctx, &[(11, 201)], &planes, &rows)
        .expect_err("one thicken offset exceeds the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn thicken_plane_offsets_refuse_collection_limit() {
    thicken_offset_limit_error(0, "creo thicken plane offsets");
}

#[test]
fn thicken_plane_magnitudes_refuse_collection_limit() {
    thicken_offset_limit_error(1, "creo thicken plane magnitudes");
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
            offset: entity_id as usize,
            end_offset: entity_id as usize,
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

fn transition_limit_error(limit: u64, operation: &'static str, dependency_route: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let entry = |entity_id, class_id, related_entity_id| {
        crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(
                class_id,
                None,
                related_entity_id,
                related_entity_id.map(|_| 0),
            ),
            entity_id,
            prefixed: true,
            offset: entity_id as usize,
            end_offset: entity_id as usize,
        }
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
        offset: id as usize,
    };
    let rows = [row(11, 3), row(201, 17)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = if dependency_route {
        super::surface_transition_dependencies(&ctx, 17, &[table], &rows).map(|_| ())
    } else {
        super::feature_surface_transitions(&ctx, 17, &[table], &rows).map(|_| ())
    }
    .expect_err("one transition exceeds the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn transition_output_identity_nodes_refuse_collection_limit() {
    transition_limit_error(0, "creo transition output identity nodes", false);
}

#[test]
fn transition_intermediate_identity_nodes_refuse_collection_limit() {
    transition_limit_error(1, "creo transition intermediate identity nodes", false);
}

#[test]
fn transition_source_identity_nodes_refuse_collection_limit() {
    transition_limit_error(2, "creo transition source identity nodes", false);
}

#[test]
fn surface_transitions_refuse_collection_limit() {
    transition_limit_error(3, "creo surface transitions", false);
}

#[test]
fn transition_dependencies_refuse_collection_limit() {
    transition_limit_error(4, "creo transition dependencies", true);
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
    assert_eq!(crate::decode::with_test_decode_ctx(|ctx| feature_result_edge_ids(ctx, &curve_rows, 97))
        .expect("service profile admits result edge IDs"), Some(vec![77]));
    let duplicate_curve_rows = [
        curve_rows[0].clone(),
        crate::curve::CurveTopologyRow {
            offset: 1,
            ..curve_rows[0].clone()
        },
    ];
    assert!(crate::decode::with_test_decode_ctx(|ctx| feature_result_edge_ids(ctx, &duplicate_curve_rows, 97))
        .expect("service profile admits duplicate check").is_none());
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_surface_ids(ctx, std::slice::from_ref(&table), &rows, 97))
            .expect("service profile admits the result surfaces"),
        Some(vec![98, 145])
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_topology(ctx, std::slice::from_ref(&table), &rows, &curve_rows, 97))
            .expect("service profile admits the result topology")
            .expect("complete result topology")
            .faces(),
        vec!["surface#98", "surface#145"]
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_topology(ctx, std::slice::from_ref(&table), &rows, &curve_rows, 97))
            .expect("service profile admits the result topology")
            .expect("complete result topology")
            .edges(),
        vec!["curve#77"]
    );

    let mut duplicate = table.clone();
    let extra = entry(98, 204, None);
    duplicate.entries.push(extra);
    duplicate.mark_surface_id(98);
    assert!(crate::decode::with_test_decode_ctx(|ctx| feature_result_surface_ids(ctx, &[duplicate], &rows, 97))
        .expect("service profile admits the duplicate check").is_none());

    let mut missing = table;
    missing.entries[1] = entry(146, 203, None);
    missing.mark_surface_id(146);
    assert!(crate::decode::with_test_decode_ctx(|ctx| feature_result_surface_ids(ctx, &[missing], &rows, 97))
        .expect("service profile admits the missing-row check").is_none());

    let foreign = crate::feature::entity::FeatureEntityTable::new(
        97,
        29,
        vec![entry(145, 203, None)],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([145]);
    assert!(crate::decode::with_test_decode_ctx(|ctx| feature_result_surface_ids(ctx, &[foreign], &[row(145, 144)], 97))
        .expect("service profile admits the foreign-row check").is_none());
}
