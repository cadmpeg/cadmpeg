// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn native_design_objects_preserve_payload_references_to_target_owners() {
    let bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x83],
            &[0x3b, 0x82, 0x81, 0x83, 0x81, 0x83, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0x81, 0x81, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0x81, 0x81, 0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_entity_id, 1);
    assert_eq!(native.design_objects[0].ordinal, 0);
    assert_eq!(
        native.design_objects[0].first_field_byte_offset,
        native.object_graphs[0].records[0].byte_offset
    );
    assert_eq!(native.design_objects[0].fields.len(), 2);
    assert!(native.design_objects[0].field_classes.is_empty());
    let graph = &native.object_graphs[0];
    assert_eq!(
        graph.records[0].design_object.as_deref(),
        Some(native.design_objects[0].id.as_str())
    );
    assert_eq!(
        graph.records[0].references,
        [
            crate::native::CatiaObjectRecordReference::from_parts(
                3,
                2,
                crate::native::CatiaObjectRecordReferenceSource::ListItem {
                    list_payload_offset: 0,
                    item_ordinal: 0,
                },
                false,
                Some(graph.records[2].id.clone()),
                graph.records[2].design_object.clone(),
            ),
            crate::native::CatiaObjectRecordReference::from_parts(
                3,
                4,
                crate::native::CatiaObjectRecordReferenceSource::ListItem {
                    list_payload_offset: 0,
                    item_ordinal: 1,
                },
                false,
                Some(graph.records[2].id.clone()),
                graph.records[2].design_object.clone(),
            ),
        ]
    );
    assert_eq!(
        native.design_objects[0].relations,
        [
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 2,
                    container: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                        list_payload_offset: 0,
                        item_ordinal: 0,
                    },
                },
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 4,
                    container: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                        list_payload_offset: 0,
                        item_ordinal: 1,
                    },
                },
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[1].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 0,
                    container: crate::native::CatiaObjectRecordReferenceSource::Field,
                },
                target_entity_id: 1,
                target_field: graph.records[0].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[0].id.clone()),
            },
        ]
    );
    assert_eq!(
        graph.records[1].references,
        [crate::native::CatiaObjectRecordReference::from_parts(
            1,
            0,
            crate::native::CatiaObjectRecordReferenceSource::Field,
            false,
            Some(graph.records[0].id.clone()),
            graph.records[0].design_object.clone(),
        )]
    );
    assert_eq!(native.design_objects[1].owner_entity_id, 3);
    assert_eq!(native.design_objects[1].ordinal, 1);
    assert_eq!(
        native.design_objects[1].first_field_byte_offset,
        native.object_graphs[0].records[2].byte_offset
    );
    assert_eq!(
        native.design_objects[1].relations,
        [crate::native::CatiaDesignObjectRelation {
            source_field: graph.records[2].id.clone(),
            source_class: None,
            source: crate::native::CatiaDesignObjectRelationSource::Payload {
                payload_offset: 0,
                container: crate::native::CatiaObjectRecordReferenceSource::Field,
            },
            target_entity_id: 1,
            target_field: graph.records[0].id.clone(),
            target_class: None,
            target_design_object: Some(native.design_objects[0].id.clone()),
        }]
    );
}

#[test]
fn native_design_objects_preserve_storage_relations_before_payload_relations() {
    let bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x84, 0x83], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x86], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let graph = &native.object_graphs[0];
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode storage and payload relations");

    assert_eq!(
        native.design_objects[0].relations,
        [
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Storage,
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 0,
                    container: crate::native::CatiaObjectRecordReferenceSource::Field,
                },
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
        ]
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_OBJECT_RELATION_COUNT),
        2
    );

    let mut malformed = native.clone();
    malformed.design_objects[0].relations.swap(0, 1);
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed
        .store(&mut namespace)
        .expect("store reordered design relations");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_design_objects_preserve_relations_to_unowned_fields() {
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81], &[0x81, 0x82, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0xe5, 0xff, 0xff, 0xff, 0xe4], &[0xfe]),
    ];
    let bytes = sequential_entity_backed_object_graph(&records);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode relation to unowned field");
    let native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load relation to unowned field");
    let graph = &native.object_graphs[0];

    assert_eq!(native.design_objects.len(), 1);
    assert_eq!(
        native.design_objects[0].relations,
        [crate::native::CatiaDesignObjectRelation {
            source_field: graph.records[0].id.clone(),
            source_class: None,
            source: crate::native::CatiaDesignObjectRelationSource::Payload {
                payload_offset: 0,
                container: crate::native::CatiaObjectRecordReferenceSource::Field,
            },
            target_entity_id: 2,
            target_field: graph.records[1].id.clone(),
            target_class: None,
            target_design_object: None,
        }]
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_UNOWNED_FIELD_RELATION_COUNT),
        1
    );
}

