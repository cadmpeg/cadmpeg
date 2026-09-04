// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::Exactness;

use crate::container::{self};
use crate::test_support::*;
use crate::CreoCodec;

#[test]
fn scan_collects_feature_owners_from_rows_and_parent_lists() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"parent_feats\0\xf8\x02\x04\x09");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.features.ids, vec![4, 9]);
}

#[test]
fn scan_binds_allfeatur_mixed_entity_table_to_known_feature() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        &[
            0xf8, 2, 0xf7, 0x1d, 0xfb, 0xe3, // two mixed entity references
            7, 0x80, 0xc8, 1, 0, 0xe3, // a materialized surface id
            0xf7, 0x1e, 9, 0x80, 0xc8, 2, 0, 0xe3, // a prefixed non-surface entity id
        ],
    );
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Protrusion id 4\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.entity_tables.len(), 1);
    let table = &scan.features.entity_tables[0];
    assert_eq!(table.feature_id, Some(4));
    assert_eq!(table.table_class_id, 29);
    assert_eq!(table.entry_ids, vec![7, 9]);
    assert_eq!(table.entries.len(), 2);
    assert!(!table.entries[0].prefixed);
    assert!(table.entries[1].prefixed);
    assert_eq!(table.entries[0].entity_id, 7);
    assert_eq!(table.entries[1].entity_id, 9);
    assert_eq!(table.entries[0].class_id, 200);
    assert_eq!(table.entries[1].class_id, 200);
    assert_eq!(table.entries[0].source_entity_id, Some(1));
    assert_eq!(table.entries[1].source_entity_id, Some(2));
    assert_eq!(table.entries[0].end_offset, table.entries[1].offset - 2);
    assert_eq!(table.surface_ids, vec![7]);
    assert_eq!(table.non_surface_entity_ids, vec![9]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#4")
        .expect("feature 4");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude { .. }
    ));
    assert_eq!(
        feature.source_properties["native_parameter.generated_entity.7.source_section_entity_id"],
        "1"
    );
    assert_eq!(
        feature.source_properties["native_parameter.generated_entity.7.entry_class"],
        "200"
    );
    assert_eq!(
        feature.source_properties["native_parameter.generated_entity.9.source_section_entity_id"],
        "2"
    );
    let tables = &result.ir().native.namespace("creo").unwrap().arenas["feature_entity_tables"];
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].fields()["owner_feature_id"], 4);
    assert_eq!(tables[0].fields()["table_class_id"], 29);
    assert_eq!(tables[0].fields()["entry_ids"][0], 7);
    assert_eq!(tables[0].fields()["entry_ids"][1], 9);
    assert_eq!(tables[0].fields()["entries"][0]["class_id"], 200);
    assert_eq!(tables[0].fields()["entries"][0]["source_entity_id"], 1);
    assert_eq!(tables[0].fields()["entries"][1]["prefixed"], true);
    assert_annotation(
        &result.source_fidelity().annotations,
        tables[0].id(),
        "creo:AllFeatur",
        table.offset as u64,
        "feature_entity_table",
        Exactness::ByteExact,
    );
}

