// SPDX-License-Identifier: Apache-2.0

use crate::om::column_row::{IndexRow, LinkedRow, TargetRow};
use crate::om::compact::CompactIndexTarget;

use std::io::Cursor;
use std::sync::Arc;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container::{DirEntry, Region};
use crate::om::{EntityRecord, IndexedSection, IndexedStore};
use crate::test_support::*;
use crate::NxCodec;

use super::*;
use crate::native::features::operation_record::FeatureOperationRecord;

#[test]
fn unique_offset_data_store_rejects_a_second_matching_section() {
    let entry = DirEntry {
        name: "section".into(),
        region: Region::Header,
        file_span: None,
    };
    let entries = [entry];
    let entry = crate::container::entry_ref::EntryRef::new(&entries, 0).unwrap();
    let section = || IndexedSection {
        base: 0,
        entity_index_offset: 0,
        object_id_table_offset: 0,
        types: Arc::from([]),
        fields: Arc::from([]),
        store: IndexedStore::OffsetOnly {
            control: EntityRecord {
                offset: 0,
                bytes: &[],
            },
            column_storage: &[],
            records: Arc::from([EntityRecord {
                offset: 0,
                bytes: &[],
            }]),
        },
    };
    let first = section();
    let second = section();
    let single = section();
    assert_eq!(
        super::unique_offset_data_store(&[(entry, single)], &[1]),
        Some(0)
    );
    let indexed = [(entry, first), (entry, second)];

    assert_eq!(super::unique_offset_data_store(&indexed, &[1]), None);
}

#[test]
fn nx_feature_source_content_orders_payload_text() {
    let text = super::FeaturePayloadString {
        id: "text".into(),
        operation_record: "record".into(),
        ordinal: 0,
        value: crate::payload_text::PayloadText::new("Through".to_owned()).unwrap(),
        source_offset: 30,
    };
    let later = super::FeaturePayloadString {
        id: "later".into(),
        operation_record: "record".into(),
        ordinal: 1,
        value: crate::payload_text::PayloadText::new("Later".to_owned()).unwrap(),
        source_offset: 40,
    };
    let content = crate::native::attach::feature_source_content(&[&later, &text]);
    assert!(matches!(
        &content[0],
        cadmpeg_ir::features::FeatureSourceContent::Text(value) if value == "Through"
    ));
    assert!(matches!(
        &content[1],
        cadmpeg_ir::features::FeatureSourceContent::Text(value) if value == "Later"
    ));
}

#[test]
fn nx_block_dimensions_do_not_cross_expression_sections() {
    use super::{FeatureBlockConstruction, FeatureParameterBinding};
    use crate::native::om::{Expression, ExpressionDeclaration, ExpressionUnit};

    let operation = "nx:feature-history:operation-label#0-1";
    let construction = FeatureBlockConstruction {
        id: "nx:feature-history:block-construction#0-1".into(),
        operation_label: operation.into(),
        control: 0,
        members: std::array::from_fn(|ordinal| super::FeatureConstructionMember {
            reference: format!("reference#{ordinal}"),
            data_block: format!("block#{ordinal}"),
        }),
        terminal_reference: "terminal-reference".into(),
        terminal_data_block: "terminal-block".into(),
    };
    let binding = FeatureParameterBinding {
        id: "binding".into(),
        operation_label: operation.into(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        input_block: "input".into(),
        reference_ordinal: 0,
        expression_declaration: "declaration-20".into(),
        expression: Some("expression-20".into()),
        object_id: 20,
        source_offset: 1,
    };
    let declaration = |index: u32, source_entry: &str| ExpressionDeclaration {
        id: format!("declaration-{index}"),
        object_id: index,
        record: format!("{source_entry}:entry#{index}"),
        name: crate::om::parameter_name::ParameterName::<_, u32>::parse(format!("p{index}"))
            .unwrap(),
        literal: None,
        source_entry: source_entry.into(),
        source_offset: u64::from(index),
    };
    let expression = |index: u32, source_entry: &str, source_table: &str| Expression {
        id: format!("expression-{index}"),
        owner: Some(crate::native::om::ExpressionOwner {
            object_id: index,
            record: format!("{source_entry}:entry#{index}"),
        }),
        declaration: Some(format!("declaration-{index}")),
        name: crate::om::parameter_name::ParameterName::new(format!("p{index}")),
        unit: ExpressionUnit::Millimeter,
        expression: index.to_string(),
        value: Some(f64::from(index)),
        source_entry: source_entry.into(),
        source_table: source_table.into(),
        source_offset: u64::from(index),
    };
    let mut expressions = [
        expression(20, "section-a", "table-a"),
        expression(21, "section-a", "table-a"),
        expression(22, "section-b", "table-b"),
    ];
    let mut declarations = [
        declaration(20, "section-a"),
        declaration(21, "section-a"),
        declaration(22, "section-b"),
    ];

    assert!(super::feature_block_dimensions(
        std::slice::from_ref(&construction),
        std::slice::from_ref(&binding),
        &declarations,
        &expressions,
    )
    .is_empty());

    declarations[2].source_entry = "section-a".into();
    declarations[2].record = "section-a:entry#22".into();
    assert!(super::feature_block_dimensions(
        std::slice::from_ref(&construction),
        std::slice::from_ref(&binding),
        &declarations,
        &expressions,
    )
    .is_empty());

    expressions[2].source_entry = "section-a".into();
    expressions[2].source_table = "table-a".into();
    assert_eq!(
        super::feature_block_dimensions(
            std::slice::from_ref(&construction),
            std::slice::from_ref(&binding),
            &declarations,
            &expressions,
        )
        .len(),
        1
    );

    for expression in &mut expressions {
        expression.unit = ExpressionUnit::Inch;
    }
    let dimensions = super::feature_block_dimensions(
        std::slice::from_ref(&construction),
        std::slice::from_ref(&binding),
        &declarations,
        &expressions,
    );
    assert_eq!(
        dimensions[0]
            .dimensions
            .each_ref()
            .map(|dimension| dimension.value),
        [508.0, 533.4, 558.8]
    );
}

#[test]
fn nx_boolean_projection_rejects_target_tool_alias_overlap() {
    use cadmpeg_ir::features::{BodySelection, BooleanKind, FeatureDefinition};
    use std::collections::BTreeMap;

    let operation = super::FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#0".to_string(),
        kind: super::FeatureBooleanKind::Subtract,
        target: crate::test_support::native_references::boolean_reference(10, 0),
        tools: vec![crate::test_support::native_references::boolean_reference(
            20, 1,
        )],
        source_offset: 0,
    };
    let roots = BTreeMap::from([(10, 10), (20, 10)]);

    assert_eq!(
        crate::native::attach::boolean_feature_definition(
            &operation,
            &roots,
            &crate::native::segments::BooleanOffsetStoreResolution::None,
            &BTreeMap::new(),
        ),
        FeatureDefinition::Combine {
            target: BodySelection::Native("nx:om-object-index#10".to_string()),
            tools: BodySelection::Native("nx:om-object-indices#20".to_string()),
            op: BooleanKind::Cut,
            keep_tools: false,
        }
    );

    let missing_tool = BTreeMap::from([(10, 10)]);
    assert!(matches!(
        crate::native::attach::boolean_feature_definition(
            &operation,
            &missing_tool,
            &crate::native::segments::BooleanOffsetStoreResolution::None,
            &BTreeMap::new(),
        ),
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            ..
        } if target == "nx:om-object-index#10" && tools == "nx:om-object-indices#20"
    ));
}

