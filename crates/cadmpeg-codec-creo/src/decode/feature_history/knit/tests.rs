// SPDX-License-Identifier: Apache-2.0

use super::super::selections::feature_result_edge_ids_by_feature;
use super::{feature_result_surface_ids, knit_surface_feature_definition};

fn feature_result_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
    curve_rows: &[crate::curve::CurveTopologyRow],
    feature_id: u32,
) -> Result<Option<cadmpeg_ir::features::FeatureResultTopology>, cadmpeg_core::CodecError> {
    let edges = feature_result_edge_ids_by_feature(curve_rows)
        .remove(&feature_id)
        .unwrap_or_default();
    super::feature_result_topology(ctx, tables, rows, &edges, feature_id)
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

#[test]
fn knit_operand_fixture_generates_a_surface_face() {
    let scan = one_knit_scan();
    let definition =
        crate::decode::with_test_decode_ctx(|ctx| knit_surface_feature_definition(ctx, &scan, 416))
            .expect("knit surface selection");
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

#[test]
fn feature_result_topology_emits_one_row_per_result_surface() {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };

    let (tables, rows) = one_result_surface();
    let mut scan = crate::test_support::empty_container_scan();
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
    crate::decode::with_test_decode_ctx(|ctx| {
        super::emit_feature_result_topologies(ctx, &scan, &mut ir)
    })
    .expect("feature result topologies");
    // One feature with one result surface emits one topology row.
    assert_eq!(ir.model.feature_result_topologies.len(), 1);
}

#[test]
fn feature_result_surface_roster_preserves_order() {
    let (tables, rows) = one_result_surface();
    let ids = super::feature_result_surface_ids(&tables, &rows, 17);
    assert_eq!(ids, Some(vec![201]));
    let by_feature = super::feature_result_surface_ids_by_feature(&tables, &rows);
    assert_eq!(by_feature.get(&17), Some(&vec![201]));
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
        feature_result_edge_ids_by_feature(&curve_rows).remove(&97),
        Some(vec![77])
    );
    let duplicate_curve_rows = [
        curve_rows[0].clone(),
        crate::curve::CurveTopologyRow {
            offset: 1,
            ..curve_rows[0].clone()
        },
    ];
    assert!(feature_result_edge_ids_by_feature(&duplicate_curve_rows)
        .remove(&97)
        .is_none());
    assert_eq!(
        feature_result_surface_ids(std::slice::from_ref(&table), &rows, 97),
        Some(vec![98, 145])
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_topology(
            ctx,
            std::slice::from_ref(&table),
            &rows,
            &curve_rows,
            97
        ))
        .expect("service profile admits the result topology")
        .expect("complete result topology")
        .faces(),
        vec!["surface#98", "surface#145"]
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_result_topology(
            ctx,
            std::slice::from_ref(&table),
            &rows,
            &curve_rows,
            97
        ))
        .expect("service profile admits the result topology")
        .expect("complete result topology")
        .edges(),
        vec!["curve#77"]
    );

    let mut duplicate = table.clone();
    let extra = entry(98, 204, None);
    duplicate.entries.push(extra);
    duplicate.mark_surface_id(98);
    assert!(feature_result_surface_ids(&[duplicate], &rows, 97).is_none());

    let mut missing = table;
    missing.entries[1] = entry(146, 203, None);
    missing.mark_surface_id(146);
    assert!(feature_result_surface_ids(&[missing], &rows, 97).is_none());

    let foreign = crate::feature::entity::FeatureEntityTable::new(
        97,
        29,
        vec![entry(145, 203, None)],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([145]);
    assert!(feature_result_surface_ids(&[foreign], &[row(145, 144)], 97).is_none());
}
