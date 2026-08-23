// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]

use std::io::{Cursor, Write};
use std::sync::Arc;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::LossCategory;
use cadmpeg_ir::Exactness;

use crate::container;
use crate::container::{DirEntry, Region};
use crate::om::{EntityRecord, IndexedSection};
use crate::parasolid::{self, StreamKind};
use crate::test_support::*;
use crate::NxCodec;

use super::*;

#[test]
fn unique_offset_data_store_rejects_a_second_matching_section() {
    let entry = DirEntry {
        name: "section".into(),
        region: Region::Header,
        file_span: None,
    };
    let section = || IndexedSection {
        base: 0,
        entity_index_offset: 0,
        object_id_table_offset: 0,
        types: Arc::from([]),
        fields: Arc::from([]),
        control: None,
        column_storage: None,
        records: Arc::from([EntityRecord {
            object_id: None,
            object_id_offset: None,
            offset: 0,
            bytes: &[],
        }]),
    };
    let first = section();
    let second = section();
    let single = section();
    assert_eq!(
        super::unique_offset_data_store(&[(&entry, single)], &[1]),
        Some(0)
    );
    let indexed = [(&entry, first), (&entry, second)];

    assert_eq!(super::unique_offset_data_store(&indexed, &[1]), None);
}

#[test]
fn nx_feature_source_content_orders_payload_text() {
    let text = super::FeaturePayloadString {
        id: "text".into(),
        operation_record: "record".into(),
        ordinal: 0,
        value: "Through".into(),
        source_offset: 30,
    };
    let later = super::FeaturePayloadString {
        id: "later".into(),
        operation_record: "record".into(),
        ordinal: 1,
        value: "Later".into(),
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
fn nx_symbolic_thread_retains_all_complete_type_three_text_frames() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SYMBOLIC_THREAD",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x03\x0bM Profile\0\x03\x0aM3_x_0.5\0\x03\x05CUT\0";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 500,
        payload,
        label,
    };

    let frames = super::symbolic_thread_text_frames(record).expect("two text frames");
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].marker, 0x03);
    assert_eq!(frames[0].offset, 500);
    assert_eq!(frames[0].value, "M Profile");
    assert_eq!(frames[1].offset, 512);
    assert_eq!(frames[1].value, "M3_x_0.5");
    assert_eq!(frames[2].offset, 523);
    assert_eq!(frames[2].value, "CUT");
}

#[test]
fn nx_symbolic_thread_requires_two_complete_type_three_text_frames() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SYMBOLIC_THREAD",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x03\x0bM Profile\0\x03\x0aM3_x_0.5";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 500,
        payload,
        label,
    };

    assert!(super::symbolic_thread_text_frames(record).is_none());
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
        member_references: Vec::new(),
        member_data_blocks: Vec::new(),
        terminal_reference: "terminal-reference".into(),
        terminal_data_block: "terminal-block".into(),
    };
    let binding = FeatureParameterBinding {
        id: "binding".into(),
        operation_label: operation.into(),
        input_slot: 0,
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
        name: format!("p{index}"),
        parameter_index: index,
        qualifier: None,
        literal: None,
        source_entry: source_entry.into(),
        source_offset: u64::from(index),
    };
    let expression = |index: u32, source_entry: &str, source_table: &str| Expression {
        id: format!("expression-{index}"),
        object_id: Some(index),
        record: Some(format!("{source_entry}:entry#{index}")),
        declaration: Some(format!("declaration-{index}")),
        name: format!("p{index}"),
        parameter_index: Some(index),
        qualifier: None,
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
    assert_eq!(dimensions[0].values, [508.0, 533.4, 558.8]);
}