#[test]
fn scan_decodes_source_entity_id_whose_compact_tail_is_e3() {
    let mut geometry = visibgeom_payload(2, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    geometry.extend_from_slice(&[8, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        &[
            0xf8, 2, 0xf7, 0x1d, 0xfb, 0xe3, 7, 0x80, 0xc8, 0x80, 0xe3, 0, 0xe3, 8, 0x80, 0xc8, 3,
            0, 0xe3,
        ],
    );
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    let [table] = scan.features.entity_tables.as_slice() else {
        panic!("expected one generated-entity table");
    };
    assert_eq!(table.entry_ids, vec![7, 8]);
    assert_eq!(table.entries[0].class_id, 200);
    assert_eq!(table.entries[0].source_entity_id, Some(227));
    assert_eq!(table.entries[1].source_entity_id, Some(3));
}

#[test]
fn scan_accepts_large_structurally_bounded_feature_entity_tables() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let mut allfeatur = allfeatur_row(4, [0xeb, 0x04], 917, &[0xf8, 65, 0xf7, 0x1d, 0xfb, 0xe3]);
    allfeatur.extend_from_slice(&[7, 0x80, 0xc8, 1, 0, 0xe3]);
    for _ in 1..65 {
        allfeatur.extend_from_slice(&[9, 0x80, 0xc8, 1, 0, 0xe3]);
    }
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    let [table] = scan.features.entity_tables.as_slice() else {
        panic!("expected one large generated-entity table");
    };
    assert_eq!(table.feature_id, Some(4));
    assert_eq!(table.entry_ids.len(), 65);
    assert_eq!(table.surface_ids, vec![7]);
    assert_eq!(table.non_surface_entity_ids.len(), 64);
}

#[test]
fn scan_rejects_feature_entity_table_that_crosses_the_next_feature_row() {
    let mut geometry = visibgeom_payload(2, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    geometry.extend_from_slice(&[8, 0x22, 9, 0x01, 0, 0]);
    let mut allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        &[0xf8, 2, 0xf7, 0x1d, 0xfb, 0xe3, 7, 0x80, 0xc8, 1, 0, 0xe3],
    );
    // The second declared entry is absent before feature 9 starts.
    allfeatur.extend(allfeatur_row(9, [0x90, 0x01], 913, &[8, 0xe3]));
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert!(scan.features.entity_tables.is_empty());
}

#[test]
fn scan_bounds_known_allfeatur_feature_rows() {
    let mut geometry = visibgeom_payload(2, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    geometry.extend_from_slice(&[8, 0x22, 9, 0x01, 0, 0]);
    let mut allfeatur = allfeatur_row(4, [0xeb, 0x04], 917, &[0xaa, 0xbb, 0xe3]);
    allfeatur.extend(allfeatur_row(9, [0x90, 0x01], 913, &[0xcc]));
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.features.rows.len(), 2);
    assert_eq!(scan.features.rows[0].feature_id, 4);
    assert_eq!(scan.features.rows[0].header, [0xeb, 0x04]);
    assert_eq!(
        scan.features.rows[0].body,
        vec![
            0xeb, 0x04, 0x00, 0x10, 0x01, 0x80, 0x80, 0x00, 0xe4, 0xe3, 0xf6, 0x83, 0x95, 0xe1,
            0xaa, 0xbb, 0xe3,
        ]
    );
    assert_eq!(scan.features.rows[1].feature_id, 9);
    assert_eq!(
        scan.features.rows[1].body,
        vec![
            0x90, 0x01, 0x00, 0x10, 0x01, 0x80, 0x80, 0x00, 0xe4, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
            0xcc,
        ]
    );
}

#[test]
fn scan_decodes_allfeatur_root_featdefs_schema_class() {
    let mut geometry = visibgeom_payload(2, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    geometry.extend_from_slice(&[8, 0x22, 9, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 0xe3, 9,
        0xeb, 0x04, 0, 0x10, 1, 0, 0xe5, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
    ];
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            (
                "MdlStatus",
                b"protrevolve\0Revolve id 4\0Round id 9\0".to_vec(),
            ),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.rows[0].root_schema_class, Some(917));
    assert_eq!(scan.features.rows[1].root_schema_class, Some(913));

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(
        result.ir().model.features[0]
            .source_properties
            .get("featdefs_schema_class")
            .map(String::as_str),
        Some("917")
    );
    assert_eq!(
        result.ir().model.features[0]
            .source_properties
            .get("recipe")
            .map(String::as_str),
        Some("protrevolve")
    );
    assert_eq!(
        result.ir().model.features[1]
            .source_properties
            .get("featdefs_schema_class")
            .map(String::as_str),
        Some("913")
    );
}

#[test]
fn scan_resolves_allfeatur_walker_order_entity_references() {
    let allfeatur =
        b"\xe0\x00Sld_Features\0\xe0\x22first\0\xf7\x02\xe3\xe0\x24second\0\xf7\x01\xe3".to_vec();
    let data = build_prt("c", &[("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.entities.len(), 3);
    assert_eq!(scan.features.entities[0].entity_id, 0);
    assert_eq!(scan.features.entities[0].name, "Sld_Features");
    assert_eq!(scan.features.entities[1].entity_id, 1);
    assert_eq!(scan.features.entities[1].name, "first");
    assert_eq!(scan.features.entity_references.len(), 2);
    assert_eq!(scan.features.entity_references[0].source_entity_id, Some(1));
    assert_eq!(scan.features.entity_references[0].target_entity_id, 2);
    assert!(scan.features.entity_references[0].target_resolved);
    assert_eq!(scan.features.entity_references[1].source_entity_id, Some(2));
    assert_eq!(scan.features.entity_references[1].target_entity_id, 1);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let namespace = result
        .ir()
        .native
        .namespace("creo")
        .expect("creo namespace");
    let entities = &namespace.arenas["feature_entities"];
    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].id(), "creo:allfeatur:entity#0");
    assert_eq!(entities[0].fields()["type_byte"], 0);
    assert_eq!(entities[0].fields()["name"], "Sld_Features");
    let references = &namespace.arenas["feature_entity_references"];
    assert_eq!(references.len(), 2);
    let forward = references
        .iter()
        .find(|reference| reference.fields()["target_entity_id"] == 2)
        .expect("forward reference");
    assert_eq!(forward.fields()["source_entity_id"], 1);
    assert_eq!(forward.fields()["target_resolved"], true);
    assert_annotation(
        &result.source_fidelity().annotations,
        entities[0].id(),
        "creo:AllFeatur",
        scan.features.entities[0].offset as u64,
        "feature_entity",
        Exactness::ByteExact,
    );
}

#[test]
fn scan_bounds_allfeatur_procedural_choice_spans() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        b"\xe0\x22blend_choice\0\x11\x12\xe0\x24depth_choice\0\x07",
    );
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.features.choices.len(), 2);
    assert_eq!(scan.features.choices[0].feature_id, 4);
    assert_eq!(scan.features.choices[0].label, "blend_choice");
    assert_eq!(scan.features.choices[0].type_byte, Some(0x22));
    assert_eq!(scan.features.choices[0].payload, vec![0x11, 0x12]);
    assert_eq!(scan.features.choices[1].label, "depth_choice");
    assert_eq!(scan.features.choices[1].payload, vec![0x07]);
}

