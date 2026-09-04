// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn decode_persists_external_references_in_native_namespace() {
    let mut file = standard_catpart();
    file.extend_from_slice(&external_reference_segment("Support.CATPart"));
    let file_len = u32::try_from(file.len()).expect("external-reference fixture length");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode external-reference fixture");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load CATIA native namespace");
    let [reference] = native.external_references.as_slice() else {
        panic!("one external reference");
    };
    assert_eq!(reference.target, "Support.CATPart");
    assert!(native
        .finjpl_segments
        .iter()
        .any(|segment| segment.id == reference.segment));
}

#[test]
fn native_namespace_retains_summary_preview_bytes() {
    let bytes = summary_preview_segment();
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.preview_images.len(), 1);
    let preview = &native.preview_images[0];
    assert_eq!(
        (preview.width, preview.height, preview.components),
        (640, 288, 1)
    );
    assert_eq!(preview.data.len() as u64, preview.byte_len);
    assert_eq!(&preview.data[..2], [0xff, 0xd8]);
    assert_eq!(&preview.data[preview.data.len() - 2..], [0xff, 0xd9]);
    assert_eq!(native.finjpl_segments.len(), 1);
    assert_eq!(
        native.finjpl_segments[0].name.as_deref(),
        Some("CATSummaryInformation")
    );
    assert_eq!(native.finjpl_segments[0].family, "project-flags");
    assert_eq!(native.finjpl_segments[0].data, bytes);
}

#[test]
fn native_value_blocks_distinguish_the_terminal_schema_sentinel() {
    let mut bytes = value_block_stream(&[0x32, 4, 0, 0, 0, 0x83, 0x32, 5, 0, 0, 0, 0x82]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let block = &native.value_blocks[0];
    assert_eq!(block.schema_selections.len(), 1);
    assert_eq!(block.schema_selections[0].ordinal, 4);
    assert_eq!(block.schema_selections[0].entry(), None);
    assert_eq!(block.schema_selections[0].name(), None);
    assert!(block.schema_selections[0].encoded_value().is_empty());
    assert!(block.fields.iter().any(|field| matches!(
        field,
        crate::value_block::ValueField::SchemaSelector { ordinal: 5, .. }
    )));
}

#[test]
fn native_value_blocks_frame_values_between_catalog_valid_selectors() {
    let mut bytes = value_block_stream(&[
        0x32, 3, 0, 0, 0, 0x83, 0x32, 5, 0, 0, 0, 0x84, 0x32, 2, 0, 0, 0, 0x32, 1, 0, 0, 0, 0x82,
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let selections = &native.value_blocks[0].schema_selections;
    assert_eq!(selections.len(), 3);
    assert_eq!(selections[0].parent, native.value_blocks[0].id);
    assert_eq!(
        selections[0].id,
        format!(
            "catia:outer:value-selection#{:010}",
            native.value_blocks[0].byte_offset + 6 + selections[0].offset
        )
    );
    assert_eq!(selections[0].ordinal, 3);
    assert!(matches!(
        selections[0].encoded_value(),
        [
            crate::value_block::ValueField::Atom { value: 3, .. },
            crate::value_block::ValueField::SchemaSelector { ordinal: 5, .. },
            crate::value_block::ValueField::Atom { value: 4, .. },
        ]
    ));
    assert_eq!(selections[1].ordinal, 2);
    assert!(selections[1].encoded_value().is_empty());
    assert_eq!(selections[2].ordinal, 1);
    assert!(matches!(
        selections[2].encoded_value(),
        [crate::value_block::ValueField::Atom { value: 2, .. }]
    ));
}

#[test]
fn native_design_inventory_excludes_records_inside_object_payloads() {
    let mut nested = value_block_stream(&[0x81]);
    nested.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));
    nested.extend(surface_alias_stream());
    let mut payload = vec![0xe5];
    payload.extend_from_slice(
        &u32::try_from(nested.len())
            .expect("fixture nested design length")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&nested);
    payload.push(0xfe);
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &payload)]);

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.object_graphs.len(), 1);
    assert!(native.alias_rows.is_empty());
    assert!(native.catalogs.is_empty());
    assert!(native.value_blocks.is_empty());
}

#[test]
fn native_design_inventory_excludes_records_inside_value_payloads() {
    let mut nested = value_block_stream(&[0x81]);
    nested.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));
    nested.extend(surface_alias_stream());
    let mut bytes = value_block_stream(&nested);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    assert_eq!(crate::value_block::parse(&bytes).len(), 1);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert!(native.alias_rows.is_empty());
    assert_eq!(native.value_blocks.len(), 1);
    assert_eq!(native.catalogs.len(), 1);
    assert_eq!(native.value_blocks[0].catalog, native.catalogs[0].id);
}