#[test]
fn nx_boolean_projection_rejects_target_tool_alias_overlap() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};
    use std::collections::BTreeMap;

    let operation = super::FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#0".to_string(),
        kind: super::FeatureBooleanKind::Subtract,
        target_object_index: 10,
        raw_target_object_index: vec![10],
        target_source_offset: 0,
        tool_object_indices: vec![20],
        raw_tool_object_indices: vec![vec![20]],
        tool_source_offsets: vec![1],
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
            op: BooleanOp::Cut,
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
fn nx_simple_hole_template_requires_exact_ordered_tokens() {
    use super::{
        FeatureOperationLabel, FeatureOperationRecord, FeaturePayloadString,
        SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
    };

    let label = FeatureOperationLabel {
        id: "operation#3".to_string(),
        section_link: "section#0".to_string(),
        ordinal: 3,
        value: "SIMPLE HOLE".to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        stable_identity: None,
        source_offset: 100,
    };
    let record = FeatureOperationRecord {
        id: "record#3".to_string(),
        operation_label: label.id.clone(),
        ordinal: 3,
        byte_len: 80,
        sha256: "a".repeat(64),
        payload_byte_len: 40,
        payload_sha256: "b".repeat(64),
        stable_identity: None,
        payload_source_offset: 120,
        source_offset: 90,
    };
    let string = FeaturePayloadString {
        id: "payload-string#3-0".to_string(),
        operation_record: record.id.clone(),
        ordinal: 0,
        value: "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer".to_string(),
        source_offset: 130,
    };
    let templates = super::feature_simple_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&string),
    );
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].payload_string, string.id);
    assert_eq!(templates[0].family, SimpleHoleFamily::GeneralHole);
    assert_eq!(templates[0].form, SimpleHoleForm::Simple);
    assert_eq!(templates[0].extent, SimpleHoleExtent::Through);
    assert_eq!(
        templates[0].start_treatment,
        SimpleHoleEndTreatment::Chamfer
    );
    assert_eq!(templates[0].end_treatment, SimpleHoleEndTreatment::Chamfer);

    assert_eq!(
        super::parse_simple_hole_template("Hole_GeneralHole_Counterbored_Through"),
        Some((
            SimpleHoleFamily::GeneralHole,
            SimpleHoleForm::Counterbored,
            SimpleHoleExtent::Through,
            SimpleHoleEndTreatment::None,
            SimpleHoleEndTreatment::None,
        ))
    );
    assert_eq!(
        super::parse_simple_hole_template("Hole_GeneralHole_Simple_Blind"),
        Some((
            SimpleHoleFamily::GeneralHole,
            SimpleHoleForm::Simple,
            SimpleHoleExtent::Blind,
            SimpleHoleEndTreatment::None,
            SimpleHoleEndTreatment::None,
        ))
    );
    assert_eq!(
        super::parse_simple_hole_template("Hole_GeneralHole_Countersunk_Through"),
        Some((
            SimpleHoleFamily::GeneralHole,
            SimpleHoleForm::Countersunk,
            SimpleHoleExtent::Through,
            SimpleHoleEndTreatment::None,
            SimpleHoleEndTreatment::None,
        ))
    );
    assert_eq!(
        super::parse_simple_hole_template("Hole_GeneralHole_Countersunk_Blind"),
        Some((
            SimpleHoleFamily::GeneralHole,
            SimpleHoleForm::Countersunk,
            SimpleHoleExtent::Blind,
            SimpleHoleEndTreatment::None,
            SimpleHoleEndTreatment::None,
        ))
    );
    assert!(super::parse_simple_hole_template("Hole_GeneralHole_Simple_Through").is_none());
    assert!(super::parse_simple_hole_template("Hole_GeneralHole_Counterbored_Blind").is_none());

    let mut counterbored_label = label.clone();
    counterbored_label.id = "operation#4".to_string();
    counterbored_label.value = "CBORE_HOLE".to_string();
    let mut counterbored_record = record.clone();
    counterbored_record.id = "record#4".to_string();
    counterbored_record.operation_label = counterbored_label.id.clone();
    let counterbored_string = FeaturePayloadString {
        id: "payload-string#4-0".to_string(),
        operation_record: counterbored_record.id.clone(),
        ordinal: 0,
        value: "Hole_GeneralHole_Counterbored_Through".to_string(),
        source_offset: 130,
    };
    let counterbored_templates = super::feature_simple_hole_templates(
        &[counterbored_label],
        &[counterbored_record],
        &[counterbored_string],
    );
    let [counterbored_template] = counterbored_templates.as_slice() else {
        panic!("counterbored hole template was not admitted");
    };
    assert_eq!(counterbored_template.form, SimpleHoleForm::Counterbored);
    assert_eq!(counterbored_template.extent, SimpleHoleExtent::Through);

    let mut countersunk_label = label.clone();
    countersunk_label.id = "operation#5".to_string();
    countersunk_label.value = "CSUNK_HOLE".to_string();
    let mut countersunk_record = record.clone();
    countersunk_record.id = "record#5".to_string();
    countersunk_record.operation_label = countersunk_label.id.clone();
    let countersunk_string = FeaturePayloadString {
        id: "payload-string#5-0".to_string(),
        operation_record: countersunk_record.id.clone(),
        ordinal: 0,
        value: "Hole_GeneralHole_Countersunk_Blind".to_string(),
        source_offset: 130,
    };
    let countersunk_templates = super::feature_simple_hole_templates(
        &[countersunk_label],
        &[countersunk_record],
        &[countersunk_string],
    );
    let [countersunk_template] = countersunk_templates.as_slice() else {
        panic!("countersunk hole template was not admitted");
    };
    assert_eq!(countersunk_template.form, SimpleHoleForm::Countersunk);
    assert_eq!(countersunk_template.extent, SimpleHoleExtent::Blind);

    let mut duplicate = string.clone();
    duplicate.id = "payload-string#3-1".to_string();
    duplicate.ordinal = 1;
    duplicate.source_offset += 64;
    assert!(super::feature_simple_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        &[string.clone(), duplicate],
    )
    .is_empty());

    let unknown = FeaturePayloadString {
        id: "payload-string#3-1".to_string(),
        operation_record: record.id.clone(),
        ordinal: 1,
        value: "Hole_Unknown".to_string(),
        source_offset: 194,
    };
    assert!(super::feature_simple_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        &[string.clone(), unknown],
    )
    .is_empty());

    let mut malformed = string;
    malformed.value = "Hole_GeneralHole_Simple_Through_EndChamfer_StartChamfer".to_string();
    assert!(super::feature_simple_hole_templates(&[label], &[record], &[malformed]).is_empty());
}

#[test]
fn nx_threaded_hole_template_requires_simple_hole_and_exact_tokens() {
    use super::{
        FeatureOperationLabel, FeatureOperationRecord, FeaturePayloadString, SimpleHoleExtent,
        ThreadedHoleFamily,
    };

    let label = FeatureOperationLabel {
        id: "operation#threaded".to_string(),
        section_link: "section#0".to_string(),
        ordinal: 7,
        value: "SIMPLE HOLE".to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        stable_identity: None,
        source_offset: 100,
    };
    let record = FeatureOperationRecord {
        id: "record#threaded".to_string(),
        operation_label: label.id.clone(),
        ordinal: 7,
        byte_len: 80,
        sha256: "a".repeat(64),
        payload_byte_len: 40,
        payload_sha256: "b".repeat(64),
        stable_identity: None,
        payload_source_offset: 120,
        source_offset: 90,
    };
    let string = FeaturePayloadString {
        id: "payload-string#threaded-0".to_string(),
        operation_record: record.id.clone(),
        ordinal: 0,
        value: "Hole_ThreadedHole_M Profile_Blind".to_string(),
        source_offset: 130,
    };
    let templates = super::feature_threaded_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&string),
    );
    let [template] = templates.as_slice() else {
        panic!("threaded-hole template was not admitted");
    };
    assert_eq!(template.payload_string, string.id);
    assert_eq!(template.family, ThreadedHoleFamily::MProfile);
    assert_eq!(template.extent, SimpleHoleExtent::Blind);
    assert_eq!(template.source_offset, string.source_offset);
    assert_eq!(
        super::parse_threaded_hole_template("Hole_ThreadedHole_UNC_Blind"),
        Some((ThreadedHoleFamily::Unc, SimpleHoleExtent::Blind,))
    );
    assert!(super::parse_threaded_hole_template("Hole_ThreadedHole_UNF_Blind").is_none());
    assert!(super::parse_threaded_hole_template("Hole_ThreadedHole_M Profile_Through").is_none());

    let mut non_simple_label = label.clone();
    non_simple_label.value = "CBORE_HOLE".to_string();
    assert!(super::feature_threaded_hole_templates(
        &[non_simple_label],
        std::slice::from_ref(&record),
        std::slice::from_ref(&string),
    )
    .is_empty());

    let mut duplicate = string.clone();
    duplicate.id = "payload-string#threaded-1".to_string();
    duplicate.ordinal = 1;
    duplicate.source_offset += 64;
    assert!(super::feature_threaded_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        &[string.clone(), duplicate],
    )
    .is_empty());

    let mut unknown = string;
    unknown.value = "Hole_ThreadedHole_M Profile_Blind_Extra".to_string();
    assert!(super::feature_threaded_hole_templates(&[label], &[record], &[unknown]).is_empty());
}