#[test]
fn scan_decodes_allfeatur_choice_field_wrappers() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        913,
        b"\xe0\x22blend_choice\0\xe0\x21count\0\x07\xe0\x22refs\0\xf8\x02\x03\x04\xe0\x24depth_choice\0",
    );
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.choice_fields.len(), 2);
    assert_eq!(scan.features.choice_fields[0].name, "count");
    assert_eq!(
        scan.features.choice_fields[0].value,
        crate::feature::FeatureFieldValue::CompactInt(7)
    );
    assert_eq!(scan.features.choice_fields[1].name, "refs");
    assert_eq!(
        scan.features.choice_fields[1].value,
        crate::feature::FeatureFieldValue::CompactIntArray(vec![3, 4])
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = &result.ir().model.features[0];
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [group]
            if matches!(group.edges, cadmpeg_ir::features::EdgeSelection::Unresolved)
                && group.radius.is_unresolved())
    ));
    assert_eq!(
        feature.source_properties["native_parameter.choice.blend_choice.count"],
        "7"
    );
    assert_eq!(
        feature.source_properties["native_parameter.choice.blend_choice.refs"],
        "3,4"
    );
}

#[test]
fn scan_decodes_complete_allfeatur_f9_scalar_slots() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let mut allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        b"\xe0\x22blend_choice\0\xe0\x21values\0\xf9\x01\x03\x0f\xe4",
    );
    allfeatur.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(
        scan.features.choice_fields[0].value,
        crate::feature::FeatureFieldValue::ScalarArray {
            dimensions: 1,
            count: 3,
            body: vec![0x0f, 0xe4, 0x46, 0x08, 0, 0, 0, 0, 0, 0],
            decoded_values: Some(vec![0.0, 1.0, 3.0]),
        }
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let namespace = result.ir().native.namespace("creo").unwrap();
    let rows = &namespace.arenas["feature_rows"];
    assert_eq!(rows[0].fields()["owner_feature_id"], 4);
    assert_eq!(rows[0].fields()["header"][0], 0xeb);
    assert_eq!(rows[0].fields()["header"][1], 0x04);
    assert_eq!(rows[0].fields()["body"][0], 0xeb);
    assert_eq!(rows[0].fields()["body"][14], 0xe0);
    let choices = &namespace.arenas["feature_choices"];
    assert_eq!(choices[0].fields()["owner_feature_id"], 4);
    assert_eq!(choices[0].fields()["label"], "blend_choice");
    let fields = &namespace.arenas["feature_choice_fields"];
    assert_eq!(fields[0].fields()["choice_label"], "blend_choice");
    assert_eq!(fields[0].fields()["name"], "values");
    assert_eq!(fields[0].fields()["value"]["kind"], "scalar_array");
    assert_eq!(fields[0].fields()["value"]["decoded_values"][2], 3.0);
}