#[test]
fn native_design_inventory_excludes_object_graphs_inside_value_payloads() {
    let nested =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let mut bytes = value_block_stream(&nested);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    assert_eq!(crate::object_graph::parse_all(&bytes).len(), 1);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert!(native.object_graphs.is_empty());
    assert!(native.design_objects.is_empty());
    assert_eq!(native.value_blocks.len(), 1);
    assert_eq!(native.catalogs.len(), 1);
    assert_eq!(native.value_blocks[0].catalog, native.catalogs[0].id);
}

#[test]
fn native_design_inventory_excludes_alias_rows_inside_catalog_entries() {
    let mut alias = 1u32.to_le_bytes().to_vec();
    alias.extend_from_slice(&[0x01, 0x00, 0x04, 0x00]);
    alias.extend_from_slice(&0x0012_3456u32.to_le_bytes());
    alias.extend_from_slice(&[1, 2, 3, 4]);
    alias.extend_from_slice(&0x1122_3344u32.to_le_bytes());
    alias.extend_from_slice(&0x5566_7744u32.to_le_bytes());
    let entry = String::from_utf8(alias).expect("alias-shaped UTF-8 entry bytes");
    let bytes = catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        &entry,
    ]);

    assert_eq!(crate::object_graph::surface_aliases(&bytes).len(), 1);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.catalogs.len(), 1);
    assert!(native.alias_rows.is_empty());
}

#[test]
fn decode_retains_outer_object_graph_order_and_references() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_object_graph()),
            &DecodeOptions::default(),
        )
        .expect("decode generated object graph part");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.object_graphs.len(), 1);
    let graph = &native.object_graphs[0];
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].ordinal, 0);
    assert_eq!(graph.records[0].owner_entity_id(), Some(2));
    assert_eq!(graph.records[0].class_ref(), Some(3));
    assert_eq!(graph.records[0].storage_ref(), Some(4));
    assert_eq!(graph.records[1].ordinal, 1);
    assert_eq!(graph.records[1].owner_entity_id(), Some(2));
    assert_eq!(graph.records[1].class_ref(), Some(4));
    assert_eq!(native.design_objects.len(), 1);
    let object = &native.design_objects[0];
    assert_eq!(object.parent, graph.id);
    assert_eq!(object.owner_entity_id, 2);
    assert_eq!(
        object.owner_record.as_deref(),
        Some(graph.records[1].id.as_str())
    );
    assert_eq!(
        object.fields,
        graph
            .records
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OBJECT_GRAPH_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OBJECT_RECORD_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_OBJECT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_FIELD_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_OBJECT_RELATION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::CLASSIFIED_DESIGN_OBJECT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_OWNER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FEATURE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_SKETCH_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_SKETCH_CONSTRAINT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONFIGURATION_COUNT),
        0
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("1 design object(s)")
            && loss.message.contains("2 object-graph field record(s)")
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation
        .findings
        .iter()
        .all(|finding| finding.check != cadmpeg_ir::report::Check::Identity));
}

#[test]
fn object_graphs_retain_exact_finjpl_containment() {
    let preamble_graph =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let segment_graph =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x82, 0x82], &[0xfe])]);
    let mut bytes = preamble_graph;
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0101_0001u32.to_be_bytes());
    bytes.extend_from_slice(&segment_graph);

    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.object_graphs.len(), 2);
    assert_eq!(native.object_graphs[0].finjpl_segment, None);
    assert_eq!(
        native.object_graphs[1].finjpl_segment.as_deref(),
        Some(native.finjpl_segments[0].id.as_str())
    );

    let mut invalid = native;
    invalid.object_graphs[1].finjpl_segment = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store malformed graph segment link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn object_graphs_retain_exact_outer_container_declarations() {
    let (bytes, graph_offset) = outer_container_object_graph_catpart();

    let native = crate::native::CatiaNative::decode(&bytes);
    let graph = native
        .object_graphs
        .iter()
        .find(|graph| graph.byte_offset == graph_offset)
        .expect("declared-stream object graph");
    let container = graph
        .outer_container
        .as_ref()
        .expect("outer container binding");
    assert_eq!(container.data_offset, 0);
    assert_eq!(container.ordinal, 2);
    assert_eq!(container.class_name, "CATPrtCont");
    assert_eq!(container.base_class, "CATProdCont");
    assert_eq!(container.stream_name, "1048_62eb7b6f_1825");
    let expected = container.clone();

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store outer container binding");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load outer container binding");
    assert_eq!(
        loaded
            .object_graphs
            .iter()
            .find(|graph| graph.byte_offset == graph_offset)
            .and_then(|graph| graph.outer_container.as_ref()),
        Some(&expected)
    );
}