#[test]
fn nx_sketch_record_joins_exact_operation_and_ordered_input_lanes() {
    use super::{FeatureInputBlock, FeatureOperationLabel, FeatureSketchReference};

    let label = FeatureOperationLabel {
        id: "nx:feature-history:operation-label#0-7".to_string(),
        section_link: "nx:feature-history#0".to_string(),
        ordinal: 7,
        value: "SKETCH".to_string(),
        objects: crate::om::header_references::HeaderReferences::from_wire(
            [Some(45), None, Some(81), None],
            [&[45], &[0xff], &[81], &[0xff]],
        )
        .unwrap(),
        stable_identity: None,
        source_offset: 700,
    };
    let record = FeatureOperationRecord {
        id: "nx:feature-history:operation-record#0-7".to_string(),
        operation_label: label.id.clone(),
        ordinal: 7,
        sha256: "00".repeat(32),
        payload_sha256: "11".repeat(32),
        stable_identity: None,
        span: crate::native::features::operation_record::OperationRecordSpan::new(700, 733, 140)
            .unwrap(),
    };
    let input = |slot, index| FeatureInputBlock {
        id: format!("nx:feature-history:input-block#0-7-{slot}"),
        operation_label: label.id.clone(),
        input_slot: crate::om::header_references::HeaderSlot::try_from(slot).unwrap(),
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(index, &[index as u8])
            .unwrap(),
        data_block: format!("nx:om-data-blocks-2:block#{index}"),
        source_offset: 710 + u64::from(slot),
    };
    let inputs = [input(2, 81), input(0, 45)];
    let reference = |ordinal, index| FeatureSketchReference {
        id: format!("nx:feature-history:sketch-reference#0-7-{ordinal}"),
        operation_label: label.id.clone(),
        position: crate::om::sketch_references::SketchReferencePosition::new(2, ordinal).unwrap(),
        token: crate::om::reference_index::ReferenceIndexToken::from_wire(
            index,
            &[0xf0, index as u8],
        )
        .unwrap(),
        data_block: Some(format!("nx:om-data-blocks-2:block#{index}")),
        source_offset: 740 + u64::from(ordinal),
    };
    let references = [reference(1, 97), reference(0, 96)];

    let sketches = super::feature_sketch_records(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        &inputs,
        &references,
    );
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].ordinal, 7);
    assert_eq!(
        sketches[0].operation_record,
        "nx:feature-history:operation-record#0-7"
    );
    assert_eq!(
        sketches[0].input_blocks,
        [
            "nx:feature-history:input-block#0-7-0",
            "nx:feature-history:input-block#0-7-2"
        ]
    );
    assert_eq!(
        sketches[0].payload_references,
        [
            "nx:feature-history:sketch-reference#0-7-0",
            "nx:feature-history:sketch-reference#0-7-1"
        ]
    );
    let mut duplicate_record = record.clone();
    duplicate_record.id.push_str("-duplicate");
    assert!(super::feature_sketch_records(
        std::slice::from_ref(&label),
        &[record.clone(), duplicate_record],
        &inputs,
        &references,
    )
    .is_empty());
    let construction = super::feature_sketch_construction_inputs(&sketches, &references);
    assert_eq!(construction.len(), 1);
    assert_eq!(
        construction[0]
            .members
            .iter()
            .map(|member| member.reference.as_str())
            .collect::<Vec<_>>(),
        ["nx:feature-history:sketch-reference#0-7-0"]
    );
    assert_eq!(
        construction[0]
            .members
            .iter()
            .map(|member| member.data_block.as_str())
            .collect::<Vec<_>>(),
        ["nx:om-data-blocks-2:block#96"]
    );
    assert_eq!(
        construction[0].terminal_reference,
        "nx:feature-history:sketch-reference#0-7-1"
    );
    assert_eq!(
        construction[0].terminal_data_block,
        "nx:om-data-blocks-2:block#97"
    );

    let mut malformed = references;
    malformed[0].position =
        crate::om::sketch_references::SketchReferencePosition::new(3, 2).unwrap();
    assert!(super::feature_sketch_construction_inputs(&sketches, &malformed).is_empty());
}

#[test]
fn nx_offset_store_block_bytes_follow_catalog_identity() {
    let control = crate::om::EntityRecord {
        offset: 5,
        bytes: &[0xaa],
    };
    let first = crate::om::EntityRecord {
        offset: 6,
        bytes: &[0xbb],
    };
    let second = crate::om::EntityRecord {
        offset: 7,
        bytes: &[0xcc],
    };
    let controlled = super::offset_data_block_bytes_for_section(3, 100, &control, &[first, second]);
    assert_eq!(
        controlled["nx:om-data-blocks-3:block#0"],
        (&[0xaa][..], 105)
    );
    assert_eq!(
        controlled["nx:om-data-blocks-3:block#1"],
        (&[0xbb][..], 106)
    );
    assert_eq!(
        controlled["nx:om-data-blocks-3:block#2"],
        (&[0xcc][..], 107)
    );
}

#[test]
fn feature_history_links_follow_unique_physical_section_order() {
    use crate::native::om::OmSchemaRole;
    use crate::native::segments::{SegmentIndexSlot, SegmentOmLink};

    let link = |id: &str, schema_role, source_offset, section_offset| SegmentOmLink {
        id: id.to_string(),
        row: format!("row-{id}"),
        slot: SegmentIndexSlot::Value,
        schema_role,
        separator_byte_len: (section_offset - source_offset) as u32,
        source_offset,
        section_offset,
    };
    let links = super::canonical_feature_history_links([
        link("late", OmSchemaRole::FeatureHistory, 300, 300),
        link("model", OmSchemaRole::Model, 50, 50),
        link("duplicate", OmSchemaRole::FeatureHistory, 100, 100),
        link("early", OmSchemaRole::FeatureHistory, 100, 100),
    ]);

    assert_eq!(
        links
            .iter()
            .map(|link| (link.id.as_str(), link.section_offset))
            .collect::<Vec<_>>(),
        [("duplicate", 100), ("late", 300)]
    );
}