#[test]
fn scan_decodes_allfeatur_generated_geometry_manifest() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        b"edg_id_tab_ptr\0\xf1\xf8\x03\xf7\x53\xfb\xe3used_bodies\0\xf8\x01\xf7\x60\xfb\xe2dtm_id_tab\0\xf2\xf8\x02\xf7\x57\xfb\xe2\xe0\x01dtm_id\0\x2a\xe0\x01dtm_id\0\x2b",
    );
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.geometry_tables.len(), 3);
    assert_eq!(scan.features.geometry_tables[0].feature_id, 4);
    assert_eq!(
        scan.features.geometry_tables[0].kind,
        crate::feature::FeatureGeometryTableKind::EdgeIds
    );
    assert_eq!(scan.features.geometry_tables[0].count, 3);
    assert_eq!(scan.features.geometry_tables[0].entity_class, 0x53);
    assert_eq!(
        scan.features.geometry_tables[1].kind,
        crate::feature::FeatureGeometryTableKind::UsedBodies
    );
    assert_eq!(
        scan.features.geometry_tables[2].kind,
        crate::feature::FeatureGeometryTableKind::DatumIds
    );
    assert_eq!(
        scan.features.geometry_tables[2].entry_ids,
        Some(vec![42, 43])
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let tables = &result.ir().native.namespace("creo").unwrap().arenas["feature_geometry_tables"];
    assert_eq!(tables.len(), 3);
    assert_eq!(tables[0].fields()["owner_feature_id"], 4);
    assert_eq!(tables[0].fields()["kind"], "edge_ids");
    assert_eq!(tables[0].fields()["declared_count"], 3);
    assert_eq!(tables[0].fields()["entity_class_id"], 0x53);
    assert_eq!(tables[2].fields()["entry_ids"][0], 42);
    assert_eq!(tables[2].fields()["entry_ids"][1], 43);
    assert_annotation(
        &result.source_fidelity().annotations,
        tables[0].id(),
        "creo:AllFeatur",
        scan.features.geometry_tables[0].offset as u64,
        "feature_geometry_table",
        Exactness::ByteExact,
    );
}