#[test]
fn nx_sketch_record_joins_exact_operation_and_ordered_input_lanes() {
    use super::{
        FeatureInputBlock, FeatureOperationLabel, FeatureOperationRecord, FeatureSketchReference,
    };

    let label = FeatureOperationLabel {
        id: "nx:feature-history:operation-label#0-7".to_string(),
        section_link: "nx:feature-history#0".to_string(),
        ordinal: 7,
        value: "SKETCH".to_string(),
        object_indices: [Some(45), None, Some(81), None],
        raw_object_indices: [vec![45], vec![0xff], vec![81], vec![0xff]],
        stable_identity: None,
        source_offset: 700,
    };
    let record = FeatureOperationRecord {
        id: "nx:feature-history:operation-record#0-7".to_string(),
        operation_label: label.id.clone(),
        ordinal: 7,
        byte_len: 173,
        sha256: "00".repeat(32),
        payload_byte_len: 140,
        payload_sha256: "11".repeat(32),
        stable_identity: None,
        payload_source_offset: 733,
        source_offset: 700,
    };
    let input = |slot, index| FeatureInputBlock {
        id: format!("nx:feature-history:input-block#0-7-{slot}"),
        operation_label: label.id.clone(),
        input_slot: slot,
        object_index: index,
        raw_object_index: vec![index as u8],
        data_block: format!("nx:om-data-blocks-2:block#{index}"),
        source_offset: 710 + u64::from(slot),
    };
    let inputs = [input(2, 81), input(0, 45)];
    let reference = |ordinal, index| FeatureSketchReference {
        id: format!("nx:feature-history:sketch-reference#0-7-{ordinal}"),
        operation_label: label.id.clone(),
        ordinal,
        declared_count: 2,
        terminal: ordinal == 1,
        object_index: index,
        raw_object_index: vec![0xf0, index as u8],
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
        construction[0].member_references,
        ["nx:feature-history:sketch-reference#0-7-0"]
    );
    assert_eq!(
        construction[0].member_data_blocks,
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
    malformed[0].ordinal = 2;
    assert!(super::feature_sketch_construction_inputs(&sketches, &malformed).is_empty());
}

#[test]
fn nx_sketch_payload_join_preserves_order_and_cross_block_values() {
    let ids = vec!["block#2".to_string(), "block#3".to_string()];
    let blocks = std::collections::BTreeMap::from([
        ("block#2".to_string(), (&[0x30, 0x43][..], 120_u64)),
        (
            "block#3".to_string(),
            (&[0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72][..], 900_u64),
        ),
    ]);
    let joined = super::join_data_block_bytes(&ids, &blocks).expect("required invariant");
    assert_eq!(joined.0, [0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72]);
    assert_eq!(joined.1, [0, 2]);
    assert_eq!(joined.2, [2, 6]);
    assert_eq!(joined.3, [120, 900]);

    let missing = vec!["block#2".to_string(), "missing".to_string()];
    assert!(super::join_data_block_bytes(&missing, &blocks).is_none());
}

#[test]
fn nx_offset_store_block_bytes_follow_catalog_identity() {
    let control = crate::om::EntityRecord {
        object_id: None,
        object_id_offset: None,
        offset: 5,
        bytes: &[0xaa],
    };
    let first = crate::om::EntityRecord {
        object_id: None,
        object_id_offset: None,
        offset: 6,
        bytes: &[0xbb],
    };
    let second = crate::om::EntityRecord {
        object_id: None,
        object_id_offset: None,
        offset: 7,
        bytes: &[0xcc],
    };
    let controlled = super::offset_data_block_bytes_for_section(
        3,
        100,
        Some(&control),
        &[first.clone(), second.clone()],
    );
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

    let control_free = super::offset_data_block_bytes_for_section(4, 200, None, &[first, second]);
    assert_eq!(
        control_free["nx:om-data-blocks-4:block#0"],
        (&[0xbb][..], 206)
    );
    assert_eq!(
        control_free["nx:om-data-blocks-4:block#1"],
        (&[0xcc][..], 207)
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
        labels[0].raw_object_indices,
        [
            vec![0x01],
            vec![0x82, 0x40],
            vec![0x90, 0x17, 0xd3],
            vec![0xff]
        ]
    );
    assert_eq!(labels[1].raw_object_indices, labels[0].raw_object_indices);
    let records = namespace
        .arena_as::<super::FeatureOperationRecord>("feature_operation_records")
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
    assert_eq!(areas[0].product_version, "NX 2027.3102");
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
        labels[0].object_indices,
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[0].section_link, areas[0].section_link);
    let records = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::FeatureOperationRecord>("feature_operation_records")
        .expect("required invariant");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation_label, labels[0].id);
    assert!(records[0].byte_len > 40);
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
    assert_eq!(booleans[0].target_object_index, 6466);
    assert_eq!(booleans[0].tool_object_indices, [6476, 127]);
    let body_references = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::FeatureBodyReference>("feature_body_references")
        .expect("required invariant");
    assert_eq!(body_references.len(), 1);
    assert_eq!(body_references[0].operation_label, labels[0].id);
    assert_eq!(body_references[0].body_object_index, 6466);
    let body_reference_occurrences = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::FeatureBodyReferenceOccurrence>("feature_body_reference_occurrences")
        .expect("required invariant");
    assert_eq!(body_reference_occurrences.len(), 1);
    assert_eq!(body_reference_occurrences[0].operation_label, labels[0].id);
    assert_eq!(body_reference_occurrences[0].ordinal, 0);
    assert_eq!(body_reference_occurrences[0].body_object_index, 6466);
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
            op: cadmpeg_ir::features::BooleanOp::Join,
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
    assert_eq!(inputs[0].input_slot, 0);
    assert_eq!(inputs[0].object_index, 1);
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
    assert_eq!(references[0].object_id, 42);
    assert_eq!(references[0].target_record, None);
}