#[test]
fn native_design_objects_preserve_reflexive_field_relations() {
    let records = [object_graph_record(
        &[0x04, 0x01, 0x81],
        &[0x81, 0x81, 0xfe],
    )];
    let bytes = sequential_entity_backed_object_graph(&records);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode reflexive field relation");
    let native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load reflexive field relation");
    let field = &native.object_graphs[0].records[0];

    assert_eq!(
        native.design_objects[0].relations,
        [crate::native::CatiaDesignObjectRelation {
            source_field: field.id.clone(),
            source_class: None,
            source: crate::native::CatiaDesignObjectRelationSource::Payload {
                payload_offset: 0,
                container: crate::native::CatiaObjectRecordReferenceSource::Field,
            },
            target_entity_id: 1,
            target_field: field.id.clone(),
            target_class: None,
            target_design_object: Some(native.design_objects[0].id.clone()),
        }]
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_SAME_OBJECT_RELATION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_REFLEXIVE_FIELD_RELATION_COUNT),
        1
    );
}

#[test]
fn native_object_references_select_sparse_entity_identities() {
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x87, 0x86], &[0xfe]),
    ];
    let native =
        crate::native::CatiaNative::decode(&entity_backed_object_graph(&records, &[1, 3, 7]));
    let graph = &native.object_graphs[0];

    assert_eq!(
        native
            .entity_records
            .iter()
            .map(|record| record.entity_id)
            .collect::<Vec<_>>(),
        [1, 3, 7]
    );
    assert_eq!(
        graph
            .records
            .iter()
            .map(|record| record.entity_id())
            .collect::<Vec<_>>(),
        [Some(1), Some(3), Some(7)]
    );
    assert_eq!(
        graph.records[0].references[0].target(),
        Some(graph.records[1].id.as_str())
    );
    assert_ne!(
        graph.records[0].references[0].target(),
        Some(graph.records[2].id.as_str())
    );
    assert_eq!(
        native
            .design_objects
            .iter()
            .map(|object| object.owner_entity_id)
            .collect::<Vec<_>>(),
        [1, 3, 7]
    );
}

#[test]
fn native_design_relations_preserve_both_endpoint_schema_classes() {
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x86], &[0xfe]),
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Profile",
        "Limit",
        "Pad",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let relation = &native.design_objects[0].relations[0];
    assert_eq!(
        relation
            .source_class
            .as_ref()
            .map(|class| class.name.as_str()),
        Some("Profile")
    );
    assert_eq!(
        relation
            .target_class
            .as_ref()
            .map(|class| class.name.as_str()),
        Some("Pad")
    );
    assert_eq!(
        relation
            .source_class
            .as_ref()
            .map(|class| class.entry.as_str()),
        native.object_graphs[0].records[0].class_entry()
    );
    assert_eq!(
        relation
            .target_class
            .as_ref()
            .map(|class| class.entry.as_str()),
        native.object_graphs[0].records[2].class_entry()
    );
}

#[test]
fn compact_design_objects_use_field_vocabulary_not_anchor_class() {
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "BaseFeature",
        "Groove",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.design_objects.len(), 1);
    let object = &native.design_objects[0];
    assert_eq!(object.owner_entity_id, 2);
    assert!(object.owner_record.is_some());
    assert_eq!(object.owner_class, None);
    assert_eq!(object.owner_storage_ref, None);
    assert_eq!(
        object.field_classes,
        [
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[4].id.clone(),
                name: "BaseFeature".to_string(),
            },
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[5].id.clone(),
                name: "Groove".to_string(),
            },
        ]
    );
}