#[test]
fn scan_decodes_complete_allfeatur_loop_history_rosters() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        b"\xe0\x00lo_id_tab_ptr\0\xf8\x02\xf7\x60\xfb\xe3\
        \xe0\x01lo_hist\0\xf8\x06\x2a\x01\xf6\xe5\x02\xf1\xf7\x60\xe3\
        \x2b\x03\x04\xe4\xf6\x05\xe0\x00next\0",
    );
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.loop_history_entries.len(), 2);
    assert_eq!(scan.features.loop_history_entries[0].feature_id, 4);
    assert_eq!(scan.features.loop_history_entries[0].ordinal, 0);
    assert_eq!(scan.features.loop_history_entries[0].loop_id, 42);
    assert_eq!(scan.features.loop_history_entries[1].ordinal, 1);
    assert_eq!(scan.features.loop_history_entries[1].loop_id, 43);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records =
        &result.ir().native.namespace("creo").unwrap().arenas["feature_loop_history_entries"];
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].fields()["owner_feature_id"], 4);
    assert_eq!(records[0].fields()["ordinal"], 0);
    assert_eq!(records[0].fields()["loop_id"], 42);
    assert_eq!(records[0].fields()["field_bytes"][0][0], 1);
    assert_eq!(records[0].fields()["boundary"], "reference_continue");
    assert_eq!(records[0].fields()["boundary_reference"], 96);
    assert_eq!(records[1].fields()["ordinal"], 1);
    assert_eq!(records[1].fields()["loop_id"], 43);
    assert_eq!(
        records[1].fields()["field_bytes"].as_array().unwrap().len(),
        5
    );
    assert_eq!(records[1].fields()["boundary"], "named_record");
    assert_annotation(
        &result.source_fidelity().annotations,
        records[0].id(),
        "creo:AllFeatur",
        scan.features.loop_history_entries[0].offset as u64,
        "feature_loop_history_entry",
        Exactness::ByteExact,
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_FEATURE_LOOP_HISTORY_ENTRY_COUNT),
        2
    );
}

#[test]
fn scan_decodes_allfeatur_affected_id_arrays() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        b"\xe0\x21geoms_affected\0\xf8\x03\x07\x80\x80\x09\
        \xe0\x22contours\0\xf8\x01\x2a\xe0\x01parent_table\0\xf8\x02\x01\x03",
    );
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.affected_ids.len(), 3);
    assert_eq!(
        scan.features.affected_ids[0].kind,
        crate::feature::AffectedIdKind::Geometry
    );
    assert_eq!(scan.features.affected_ids[0].ids, vec![7, 128, 9]);
    assert_eq!(
        scan.features.affected_ids[1].kind,
        crate::feature::AffectedIdKind::Contours
    );
    assert_eq!(scan.features.affected_ids[1].ids, vec![42]);
    assert_eq!(
        scan.features.affected_ids[2].kind,
        crate::feature::AffectedIdKind::Parents
    );
    assert_eq!(scan.features.affected_ids[2].ids, vec![1, 3]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records = &result.ir().native.namespace("creo").unwrap().arenas["feature_affected_ids"];
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].fields()["owner_feature_id"], 4);
    assert_eq!(records[0].fields()["kind"], "geometry");
    assert_eq!(records[0].fields()["ids"][1], 128);
}

#[test]
fn scan_partitions_allfeatur_positional_round_operands() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let mut allfeatur = b"\x04\xeb\x04\xe3\xf6\x83\x91\xe1\xf1\xf7\x42\xd8\x80\x01\xe3\xf8\x02\x07\x80\x80\xf8\x01\x09".to_vec();
    allfeatur.extend_from_slice(&[0xf5, 0x96, 0x92]);
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.replay_affected_ids.len(), 1);
    assert_eq!(scan.features.replay_affected_ids[0].feature_id, 4);
    assert_eq!(
        scan.features.replay_affected_ids[0].geometry_ids,
        vec![7, 128]
    );
    assert_eq!(scan.features.replay_affected_ids[0].edge_ids, vec![9]);
    assert_eq!(
        scan.features.replay_affected_ids[0].geometry_extent,
        crate::feature::ReplayExtentSource::Explicit
    );
    assert_eq!(
        scan.features.replay_affected_ids[0].edge_extent,
        crate::feature::ReplayExtentSource::Explicit
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert!(matches!(
        &result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [group]
            if matches!(&group.edges, cadmpeg_ir::features::EdgeSelection::Native(selection)
                if selection == "creo:allfeatur:replay_edgs_affected#4:9")
                && group.radius.is_unresolved())
    ));
    let records =
        &result.ir().native.namespace("creo").unwrap().arenas["feature_replay_affected_ids"];
    assert_eq!(records[0].fields()["geometry_extent"], "explicit");
    assert_eq!(records[0].fields()["edge_ids"][0], 9);
}

