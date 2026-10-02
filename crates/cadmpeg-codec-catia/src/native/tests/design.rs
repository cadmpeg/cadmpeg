// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use cadmpeg_test_support::wire;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::test_object_graph::{
    catalog_stream, entity_backed_object_graph, object_graph_from_records, object_graph_record,
    sequential_entity_backed_object_graph, standard_catpart_with_nested_design_objects,
    value_block_stream,
};
use crate::CatiaCodec;

fn parallel_table() -> crate::native::CatiaDesignParallelReferenceTable {
    crate::native::CatiaDesignParallelReferenceTable::new(
        vec![crate::native::CatiaDesignReferenceColumn {
            field: "catia:test:field#0".to_owned(),
            field_class: None,
            list_payload_offset: 0,
        }],
        Vec::new(),
    )
    .expect("empty rows satisfy cardinality")
}

#[test]
fn design_parallel_table_borrowed_wire_preserves_json_bytes() {
    let table = parallel_table();
    let owned: crate::native::CatiaDesignParallelReferenceTableWire = table.clone().into();
    assert_eq!(
        serde_json::to_vec(&table).expect("borrowed table JSON"),
        serde_json::to_vec(&owned).expect("owned table JSON")
    );
}

#[test]
fn design_parallel_table_retained_limit_refuses_json_record() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        #[serde(flatten)]
        table: &'a crate::native::CatiaDesignParallelReferenceTable,
    }
    let table = parallel_table();

    let record = Record {
        id: "catia:test:parallel-table#0",
        table: &table,
    };
    let arena_name = "parallel_tables";
    let json_len = serde_json::to_vec(&record).expect("table JSON").len();
    let limit = u64::try_from(json_len + arena_name.len() - 1).expect("small JSON");
    let refused = crate::test_support::with_retained_limit(limit, |ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace.set_arena(ctx, arena_name, std::slice::from_ref(&record))
    });
    let error = refused.expect_err("record exceeds retained-byte limit");
    assert!(error.to_string().contains("RetainedBytes"), "{error}");
    crate::test_support::with_service_context(|ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace
            .set_arena(ctx, arena_name, std::slice::from_ref(&record))
            .expect("service profile admits table");
    });
}

fn design_reference_cell() -> crate::native::CatiaDesignReferenceCell {
    crate::native::CatiaDesignReferenceCell::Resolved {
        payload_offset: 12,
        entity_id: 7,
        field: "catia:test:field#7".to_owned(),
        field_class: Some(crate::native::CatiaDesignClass {
            entry: "Entry".to_owned(),
            name: "Name".to_owned(),
        }),
        design_object: Some("catia:test:design#0".to_owned()),
    }
}

#[test]
fn design_reference_cell_borrowed_wire_preserves_json_bytes() {
    let cell = design_reference_cell();
    let owned: crate::native::CatiaDesignReferenceCellWire = cell.clone().into();
    assert_eq!(
        serde_json::to_vec(&cell).expect("borrowed cell JSON"),
        serde_json::to_vec(&owned).expect("owned cell JSON")
    );
}

#[test]
fn design_reference_cell_retained_limit_refuses_json_record() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        #[serde(flatten)]
        cell: &'a crate::native::CatiaDesignReferenceCell,
    }
    let cell = design_reference_cell();

    let record = Record {
        id: "catia:test:reference-cell#0",
        cell: &cell,
    };
    let arena_name = "design_reference_cells";
    let json_len = serde_json::to_vec(&record).expect("cell JSON").len();
    let limit = u64::try_from(json_len + arena_name.len() - 1).expect("small JSON");
    let refused = crate::test_support::with_retained_limit(limit, |ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace.set_arena(ctx, arena_name, std::slice::from_ref(&record))
    });
    let error = refused.expect_err("record exceeds retained-byte limit");
    assert!(error.to_string().contains("RetainedBytes"), "{error}");
    crate::test_support::with_service_context(|ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace
            .set_arena(ctx, arena_name, std::slice::from_ref(&record))
            .expect("service profile admits cell");
    });
}

#[test]
fn native_design_objects_refuse_caller_collection_limit() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_nested_design_objects());
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::design_objects(ctx, &native.object_graphs, &native.entity_records)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
    let retained = crate::test_support::with_retained_limit(0, |ctx| {
        super::super::design_objects(ctx, &native.object_graphs, &native.entity_records)
    });
    assert!(
        matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_design_object_id")
    );
    let admitted = crate::test_support::with_service_context(|ctx| {
        super::super::design_objects(ctx, &native.object_graphs, &native.entity_records)
    })
    .expect("service profile admits design objects");
    assert_eq!(admitted, native.design_objects);
}