#[test]
fn null_storage_roles_are_not_unresolved_storage_links() {
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x16, 0x84, 0x80, 0x82], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "BaseFeature",
    ]));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode null storage role");

    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_STORAGE_RECORD_COUNT),
        0
    );
}

#[test]
fn native_design_objects_preserve_unresolved_owner_identities() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x80, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x84, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let graph = &native.object_graphs[0];

    assert_eq!(graph.records[0].owner_entity_id(), Some(0));
    assert_eq!(graph.records[1].owner_entity_id(), Some(4));
    assert!(graph
        .records
        .iter()
        .all(|record| record.design_object.is_some()));
    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_entity_id, 0);
    assert_eq!(native.design_objects[1].owner_entity_id, 4);
    assert!(native
        .design_objects
        .iter()
        .all(|object| object.owner_record.is_none()));
}

#[test]
fn native_design_objects_retain_and_validate_parallel_reference_tables() {
    let list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x84, 0x85, 0xfe];
    let list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &list_b),
        object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Profile",
        "Limit",
        "Profile",
        "Limit",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let table = native.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table");
    assert_eq!(
        table
            .columns
            .iter()
            .map(|column| &column.field)
            .collect::<Vec<_>>(),
        native.design_objects[0].fields.iter().collect::<Vec<_>>()
    );
    assert!(table
        .columns
        .iter()
        .all(|column| column.field_class.is_some()));
    assert!(table
        .columns
        .iter()
        .all(|column| column.list_payload_offset == 0));
    assert_eq!(table.rows.len(), 2);
    assert_eq!(
        table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| cell.entity_id())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![3, 4], vec![4, 3]]
    );
    assert_eq!(
        table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| cell.payload_offset())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![2, 2], vec![4, 4]]
    );
    assert!(table.rows.iter().flat_map(|row| &row.cells).all(|cell| {
        cell.field().is_some() && cell.field_class().is_some() && cell.design_object().is_some()
    }));
    assert_eq!(
        table.rows[0].matching_design_object,
        table.rows[0].cells[0].design_object().map(str::to_owned)
    );
    assert!(table.rows[0].matching_design_object.is_some());
    assert!(table.rows[1].matching_design_object.is_none());

    let expected = table.clone();
    let mut malformed = native.clone();
    let malformed_cell = malformed.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table")
        .rows[0]
        .cells[0]
        .clone();
    let malformed_entity_id = malformed_cell.entity_id() + 1;
    malformed.design_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows[0]
        .cells[0] = malformed_cell.with_entity_id(malformed_entity_id);
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed
        .store(&mut namespace)
        .expect("store malformed parallel reference table");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_offset = native.clone();
    let offset_cell = malformed_offset.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table")
        .rows[0]
        .cells[0]
        .clone();
    let next_offset = offset_cell.payload_offset() + 1;
    malformed_offset.design_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows[0]
        .cells[0] = offset_cell.with_payload_offset(next_offset);
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_offset
        .store(&mut namespace)
        .expect("store malformed parallel-reference cell offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_list_offset = native.clone();
    malformed_list_offset.design_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .columns[0]
        .list_payload_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_list_offset
        .store(&mut namespace)
        .expect("store malformed parallel-reference list offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut version_256_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut version_256_namespace)
        .expect("store pre-column-incidence parallel reference table");
    let mut stored_fields = version_256_namespace
        .arenas
        .get_mut("design_objects")
        .expect("stored design objects")[0]
        .fields_mut();
    let columns = stored_fields
        .get_mut("parallel_reference_table")
        .expect("stored parallel reference table")
        .as_object_mut()
        .expect("stored parallel reference table")
        .get_mut("columns")
        .expect("stored parallel reference columns")
        .as_array_mut()
        .expect("stored parallel reference columns");
    for column in columns {
        *column = column
            .as_object()
            .expect("stored parallel reference column")["field"]
            .clone();
    }
    drop(stored_fields);
    version_256_namespace.set_version(
        std::num::NonZeroU32::new(
            crate::native::CATIA_PARALLEL_REFERENCE_COLUMN_INCIDENCE_VERSION - 1,
        )
        .unwrap(),
    );
    let migrated = crate::native::CatiaNative::load(&version_256_namespace)
        .expect("migrate parallel-reference column incidences");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_255_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut version_255_namespace)
        .expect("store pre-offset parallel reference table");
    let mut version_255_objects: Vec<crate::native::CatiaDesignObject> = version_255_namespace
        .arena_as("design_objects")
        .expect("load version 255 design objects");
    for cell in version_255_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
    {
        *cell = cell.clone().with_payload_offset(0);
    }
    version_255_namespace
        .set_arena("design_objects", &version_255_objects)
        .expect("store version 255 design objects");
    version_255_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_PARALLEL_REFERENCE_CELL_OFFSET_VERSION - 1)
            .unwrap(),
    );
    let migrated = crate::native::CatiaNative::load(&version_255_namespace)
        .expect("migrate parallel-reference cell offsets");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut previous_namespace)
        .expect("store current parallel reference table");
    let mut previous_objects: Vec<crate::native::CatiaDesignObject> = previous_namespace
        .arena_as("design_objects")
        .expect("load stored design objects");
    previous_objects[0].parallel_reference_table = None;
    previous_namespace
        .set_arena("design_objects", &previous_objects)
        .expect("store previous design objects");
    previous_namespace.set_version(std::num::NonZeroU32::new(200).unwrap());
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate previous parallel reference table");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_203_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut version_203_namespace)
        .expect("store current parallel reference row matches");
    let mut version_203_objects: Vec<crate::native::CatiaDesignObject> = version_203_namespace
        .arena_as("design_objects")
        .expect("load version 203 design objects");
    for row in &mut version_203_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
    {
        row.matching_design_object = None;
    }
    version_203_namespace
        .set_arena("design_objects", &version_203_objects)
        .expect("store version 203 design objects");
    version_203_namespace.set_version(std::num::NonZeroU32::new(203).unwrap());
    let migrated = crate::native::CatiaNative::load(&version_203_namespace)
        .expect("migrate version 203 parallel reference row matches");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_202_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut version_202_namespace)
        .expect("store current classified parallel reference columns");
    let mut version_202_objects: Vec<crate::native::CatiaDesignObject> = version_202_namespace
        .arena_as("design_objects")
        .expect("load version 202 design objects");
    for column in &mut version_202_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .columns
    {
        column.field_class = None;
    }
    version_202_namespace
        .set_arena("design_objects", &version_202_objects)
        .expect("store version 202 design objects");
    version_202_namespace.set_version(std::num::NonZeroU32::new(202).unwrap());
    let migrated = crate::native::CatiaNative::load(&version_202_namespace)
        .expect("migrate version 202 source field classes");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_201_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut version_201_namespace)
        .expect("store current classified parallel reference table");
    let mut version_201_objects: Vec<crate::native::CatiaDesignObject> = version_201_namespace
        .arena_as("design_objects")
        .expect("load version 201 design objects");
    for cell in version_201_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
    {
        *cell = cell.clone().without_field_class();
    }
    version_201_namespace
        .set_arena("design_objects", &version_201_objects)
        .expect("store version 201 design objects");
    version_201_namespace.set_version(std::num::NonZeroU32::new(201).unwrap());
    let migrated = crate::native::CatiaNative::load(&version_201_namespace)
        .expect("migrate version 201 target field classes");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected)
    );

    let null_list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x85, 0x85, 0xfe];
    let null_list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x85, 0x86, 0xfe];
    let terminal_null =
        crate::native::CatiaNative::decode(&sequential_entity_backed_object_graph(&[
            object_graph_record(&[0x04, 0x01, 0x81, 0x83], &null_list_a),
            object_graph_record(&[0x04, 0x01, 0x81, 0x84], &null_list_b),
            object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
            object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
        ]));
    let null_table = terminal_null.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table with terminal null row");
    assert!(null_table.rows[1].cells.iter().all(|cell| {
        cell.entity_id() == 5
            && cell.is_null()
            && cell.field().is_none()
            && cell.design_object().is_none()
    }));

    let mut version_210_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    terminal_null
        .store(&mut version_210_namespace)
        .expect("store terminal null parallel reference cells");
    let mut version_210_records: Vec<crate::native::CatiaObjectRecord> = version_210_namespace
        .arena_as("object_graph_records")
        .expect("load version 210 object records");
    for reference in version_210_records
        .iter_mut()
        .flat_map(|record| &mut record.references)
    {
        *reference = reference.clone().with_null_cleared();
    }
    version_210_namespace
        .set_arena("object_graph_records", &version_210_records)
        .expect("store version 210 object records");
    let mut version_210_objects: Vec<crate::native::CatiaDesignObject> = version_210_namespace
        .arena_as("design_objects")
        .expect("load version 210 design objects");
    for cell in version_210_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
    {
        *cell = cell.clone().with_null_cleared();
    }
    version_210_namespace
        .set_arena("design_objects", &version_210_objects)
        .expect("store version 210 design objects");
    version_210_namespace.set_version(std::num::NonZeroU32::new(210).unwrap());
    let migrated = crate::native::CatiaNative::load(&version_210_namespace)
        .expect("migrate terminal null parallel reference cells");
    assert!(migrated.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("migrated parallel reference table")
        .rows[1]
        .cells
        .iter()
        .all(|cell| cell.is_null()));

    let three_references = [0x3b, 0x83, 0x81, 0x83, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
    let mismatched = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &three_references),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x84, 0x86], &[0xfe]),
    ]);
    assert!(
        crate::native::CatiaNative::decode(&mismatched).design_objects[0]
            .parallel_reference_table
            .is_none()
    );
}