#[test]
fn scan_decodes_allfeatur_loop_restore_direction_compact_integers() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        1104,
        b"lo_restore\0\xe0\x01direction\0\x00\
        \xe0\x01direction2\0\x80\xa7\xe0\x01direction\0\x01",
    );
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.loop_restore_directions.len(), 3);
    assert_eq!(scan.features.loop_restore_directions[0].value, 0);
    assert_eq!(scan.features.loop_restore_directions[1].value, 167);
    assert_eq!(scan.features.loop_restore_directions[2].value, 1);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records =
        &result.ir().native.namespace("creo").unwrap().arenas["feature_loop_restore_directions"];
    assert_eq!(records[0].fields()["value"], 0);
    assert_eq!(records[1].fields()["value"], 167);
    assert_eq!(records[2].fields()["value"], 1);
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("feature");
    let cadmpeg_ir::features::FeatureDefinition::Native { parameters, .. } = &feature.definition
    else {
        panic!("native feature");
    };
    assert_eq!(parameters["loop_restore.direction"], "0");
    assert_eq!(parameters["loop_restore.direction#2"], "1");
    assert_eq!(parameters["loop_restore.direction2"], "167");
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FEATURE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_TYPED_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_NATIVE_FEATURE_COUNT),
        1
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("retain only source-native semantics")));
}

#[test]
fn scan_partitions_multiple_depdb_recipe_rows() {
    let depdb = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
        \xf7\x50\x9f\x77\x83\x94\xf6\x9f\x75Profile 2\0\xf6\0cutextrude\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data);

    assert_eq!(scan.features.depdb_recipe_rows.len(), 2);
    assert_eq!(scan.features.depdb_recipe_rows[0].feature_id, 8053);
    assert_eq!(
        scan.features.depdb_recipe_rows[0].root_schema_class,
        Some(917)
    );
    assert_eq!(scan.features.depdb_recipe_rows[1].feature_id, 8055);
    assert_eq!(
        scan.features.depdb_recipe_rows[1].root_schema_class,
        Some(916)
    );
    assert!(scan.features.depdb_recipe_rows[0].offset < scan.features.depdb_recipe_rows[1].offset);
    assert!(
        scan.features.depdb_recipe_rows[0].body_offset <= scan.features.depdb_recipe_rows[0].offset
    );
}

#[test]
fn scan_binds_standalone_depdb_section_to_its_recipe_owner() {
    let mut depdb = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0\
        var_arr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2"
        .to_vec();
    depdb.extend_from_slice(&[1, 7, 0xe4, 0x0f, 1, 0, 3, 0xe2]);
    depdb.extend_from_slice(&[2, 7, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 4, 0xe2]);
    depdb.extend_from_slice(
        b"\xe3Body ID 17\0\xe3\
        \xf7\x3b\x11\x83\x95\xf6\x04Profile 1\0\xf6\0protextrude\0",
    );
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.definitions.len(), 1);
    let definition = &scan.features.definitions[0];
    assert_eq!(definition.id, 2);
    assert_eq!(definition.owner_feature_id, Some(17));
    let variables = definition.variables.as_ref().expect("var_arr");
    assert_eq!(variables.points.len(), 1);
    assert_eq!(variables.points[0].point_id, 7);
    assert_eq!(variables.points[0].u, Some(1.0));
    assert_eq!(variables.points[0].v, Some(3.0));

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records = &result.ir().native.namespace("creo").unwrap().arenas["feature_definitions"];
    assert_eq!(records[0].fields()["source_section"], "DEPDB_DATA");
    assert_annotation(
        &result.source_fidelity().annotations,
        "creo:featdefs:feature_definition#2",
        "creo:DEPDB_DATA",
        definition.offset as u64,
        "feature_definition_record",
        Exactness::ByteExact,
    );
}