#[test]
fn decode_orders_and_deduplicates_linked_feature_history_sections() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        multi_section_feature_history_payload(),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    let namespace = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant");
    let links = namespace
        .arena_as::<crate::native::segments::SegmentOmLink>("segment_om_links")
        .expect("required invariant");
    assert_eq!(links.len(), 4);
    let labels = namespace
        .arena_as::<super::FeatureOperationLabel>("feature_operation_labels")
        .expect("required invariant");
    assert_eq!(
        labels
            .iter()
            .map(|label| (label.value.as_str(), label.ordinal))
            .collect::<Vec<_>>(),
        [("BLOCK", 0), ("UNITE", 0)]
    );
    assert_ne!(labels[0].section_link, labels[1].section_link);
    assert_eq!(
        labels[0]
            .objects
            .0
            .map(|token| token.map_or_else(|| vec![0xff], |token| token.raw().to_vec())),
        [
            vec![0x01],
            vec![0x82, 0x40],
            vec![0x90, 0x17, 0xd3],
            vec![0xff]
        ]
    );
    assert_eq!(
        labels[1]
            .objects
            .0
            .map(|token| token.map_or_else(|| vec![0xff], |token| token.raw().to_vec())),
        labels[0]
            .objects
            .0
            .map(|token| token.map_or_else(|| vec![0xff], |token| token.raw().to_vec()))
    );
    let records = namespace
        .arena_as::<FeatureOperationRecord>("feature_operation_records")
        .expect("required invariant");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].operation_label, labels[0].id);
    assert_eq!(records[1].operation_label, labels[1].id);
    assert_eq!(
        result
            .ir()
            .model
            .features
            .iter()
            .map(|feature| feature.name.as_deref())
            .collect::<Vec<_>>(),
        [Some("BLOCK"), Some("UNITE")]
    );
}

#[test]
fn decoded_feature_ids_preserve_source_order_and_ordinals_reverse_history() {
    let section = size_framed_om_section_with_repeated_operations(12);
    let mut payload = Vec::new();
    for word in [24_u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&section);
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    let labels = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::FeatureOperationLabel>("feature_operation_labels")
        .expect("required invariant");

    assert_eq!(
        labels.iter().map(|label| label.ordinal).collect::<Vec<_>>(),
        (0..12).collect::<Vec<_>>()
    );
    assert!(labels
        .windows(2)
        .all(|pair| pair[0].id.as_str() < pair[1].id.as_str()));
    let features = &result.ir().model.features;
    assert_eq!(
        features
            .iter()
            .map(|feature| feature.ordinal)
            .collect::<Vec<_>>(),
        (0..12).rev().collect::<Vec<_>>()
    );
    assert_eq!(features[0].dependencies, [features[1].id.clone()]);
    assert!(features[11].dependencies.is_empty());
}

#[test]
fn decode_retains_role_scoped_om_record_area_header() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_record_area_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    let areas = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<crate::native::om::OmRecordArea>("om_record_areas")
        .expect("required invariant");
    assert_eq!(areas.len(), 1);
    assert_eq!(
        areas[0].schema_role,
        crate::native::om::OmSchemaRole::FeatureHistory
    );
    assert_eq!(areas[0].control_words, [13, 14, 44]);
    assert_eq!(areas[0].product_version.as_str(), "NX 2027.3102");
    assert!(areas[0].byte_len > 12);
    assert_eq!(areas[0].sha256.len(), 64);
    let labels = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::FeatureOperationLabel>("feature_operation_labels")
        .expect("required invariant");
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].ordinal, 0);
    assert_eq!(labels[0].value, "UNITE");
    assert_eq!(
        labels[0].objects.values(),
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[0].section_link, areas[0].section_link);
    let records = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<FeatureOperationRecord>("feature_operation_records")
        .expect("required invariant");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation_label, labels[0].id);
    assert!(records[0].span.byte_len() > 40);
    assert_eq!(records[0].sha256.len(), 64);
    let booleans = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::FeatureBooleanOperation>("feature_boolean_operations")
        .expect("required invariant");
    assert_eq!(booleans.len(), 1);
    assert_eq!(booleans[0].kind, super::FeatureBooleanKind::Unite);
    assert_eq!(booleans[0].target.token.value(), 6466);
    assert_eq!(
        booleans[0]
            .tools
            .iter()
            .map(|token| token.token.value())
            .collect::<Vec<_>>(),
        [6476, 127]
    );
    let body_references = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::FeatureBodyReference>("feature_body_references")
        .expect("required invariant");
    assert_eq!(body_references.len(), 1);
    assert_eq!(body_references[0].operation_label, labels[0].id);
    assert_eq!(body_references[0].body.value(), 6466);
    let body_reference_occurrences = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::FeatureBodyReference>("feature_body_reference_occurrences")
        .expect("required invariant");
    assert_eq!(body_reference_occurrences.len(), 1);
    assert_eq!(body_reference_occurrences[0].operation_label, labels[0].id);
    assert_eq!(body_reference_occurrences[0].ordinal, Some(0));
    assert_eq!(body_reference_occurrences[0].body.value(), 6466);
    let feature = result.ir().model.features.first().expect("neutral feature");
    assert_eq!(feature.name.as_deref(), Some("UNITE"));
    assert_eq!(feature.suppressed, None);
    assert_eq!(feature.native_ref.as_deref(), Some(labels[0].id.as_str()));
    assert_eq!(
        feature.source_properties.get("body_reference.0"),
        Some(&"6466".to_string())
    );
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Combine {
            target: cadmpeg_ir::features::BodySelection::Native(target),
            tools: cadmpeg_ir::features::BodySelection::Native(tools),
            op: cadmpeg_ir::features::BooleanKind::Join,
            keep_tools: false,
        } if target == "nx:om-object-index#6466" && tools == "nx:om-object-indices#6476,127"
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_resolves_feature_header_input_to_unique_data_block() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_input_store_payload(),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    let inputs = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::FeatureInputBlock>("feature_input_blocks")
        .expect("required invariant");
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].input_slot.number(), 0);
    assert_eq!(inputs[0].object.value(), 1);
    assert!(inputs[0].data_block.ends_with(":block#1"));
    assert_eq!(
        result.ir().model.features[0].source_properties["input_block.0"],
        inputs[0].data_block
    );
    assert_eq!(
        result.ir().model.features[0].source_properties["input_block_record.0"],
        inputs[0].id
    );
    let references = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<crate::native::om::DataBlockReference>("data_block_references")
        .expect("required invariant");
    assert_eq!(references.len(), 1);
    assert!(references[0].data_block.ends_with(":block#2"));
    assert_ne!(references[0].data_block, inputs[0].data_block);
    assert_eq!(references[0].object.value(), 42);
    assert_eq!(references[0].target_record, None);
}

