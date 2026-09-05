// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use super::super::definitions::*;
use super::super::entity::*;
use super::super::operations::*;
use super::super::rows::*;

#[test]
fn binds_missing_definition_owner_from_unique_generated_datum_table() {
    let mut definitions = [FeatureDefinition {
        id: 917,
        owner_feature_id: None,
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(FeatureSection3d {
            sketch_plane_entity_id: Some(12),
            sketch_plane_flip: None,
            reference_plane_entity_ids: Vec::new(),
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 1,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    }];
    bind_definition_owners(
        &mut definitions,
        &[FeatureGeometryTable {
            feature_id: 10,
            kind: FeatureGeometryTableKind::DatumIds,
            count: 1,
            entity_class: 87,
            entry_ids: Some(vec![12]),
            offset: 2,
        }],
    );

    assert_eq!(definitions[0].owner_feature_id, Some(10));
}

fn pending_replay(external_ids: &[u32]) -> FeatureDefinition {
    FeatureDefinition {
        id: 0,
        owner_feature_id: None,
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(FeatureOrderTable {
            declared_count: external_ids.len() as u32,
            has_prototype: false,
            entity_ref: Some(1),
            rows: external_ids
                .iter()
                .enumerate()
                .map(|(index, external_id)| FeatureOrderRow {
                    external_id: *external_id,
                    internal_id: index as u32 + 1,
                    bitmask: 0,
                    offset: index,
                })
                .collect(),
            offset: 0,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    }
}

fn pending_trimmed_definition(external_ids: &[u32]) -> FeatureDefinition {
    let mut definition = pending_replay(&[]);
    definition.id = 917;
    definition.trim_entities = Some(FeatureTrimEntityTable {
        declared_count: Some(external_ids.len() as u32),
        entity_ref: Some(1),
        entry_ref: None,
        buckets: Vec::new(),
        rows: external_ids
            .iter()
            .enumerate()
            .map(|(index, external_id)| FeatureTrimEntity {
                external_id: *external_id,
                kind: TrimEntityKind::Line,
                mode: Some(0),
                vertices: [index as u32, index as u32 + 1],
                center_vertex: None,
                offset: index,
            })
            .collect(),
        solved_external_ids: external_ids.to_vec(),
        offset: 0,
    });
    definition
}

fn generated_entity_table(owner: u32, source_ids: &[u32]) -> FeatureEntityTable {
    FeatureEntityTable {
        feature_id: owner,
        table_class_id: 80,
        entries: source_ids
            .iter()
            .enumerate()
            .map(|(index, source_id)| FeatureEntityTableEntry {
                entity_id: index as u32 + 1,
                class_id: 200,
                source_entity_id: Some(*source_id),
                related_entity_id: None,
                related_entity_state: None,
                prefixed: true,
                offset: index,
                end_offset: index + 1,
                is_surface: false,
            })
            .collect(),
        offset: 0,
    }
    .with_surface_ids([])
}

#[test]
fn binds_replay_owner_from_unique_source_entity_subset() {
    let mut definitions = [pending_replay(&[10, 11, 12])];
    bind_replay_definition_owners(
        &mut definitions,
        &[generated_entity_table(42, &[10, 12])],
        &BTreeSet::new(),
    );

    assert_eq!(definitions[0].id, 42);
    assert_eq!(definitions[0].owner_feature_id, Some(42));
}

#[test]
fn binds_replay_owner_from_exact_trimmed_entity_set() {
    let mut definition = pending_trimmed_definition(&[9, 10, 11, 12]);
    definition.id = 0;
    definition.order_table = pending_replay(&[10, 11, 12]).order_table;
    let mut definitions = [definition];
    bind_replay_definition_owners(
        &mut definitions,
        &[
            generated_entity_table(42, &[12, 11, 10, 9]),
            generated_entity_table(43, &[10]),
        ],
        &BTreeSet::new(),
    );

    assert_eq!(definitions[0].id, 42);
    assert_eq!(definitions[0].owner_feature_id, Some(42));
}

#[test]
fn falls_back_to_replay_order_when_trimmed_entities_do_not_join() {
    let mut definition = pending_trimmed_definition(&[9, 10]);
    definition.id = 0;
    definition.order_table = pending_replay(&[10, 11, 12]).order_table;
    let mut definitions = [definition];
    bind_replay_definition_owners(
        &mut definitions,
        &[generated_entity_table(42, &[10, 12])],
        &BTreeSet::new(),
    );

    assert_eq!(definitions[0].id, 42);
    assert_eq!(definitions[0].owner_feature_id, Some(42));
}

#[test]
fn falls_back_to_replay_order_for_duplicate_trimmed_entity_ids() {
    let mut definition = pending_trimmed_definition(&[9, 9]);
    definition.id = 0;
    definition.order_table = pending_replay(&[9]).order_table;
    let mut definitions = [definition];
    bind_replay_definition_owners(
        &mut definitions,
        &[generated_entity_table(42, &[9])],
        &BTreeSet::new(),
    );

    assert_eq!(definitions[0].id, 42);
    assert_eq!(definitions[0].owner_feature_id, Some(42));
}

#[test]
fn binds_saved_section_owner_from_exact_trimmed_entity_set() {
    let mut definitions = [pending_trimmed_definition(&[9, 10, 11, 14, 21])];
    bind_trimmed_definition_owners(
        &mut definitions,
        &[generated_entity_table(667, &[14, 21, 11, 10, 9])],
    );

    assert_eq!(definitions[0].id, 917);
    assert_eq!(definitions[0].owner_feature_id, Some(667));
}

#[test]
fn withholds_saved_section_owner_for_partial_reused_or_duplicate_entity_sets() {
    let mut partial = [pending_trimmed_definition(&[9, 10, 11])];
    bind_trimmed_definition_owners(&mut partial, &[generated_entity_table(667, &[9, 10])]);
    assert_eq!(partial[0].owner_feature_id, None);

    let mut reused = [
        pending_trimmed_definition(&[9, 10]),
        pending_trimmed_definition(&[9, 10]),
    ];
    bind_trimmed_definition_owners(&mut reused, &[generated_entity_table(667, &[9, 10])]);
    assert!(reused
        .iter()
        .all(|definition| definition.owner_feature_id.is_none()));

    let mut duplicate = [pending_trimmed_definition(&[9, 9])];
    bind_trimmed_definition_owners(&mut duplicate, &[generated_entity_table(667, &[9])]);
    assert_eq!(duplicate[0].owner_feature_id, None);
}

#[test]
fn saved_section_owner_uses_only_class_200_source_ids() {
    let mut table = generated_entity_table(667, &[9]);
    table.entries.push(FeatureEntityTableEntry {
        entity_id: 2,
        class_id: 201,
        source_entity_id: Some(10),
        related_entity_id: None,
        related_entity_state: None,
        prefixed: true,
        offset: 1,
        end_offset: 2,
        is_surface: false,
    });
    let mut definitions = [pending_trimmed_definition(&[9, 10])];

    bind_trimmed_definition_owners(&mut definitions, &[table]);

    assert_eq!(definitions[0].owner_feature_id, None);
}

#[test]
fn withholds_replay_owner_for_empty_or_ambiguous_source_joins() {
    let mut empty = [pending_replay(&[10])];
    bind_replay_definition_owners(
        &mut empty,
        &[generated_entity_table(42, &[])],
        &BTreeSet::new(),
    );
    assert_eq!(empty[0].owner_feature_id, None);

    let mut ambiguous = [pending_replay(&[10, 11])];
    bind_replay_definition_owners(
        &mut ambiguous,
        &[
            generated_entity_table(42, &[10]),
            generated_entity_table(43, &[11]),
        ],
        &BTreeSet::new(),
    );
    assert_eq!(ambiguous[0].owner_feature_id, None);

    let mut repeated_owner = [pending_replay(&[10]), pending_replay(&[10, 11])];
    bind_replay_definition_owners(
        &mut repeated_owner,
        &[generated_entity_table(42, &[10])],
        &BTreeSet::new(),
    );
    assert!(repeated_owner
        .iter()
        .all(|definition| definition.owner_feature_id.is_none()));

    let mut claimed = [pending_replay(&[10])];
    bind_replay_definition_owners(
        &mut claimed,
        &[generated_entity_table(42, &[10])],
        &BTreeSet::from([42]),
    );
    assert_eq!(claimed[0].owner_feature_id, None);

    let mut exact_ambiguous = pending_trimmed_definition(&[9, 10]);
    exact_ambiguous.id = 0;
    exact_ambiguous.order_table = pending_replay(&[9]).order_table;
    let mut exact_ambiguous = [exact_ambiguous];
    bind_replay_definition_owners(
        &mut exact_ambiguous,
        &[
            generated_entity_table(42, &[9, 10]),
            generated_entity_table(43, &[10, 9]),
        ],
        &BTreeSet::new(),
    );
    assert_eq!(exact_ambiguous[0].owner_feature_id, None);
}

fn operation(feature_id: u32, recipe: Option<FeatureRecipe>, offset: usize) -> FeatureOperation {
    FeatureOperation {
        feature_id,
        kind: OperationKind::Stored(String::new()),
        name: OperationName::Recipe,
        recipe,
        recipe_conflict: false,
        display_state_conflict: false,
        depdb: None,
        offset,
        state_offset: offset,
    }
}

#[test]
fn binds_unique_depdb_section_from_recipe_datum_plane_chain() {
    let mut definition = pending_replay(&[]);
    definition.section_3d = Some(FeatureSection3d {
        sketch_plane_entity_id: Some(249),
        sketch_plane_flip: None,
        reference_plane_entity_ids: Vec::new(),
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: 0,
    });
    let operations = [
        operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
        operation(248, None, 20),
    ];

    bind_section_owners(
        std::slice::from_mut(&mut definition),
        &operations,
        &[(0, usize::MAX)],
    );

    assert_eq!(definition.id, 247);
    assert_eq!(definition.owner_feature_id, Some(247));
}

#[test]
fn depdb_owner_binding_preserves_stored_definition_identifier() {
    let mut definition = pending_replay(&[]);
    definition.id = 2;
    definition.section_3d = Some(FeatureSection3d {
        sketch_plane_entity_id: Some(249),
        sketch_plane_flip: None,
        reference_plane_entity_ids: Vec::new(),
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: 0,
    });
    let operations = [
        operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
        operation(248, None, 20),
    ];

    bind_section_owners(
        std::slice::from_mut(&mut definition),
        &operations,
        &[(0, usize::MAX)],
    );

    assert_eq!(definition.id, 2);
    assert_eq!(definition.owner_feature_id, Some(247));
}

#[test]
fn decodes_owned_depdb_full_turn_for_rotational_recipe() {
    let mut definition = pending_replay(&[]);
    definition.id = 247;
    definition.owner_feature_id = Some(247);
    definition.offset = 100;
    definition.body = vec![
        0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00,
        0x00, 0x00,
    ];
    let revolve = operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10);

    let decoded = definition_revolution_extents(&[definition.clone()], &[revolve]);

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].feature_id, 247);
    assert_eq!(decoded[0].kind, FeatureRevolutionExtentKind::FullTurn);
    assert_eq!(decoded[0].offset, 106);

    let extrude = operation(247, Some(FeatureRecipe::ProtrudeExtrude), 10);
    assert!(definition_revolution_extents(&[definition], &[extrude]).is_empty());
}

#[test]
fn preserves_repeated_identical_depdb_full_turn_states() {
    let sequence = [
        0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00,
        0x00, 0x00,
    ];
    let mut definition = pending_replay(&[]);
    definition.id = 247;
    definition.owner_feature_id = Some(247);
    definition.offset = 100;
    definition.body.extend(sequence);
    definition.body.extend([0xe7, 0x04, 0x00, 0xe1]);
    definition.body.extend(sequence);
    let revolve = operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10);

    let decoded = definition_revolution_extents(&[definition], &[revolve]);

    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].offset, 106);
    assert_eq!(decoded[1].offset, 127);
    assert!(decoded
        .iter()
        .all(|extent| extent.kind == FeatureRevolutionExtentKind::FullTurn));
}