#[test]
fn sketch_point_blocks_establish_ordered_datum_csys_dependencies() {
    use super::{
        FeatureDatumCsysConstruction, FeatureDatumCsysPayloadScalar, FeatureOperationLabel,
        FeatureSketchDatumCsysBlockRelation, FeatureSketchPointUse, OffsetStoreNamedPoint,
    };

    let label = |id: &str, value: &str, ordinal| FeatureOperationLabel {
        id: id.to_string(),
        section_link: "section".to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        stable_identity: None,
        source_offset: 100 + u64::from(ordinal),
    };
    let labels = [label("csys", "DATUM_CSYS", 0), label("sketch", "SKETCH", 1)];
    let point = OffsetStoreNamedPoint {
        id: "point".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["point-first".to_string(), "shared".to_string()],
        values: [1.0, 2.0],
        raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
        value_source_offsets: [200, 220],
        source_offset: 190,
    };
    let point_use = FeatureSketchPointUse {
        id: "point-use".to_string(),
        operation_label: "sketch".to_string(),
        sketch_references: vec!["reference".to_string()],
        block_uses: vec!["block-use".to_string()],
        sketch_point_group: "point-group".to_string(),
        named_point: point.id.clone(),
        source_offsets: vec![300],
    };
    let mut blocks = std::array::from_fn(|index| format!("block-{index}"));
    blocks[3] = "shared".to_string();
    let construction = FeatureDatumCsysConstruction {
        id: "construction".to_string(),
        operation_label: "csys".to_string(),
        control: 19,
        object_indices: [0; 8],
        raw_object_indices: std::array::from_fn(|_| vec![0]),
        data_blocks: blocks,
        source_offsets: [400; 8],
    };
    let scalar = FeatureDatumCsysPayloadScalar {
        id: "csys-scalar".to_string(),
        operation_label: "csys".to_string(),
        datum_csys_payload: "payload".to_string(),
        ordinal: 0,
        field_code: 0x64,
        value: 2.0,
        raw_value: shifted_f64_bytes(2.0),
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
        values: [3.0, 4.0],
        raw_values: [shifted_f64_bytes(3.0), shifted_f64_bytes(4.0)],
        value_source_offsets: [500, 520],
        source_offset: 490,
    };
    let consecutive_use = FeatureSketchPointUse {
        id: "consecutive-use".to_string(),
        named_point: consecutive_point.id.clone(),
        ..point_use.clone()
    };
    let mut consecutive_construction = construction.clone();
    consecutive_construction.id = "consecutive-construction".to_string();
    consecutive_construction.data_blocks[0] = "nx:om:offset-store#7:block#12".to_string();
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
        identity: "012345678901234567890123456789".into(),
        suffix: vec![b'?', b'A'],
        schema_index: 1,
        label: "p".into(),
        source_offset: 10,
    };
    let csys = super::FeatureDatumCsysDescriptor {
        id: "csys-descriptor".into(),
        operation_label: "operation#2".into(),
        construction: "csys-construction".into(),
        reference_ordinal: 7,
        data_block: "csys-block".into(),
        prefix: vec![2, 1],
        identity: plane.identity.clone(),
        suffix: vec![b'?', b'A'],
        source_offset: 20,
        identity_source_offset: 22,
    };
    let uses = super::feature_datum_plane_csys_identity_uses(&[plane], &[csys]);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].identity, "012345678901234567890123456789");
    assert_eq!(uses[0].datum_plane_operation_label, "operation#4");
    assert_eq!(uses[0].datum_csys_operation_label, "operation#2");
    assert_eq!(uses[0].datum_csys_reference_ordinal, 7);
}