#[test]
fn sketch_point_blocks_establish_ordered_datum_csys_dependencies() {
    use super::{
        FeatureDatumCsysConstruction, FeatureOperationLabel, FeaturePayloadScalar,
        FeatureSketchDatumCsysBlockRelation, FeatureSketchPointUse, OffsetStoreNamedPoint,
    };

    let label = |id: &str, value: &str, ordinal| FeatureOperationLabel {
        id: id.to_string(),
        section_link: "section".to_string(),
        ordinal,
        value: value.to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 100 + u64::from(ordinal),
    };
    let labels = [label("csys", "DATUM_CSYS", 0), label("sketch", "SKETCH", 1)];
    let point = OffsetStoreNamedPoint {
        id: "point".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["point-first".to_string(), "shared".to_string()],
        values: [(1.0, 200), (2.0, 220)].map(|(value, source_offset)| {
            crate::native::features::FeatureBinary64ScalarToken {
                scalar: crate::om::scalar::ShiftedBinary64::try_from(shifted_f64_bytes(value))
                    .unwrap(),
                source_offset,
            }
        }),
        source_offset: 190,
    };
    let point_use = FeatureSketchPointUse {
        id: "point-use".to_string(),
        operation_label: "sketch".to_string(),
        references: vec![crate::native::features::FeatureSketchPointUseReference {
            sketch_reference: "reference".to_string(),
            block_use: "block-use".to_string(),
            source_offset: 300,
        }],
        sketch_point_group: "point-group".to_string(),
        named_point: point.id.clone(),
    };
    let mut blocks = std::array::from_fn(|index| format!("block-{index}"));
    blocks[3] = "shared".to_string();
    let construction = FeatureDatumCsysConstruction {
        id: "construction".to_string(),
        operation_label: "csys".to_string(),
        frame: crate::om::datum_csys::DatumCsysFrame::new(
            19,
            386,
            blocks.map(|data_block| {
                (
                    crate::om::reference_index::PayloadIndexToken::from_wire(0, &[0xf0, 0])
                        .unwrap(),
                    data_block,
                )
            }),
        )
        .unwrap(),
    };
    let scalar = FeaturePayloadScalar {
        id: "csys-scalar".to_string(),
        operation_label: "csys".to_string(),
        payload: crate::native::features::FeatureScalarPayload::DatumCsys {
            datum_csys_payload: "payload".to_string(),
        },
        ordinal: 0,
        field_code: 0x64,
        scalar: crate::om::scalar::ShiftedBinary64::try_from(shifted_f64_bytes(2.0)).unwrap(),
        payload_offset: 8,
        source_offset: 220,
    };

    let dependencies = super::feature_sketch_datum_csys_dependencies(
        &labels,
        std::slice::from_ref(&point),
        std::slice::from_ref(&point_use),
        std::slice::from_ref(&construction),
        std::slice::from_ref(&scalar),
    );
    assert_eq!(dependencies[0].datum_csys_operation_label, "csys");
    assert_eq!(dependencies[0].sketch_operation_label, "sketch");
    assert_eq!(dependencies[0].sketch_point_use, "point-use");
    assert_eq!(
        dependencies[0].block_relation,
        FeatureSketchDatumCsysBlockRelation::Shared {
            data_block: "shared".to_string()
        }
    );
    assert_eq!(dependencies[0].scalar_aliases.len(), 1);
    assert_eq!(
        dependencies[0].scalar_aliases[0].sketch_coordinate_ordinal,
        1
    );
    assert_eq!(
        dependencies[0].scalar_aliases[0].datum_csys_scalar,
        "csys-scalar"
    );
    assert_eq!(dependencies[0].scalar_aliases[0].value_source_offset, 220);

    let mut equal_at_another_offset = scalar.clone();
    equal_at_another_offset.source_offset = 219;
    let unaliased = super::feature_sketch_datum_csys_dependencies(
        &labels,
        std::slice::from_ref(&point),
        std::slice::from_ref(&point_use),
        std::slice::from_ref(&construction),
        std::slice::from_ref(&equal_at_another_offset),
    );
    assert!(unaliased[0].scalar_aliases.is_empty());

    let consecutive_point = OffsetStoreNamedPoint {
        id: "consecutive-point".to_string(),
        name: "Point2".to_string(),
        data_blocks: vec![
            "nx:om:offset-store#7:block#10".to_string(),
            "nx:om:offset-store#7:block#11".to_string(),
        ],
        values: [(3.0, 500), (4.0, 520)].map(|(value, source_offset)| {
            crate::native::features::FeatureBinary64ScalarToken {
                scalar: crate::om::scalar::ShiftedBinary64::try_from(shifted_f64_bytes(value))
                    .unwrap(),
                source_offset,
            }
        }),
        source_offset: 490,
    };
    let consecutive_use = FeatureSketchPointUse {
        id: "consecutive-use".to_string(),
        named_point: consecutive_point.id.clone(),
        ..point_use.clone()
    };
    let mut consecutive_construction = construction.clone();
    consecutive_construction.id = "consecutive-construction".to_string();
    let mut members = consecutive_construction.frame.members().clone();
    members[0].1 = "nx:om:offset-store#7:block#12".to_string();
    consecutive_construction.frame =
        crate::om::datum_csys::DatumCsysFrame::new(19, 386, members).unwrap();
    let consecutive_dependencies = super::feature_sketch_datum_csys_dependencies(
        &labels,
        &[consecutive_point],
        &[consecutive_use],
        &[consecutive_construction],
        &[],
    );
    assert_eq!(
        consecutive_dependencies[0].block_relation,
        FeatureSketchDatumCsysBlockRelation::Consecutive {
            point_data_block: "nx:om:offset-store#7:block#11".to_string(),
            construction_data_block: "nx:om:offset-store#7:block#12".to_string(),
        }
    );

    let mut ambiguous_point = point.clone();
    ambiguous_point.id = "ambiguous-point".to_string();
    let ambiguous_use = FeatureSketchPointUse {
        id: "ambiguous-use".to_string(),
        named_point: ambiguous_point.id.clone(),
        ..point_use.clone()
    };
    assert!(super::feature_sketch_datum_csys_dependencies(
        &labels,
        &[point.clone(), ambiguous_point],
        &[point_use.clone(), ambiguous_use],
        std::slice::from_ref(&construction),
        &[],
    )
    .is_empty());

    let reversed_labels = [label("sketch", "SKETCH", 0), label("csys", "DATUM_CSYS", 1)];
    assert!(super::feature_sketch_datum_csys_dependencies(
        &reversed_labels,
        &[point],
        &[point_use],
        &[construction],
        &[],
    )
    .is_empty());
}