#[test]
fn withholds_depdb_owner_for_repeated_plane_or_nonconsecutive_datum() {
    let section = FeatureSection3d {
        sketch_plane_entity_id: Some(249),
        sketch_plane_flip: None,
        reference_plane_entity_ids: Vec::new(),
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: 0,
    };
    let mut repeated = [pending_replay(&[]), pending_replay(&[])];
    for definition in &mut repeated {
        definition.section_3d = Some(section.clone());
    }
    let consecutive = [
        operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
        operation(248, None, 20),
    ];
    bind_section_owners(&mut repeated, &consecutive, &[(0, usize::MAX)]);
    assert!(repeated
        .iter()
        .all(|definition| definition.owner_feature_id.is_none()));

    let mut separated = pending_replay(&[]);
    separated.section_3d = Some(section);
    let operations = [
        operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
        operation(900, None, 15),
        operation(248, None, 20),
    ];
    bind_section_owners(
        std::slice::from_mut(&mut separated),
        &operations,
        &[(0, usize::MAX)],
    );
    assert_eq!(separated.owner_feature_id, None);

    let mut claimed = pending_replay(&[]);
    claimed.id = 247;
    claimed.owner_feature_id = Some(247);
    let mut candidate = pending_replay(&[]);
    candidate.section_3d = Some(FeatureSection3d {
        sketch_plane_entity_id: Some(249),
        sketch_plane_flip: None,
        reference_plane_entity_ids: Vec::new(),
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: 0,
    });
    let mut definitions = [claimed, candidate];
    bind_section_owners(&mut definitions, &consecutive, &[(0, usize::MAX)]);
    assert_eq!(definitions[1].owner_feature_id, None);
}

