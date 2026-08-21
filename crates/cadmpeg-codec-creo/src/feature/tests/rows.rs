// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use super::super::definitions::*;
use super::super::entity::*;
use super::super::rows::*;
use crate::psb;

#[test]
fn rows_retain_distinct_root_schema_classes_for_one_feature_id() {
    let payload = [
        7, 0xeb, 0x04, 0, 0, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 0xaa, 0xe3, 7, 0x90, 0x01, 0xe3, 0xf6,
        0x83, 0x91, 0xe1, 0xbb,
    ];
    let feature_ids = BTreeSet::from([7]);

    let decoded = rows(&payload, &feature_ids);

    assert_eq!(decoded.len(), 2);
    assert_eq!(
        decoded
            .iter()
            .map(|row| (row.header, row.root_schema_class))
            .collect::<Vec<_>>(),
        [([0xeb, 0x04], Some(917)), ([0x90, 0x01], Some(913))]
    );
}

#[test]
fn rows_suppress_repeated_same_class_candidates() {
    let payload = [
        7, 0xeb, 0x04, 0, 0, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 0xaa, 0xe3, 7, 0x90, 0x01, 0xe3, 0xf6,
        0x83, 0x95, 0xe1, 0xbb,
    ];
    let feature_ids = BTreeSet::from([7]);

    let decoded = rows(&payload, &feature_ids);

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].root_schema_class, Some(917));
}

#[test]
fn rows_accept_an_unlisted_header_with_the_fixed_root_prefix() {
    let payload = [
        7, 0x88, 0x01, 0x00, 0x88, 0x00, 0x00, 0xe3, 0xf6, 0x83, 0xb5, 0xe1, 0xbb,
    ];
    let feature_ids = BTreeSet::from([7]);

    let decoded = rows(&payload, &feature_ids);

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].header, [0x88, 0x01]);
    assert_eq!(decoded[0].root_schema_class, Some(949));
}

#[test]
fn rows_require_the_root_marker_after_the_row_header() {
    let payload = [7, 0x88, 0x01, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
    let feature_ids = BTreeSet::from([7]);

    assert!(rows(&payload, &feature_ids).is_empty());
}

#[test]
fn rows_accept_a_root_marker_immediately_after_the_header() {
    let payload = [40, 0xeb, 0x04, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 0xaa];
    let feature_ids = BTreeSet::from([40]);

    assert_eq!(rows(&payload, &feature_ids).len(), 1);
}

#[test]
fn rows_accept_a_row_after_the_raw_section_header() {
    let mut payload = b"#AllFeatur\n".to_vec();
    payload.extend_from_slice(&[7, 0x88, 0x01, 0xe3, 0xf6, 0x83, 0xb5, 0xe1, 0xbb]);
    let feature_ids = BTreeSet::from([7]);

    let decoded = rows(&payload, &feature_ids);

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].offset, b"#AllFeatur\n".len());
}

#[test]
fn rows_ignore_a_valid_prefix_inside_an_existing_row() {
    let payload = [
        7, 0xeb, 0x04, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 0x11, 7, 0x88, 0x01, 0xe3, 0xf6, 0x83, 0xb5,
        0xe1, 0xbb,
    ];
    let feature_ids = BTreeSet::from([7]);

    let decoded = rows(&payload, &feature_ids);

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].header, [0xeb, 0x04]);
}

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
                0x00, 0xf8, 0x02, 0xf7, 0x57, 0xfb, 0xe2, 0xf7, 0x58, 0x80, 0x91, 0xf6, 0xf1, 0xf7,
                0x57, 0xe2, 0x80, 0x92, 0xf6, 0xe3,
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
        0xf7, 0x50, 0x0d, 0x80, 0xcc, 0x00, 0xe4, 0xf1, 0xf7, 0x4f, 0xe3, 0x12, 0x80, 0xcb, 0x00,
        0xe4, 0xe3,
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
fn positional_round_replay_rejects_explicit_arrays_without_compound_close_start() {
    let row = unanchored_replay_row(1, 40, None, &[0x00, 0xf8, 2, 10, 11, 0xf8, 2, 20, 21]);

    assert!(replay_affected_ids(&[row]).is_empty());
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