#[test]
fn parallel_reference_row_match_requires_distinct_target_fields() {
    let list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x83, 0x85, 0xfe];
    let list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &list_b),
        object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Profile",
        "Profile",
        "Profile",
        "Profile",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let table = native.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table");

    assert!(table.rows[0].matching_design_object.is_some());
    assert!(table.rows[1].matching_design_object.is_none());
    assert_eq!(
        table.rows[1].cells[0].field(),
        table.rows[1].cells[1].field()
    );

    let mut version_204_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut version_204_namespace)
        .expect("store current parallel reference row matches");
    let mut version_204_objects: Vec<crate::native::CatiaDesignObject> = version_204_namespace
        .arena_as("design_objects")
        .expect("load version 204 design objects");
    version_204_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows[1]
        .matching_design_object = table.rows[1].cells[0]
        .design_object()
        .map(str::to_owned)
        .clone();
    version_204_namespace
        .set_arena("design_objects", &version_204_objects)
        .expect("store version 204 design objects");
    version_204_namespace.set_version(std::num::NonZeroU32::new(204).unwrap());

    let migrated = crate::native::CatiaNative::load(&version_204_namespace)
        .expect("migrate version 204 parallel reference row matches");
    assert!(migrated.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("migrated parallel reference table")
        .rows[1]
        .matching_design_object
        .is_none());
}