#[test]
fn scan_binds_standalone_depdb_datum_and_parent_tables_to_recipe_owner() {
    let depdb = b"nested dtm_id_tab\0\xe1\
        \xe0\x01dtm_id_tab\0\xf8\x01\xf7\x24\xe2\xe0\x01dtm_id\0\x29\
        \xe0\x01parent_table\0\xf8\x02\x03\x05\xf7\x24\xe3\
        Body ID 17\0\xe3\xf7\x3b\x11\x83\x95\xf6\x04Profile 1\0\xf6\0protextrude\0"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("DEPDB_DATA", depdb)]));

    let datum_table = scan
        .features
        .geometry_tables
        .iter()
        .find(|table| table.kind == crate::feature::FeatureGeometryTableKind::DatumIds)
        .expect("datum table");
    assert_eq!(datum_table.feature_id, 17);
    assert_eq!(datum_table.entry_ids.as_deref(), Some(&[41][..]));

    let parents = scan
        .features
        .affected_ids
        .iter()
        .find(|record| record.kind == crate::feature::AffectedIdKind::Parents)
        .expect("parent table");
    assert_eq!(parents.feature_id, 17);
    assert_eq!(parents.ids, [3, 5]);
}

#[test]
fn scan_distinguishes_null_and_referenced_family_tables() {
    let null_data = build_prt(
        "c",
        &[(
            "FamilyInf",
            b"Sld_FamilyInfo\0drv_tbl_ptr\0\xe1\xf1".to_vec(),
        )],
    );
    let null = container::scan_bytes(null_data.clone());
    assert_eq!(
        null.framing.family_table.unwrap().pointer,
        crate::container::FamilyTablePointer::Null
    );
    let decoded = CreoCodec
        .decode(&mut Cursor::new(null_data), &DecodeOptions::default())
        .expect("decode null family table");
    let configuration = &decoded.ir().native.namespace("creo").unwrap().arenas["configuration"];
    assert_eq!(configuration.len(), 1);
    assert_eq!(configuration[0].id(), "creo:family_info:driver_table#root");
    assert_eq!(configuration[0].fields()["pointer_kind"], "null");
    assert!(configuration[0].fields()["table_entity_id"].is_null());
    assert_eq!(
        decoded.ir().source.as_ref().unwrap().attributes["configuration_state"],
        "none"
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONFIGURATION_DRIVER_TABLE_REFERENCE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONFIGURATION_DRIVER_TABLE_COUNT),
        0
    );
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("configuration")));
    let referenced_data = build_prt(
        "c",
        &[(
            "FamilyInf",
            b"Sld_FamilyInfo\0drv_tbl_ptr\0\xf7\x81\x23\xf1".to_vec(),
        )],
    );
    let referenced = container::scan_bytes(referenced_data.clone());
    assert_eq!(
        referenced.framing.family_table.unwrap().pointer,
        crate::container::FamilyTablePointer::Entity(0x0123)
    );
    let decoded = CreoCodec
        .decode(&mut Cursor::new(referenced_data), &DecodeOptions::default())
        .expect("decode referenced family table");
    let configuration = &decoded.ir().native.namespace("creo").unwrap().arenas["configuration"];
    assert_eq!(
        configuration[0].fields()["pointer_kind"],
        "entity_reference"
    );
    assert_eq!(configuration[0].fields()["table_entity_id"], 0x0123);
    assert_eq!(
        decoded.ir().source.as_ref().unwrap().attributes["configuration_state"],
        "driver_table_unresolved"
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONFIGURATION_DRIVER_TABLE_REFERENCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONFIGURATION_DRIVER_TABLE_COUNT),
        0
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss
                .message
                .contains("1 referenced configuration driver table(s) retain unresolved")
    }));
}