#[test]
fn nx_datum_csys_block_uses_preserve_reference_and_input_order() {
    let construction = super::FeatureDatumCsysConstruction {
        id: "construction".to_string(),
        operation_label: "operation#0".to_string(),
        control: 0x13,
        object_indices: std::array::from_fn(|index| index as u32 + 40),
        raw_object_indices: std::array::from_fn(|index| vec![index as u8 + 40]),
        data_blocks: std::array::from_fn(|index| format!("block#{}", index + 40)),
        source_offsets: std::array::from_fn(|index| index as u64 + 100),
    };
    let input = |id: &str, operation: &str, slot: u8, block: &str| super::FeatureInputBlock {
        id: id.to_string(),
        operation_label: operation.to_string(),
        input_slot: slot,
        object_index: 44,
        raw_object_index: vec![44],
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
    assert_eq!(uses[0].reference_ordinal, 3);
    assert_eq!(uses[0].input_operation_label, "operation#0");
    assert_eq!(uses[1].reference_ordinal, 4);
    assert_eq!(uses[1].input_operation_label, "operation#6");
    assert_eq!(uses[2].reference_ordinal, 4);
    assert_eq!(uses[2].input_operation_label, "operation#7");
}

#[test]
fn nx_extrude_construction_profile_requires_matching_resolved_encodings() {
    use super::{
        FeatureExtrudeProfileReference, FeatureOperationBodyReferenceLane,
        FeatureOperationBodyReferenceLaneEncoding,
    };

    let references = [10, 11].map(|ordinal| FeatureExtrudeProfileReference {
        id: format!("profile-{ordinal}"),
        operation_label: "operation".to_string(),
        ordinal: ordinal - 10,
        witnessed: true,
        object_index: ordinal + 90,
        raw_object_index: vec![(ordinal + 90) as u8],
        data_block: Some(format!("block-{ordinal}")),
        source_offset: u64::from(ordinal),
    });
    let lane = FeatureOperationBodyReferenceLane {
        id: "lane".to_string(),
        operation_label: "operation".to_string(),
        body_reference_ordinal: 0,
        body_object_index: 42,
        branch: 0x11,
        encoding: FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex,
        object_indices: vec![100, 101],
        raw_object_indices: vec![vec![0xf0, 100], vec![0xf0, 101]],
        data_blocks: vec![Some("block-10".to_string()), Some("block-11".to_string())],
        source_offsets: vec![20, 21],
    };
    let profiles =
        super::feature_extrude_construction_profiles(&references, std::slice::from_ref(&lane));
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].body_object_index, 42);
    assert_eq!(profiles[0].object_indices, [100, 101]);
    assert_eq!(profiles[0].data_blocks, ["block-10", "block-11"]);

    for ordinal in [0, 2] {
        let mut malformed = references.clone();
        malformed[1].ordinal = ordinal;
        assert!(super::feature_extrude_construction_profiles(
            &malformed,
            std::slice::from_ref(&lane),
        )
        .is_empty());
    }

    let mut mismatched = lane.clone();
    mismatched.object_indices[1] = 102;
    assert!(super::feature_extrude_construction_profiles(&references, &[mismatched]).is_empty());

    let mut unresolved = FeatureOperationBodyReferenceLane {
        id: "lane".to_string(),
        operation_label: "operation".to_string(),
        body_reference_ordinal: 0,
        body_object_index: 42,
        branch: 0x11,
        encoding: FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex,
        object_indices: vec![100, 101],
        raw_object_indices: vec![vec![0xf0, 100], vec![0xf0, 101]],
        data_blocks: vec![Some("block-10".to_string()), Some("block-11".to_string())],
        source_offsets: vec![20, 21],
    };
    unresolved.data_blocks[1] = None;
    assert!(super::feature_extrude_construction_profiles(&references, &[unresolved]).is_empty());
}