#[test]
fn nx_sketch_point_names_require_positive_decimal_suffixes() {
    assert_eq!(super::parse_sketch_point_name("Point1"), Some(1));
    assert_eq!(super::parse_sketch_point_name("Point2048"), Some(2048));
    for malformed in ["Point", "Point0", "point1", "Point-1", "Point1A"] {
        assert_eq!(super::parse_sketch_point_name(malformed), None);
    }
}

#[test]
fn nx_datum_plane_csys_identity_uses_join_only_equal_typed_identities() {
    let plane = super::FeatureDatumPlaneDescriptor {
        id: "plane-descriptor".into(),
        operation_label: "operation#4".into(),
        datum_plane_header: "plane-header".into(),
        ordinal: 0,
        data_block: "plane-block".into(),
        descriptor: crate::om::plane_descriptor::PlaneDescriptor::read(
            b"012345678901234567890123456789?A\x01\xff\x02\x01abcd",
        )
        .unwrap(),
        source_offset: 10,
    };
    let csys = super::FeatureDatumCsysDescriptor {
        id: "csys-descriptor".into(),
        operation_label: "operation#2".into(),
        construction: "csys-construction".into(),
        reference_ordinal: crate::om::csys_descriptor::CsysDescriptorSlot::Seven,
        data_block: "csys-block".into(),
        descriptor: crate::om::csys_descriptor::LocatedCsysDescriptor::new(
            crate::om::csys_descriptor::CsysDescriptor::from_wire(
                vec![2, 1],
                plane.descriptor.identity().to_owned().try_into().unwrap(),
                vec![b'?', b'A'],
            )
            .unwrap(),
            20,
        )
        .unwrap(),
    };
    let uses = super::feature_datum_plane_csys_identity_uses(&[plane], &[csys]);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].identity.as_str(), "012345678901234567890123456789");
    assert_eq!(uses[0].datum_plane_operation_label, "operation#4");
    assert_eq!(uses[0].datum_csys_operation_label, "operation#2");
    assert_eq!(u8::from(uses[0].datum_csys_reference_ordinal), 7);
}

#[test]
fn nx_datum_csys_block_uses_preserve_reference_and_input_order() {
    let construction = super::FeatureDatumCsysConstruction {
        id: "construction".to_string(),
        operation_label: "operation#0".to_string(),
        frame: crate::om::datum_csys::DatumCsysFrame::new(
            0x13,
            86,
            std::array::from_fn(|index| {
                (
                    crate::om::reference_index::PayloadIndexToken::from_wire(
                        index as u32 + 40,
                        &[0xf0, index as u8 + 40],
                    )
                    .unwrap(),
                    format!("block#{}", index + 40),
                )
            }),
        )
        .unwrap(),
    };
    let input = |id: &str, operation: &str, slot: u8, block: &str| super::FeatureInputBlock {
        id: id.to_string(),
        operation_label: operation.to_string(),
        input_slot: crate::om::header_references::HeaderSlot::try_from(slot).unwrap(),
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(44, &[44]).unwrap(),
        data_block: block.to_string(),
        source_offset: 200,
    };
    let uses = super::feature_datum_csys_block_uses(
        &[construction],
        &[
            input("input#0", "operation#0", 1, "block#43"),
            input("input#1", "operation#6", 0, "block#44"),
            input("input#2", "operation#7", 0, "block#44"),
        ],
    );
    assert_eq!(uses.len(), 3);
    assert_eq!(
        uses[0].id,
        "nx:feature-history:datum-csys-block-use#0-3-0-1"
    );
    assert_eq!(u8::from(uses[0].reference_ordinal), 3);
    assert_eq!(uses[0].input_operation_label, "operation#0");
    assert_eq!(u8::from(uses[1].reference_ordinal), 4);
    assert_eq!(uses[1].input_operation_label, "operation#6");
    assert_eq!(u8::from(uses[2].reference_ordinal), 4);
    assert_eq!(uses[2].input_operation_label, "operation#7");
}

#[test]
fn nx_extrude_construction_profile_requires_matching_resolved_encodings() {
    use super::FeatureExtrudeProfileReference;

    let references = [10, 11].map(|ordinal| FeatureExtrudeProfileReference {
        id: format!("profile-{ordinal}"),
        operation_label: "operation".to_string(),
        ordinal: ordinal - 10,
        field_tag: 0x16,
        witness_source_offset: Some(u64::from(ordinal + 20)),
        token: crate::om::reference_index::PayloadIndexToken::from_wire(
            ordinal + 90,
            &[0xf0, (ordinal + 90) as u8],
        )
        .unwrap(),
        data_block: Some(format!("block-{ordinal}")),
        source_offset: u64::from(ordinal),
    });
    let profiles = super::feature_extrude_construction_profiles(&references);
    assert_eq!(profiles.len(), 1);
    assert_eq!(
        profiles[0]
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [100, 101]
    );
    assert_eq!(
        profiles[0]
            .references
            .iter()
            .map(|reference| reference.data_block.as_str())
            .collect::<Vec<_>>(),
        ["block-10", "block-11"]
    );
    assert_eq!(
        profiles[0]
            .references
            .iter()
            .map(|reference| reference.witness_source_offset)
            .collect::<Vec<_>>(),
        [30, 31]
    );

    for ordinal in [0, 2] {
        let mut malformed = references.clone();
        malformed[1].ordinal = ordinal;
        assert!(super::feature_extrude_construction_profiles(&malformed).is_empty());
    }

    let mut unwitnessed = references.clone();
    unwitnessed[1].witness_source_offset = None;
    assert!(super::feature_extrude_construction_profiles(&unwitnessed).is_empty());
    let mut unresolved = references;
    unresolved[1].data_block = None;
    assert!(super::feature_extrude_construction_profiles(&unresolved).is_empty());
}