#[test]
fn decode_retains_catalog_schema_names_without_promoting_features() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_catalog()),
            &DecodeOptions::default(),
        )
        .expect("decode generated catalog part");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.catalogs.len(), 1);
    assert_eq!(native.catalogs[0].entries[4].value, "Sketch");
    assert_eq!(native.catalogs[0].entries[5].value, "Pad");
    assert_eq!(native.catalogs[0].entries[6].value, "GSMLoft");
    assert_eq!(native.catalogs[0].entries[7].value, "GSMPointBetweenValues");
    assert_eq!(native.catalogs[0].entries[8].value, "GSMPlaneAngle");
    assert!(decoded.ir().model.features.is_empty());
}

#[test]
fn decode_retains_value_blocks_at_their_schema_boundary() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_value_block()),
            &DecodeOptions::default(),
        )
        .expect("decode generated value block part");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.value_blocks.len(), 1);
    assert_eq!(
        native.value_blocks[0].byte_offset,
        u64::try_from(16 + object_graph_stream().len()).unwrap()
    );
    assert_eq!(native.value_blocks[0].byte_len, 16);
    assert_eq!(native.value_blocks[0].catalog, native.catalogs[0].id);
    assert_eq!(
        native.value_blocks[0].object_graph.as_deref(),
        Some(native.object_graphs[0].id.as_str())
    );
    assert_eq!(
        native.value_blocks[0].payload,
        [0x81, 0x83, 0x32, 4, 0, 0, 0, 0x83, 0x82]
    );
    assert_eq!(native.value_blocks[0].schema_selections.len(), 1);
    assert_eq!(native.value_blocks[0].schema_selections[0].ordinal, 4);
    assert_eq!(
        native.value_blocks[0].schema_selections[0].entry(),
        Some(native.catalogs[0].entries[4].id.as_str())
    );
    assert_eq!(
        native.value_blocks[0].schema_selections[0].name(),
        Some("VPGlobal")
    );
    assert_eq!(
        native.value_blocks[0].schema_selections[0].encoded_value(),
        [
            crate::value_block::ValueField::Atom {
                value: 3,
                width: 1,
                offset: 7,
            },
            crate::value_block::ValueField::Atom {
                value: 2,
                width: 1,
                offset: 8,
            },
        ]
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Attribute
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("1 visualization value block(s)")
            && loss
                .message
                .contains("1 schema-selected presentation value(s)")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("neutral features")
            && !loss.message.contains("value block")
    }));
}

#[test]
fn native_namespace_retains_and_validates_alias_group_membership() {
    let mut bytes = vec![0x02, 0x00];
    bytes.extend_from_slice(&0xafu32.to_le_bytes());
    bytes.extend_from_slice(&0x148u32.to_le_bytes());
    bytes.extend_from_slice(&[0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00]);
    let mut alias = surface_alias_stream();
    alias[15..19].copy_from_slice(&0x0000_017bu32.to_le_bytes());
    bytes.extend(alias);

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.alias_rows[0]
            .group
            .as_ref()
            .expect("group membership")
            .target_slot,
        0x17b
    );
    assert_eq!(
        native.alias_rows[0].canonical_surface_tag,
        Some(0x0012_3456)
    );
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store grouped alias row");
    let loaded = crate::native::CatiaNative::load(&namespace).expect("load grouped alias row");
    assert_eq!(loaded, native);

    let mut invalid = native;
    invalid.alias_rows[0]
        .group
        .as_mut()
        .expect("group membership")
        .target_slot += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store invalid grouped alias row");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid = loaded;
    invalid.alias_rows[0]
        .group
        .as_mut()
        .expect("group membership")
        .storage_prefix = vec![2, 0, 0];
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store invalid group storage");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut legacy = crate::native::CatiaNative::decode(&bytes);
    for row in &mut legacy.alias_rows {
        row.canonical_surface_tag = None;
    }
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    legacy
        .store(&mut namespace)
        .expect("store legacy alias rows");
    namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_ALIAS_SURFACE_TAG_VERSION - 1).unwrap(),
    );
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load legacy alias rows")
            .alias_rows,
        legacy.alias_rows
    );
}