#[test]
fn section_owner_binding_does_not_cross_source_range_boundaries() {
    let mut in_range = pending_replay(&[]);
    in_range.offset = 100;
    in_range.section_3d = Some(FeatureSection3d {
        sketch_plane_entity_id: Some(249),
        sketch_plane_flip: None,
        reference_plane_entity_ids: Vec::new(),
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: 0,
    });
    let mut outside = in_range.clone();
    outside.offset = 200;
    let mut definitions = [in_range, outside];
    let operations = [
        operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
        operation(248, None, 20),
    ];

    bind_section_owners(&mut definitions, &operations, &[(100, 150)]);

    assert_eq!(definitions[0].owner_feature_id, Some(247));
    assert_eq!(definitions[1].owner_feature_id, None);
}

#[test]
fn positional_replays_exclude_the_contextually_owned_instance() {
    let payload = b"feat_defs_917\0template\0\xe0\x01feat_id\0\x2a\
            \xe0\x00ref_model_info\0\xe3S2D0004\0owned\
            \xe3S2D0004\0pending";

    let decoded = positional_replay_definitions(payload);

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].id, 917);
    assert_eq!(decoded[0].owner_feature_id, None);
    assert!(decoded[0].body.starts_with(b"\xe3S2D0004\0"));
    assert!(decoded[0].body.ends_with(b"pending"));
}