#[test]
fn nx_operation_body_operands_require_known_distinct_body_identities() {
    use super::{FeatureBodyReference, FeatureOperationBodyMember};
    use crate::native::segments::SegmentBodyBinding;
    let member = |ordinal, member_index| FeatureOperationBodyMember {
        id: format!("nx:feature-history:operation-body-member#0-{ordinal}"),
        operation_label: "operation".to_string(),
        body_reference_ordinal: 0,
        body_object_index: 10,
        ordinal,
        member: crate::om::compact::LocatedCompactIndex {
            atom: crate::om::compact::CompactIndexAtom::from_wire(
                member_index,
                &[member_index as u8],
            )
            .unwrap(),
            offset: u64::from(ordinal),
        },
    };
    let members = [member(0, 20), member(1, 30), member(2, 10)];
    let references = [FeatureBodyReference {
        id: "reference".to_string(),
        operation_label: "earlier".to_string(),
        ordinal: Some(0),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(20, &[20]).unwrap(),
        source_offset: 0,
    }];
    let bindings = [SegmentBodyBinding {
        id: "binding".to_string(),
        stream_link: "stream".to_string(),
        stream_ordinal: 0,
        stream_kind: crate::parasolid::StreamKind::Partition,
        body_object_index: 40,
        body_alias_object_index: 30,
        stream_role: 0,
        source_offset: 0,
    }];
    let operands =
        super::feature_operation_body_operands(&members, &references, &[], &[], &bindings);
    assert_eq!(
        operands
            .iter()
            .map(|operand| operand.operand.atom.value())
            .collect::<Vec<_>>(),
        [20, 30]
    );
    assert!(operands[0].segment_body_bindings.is_empty());
    assert_eq!(operands[1].segment_body_bindings, ["binding"]);

    let mut second_clause = operands[0].clone();
    second_clause.body_reference_ordinal = 1;
    assert_eq!(
        operands[0].source_property_key(),
        "operation_body_operand.0.0"
    );
    assert_eq!(
        second_clause.source_property_key(),
        "operation_body_operand.1.0"
    );

    let input = |operation: &str, data_block: &str| FeatureInputBlock {
        id: format!("input-{operation}"),
        operation_label: operation.to_string(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(1, &[1]).unwrap(),
        data_block: data_block.to_string(),
        source_offset: 0,
    };
    let block = |id: &str, section_ordinal| crate::native::om::DataBlock {
        id: id.to_string(),
        section_ordinal,
        block_ordinal: 20,
        role: crate::native::om::DataBlockRole::Column,
        section_offset: 0,
        byte_len: 1,
        sha256: "hash".to_string(),
        stable_identity: None,
        source_entry: "entry".to_string(),
        source_offset: 0,
    };
    let inputs = [
        input("operation", "nx:om-data-blocks-1:block#1"),
        input("earlier", "nx:om-data-blocks-2:block#1"),
    ];
    let blocks = [
        block("nx:om-data-blocks-1:block#20", 1),
        block("nx:om-data-blocks-1:block#30", 1),
        block("nx:om-data-blocks-2:block#20", 2),
    ];
    assert!(super::feature_operation_body_operands(
        &members,
        &references,
        &inputs,
        &blocks,
        &bindings,
    )
    .is_empty());

    let same_store_reference = FeatureBodyReference {
        operation_label: "same-store".to_string(),
        ..references[0].clone()
    };
    let mut same_store_inputs = inputs.to_vec();
    same_store_inputs.push(input("same-store", "nx:om-data-blocks-1:block#2"));
    let same_store = super::feature_operation_body_operands(
        &members,
        &[same_store_reference],
        &same_store_inputs,
        &blocks,
        &bindings,
    );
    assert_eq!(same_store.len(), 2);
    assert_eq!(
        same_store[0].operand_data_block.as_deref(),
        Some("nx:om-data-blocks-1:block#20")
    );
    assert_eq!(
        same_store[1].operand_data_block.as_deref(),
        Some("nx:om-data-blocks-1:block#30")
    );
    assert!(same_store[0].segment_body_bindings.is_empty());

    let distinct_member = member(0, 30);
    let distinct_member_operand = super::feature_operation_body_operands(
        &[distinct_member],
        &[FeatureBodyReference {
            operation_label: "same-store".to_string(),
            ..references[0].clone()
        }],
        &same_store_inputs,
        &blocks,
        &bindings,
    );
    assert_eq!(distinct_member_operand.len(), 1);
    assert_eq!(distinct_member_operand[0].operand.atom.value(), 30);
    assert_eq!(
        distinct_member_operand[0].operand_data_block.as_deref(),
        Some("nx:om-data-blocks-1:block#30")
    );
    assert!(super::feature_operation_body_operands(
        &members,
        &[FeatureBodyReference {
            operation_label: "same-store".to_string(),
            ..references[0].clone()
        }],
        &same_store_inputs,
        &blocks[2..],
        &bindings,
    )
    .is_empty());
}

#[test]
fn nx_extrude_32_construction_requires_resolved_contiguous_profile() {
    let reference = super::FeatureExtrudeProfileReference {
        id: "profile#0".to_string(),
        operation_label: "operation".to_string(),
        ordinal: 0,
        field_tag: 0x16,
        witness_source_offset: None,
        token: crate::om::reference_index::PayloadIndexToken::from_wire(100, &[0xf0, 100]).unwrap(),
        data_block: Some("block#100".to_string()),
        source_offset: 10,
    };
    let branch = crate::native::features::extrude_32::FeatureExtrudePayload32Branch {
        id: "branch".to_string(),
        operation_label: "operation".to_string(),
        frame: crate::om::extrude_32::Extrude32Frame::new(
            20,
            crate::om::scalar::ShiftedBinary64::read(&[0x2f, 0xf0, 0, 0, 0, 0, 0, 0]).unwrap(),
            crate::om::branch_items::BranchItems::new(vec![(
                crate::om::compact::WrappedCompactIndex::from_wire(1, 0x3d80_0100).unwrap(),
                Some("block#1".to_string()),
            )])
            .unwrap(),
            crate::om::branch_items::BranchItems::new(vec![(
                crate::om::compact::CompactIndexAtom::from_wire(2, &[2]).unwrap(),
                Some("block#2".to_string()),
            )])
            .unwrap(),
            crate::om::branch_items::BranchItems::new(vec![(
                crate::om::compact::CompactIndexAtom::from_wire(3, &[3]).unwrap(),
                Some("block#3".to_string()),
            )])
            .unwrap(),
            crate::om::reference_index::FeatureReferenceToken::from_wire(42, &[42]).unwrap(),
        )
        .unwrap(),
    };
    let constructions = super::feature_extrude_32_constructions(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&branch),
    );
    assert_eq!(constructions.len(), 1);
    assert_eq!(constructions[0].body_object_index, 42);
    assert_eq!(
        constructions[0]
            .profiles
            .as_slice()
            .iter()
            .map(|member| member.reference.as_str())
            .collect::<Vec<_>>(),
        ["profile#0"]
    );
    assert_eq!(
        constructions[0]
            .profiles
            .as_slice()
            .iter()
            .map(|member| member.data_block.as_str())
            .collect::<Vec<_>>(),
        ["block#100"]
    );
    assert_eq!(constructions[0].atom_data_blocks.as_slice(), ["block#1"]);
    assert_eq!(constructions[0].first_data_blocks.as_slice(), ["block#2"]);
    assert_eq!(constructions[0].second_data_blocks.as_slice(), ["block#3"]);

    assert!(super::feature_extrude_32_constructions(
        std::slice::from_ref(&reference),
        &[branch.clone(), branch.clone()],
    )
    .is_empty());

    let mut unresolved = reference;
    unresolved.data_block = None;
    assert!(
        super::feature_extrude_32_constructions(&[unresolved], std::slice::from_ref(&branch),)
            .is_empty()
    );
    let mut unresolved_lane = branch;
    unresolved_lane.frame =
        unresolved_lane
            .frame
            .map_bindings(|index, binding| if index == 2 { None } else { binding });
    assert!(super::feature_extrude_32_constructions(
        &[super::FeatureExtrudeProfileReference {
            id: "profile#0".to_string(),
            operation_label: "operation".to_string(),
            ordinal: 0,
            field_tag: 0x16,
            witness_source_offset: None,
            token: crate::om::reference_index::PayloadIndexToken::from_wire(100, &[0xf0, 100])
                .unwrap(),
            data_block: Some("block#100".to_string()),
            source_offset: 10,
        }],
        &[unresolved_lane],
    )
    .is_empty());
}