#[test]
fn parallel_reference_table_refuses_nested_collection_limit() {
    use std::collections::HashMap;

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
    let graph = &native.object_graphs[0];
    let owner = native.design_objects[0].owner_entity_id;
    let fields = graph
        .records
        .iter()
        .filter(|record| record.owner_entity_id() == Some(owner))
        .collect::<Vec<_>>();
    let indices = graph
        .records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| Some((record.entity_id()?, index)))
        .collect::<HashMap<_, _>>();
    let refused = crate::test_support::with_collection_limit(2, |ctx| {
        super::super::design_parallel_reference_table(ctx, &fields, graph, &indices)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_design_row_cells")
    );
    let retained = crate::test_support::with_retained_limit(0, |ctx| {
        super::super::design_parallel_reference_table(ctx, &fields, graph, &indices)
    });
    assert!(
        matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_design_column_field")
    );
    let admitted = crate::test_support::with_service_context(|ctx| {
        super::super::design_parallel_reference_table(ctx, &fields, graph, &indices)
    })
    .expect("service profile admits parallel reference table");
    assert_eq!(admitted, native.design_objects[0].parallel_reference_table);
}

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
    let bytes = [b"V5_CFV2\0".as_slice(), &[0; 8], bytes.as_slice()].concat();
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
        wire::coverage_count(
            decoded.report(),
            crate::coverage::DECODED_DESIGN_OBJECT_RELATION_COUNT.as_str()
        ),
        2
    );

    let mut malformed = native.clone();
    malformed.design_objects[0].relations.swap(0, 1);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
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
    let bytes = [b"V5_CFV2\0".as_slice(), &[0; 8], bytes.as_slice()].concat();
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
        wire::coverage_count(
            decoded.report(),
            crate::coverage::DECODED_DESIGN_UNOWNED_FIELD_RELATION_COUNT.as_str()
        ),
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
    let bytes = [b"V5_CFV2\0".as_slice(), &[0; 8], bytes.as_slice()].concat();
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
        wire::coverage_count(
            decoded.report(),
            crate::coverage::DECODED_DESIGN_SAME_OBJECT_RELATION_COUNT.as_str()
        ),
        1
    );
    assert_eq!(
        wire::coverage_count(
            decoded.report(),
            crate::coverage::DECODED_DESIGN_REFLEXIVE_FIELD_RELATION_COUNT.as_str()
        ),
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
            .map(super::super::CatiaObjectRecord::entity_id)
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

    let bytes = [b"V5_CFV2\0".as_slice(), &[0; 8], bytes.as_slice()].concat();
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode null storage role");

    assert_eq!(
        wire::coverage_count(
            decoded.report(),
            crate::coverage::UNRESOLVED_STORAGE_RECORD_COUNT.as_str()
        ),
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
    assert_eq!(table.rows().len(), 2);
    assert_eq!(
        table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(super::super::CatiaDesignReferenceCell::entity_id)
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
                    .map(super::super::CatiaDesignReferenceCell::payload_offset)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![2, 2], vec![4, 4]]
    );
    assert!(table.rows().iter().flat_map(|row| &row.cells).all(|cell| {
        cell.field().is_some() && cell.field_class().is_some() && cell.design_object().is_some()
    }));
    assert_eq!(
        table.rows()[0].matching_design_object,
        table.rows()[0].cells[0].design_object().map(str::to_owned)
    );
    assert!(table.rows()[0].matching_design_object.is_some());
    assert!(table.rows()[1].matching_design_object.is_none());

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
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
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
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
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
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_list_offset
        .store(&mut namespace)
        .expect("store malformed parallel-reference list offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

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
    assert!(null_table.rows()[1].cells.iter().all(|cell| {
        cell.entity_id() == 5
            && cell.is_null()
            && cell.field().is_none()
            && cell.design_object().is_none()
    }));

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

    assert!(table.rows()[0].matching_design_object.is_some());
    assert!(table.rows()[1].matching_design_object.is_none());
    assert_eq!(
        table.rows[1].cells[0].field(),
        table.rows[1].cells[1].field()
    );
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

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
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
        wire::coverage_count(
            decoded.report(),
            crate::coverage::DECODED_DESIGN_OBJECT_OWNER_LINK_COUNT.as_str()
        ),
        1
    );
}