#[test]
fn unlabeled_replay_boundary_ends_the_preceding_definition() {
    let payload = b"feat_defs_917\0template\xe3S2D0004\0replay";

    let decoded = definitions(payload);

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].body, b"feat_defs_917\0template");
}

#[test]
fn positional_saved_section_starts_an_owned_definition() {
    let payload = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
            template\0\xe0\x01feat_id\0\x2a\xe0\x00ref_model_info\0\
            \xe0\x00name\0S2D0004\0saved";

    let decoded = definitions(payload);

    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].id, 917);
    assert_eq!(decoded[0].owner_feature_id, Some(40));
    assert_eq!(decoded[0].body.last(), Some(&0));
    assert_eq!(decoded[1].id, 42);
    assert_eq!(decoded[1].owner_feature_id, Some(42));
    assert!(decoded[1].body.starts_with(b"\xe0\x01feat_id\0"));
    assert!(decoded[1].body.ends_with(b"saved"));
}

#[test]
fn positional_saved_section_replays_its_segment_table() {
    let mut payload = b"feat_defs_917\0template\0\xe0\x01feat_id\0\x2a\
            \xe0\x00ref_model_info\0\xe3S2D0004\0\xf8\x03\xf7\x01\xfb\xe2\
            \xf2\xf7\x01\xe2"
        .to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2]);

    let decoded = definitions(&payload);
    let segments = decoded[1].segments.as_ref().expect("positional segtab");

    assert_eq!(segments.declared_count, 3);
    assert!(segments.has_elided_prototype);
    assert_eq!(segments.entity_ref, Some(1));
    assert_eq!(segments.rows.len(), 2);
    assert!(segments.is_complete());
    assert_eq!(segments.rows[0].point_ids, [7, 8]);
    assert_eq!(segments.rows[1].kind, FeatureSegmentKind::Arc);
    assert_eq!(segments.rows[1].center_id, Some(10));
    assert_eq!(segments.rows[1].external_id, 43);
}