#[test]
fn nx_block_construction_requires_complete_resolved_reference_field() {
    let references = (0..19)
        .map(|ordinal| {
            crate::native::features::block_reference::FeatureBlockConstructionReference {
                id: format!("reference#{ordinal}"),
                operation_label: "operation".to_string(),
                control: 0x26,
                position: crate::native::features::block_reference::BlockReferencePosition::new(
                    ordinal,
                )
                .unwrap(),
                token: crate::om::reference_index::PayloadIndexToken::from_wire(
                    ordinal + 100,
                    &[0xf0, (ordinal + 100) as u8],
                )
                .unwrap(),
                data_block: Some(format!("block#{ordinal}")),
                source_offset: u64::from(ordinal),
            }
        })
        .collect::<Vec<_>>();
    let constructions = super::feature_block_constructions(&references);
    assert_eq!(constructions.len(), 1);
    assert_eq!(constructions[0].control, 0x26);
    assert_eq!(constructions[0].members.len(), 18);
    assert_eq!(constructions[0].terminal_reference, "reference#18");
    assert_eq!(constructions[0].terminal_data_block, "block#18");

    let mut duplicate = references.clone();
    duplicate[7].position =
        crate::native::features::block_reference::BlockReferencePosition::new(8).unwrap();
    assert!(super::feature_block_constructions(&duplicate).is_empty());

    let mut unresolved = references;
    unresolved[7].data_block = None;
    assert!(super::feature_block_constructions(&unresolved).is_empty());
}

#[test]
fn data_block_object_frame_ids_include_the_store_qualifier() {
    let first = super::data_block_object_frame_id("nx:om-data-blocks-2:block#17", 0);
    let second = super::data_block_object_frame_id("nx:om-data-blocks-3:block#17", 0);
    assert_eq!(first, "nx:om-data-block-object-frames-2:block-frame#17-0");
    assert_eq!(second, "nx:om-data-block-object-frames-3:block-frame#17-0");
    assert_ne!(first, second);
}

#[test]
fn feature_input_identity_groups_require_distinct_operations_and_preserve_order() {
    use super::{feature_input_block_identity_groups, FeatureInputBlock};

    let input = |id: &str, operation: &str, slot: u8, block: &str, offset: u64| FeatureInputBlock {
        id: id.to_string(),
        operation_label: operation.to_string(),
        input_slot: crate::om::header_references::HeaderSlot::try_from(slot).unwrap(),
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(7, &[7]).unwrap(),
        data_block: block.to_string(),
        source_offset: offset,
    };
    let groups = feature_input_block_identity_groups(&[
        input("late", "operation-b", 1, "block-7", 30),
        input("single-a", "operation-a", 0, "block-8", 10),
        input("early", "operation-a", 2, "block-7", 20),
        input("single-b", "operation-a", 3, "block-8", 40),
    ]);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].data_block, "block-7");
    assert_eq!(
        groups[0]
            .members
            .iter()
            .map(|member| member.input_block.as_str())
            .collect::<Vec<_>>(),
        ["early", "late"]
    );
    assert_eq!(
        groups[0]
            .members
            .iter()
            .map(|member| member.operation_label.as_str())
            .collect::<Vec<_>>(),
        ["operation-a", "operation-b"]
    );
    assert_eq!(
        groups[0]
            .members
            .iter()
            .map(|member| member.input_slot.number())
            .collect::<Vec<_>>(),
        [2, 1]
    );
    assert_eq!(
        groups[0]
            .members
            .iter()
            .map(|member| member.source_offset)
            .collect::<Vec<_>>(),
        [20, 30]
    );
}

#[test]
fn feature_input_column_row_uses_preserve_index_row_slots() {
    use super::{feature_input_column_row_uses, ColumnIndexRowKind, FeatureInputBlock};
    use crate::native::om::column_row::DataBlockIndexRow;

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: crate::om::header_references::HeaderSlot::Two,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(7, &[7]).unwrap(),
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockIndexRow {
        id: "row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        frame: IndexRow::<String, u64>::new(
            crate::om::compact::CompactIndexAtom::from_wire(20, &[128, 20]).unwrap(),
            crate::om::discriminators::LinkedIndexFlag::Form03,
            [4, 4, 5, 6].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            100,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
    };

    let uses = feature_input_column_row_uses(&[input], &[row], &[], &[], &[]);
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot.number(), 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::Index);
    assert_eq!(uses[0].column_row, "row#3");
    assert_eq!(u8::from(uses[0].row_slot), 0);
    assert_eq!(uses[0].source_offset, 108);
    assert_eq!(u8::from(uses[1].row_slot), 1);
    assert_eq!(uses[1].source_offset, 109);
}