#[test]
fn nx_operation_body_operands_require_known_distinct_body_identities() {
    use super::{FeatureBodyReferenceOccurrence, FeatureOperationBodyMember};
    use crate::native::segments::SegmentBodyBinding;
    let member = |ordinal, member_index| FeatureOperationBodyMember {
        id: format!("nx:feature-history:operation-body-member#0-{ordinal}"),
        operation_label: "operation".to_string(),
        body_reference_ordinal: 0,
        body_object_index: 10,
        ordinal,
        member_index,
        raw_member_index: vec![member_index as u8],
        source_offset: u64::from(ordinal),
    };
    let members = [member(0, 20), member(1, 30), member(2, 10)];
    let references = [FeatureBodyReferenceOccurrence {
        id: "reference".to_string(),
        operation_label: "earlier".to_string(),
        ordinal: 0,
        body_object_index: 20,
        raw_body_object_index: vec![20],
        source_offset: 0,
    }];
    let bindings = [SegmentBodyBinding {
        id: "binding".to_string(),
        stream_link: "stream".to_string(),
        stream_ordinal: 0,
        stream_kind: "partition".to_string(),
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
            .map(|operand| operand.operand_object_index)
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
        input_slot: 0,
        object_index: 1,
        raw_object_index: vec![1],
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

    let same_store_reference = FeatureBodyReferenceOccurrence {
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
        &[FeatureBodyReferenceOccurrence {
            operation_label: "same-store".to_string(),
            ..references[0].clone()
        }],
        &same_store_inputs,
        &blocks,
        &bindings,
    );
    assert_eq!(distinct_member_operand.len(), 1);
    assert_eq!(distinct_member_operand[0].operand_object_index, 30);
    assert_eq!(
        distinct_member_operand[0].operand_data_block.as_deref(),
        Some("nx:om-data-blocks-1:block#30")
    );
    assert!(super::feature_operation_body_operands(
        &members,
        &[FeatureBodyReferenceOccurrence {
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
        witnessed: false,
        object_index: 100,
        raw_object_index: vec![100],
        data_block: Some("block#100".to_string()),
        source_offset: 10,
    };
    let branch = super::FeatureExtrudePayload32Branch {
        id: "branch".to_string(),
        operation_label: "operation".to_string(),
        body_object_index: 42,
        scalar: 1.0,
        raw_scalar: [0x2f, 0xf0, 0, 0, 0, 0, 0, 0],
        atoms_be: vec![0x3d80_0100],
        atom_source_offsets: vec![20],
        atom_indices: vec![1],
        atom_data_blocks: vec![Some("block#1".to_string())],
        first_indices: vec![2],
        raw_first_indices: vec![vec![2]],
        first_index_source_offsets: vec![21],
        first_data_blocks: vec![Some("block#2".to_string())],
        second_indices: vec![3],
        raw_second_indices: vec![vec![3]],
        second_index_source_offsets: vec![22],
        second_data_blocks: vec![Some("block#3".to_string())],
        terminal_object_index: 42,
        raw_terminal_object_index: vec![42],
        terminal_source_offset: 23,
        source_offset: 20,
    };
    let constructions = super::feature_extrude_32_constructions(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&branch),
    );
    assert_eq!(constructions.len(), 1);
    assert_eq!(constructions[0].body_object_index, 42);
    assert_eq!(constructions[0].profile_references, ["profile#0"]);
    assert_eq!(constructions[0].profile_data_blocks, ["block#100"]);
    assert_eq!(constructions[0].atom_data_blocks, ["block#1"]);
    assert_eq!(constructions[0].first_data_blocks, ["block#2"]);
    assert_eq!(constructions[0].second_data_blocks, ["block#3"]);

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
    unresolved_lane.first_data_blocks[0] = None;
    assert!(super::feature_extrude_32_constructions(
        &[super::FeatureExtrudeProfileReference {
            id: "profile#0".to_string(),
            operation_label: "operation".to_string(),
            ordinal: 0,
            witnessed: false,
            object_index: 100,
            raw_object_index: vec![100],
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
        .map(|ordinal| super::FeatureBlockConstructionReference {
            id: format!("reference#{ordinal}"),
            operation_label: "operation".to_string(),
            control: 0x26,
            ordinal,
            terminal: ordinal == 18,
            object_index: ordinal + 100,
            raw_object_index: vec![(ordinal + 100) as u8],
            data_block: Some(format!("block#{ordinal}")),
            source_offset: u64::from(ordinal),
        })
        .collect::<Vec<_>>();
    let constructions = super::feature_block_constructions(&references);
    assert_eq!(constructions.len(), 1);
    assert_eq!(constructions[0].control, 0x26);
    assert_eq!(constructions[0].member_references.len(), 18);
    assert_eq!(constructions[0].terminal_reference, "reference#18");
    assert_eq!(constructions[0].terminal_data_block, "block#18");

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
        input_slot: slot,
        object_index: 7,
        raw_object_index: vec![7],
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
    assert_eq!(groups[0].input_blocks, ["early", "late"]);
    assert_eq!(groups[0].operation_labels, ["operation-a", "operation-b"]);
    assert_eq!(groups[0].input_slots, [2, 1]);
    assert_eq!(groups[0].source_offsets, [20, 30]);
}

#[test]
fn feature_input_column_row_uses_preserve_index_row_slots() {
    use super::{feature_input_column_row_uses, ColumnIndexRowKind, FeatureInputBlock};
    use crate::native::om::DataBlockIndexRow;

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: 2,
        object_index: 7,
        raw_object_index: vec![7],
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockIndexRow {
        id: "row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        first_index: 20,
        raw_first_index: vec![20],
        flag: 3,
        indices: [4, 4, 5, 6],
        raw_indices: [vec![4], vec![4], vec![5], vec![6]],
        data_blocks: [
            "block#4".into(),
            "block#4".into(),
            "block#5".into(),
            "block#6".into(),
        ],
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
        source_offset: 100,
        first_index_source_offset: 103,
        index_source_offsets: [108, 109, 110, 111],
    };

    let uses = feature_input_column_row_uses(&[input], &[row], &[], &[], &[]);
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot, 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::Index);
    assert_eq!(uses[0].column_row, "row#3");
    assert_eq!(uses[0].row_slot, 0);
    assert_eq!(uses[0].source_offset, 108);
    assert_eq!(uses[1].row_slot, 1);
    assert_eq!(uses[1].source_offset, 109);
}

#[test]
fn feature_input_column_row_uses_preserve_linked_row_slots() {
    use super::{
        feature_input_column_row_uses, feature_input_column_targets, ColumnIndexRowKind,
        FeatureInputBlock,
    };
    use crate::native::om::{DataBlockColumnIndexTable, DataBlockLinkedIndexRow};

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: 2,
        object_index: 4,
        raw_object_index: vec![4],
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockLinkedIndexRow {
        id: "linked-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        first_index: 20,
        raw_first_index: vec![20],
        discriminator: 0x16,
        target_index: 4,
        raw_target_index: vec![4],
        indices: [5, 6, 4],
        raw_indices: [vec![5], vec![6], vec![4]],
        data_blocks: [
            "block#4".into(),
            "block#5".into(),
            "block#6".into(),
            "block#4".into(),
        ],
        flag: 3,
        mode: 4,
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
        source_offset: 100,
        first_index_source_offset: 102,
        target_index_source_offset: 107,
        index_source_offsets: [112, 113, 114],
    };

    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: row.id.clone(),
        target_rows: vec!["target-row".into()],
        linked_rows: vec!["suffix-row".into()],
        first_target_index: 4,
        last_target_index: 2,
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
    assert_eq!(uses[0].input_slot, 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::LinkedIndex);
    assert_eq!(uses[0].column_row, "linked-row#3");
    assert_eq!(uses[0].row_slot, 0);
    assert_eq!(uses[0].source_offset, 107);
    assert_eq!(uses[1].row_slot, 3);
    assert_eq!(uses[1].source_offset, 114);
    let targets = feature_input_column_targets(&[input], &uses, &[row], &[]);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].leading_index, Some(20));
    assert_eq!(targets[0].leading_index_source_offset, Some(102));
    assert_eq!(targets[0].discriminator, Some(0x16));
    assert_eq!(targets[0].field_indices, [5, 6, 4]);
    assert_eq!(targets[0].flag, Some(3));
    assert_eq!(targets[0].mode, 4);
}

#[test]
fn feature_input_column_row_uses_preserve_target_row_slots() {
    use super::{
        feature_input_column_row_uses, feature_input_column_targets, ColumnIndexRowKind,
        FeatureInputBlock,
    };
    use crate::native::om::{DataBlockColumnIndexTable, DataBlockTargetIndexRow};

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: 2,
        object_index: 4,
        raw_object_index: vec![4],
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockTargetIndexRow {
        id: "target-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        target_index: 4,
        raw_target_index: vec![4],
        indices: [5, 6, 4],
        raw_indices: [vec![5], vec![6], vec![4]],
        data_blocks: [
            "block#4".into(),
            "block#5".into(),
            "block#6".into(),
            "block#4".into(),
        ],
        mode: 7,
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
        source_offset: 100,
        target_index_source_offset: 105,
        index_source_offsets: [110, 111, 112],
    };

    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: "opening-row".into(),
        target_rows: vec!["target-row#3".into()],
        linked_rows: vec!["suffix-row".into()],
        first_target_index: 5,
        last_target_index: 3,
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
    assert_eq!(uses[0].input_slot, 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::TargetIndex);
    assert_eq!(uses[0].column_row, "target-row#3");
    assert_eq!(uses[0].column_table.as_deref(), Some("column-table"));
    assert_eq!(uses[0].row_slot, 0);
    assert_eq!(uses[0].source_offset, 105);
    assert_eq!(uses[1].row_slot, 3);
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
    assert_eq!(targets[0].mode, 7);
    assert_eq!(targets[0].leading_index, None);
    let mut duplicate = uses.clone();
    duplicate.push(uses[0].clone());
    assert!(feature_input_column_targets(&[input], &duplicate, &[], &[row]).is_empty());
}

#[test]
fn datum_csys_column_row_uses_preserve_both_lane_offsets() {
    use super::{
        feature_datum_csys_column_row_uses, ColumnIndexRowKind, FeatureDatumCsysConstruction,
    };
    use crate::native::om::{DataBlockColumnIndexTable, DataBlockTargetIndexRow};

    let construction = FeatureDatumCsysConstruction {
        id: "construction#1".into(),
        operation_label: "operation#1".into(),
        control: 0x16,
        object_indices: std::array::from_fn(|slot| slot as u32),
        raw_object_indices: std::array::from_fn(|slot| vec![slot as u8]),
        data_blocks: std::array::from_fn(|slot| format!("block#{slot}")),
        source_offsets: std::array::from_fn(|slot| 200 + slot as u64),
    };
    let row = DataBlockTargetIndexRow {
        id: "target-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        target_index: 5,
        raw_target_index: vec![5],
        indices: [6, 7, 5],
        raw_indices: [vec![6], vec![7], vec![5]],
        data_blocks: [
            "block#5".into(),
            "block#6".into(),
            "block#7".into(),
            "block#5".into(),
        ],
        mode: 7,
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
        source_offset: 100,
        target_index_source_offset: 105,
        index_source_offsets: [110, 111, 112],
    };
    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: "opening-row".into(),
        target_rows: vec![row.id.clone()],
        linked_rows: vec![],
        first_target_index: 5,
        last_target_index: 3,
        source_entry: "entry".into(),
        source_offset: 50,
    };

    let uses = feature_datum_csys_column_row_uses(&[construction], &[], &[], &[row], &[table]);
    assert_eq!(uses.len(), 4);
    assert_eq!(
        uses.iter()
            .map(|use_| (use_.construction_slot, use_.row_slot))
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

#[test]
fn sketch_named_records_own_fixed_pairs_within_their_intervals() {
    use super::{
        feature_sketch_fixed_points, feature_sketch_payload_named_records,
        FeatureSketchConstructionPayload, FeatureSketchPayloadFixedPair, FeatureSketchPayloadName,
    };
    let payload = FeatureSketchConstructionPayload {
        id: "payload".to_string(),
        operation_label: "sketch".to_string(),
        construction_inputs: "inputs".to_string(),
        data_blocks: vec!["block".to_string()],
        byte_len: 100,
        sha256: "00".repeat(32),
        block_payload_offsets: vec![0],
        block_byte_lengths: vec![100],
        block_source_offsets: vec![1000],
    };
    let name = |id: &str, ordinal, offset| FeatureSketchPayloadName {
        id: id.to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal,
        type_code: Some(1),
        raw_type_code: Some(vec![1]),
        type_code_payload_offset: Some(offset + 1),
        type_code_source_offset: Some(1001 + offset),
        payload_leading: false,
        value: format!("Point{}", ordinal + 1),
        payload_offset: offset,
        source_offset: 1000 + offset,
    };
    let pair = FeatureSketchPayloadFixedPair {
        id: "pair".to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal: 0,
        values: [0.5, -0.5],
        raw_values: [[0; 7]; 2],
        discriminator: vec![0x04],
        payload_offset: 20,
        value_payload_offsets: [28, 37],
        source_offset: 1020,
        value_source_offsets: [1028, 1037],
    };

    let names = [name("first", 0, 10), name("second", 1, 50)];
    let auxiliary_pair = FeatureSketchPayloadFixedPair {
        id: "auxiliary-pair".to_string(),
        ordinal: 1,
        discriminator: vec![0x0b],
        payload_offset: 40,
        source_offset: 1040,
        value_payload_offsets: [55, 64],
        value_source_offsets: [1055, 1064],
        ..pair.clone()
    };
    let pairs = [pair, auxiliary_pair];
    let records = feature_sketch_payload_named_records(&[payload], &names, &[], &pairs, &[]);
    assert_eq!(records[0].fixed_pairs, ["pair", "auxiliary-pair"]);
    assert!(records[1].fixed_pairs.is_empty());
    let points = feature_sketch_fixed_points(&records, &names, &pairs);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].name, "Point1");
    assert_eq!(points[0].values, [0.5, -0.5]);
}

#[test]
fn sketch_named_point_block_uses_require_exact_shared_block_identity() {
    use super::{
        feature_sketch_named_point_block_uses, FeatureSketchReference, OffsetStoreNamedPoint,
    };

    let point = OffsetStoreNamedPoint {
        id: "nx:offset-store:named-point#2-10".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["block-10".to_string(), "block-11".to_string()],
        values: [1.0, 2.0],
        raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
        value_source_offsets: [100, 120],
        source_offset: 90,
    };
    let reference = |id: &str, ordinal: u32, block: Option<&str>| FeatureSketchReference {
        id: id.to_string(),
        operation_label: "nx:feature-history:operation-label#1-4".to_string(),
        ordinal,
        declared_count: 2,
        terminal: ordinal == 1,
        object_index: 10 + ordinal,
        raw_object_index: vec![0xf0, (10 + ordinal) as u8],
        data_block: block.map(str::to_string),
        source_offset: 200 + u64::from(ordinal),
    };
    let uses = feature_sketch_named_point_block_uses(
        &[
            reference("miss", 0, Some("block-9")),
            reference("hit", 1, Some("block-11")),
            reference("unresolved", 2, None),
        ],
        &[point],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].sketch_reference, "hit");
    assert_eq!(uses[0].reference_ordinal, 1);
    assert_eq!(uses[0].point_block_ordinal, 1);
    assert_eq!(uses[0].data_block, "block-11");
}

#[test]
fn sketch_preceding_named_point_uses_require_a_complete_unique_consecutive_lane() {
    use super::{
        feature_sketch_preceding_named_point_uses, FeatureSketchReference, OffsetStoreNamedPoint,
    };

    let reference = |ordinal, terminal, block: Option<&str>| FeatureSketchReference {
        id: format!("reference-{ordinal}"),
        operation_label: "nx:feature-history:operation-label#1-4".to_string(),
        ordinal,
        declared_count: 2,
        terminal,
        object_index: 12 + ordinal,
        raw_object_index: vec![0xf0, (12 + ordinal) as u8],
        data_block: block.map(str::to_string),
        source_offset: 300 + u64::from(ordinal),
    };
    let references = [
        reference(0, false, Some("nx:om-data-blocks-2:block#12")),
        reference(1, true, Some("nx:om-data-blocks-2:block#13")),
    ];
    let point = |id: &str, blocks: &[&str]| OffsetStoreNamedPoint {
        id: id.to_string(),
        name: "Point1".to_string(),
        data_blocks: blocks.iter().map(|block| (*block).to_string()).collect(),
        values: [1.0, 2.0],
        raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
        value_source_offsets: [200, 220],
        source_offset: 190,
    };
    let preceding = point(
        "nx:offset-store:named-point#2-10",
        &[
            "nx:om-data-blocks-2:block#10",
            "nx:om-data-blocks-2:block#11",
        ],
    );
    let uses =
        feature_sketch_preceding_named_point_uses(&references, std::slice::from_ref(&preceding));
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].first_sketch_reference, references[0].id);
    assert_eq!(uses[0].named_point, preceding.id);
    assert_eq!(uses[0].following_data_block, "nx:om-data-blocks-2:block#12");

    let ambiguous = point(
        "nx:offset-store:named-point#2-11",
        &["nx:om-data-blocks-2:block#11"],
    );
    assert!(feature_sketch_preceding_named_point_uses(
        &references,
        &[preceding.clone(), ambiguous]
    )
    .is_empty());
    let gap = point(
        "nx:offset-store:named-point#2-9",
        &["nx:om-data-blocks-2:block#9"],
    );
    let other_store = point(
        "nx:offset-store:named-point#3-11",
        &["nx:om-data-blocks-3:block#11"],
    );
    assert!(feature_sketch_preceding_named_point_uses(&references, &[gap, other_store]).is_empty());

    let unresolved = [references[0].clone(), reference(1, true, None)];
    assert!(feature_sketch_preceding_named_point_uses(
        &unresolved,
        std::slice::from_ref(&preceding)
    )
    .is_empty());
    let noncontiguous = [
        references[0].clone(),
        reference(2, true, Some("nx:om-data-blocks-2:block#13")),
    ];
    assert!(feature_sketch_preceding_named_point_uses(
        &noncontiguous,
        std::slice::from_ref(&preceding),
    )
    .is_empty());
    let bad_terminal = [
        references[0].clone(),
        reference(1, false, Some("nx:om-data-blocks-2:block#13")),
    ];
    assert!(feature_sketch_preceding_named_point_uses(&bad_terminal, &[preceding]).is_empty());
}

#[test]
fn sketch_point_uses_retain_identical_witnesses_and_reject_conflicts() {
    use super::{
        feature_sketch_point_groups, feature_sketch_point_uses, FeatureSketchNamedPointBlockUse,
        FeatureSketchPoint, OffsetStoreNamedPoint,
    };

    let operation_label = "nx:feature-history:operation-label#1-4".to_string();
    let point = FeatureSketchPoint {
        id: "payload-point".to_string(),
        operation_label: operation_label.clone(),
        named_record: "named-record".to_string(),
        name: "Point1".to_string(),
        coordinates: [1.0, 2.0],
        scalar_fields: ["scalar-1".to_string(), "scalar-2".to_string()],
    };
    let named_point = OffsetStoreNamedPoint {
        id: "named-point".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["block-10".to_string()],
        values: [1.0, 2.0],
        raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
        value_source_offsets: [200, 220],
        source_offset: 190,
    };
    let block_use = FeatureSketchNamedPointBlockUse {
        id: "nx:feature-history:sketch-named-point-block-use#1-4-0".to_string(),
        operation_label,
        sketch_reference: "reference".to_string(),
        reference_ordinal: 0,
        named_point: named_point.id.clone(),
        data_block: "block-10".to_string(),
        point_block_ordinal: 0,
        source_offset: 300,
    };
    let mut second_block_use = block_use.clone();
    second_block_use.id = "nx:feature-history:sketch-named-point-block-use#1-4-1".to_string();
    second_block_use.sketch_reference = "reference-2".to_string();
    second_block_use.reference_ordinal = 1;
    second_block_use.source_offset = 301;

    let groups = feature_sketch_point_groups(std::slice::from_ref(&point));
    let uses = feature_sketch_point_uses(
        &groups,
        std::slice::from_ref(&named_point),
        &[second_block_use.clone(), block_use.clone()],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].sketch_point_group, groups[0].id);
    assert_eq!(uses[0].named_point, named_point.id);
    assert_eq!(uses[0].sketch_references, ["reference", "reference-2"]);
    assert_eq!(uses[0].block_uses.len(), 2);
    assert_eq!(uses[0].source_offsets, [300, 301]);

    let mut different = point.clone();
    different.id = "different".to_string();
    different.coordinates[1] = f64::from_bits(2.0_f64.to_bits() + 1);
    let different_groups = feature_sketch_point_groups(std::slice::from_ref(&different));
    assert!(feature_sketch_point_uses(
        &different_groups,
        std::slice::from_ref(&named_point),
        std::slice::from_ref(&block_use),
    )
    .is_empty());
    let mut duplicate = point.clone();
    duplicate.id = "payload-point-2".to_string();
    let duplicate_groups = feature_sketch_point_groups(&[point.clone(), duplicate.clone()]);
    assert_eq!(duplicate_groups[0].points, [point.id.clone(), duplicate.id]);
    let uses = feature_sketch_point_uses(
        &duplicate_groups,
        std::slice::from_ref(&named_point),
        std::slice::from_ref(&block_use),
    );
    assert_eq!(uses[0].sketch_point_group, duplicate_groups[0].id);
    let conflicting_groups = feature_sketch_point_groups(&[point, different]);
    assert!(conflicting_groups.is_empty());
    assert!(
        feature_sketch_point_uses(&conflicting_groups, &[named_point], &[block_use]).is_empty()
    );
}