#[test]
fn positional_segment_table_stops_at_the_next_s2d_record() {
    let mut payload = b"\xe3S2D0004\0\xf8\x03\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2]);
    payload.extend_from_slice(b"\xe3S2D0004\0");
    payload.extend_from_slice(&[2, 0, 0, 0, 1, 2, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2]);

    let segments = positional_segment_table(&payload, 0, payload.len())
        .expect("first positional segment table");

    assert_eq!(segments.declared_count, 3);
    assert!(segments.has_elided_prototype);
    assert_eq!(segments.rows.len(), 2);
    assert!(segments.is_complete());
    assert_eq!(segments.segment(42), Some(&segments.rows[0]));
    assert_eq!(segments.rows[0].external_id, 42);
    assert_eq!(segments.rows[1].external_id, 43);
}

#[test]
fn positional_segment_extent_excludes_rows_after_the_declared_extent() {
    let mut payload = b"\xe3S2D0004\0\xf8\x02\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2]);

    let segments = positional_segment_table(&payload, 0, payload.len()).expect("positional segtab");

    assert!(segments.has_elided_prototype);
    assert_eq!(segments.rows.len(), 1);
    assert_eq!(segments.rows[0].external_id, 42);
    assert!(segments.is_complete());
}

#[test]
fn positional_segment_rows_follow_variable_structural_trailers() {
    let mut payload = b"\xe3S2D0004\0\xf8\x03\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2]);
    payload.extend_from_slice(&[0xe3, 0xe2, 0x81, 0x18, 0x07, 0xe2]);
    payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2]);

    let segments = positional_segment_table(&payload, 0, payload.len()).expect("positional segtab");

    assert!(segments.is_complete());
    assert_eq!(segments.rows.len(), 2);
    assert_eq!(segments.rows[1].kind, FeatureSegmentKind::Arc);
    assert_eq!(segments.rows[1].external_id, 43);
}

#[test]
fn segment_tables_retain_extents_without_decoded_rows() {
    let named = b"segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2";
    let segments = segment_table(named, 0, named.len()).expect("named segtab header");
    assert_eq!(segments.declared_count, 2);
    assert_eq!(segments.entity_ref, Some(1));
    assert!(segments.rows.is_empty());
    assert!(!segments.is_complete());

    let positional = b"\xf8\x02\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2";
    let segments = segment_table_body(positional, 0, 0, positional.len(), false)
        .expect("positional segtab header");
    assert_eq!(segments.declared_count, 2);
    assert_eq!(segments.entity_ref, Some(1));
    assert!(segments.rows.is_empty());
    assert!(!segments.is_complete());
}

#[test]
fn segment_table_prototype_close_requires_the_header_class() {
    let payload = b"\xf8\x02\xf7\x01\xfb\xe2\xf2\xf7\x02\xe2";

    assert!(segment_table_body(payload, 0, 0, payload.len(), true).is_none());
}