#[test]
fn feature_input_column_row_uses_preserve_linked_row_slots() {
    use super::{
        feature_input_column_row_uses, feature_input_column_targets, ColumnIndexRowKind,
        FeatureInputBlock, FeatureInputColumnTargetRow,
    };
    use crate::native::om::column_row::DataBlockLinkedIndexRow;
    use crate::native::om::DataBlockColumnIndexTable;

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: crate::om::header_references::HeaderSlot::Two,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(4, &[4]).unwrap(),
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockLinkedIndexRow {
        id: "linked-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        frame: LinkedRow::<String, u64>::new(
            crate::om::compact::CompactIndexAtom::from_wire(20, &[128, 20]).unwrap(),
            crate::om::discriminators::LinkedIndexDiscriminator::Form16,
            CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[4]).unwrap(),
                target: "block#4".into(),
            },
            [5, 6, 4].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            crate::om::discriminators::LinkedIndexFlag::Form03,
            crate::om::discriminators::IndexRowMode::Form04,
            100,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
    };

    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: row.id.clone(),
        rows: crate::native::om::column_index::ColumnIndexRows::new(
            4,
            vec!["target-row".into()],
            vec!["suffix-row".into()],
        )
        .unwrap(),
        source_entry: "entry".into(),
        source_offset: 100,
    };
    let uses = feature_input_column_row_uses(
        std::slice::from_ref(&input),
        &[],
        std::slice::from_ref(&row),
        &[],
        &[table],
    );
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot.number(), 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::LinkedIndex);
    assert_eq!(uses[0].column_row, "linked-row#3");
    assert_eq!(u8::from(uses[0].row_slot), 0);
    assert_eq!(uses[0].source_offset, 107);
    assert_eq!(u8::from(uses[1].row_slot), 3);
    assert_eq!(uses[1].source_offset, 114);
    let targets = feature_input_column_targets(&[input], &uses, &[row], &[]);
    assert_eq!(targets.len(), 1);
    assert_eq!(
        targets[0].row,
        FeatureInputColumnTargetRow::Linked {
            leading_index: 20,
            leading_index_source_offset: 102,
            discriminator: crate::om::discriminators::LinkedIndexDiscriminator::Form16,
            flag: crate::om::discriminators::LinkedIndexFlag::Form03,
        }
    );
    assert_eq!(targets[0].field_indices, [5, 6, 4]);
    assert_eq!(u8::from(targets[0].mode), 4);
}

#[test]
fn feature_input_column_row_uses_preserve_target_row_slots() {
    use super::{
        feature_input_column_row_uses, feature_input_column_targets, ColumnIndexRowKind,
        FeatureInputBlock, FeatureInputColumnTargetRow,
    };
    use crate::native::om::column_row::DataBlockTargetIndexRow;
    use crate::native::om::DataBlockColumnIndexTable;

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: crate::om::header_references::HeaderSlot::Two,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(4, &[4]).unwrap(),
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockTargetIndexRow {
        id: "target-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        frame: TargetRow::<String, u64>::new(
            CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[4]).unwrap(),
                target: "block#4".into(),
            },
            [5, 6, 4].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            crate::om::discriminators::IndexRowMode::Form07,
            100,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
    };

    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: "opening-row".into(),
        rows: crate::native::om::column_index::ColumnIndexRows::new(
            5,
            vec!["target-row#3".into()],
            vec!["suffix-row".into()],
        )
        .unwrap(),
        source_entry: "entry".into(),
        source_offset: 50,
    };
    let ambiguous = feature_input_column_row_uses(
        std::slice::from_ref(&input),
        &[],
        &[],
        std::slice::from_ref(&row),
        &[table.clone(), table.clone()],
    );
    assert!(ambiguous.iter().all(|use_| use_.column_table.is_none()));
    let uses = feature_input_column_row_uses(
        std::slice::from_ref(&input),
        &[],
        &[],
        std::slice::from_ref(&row),
        &[table],
    );
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot.number(), 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::TargetIndex);
    assert_eq!(uses[0].column_row, "target-row#3");
    assert_eq!(uses[0].column_table.as_deref(), Some("column-table"));
    assert_eq!(u8::from(uses[0].row_slot), 0);
    assert_eq!(uses[0].source_offset, 105);
    assert_eq!(u8::from(uses[1].row_slot), 3);
    assert_eq!(uses[1].source_offset, 112);
    let targets = feature_input_column_targets(
        std::slice::from_ref(&input),
        &uses,
        &[],
        std::slice::from_ref(&row),
    );
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].input_block, input.id);
    assert_eq!(targets[0].column_row, "target-row#3");
    assert_eq!(targets[0].column_table, "column-table");
    assert_eq!(targets[0].field_indices, [5, 6, 4]);
    assert_eq!(
        targets[0].field_data_blocks,
        ["block#5", "block#6", "block#4"]
    );
    assert_eq!(targets[0].field_source_offsets, [110, 111, 112]);
    assert_eq!(u8::from(targets[0].mode), 7);
    assert_eq!(targets[0].row, FeatureInputColumnTargetRow::Target);
    let mut duplicate = uses.clone();
    duplicate.push(uses[0].clone());
    assert!(feature_input_column_targets(&[input], &duplicate, &[], &[row]).is_empty());
}

#[test]
fn datum_csys_column_row_uses_preserve_both_lane_offsets() {
    use super::{
        feature_datum_csys_column_row_uses, ColumnIndexRowKind, FeatureDatumCsysConstruction,
    };
    use crate::native::om::column_row::DataBlockTargetIndexRow;
    use crate::native::om::DataBlockColumnIndexTable;

    let construction = FeatureDatumCsysConstruction {
        id: "construction#1".into(),
        operation_label: "operation#1".into(),
        frame: crate::om::datum_csys::DatumCsysFrame::new(
            0x16,
            181,
            std::array::from_fn(|slot| {
                (
                    crate::om::reference_index::PayloadIndexToken::from_wire(
                        slot as u32,
                        &[0xf0, slot as u8],
                    )
                    .unwrap(),
                    format!("block#{slot}"),
                )
            }),
        )
        .unwrap(),
    };
    let row = DataBlockTargetIndexRow {
        id: "target-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        frame: TargetRow::<String, u64>::new(
            CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[5]).unwrap(),
                target: "block#5".into(),
            },
            [6, 7, 5].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            crate::om::discriminators::IndexRowMode::Form07,
            100,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
    };
    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: "opening-row".into(),
        rows: crate::native::om::column_index::ColumnIndexRows::new(
            5,
            vec![row.id.clone()],
            vec!["suffix-row".into()],
        )
        .unwrap(),
        source_entry: "entry".into(),
        source_offset: 50,
    };

    let uses = feature_datum_csys_column_row_uses(&[construction], &[], &[], &[row], &[table]);
    assert_eq!(uses.len(), 4);
    assert_eq!(
        uses.iter()
            .map(|use_| (u8::from(use_.construction_slot), u8::from(use_.row_slot)))
            .collect::<Vec<_>>(),
        [(5, 0), (5, 3), (6, 1), (7, 2)]
    );
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::TargetIndex);
    assert_eq!(uses[0].column_table.as_deref(), Some("column-table"));
    assert_eq!(uses[0].construction_source_offset, 205);
    assert_eq!(uses[0].row_source_offset, 105);
    assert_eq!(uses[1].construction_source_offset, 205);
    assert_eq!(uses[1].row_source_offset, 112);
}
mod sketch_point_ownership;