#[test]
fn grouped_non_surface_alias_selects_the_unique_surface_storage_tag() {
    let mut bytes = grouped_surface_alias_stream(0, 0x1234, 0x148);
    bytes.extend(grouped_surface_alias_stream(1, 0x5678, 0x148));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.alias_rows.len(), 2);
    assert_eq!(native.alias_rows[0].canonical_surface_tag, Some(0x5678));
    assert_eq!(native.alias_rows[1].canonical_surface_tag, Some(0x5678));

    let mut invalid = native;
    invalid.alias_rows[0].canonical_surface_tag = Some(0x1234);
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store invalid alias closure");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn grouped_non_surface_alias_rejects_ambiguous_surface_storage() {
    let mut bytes = grouped_surface_alias_stream(0, 0x1234, 0x148);
    bytes.extend(grouped_surface_alias_stream(1, 0x5678, 0x148));
    bytes.extend(grouped_surface_alias_stream(1, 0x9abc, 0x148));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.alias_rows.len(), 3);
    assert_eq!(native.alias_rows[0].canonical_surface_tag, None);
    assert_eq!(native.alias_rows[1].canonical_surface_tag, Some(0x5678));
    assert_eq!(native.alias_rows[2].canonical_surface_tag, Some(0x9abc));
}

#[test]
fn pre_route_surface_alias_map_closes_only_unique_group_targets() {
    let mut bytes = grouped_surface_alias_stream(0, 0x1234, 0x148);
    bytes.extend(grouped_surface_alias_stream(1, 0x5678, 0x148));

    let tags = crate::object_graph::surface_alias_tag_map(&bytes);
    assert_eq!(tags.get(&0x1234), Some(&Some(0x5678)));
    assert_eq!(tags.get(&0x5678), Some(&Some(0x5678)));

    bytes.extend(grouped_surface_alias_stream(1, 0x9abc, 0x148));
    let tags = crate::object_graph::surface_alias_tag_map(&bytes);
    assert_eq!(tags.get(&0x1234), Some(&None));
    assert_eq!(tags.get(&0x5678), Some(&Some(0x5678)));
    assert_eq!(tags.get(&0x9abc), Some(&Some(0x9abc)));
}

#[test]
fn native_namespace_retains_surface_alias_core() {
    let native = crate::native::CatiaNative::decode(&surface_alias_stream());
    let [row] = native.alias_rows.as_slice() else {
        panic!("one alias row")
    };
    assert_eq!(row.byte_offset, 4);
    assert_eq!(row.tag, 0x0012_3456);
    assert_eq!(row.tag_raw, 0xab12_3456);
    assert_eq!(row.entity_record_ordinal, 7);
    assert!(row.design_object.is_none());
    assert_eq!((row.f2, row.f3), (0x1122_3344, 0x5566_7788));
    assert!(row.group.is_none());

    let mut invalid = native;
    invalid.alias_rows[0].design_object = Some("catia:missing-design-object".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store unresolved alias with a design-object link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_alias_f1_without_part_container_remains_unbound() {
    let graph = object_graph_stream();
    let mut alias = surface_alias_stream();
    alias[13..16].copy_from_slice(&[3, 0, 2]);
    let mut bytes = graph;
    bytes.extend(alias);

    let native = crate::native::CatiaNative::decode(&bytes);
    let [row] = native.alias_rows.as_slice() else {
        panic!("one alias row")
    };
    assert!(row.object_graph.is_none());
    assert!(row.object_record.is_none());
    assert!(row.design_object.is_none());

    let mut invalid = native;
    invalid.alias_rows[0].design_object = Some("catia:missing-design-object".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store invalid alias design-object link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_alias_f1_resolves_record_in_declared_part_container() {
    let mut stream = object_graph_stream();
    let mut alias = surface_alias_stream();
    alias[13..16].copy_from_slice(&[3, 0, 2]);
    stream.extend(alias);
    let (bytes, _) = outer_container_catpart(&stream);

    let native = crate::native::CatiaNative::decode(&bytes);
    let [row] = native.alias_rows.as_slice() else {
        panic!("one alias row")
    };
    let graph = native
        .object_graphs
        .iter()
        .find(|graph| {
            graph
                .outer_container
                .as_ref()
                .is_some_and(|container| container.class_name == "CATPrtCont")
        })
        .expect("declared part-container graph");
    let record = &graph.records[1];
    assert_eq!(row.object_graph.as_deref(), Some(graph.id.as_str()));
    assert_eq!(row.object_record.as_deref(), Some(record.id.as_str()));
    assert_eq!(row.design_object, record.design_object);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store alias linked to declared part container");
    crate::native::CatiaNative::load(&namespace)
        .expect("load alias linked to declared part container");
}