#[test]
fn segment_tables_type_section_reference_lines() {
    let mut payload = b"segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2\
            type\0\xc0\x80\x01dir\0\xf8\x03\x00\xe5\xe4\
            pointid\0\xf8\x02\xf6\xe4cntrid\0\x00arcorient\0\x00\
            verhor\0\x00radius\0\xf6radius2\0\xf6ext_id\0\x04\
            \xf2\xf7\x01\xe2"
        .to_vec();
    payload.extend_from_slice(&[0x19, 0, 1, 0, 10, 11, 0xf6, 0, 0, 0xf6, 0xf6, 1, 0xe2]);

    let segments = segment_table(&payload, 0, payload.len()).expect("segment table");

    assert!(segments.is_complete());
    assert!(segments.rows.is_empty());
    assert_eq!(segments.point_rows.len(), 1);
    assert_eq!(segments.point_rows[0].point_id, 0);
    assert_eq!(segments.point_rows[0].external_id, 4);
    assert!(segments.opaque_rows.is_empty());
    let [reference] = segments.reference_line_rows.as_slice() else {
        panic!("one reference line");
    };
    assert_eq!(reference.directions, [Some(0), Some(1), Some(0)]);
    assert_eq!(reference.point_ids, [Some(10), Some(11)]);
    assert_eq!(reference.vertical_horizontal, Some(0));
    assert_eq!(reference.external_id, 1);

    let malformed_known = [
        0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 2, 0, 1, 0, 10, 0xf6, 0xf6, 0, 0, 0xf6,
        0xf6, 1, 0xe2,
    ];
    let segments = segment_table_body(&malformed_known, 0, 0, malformed_known.len(), false)
        .expect("malformed known segment table");
    assert!(!segments.is_complete());
    assert!(segments.rows.is_empty());
    assert!(segments.opaque_rows.is_empty());
}

#[test]
fn segment_tables_type_bounded_section_curves() {
    let payload = [
        0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 12, 0, 0, 0, 2, 3, 0xf6, 1, 0, 2, 0xf6,
        22, 0xe2,
    ];

    let segments = segment_table_body(&payload, 0, 0, payload.len(), false)
        .expect("bounded curve segment table");

    assert!(segments.is_complete());
    assert!(segments.opaque_rows.is_empty());
    assert_eq!(
        segments.bounded_curve_rows,
        vec![FeatureBoundedCurveSegment {
            directions: [Some(0); 3],
            point_ids: [2, 3],
            center_id: None,
            arc_orientation: Some(1),
            vertical_horizontal: Some(0),
            radius_ref: Some(2),
            radius2_ref: None,
            external_id: 22,
            offset: 10,
        }]
    );

    let mut missing_endpoint = payload;
    missing_endpoint[14] = 0xf6;
    let segments = segment_table_body(&missing_endpoint, 0, 0, missing_endpoint.len(), false)
        .expect("incomplete bounded curve segment table");
    assert!(segments.is_complete());
    assert!(segments.bounded_curve_rows.is_empty());
    assert_eq!(segments.opaque_rows.len(), 1);
    assert_eq!(segments.opaque_rows[0].kind, 12);
}

#[test]
fn segment_tables_type_complete_circle_rows() {
    let payload = [
        0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 10, 0, 0, 0, 0xf6, 1, 2, 0, 0, 1, 0xf6,
        22, 0xe2,
    ];

    let segments =
        segment_table_body(&payload, 0, 0, payload.len(), false).expect("circle segment table");

    assert!(segments.is_complete());
    assert!(segments.rows.is_empty());
    assert!(segments.opaque_rows.is_empty());
    assert_eq!(
        segments.circle_rows,
        vec![FeatureCircleSegment {
            center_id: 2,
            radius_ref: 1,
            external_id: 22,
            offset: 10,
        }]
    );

    let mut malformed = payload;
    malformed[11] = 1;
    let segments = segment_table_body(&malformed, 0, 0, malformed.len(), false)
        .expect("noncanonical circle segment table");
    assert!(segments.is_complete());
    assert!(segments.circle_rows.is_empty());
    assert_eq!(segments.opaque_rows.len(), 1);
    assert_eq!(segments.opaque_rows[0].kind, 10);
}

#[test]
fn segment_tables_type_saved_conic_rows() {
    let payload = [
        0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 58, 0, 0, 0, 0xf6, 1, 4, 0, 2, 0, 1,
        120, 0xe2,
    ];

    let segments =
        segment_table_body(&payload, 0, 0, payload.len(), false).expect("conic segment table");

    assert!(segments.is_complete());
    assert!(segments.opaque_rows.is_empty());
    assert_eq!(
        segments.conic_rows,
        vec![FeatureConicSegment {
            center_id: 4,
            first_coefficient_ref: 0,
            second_coefficient_ref: 1,
            external_id: 120,
            offset: 10,
        }]
    );

    let mut malformed = payload;
    malformed[18] = 0;
    let segments = segment_table_body(&malformed, 0, 0, malformed.len(), false)
        .expect("noncanonical conic segment table");
    assert!(segments.is_complete());
    assert!(segments.conic_rows.is_empty());
    assert_eq!(segments.opaque_rows.len(), 1);
    assert_eq!(segments.opaque_rows[0].kind, 58);
}

