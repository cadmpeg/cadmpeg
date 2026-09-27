// SPDX-License-Identifier: Apache-2.0

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
        super::draft_neutral_plane_selection(&scan, 225),
        cadmpeg_ir::features::FaceSelection::Native("creo:visibgeom:surface#226".to_string())
    );

    scan.features.entity_tables[0]
        .entries
        .push(crate::feature::entity::dummy_table_entry(226));
    assert_eq!(
        super::draft_neutral_plane_selection(&scan, 225),
        cadmpeg_ir::features::FaceSelection::Unresolved
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
        super::feature_surface_transitions(17, std::slice::from_ref(&table), &rows),
        Some(vec![(11, 201)])
    );

    table.entries.push(entry(201, 210, Some(101)));
    table.mark_surface_id(201);
    assert_eq!(
        super::feature_surface_transitions(17, std::slice::from_ref(&table), &rows),
        None
    );
}