#[test]
fn native_design_objects_follow_first_field_order() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x83, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(
        native
            .design_objects
            .iter()
            .map(|object| object.owner_entity_id)
            .collect::<Vec<_>>(),
        [3, 1]
    );
    assert_eq!(native.design_objects[0].fields.len(), 2);
    assert_eq!(native.design_objects[1].fields.len(), 1);
    assert_eq!(
        native
            .design_objects
            .iter()
            .map(|object| (object.ordinal, object.first_field_byte_offset))
            .collect::<Vec<_>>(),
        [
            (0, native.object_graphs[0].records[0].byte_offset),
            (1, native.object_graphs[0].records[1].byte_offset),
        ]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store source-ordered design objects");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load source-ordered design objects");
    assert_eq!(
        loaded
            .design_objects
            .iter()
            .map(|object| object.owner_entity_id)
            .collect::<Vec<_>>(),
        [3, 1]
    );
}

#[test]
fn decode_links_design_objects_through_their_owner_record_group() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_nested_design_objects()),
            &DecodeOptions::default(),
        )
        .expect("decode nested design objects");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_entity_id, 2);
    assert_eq!(native.design_objects[1].owner_entity_id, 3);
    assert_eq!(
        native.design_objects[0].owner_design_object.as_deref(),
        Some(native.design_objects[1].id.as_str())
    );
    assert_eq!(native.design_objects[1].owner_design_object, None);
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_OBJECT_OWNER_LINK_COUNT),
        1
    );
}