#[test]
fn segment_tables_type_complete_point_rows() {
    let payload = [
        0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 1, 0, 0, 0, 0xf6, 1, 2, 0, 0, 0xf6,
        0xf6, 22, 0xe2,
    ];

    let segments =
        segment_table_body(&payload, 0, 0, payload.len(), false).expect("point segment table");

    assert!(segments.is_complete());
    assert!(segments.rows.is_empty());
    assert!(segments.opaque_rows.is_empty());
    assert_eq!(
        segments.point_rows,
        vec![FeaturePointSegment {
            point_id: 2,
            external_id: 22,
            offset: 10,
        }]
    );

    let mut malformed = payload;
    malformed[19] = 1;
    let segments = segment_table_body(&malformed, 0, 0, malformed.len(), false)
        .expect("noncanonical point segment table");
    assert!(segments.is_complete());
    assert!(segments.point_rows.is_empty());
    assert_eq!(segments.opaque_rows.len(), 1);
    assert_eq!(segments.opaque_rows[0].kind, 1);
}

#[test]
fn segment_tables_type_complete_centered_line_rows() {
    let payload = [
        0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 47, 0, 0, 0, 0xf6, 1, 2, 0, 0, 1, 0xf6,
        22, 0xe2,
    ];

    let segments = segment_table_body(&payload, 0, 0, payload.len(), false)
        .expect("centered line segment table");

    assert!(segments.is_complete());
    assert!(segments.rows.is_empty());
    assert!(segments.opaque_rows.is_empty());
    assert_eq!(
        segments.centered_line_rows,
        vec![FeatureCenteredLineSegment {
            center_id: 2,
            external_id: 22,
            offset: 10,
        }]
    );

    let mut other_type_47 = payload;
    other_type_47[16] = 0;
    let segments = segment_table_body(&other_type_47, 0, 0, other_type_47.len(), false)
        .expect("other type-47 segment table");
    assert!(segments.is_complete());
    assert_eq!(
        segments.centered_line_rows,
        vec![FeatureCenteredLineSegment {
            center_id: 0,
            external_id: 22,
            offset: 10,
        }]
    );
    assert!(segments.opaque_rows.is_empty());

    let mut missing_construction_ref = payload;
    missing_construction_ref[19] = 0xf6;
    let segments = segment_table_body(
        &missing_construction_ref,
        0,
        0,
        missing_construction_ref.len(),
        false,
    )
    .expect("incomplete centered-line segment table");
    assert!(segments.is_complete());
    assert!(segments.centered_line_rows.is_empty());
    assert_eq!(segments.opaque_rows.len(), 1);
    assert_eq!(segments.opaque_rows[0].kind, 47);
    assert_eq!(segments.opaque_rows[0].body, missing_construction_ref[10..]);
}

#[test]
fn segment_rows_expand_compact_slots_and_accept_the_c1_type_wrapper() {
    let payload = [
        0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 0xc1, 0x00, 2, 0xe5, 0xe4, 9, 11, 0xf6,
        3, 0, 0xe6, 0xe2,
    ];

    let segments = segment_table_body(&payload, 0, 0, payload.len(), false).expect("segment table");

    assert!(segments.is_complete());
    assert_eq!(segments.rows.len(), 1);
    assert_eq!(segments.rows[0].kind, FeatureSegmentKind::Line);
    assert_eq!(segments.rows[0].directions, [Some(0), Some(0), Some(1)]);
    assert_eq!(segments.rows[0].point_ids, [9, 11]);
    assert_eq!(segments.rows[0].external_id, 0);
    assert_eq!(
        segments.rows[0].body,
        payload[10..],
        "the retained body includes the optional type wrapper and row close"
    );
}
