use std::collections::BTreeSet;

use super::definitions::*;
use super::entity::*;
use super::operations::*;
use super::rows::*;
use crate::psb;
use crate::scalar;

    #[test]
    fn final_generated_entry_may_terminate_at_the_table_separator() {
        let payload = [10, 0x80, 200, 4, 0, 0xe3, 11, 0x80, 200, 7, 1, 0xf2, 0xf7];
        let entries = read_entries(&payload, 0, 2).expect("complete generated table");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source_entity_id, Some(4));
        assert_eq!(entries[0].end_offset, 6);
        assert_eq!(entries[1].source_entity_id, Some(7));
        assert_eq!(entries[1].end_offset, 11);
    }

    #[test]
    fn generated_table_prototype_uses_its_prefixed_entry_class() {
        let payload = [0xf7, 30, 20, 0xe4, 0xe3, 11, 0x80, 200, 7, 1, 0xe3];
        let entries = read_entries(&payload, 0, 2).expect("prototype and positional entry");

        assert_eq!(entries[0].entity_id, 20);
        assert_eq!(entries[0].class_id, 30);
        assert!(entries[0].prefixed);
        assert_eq!(entries[0].end_offset, 5);
        assert_eq!(entries[1].class_id, 200);
        assert_eq!(entries[1].source_entity_id, Some(7));

        let misplaced = [10, 30, 0, 0xe3, 0xf7, 31, 20, 0xe4, 0xe3];
        assert!(read_entries(&misplaced, 0, 2).is_none());
    }

    #[test]
    fn class_219_generated_entry_retains_its_related_entity() {
        let payload = [0x85, 0xba, 0x80, 0xdb, 0x84, 0x97, 0, 0xe3];
        let entries = read_entries(&payload, 0, 1).expect("class-219 generated entry");

        assert_eq!(entries[0].entity_id, 1466);
        assert_eq!(entries[0].class_id, 219);
        assert_eq!(entries[0].source_entity_id, None);
        assert_eq!(entries[0].related_entity_id, Some(1175));
        assert_eq!(entries[0].related_entity_state, Some(0));
        assert_eq!(entries[0].end_offset, payload.len());
    }

    #[test]
    fn final_class_219_entry_may_terminate_at_the_table_separator() {
        let payload = [0x85, 0xba, 0x80, 0xdb, 0x84, 0x97, 0, 0xf2, 0xf7];
        let entries = read_entries(&payload, 0, 1).expect("terminal class-219 entry");

        assert_eq!(entries[0].related_entity_id, Some(1175));
        assert_eq!(entries[0].end_offset, 7);
    }

    #[test]
    fn class_2017_generated_entry_retains_related_entity_and_state() {
        let payload = [0x92, 0x56, 0x87, 0xe1, 0x92, 0x48, 1, 0xe3];
        let entries = read_entries(&payload, 0, 1).expect("class-2017 generated entry");

        assert_eq!(entries[0].entity_id, 4694);
        assert_eq!(entries[0].class_id, 2017);
        assert_eq!(entries[0].related_entity_id, Some(4680));
        assert_eq!(entries[0].related_entity_state, Some(1));
        assert_eq!(entries[0].end_offset, payload.len());
    }

    #[test]
    fn final_class_2017_entry_may_terminate_at_the_table_separator() {
        let payload = [0x94, 0x92, 0x87, 0xe1, 0x94, 0x90, 1, 0xf2, 0xf7];
        let entries = read_entries(&payload, 0, 1).expect("terminal class-2017 entry");

        assert_eq!(entries[0].related_entity_id, Some(5264));
        assert_eq!(entries[0].related_entity_state, Some(1));
        assert_eq!(entries[0].end_offset, 7);
    }

    #[test]
    fn class_210_generated_entry_retains_its_nonvisible_entity_link() {
        let payload = [0x85, 0xb7, 0x80, 0xd2, 0x85, 0x59, 0, 0xe3];
        let entries = read_entries(&payload, 0, 1).expect("class-210 generated entry");

        assert_eq!(entries[0].entity_id, 1463);
        assert_eq!(entries[0].class_id, 210);
        assert_eq!(entries[0].related_entity_id, Some(1369));
        assert_eq!(entries[0].related_entity_state, Some(0));
    }

    #[test]
    fn class_214_generated_entry_retains_its_related_entity() {
        let payload = [0x85, 0x49, 0x80, 0xd6, 0x80, 0xb8, 0, 0xe3];
        let entries = read_entries(&payload, 0, 1).expect("class-214 generated entry");

        assert_eq!(entries[0].entity_id, 1353);
        assert_eq!(entries[0].class_id, 214);
        assert_eq!(entries[0].related_entity_id, Some(184));
        assert_eq!(entries[0].related_entity_state, Some(0));
    }

    #[test]
    fn choice_fields_ignore_overlapping_headers() {
        let choices = [FeatureChoice {
            feature_id: 7,
            label: "choice".into(),
            type_byte: None,
            payload: vec![psb::token::NAMED_RECORD, psb::token::NAMED_RECORD, b'a', 0],
            payload_offset: 100,
            offset: 90,
        }];

        let fields = choice_fields(&choices);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].offset, 101);
        assert_eq!(fields[0].name, "");
    }

    #[test]
    fn final_procedural_choice_ends_before_post_choice_fields() {
        let body = b"\xe0\x01blend_choice\0\x00\
                     \xe0\x01misc_choice\0\xf8\x05\x00\
                     \xe0\x07assoc_type\0\xf1"
            .to_vec();
        let rows = [FeatureRow {
            feature_id: 7,
            header: [0xeb, 0x04],
            root_schema_class: Some(917),
            stream_offset: 10,
            body,
            body_offset: 100,
            offset: 98,
        }];

        let choices = choices(&rows);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].label, "blend_choice");
        assert_eq!(choices[0].payload, [0]);
        assert_eq!(choices[1].label, "misc_choice");
        assert_eq!(choices[1].payload, [0xf8, 0x05, 0]);
        assert!(choice_fields(&choices).is_empty());
    }

    #[test]
    fn positional_datum_table_replays_the_named_stream_schema() {
        let row = |feature_id, stream_offset, body: Vec<u8>| FeatureRow {
            feature_id,
            header: [0xeb, 0x04],
            root_schema_class: Some(917),
            stream_offset,
            body,
            body_offset: feature_id as usize * 100,
            offset: feature_id as usize * 100 - 2,
        };
        let rows = [
            row(
                1,
                10,
                b"\xe0\x00dtm_id_tab\0\xf2\xf8\x01\xf7\x57\xfb\xe2\
                  \xe0\x01dtm_id\0\x2a\xe0\x01dim_id\0\xf6"
                    .to_vec(),
            ),
            row(
                2,
                10,
                vec![
                    0x00, 0xf8, 0x02, 0xf7, 0x57, 0xfb, 0xe2, 0xf7, 0x58, 0x80, 0x91, 0xf6, 0xf1,
                    0xf7, 0x57, 0xe2, 0x80, 0x92, 0xf6, 0xe3,
                ],
            ),
            row(
                3,
                11,
                vec![
                    0xf8, 0x01, 0xf7, 0x57, 0xfb, 0xe2, 0xf7, 0x58, 0x2b, 0xf6, 0xe3,
                ],
            ),
        ];

        let decoded = geometry_tables(&rows);

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].feature_id, 1);
        assert_eq!(decoded[0].entry_ids, Some(vec![42]));
        assert_eq!(decoded[1].feature_id, 2);
        assert_eq!(decoded[1].count, 2);
        assert_eq!(decoded[1].entity_class, 87);
        assert_eq!(decoded[1].entry_ids, Some(vec![145, 146]));
        assert_eq!(decoded[1].offset, 201);
    }

    #[test]
    fn loop_history_roster_uses_declared_loop_count_and_stored_order() {
        let mut body = b"\xe0\x00lo_id_tab_ptr\0\xf8\x03\xf7\x60\xfb\xe3\
                         \xe0\x01lo_hist\0\xf8\x06"
            .to_vec();
        let first_offset = body.len();
        body.extend_from_slice(&[42, 1, 0xf6, 0xe5, 2, 0xf1, 0xf7, 96, 0xe3]);
        let second_offset = body.len();
        body.extend_from_slice(&[43, 3, 0xf6, 0xe5, 4, 0xe3]);
        let third_offset = body.len();
        body.extend_from_slice(&[44, 5, 6, 0xe4, 0xf6, 7]);
        let named_boundary_offset = body.len();
        body.extend_from_slice(b"\xe0\x00next\0");
        let rows = [FeatureRow {
            feature_id: 7,
            header: [0xeb, 0x04],
            root_schema_class: Some(917),
            stream_offset: 10,
            body,
            body_offset: 1_000,
            offset: 998,
        }];

        let tables = geometry_tables(&rows);
        let entries = loop_history_entries(&rows, &tables);

        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries
                .iter()
                .map(|entry| (
                    entry.feature_id,
                    entry.ordinal,
                    entry.loop_id,
                    entry.offset,
                    entry.end_offset,
                ))
                .collect::<Vec<_>>(),
            vec![
                (7, 0, 42, 1_000 + first_offset, 1_000 + second_offset),
                (7, 1, 43, 1_000 + second_offset, 1_000 + third_offset),
                (
                    7,
                    2,
                    44,
                    1_000 + third_offset,
                    1_000 + named_boundary_offset
                ),
            ]
        );
        assert_eq!(
            entries[0].field_bytes,
            vec![vec![1], vec![0xf6], vec![0xe5], vec![2]]
        );
        assert_eq!(
            entries[0].boundary,
            FeatureLoopHistoryBoundary::ReferenceContinue(96)
        );
        assert_eq!(
            entries[1].boundary,
            FeatureLoopHistoryBoundary::CompoundClose
        );
        assert_eq!(entries[2].field_bytes.len(), 5);
        assert_eq!(entries[2].boundary, FeatureLoopHistoryBoundary::NamedRecord);
    }

    #[test]
    fn loop_history_roster_rejects_incomplete_and_early_boundaries() {
        assert!(loop_history_roster(&[1, 2, 0xe3], 0, 1).is_none());
        assert!(loop_history_roster(&[1, 2, 3, 4, 5, 0xe3], 0, 2).is_none());
        let direct_named = loop_history_roster(b"\x01\x02\xf6\xe5\x03\xe0\x00next\0", 0, 1)
            .expect("direct named boundary");
        assert_eq!(direct_named.len(), 1);
        assert_eq!(direct_named[0].loop_id, 1);
        assert_eq!(direct_named[0].offset, 0);
        assert_eq!(direct_named[0].end_offset, 5);
        assert_eq!(
            direct_named[0].boundary,
            FeatureLoopHistoryBoundary::NamedRecord
        );

        let body = b"\xe0\x00lo_id_tab_ptr\0\xf8\x01\xf7\x60\xfb\xe3\
                     \xe0\x01lo_hist\0\xf8\x05\x2a\xe3"
            .to_vec();
        let rows = [FeatureRow {
            feature_id: 7,
            header: [0xeb, 0x04],
            root_schema_class: Some(917),
            stream_offset: 10,
            body,
            body_offset: 1_000,
            offset: 998,
        }];
        assert!(loop_history_entries(&rows, &geometry_tables(&rows)).is_empty());
    }

    #[test]
    fn entity_graph_requires_the_solid_features_root() {
        let packed_lookalike = b"\xe0\x00SlV\xff\0\xf7\x01";
        assert_eq!(entity_graph(packed_lookalike), (Vec::new(), Vec::new()));

        let payload = b"\xe0\x00Sld_Features\0\xe0\x00first_feat_ptr\0\xf7\x00";
        let (entities, references) = entity_graph(payload);
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].name, "Sld_Features");
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].source_entity_id, Some(1));
        assert!(references[0].target_resolved);
    }

    #[test]
    fn generated_entity_entries_accept_variable_schema_classes() {
        let payload = [
            0xf7, 0x50, 0x0d, 0x80, 0xcc, 0x00, 0xe4, 0xf1, 0xf7, 0x4f, 0xe3, 0x12, 0x80, 0xcb,
            0x00, 0xe4, 0xe3,
        ];

        let entries = read_entries(&payload, 0, 2).expect("generated entity entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].entity_id, 13);
        assert_eq!(entries[0].class_id, 204);
        assert!(entries[0].prefixed);
        assert_eq!(entries[1].entity_id, 18);
        assert_eq!(entries[1].class_id, 203);
        assert!(!entries[1].prefixed);
    }

    fn replay_row(feature_id: u32, operands: &[u8]) -> FeatureRow {
        let mut body = vec![0xf1, 0xf7, 0x42, 0xd8, 0x80, 0x01, 0xe3];
        body.extend_from_slice(operands);
        body.extend_from_slice(&[0xf5, 0x96, 0x92]);
        FeatureRow {
            feature_id,
            header: [0xeb, 0x04],
            root_schema_class: Some(913),
            stream_offset: 100,
            body,
            body_offset: 200,
            offset: 190,
        }
    }

    fn unanchored_replay_row(
        feature_id: u32,
        row_id: u8,
        suffix_reference: Option<u8>,
        operands: &[u8],
    ) -> FeatureRow {
        let mut row = replay_row(feature_id, operands);
        row.body.clear();
        row.body.push(psb::token::COMPOUND_CLOSE);
        row.body.extend_from_slice(operands);
        row.body
            .extend_from_slice(&[0xe1, 0xe1, row_id, psb::token::COMPOUND_CLOSE]);
        if let Some(reference) = suffix_reference {
            row.body.extend_from_slice(&[
                psb::token::ENTITY_REF,
                reference,
                psb::token::COMPOUND_CLOSE,
            ]);
        } else {
            row.body.push(psb::token::COMPOUND_CLOSE);
        }
        row.body
            .extend_from_slice(&[3, row_id, 0x00, 0xe1, 0x00, psb::token::COMPOUND_CLOSE]);
        row
    }

    fn surface_merge_row(feature_id: u32, row_id: u8, operands: &[u8]) -> FeatureRow {
        let mut body = vec![
            psb::token::COMPOUND_CLOSE,
            psb::token::ENTITY_REF,
            0x80,
            0x96,
            psb::token::ARRAY_OPEN,
            1,
            99,
            0x01,
            psb::token::COMPOUND_CLOSE,
        ];
        body.extend_from_slice(operands);
        body.extend_from_slice(&[
            0xe1,
            0xe1,
            row_id,
            psb::token::COMPOUND_CLOSE,
            psb::token::COMPOUND_CLOSE,
            3,
            row_id,
            0x00,
            0xe1,
            0x00,
            psb::token::COMPOUND_CLOSE,
        ]);
        FeatureRow {
            feature_id,
            header: [0xeb, 0x04],
            root_schema_class: Some(946),
            stream_offset: 100,
            body,
            body_offset: 200,
            offset: 190,
        }
    }

    #[test]
    fn positional_surface_merge_replay_inherits_geometry_edge_and_quilt_extents() {
        let rows = [
            surface_merge_row(
                1,
                40,
                &[
                    0xf8, 2, 10, 11, 0xf8, 2, 20, 21, 0xf0, 0xf7, 0x80, 0x99, 0xf8, 2, 30, 31,
                ],
            ),
            surface_merge_row(2, 41, &[12, 13, 22, 23, 0xf0, 0xf7, 0x80, 0x99, 32, 33]),
        ];

        let decoded = surface_merge_replay_affected_ids(&rows, &[]);

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].geometry_ids, [10, 11]);
        assert_eq!(decoded[0].edge_ids, [20, 21]);
        assert_eq!(decoded[0].quilt_ids, [30, 31]);
        assert_eq!(decoded[1].geometry_ids, [12, 13]);
        assert_eq!(decoded[1].edge_ids, [22, 23]);
        assert_eq!(decoded[1].quilt_ids, [32, 33]);
        assert_eq!(decoded[1].geometry_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[1].edge_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[1].quilt_extent, ReplayExtentSource::Inherited);
    }

    #[test]
    fn named_surface_merge_arrays_seed_positional_replay_extents() {
        let rows = [
            surface_merge_row(1, 40, &[]),
            surface_merge_row(2, 41, &[12, 13, 22, 23, 0xf0, 0xf7, 0x80, 0x99, 32, 33]),
        ];
        let named = [
            FeatureAffectedIds {
                feature_id: 1,
                kind: AffectedIdKind::Geometry,
                ids: vec![10, 11],
                offset: 1,
            },
            FeatureAffectedIds {
                feature_id: 1,
                kind: AffectedIdKind::Edges,
                ids: vec![20, 21],
                offset: 2,
            },
            FeatureAffectedIds {
                feature_id: 1,
                kind: AffectedIdKind::Quilts,
                ids: vec![30, 31],
                offset: 3,
            },
        ];

        let decoded = surface_merge_replay_affected_ids(&rows, &named);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].feature_id, 2);
        assert_eq!(decoded[0].geometry_ids, [12, 13]);
        assert_eq!(decoded[0].edge_ids, [22, 23]);
        assert_eq!(decoded[0].quilt_ids, [32, 33]);
        assert_eq!(decoded[0].geometry_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[0].edge_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[0].quilt_extent, ReplayExtentSource::Inherited);
    }

    #[test]
    fn positional_round_replay_inherits_each_array_extent() {
        let mut rows = [
            replay_row(1, &[0xf8, 2, 10, 11, 0xf7, 42, 0xf8, 3, 20, 21, 22]),
            replay_row(2, &[12, 13, 23, 24, 25]),
            replay_row(3, &[0xf8, 1, 14, 26, 27, 28]),
        ];
        rows[0].body[3] = 0xc8;

        let decoded = replay_affected_ids(&rows);

        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].geometry_ids, vec![10, 11]);
        assert_eq!(decoded[0].edge_ids, vec![20, 21, 22]);
        assert_eq!(decoded[1].geometry_ids, vec![12, 13]);
        assert_eq!(decoded[1].edge_ids, vec![23, 24, 25]);
        assert_eq!(decoded[1].geometry_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[1].edge_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[2].geometry_ids, vec![14]);
        assert_eq!(decoded[2].edge_ids, vec![26, 27, 28]);
        assert_eq!(decoded[2].geometry_extent, ReplayExtentSource::Explicit);
        assert_eq!(decoded[2].edge_extent, ReplayExtentSource::Inherited);
    }

    #[test]
    fn positional_round_replay_uses_repeated_row_id_suffix() {
        let rows = [
            unanchored_replay_row(1, 40, None, &[0xf8, 2, 10, 11, 0xf8, 2, 20, 21]),
            unanchored_replay_row(2, 41, None, &[12, 13, 22, 23]),
        ];

        let decoded = replay_affected_ids(&rows);

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].geometry_ids, vec![10, 11]);
        assert_eq!(decoded[0].edge_ids, vec![20, 21]);
        assert_eq!(decoded[1].geometry_ids, vec![12, 13]);
        assert_eq!(decoded[1].edge_ids, vec![22, 23]);
        assert_eq!(decoded[1].geometry_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[1].edge_extent, ReplayExtentSource::Inherited);
    }

    #[test]
    fn positional_chamfer_replay_uses_referenced_row_id_suffix() {
        let mut row = unanchored_replay_row(1, 40, Some(74), &[0xf8, 2, 10, 11, 0xf8, 2, 20, 21]);
        row.root_schema_class = Some(914);

        let decoded = replay_affected_ids(&[row]);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].geometry_ids, vec![10, 11]);
        assert_eq!(decoded[0].edge_ids, vec![20, 21]);
    }

    #[test]
    fn positional_round_replay_uses_final_explicit_arrays_before_row_suffix() {
        let mut row = unanchored_replay_row(
            1,
            40,
            None,
            &[
                0xf8, 3, 1, 2, 3, 0xf1, 0xf7, 54, 1, 0xe3, 0xf7, 0x80, 0x97, 0xf8, 2, 10, 11, 0xf1,
                0xf7, 56,
            ],
        );
        row.body.splice(1..1, [0xf8, 4, 30, 31, 32, 33]);

        let decoded = replay_affected_ids(&[row]);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].geometry_ids, vec![1, 2, 3]);
        assert_eq!(decoded[0].edge_ids, vec![10, 11]);
        assert_eq!(decoded[0].geometry_extent, ReplayExtentSource::Explicit);
        assert_eq!(decoded[0].edge_extent, ReplayExtentSource::Explicit);
    }

    #[test]
    fn positional_round_replay_accepts_null_row_tail() {
        let mut row = unanchored_replay_row(
            1,
            40,
            Some(74),
            &[0xf8, 2, 10, 11, 0xf0, 0xf7, 75, 0xf8, 2, 20, 21],
        );
        *row.body.last_mut().expect("suffix tail") = 0xe1;

        let decoded = replay_affected_ids(&[row]);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].geometry_ids, vec![10, 11]);
        assert_eq!(decoded[0].edge_ids, vec![20, 21]);
    }

    #[test]
    fn radius_dimension_type_uses_model_length_units() {
        assert_eq!(dimension_unit(0x03), DimensionUnit::Millimeters);
    }

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
            feature_id: Some(owner),
            table_class_id: 80,
            entry_ids: Vec::new(),
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
                })
                .collect(),
            surface_ids: Vec::new(),
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        }
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

    fn operation(
        feature_id: u32,
        recipe: Option<FeatureRecipe>,
        offset: usize,
    ) -> FeatureOperation {
        FeatureOperation {
            feature_id,
            kind: String::new(),
            display_name_stored: false,
            stored_name: None,
            stored_name_bytes: None,
            identifier_keyword: None,
            stored_name_prefix: None,
            recipe,
            root_schema_class: None,
            parent_feature_id: None,
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
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        });
        let operations = [
            operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
            operation(248, None, 20),
        ];

        bind_depdb_section_owners(
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
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        });
        let operations = [
            operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
            operation(248, None, 20),
        ];

        bind_depdb_section_owners(
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
            0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00,
            0x00, 0x00, 0x00,
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
            0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00,
            0x00, 0x00, 0x00,
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
        bind_depdb_section_owners(&mut repeated, &consecutive, &[(0, usize::MAX)]);
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
        bind_depdb_section_owners(
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
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        });
        let mut definitions = [claimed, candidate];
        bind_depdb_section_owners(&mut definitions, &consecutive, &[(0, usize::MAX)]);
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

        let segments =
            positional_segment_table(&payload, 0, payload.len()).expect("positional segtab");

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

        let segments =
            positional_segment_table(&payload, 0, payload.len()).expect("positional segtab");

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
            0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 2, 0, 1, 0, 10, 0xf6, 0xf6, 0, 0,
            0xf6, 0xf6, 1, 0xe2,
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
            0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 12, 0, 0, 0, 2, 3, 0xf6, 1, 0, 2,
            0xf6, 22, 0xe2,
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
            0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 10, 0, 0, 0, 0xf6, 1, 2, 0, 0, 1,
            0xf6, 22, 0xe2,
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
            0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 47, 0, 0, 0, 0xf6, 1, 2, 0, 0, 1,
            0xf6, 22, 0xe2,
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
            0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 0xc1, 0x00, 2, 0xe5, 0xe4, 9, 11,
            0xf6, 3, 0, 0xe6, 0xe2,
        ];

        let segments =
            segment_table_body(&payload, 0, 0, payload.len(), false).expect("segment table");

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

    #[test]
    fn positional_dimension_table_uses_the_inherited_table_class() {
        let mut payload = b"prefix\xf8\x02\xf7\x58\xfb\xe2\xf7\x59".to_vec();
        payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
        payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
        payload.extend_from_slice(&[10, 0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f, 0, 0x18, 44]);
        let cache = scalar::ScalarCache::from_section(&payload);

        let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
            .expect("positional dimtab");

        assert_eq!(dimensions.declared_count, 2);
        assert_eq!(dimensions.entity_ref, Some(88));
        assert_eq!(dimensions.rows.len(), 2);
        assert_eq!(dimensions.rows[0].value, Some(3.0));
        assert_eq!(
            dimensions.rows[0].value_body,
            [0x46, 0x08, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(dimensions.rows[0].auxiliary_body, [0x18]);
        assert_eq!(dimensions.rows[0].external_id, 43);
        assert_eq!(dimensions.rows[1].dimension_type, 10);
        assert_eq!(
            dimensions.rows[1].value_body,
            [0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f]
        );
        assert_eq!(dimensions.rows[1].external_id, 44);
    }

    #[test]
    fn positional_dimension_table_is_self_describing_when_multiple_rows_close() {
        let mut payload = b"prefix\xf8\x04\xf7\x58\xfb\xe2\xf7\x59".to_vec();
        for (index, row) in [
            [1, 0xe4, 0, 0x18, 2],
            [2, 0x0e, 0, 0x18, 0],
            [2, 0xe4, 0, 0x18, 3],
            [2, 0xe4, 0, 0x18, 1],
        ]
        .into_iter()
        .enumerate()
        {
            payload.extend_from_slice(&row);
            if index < 3 {
                payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
            }
        }
        let cache = scalar::ScalarCache::from_section(&payload);

        let dimensions =
            self_described_positional_dimension_table(&payload, 0, payload.len(), &cache)
                .expect("self-described dimension table");

        assert_eq!(dimensions.entity_ref, Some(88));
        assert_eq!(dimensions.rows.len(), 4);
        assert_eq!(dimensions.rows[0].external_id, 2);
        assert_eq!(dimensions.rows[1].value, Some(-0.5));
    }

    #[test]
    fn one_row_positional_table_does_not_self_identify_as_dimensions() {
        let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\x01\xe4\x00\x18\x02";
        assert_eq!(
            self_described_positional_dimension_table(
                payload,
                0,
                payload.len(),
                &scalar::ScalarCache::default(),
            ),
            None
        );
    }

    #[test]
    fn positional_dimension_table_retains_bounded_opaque_values() {
        let mut payload = b"prefix\xf8\x03\xf7\x58\xfb\xe2\xf7\x59".to_vec();
        payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
        payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
        payload.extend_from_slice(&[1, 0x00, 0x04, 0xa6, 0, 0x18, 44]);
        payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
        payload.extend_from_slice(&[5, 0x0d, 0, 0x18, 45]);
        let cache = scalar::ScalarCache::from_section(&payload);

        let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
            .expect("positional dimtab");

        assert_eq!(dimensions.rows.len(), 3);
        assert_eq!(dimensions.rows[1].value, None);
        assert_eq!(
            dimensions.rows[1].unresolved_value_token.as_deref(),
            Some(&[0x00, 0x04, 0xa6][..])
        );
        assert_eq!(dimensions.rows[1].value_body, [0x00, 0x04, 0xa6]);
        assert_eq!(dimensions.rows[1].auxiliary_body, [0x18]);
        assert_eq!(dimensions.rows[1].external_id, 44);
        assert_eq!(dimensions.rows[2].value, Some(-1.0));
        assert_eq!(dimensions.rows[2].external_id, 45);
    }

    #[test]
    fn positional_dimensions_decode_the_positive_dict_lattice_and_bounded_opaque_forms() {
        let positive = [1, 0x53, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f, 0, 0x18, 46];
        let opaque_three = [1, 0x00, 0x04, 0xa6, 0, 0x18, 47];
        let opaque_four = [1, 0x01, 0x04, 0xfe, 0xf2, 0, 0x18, 48];
        let zero = [2, 0x18, 0, 0x18, 49];
        let negative_half = [1, 0x0e, 0, 0x18, 50];
        let cache = scalar::ScalarCache::default();

        let positive_row = positional_dimension(&positive, 0, positive.len(), &cache)
            .expect("positive dictionary dimension");
        assert_eq!(
            positive_row.value,
            Some(f64::from_be_bytes([
                0x3f, 0xc8, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f,
            ]))
        );
        assert_eq!(positive_row.direction_byte, 0);
        assert_eq!(positive_row.auxiliary_value, Some(0.0));
        assert_eq!(positive_row.value_body, positive[1..8]);
        assert_eq!(positive_row.auxiliary_body, [0x18]);
        assert_eq!(positive_row.external_id, 46);
        for (body, external_id, token) in [
            (&opaque_three[..], 47, &[0x00, 0x04, 0xa6][..]),
            (&opaque_four[..], 48, &[0x01, 0x04, 0xfe, 0xf2][..]),
        ] {
            let row = positional_dimension(body, 0, body.len(), &cache)
                .expect("bounded opaque dimension");
            assert_eq!(row.value, None);
            assert_eq!(row.unresolved_value_token.as_deref(), Some(token));
            assert_eq!(row.external_id, external_id);
        }
        let zero_row = positional_dimension(&zero, 0, zero.len(), &cache).expect("zero dimension");
        assert_eq!(zero_row.value, Some(0.0));
        assert_eq!(zero_row.external_id, 49);
        let negative_half_row =
            positional_dimension(&negative_half, 0, negative_half.len(), &cache)
                .expect("negative half dimension");
        assert_eq!(negative_half_row.value, Some(-0.5));
        assert_eq!(negative_half_row.external_id, 50);
    }

    #[test]
    fn positional_dimension_seven_byte_positive_value_preserves_field_alignment() {
        let body = [2, 0x31, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0, 0x18, 27];
        let row = positional_dimension(&body, 0, body.len(), &scalar::ScalarCache::default())
            .expect("seven-byte positive dimension");

        assert_eq!(
            row.value,
            Some(f64::from_be_bytes([
                0x40, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0,
            ]))
        );
        assert_eq!(row.direction_byte, 0);
        assert_eq!(row.auxiliary_value, Some(0.0));
        assert_eq!(row.external_id, 27);
    }

    #[test]
    fn dimension_tables_retain_extents_without_decoded_rows() {
        let named = b"dimtab_ptr\0\xf8\x02\xf7\x58\xfb\xe2";
        let cache = scalar::ScalarCache::from_section(named);
        let dimensions =
            dimension_table(named, 0, named.len(), &cache).expect("named dimtab header");
        assert_eq!(dimensions.declared_count, 2);
        assert_eq!(dimensions.entity_ref, Some(88));
        assert!(dimensions.rows.is_empty());

        let positional = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59";
        let cache = scalar::ScalarCache::from_section(positional);
        let dimensions = positional_dimension_table(positional, 0, positional.len(), 88, &cache)
            .expect("positional dimtab header");
        assert_eq!(dimensions.declared_count, 2);
        assert_eq!(dimensions.entity_ref, Some(88));
        assert!(dimensions.rows.is_empty());
    }

    #[test]
    fn positional_definition_inherits_the_labeled_dimension_table_class() {
        let mut payload = b"feat_defs_917\0dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe0\x01feat_id\0\x2a\xe0\x00ref_model_info\0\xe3S2D0004\0\
            \xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
            .to_vec();
        payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

        let decoded = definitions(&payload);
        let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

        assert_eq!(decoded[1].owner_feature_id, Some(42));
        assert_eq!(dimensions.entity_ref, Some(88));
        assert_eq!(dimensions.rows.len(), 1);
        assert_eq!(dimensions.rows[0].value, Some(3.0));
        assert_eq!(dimensions.rows[0].external_id, 43);
    }

    #[test]
    fn depdb_gsec2d_definition_anchors_positional_table_replay() {
        let mut payload = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0\
            dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe3S2D0003\0\xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
            .to_vec();
        payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

        let decoded = depdb_definitions(&payload);
        let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].id, 2);
        assert_eq!(decoded[1].id, 2);
        assert!(decoded
            .iter()
            .all(|definition| definition.owner_feature_id.is_none()));
        assert_eq!(dimensions.entity_ref, Some(88));
        assert_eq!(dimensions.rows.len(), 1);
        assert_eq!(dimensions.rows[0].value, Some(3.0));
        assert_eq!(dimensions.rows[0].external_id, 43);
    }

    #[test]
    fn positional_variable_table_joins_coordinate_rows() {
        let payload = b"prefix\xf8\x02\xf7\x77\xfb\xe2\xf7\x78\
            \x01\x07\x18\x18\x01\x00\x09\xf1\xf7\x77\xe2\
            \x02\x07\x18\x18\x01\x00\x0a";
        let cache = scalar::ScalarCache::from_section(payload);

        let variables = positional_variable_table(payload, 0, payload.len(), 119, &cache)
            .expect("positional var_arr");

        assert_eq!(variables.declared_count, 2);
        assert_eq!(variables.entity_ref, Some(119));
        assert_eq!(variables.rows.len(), 2);
        assert!(variables.is_complete());
        assert_eq!(variables.rows[0].value_body, [0x18]);
        assert_eq!(variables.rows[0].guess_body, [0x18]);
        assert_eq!(variables.rows[0].guess, Some(0.0));
        assert_eq!(variables.rows[0].known, Some(1));
        assert_eq!(variables.rows[0].homogeneity, Some(0));
        assert_eq!(variables.rows[0].uvar_id, Some(9));
        assert_eq!(variables.rows[1].guess, Some(0.0));
        assert_eq!(variables.rows[1].known, Some(1));
        assert_eq!(variables.rows[1].homogeneity, Some(0));
        assert_eq!(variables.rows[1].uvar_id, Some(10));
        assert_eq!(variables.points.len(), 1);
        assert_eq!(variables.points[0].point_id, 7);
        assert_eq!(variables.points[0].u, Some(0.0));
        assert_eq!(variables.points[0].v, Some(0.0));
    }

    #[test]
    fn positional_variable_guess_zero_preserves_compact_trailing_fields_at_table_boundary() {
        let payload = b"prefix\xf8\x02\xf7\x77\xfb\xe2\xf7\x78\
            \x07\x00\x18\x18\x01\x01\x0f\xf1\xf7\x77\xe2\
            \x07\x01\x18\x18\x00\x01\x07\xf2next_table\0";
        let cache = scalar::ScalarCache::from_section(payload);

        let variables = positional_variable_table(payload, 0, payload.len(), 119, &cache)
            .expect("positional var_arr");

        assert!(variables.is_complete());
        assert_eq!(variables.rows.len(), 2);
        assert_eq!(variables.rows[0].guess, Some(0.0));
        assert_eq!(variables.rows[0].known, Some(1));
        assert_eq!(variables.rows[0].homogeneity, Some(1));
        assert_eq!(variables.rows[0].uvar_id, Some(15));
        assert_eq!(variables.rows[1].guess, Some(0.0));
        assert_eq!(variables.rows[1].known, Some(0));
        assert_eq!(variables.rows[1].homogeneity, Some(1));
        assert_eq!(variables.rows[1].uvar_id, Some(7));
    }

    #[test]
    fn variable_tables_retain_extents_without_decoded_rows() {
        let named = b"var_arr\0\xf8\x02\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2";
        let cache = scalar::ScalarCache::from_section(named);
        let variables =
            variable_table(named, 0, named.len(), &cache).expect("named var_arr header");
        assert_eq!(variables.declared_count, 2);
        assert_eq!(variables.entity_ref, Some(119));
        assert!(variables.rows.is_empty());
        assert!(variables.points.is_empty());
        assert!(!variables.is_complete());

        let positional = b"\xf8\x02\xf7\x77\xfb\xe2\xf7\x78";
        let cache = scalar::ScalarCache::from_section(positional);
        let variables = positional_variable_table(positional, 0, positional.len(), 119, &cache)
            .expect("positional var_arr header");
        assert_eq!(variables.declared_count, 2);
        assert_eq!(variables.entity_ref, Some(119));
        assert!(variables.rows.is_empty());
        assert!(variables.points.is_empty());
        assert!(!variables.is_complete());
    }

    #[test]
    fn variable_table_withholds_duplicate_coordinate_identities() {
        let row = |variable_type, value, offset| FeatureVariableRow {
            variable_type,
            key: 7,
            value: Some(value),
            value_body: Vec::new(),
            guess: None,
            guess_body: Vec::new(),
            guess_dimension_driven: false,
            known: None,
            homogeneity: None,
            uvar_id: None,
            dimension_driven: false,
            offset,
        };
        let table = variable_table_from_rows(
            3,
            Some(119),
            vec![row(1, 2.0, 10), row(1, 2.0, 20), row(2, 3.0, 30)],
            5,
        );

        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.points.len(), 1);
        assert_eq!(table.points[0].point_id, 7);
        assert_eq!(table.points[0].u, None);
        assert_eq!(table.points[0].v, Some(3.0));
    }

    #[test]
    fn radius_variables_do_not_create_section_points() {
        let row = |variable_type, key, value, offset| FeatureVariableRow {
            variable_type,
            key,
            value: Some(value),
            value_body: Vec::new(),
            guess: None,
            guess_body: Vec::new(),
            guess_dimension_driven: false,
            known: None,
            homogeneity: None,
            uvar_id: None,
            dimension_driven: false,
            offset,
        };
        let table = variable_table_from_rows(
            3,
            Some(119),
            vec![row(1, 7, 2.0, 10), row(2, 7, 3.0, 20), row(3, 99, 4.0, 30)],
            5,
        );

        assert_eq!(table.points.len(), 1);
        assert_eq!(table.points[0].point_id, 7);
        let (points, ambiguous) = table.reconciled_points();
        assert_eq!(points.get(&7), Some(&[Some(2.0), Some(3.0)]));
        assert!(!points.contains_key(&99));
        assert!(ambiguous.is_empty());
    }

    #[test]
    fn variable_coordinate_7e_and_c6_are_the_f3_dict_sign_pair() {
        let positive = [0x7e, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
        let negative = [0xc6, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
        let cache = scalar::ScalarCache::from_section(&positive);

        assert_eq!(
            decode_variable_scalar(&positive, 0, positive.len(), &cache),
            (
                Some(f64::from_be_bytes([
                    0x3f, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
                ])),
                7,
                false
            )
        );
        assert_eq!(
            decode_variable_scalar(&negative, 0, negative.len(), &cache),
            (
                Some(f64::from_be_bytes([
                    0xbf, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
                ])),
                7,
                false
            )
        );
    }

    #[test]
    fn positional_gsec3d_decodes_placement_and_reference_rows() {
        let payload = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
            \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
            \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\
            \x07\x05\xf6\x04\xf6\x01";

        let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

        assert_eq!(section.sketch_plane_entity_id, Some(513));
        assert_eq!(section.sketch_plane_flip, None);
        assert_eq!(section.reference_plane_entity_ids, vec![6, 7]);
        assert_eq!(section.reference_plane_datum_geometry_id, None);
        assert_eq!(section.orientation.section_flip, Some(BinaryFlag::Set));
        assert_eq!(section.orientation.reference_type, Some(5));
        assert_eq!(section.orientation.segment_id, Some(3));
        assert_eq!(section.orientation.reference_flip, Some(BinaryFlag::Clear));
    }

    #[test]
    fn positional_gsec3d_retains_its_header_without_a_body() {
        let payload = b"prefix\x07S2D0004\0";

        let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

        assert_eq!(section.offset, 6);
        assert_eq!(section.sketch_plane_entity_id, None);
        assert!(section.reference_plane_entity_ids.is_empty());
        assert_eq!(section.orientation, FeatureSectionOrientation::default());
    }

    #[test]
    fn positional_gsec3d_retains_placement_and_complete_reference_prefix() {
        let payload = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
            \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
            \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\x07";

        let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

        assert_eq!(section.sketch_plane_entity_id, Some(513));
        assert_eq!(section.reference_plane_entity_ids, [6]);
        assert_eq!(section.orientation.section_flip, Some(BinaryFlag::Set));
        assert_eq!(section.orientation.reference_type, Some(5));
        assert_eq!(section.orientation.segment_id, Some(3));
        assert_eq!(section.orientation.reference_flip, Some(BinaryFlag::Clear));
    }

    #[test]
    fn positional_relation_table_replays_rows_after_its_prototype() {
        let payload = b"prefix\xf8\x03\xf7\x64\xfb\xe2\xf7\x65\
            prototype\xf1\xf7\x64\xe2\
            \x08\x00\x03\x0f\xf6\xe4\x01\xe4\x00\xe4\x0f\x10\x0f\x18\x00\xf6\x00\xe2";

        let relations = positional_relation_table(payload, 0, payload.len(), 100)
            .expect("positional relat_ptr");

        assert_eq!(relations.declared_count, 3);
        assert_eq!(relations.entity_ref, Some(100));
        assert_eq!(relations.rows.len(), 1);
        assert_eq!(relations.rows[0].relation_id, 8);
        assert_eq!(relations.rows[0].used, 0);
        assert_eq!(relations.rows[0].sign, 0);
        assert_eq!(relations.rows[0].dimension_id, 246);
        assert_eq!(relations.rows[0].relation_type, 0);
        assert!(relations.rows[0].operand_vectors.is_some());
    }

    #[test]
    fn relation_table_retains_solver_children_after_an_invalid_row() {
        let payload = b"relat_ptr\0\xf4\x04\xf8\x03\xf7\x6a\xfb\xe2\
            schema\xf1\xf7\x6a\xe2invalid\
            skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2";

        let relations = relation_table(payload, 0, payload.len()).expect("relat_ptr header");

        assert_eq!(relations.declared_count, 3);
        assert_eq!(relations.entity_ref, Some(106));
        assert!(relations.rows.is_empty());
        assert_eq!(relations.skamps.len(), 1);
        assert_eq!(relations.skamps[0].id, 5);
    }

    #[test]
    fn relation_tables_retain_extents_without_their_prototypes() {
        let named = b"relat_ptr\0\xf8\x03\xf7\x64\xfb\xe2";
        let relations = relation_table(named, 0, named.len()).expect("named relat_ptr header");
        assert_eq!(relations.declared_count, 3);
        assert_eq!(relations.entity_ref, Some(100));
        assert!(relations.rows.is_empty());

        let positional = b"\xf8\x03\xf7\x64\xfb\xe2";

        let relations = positional_relation_table(positional, 0, positional.len(), 100)
            .expect("positional relat_ptr header");

        assert_eq!(relations.declared_count, 3);
        assert_eq!(relations.entity_ref, Some(100));
        assert!(relations.rows.is_empty());
    }

    #[test]
    fn positional_skamp_table_replays_counted_nested_items() {
        let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2\
            \x02\x01\xea\x22\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

        let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

        assert_eq!(skamps.len(), 2);
        assert_eq!(skamps[0].id, 1);
        assert_eq!(skamps[0].kind, 0);
        assert_eq!(skamps[0].items.len(), 2);
        assert_eq!(skamps[0].items[0].entity_id, 6);
        assert_eq!(skamps[0].items[1].sense, 2);
        assert_eq!(skamps[1].kind, 1);
        assert_eq!(skamps[1].flags, 34);
        assert_eq!(skamps[1].status, 35);
        assert_eq!(skamps[1].items[0].entity_id, 8);
    }

    #[test]
    fn positional_solver_tables_retain_complete_prefix_rows() {
        let skamps = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2";
        let rows = positional_feature_skamps(skamps, 0, skamps.len(), 88);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 1);

        let triples = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2";
        let rows = positional_relation_triples(triples, 0, triples.len(), 100);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].relation_id, Some(1));
    }

    #[test]
    fn solver_header_does_not_adopt_a_later_array() {
        let payload = b"skamp_ptr\0opaque\xf8\x02\xf7\x58\xfb\xe2";

        assert!(named_solver_table_header(payload, b"skamp_ptr\0", 0, payload.len()).is_none());
    }

    #[test]
    fn named_solver_tables_retain_complete_prefix_rows() {
        let skamps = b"skamp_ptr\0\xf3\xf8\x02\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2invalid";
        let rows = feature_skamps(skamps, 0, skamps.len());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 5);

        let triples = b"triples_ptr\0\xf4\x04\xf8\x02\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2\x01\x02\x03";
        let rows = feature_relation_triples(triples, 0, triples.len());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].relation_id, Some(7));
    }

    #[test]
    fn positional_definition_preserves_its_named_solver_tables() {
        let solver_tables = b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2\
            triples_ptr\0\xf4\x04\xf8\x01\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2";
        let mut payload =
            b"relat_ptr\0\xf4\x04\xf8\x02\xf7\x6a\xfb\xe2schema\xf1\xf7\x6a\xe2".to_vec();
        payload.extend_from_slice(solver_tables);
        let positional_start = payload.len();
        payload.extend_from_slice(solver_tables);
        payload.extend_from_slice(b"\xf8\x02\xf7\x6a\xfb\xe2");
        let prototype_offset = payload.len() + 3;
        assert!((128..=16_383).contains(&prototype_offset));
        payload.extend_from_slice(&[
            psb::token::ENTITY_REF,
            0x80 + u8::try_from(prototype_offset >> 8).expect("prototype offset high byte"),
            u8::try_from(prototype_offset & 0xff).expect("prototype offset low byte"),
        ]);
        payload.extend_from_slice(b"\xf1\xf7\x6a\xe2");

        let definitions = definitions_in_ranges(
            &payload,
            &[(0, 1, None, false), (positional_start, 2, None, true)],
        );
        let relations = definitions[1].relations.as_ref().expect("relations");

        assert_eq!(relations.skamps.len(), 1);
        assert_eq!(relations.skamps[0].id, 5);
        assert_eq!(
            relations
                .skamp_header
                .as_ref()
                .expect("skamp header")
                .declared_count,
            1
        );
        assert_eq!(relations.triples.len(), 1);
        assert_eq!(relations.triples[0].relation_id, Some(7));
        assert_eq!(
            relations
                .triples_header
                .as_ref()
                .expect("triples header")
                .declared_count,
            1
        );
    }

    #[test]
    fn positional_triples_replay_nullable_relation_joins() {
        let payload = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2\x02\xf6\x05";

        let triples = positional_relation_triples(payload, 0, payload.len(), 100);

        assert_eq!(triples.len(), 2);
        assert_eq!(triples[0].relation_id, Some(1));
        assert_eq!(triples[0].equation_id, None);
        assert_eq!(triples[0].skamp_id, Some(4));
        assert_eq!(triples[1].relation_id, Some(2));
        assert_eq!(triples[1].skamp_id, Some(5));
    }

    #[test]
    fn positional_trim_entity_table_decodes_without_segments() {
        let payload = b"prefix\xf8\x07\xf7\x42\xfb\xe2\xf7\x43\x00\xe3\
            \x09\x00\x03\x04\xf6\x00\
            \xf4\x04\xf7\x42\xe2\x01\xf8\x13\xf7\x44\xfb\xe2";
        let entities = positional_trim_entity_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 67,
            },
            Some(68),
        )
        .expect("positional ent_tab");

        assert_eq!(entities.declared_count, Some(7));
        assert_eq!(entities.entity_ref, Some(66));
        assert_eq!(entities.entry_ref, Some(67));
        assert_eq!(entities.solved_external_ids, vec![9]);
        assert_eq!(entities.rows[0].vertices, [3, 4]);
        assert_eq!(entities.rows[0].kind, TrimEntityKind::Line);
    }

    #[test]
    fn positional_trim_entity_table_retains_an_empty_extent() {
        let payload = b"prefix\xf8\x00\xf7\x42\xfb\xe2\
            \xf8\x01\xf7\x44\xfb\xe2";

        let entities = positional_trim_entity_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 67,
            },
            Some(68),
        )
        .expect("empty positional ent_tab");

        assert_eq!(entities.declared_count, Some(0));
        assert_eq!(entities.entity_ref, Some(66));
        assert_eq!(entities.entry_ref, Some(67));
        assert!(entities.rows.is_empty());
        assert!(entities.solved_external_ids.is_empty());
    }

    #[test]
    fn positional_trim_entity_table_withholds_rows_without_the_entry_class() {
        let payload = b"prefix\xf8\x01\xf7\x42\xfb\xe2\
            \x00\xe3\x09\x00\x03\x04\xf6\x00";

        let entities = positional_trim_entity_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 67,
            },
            None,
        )
        .expect("positional ent_tab header");

        assert_eq!(entities.declared_count, Some(1));
        assert!(entities.rows.is_empty());
        assert!(entities.solved_external_ids.is_empty());
    }

    #[test]
    fn positional_order_table_replays_prototype_and_following_rows() {
        let payload = b"prefix\xf8\x03\xf7\x42\xfb\xe2\xf7\x43\
            \x09\x01\x00\xf1\xf7\x42\xe2\
            \x0a\x02\x01\xe2\x0b\x03\x00";

        let order =
            positional_order_table(payload, 0, payload.len(), 66).expect("positional order_table");

        assert_eq!(order.declared_count, 3);
        assert!(order.has_prototype);
        assert!(order.is_complete());
        assert_eq!(order.entity_ref, Some(66));
        assert_eq!(order.rows.len(), 2);
        assert_eq!(order.rows[0].external_id, 10);
        assert_eq!(order.rows[0].internal_id, 2);
        assert_eq!(order.rows[0].bitmask, 1);
        assert_eq!(order.rows[1].external_id, 11);
        assert_eq!(order.internal_id(10), Some(2));
        assert_eq!(order.external_id(2), Some(10));

        let mut duplicate_external = order.clone();
        duplicate_external.declared_count += 1;
        duplicate_external.rows.push(FeatureOrderRow {
            external_id: 10,
            internal_id: 4,
            bitmask: 0,
            offset: 20,
        });
        assert_eq!(duplicate_external.internal_id(10), None);
        assert_eq!(duplicate_external.external_id(2), None);
        let mut duplicate_internal = order;
        duplicate_internal.declared_count += 1;
        duplicate_internal.rows.push(FeatureOrderRow {
            external_id: 12,
            internal_id: 2,
            bitmask: 0,
            offset: 21,
        });
        assert_eq!(duplicate_internal.external_id(2), None);
        assert_eq!(duplicate_internal.internal_id(10), None);
    }

    #[test]
    fn named_order_table_replays_prototype_and_following_rows() {
        let payload = b"order_table\0\xf8\x03\xf7\x42\xfb\xe2\
            \xe0\x01ext_id\0\x09\xe0\x01int_id\0\x01\
            \xe0\x01bitmask\0\x00\xf1\xf7\x42\xe2\
            \x0a\x02\x01\xe2\x0b\x03\x00";

        let order = order_table(payload, 0, payload.len()).expect("named order_table");

        assert_eq!(order.declared_count, 3);
        assert!(order.has_prototype);
        assert!(order.is_complete());
        assert_eq!(order.entity_ref, Some(66));
        assert_eq!(order.rows.len(), 2);
        assert_eq!(order.external_id(2), Some(10));
        assert_eq!(order.internal_id(11), Some(3));
    }

    #[test]
    fn order_tables_retain_extents_without_decoded_rows() {
        let named = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\xf1\xf7\x42\xe2";
        let order = order_table(named, 0, named.len()).expect("named order_table header");
        assert_eq!(order.declared_count, 2);
        assert!(!order.has_prototype);
        assert!(!order.is_complete());
        assert_eq!(order.entity_ref, Some(66));
        assert!(order.rows.is_empty());

        let positional = b"\xf8\x02\xf7\x42\xfb\xe2";
        let order = positional_order_table(positional, 0, positional.len(), 66)
            .expect("positional order_table header");
        assert_eq!(order.declared_count, 2);
        assert!(!order.has_prototype);
        assert!(!order.is_complete());
        assert_eq!(order.entity_ref, Some(66));
        assert!(order.rows.is_empty());
    }

    #[test]
    fn incomplete_order_tables_do_not_resolve_identifiers() {
        let named = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\
            \xf1\xf7\x42\xe2\x0a\x02\x00";
        let order = order_table(named, 0, named.len()).expect("named order_table");
        assert_eq!(order.rows.len(), 1);
        assert!(!order.is_complete());
        assert_eq!(order.internal_id(10), None);
        assert_eq!(order.external_id(2), None);

        let positional = b"\xf8\x02\xf7\x42\xfb\xe2";
        let order = positional_order_table(positional, 0, positional.len(), 66)
            .expect("positional order_table");
        assert!(!order.is_complete());
        assert_eq!(order.internal_id(10), None);
    }

    #[test]
    fn positional_trim_vertex_table_is_independent_of_entity_rows() {
        let payload = b"prefix\xf8\x13\xf7\x44\xfb\xe2\xf7\x45\
            \x01\x02\x03\x00\xe2";
        let vertices = positional_trim_vertex_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 68,
                bucket: 69,
                entry: 69,
            },
            None,
            None,
        )
        .expect("positional vert_tab");

        assert_eq!(vertices.declared_count, Some(19));
        assert_eq!(vertices.entity_ref, Some(68));
        assert_eq!(vertices.entry_ref, Some(69));
        assert_eq!(vertices.rows.len(), 1);
        assert_eq!(vertices.rows[0].vertex_id, 3);
        assert_eq!(vertices.rows[0].entities, [1, 2]);
    }

    #[test]
    fn positional_trim_vertex_table_retains_an_empty_extent() {
        let payload = b"prefix\xf8\x00\xf7\x44\xfb\xe2";

        let vertices = positional_trim_vertex_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 68,
                bucket: 69,
                entry: 69,
            },
            None,
            None,
        )
        .expect("empty positional vert_tab");

        assert_eq!(vertices.declared_count, Some(0));
        assert_eq!(vertices.entity_ref, Some(68));
        assert_eq!(vertices.entry_ref, Some(69));
        assert!(vertices.rows.is_empty());
    }

    #[test]
    fn trim_vertex_uses_unique_shared_point_for_mixed_curves() {
        let segment = |kind, point_ids, external_id| FeatureSegment {
            kind,
            directions: [None; 3],
            point_ids,
            center_id: (kind == FeatureSegmentKind::Arc).then_some(4),
            arc_orientation: (kind == FeatureSegmentKind::Arc).then_some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: 0,
        };
        let segments = FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![
                segment(FeatureSegmentKind::Line, [1, 2], 9),
                segment(FeatureSegmentKind::Arc, [2, 3], 10),
            ],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        };
        let variables = FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![FeatureSectionPoint {
                point_id: 2,
                u: Some(3.0),
                v: Some(4.0),
            }],
            offset: 0,
        };

        assert_eq!(
            entity_intersection([9, 10], Some(&segments), Some(&variables)),
            Some([3.0, 4.0])
        );

        let mut duplicate_segments = segments.clone();
        duplicate_segments.rows.push(segments.rows[0].clone());
        assert!(duplicate_segments.segment(9).is_none());
        assert!(
            entity_intersection([9, 10], Some(&duplicate_segments), Some(&variables)).is_none()
        );

        let mut duplicate_points = variables.clone();
        duplicate_points.points.push(variables.points[0].clone());
        assert_eq!(
            duplicate_points.reconciled_points().0.get(&2),
            Some(&[Some(3.0), Some(4.0)])
        );
        assert_eq!(
            entity_intersection([9, 10], Some(&segments), Some(&duplicate_points)),
            Some([3.0, 4.0])
        );
        duplicate_points.points[1].u = Some(5.0);
        assert!(duplicate_points.reconciled_points().1.contains(&2));
        assert!(entity_intersection([9, 10], Some(&segments), Some(&duplicate_points)).is_none());
        let row = |variable_type, value, offset| FeatureVariableRow {
            variable_type,
            key: 2,
            value: Some(value),
            value_body: Vec::new(),
            guess: None,
            guess_body: Vec::new(),
            guess_dimension_driven: false,
            known: None,
            homogeneity: None,
            uvar_id: None,
            dimension_driven: false,
            offset,
        };
        let mut repeated_raw = variables.clone();
        repeated_raw.points[0] = FeatureSectionPoint {
            point_id: 2,
            u: None,
            v: None,
        };
        repeated_raw.rows = vec![row(1, 3.0, 30), row(1, 3.0, 31), row(2, 4.0, 32)];
        assert_eq!(
            repeated_raw.reconciled_points().0.get(&2),
            Some(&[Some(3.0), Some(4.0)])
        );
        repeated_raw.rows[1].value = Some(5.0);
        assert!(repeated_raw.reconciled_points().1.contains(&2));
    }

    #[test]
    fn trim_vertex_template_identifies_table_and_entry_classes() {
        let payload = b"vert_tab\0\xf8\x13\xf7\x44\xfb\xe2\
            attrs\0\xf1\xf7\x46\xe3bucket_xar\0\xf8\x01\xf7\x46\xfb\xe3\
            \xf7\x45\x09\x0a\x03\x00";

        assert_eq!(
            trim_table_header(payload, b"vert_tab\0", 0, payload.len()),
            Some(TrimTableHeader {
                declared_count: 19,
                classes: TrimTableClasses {
                    table: 68,
                    bucket: 70,
                    entry: 69,
                },
            })
        );
    }

    #[test]
    fn trim_buckets_require_the_complete_declared_sequence_and_counts() {
        let payload = b"bucket_index\0\x00bucket_xar\0\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x09\x0a\x03\x00\xe2\x01\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x09\x0a\x03\x00\xe2\x02\xf1\xf7\x42\xe2\x03\xe2\
            \x04\xf0\xf7\x43\xf8\x01\xf7\x43\xfb\xe3\xf7\x44\x0b\x0c\
            \x05\x00\xe2\x05\xf8\x01\xf7\x43\xfb\xe3\xf7\x44\x0d\x0e\
            \x06\x00\xe2\x06\xe0\x00next\0";
        let header = TrimTableHeader {
            declared_count: 7,
            classes: TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 68,
            },
        };

        assert_eq!(
            trim_buckets(payload, 0, payload.len(), header, TrimEntryKind::Vertex)
                .iter()
                .map(|bucket| (
                    bucket.index,
                    bucket.declared_entry_count,
                    bucket.decoded_entry_count
                ))
                .collect::<Vec<_>>(),
            (0..7)
                .zip([1, 1, 0, 0, 1, 1, 0])
                .map(|(index, count)| (index, count, count))
                .collect::<Vec<_>>()
        );
        let truncated = payload
            .windows(2)
            .position(|bytes| bytes == [0xe2, 0x06])
            .expect("last bucket index");
        assert_eq!(
            trim_buckets(payload, 0, truncated, header, TrimEntryKind::Vertex)
                .iter()
                .map(|bucket| bucket.index)
                .collect::<Vec<_>>(),
            (0..6).collect::<Vec<_>>()
        );
    }

    #[test]
    fn trim_bucket_completeness_rejects_missing_and_extra_vertex_entries() {
        let header = TrimTableHeader {
            declared_count: 1,
            classes: TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 68,
            },
        };
        let missing = b"bucket_index\0\x00bucket_xar\0\xf8\x02\xf7\x43\xfb\xe3\
            \xf7\x44\x01\x02\x03\x00\xe0";
        let buckets = trim_buckets(missing, 0, missing.len(), header, TrimEntryKind::Vertex);
        assert_eq!(buckets[0].declared_entry_count, 2);
        assert_eq!(buckets[0].decoded_entry_count, 1);
        assert!(!buckets[0].is_complete());

        let extra = b"bucket_index\0\x00bucket_xar\0\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x01\x02\x03\x00\xe3\x04\x05\x06\x00\xe0";
        let buckets = trim_buckets(extra, 0, extra.len(), header, TrimEntryKind::Vertex);
        assert_eq!(buckets[0].declared_entry_count, 1);
        assert_eq!(buckets[0].decoded_entry_count, 2);
        assert!(!buckets[0].is_complete());
    }

    #[test]
    fn trim_vertex_entries_retain_variable_incident_entity_counts() {
        let counted = b"\xf8\x03\x0a\x0b\x0c\x07\x00";
        assert_eq!(
            trim_vertex_entry(counted, 0, counted.len()),
            Some((vec![10, 11, 12], 7, counted.len()))
        );
        let direct = b"\x0a\x0b\x0c\x07\x00";
        assert_eq!(
            trim_vertex_entry(direct, 0, direct.len()),
            Some((vec![10, 11, 12], 7, direct.len()))
        );
    }

    #[test]
    fn trim_entity_bucket_counts_the_named_prototype_and_complete_bodies() {
        let payload = b"bucket_index\0\x00bucket_xar\0\xf8\x02\xf7\x43\xfb\xe3\
            entry_ptr(entity_entry)\0\xe3xid\0\x00ent_mode\0\x00start_vtx\0\xf6\
            end_vtx\0\xf6center_vtx\0\xf6pers_attribs\0\x00\
            \xf4\x04\xf7\x42\xe2\xe3\
            \x09\x00\x03\x04\xf6\x00\xe0";
        let header = TrimTableHeader {
            declared_count: 1,
            classes: TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 68,
            },
        };
        let buckets = trim_buckets(payload, 0, payload.len(), header, TrimEntryKind::Entity);
        assert_eq!(buckets[0].decoded_entry_count, 2);
        assert!(buckets[0].is_complete());

        let truncated = payload.len() - 2;
        let buckets = trim_buckets(payload, 0, truncated, header, TrimEntryKind::Entity);
        assert_eq!(buckets[0].decoded_entry_count, 1);
        assert!(!buckets[0].is_complete());
    }

    #[test]
    fn decodes_var_arr_dictionary_sign_pairs() {
        let cache = scalar::ScalarCache::default();
        let cases = [
            (
                [0x97, 0xc3, 0x95, 0x81, 0x06, 0x24, 0xdc],
                3.595_499_999_999_999_5,
            ),
            (
                [0xdd, 0xc3, 0x95, 0x81, 0x06, 0x24, 0xdc],
                -3.595_499_999_999_999_5,
            ),
            (
                [0x80, 0x58, 0x23, 0x8b, 0x27, 0x55, 0x6f],
                1.334_018_271_988_806_7,
            ),
            ([0x7f, 0xa3, 0xd7, 0x0a, 0x3d, 0x70, 0xa4], 1.29),
            ([0xc7, 0xa3, 0xd7, 0x0a, 0x3d, 0x70, 0xa4], -1.29),
            (
                [0xc8, 0x58, 0x23, 0x8b, 0x27, 0x55, 0x6f],
                -1.334_018_271_988_806_7,
            ),
        ];
        for (bytes, expected) in cases {
            let (value, next, dimension_driven) =
                decode_variable_scalar(&bytes, 0, bytes.len(), &cache);
            assert_eq!(value, Some(expected));
            assert_eq!(next, bytes.len());
            assert!(!dimension_driven);
        }
    }

    #[test]
    fn decodes_var_arr_negative_subunit_form() {
        let bytes = [0xd5, 0xd9, 0x52, 0xa4, 0x85, 0x40, 0x39];
        let (value, next, dimension_driven) =
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());

        assert_eq!(value, Some(-0.395_669_107_559_015_74));
        assert_eq!(next, bytes.len());
        assert!(!dimension_driven);
    }

    #[test]
    fn decodes_var_arr_positive_subunit_form() {
        let bytes = [0x4f, 0xdf, 0x46, 0xa2, 0x52, 0x96, 0xd1];
        let (value, next, dimension_driven) =
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());

        assert_eq!(value, Some(0.488_686_161_664_432_46));
        assert_eq!(next, bytes.len());
        assert!(!dimension_driven);
    }

    #[test]
    fn variable_row_bounds_an_unresolved_guess_from_its_fixed_suffix() {
        let payload = b"var_arr\0\xf8\x01\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2\
            \x00\x41\x18\x20\x96\x61\x01\x01\x82\x06\xe2";
        let variables = variable_table(payload, 0, payload.len(), &scalar::ScalarCache::default())
            .expect("variable table");
        let [row] = variables.rows.as_slice() else {
            panic!("one structurally complete variable row");
        };

        assert!(variables.is_complete());
        assert_eq!(row.variable_type, 0);
        assert_eq!(row.key, 65);
        assert_eq!(row.value, Some(0.0));
        assert_eq!(row.value_body, [0x18]);
        assert_eq!(row.guess, None);
        assert_eq!(row.guess_body, [0x20, 0x96, 0x61]);
        assert_eq!(row.known, Some(1));
        assert_eq!(row.homogeneity, Some(1));
        assert_eq!(row.uvar_id, Some(518));
    }

    #[test]
    fn variable_row_classifies_value_and_guess_sentinels_independently() {
        let payload = b"var_arr\0\xf8\x01\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2\
            \x01\x07\xed\x01\x02\x03\x04\x05\x06\x07\x08\
            \xed\x11\x12\x13\x14\x15\x16\x17\x18\x01\x01\x09\xe2";
        let variables = variable_table(payload, 0, payload.len(), &scalar::ScalarCache::default())
            .expect("variable table");
        let [row] = variables.rows.as_slice() else {
            panic!("one structurally complete variable row");
        };

        assert!(variables.is_complete());
        assert_eq!(
            row.value_body,
            [0xed, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
        assert!(row.dimension_driven);
        assert_eq!(
            row.guess_body,
            [0xed, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]
        );
        assert!(row.guess_dimension_driven);
        assert_eq!(row.known, Some(1));
        assert_eq!(row.homogeneity, Some(1));
        assert_eq!(row.uvar_id, Some(9));
    }

    #[test]
    fn var_arr_world_coordinate_2d_is_positive() {
        let bytes = [0x2d, 0x34, 0x43, 0xf5, 0x12, 0xe8, 0x00, 0x45];
        let (value, next, dimension_driven) = decode_section_coordinate_scalar(
            &bytes,
            0,
            bytes.len(),
            &scalar::ScalarCache::default(),
        );

        assert_eq!(value, Some(20.265_458_280_220_873));
        assert_eq!(next, bytes.len());
        assert!(!dimension_driven);
        assert_eq!(
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()).0,
            Some(-20.265_458_280_220_873)
        );
    }

    #[test]
    fn saved_section_world_coordinate_2d_is_positive() {
        let bytes = [0x2d, 0x52, 0xa4, 0x0d, 0xb4, 0x1f, 0x70, 0xed];

        assert_eq!(
            saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (Some(74.563_336_401_657_31), bytes.len())
        );
    }

    #[test]
    fn decodes_var_arr_positional_dict_lattice() {
        for (bytes, head) in [
            ([0x51, 1, 2, 3, 4, 5, 6], [0x3f, 0xc6]),
            ([0x64, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
            ([0x69, 1, 2, 3, 4, 5, 6], [0x3f, 0xde]),
            ([0x9c, 1, 2, 3, 4, 5, 6], [0x40, 0x11]),
            ([0x9d, 1, 2, 3, 4, 5, 6], [0x40, 0x12]),
            ([0x9f, 1, 2, 3, 4, 5, 6], [0x40, 0x14]),
            ([0xa0, 1, 2, 3, 4, 5, 6], [0x40, 0x15]),
            ([0xa7, 1, 2, 3, 4, 5, 6], [0xbf, 0xd3]),
            ([0xaa, 1, 2, 3, 4, 5, 6], [0xbf, 0xd6]),
            ([0xae, 1, 2, 3, 4, 5, 6], [0xbf, 0xda]),
            ([0xad, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
            ([0xb3, 1, 2, 3, 4, 5, 6], [0xbf, 0xe0]),
            ([0xbd, 1, 2, 3, 4, 5, 6], [0xbf, 0xea]),
            ([0xc3, 1, 2, 3, 4, 5, 6], [0xbf, 0xf0]),
            ([0xc9, 1, 2, 3, 4, 5, 6], [0xbf, 0xf6]),
            ([0xca, 1, 2, 3, 4, 5, 6], [0xbf, 0xf7]),
            ([0xcb, 1, 2, 3, 4, 5, 6], [0xbf, 0xf8]),
            ([0xcc, 1, 2, 3, 4, 5, 6], [0xbf, 0xf9]),
            ([0xcd, 1, 2, 3, 4, 5, 6], [0xbf, 0xfa]),
            ([0xce, 1, 2, 3, 4, 5, 6], [0xbf, 0xfb]),
            ([0xd0, 1, 2, 3, 4, 5, 6], [0xbf, 0xfe]),
            ([0xd2, 1, 2, 3, 4, 5, 6], [0xc0, 0x00]),
            ([0xd4, 1, 2, 3, 4, 5, 6], [0xc0, 0x02]),
            ([0xd6, 1, 2, 3, 4, 5, 6], [0xc0, 0x04]),
            ([0xd8, 1, 2, 3, 4, 5, 6], [0xc0, 0x06]),
            ([0xda, 1, 2, 3, 4, 5, 6], [0xc0, 0x08]),
        ] {
            let (value, next, dimension_driven) =
                decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());
            assert_eq!(
                value,
                Some(f64::from_be_bytes([
                    head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                ]))
            );
            assert_eq!(next, bytes.len());
            assert!(!dimension_driven);
        }
        let bytes = [0x28, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (
                Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])),
                bytes.len(),
                false,
            )
        );
        for prefix in [0x19, 0x32, 0x37, 0x41] {
            let bytes = [prefix, 1, 2, 3, 4, 5, 6, 7];
            assert_eq!(
                decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
                (
                    Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])),
                    bytes.len(),
                    false,
                )
            );
        }
        assert_eq!(
            decode_section_coordinate_scalar(
                &[0x34, 0xd0, 0x00],
                0,
                3,
                &scalar::ScalarCache::default()
            ),
            (None, 3, false)
        );
        assert_eq!(
            decode_section_coordinate_scalar(
                &[0x00, 0x04, 0xa6],
                0,
                3,
                &scalar::ScalarCache::default()
            ),
            (None, 3, false)
        );
        assert_eq!(
            decode_section_coordinate_scalar(
                &[0x01, 0x04, 0xfe, 0xf2],
                0,
                4,
                &scalar::ScalarCache::default()
            ),
            (None, 4, false)
        );
    }

    #[test]
    fn saved_line_accepts_bare_entity_reference_before_coordinates() {
        let payload = b"\xe0\0entity(line)\0\x05\xe2\xf7\x2a\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\xf1\xf7\x2b\xe3";
        let entities =
            saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

        assert_eq!(entities.len(), 1);
        let FeatureSavedEntity::Line(line) = &entities[0] else {
            panic!("expected saved line");
        };
        assert_eq!(line.entity_id, 5);
        assert_eq!(line.references, [42, 43]);
        assert_eq!(line.endpoints, [[Some(8.0); 3]; 2]);
        let body_start = b"\xe0\0entity(line)\0".len();
        assert_eq!(line.body, payload[body_start..payload.len() - 1]);
    }

    #[test]
    fn saved_line_expands_compact_basis_triple() {
        let payload = b"\xe0\0entity(line)\0\x05\xe2\x18\xe5\x2f\x20\0\x2f\x20\0\x2f\x20\0\xe3";
        let entities =
            saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let FeatureSavedEntity::Line(line) = &entities[0] else {
            panic!("expected saved line");
        };
        assert_eq!(
            line.endpoints,
            [
                [Some(0.0), Some(1.0), Some(0.0)],
                [Some(8.0), Some(8.0), Some(8.0)]
            ]
        );
        let body_start = b"\xe0\0entity(line)\0".len();
        assert_eq!(line.body, payload[body_start..payload.len() - 1]);
    }

    #[test]
    fn saved_line_replay_continues_after_point_prototype() {
        let scalar_triple = b"\x2f\x20\0\x2f\x20\0\x2f\x20\0";
        let mut payload = b"\xe0\0entity(line)\0\x05\xe2".to_vec();
        payload.extend_from_slice(scalar_triple);
        payload.extend_from_slice(scalar_triple);
        payload.push(0xe3);
        payload.extend_from_slice(b"\xe0\0entity(point)\0\xe0\x01id\0\x04\xf1\xf7\x2a\xe3\x06\xe2");
        payload.extend_from_slice(scalar_triple);
        payload.extend_from_slice(scalar_triple);
        payload.extend_from_slice(b"\xe0\0entity(arc)\0");

        let entities =
            saved_line_entities(&payload, 0, payload.len(), &scalar::ScalarCache::default());

        assert_eq!(entities.len(), 2);
        assert_eq!(
            entities
                .iter()
                .filter_map(|entity| match entity {
                    FeatureSavedEntity::Line(line) => Some(line.entity_id),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [5, 6]
        );
    }

    #[test]
    fn saved_line_accepts_named_record_boundary() {
        let payload = b"\xe0\0entity(line)\0\x03\xe2\xf1\xf7\x80\xc4\
            \x48\x20\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\
            \x48\x1e\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\x8a\x01\x02\x03\x04\x05\x0f\
            \xe0\0entity(point)\0\xf1\xf7\x2a\xe3\xe0\0entity(arc)\0";
        let entities =
            saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

        assert_eq!(entities.len(), 1);
        let FeatureSavedEntity::Line(line) = &entities[0] else {
            panic!("expected saved line");
        };
        assert_eq!(line.entity_id, 3);
        assert_eq!(line.references, [196]);
        let body_start = b"\xe0\0entity(line)\0".len();
        let body_end = payload[body_start..]
            .windows(b"\xe0\0entity(point)\0".len())
            .position(|window| window == b"\xe0\0entity(point)\0")
            .map(|relative| body_start + relative)
            .expect("point boundary");
        assert_eq!(line.body, payload[body_start..body_end]);
    }

    #[test]
    fn saved_line_retains_its_identity_and_coordinate_prefix() {
        let payload = b"\xe0\0entity(line)\0\x07\xe2\x0f\x0f\x0f\
            \xe0\0entity(arc)\0";

        let entities =
            saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

        let [FeatureSavedEntity::Line(line)] = entities.as_slice() else {
            panic!("saved line");
        };
        assert_eq!(line.entity_id, 7);
        assert_eq!(
            line.endpoints,
            [[Some(0.0), Some(0.0), Some(0.0)], [None; 3]]
        );
    }

    #[test]
    fn saved_section_retains_an_empty_named_table() {
        let payload = b"\xe0\0p_saved_result\0\xe0\x02local_sys\0";

        let section = saved_section(
            payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            None,
            None,
        )
        .expect("saved section header");

        assert_eq!(section.offset, 0);
        assert!(section.entities.is_empty());
    }

    #[test]
    fn saved_section_41_form_occupies_eight_bytes() {
        let bytes = [0x41, 0xfd, 0x6b, 0xf1, 0xa1, 0xc2, 0x1f, 0xf0];
        let (value, next) =
            saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());
        assert_eq!(next, bytes.len());
        assert_eq!(
            value,
            Some(f64::from_be_bytes([
                0x3f, 0xfd, 0x6b, 0xf1, 0xa1, 0xc2, 0x1f, 0xf0
            ]))
        );
    }

    #[test]
    fn saved_section_zero_does_not_consume_named_record_opener() {
        let mut section = Vec::new();
        for index in 0_u16..=224 {
            section.extend_from_slice(&[0x46, 0x08, (index >> 8) as u8, index as u8, 0, 0, 0, 0]);
        }
        let cache = scalar::ScalarCache::from_section(&section);

        assert_eq!(
            saved_section_scalar(&[0x18, 0xe0], 0, 2, &cache),
            (Some(0.0), 1)
        );
    }

    #[test]
    fn saved_section_consecutive_zero_slots_remain_distinct() {
        let cache = scalar::ScalarCache::default();
        let bytes = [0x18, 0x18, 0x81, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            saved_section_scalar(&bytes, 0, bytes.len(), &cache),
            (Some(0.0), 1)
        );
        assert_eq!(
            saved_section_scalar(&bytes, 1, bytes.len(), &cache),
            (Some(0.0), 2)
        );
    }

    #[test]
    fn saved_section_dd_form_supplies_ieee_high_bytes() {
        let bytes = [0xdd, 0xe6, 0x8a, 0x84, 0x79, 0xd0, 0x62];
        assert_eq!(
            saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (
                Some(f64::from_be_bytes([
                    0x40, 0x0c, 0xe6, 0x8a, 0x84, 0x79, 0xd0, 0x62,
                ])),
                7,
            )
        );
    }

    #[test]
    fn saved_section_negative_dict_forms_supply_ieee_high_bytes() {
        for (bytes, head) in [
            ([0xb3, 1, 2, 3, 4, 5, 6], [0xbf, 0xe0]),
            ([0xcb, 1, 2, 3, 4, 5, 6], [0xbf, 0xf8]),
            ([0xd6, 1, 2, 3, 4, 5, 6], [0xc0, 0x04]),
        ] {
            assert_eq!(
                saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
                (
                    Some(f64::from_be_bytes([
                        head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
                        bytes[6],
                    ])),
                    7,
                )
            );
        }
    }

    #[test]
    fn saved_arc_negative_dict_forms_supply_ieee_high_bytes() {
        for (bytes, head) in [
            ([0x9b, 1, 2, 3, 4, 5, 6], [0x40, 0x10]),
            ([0x9c, 1, 2, 3, 4, 5, 6], [0x40, 0x11]),
            ([0x9d, 1, 2, 3, 4, 5, 6], [0x40, 0x12]),
            ([0x9e, 1, 2, 3, 4, 5, 6], [0x40, 0x13]),
            ([0x9f, 1, 2, 3, 4, 5, 6], [0x40, 0x14]),
            ([0xa0, 1, 2, 3, 4, 5, 6], [0x40, 0x15]),
            ([0x5e, 1, 2, 3, 4, 5, 6], [0x3f, 0xd3]),
            ([0x60, 1, 2, 3, 4, 5, 6], [0x3f, 0xd5]),
            ([0x64, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
            ([0xad, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
            ([0xcc, 1, 2, 3, 4, 5, 6], [0xbf, 0xf9]),
            ([0xd0, 1, 2, 3, 4, 5, 6], [0xbf, 0xfe]),
            ([0xd2, 1, 2, 3, 4, 5, 6], [0xc0, 0x00]),
            ([0xd5, 1, 2, 3, 4, 5, 6], [0xc0, 0x03]),
            ([0xde, 1, 2, 3, 4, 5, 6], [0xc0, 0x10]),
            ([0xdf, 1, 2, 3, 4, 5, 6], [0xc0, 0x11]),
        ] {
            let expected = f64::from_be_bytes([
                head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
            ]);
            assert_eq!(
                saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
                (Some(expected), 7)
            );
        }
        let d5 = [0xd5, 1, 2, 3, 4, 5, 6];
        assert_eq!(
            saved_section_scalar(&d5, 0, d5.len(), &scalar::ScalarCache::default()),
            (Some(f64::from_be_bytes([0xbf, 1, 2, 3, 4, 5, 6, 0])), 7)
        );
    }

    #[test]
    fn saved_arc_28_form_supplies_ieee_high_byte() {
        let bytes = [0x28, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(
            saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])), 8)
        );
    }

    #[test]
    fn saved_arc_zero_does_not_consume_arc_scalar_opener() {
        let bytes = [0x18, 0x5e, 1, 2, 3, 4, 5, 6];
        let cache = scalar::ScalarCache::default();
        assert_eq!(
            saved_arc_scalar(&bytes, 0, bytes.len(), &cache),
            (Some(0.0), 1)
        );
        assert_eq!(
            saved_arc_scalar(&bytes, 1, bytes.len(), &cache),
            (Some(f64::from_be_bytes([0x3f, 0xd3, 1, 2, 3, 4, 5, 6])), 8)
        );
    }

    #[test]
    fn saved_circular_entities_retain_ids_and_independent_fields() {
        let payload = b"\xe0\x00entity(arc)\0\
            \xe0\x01id\0\x07\xe0\x02center\0\x0f\x0f\x0f\
            \xe0\x00entity(circle)\0\
            \xe0\x01id\0\x08\xe0\x02radius\0\x0f";

        let entities = saved_circular_entities(
            payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            None,
            None,
        );

        let [FeatureSavedEntity::Arc(arc), FeatureSavedEntity::Circle(circle)] =
            entities.as_slice()
        else {
            panic!("saved circular entities");
        };
        assert_eq!(arc.entity_id, 7);
        assert_eq!(arc.center, [Some(0.0); 3]);
        assert_eq!(arc.radius, None);
        assert_eq!(arc.endpoints, [[None; 3]; 2]);
        assert_eq!(arc.parameters, [None; 2]);
        let arc_body_start = b"\xe0\x00entity(arc)\0".len();
        let circle_label = b"\xe0\x00entity(circle)\0";
        let circle_offset = payload
            .windows(circle_label.len())
            .position(|window| window == circle_label)
            .expect("circle boundary");
        assert_eq!(arc.body, payload[arc_body_start..circle_offset]);
        assert_eq!(circle.entity_id, 8);
        assert_eq!(circle.center, [None; 3]);
        assert_eq!(circle.radius, Some(0.0));
        assert_eq!(circle.body, payload[circle_offset + circle_label.len()..]);
    }

    #[test]
    fn saved_conic_retains_coefficients_parameters_and_planar_frame() {
        let payload = b"\xe0\x00entity(conic)\0\
            \xe0\x01id\0\x02\xe0\x01type\0\x3a\
            \xe0\x02end1\0\xf8\x03\x18\xe5\
            \xe0\x02end2\0\xf8\x03\x18\xe5\
            \xe0\x02t0\0\x0f\xe0\x02t1\0\xf6\
            \xe0\x02c1\0\xe4\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\
            \xe4\x0f\x0f\x0f\xe4\x18\xe5\x0f\x0f\x0f\x0f\
            \xe0\x01trailing_field\0\x07";

        let entities =
            saved_conic_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Conic(conic)] = entities.as_slice() else {
            panic!("one saved conic");
        };

        assert_eq!(conic.entity_id, 2);
        assert_eq!(conic.endpoints, [[Some(0.0), Some(1.0), Some(0.0)]; 2]);
        assert_eq!(conic.parameters, [Some(0.0), None]);
        assert_eq!(conic.coefficients, [Some(1.0); 2]);
        assert_eq!(
            conic.local_system,
            Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])
        );
        assert_eq!(conic.body, payload[b"\xe0\x00entity(conic)\0".len()..]);
    }

    #[test]
    fn saved_arc_replay_uses_order_table_row_boundaries() {
        let mut payload = vec![0xe3, 7, 0xe2];
        payload.extend([0x0f; 12]);
        payload.push(0xe3);
        let order = FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![FeatureOrderRow {
                external_id: 42,
                internal_id: 7,
                bitmask: 0,
                offset: 0,
            }],
            offset: 0,
        };
        let segments = FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![FeatureSegment {
                kind: FeatureSegmentKind::Arc,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: Some(3),
                arc_orientation: Some(0),
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 42,
                body: Vec::new(),
                offset: 0,
            }],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        };

        let entities = saved_positional_generated_entities(
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        );

        assert_eq!(entities.len(), 1);
        let FeatureSavedEntity::Arc(arc) = &entities[0] else {
            panic!("expected saved arc");
        };
        assert_eq!(arc.entity_id, 7);
        assert_eq!(arc.center, [Some(0.0); 3]);
        assert_eq!(arc.radius, Some(0.0));
        assert_eq!(arc.body, payload[1..payload.len() - 1]);
        let section = positional_saved_section(
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        )
        .expect("positional saved section");
        assert_eq!(section.entities.len(), 1);
        assert_eq!(section.offset, 1);

        let named_prefix = b"\xe0\x00entity(arc)\0\xe0\x01id\0\x09";
        let mut named_payload = named_prefix.to_vec();
        named_payload.extend_from_slice(&payload);
        let named_entities = saved_circular_entities(
            &named_payload,
            0,
            named_payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        );
        let [FeatureSavedEntity::Arc(named), FeatureSavedEntity::Arc(replay)] =
            named_entities.as_slice()
        else {
            panic!("named arc and replay");
        };
        assert_eq!(
            named.body, b"\xe0\x01id\0\x09",
            "named body must stop before the replay separator"
        );
        assert_eq!(replay.body, payload[1..payload.len() - 1]);
    }

    #[test]
    fn saved_arc_replay_retains_a_structurally_terminated_scalar_prefix() {
        let mut payload = vec![0xe3, 7, 0xe2];
        payload.extend([0x0f; 6]);
        payload.push(0xe3);
        let order = FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![FeatureOrderRow {
                external_id: 42,
                internal_id: 7,
                bitmask: 0,
                offset: 0,
            }],
            offset: 0,
        };
        let segments = FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![FeatureSegment {
                kind: FeatureSegmentKind::Arc,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: Some(3),
                arc_orientation: Some(0),
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 42,
                body: Vec::new(),
                offset: 0,
            }],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        };

        let entities = saved_positional_generated_entities(
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        );

        let [FeatureSavedEntity::Arc(arc)] = entities.as_slice() else {
            panic!("expected saved arc");
        };
        assert_eq!(arc.entity_id, 7);
        assert_eq!(arc.center, [Some(0.0); 3]);
        assert_eq!(arc.radius, Some(0.0));
        assert_eq!(arc.endpoints[0], [Some(0.0), Some(0.0), None]);
        assert_eq!(arc.endpoints[1], [None; 3]);
        assert_eq!(arc.parameters, [None; 2]);
    }

    #[test]
    fn saved_generated_line_requires_its_orientation_invariant() {
        let payload = [0xe3, 8, 0xe2, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe3];
        let order = FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![FeatureOrderRow {
                external_id: 43,
                internal_id: 8,
                bitmask: 0,
                offset: 0,
            }],
            offset: 0,
        };
        let segments = FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![FeatureSegment {
                kind: FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: Some(0),
                vertical_horizontal: Some(1),
                radius_ref: None,
                radius2_ref: None,
                external_id: 43,
                body: Vec::new(),
                offset: 0,
            }],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        };

        let entities = saved_positional_generated_entities(
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        );

        assert_eq!(entities.len(), 1);
        let FeatureSavedEntity::Line(line) = &entities[0] else {
            panic!("expected saved line");
        };
        assert_eq!(line.entity_id, 8);
        assert_eq!(line.endpoints[0], [Some(0.0); 3]);
        assert_eq!(line.endpoints[1], [Some(1.0), Some(0.0), Some(0.0)]);
        assert_eq!(line.body, payload[1..payload.len() - 1]);
    }

    #[test]
    fn decodes_mdlstatus_recipe_discriminators_within_their_records() {
        let payload = b"\xe3icon\0protextrude\0Protrusion id 40\0\xe2\xe3\
            icon\0protrevolve\0Revolve id 41\0\xe2\xe3\
            icon\0cutextrude\0Cut id 42\0\xe2\xe3\
            icon\0cutrevolve\0Cut id 43\0\xe2\xe3Datum Plane id 44\0\xe3K\xc3\xb6rper ID 45\0";
        let operations = operations(payload);
        assert_eq!(operations.len(), 6);
        assert_eq!(operations[0].recipe, Some(FeatureRecipe::ProtrudeExtrude));
        assert_eq!(operations[1].recipe, Some(FeatureRecipe::ProtrudeRevolve));
        assert_eq!(operations[2].recipe, Some(FeatureRecipe::CutExtrude));
        assert_eq!(operations[3].recipe, Some(FeatureRecipe::CutRevolve));
        assert_eq!(operations[4].recipe, None);
        assert_eq!(operations[5].kind, "Körper");
        assert_eq!(operations[5].feature_id, 45);
    }

    #[test]
    fn binds_depdb_recipe_records_to_compact_feature_ids() {
        let payload = b"\xe3K\xc3\xb6rper ID 247\0\xe3\
            \xf7\x3b\x80\xf7\x83\x95\xf6\x20Drehen 1\0\xf6\0protrevolve\0\
            \xe3Body ID 8053\0\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

        let operations = operations(payload);
        assert_eq!(operations.len(), 2);
        assert_eq!(operations[0].feature_id, 247);
        assert_eq!(operations[0].recipe, Some(FeatureRecipe::ProtrudeRevolve));
        assert_eq!(operations[0].root_schema_class, Some(917));
        assert_eq!(operations[0].parent_feature_id, Some(32));
        assert_eq!(operations[1].feature_id, 8053);
        assert_eq!(operations[1].recipe, Some(FeatureRecipe::ProtrudeExtrude));
        assert_eq!(operations[1].root_schema_class, Some(917));
        assert_eq!(operations[1].parent_feature_id, Some(8051));
    }

    #[test]
    fn preserves_competing_depdb_recipe_bindings() {
        let payload = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0";

        let states = operation_states(payload);
        assert_eq!(states.len(), 2);
        assert_eq!(states[0].feature_id, 8053);
        assert_eq!(states[0].recipe, Some(FeatureRecipe::ProtrudeExtrude));
        assert_eq!(states[0].root_schema_class, Some(917));
        assert_eq!(states[1].feature_id, 8053);
        assert_eq!(states[1].recipe, Some(FeatureRecipe::CutExtrude));
        assert_eq!(states[1].root_schema_class, Some(916));

        let current = operations(payload);
        assert_eq!(current.len(), 1);
        assert_eq!(current[0], states[1]);

        let repeated = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 2\0\xf6\0protextrude\0";
        let repeated_states = operation_states(repeated);
        assert_eq!(repeated_states.len(), 2);
        assert_eq!(repeated_states[0].recipe, repeated_states[1].recipe);
        assert_ne!(repeated_states[0].offset, repeated_states[1].offset);
    }

    #[test]
    fn promotes_depdb_recipe_without_operation_display_name() {
        let payload = b"\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

        let operations = operations(payload);
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].feature_id, 8053);
        assert_eq!(operations[0].kind, "Extrude");
        assert_eq!(operations[0].recipe, Some(FeatureRecipe::ProtrudeExtrude));
        assert_eq!(operations[0].root_schema_class, Some(917));
        assert_eq!(operations[0].parent_feature_id, Some(8051));
        assert_eq!(operations[0].offset, 1);
    }

    #[test]
    fn decodes_count_bounded_saved_spline_interpolation_points() {
        let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x02\x03\
            \xe4\x0f\x0d\x0f\xe4\x0f\
            \xe0\x02end_tangts\0\xf9\x02\x03\
            \xe4\x0f\x0f\xe4\x0f\x0f\
            \xe0\x02params\0\xf8\x02\x0f\xe4\
            \xe0\x01tan_cond\0\x00";

        let entities =
            saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
            panic!("saved spline");
        };
        assert_eq!(spline.entity_id, Some(7));
        assert_eq!(spline.declared_point_count, Some(2));
        assert_eq!(
            spline.interpolation_points,
            [[1.0, 0.0, -1.0], [0.0, 1.0, 0.0]]
        );
        assert_eq!(
            spline.interpolation_points_body,
            b"\xf9\x02\x03\xe4\x0f\x0d\x0f\xe4\x0f"
        );
        assert_eq!(
            spline.endpoint_tangents,
            Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
        );
        assert_eq!(
            spline.endpoint_tangents_body.as_deref(),
            Some(b"\xf9\x02\x03\xe4\x0f\x0f\xe4\x0f\x0f".as_slice())
        );
        assert_eq!(spline.parameters, Some(vec![0.0, 1.0]));
        assert_eq!(
            spline.parameters_body.as_deref(),
            Some(b"\xf8\x02\x0f\xe4".as_slice())
        );
    }

    #[test]
    fn decodes_compact_saved_spline_point_count() {
        let mut payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x80\x88\x03"
            .to_vec();
        payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

        let entities =
            saved_spline_entities(&payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
            panic!("saved spline");
        };
        assert_eq!(spline.declared_point_count, Some(136));
        assert_eq!(spline.interpolation_points.len(), 136);
        assert_eq!(
            spline.interpolation_points_body,
            payload
                [b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x01id\0\x07\xe0\x02i_pnts\0".len()..]
        );
        assert!(spline
            .interpolation_points
            .iter()
            .all(|point| *point == [0.0; 3]));
    }

    #[test]
    fn saved_spline_retains_its_declared_count_and_complete_point_prefix() {
        let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x02\x03\
            \x0f\x0f\x0f\xe0\x01tan_cond\0\x00";

        let entities =
            saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
            panic!("saved spline");
        };

        assert_eq!(spline.entity_id, Some(7));
        assert_eq!(spline.declared_point_count, Some(2));
        assert_eq!(spline.interpolation_points, [[0.0; 3]]);
        assert_eq!(
            spline.interpolation_points_body,
            b"\xf9\x02\x03\x0f\x0f\x0f"
        );
        assert_eq!(spline.endpoint_tangents, None);
        assert_eq!(spline.endpoint_tangents_body, None);
        assert_eq!(spline.parameters, None);
        assert_eq!(spline.parameters_body, None);
    }

    #[test]
    fn saved_spline_retains_its_identity_without_a_point_table() {
        let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x01id\0\x07";

        let entities =
            saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
            panic!("saved spline");
        };

        assert_eq!(spline.entity_id, Some(7));
        assert_eq!(spline.declared_point_count, None);
        assert!(spline.interpolation_points.is_empty());
        assert!(spline.interpolation_points_body.is_empty());
    }

    #[test]
    fn saved_spline_retains_a_valid_point_wrapper_when_allocation_is_rejected() {
        let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x02i_pnts\0\xf9\xbf\xff\x03";

        let entities =
            saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
            panic!("saved spline");
        };

        assert_eq!(spline.declared_point_count, Some(16_383));
        assert!(spline.interpolation_points.is_empty());
        assert_eq!(spline.interpolation_points_body, b"\xf9\xbf\xff\x03");
    }

    #[test]
    fn decodes_compact_feature_scalar_array_extents() {
        let mut payload = vec![psb::token::SCALAR_BODY, 0x80, 0x88, 0x03];
        payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

        let FeatureFieldValue::ScalarArray {
            dimensions,
            count,
            body,
            decoded_values,
        } = field_value(&payload)
        else {
            panic!("scalar array");
        };
        assert_eq!(dimensions, 136);
        assert_eq!(count, 3);
        assert_eq!(body.len(), 408);
        assert_eq!(decoded_values, Some(vec![0.0; 408]));
    }

    #[test]
    fn decodes_saved_spline_chord_parameter_lane() {
        let body = [
            0x18, 0x6d, 0x31, 0xd2, 0x2a, 0x7f, 0x68, 0x39, 0x85, 0x06, 0x5f, 0x25, 0x83, 0xf4,
            0x6c, 0x93, 0xd8, 0xd4, 0xfb, 0x45, 0xbc, 0x38, 0x9e, 0x51, 0xef, 0x1e, 0x96, 0xe2,
            0x6c, 0x2d, 0x1a, 0xfc, 0x59, 0x51, 0xbd, 0x0a, 0x38,
        ];
        let cache = scalar::ScalarCache::default();
        let expected = [
            0.0,
            0.568_581_660_273_827_7,
            1.626_555_582_565_994_3,
            3.105_874_980_035_448_4,
            4.830_013_730_963_952,
            6.746_434_476_054_269,
        ];
        let mut cursor = 0;
        for expected in expected {
            let (value, next) = saved_spline_parameter(&body, cursor, &cache).expect("parameter");
            assert_eq!(value, expected);
            cursor = next;
        }
        assert_eq!(cursor, body.len());
    }

    #[test]
    fn decodes_zero_offset_positional_placement_instruction() {
        let payload = b"place_instruction_ptrs\0\xf8\x03\xf7\x0b\xfb\xe3\
            \xf1\xf7\x0b\xe3\xc0\x4e\x9f\x18\xf6\xf6\x02\xf6\x00\x00\x00\xe6";
        let rows = placement_instruction_rows(payload, 1000);
        let [row] = rows.as_slice() else {
            panic!("placement row");
        };
        assert_eq!(row.kind, 20_127);
        assert!(row.zero_offset);
        assert_eq!(row.dimension_id, None);
        assert_eq!(row.reference_id, None);
        assert_eq!(row.geometry1_id, Some(2));
        assert_eq!(row.geometry2_id, None);
        assert_eq!([row.member1, row.member2], [0, 0]);
        assert_eq!(row.offset, 1029);
    }

    #[test]
    fn model_reference_entry_joins_feature_name_to_feature_id() {
        let payload = b"\0\xf7\x71\x2a\x05\x29Datum Plane id 41\0\x2a\x2a\x10\0\
            \xf7\x71\x30\x05\x2fBroken\0\x30\x31";

        assert_eq!(
            reference_names(payload),
            [FeatureReferenceName {
                feature_id: 41,
                name: "Datum Plane id 41".to_string(),
                name_bytes: b"Datum Plane id 41".to_vec(),
                own_reference_id: 42,
                reference_type: 5,
                offset: 1,
            }]
        );
    }
