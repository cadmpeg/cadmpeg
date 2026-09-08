// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn native_load_rejects_orphaned_and_ambiguously_owned_design_records() {
    let mut bytes = object_graph_stream();
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA native namespace");

    for arena_name in ["catalogs", "object_graphs"] {
        let mut malformed = namespace.clone();
        malformed
            .arenas_mut()
            .get_mut(arena_name)
            .expect("owner arena")
            .clear();
        assert!(matches!(
            crate::native::CatiaNative::load(&malformed),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }

    for arena_name in ["catalogs", "object_graphs"] {
        let mut malformed = namespace.clone();
        let arena = malformed
            .arenas_mut()
            .get_mut(arena_name)
            .expect("owner arena");
        arena.push(arena.first().expect("owner record").clone());
        assert!(matches!(
            crate::native::CatiaNative::load(&malformed),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }

    let mut stale_design_objects = namespace.clone();
    stale_design_objects
        .arenas_mut()
        .get_mut("design_objects")
        .expect("derived design-object arena")
        .clear();
    assert!(matches!(
        crate::native::CatiaNative::load(&stale_design_objects),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_dangling_cross_arena_links() {
    let mut value_native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    value_native.value_blocks[0].catalog = "catia:missing-catalog".to_string();
    let mut value_namespace = cadmpeg_ir::NativeNamespace::default();
    value_native
        .store(&mut value_namespace)
        .expect("store malformed value link");
    assert!(matches!(
        crate::native::CatiaNative::load(&value_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut omitted_value_graph =
        crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    omitted_value_graph.value_blocks[0].object_graph = None;
    let mut omitted_value_namespace = cadmpeg_ir::NativeNamespace::default();
    omitted_value_graph
        .store(&mut omitted_value_namespace)
        .expect("store omitted value-block graph link");
    assert!(matches!(
        crate::native::CatiaNative::load(&omitted_value_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut external_native =
        crate::native::CatiaNative::decode(&external_reference_segment("Support.CATPart"));
    external_native.external_references[0].segment = "catia:missing-segment".to_string();
    let mut external_namespace = cadmpeg_ir::NativeNamespace::default();
    external_native
        .store(&mut external_namespace)
        .expect("store malformed external-reference link");
    assert!(matches!(
        crate::native::CatiaNative::load(&external_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut alias_native = crate::native::CatiaNative::decode(&surface_alias_stream());
    alias_native.alias_rows[0].object_graph = Some("catia:missing-graph".to_string());
    alias_native.alias_rows[0].object_record = Some("catia:missing-record".to_string());
    let mut alias_namespace = cadmpeg_ir::NativeNamespace::default();
    alias_native
        .store(&mut alias_namespace)
        .expect("store malformed alias link");
    assert!(matches!(
        crate::native::CatiaNative::load(&alias_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let graph =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let mut linked_alias = surface_alias_stream();
    linked_alias[15] = 1;
    let mut linked_stream = graph;
    linked_stream.extend(linked_alias);
    let (linked_bytes, _) = outer_container_catpart(&linked_stream);
    let mut omitted_alias_links = crate::native::CatiaNative::decode(&linked_bytes);
    assert!(omitted_alias_links.alias_rows[0].object_graph.is_some());
    omitted_alias_links.alias_rows[0].object_graph = None;
    omitted_alias_links.alias_rows[0].object_record = None;
    let mut omitted_alias_namespace = cadmpeg_ir::NativeNamespace::default();
    omitted_alias_links
        .store(&mut omitted_alias_namespace)
        .expect("store omitted alias links");
    assert!(matches!(
        crate::native::CatiaNative::load(&omitted_alias_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_noncanonical_catalog_and_record_views() {
    let mut bytes = object_graph_stream();
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store catalogs");
    let mut catalogs: Vec<serde_json::Value> = namespace.arena_as("catalogs").unwrap();
    catalogs[0]["declared_count"] = serde_json::json!(native.catalogs[0].declared_count() + 1);
    namespace.set_arena("catalogs", &catalogs).unwrap();
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_entry_ordinal = native.clone();
    invalid_entry_ordinal.catalogs[0].entries[0].ordinal = 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_entry_ordinal
        .store(&mut namespace)
        .expect("store invalid catalog ordinal");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_record_ordinal = native.clone();
    invalid_record_ordinal.object_graphs[0].records[0].ordinal = 9;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_record_ordinal
        .store(&mut namespace)
        .expect("store invalid record ordinal");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_design_link = native.clone();
    invalid_design_link.object_graphs[0].records[0].design_object = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_design_link
        .store(&mut namespace)
        .expect("store invalid design-object link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_references = native;
    invalid_references.object_graphs[0].records[0]
        .references
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_references
        .store(&mut namespace)
        .expect("store invalid payload-reference links");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_noncanonical_value_block_views() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    let mut canonical_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut canonical_namespace)
        .expect("store canonical value selections");
    assert!(canonical_namespace
        .arenas()
        .get("value_blocks")
        .is_some_and(|blocks| blocks
            .iter()
            .all(|block| !block.fields().contains_key("schema_selections"))));
    assert_eq!(
        canonical_namespace
            .arenas()
            .get("value_schema_selections")
            .map(Vec::len),
        Some(native.value_blocks[0].schema_selections.len())
    );
    let mut orphaned_selections: Vec<crate::native::CatiaValueSchemaSelection> =
        canonical_namespace
            .arena_as("value_schema_selections")
            .expect("load stored value selections");
    orphaned_selections[0].parent = "catia:missing-value-block".to_string();
    canonical_namespace
        .set_arena("value_schema_selections", &orphaned_selections)
        .expect("store orphaned value selection");
    assert!(matches!(
        crate::native::CatiaNative::load(&canonical_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed value-block view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut invalid_wire = serde_json::to_value(&native.value_blocks[0]).unwrap();
    invalid_wire["declared_len"] = serde_json::json!(native.value_blocks[0].declared_len() + 1);
    assert!(serde_json::from_value::<crate::native::CatiaValueBlock>(invalid_wire).is_err());

    let mut invalid_payload = native.clone();
    invalid_payload.value_blocks[0].payload.push(0x80);
    assert_rejected(invalid_payload);

    let mut invalid_wire = serde_json::to_value(&native.value_blocks[0]).unwrap();
    invalid_wire["fields"] = serde_json::json!([]);
    assert!(serde_json::from_value::<crate::native::CatiaValueBlock>(invalid_wire).is_err());

    let mut invalid_selections = native;
    assert!(!invalid_selections.value_blocks[0]
        .schema_selections
        .is_empty());
    invalid_selections.value_blocks[0].schema_selections.clear();
    assert_rejected(invalid_selections);
}

#[test]
fn schema_configuration_productions_retain_exact_same_graph_incidence() {
    let file = standard_catpart_with_configuration_incidences(8, 5, 7);
    let native = crate::native::CatiaNative::decode(&file);
    let configuration = native.entity_records[0]
        .schema_configuration_record()
        .expect("complete schema-configuration production");
    assert_eq!(configuration.schema_ordinal, 8);
    assert_eq!(configuration.schema_name, "Boolean");
    assert_eq!(configuration.schema_payload_offset, 0);
    assert_eq!(configuration.entity_reference.payload_offset, 10);
    assert_eq!(configuration.entity_reference.reference.entity_id(), 5);
    assert_eq!(
        configuration.entity_reference.reference.entity(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        configuration.entity_reference.reference.class_name(),
        Some("Configuration")
    );
    let row = native.entity_records[1]
        .schema_configuration_row_link()
        .expect("complete configrow production");
    assert_eq!(row.class_reference.entity_id(), 6);
    assert_eq!(
        row.class_reference.entity(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(row.class_reference.class_name(), Some("configrow"));
    assert_eq!(row.successor_payload_offset, 5);
    assert_eq!(row.successor.entity_id(), 7);
    assert_eq!(
        row.successor.entity(),
        Some(native.entity_records[2].id.as_str())
    );
    assert_eq!(row.successor.class_name(), Some("body"));
    assert_eq!(native.schema_configuration_row_chains.len(), 1);
    let chain = &native.schema_configuration_row_chains[0];
    assert_eq!(chain.object_graph, native.entity_records[1].object_graph);
    let graph_key = chain
        .object_graph
        .split_once('#')
        .expect("object graph identity")
        .1;
    assert_eq!(
        chain.id,
        format!("catia:outer:schema-configuration-row-chain#{graph_key}:6")
    );
    assert_eq!(chain.links().len(), 1);
    assert_eq!(chain.links()[0].row, row.class_reference);
    assert_eq!(
        chain.links()[0].successor_payload_offset,
        row.successor_payload_offset
    );
    assert_eq!(
        chain
            .links()
            .iter()
            .map(|link| link.row.entity_id())
            .collect::<Vec<_>>(),
        [6]
    );
    assert_eq!(
        chain.links()[0].row.entity(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(chain.successor(0), Some(&row.successor));
    assert!(native.entity_records[2]
        .schema_configuration_record()
        .is_none());
    assert!(native.entity_records[2]
        .schema_configuration_row_link()
        .is_none());

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode configuration incidences");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_CONFIGURATION_RECORD_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_CONFIGURATION_SELECTOR_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNCLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_COMPLETE_SCHEMA_CONFIGURATION_ROW_CHAIN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ORDERED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_ORDER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONFIGURATION_COUNT),
        0
    );
    assert!(decoded.ir().model.configurations.is_empty());
}

#[test]
fn schema_configuration_row_chain_retains_complete_source_order() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_schema_configuration_row_chain());
    assert_eq!(native.schema_configuration_row_chains.len(), 1);
    let chain = &native.schema_configuration_row_chains[0];
    assert_eq!(chain.links()[0].row.entity_id(), 5);
    assert_eq!(
        chain
            .links()
            .iter()
            .map(|link| link.row.entity_id())
            .collect::<Vec<_>>(),
        [5, 7, 9]
    );
    assert!(chain
        .links()
        .iter()
        .all(|link| link.row.class_name() == Some("configrow")));
    assert_eq!(
        chain
            .links()
            .iter()
            .map(|link| link.successor_payload_offset)
            .collect::<Vec<_>>(),
        [5, 5, 5]
    );
    assert_eq!(
        chain
            .links()
            .iter()
            .map(|link| {
                link.intervening_entities
                    .as_ref()
                    .expect("source-ordered row interval")
                    .iter()
                    .map(super::super::CatiaEntityReference::entity_id)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![6], vec![8], vec![10]]
    );
    assert!(chain
        .links()
        .iter()
        .flat_map(|link| {
            link.intervening_entities
                .as_ref()
                .expect("source-ordered row interval")
        })
        .all(|reference| reference.class_name() == Some("body")));
    assert_eq!(chain.successor(2).unwrap().entity_id(), 11);
    assert_eq!(chain.successor(2).unwrap().class_name(), Some("body"));

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_schema_configuration_row_chain()),
            &DecodeOptions::default(),
        )
        .expect("decode configuration row intervals");
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_ENTITY_COUNT
        ),
        3
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_SCHEMA_CONFIGURATION_COUNT
        ),
        0
    );
}

#[test]
fn schema_configuration_productions_preserve_unresolved_identities() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(8, 15, 16),
    );
    let configuration = native.entity_records[0]
        .schema_configuration_record()
        .expect("complete schema-configuration production");
    assert_eq!(configuration.schema_name, "Boolean");
    assert!(configuration.entity_reference.reference.entity().is_none());
    let row = native.entity_records[1]
        .schema_configuration_row_link()
        .expect("complete configrow production");
    assert!(row.successor.entity().is_none());

    let mismatched_schema = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(14, 15, 16),
    );
    assert!(mismatched_schema.entity_records[0]
        .schema_configuration_record()
        .is_none());

    let mut malformed = standard_catpart_with_configuration_incidences(8, 15, 16);
    let marker = [0x80, 250, 0, 0, 0];
    let offset = malformed
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("configrow marker");
    malformed[offset + 1] = 249;
    let malformed = crate::native::CatiaNative::decode(&malformed);
    assert!(malformed
        .entity_records
        .iter()
        .all(|entity| entity.schema_configuration_row_link().is_none()));

    let cyclic_file = standard_catpart_with_configuration_incidences(8, 15, 6);
    let cyclic_native = crate::native::CatiaNative::decode(&cyclic_file);
    assert!(cyclic_native.schema_configuration_row_chains.is_empty());
    let cyclic = CatiaCodec
        .decode(&mut Cursor::new(cyclic_file), &DecodeOptions::default())
        .expect("decode cyclic configuration row");
    assert_eq!(
        cyclic
            .report()
            .coverage_count(crate::coverage::DECODED_COMPLETE_SCHEMA_CONFIGURATION_ROW_CHAIN_COUNT),
        0
    );
    assert_eq!(
        cyclic
            .report()
            .coverage_count(crate::coverage::DECODED_ORDERED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT),
        0
    );
    assert_eq!(
        cyclic
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_ORDER_COUNT),
        1
    );

    let descending = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(8, 15, 5),
    );
    assert_eq!(descending.schema_configuration_row_chains.len(), 1);
    assert!(descending.schema_configuration_row_chains[0].links()[0]
        .intervening_entities
        .is_none());
}

#[test]
fn schema_configuration_productions_distinguish_terminal_null_identities() {
    let file = standard_catpart_with_configuration_incidences(8, 8, 8);
    let native = crate::native::CatiaNative::decode(&file);
    let configuration = native.entity_records[0]
        .schema_configuration_record()
        .expect("complete schema-configuration production");
    assert!(configuration.entity_reference.reference.is_null());
    assert!(configuration.entity_reference.reference.entity().is_none());
    let row = native.entity_records[1]
        .schema_configuration_row_link()
        .expect("complete configrow production");
    assert!(!row.class_reference.is_null());
    assert!(row.successor.is_null());
    assert!(row.successor.entity().is_none());
    assert_eq!(native.schema_configuration_row_chains.len(), 1);
    assert!(native.schema_configuration_row_chains[0].terminal.is_null());

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode terminal-null configuration incidences");
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT
        ),
        0
    );
}

#[test]
fn native_load_validates_configuration_incidences() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(8, 5, 7),
    );
    let mut malformed_chain = native.clone();
    malformed_chain.schema_configuration_row_chains[0].terminal = malformed_chain
        .schema_configuration_row_chains[0]
        .terminal
        .clone()
        .with_entity_id(6);
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed_chain
        .store(&mut current)
        .expect("store malformed configuration chain");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_chain_offset = native.clone();
    malformed_chain_offset.schema_configuration_row_chains[0].links_mut()[0]
        .successor_payload_offset += 1;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed_chain_offset
        .store(&mut current)
        .expect("store malformed configuration-chain offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_offsets = native.clone();
    let configuration = malformed_offsets.entity_records[0]
        .schema_configuration_record_mut()
        .expect("decoded schema-configuration record");
    configuration.schema_payload_offset += 1;
    configuration.entity_reference.payload_offset += 1;
    malformed_offsets.entity_records[1]
        .schema_configuration_row_link_mut()
        .expect("decoded configrow link")
        .successor_payload_offset += 1;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed_offsets
        .store(&mut current)
        .expect("store malformed configuration offsets");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let interval_native =
        crate::native::CatiaNative::decode(&standard_catpart_with_schema_configuration_row_chain());
    let mut malformed_intervals = interval_native;
    malformed_intervals.schema_configuration_row_chains[0].links_mut()[0]
        .intervening_entities
        .as_mut()
        .expect("source-ordered row interval")[0] = {
        let current = malformed_intervals.schema_configuration_row_chains[0].links_mut()[0]
            .intervening_entities
            .as_ref()
            .expect("source-ordered row interval")[0]
            .clone();
        current.with_entity_id(8)
    };
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed_intervals
        .store(&mut current)
        .expect("store malformed schema-configuration-row intervals");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    let malformed_successor = malformed.entity_records[1]
        .schema_configuration_row_link()
        .expect("decoded configrow link")
        .successor
        .clone()
        .with_entity_id(6);
    malformed.entity_records[1]
        .schema_configuration_row_link_mut()
        .expect("decoded configrow link")
        .successor = malformed_successor;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut current)
        .expect("store malformed current namespace");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_noncanonical_graph_catalog_views() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    assert!(native.object_graphs[0].catalog_byte_offset.is_some());
    assert!(native.object_graphs[0].catalog.is_some());
    assert!(native.object_graphs[0].records[0].class_name().is_some());
    assert!(native.object_graphs[0].records[0].class_entry().is_some());
    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed graph-catalog view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut missing_catalog_link = native.clone();
    missing_catalog_link.object_graphs[0].catalog_byte_offset = None;
    assert_rejected(missing_catalog_link);

    let mut missing_catalog_identity = native.clone();
    missing_catalog_identity.object_graphs[0].catalog = None;
    assert_rejected(missing_catalog_identity);

    let mut invalid_class = native.clone();
    invalid_class.object_graphs[0].records[0]
        .class
        .as_mut()
        .expect("decoded class role")
        .class_name = Some("WrongClass".to_string());
    assert_rejected(invalid_class);

    let mut invalid_class_entry = native;
    invalid_class_entry.object_graphs[0].records[0]
        .class
        .as_mut()
        .expect("decoded class role")
        .class_entry = None;
    assert_rejected(invalid_class_entry);
}

#[test]
fn native_load_rejects_invalid_source_identities_and_extents() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed source identity");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut invalid_catalog_extent = native.clone();
    invalid_catalog_extent.catalogs[0].byte_len += 1;
    assert_rejected(invalid_catalog_extent);

    let mut invalid_entry_offset = native.clone();
    invalid_entry_offset.catalogs[0].entries[0].byte_offset += 1;
    assert_rejected(invalid_entry_offset);

    let mut invalid_record_offset = native.clone();
    invalid_record_offset.object_graphs[0].records[0].byte_offset += 1;
    assert_rejected(invalid_record_offset);

    let mut invalid_value_id = native;
    invalid_value_id.value_blocks[0].id = "catia:outer:value-block#wrong".to_string();
    assert_rejected(invalid_value_id);

    let mut invalid_alias_id = crate::native::CatiaNative::decode(&surface_alias_stream());
    invalid_alias_id.alias_rows[0].id = "catia:outer:alias-row#wrong".to_string();
    assert_rejected(invalid_alias_id);
}

#[test]
fn native_store_paths_cover_every_declared_arena() {
    let catalogue_names = crate::native::CATIA_FAMILIES
        .iter()
        .map(|row| row.arena)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(crate::native::CATIA_FAMILIES.len(), 43);
    assert_eq!(
        catalogue_names,
        crate::native::CATIA_ARENA_NAMES
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
    );

    let rich = crate::native::CatiaNative::decode(&standard_catpart());
    let mut rich_borrowed = cadmpeg_ir::NativeNamespace::default();
    rich.store(&mut rich_borrowed)
        .expect("store populated borrowed CATIA namespace");
    let mut rich_owned = cadmpeg_ir::NativeNamespace::default();
    rich.clone()
        .store_owned(&mut rich_owned)
        .expect("store populated owned CATIA namespace");
    assert_eq!(rich_borrowed, rich_owned);
    assert_eq!(
        crate::native::CatiaNative::load(&rich_borrowed).expect("reload populated namespace"),
        rich
    );
}

#[test]
fn native_validates_evaluated_value_names() {
    let mut bytes = Vec::new();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0x58, 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01\x06Count\xfe");
    bytes.extend([0x5f, 0xd1, 9]);
    bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
    bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let native = crate::native::CatiaNative::decode(&bytes);
    let value = &native.legacy_entity_runs[0].integer_values[0];
    assert_eq!(value.name.as_deref(), Some("Count"));

    let mut invalid = native.clone();
    invalid.legacy_entity_runs[0].integer_values[0].name = None;
    invalid.legacy_entity_runs[0].integer_values[0].name_field = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store noncanonical evaluated value name");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_load_restores_segment_source_order_and_validates_retained_views() {
    let mut bytes = Vec::new();
    for index in 0..12 {
        bytes.extend(external_reference_segment(&format!(
            "Support{index}.CATPart"
        )));
    }
    let native = crate::native::CatiaNative::decode(&bytes);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store indexed FINJPL segments");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load indexed FINJPL segments");
    assert_eq!(
        loaded
            .finjpl_segments
            .iter()
            .map(|segment| segment.id.clone())
            .collect::<Vec<_>>(),
        (0..12)
            .map(|index| format!("catia:outer:finjpl#{index}"))
            .collect::<Vec<_>>()
    );
    assert!(loaded
        .finjpl_segments
        .windows(2)
        .all(|pair| pair[0].byte_offset < pair[1].byte_offset));
    assert_eq!(
        loaded
            .external_references
            .iter()
            .map(|reference| reference.id.clone())
            .collect::<Vec<_>>(),
        (0..12)
            .map(|index| format!("catia:outer:external-reference#{index}"))
            .collect::<Vec<_>>()
    );

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed FINJPL view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };
    let mut invalid_length = native.clone();
    invalid_length.finjpl_segments[0].byte_len += 1;
    assert_rejected(invalid_length);
    let mut invalid_family = native.clone();
    invalid_family.finjpl_segments[0].family = "other".to_string();
    assert_rejected(invalid_family);
    let mut missing_reference = native.clone();
    missing_reference.external_references.pop();
    assert_rejected(missing_reference);
    let mut invalid_target = native.clone();
    invalid_target.external_references[0].target = "Wrong.CATPart".to_string();
    assert_rejected(invalid_target);
    let mut invalid_reference_offset = native.clone();
    invalid_reference_offset.external_references[0].byte_offset += 1;
    assert_rejected(invalid_reference_offset);
    let mut invalid_type = native;
    invalid_type.finjpl_segments[0].type_word ^= 1;
    assert_rejected(invalid_type);

    let mut invalid_offset = crate::native::CatiaNative::decode(&bytes);
    invalid_offset.finjpl_segments[1].byte_offset += 1;
    assert_rejected(invalid_offset);
}

#[test]
fn native_load_derives_complete_source_ordered_preview_views() {
    let mut bytes = Vec::new();
    for _ in 0..12 {
        bytes.extend(summary_preview_segment());
    }
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.preview_images.len(), 12);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store indexed preview views");
    let loaded = crate::native::CatiaNative::load(&namespace).expect("load indexed preview views");
    assert_eq!(
        loaded
            .preview_images
            .iter()
            .map(|preview| preview.id.clone())
            .collect::<Vec<_>>(),
        (0..12)
            .map(|index| format!("catia:outer:preview#{index}"))
            .collect::<Vec<_>>()
    );

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed preview view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };
    let mut missing = native.clone();
    missing.preview_images.pop();
    assert_rejected(missing);
    let mut invalid_width = native.clone();
    invalid_width.preview_images[0].width += 1;
    assert_rejected(invalid_width);
    let mut invalid_data = native;
    invalid_data.preview_images[0].data[0] = 0;
    assert_rejected(invalid_data);
}
