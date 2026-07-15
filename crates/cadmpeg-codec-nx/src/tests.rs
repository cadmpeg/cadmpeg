// SPDX-License-Identifier: Apache-2.0
//! Tests over synthetic byte fixtures. No real CAD file exists in this repo and
//! none may be added, so every fixture is a hand-built `.prt` byte image whose
//! bytes exercise the real SPLMSSTR container parse, the Parasolid zlib
//! extraction/classification, and the analytic geometry decode, and fail if the
//! code regresses.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Write};

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
use crate::parasolid::{self, StreamKind};
use crate::NxCodec;

const MAGIC: &[u8; 8] = b"SPLMSSTR";

fn be_f64(v: f64) -> [u8; 8] {
    v.to_be_bytes()
}

fn segment_index_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [7u32, 9, 11, 1, 1, 28] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
    payload
}

fn segment_stream_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&0x8000_0000u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(b"PS\0\0 (deltas) SCH_test segment stream payload with more than sixty-four inflated bytes........")
        .unwrap();
    payload.extend_from_slice(&encoder.finish().unwrap());
    payload
}

fn segment_body_binding_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [7u32, 9, 11, 1, 1, 48, 64, 0, 94, 150, 19, 0] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(64, 0);
    payload.extend_from_slice(&0x8000_0000u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(b"PS\0\0 (partition) SCH_test segment body binding payload with more than sixty-four inflated bytes........")
        .unwrap();
    payload.extend_from_slice(&encoder.finish().unwrap());
    payload
}

fn segment_extended_wrapper_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [7u32, 9, 11, 1, 1, 48, 64, 0, 94, 150, 19, 0] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(64, 0);
    payload.extend_from_slice(&0xc000_0005u32.to_le_bytes());
    payload.resize(64 + 38, 0);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(b"PS\0\0 (partition) SCH_test extended wrapper payload with more than sixty-four inflated bytes........")
        .unwrap();
    payload.extend_from_slice(&encoder.finish().unwrap());
    payload
}

fn segment_om_payload(separated: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    if separated {
        payload.extend_from_slice(&[0xc0, 0xd1, 0xf1, 0xed]);
    }
    payload.extend_from_slice(&size_framed_om_section());
    payload
}

fn segment_om_record_area_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&size_framed_om_section_with_record_area());
    payload
}

fn segment_om_record_area_with_input_store_payload() -> Vec<u8> {
    let mut payload = segment_om_record_area_payload();
    let mut store = offset_only_indexed_om_section();
    let base = payload.len() as u32;
    let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
    for index in 0..4 {
        let at = index_start + index * 4;
        let value = u32::from_le_bytes(store[at..at + 4].try_into().unwrap());
        store[at..at + 4].copy_from_slice(&(value + base).to_le_bytes());
    }
    payload.extend_from_slice(&store);
    payload
}

#[test]
fn nx_expression_parameter_references_preserve_formula_order() {
    assert_eq!(
        crate::native::expression_parameter_names(
            "max(p12, p3) + p12 + exp2 + p7_radius + p7_radius"
        ),
        vec!["p12", "p3", "p12", "p7_radius", "p7_radius"]
    );
}

#[test]
fn nx_expression_graph_evaluates_exact_qualified_dependencies() {
    let expression = |name: &str, formula: &str, value| crate::native::Expression {
        id: format!("nx:test:expression#{name}"),
        object_id: None,
        record: None,
        declaration: None,
        name: name.into(),
        parameter_index: None,
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: formula.into(),
        value,
        source_entry: "part".into(),
        source_offset: 0,
    };
    let mut expressions = vec![
        expression("p7", "3", Some(3.0)),
        expression("p7_radius", "5", Some(5.0)),
        expression("p8", "p7_radius * 2", None),
        expression("p9", "p8 + p7", None),
    ];

    crate::native::evaluate_expression_graphs(&mut expressions);

    assert_eq!(expressions[2].value, Some(10.0));
    assert_eq!(expressions[3].value, Some(13.0));
}

#[test]
fn nx_formula_dependencies_resolve_to_section_parameters() {
    let expression = |key: u32,
                      name: &str,
                      index: u32,
                      qualifier: Option<&str>,
                      text: &str,
                      value: Option<f64>| crate::native::Expression {
        id: format!("nx:test:expression#{key}"),
        object_id: Some(key),
        record: None,
        declaration: None,
        name: name.into(),
        parameter_index: Some(index),
        qualifier: qualifier.map(str::to_string),
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: text.into(),
        value,
        source_entry: "/Root/UG_PART/UG_PART".into(),
        source_offset: u64::from(key),
    };
    let expressions = [
        expression(20, "p2", 2, None, "5", Some(5.0)),
        expression(21, "p2_radius", 2, Some("radius"), "7", Some(7.0)),
        expression(90, "p9", 9, None, "p2_radius * 2 + p2_radius", None),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);

    assert_eq!(ir.model.parameters[2].value, None);
    assert_eq!(
        ir.model.parameters[2].dependencies,
        vec![ir.model.parameters[1].id.clone()]
    );
}

#[test]
fn nx_formula_dependencies_reject_ambiguous_parameter_names() {
    let expression = |key: u32, name: &str, text: &str| crate::native::Expression {
        id: format!("nx:test:expression#{key}"),
        object_id: Some(key),
        record: None,
        declaration: None,
        name: name.into(),
        parameter_index: Some(key),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: text.into(),
        value: None,
        source_entry: "/Root/UG_PART/UG_PART".into(),
        source_offset: u64::from(key),
    };
    let expressions = [
        expression(20, "p2", "5"),
        expression(21, "p2", "7"),
        expression(90, "p9", "p2 * 2"),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);

    assert!(ir.model.parameters[2].dependencies.is_empty());
}

#[test]
fn nx_parameter_uses_group_binding_witnesses_and_project_consumers() {
    use crate::native::{feature_parameter_uses, FeatureParameterBinding};

    let binding = |id: &str, operation: &str, slot: u8, offset: u64| FeatureParameterBinding {
        id: id.to_string(),
        operation_label: operation.to_string(),
        input_slot: slot,
        input_block: format!("block-{slot}"),
        reference_ordinal: 0,
        expression_declaration: "declaration".to_string(),
        expression: Some("nx:test:expression#20".to_string()),
        object_id: 20,
        source_offset: offset,
    };
    let uses = feature_parameter_uses(&[
        binding("late", "nx:feature-history:operation-label#1-2", 1, 30),
        binding("early", "nx:feature-history:operation-label#1-2", 0, 20),
        binding("other", "nx:feature-history:operation-label#1-3", 0, 40),
    ]);
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].bindings, ["early", "late"]);
    assert_eq!(uses[0].source_offsets, [20, 30]);

    let expression = crate::native::Expression {
        id: "nx:test:expression#20".to_string(),
        object_id: Some(20),
        record: None,
        declaration: None,
        name: "p20".to_string(),
        parameter_index: Some(20),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: "5".to_string(),
        value: Some(5.0),
        source_entry: "part".to_string(),
        source_offset: 20,
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::attach_expression_parameters(
        &mut ir,
        &[expression],
        &[],
        &uses,
        &mut annotations,
    );
    assert_eq!(
        ir.model.parameters[0].properties["consumer.0"],
        "nx:feature-history:feature#1-2"
    );
    assert_eq!(
        ir.model.parameters[0].properties["consumer.1"],
        "nx:feature-history:feature#1-3"
    );
}

#[test]
fn nx_feature_source_content_orders_parameter_occurrences_with_text() {
    let text = crate::native::FeaturePayloadString {
        id: "text".into(),
        operation_record: "record".into(),
        ordinal: 0,
        value: "Through".into(),
        source_offset: 30,
    };
    let parameter_use = crate::native::FeatureParameterUse {
        id: "use".into(),
        operation_label: "operation".into(),
        expression: "nx:test:expression#20".into(),
        bindings: vec!["first".into(), "second".into()],
        source_offsets: vec![20, 40],
    };
    let content = crate::decode::feature_source_content(&[&text], &[&parameter_use]);
    assert_eq!(content.len(), 3);
    assert!(matches!(
        &content[0],
        cadmpeg_ir::features::FeatureSourceContent::Parameter(id)
            if id.0 == "nx:test:parameter#20"
    ));
    assert!(matches!(
        &content[1],
        cadmpeg_ir::features::FeatureSourceContent::Text(value) if value == "Through"
    ));
    assert!(matches!(
        &content[2],
        cadmpeg_ir::features::FeatureSourceContent::Parameter(id)
            if id.0 == "nx:test:parameter#20"
    ));
}

#[test]
fn nx_native_feature_parameters_require_unique_resolved_names() {
    let expression = |id: &str, name: &str, text: &str| crate::native::Expression {
        id: id.to_string(),
        object_id: None,
        record: None,
        declaration: None,
        name: name.to_string(),
        parameter_index: None,
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: text.to_string(),
        value: None,
        source_entry: "entry".to_string(),
        source_offset: 0,
    };
    let parameter_use = |id: &str, expression: &str| crate::native::FeatureParameterUse {
        id: id.to_string(),
        operation_label: "operation".to_string(),
        expression: expression.to_string(),
        bindings: vec![format!("binding-{id}")],
        source_offsets: vec![0],
    };
    let expressions = vec![
        expression("expression-a", "p1_length", "p2_length * 2"),
        expression("expression-b", "p2_length", "12.5"),
    ];
    let uses = [
        parameter_use("use-a", "expression-a"),
        parameter_use("use-b", "expression-b"),
    ];
    let use_refs = uses.iter().collect::<Vec<_>>();
    let parameters = crate::decode::native_feature_parameters(&use_refs, &expressions);
    assert_eq!(
        parameters,
        std::collections::BTreeMap::from([
            ("p1_length".to_string(), "p2_length * 2".to_string()),
            ("p2_length".to_string(), "12.5".to_string()),
        ])
    );
    assert_eq!(
        crate::decode::non_boolean_feature_definition_with_parameters(
            "EXTRUDE",
            &[],
            None,
            None,
            None,
            parameters,
        ),
        cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "EXTRUDE".to_string(),
            parameters: std::collections::BTreeMap::from([
                ("p1_length".to_string(), "p2_length * 2".to_string()),
                ("p2_length".to_string(), "12.5".to_string()),
            ]),
            properties: std::collections::BTreeMap::new(),
        }
    );

    let duplicate_expressions = vec![
        expression("expression-a", "p1_length", "1"),
        expression("expression-b", "p1_length", "2"),
    ];
    assert!(crate::decode::native_feature_parameters(&use_refs, &duplicate_expressions).is_empty());
    let unresolved = [parameter_use("use-c", "missing")];
    assert!(crate::decode::native_feature_parameters(
        &unresolved.iter().collect::<Vec<_>>(),
        &expressions,
    )
    .is_empty());
}

#[test]
fn complete_extrude_profile_projects_without_guessing_scalar_roles() {
    use cadmpeg_ir::features::{BooleanOp, Extent, FeatureDefinition, ProfileRef};

    assert_eq!(
        crate::decode::extrude_feature_definition(Some("nx:profile#1"), None, BooleanOp::NewBody,),
        Some(FeatureDefinition::Extrude {
            profile: ProfileRef::Native("nx:profile#1".to_string()),
            direction: None,
            extent: Extent::Unresolved,
            op: BooleanOp::NewBody,
            draft: None,
        })
    );
    assert!(crate::decode::extrude_feature_definition(None, None, BooleanOp::Unresolved).is_none());
    assert!(crate::decode::extrude_feature_definition(
        Some("nx:profile#1"),
        Some("nx:profile#2"),
        BooleanOp::Unresolved,
    )
    .is_none());
}

#[test]
fn extrusion_is_new_body_only_for_one_first_written_solid_output() {
    use cadmpeg_ir::features::BooleanOp;
    use cadmpeg_ir::topology::BodyKind;

    assert_eq!(
        crate::decode::extrude_boolean_op(false, &[BodyKind::Solid]),
        BooleanOp::NewBody
    );
    assert_eq!(
        crate::decode::extrude_boolean_op(true, &[BodyKind::Solid]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        crate::decode::extrude_boolean_op(false, &[BodyKind::Sheet]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        crate::decode::extrude_boolean_op(false, &[BodyKind::Solid, BodyKind::Solid]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        crate::decode::extrude_boolean_op(false, &[]),
        BooleanOp::Unresolved
    );
}

#[test]
fn nx_block_source_content_includes_complete_ordered_dimension_run() {
    use cadmpeg_ir::features::{FeatureSourceContent, ParameterId};

    let mut content = vec![FeatureSourceContent::Parameter(ParameterId(
        "nx:test:parameter#20".into(),
    ))];
    crate::decode::append_feature_expression_content(
        &mut content,
        &[
            "nx:test:expression#20".into(),
            "nx:test:expression#21".into(),
            "nx:test:expression#22".into(),
        ],
    );
    assert_eq!(
        content,
        [
            FeatureSourceContent::Parameter(ParameterId("nx:test:parameter#20".into())),
            FeatureSourceContent::Parameter(ParameterId("nx:test:parameter#21".into())),
            FeatureSourceContent::Parameter(ParameterId("nx:test:parameter#22".into())),
        ]
    );
}

#[test]
fn nx_block_dimension_parameters_name_the_block_as_consumer() {
    let expression = |key: u32| crate::native::Expression {
        id: format!("nx:test:expression#{key}"),
        object_id: Some(key),
        record: None,
        declaration: None,
        name: format!("p{key}"),
        parameter_index: Some(key),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: key.to_string(),
        value: Some(f64::from(key)),
        source_entry: "part".into(),
        source_offset: u64::from(key),
    };
    let expressions = [expression(20), expression(21), expression(22)];
    let dimensions = crate::native::FeatureBlockDimensions {
        id: "dimensions".into(),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        construction: "construction".into(),
        anchor_bindings: vec!["binding".into()],
        declarations: ["d20".into(), "d21".into(), "d22".into()],
        expressions: [
            expressions[0].id.clone(),
            expressions[1].id.clone(),
            expressions[2].id.clone(),
        ],
        values: [20.0, 21.0, 22.0],
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);
    crate::decode::attach_block_dimension_parameter_consumers(
        &mut ir,
        &[dimensions],
        &mut annotations,
    );
    for (ordinal, parameter) in ir.model.parameters.iter().enumerate() {
        assert_eq!(
            parameter.properties[&format!("block_dimension.{ordinal}")],
            "dimensions"
        );
        assert_eq!(
            parameter.properties["consumer.0"],
            "nx:feature-history:feature#1-4"
        );
    }
}

/// Write three big-endian doubles into `rec` starting at `at`.
fn put_vec3(rec: &mut [u8], at: usize, xyz: [f64; 3]) {
    for (i, v) in xyz.iter().enumerate() {
        rec[at + 8 * i..at + 8 * i + 8].copy_from_slice(&be_f64(*v));
    }
}

fn put_f64(rec: &mut [u8], at: usize, v: f64) {
    rec[at..at + 8].copy_from_slice(&be_f64(v));
}

fn put_ref(rec: &mut [u8], at: usize, value: u16) {
    rec[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn encoded_xmt(value: u32) -> Vec<u8> {
    if i16::try_from(value).is_ok() {
        return (value as u16).to_be_bytes().to_vec();
    }
    let quotient = value / 32_767;
    let remainder = value % 32_767;
    assert!(remainder > 0 && i16::try_from(remainder).is_ok());
    let mut out = (-(remainder as i16)).to_be_bytes().to_vec();
    out.extend_from_slice(&(quotient as u16).to_be_bytes());
    out
}

/// One fixed-length analytic record: a `00 <tag>` header then zeroed payload the
/// caller fills at the documented offsets.
fn record(tag: u8, len: usize) -> Vec<u8> {
    let mut r = vec![0u8; len];
    r[0] = 0x00;
    r[1] = tag;
    r
}

fn indexed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xaa; 32];
    let base = 8usize;
    let class_name = b"UGS::EXP_expression";
    bytes[base] = (class_name.len() + 1) as u8;
    bytes[base + 1..base + 1 + class_name.len()].copy_from_slice(class_name);
    bytes[base + 1 + class_name.len()] = 0x81;
    let field_name = b"m_target";
    bytes.push((field_name.len() + 1) as u8);
    bytes.extend_from_slice(field_name);
    bytes.push(0x80);
    let root = b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables";
    let text = b"(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; ";
    let declaration_name = b"p8_CircularPattern_pattern_Circular_Dir_offset_angle";
    let mut expression = vec![0x04, (declaration_name.len() + 2) as u8];
    expression.extend_from_slice(declaration_name);
    expression.push(0);
    expression.extend_from_slice(b"\x04\x05120\0");
    expression.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    expression.extend_from_slice(text);
    expression.push(0);
    expression.extend_from_slice(b"\x66\x32\x03\x0cSKETCH_001\0");
    expression.extend_from_slice(b"\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0");
    expression.extend_from_slice(b"\x01\x02\x90\x00\x00");
    let records = [root.as_slice(), expression.as_slice()];
    let table = bytes.len() + 4 * 4;
    let table_end = table + 4 + 3 * 4;
    let first = table_end - base;
    let second = first + records[0].len();
    let end = second + records[1].len();
    for value in [0u32, first as u32, second as u32, end as u32] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for id in [0x100u32, 0x101, 0x102] {
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    bytes.extend_from_slice(records[0]);
    bytes.extend_from_slice(records[1]);
    bytes
}

fn offset_only_indexed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let metadata = bytes.len();
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    let first = bytes.len();
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0hostglobalvariables");
    let second = bytes.len();
    let text = b"(Number [mm]) length: 25; ";
    bytes.extend_from_slice(&[0x04, 0x00, 0x2a, 0x02, 0x0b]);
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);
    let end = bytes.len();
    for (index, offset) in [metadata, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

fn size_framed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xff; 16];
    bytes[4..8].fill(0);
    bytes[12..14].copy_from_slice(b"OM");
    bytes.extend_from_slice(&[0, 1, 2]);
    for (index, (name, code)) in [
        (b"UGS::FEATURE_RECORD".as_slice(), 0xa0),
        (b"UGS::ModlUtils::BooleanComponent".as_slice(), 0x65),
    ]
    .into_iter()
    .enumerate()
    {
        bytes.push((name.len() + 1) as u8);
        bytes.extend_from_slice(name);
        bytes.push(code);
        if index == 0 {
            bytes.extend_from_slice(&[
                0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
            ]);
        }
    }
    for (name, code, suffix) in [
        (b"m_target".as_slice(), 0x80, [0x01, 0x02]),
        (b"m_tools".as_slice(), 0x81, [0x03, 0x04]),
    ] {
        bytes.push((name.len() + 1) as u8);
        bytes.extend_from_slice(name);
        bytes.push(code);
        bytes.extend_from_slice(&suffix);
    }
    bytes.extend_from_slice(b"unframed UGS::PayloadText");
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

fn size_framed_om_section_with_record_area() -> Vec<u8> {
    let mut bytes = size_framed_om_section();
    let record_area = bytes.len() + 20;
    bytes.extend_from_slice(&(record_area as u32).to_le_bytes());
    bytes.resize(record_area, 0);
    bytes.extend_from_slice(&13u32.to_le_bytes());
    bytes.extend_from_slice(&14u32.to_le_bytes());
    bytes.extend_from_slice(&44u32.to_le_bytes());
    bytes.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0feature-records\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x01\x82\x40\x90\x17\xd3\xff\x03\x07UNITE\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x42\x00\x01\x03\x90\x19\x4c\x7f\x00\x01\x02\x10\x90\x19\x42\xff");
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

#[test]
fn om_index_pairs_object_ids_with_bounded_entity_records() {
    let bytes = indexed_om_section();
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 8);
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(sections[0].records[0].object_id, Some(0x101));
    assert_eq!(
        sections[0].records[0].bytes,
        b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables"
    );
    assert_eq!(sections[0].records[1].object_id, Some(0x102));
    assert_eq!(sections[0].column_storage, None);
    assert_eq!(sections[0].fields.len(), 1);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(
        sections[0].records[1].bytes,
        b"\x04\x36p8_CircularPattern_pattern_Circular_Dir_offset_angle\x00\x04\x05120\x00\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00\x66\x32\x03\x0cSKETCH_001\0\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\x01\x02\x90\x00\x00"
    );
}

#[test]
fn ug_part_segment_index_uses_row_one_self_boundary() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let container = container::scan_bytes(file).unwrap();
    let (_, index) = container.segment_index().expect("segment index");
    assert_eq!(index.byte_len, 28);
    assert_eq!(index.rows.len(), 2);
    assert_eq!(index.rows[0].type_code, 7);
    assert_eq!(index.rows[0].subtype_code, 9);
    assert_eq!(index.rows[0].value, 11);
    assert_eq!(index.rows[1].type_code, 1);
    assert_eq!(index.rows[1].subtype_code, 1);
    assert_eq!(index.rows[1].value, 28);
    assert_eq!(index.padding, &[0xaa, 0xbb, 0xcc, 0xdd]);
}

#[test]
fn decode_retains_ordered_ug_part_segment_index_rows() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let namespace = result.ir.native.namespace("nx").expect("NX namespace");
    assert_eq!(namespace.version, 145);
    let rows = namespace
        .arena_as::<crate::native::SegmentIndexRow>("segment_index_rows")
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].ordinal, 0);
    assert_eq!(rows[1].value, 28);
    assert_eq!(rows[1].source_entry, "/Root/UG_PART/UG_PART");
    assert_eq!(rows[1].source_offset, rows[0].source_offset + 12);
}

#[test]
fn decode_links_segment_index_word_to_validated_stream_wrapper() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let links = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::SegmentStreamLink>("segment_stream_links")
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].row, "nx:segment-index:row#0");
    assert_eq!(links[0].slot, crate::native::SegmentIndexSlot::TypeCode);
    assert_eq!(links[0].stream_ordinal, 0);
    assert_eq!(links[0].stream_kind, "deltas");
    assert_eq!(links[0].wrapper_byte_len, 8);
}

#[test]
fn decode_binds_segment_body_object_index_to_partition_stream() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_body_binding_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let bindings = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::SegmentBodyBinding>("segment_body_bindings")
        .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].stream_ordinal, 0);
    assert_eq!(bindings[0].stream_kind, "partition");
    assert_eq!(bindings[0].body_object_index, 94);
    assert_eq!(bindings[0].body_alias_object_index, 150);
    assert_eq!(bindings[0].stream_role, 19);
    assert_eq!(bindings[0].source_offset, 104);
}

#[test]
fn decode_links_extended_partition_wrapper_and_body_identity() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_extended_wrapper_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let namespace = result.ir.native.namespace("nx").unwrap();
    let links = namespace
        .arena_as::<crate::native::SegmentStreamLink>("segment_stream_links")
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].wrapper_byte_len, 38);
    let bindings = namespace
        .arena_as::<crate::native::SegmentBodyBinding>("segment_body_bindings")
        .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].body_object_index, 94);
    assert_eq!(bindings[0].body_alias_object_index, 150);
    assert_eq!(bindings[0].stream_role, 19);
}

#[test]
fn feature_body_selection_resolves_complete_segment_bindings_atomically() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let first = BodyId("nx:s2:body#3".to_string());
    let second = BodyId("nx:s4:body#3".to_string());
    let bindings = BTreeMap::from([(94, vec![first.clone()]), (122, vec![second.clone()])]);
    assert_eq!(
        crate::decode::feature_body_selection(
            &[94, 122],
            &bindings,
            "nx:om-object-indices#94,122".to_string(),
        ),
        BodySelection::Resolved {
            bodies: vec![first, second],
            native: "nx:om-object-indices#94,122".to_string(),
        }
    );
    assert!(matches!(
        crate::decode::feature_body_selection(
            &[94, 123],
            &bindings,
            "nx:om-object-indices#94,123".to_string(),
        ),
        BodySelection::Native(_)
    ));
    assert_eq!(
        crate::decode::feature_body_outputs(94, &bindings),
        vec![BodyId("nx:s2:body#3".to_string())]
    );
    assert!(crate::decode::feature_body_outputs(123, &bindings).is_empty());
}

#[test]
fn nx_sew_projects_ordered_body_operands_without_inventing_tolerance() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operand = |ordinal, object_index| crate::native::FeatureOperationBodyOperand {
        id: format!("operand#{ordinal}"),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        body_reference_ordinal: 0,
        ordinal,
        operand_object_index: object_index,
        segment_body_bindings: vec![format!("binding#{ordinal}")],
        source_offset: u64::from(ordinal),
    };
    let operands = [operand(0, 20), operand(1, 30)];
    let references = operands.iter().collect::<Vec<_>>();
    let first = BodyId("body#20".to_string());
    let second = BodyId("body#30".to_string());
    let bodies = BTreeMap::from([(20, vec![first.clone()]), (30, vec![second.clone()])]);

    assert_eq!(
        crate::decode::sew_body_feature_definition(&references, &bodies),
        Some(FeatureDefinition::SewBodies {
            bodies: BodySelection::Resolved {
                bodies: vec![first, second],
                native: "nx:om-object-indices#20,30".to_string(),
            },
            gap_tolerance: None,
        })
    );
    assert_eq!(
        crate::decode::sew_body_feature_definition(&[], &bodies),
        None
    );
}

#[test]
fn nx_trim_body_projects_distinct_target_and_ordered_tools() {
    use cadmpeg_ir::features::{BodySelection, BodyTrimSide, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operands = [crate::native::FeatureOperationBodyOperand {
        id: "operand#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        body_reference_ordinal: 0,
        ordinal: 0,
        operand_object_index: 20,
        segment_body_bindings: vec!["binding#0".to_string()],
        source_offset: 0,
    }];
    let references = operands.iter().collect::<Vec<_>>();
    let target = BodyId("body#10".to_string());
    let tool = BodyId("body#20".to_string());
    let bodies = BTreeMap::from([(10, vec![target.clone()]), (20, vec![tool.clone()])]);

    assert_eq!(
        crate::decode::trim_body_feature_definition(10, &references, &bodies),
        Some(FeatureDefinition::TrimBodies {
            targets: BodySelection::Resolved {
                bodies: vec![target],
                native: "nx:om-object-index#10".to_string(),
            },
            tools: BodySelection::Resolved {
                bodies: vec![tool],
                native: "nx:om-object-indices#20".to_string(),
            },
            keep: BodyTrimSide::Unresolved,
        })
    );
    assert_eq!(
        crate::decode::trim_body_feature_definition(10, &[], &bodies),
        None
    );
}

#[test]
fn nx_sketch_operation_projects_as_an_ordered_planar_sketch_node() {
    assert!(matches!(
        crate::decode::non_boolean_feature_definition("SKETCH", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: None,
        }
    ));
    assert!(matches!(
        crate::decode::non_boolean_feature_definition(
            "SIMPLE HOLE",
            &["Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer"],
            None,
            None,
            None,
        ),
        cadmpeg_ir::features::FeatureDefinition::Hole {
            face: None,
            position: None,
            direction: None,
            kind: cadmpeg_ir::features::HoleKind::Unresolved {
                form: Some(cadmpeg_ir::features::HoleForm::Chamfer),
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            },
            exit_kind: Some(cadmpeg_ir::features::HoleKind::Unresolved {
                form: Some(cadmpeg_ir::features::HoleForm::Chamfer),
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            }),
            diameter: None,
            extent: Some(cadmpeg_ir::features::Extent::ThroughAll),
        }
    ));
    assert!(matches!(
        crate::decode::non_boolean_feature_definition(
            "SIMPLE HOLE",
            &["unrelated"],
            None,
            None,
            None,
        ),
        cadmpeg_ir::features::FeatureDefinition::Hole { extent: None, .. }
    ));
    assert!(matches!(
        crate::decode::non_boolean_feature_definition("DATUM_PLANE", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::Native { kind, .. }
            if kind == "DATUM_PLANE"
    ));
    assert!(matches!(
        crate::decode::non_boolean_feature_definition(
            "BLOCK",
            &[],
            Some([10.0, 20.0, 30.0]),
            None,
            None,
        ),
        cadmpeg_ir::features::FeatureDefinition::Block {
            dimensions: [
                cadmpeg_ir::features::Length(10.0),
                cadmpeg_ir::features::Length(20.0),
                cadmpeg_ir::features::Length(30.0),
            ],
            placement: None,
        }
    ));
}

#[test]
fn nx_block_placement_requires_one_ordered_planar_extent_bijection() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let dimensions = [10.0, 20.0, 30.0];
    for axis in 0..3 {
        let mut surfaces = ir
            .model
            .surfaces
            .iter_mut()
            .filter_map(|surface| {
                let SurfaceGeometry::Plane { origin, normal, .. } = &mut surface.geometry else {
                    return None;
                };
                let components = [normal.x.abs(), normal.y.abs(), normal.z.abs()];
                (components[axis] > 0.5).then_some(origin)
            })
            .collect::<Vec<_>>();
        assert_eq!(surfaces.len(), 2);
        surfaces.sort_by(|first, second| {
            [first.x, first.y, first.z][axis].total_cmp(&[second.x, second.y, second.z][axis])
        });
        match axis {
            0 => {
                surfaces[0].x = 0.0;
                surfaces[1].x = dimensions[axis];
            }
            1 => {
                surfaces[0].y = 0.0;
                surfaces[1].y = dimensions[axis];
            }
            2 => {
                surfaces[0].z = 0.0;
                surfaces[1].z = dimensions[axis];
            }
            _ => unreachable!(),
        }
    }

    assert_eq!(
        crate::decode::block_placement(&ir, dimensions),
        Some(cadmpeg_ir::transform::Transform::identity())
    );
    assert_eq!(
        crate::decode::block_placement(&ir, [10.0, 10.0, 30.0]),
        None
    );
}

#[test]
fn nx_simple_hole_template_requires_exact_ordered_tokens() {
    use crate::native::{
        FeatureOperationLabel, FeatureOperationRecord, FeaturePayloadString,
        SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
    };

    let label = FeatureOperationLabel {
        id: "operation#3".to_string(),
        section_link: "section#0".to_string(),
        ordinal: 3,
        value: "SIMPLE HOLE".to_string(),
        object_indices: [None; 4],
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
    let templates = crate::native::feature_simple_hole_templates(
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

    let mut malformed = string;
    malformed.value = "Hole_GeneralHole_Simple_Through_EndChamfer_StartChamfer".to_string();
    assert!(
        crate::native::feature_simple_hole_templates(&[label], &[record], &[malformed]).is_empty()
    );
}

#[test]
fn nx_simple_hole_feature_owns_its_exact_native_constructions() {
    use crate::native::{
        FeatureSimpleHoleConstructionGroup, FeatureSimpleHoleRepeatedScalarLane,
        FeatureSimpleHoleRepeatedScalarLaneBlockReferences, FeatureSimpleHoleTemplate,
        SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
    };
    let operation = "nx:feature-history:operation-label#1-4";
    let template = FeatureSimpleHoleTemplate {
        id: "template".to_string(),
        operation_label: operation.to_string(),
        payload_string: "string".to_string(),
        family: SimpleHoleFamily::GeneralHole,
        form: SimpleHoleForm::Simple,
        extent: SimpleHoleExtent::Through,
        start_treatment: SimpleHoleEndTreatment::Chamfer,
        end_treatment: SimpleHoleEndTreatment::Chamfer,
    };
    let lane = FeatureSimpleHoleRepeatedScalarLane {
        id: "lane".to_string(),
        operation_label: operation.to_string(),
        values: vec![508.0, 38.1],
        first_witness_offsets: vec![10, 18],
        second_witness_offsets: vec![30, 38],
    };
    let blocks = FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
        id: "blocks".to_string(),
        operation_label: operation.to_string(),
        first_data_blocks: ["block#231".to_string(), "block#232".to_string()],
        second_data_blocks: ["block#233".to_string(), "block#234".to_string()],
        first_reference_offsets: [20, 22],
        second_reference_offsets: [40, 42],
    };
    let group = FeatureSimpleHoleConstructionGroup {
        id: "group".into(),
        first_data_blocks: blocks.first_data_blocks.clone(),
        second_data_blocks: blocks.second_data_blocks.clone(),
        operation_labels: vec![operation.into(), "other-operation".into()],
        scalar_lanes: vec!["lane".into(), "other-lane".into()],
        block_references: vec!["blocks".into(), "other-blocks".into()],
    };
    let properties = crate::decode::simple_hole_native_properties(
        operation,
        &[template],
        &[lane],
        &[blocks],
        &[group],
    );
    assert_eq!(properties["simple_hole_template"], "template");
    assert_eq!(properties["simple_hole_repeated_scalar_lane"], "lane");
    assert_eq!(
        properties["simple_hole_repeated_scalar_lane_block_references"],
        "blocks"
    );
    assert_eq!(properties["simple_hole_construction_group"], "group");
    assert!(crate::decode::simple_hole_native_properties(
        "nx:feature-history:operation-label#1-5",
        &[],
        &[],
        &[],
        &[],
    )
    .is_empty());
}

#[test]
fn nx_simple_hole_diameter_requires_a_complete_uniform_through_bore_bijection() {
    use crate::native::{
        FeatureSimpleHoleConstructionGroup, FeatureSimpleHoleTemplate, SimpleHoleEndTreatment,
        SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
    };
    use cadmpeg_ir::document::{CadIr, Model, IR_VERSION};
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::{FaceId, LoopId, ShellId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::native::Native;
    use cadmpeg_ir::topology::{Face, Sense};
    use cadmpeg_ir::units::{Tolerances, Units};
    use cadmpeg_ir::{Annotations, SourceObjectAssociation};

    let operations = ["hole-a".to_string(), "hole-b".to_string()];
    let templates = operations
        .iter()
        .map(|operation| FeatureSimpleHoleTemplate {
            id: format!("template-{operation}"),
            operation_label: operation.clone(),
            payload_string: format!("string-{operation}"),
            family: SimpleHoleFamily::GeneralHole,
            form: SimpleHoleForm::Simple,
            extent: SimpleHoleExtent::Through,
            start_treatment: SimpleHoleEndTreatment::Chamfer,
            end_treatment: SimpleHoleEndTreatment::Chamfer,
        })
        .collect::<Vec<_>>();
    let group = FeatureSimpleHoleConstructionGroup {
        id: "group".into(),
        first_data_blocks: ["a".into(), "b".into()],
        second_data_blocks: ["c".into(), "d".into()],
        operation_labels: operations.to_vec(),
        scalar_lanes: vec!["lane-a".into(), "lane-b".into()],
        block_references: vec!["refs-a".into(), "refs-b".into()],
    };
    let mut model = Model::default();
    for ordinal in 0..2 {
        let surface = SurfaceId(format!("surface-{ordinal}"));
        model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(ordinal as f64, 0.0, 0.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.55,
            },
            source_object: None::<SourceObjectAssociation>,
        });
        model.faces.push(Face {
            id: FaceId(format!("face-{ordinal}")),
            shell: ShellId("shell".into()),
            surface,
            sense: Sense::Reversed,
            loops: vec![
                LoopId(format!("loop-{ordinal}-0")),
                LoopId(format!("loop-{ordinal}-1")),
            ],
            name: None,
            color: None,
            tolerance: None,
        });
    }
    let ir = CadIr {
        ir_version: IR_VERSION.into(),
        source: None,
        units: Units::default(),
        tolerances: Tolerances::default(),
        model,
        annotations: Annotations::default(),
        native: Native::default(),
    };
    assert_eq!(
        crate::decode::simple_hole_diameters(&ir, &templates, std::slice::from_ref(&group)),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
        ])
    );
    assert_eq!(
        crate::decode::simple_hole_diameters(&ir, &templates, &[]),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
        ])
    );

    let mut mismatched = ir;
    let SurfaceGeometry::Cylinder { radius, .. } = &mut mismatched.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    *radius = 3.0;
    assert!(crate::decode::simple_hole_diameters(&mismatched, &templates, &[group]).is_empty());
}

#[test]
fn nx_offset_feature_requires_one_output_image_and_one_exact_distance() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let output = BodyId("nx:s4:body#3".into());
    let make_offset = |ordinal: u32, distance: f64| ProceduralSurface {
        id: ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
        surface: SurfaceId(format!("nx:s4:offset-surf#{ordinal}")),
        definition: ProceduralSurfaceDefinition::Offset {
            support: SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
            distance,
            u_sense: 1,
            v_sense: 1,
            extension_flags: Vec::new(),
        },
        cache_fit_tolerance: None,
    };
    ir.model.procedural_surfaces.push(make_offset(0, 30.0));
    ir.model.procedural_surfaces.push(make_offset(1, 30.0));

    let (definition, supports) =
        crate::decode::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("unique offset distance");
    assert_eq!(supports.len(), 2);
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            distance: cadmpeg_ir::features::Length(30.0),
            ..
        }
    ));

    ir.model.procedural_surfaces.push(make_offset(2, -30.0));
    assert!(crate::decode::offset_surface_feature_definition(&ir, &[output]).is_none());
}

#[test]
fn nx_blend_feature_requires_one_output_image_and_circular_result_carriers() {
    use cadmpeg_ir::features::{FeatureDefinition, RadiusForm, RadiusSpec};
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, ProceduralSurface, ProceduralSurfaceDefinition,
    };
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let output = BodyId("nx:s4:body#3".into());
    let make_blend = |ordinal: u32, radius: BlendRadiusLaw| ProceduralSurface {
        id: ProceduralSurfaceId(format!("nx:s4:blend-construction#{ordinal}")),
        surface: SurfaceId(format!("nx:s4:blend-surf#{ordinal}")),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [None, None],
            spine: None,
            radius,
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
    };
    ir.model.procedural_surfaces.push(make_blend(
        0,
        BlendRadiusLaw::Constant { signed_radius: 5.0 },
    ));
    ir.model.procedural_surfaces.push(make_blend(
        1,
        BlendRadiusLaw::Constant {
            signed_radius: -5.0,
        },
    ));

    let (definition, surfaces) =
        crate::decode::blend_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("one circular constant-radius blend result");
    assert_eq!(surfaces.len(), 2);
    assert!(matches!(
        definition,
        FeatureDefinition::Fillet {
            radius: RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(5.0)
            },
            ..
        }
    ));

    ir.model.procedural_surfaces.push(make_blend(
        2,
        BlendRadiusLaw::Constant { signed_radius: 7.0 },
    ));
    let (definition, _) = crate::decode::blend_feature_definition(&ir, &[output]).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Fillet {
            radius: RadiusSpec::Unresolved {
                form: Some(RadiusForm::Constant)
            },
            ..
        }
    ));
    assert!(crate::decode::blend_feature_definition(&ir, &[]).is_none());

    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("nx:s4:blend-construction#3".into()),
        surface: SurfaceId("nx:s4:blend-surf#3".into()),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [None, None],
            spine: None,
            radius: BlendRadiusLaw::Constant { signed_radius: 7.0 },
            cross_section: BlendCrossSection::Conic,
            native: None,
        },
        cache_fit_tolerance: None,
    });
    assert!(
        crate::decode::blend_feature_definition(&ir, &[BodyId("nx:s4:body#3".into())]).is_none()
    );
}

#[test]
fn nx_sketch_record_joins_exact_operation_and_ordered_input_lanes() {
    use crate::native::{
        FeatureInputBlock, FeatureOperationLabel, FeatureOperationRecord, FeatureSketchReference,
    };

    let label = FeatureOperationLabel {
        id: "nx:feature-history:operation-label#0-7".to_string(),
        section_link: "nx:feature-history#0".to_string(),
        ordinal: 7,
        value: "SKETCH".to_string(),
        object_indices: [Some(45), None, Some(81), None],
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
        payload_source_offset: 733,
        source_offset: 700,
    };
    let input = |slot, index| FeatureInputBlock {
        id: format!("nx:feature-history:input-block#0-7-{slot}"),
        operation_label: label.id.clone(),
        input_slot: slot,
        object_index: index,
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
        data_block: Some(format!("nx:om-data-blocks-2:block#{index}")),
        source_offset: 740 + u64::from(ordinal),
    };
    let references = [reference(1, 97), reference(0, 96)];

    let sketches = crate::native::feature_sketch_records(&[label], &[record], &inputs, &references);
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
    let construction = crate::native::feature_sketch_construction_inputs(&sketches, &references);
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
    assert!(crate::native::feature_sketch_construction_inputs(&sketches, &malformed).is_empty());
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
    let joined = crate::native::join_data_block_bytes(&ids, &blocks).unwrap();
    assert_eq!(joined.0, [0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72]);
    assert_eq!(joined.1, [0, 2]);
    assert_eq!(joined.2, [2, 6]);
    assert_eq!(joined.3, [120, 900]);

    let missing = vec!["block#2".to_string(), "missing".to_string()];
    assert!(crate::native::join_data_block_bytes(&missing, &blocks).is_none());
}

#[test]
fn nx_offset_store_block_bytes_follow_catalog_identity() {
    let control = crate::om::EntityRecord {
        object_id: None,
        offset: 5,
        bytes: &[0xaa],
    };
    let first = crate::om::EntityRecord {
        object_id: None,
        offset: 6,
        bytes: &[0xbb],
    };
    let second = crate::om::EntityRecord {
        object_id: None,
        offset: 7,
        bytes: &[0xcc],
    };
    let controlled = crate::native::offset_data_block_bytes_for_section(
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

    let control_free =
        crate::native::offset_data_block_bytes_for_section(4, 200, None, &[first, second]);
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
fn nx_feature_parameter_binding_joins_only_resolved_input_references() {
    use crate::native::{DataBlockReference, FeatureInputBlock};

    let input = FeatureInputBlock {
        id: "nx:feature-history:input-block#0-7-0".to_string(),
        operation_label: "nx:feature-history:operation-label#0-7".to_string(),
        input_slot: 0,
        object_index: 45,
        data_block: "nx:om-data-blocks-2:block#45".to_string(),
        source_offset: 700,
    };
    let reference = |ordinal: u32, declaration: Option<&str>| DataBlockReference {
        id: format!("nx:om-data-block-references-2-45:reference#{ordinal}"),
        data_block: input.data_block.clone(),
        ordinal,
        object_id: 201 + ordinal,
        target_record: Some(format!("nx:om-record-directory-0:entry#{ordinal}")),
        target_expression_declaration: declaration.map(str::to_string),
        source_offset: 800 + u64::from(ordinal),
    };
    let references = [
        reference(0, Some("nx:om-expression-declarations-0:declaration#3")),
        reference(1, None),
    ];

    let expression = crate::native::Expression {
        id: "nx:om-entry-9:expression#3".to_string(),
        object_id: Some(201),
        record: None,
        declaration: Some("nx:om-expression-declarations-0:declaration#3".to_string()),
        name: "p3".to_string(),
        parameter_index: Some(3),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: "12".to_string(),
        value: Some(12.0),
        source_entry: "/Root/UG_PART/UG_PART".to_string(),
        source_offset: 900,
    };
    let bindings = crate::native::feature_parameter_bindings(
        std::slice::from_ref(&input),
        &references,
        std::slice::from_ref(&expression),
    );
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].id,
        "nx:feature-history:parameter-binding#0-7-0-0"
    );
    assert_eq!(bindings[0].input_slot, 0);
    assert_eq!(bindings[0].reference_ordinal, 0);
    assert_eq!(bindings[0].object_id, 201);
    assert_eq!(
        bindings[0].expression_declaration,
        "nx:om-expression-declarations-0:declaration#3"
    );
    assert_eq!(
        bindings[0].expression.as_deref(),
        Some("nx:om-entry-9:expression#3")
    );

    let mut duplicate = expression.clone();
    duplicate.id = "nx:om-entry-9:expression#30".to_string();
    let ambiguous =
        crate::native::feature_parameter_bindings(&[input], &references, &[expression, duplicate]);
    assert_eq!(ambiguous.len(), 1);
    assert_eq!(ambiguous[0].expression, None);
}

#[test]
fn segment_order_pairs_delta_across_intervening_non_history_stream() {
    use crate::parasolid::{Stream, StreamKind};
    use std::collections::BTreeSet;

    let stream = |kind, schema: Option<&str>, file_offset| Stream {
        file_offset,
        inflated: Vec::new(),
        kind,
        schema: schema.map(str::to_string),
    };
    let streams = vec![
        stream(StreamKind::Partition, Some("SCH_A"), 10),
        stream(StreamKind::Preview, None, 20),
        stream(StreamKind::Deltas, Some("SCH_A"), 30),
        stream(StreamKind::Partition, Some("SCH_B"), 40),
        stream(StreamKind::Deltas, Some("SCH_A"), 50),
        stream(StreamKind::Deltas, Some("SCH_B"), 60),
    ];
    let eligible = BTreeSet::from([2usize, 5]);
    assert_eq!(
        crate::decode::pair_stream_indices(&streams, Some(&eligible)),
        std::collections::BTreeMap::from([(0, vec![2]), (3, vec![5])])
    );
}

#[test]
fn decode_links_segment_index_words_to_direct_and_separated_om_sections() {
    for (separated, expected_separator) in [(false, 0), (true, 4)] {
        let file =
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_payload(separated))]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .unwrap();
        let links = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<crate::native::SegmentOmLink>("segment_om_links")
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].row, "nx:segment-index:row#0");
        assert_eq!(links[0].slot, crate::native::SegmentIndexSlot::TypeCode);
        assert_eq!(
            links[0].schema_role,
            crate::native::OmSchemaRole::FeatureHistory
        );
        assert_eq!(links[0].separator_byte_len, expected_separator);
        assert_eq!(
            links[0].section_offset,
            links[0].source_offset + u64::from(expected_separator)
        );
    }
}

#[test]
fn decode_retains_role_scoped_om_record_area_header() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_record_area_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let areas = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::OmRecordArea>("om_record_areas")
        .unwrap();
    assert_eq!(areas.len(), 1);
    assert_eq!(
        areas[0].schema_role,
        crate::native::OmSchemaRole::FeatureHistory
    );
    assert_eq!(areas[0].control_words, [13, 14, 44]);
    assert_eq!(areas[0].product_version, "NX 2027.3102");
    assert!(areas[0].byte_len > 12);
    assert_eq!(areas[0].sha256.len(), 64);
    let labels = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureOperationLabel>("feature_operation_labels")
        .unwrap();
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].ordinal, 0);
    assert_eq!(labels[0].value, "UNITE");
    assert_eq!(
        labels[0].object_indices,
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[0].section_link, areas[0].section_link);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureOperationRecord>("feature_operation_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation_label, labels[0].id);
    assert!(records[0].byte_len > 40);
    assert_eq!(records[0].sha256.len(), 64);
    let booleans = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureBooleanOperation>("feature_boolean_operations")
        .unwrap();
    assert_eq!(booleans.len(), 1);
    assert_eq!(booleans[0].kind, crate::native::FeatureBooleanKind::Unite);
    assert_eq!(booleans[0].target_object_index, 6466);
    assert_eq!(booleans[0].tool_object_indices, [6476, 127]);
    let body_references = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureBodyReference>("feature_body_references")
        .unwrap();
    assert_eq!(body_references.len(), 1);
    assert_eq!(body_references[0].operation_label, labels[0].id);
    assert_eq!(body_references[0].body_object_index, 6466);
    let body_reference_occurrences = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureBodyReferenceOccurrence>(
            "feature_body_reference_occurrences",
        )
        .unwrap();
    assert_eq!(body_reference_occurrences.len(), 1);
    assert_eq!(body_reference_occurrences[0].operation_label, labels[0].id);
    assert_eq!(body_reference_occurrences[0].ordinal, 0);
    assert_eq!(body_reference_occurrences[0].body_object_index, 6466);
    let feature = result.ir.model.features.first().expect("neutral feature");
    assert_eq!(feature.name.as_deref(), Some("UNITE"));
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
        } if target == "nx:om-object-index#6466" && tools == "nx:om-object-indices#6476,127"
    ));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_feature_header_input_to_unique_data_block() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_input_store_payload(),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let inputs = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureInputBlock>("feature_input_blocks")
        .unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].input_slot, 0);
    assert_eq!(inputs[0].object_index, 1);
    assert!(inputs[0].data_block.ends_with(":block#1"));
    assert_eq!(
        result.ir.model.features[0].source_properties["input_block.0"],
        inputs[0].data_block
    );
    let references = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::DataBlockReference>("data_block_references")
        .unwrap();
    assert_eq!(references.len(), 1);
    assert!(references[0].data_block.ends_with(":block#2"));
    assert_ne!(references[0].data_block, inputs[0].data_block);
    assert_eq!(references[0].object_id, 42);
    assert_eq!(references[0].target_record, None);
}

#[test]
fn feature_input_identity_groups_require_distinct_operations_and_preserve_order() {
    use crate::native::{feature_input_block_identity_groups, FeatureInputBlock};

    let input = |id: &str, operation: &str, slot: u8, block: &str, offset: u64| FeatureInputBlock {
        id: id.to_string(),
        operation_label: operation.to_string(),
        input_slot: slot,
        object_index: 7,
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
fn om_compact_index_lane_decodes_direct_extended_and_null_entries() {
    use crate::om::CompactIndex::{Null, Value};

    assert_eq!(
        crate::om::compact_indices(&[0x00, 0x7f, 0x80, 0x80, 0x81, 0x00, 0xfe, 0xff, 0xff]),
        Some(vec![
            Value(0),
            Value(127),
            Value(128),
            Value(256),
            Value(32_511),
            Null,
        ])
    );
    assert_eq!(crate::om::compact_indices(&[0x80]), None);
}

#[test]
fn om_data_block_object_frame_requires_complete_discriminator() {
    let discriminator = [
        0x00, 0x72, 0x01, 0xc0, 0x20, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x01,
        0x02, 0x80, 0xa4,
    ];
    let mut bytes = vec![0xaa, 0x81, 0x72];
    bytes.extend_from_slice(&discriminator);
    bytes.push(0xff);

    let references = crate::om::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].object_id, 370);
    assert_eq!(references[0].offset, 1);

    bytes.extend_from_slice(&[0x73]);
    bytes.extend_from_slice(&discriminator);
    let references = crate::om::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 2);
    assert_eq!(references[1].object_id, 0x73);
    assert_eq!(references[1].offset, 22);

    bytes[8] ^= 1;
    let references = crate::om::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].object_id, 0x73);
    let mut null = vec![0xff];
    null.extend_from_slice(&discriminator);
    assert!(crate::om::data_block_object_frames(&null).is_empty());
}

#[test]
fn om_offset_store_counted_index_lane_requires_complete_non_null_members() {
    let bytes = [
        0xaa, 0x01, 0x06, 0x42, 0x62, 0x80, 0x48, 0x80, 0x50, 0x7c, 0x01, 0x11, 0xbb,
    ];
    let lanes = crate::om::offset_store_counted_index_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].declared_count, 6);
    assert_eq!(lanes[0].anchor, 0x42);
    assert_eq!(lanes[0].anchor_offset, 3);
    assert_eq!(
        lanes[0].members,
        vec![(0x62, 4), (0x48, 5), (0x50, 7), (0x7c, 9)]
    );

    assert!(
        crate::om::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0xff, 0x01, 0x11,])
            .is_empty()
    );
    assert!(
        crate::om::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0x80, 0x01, 0x11,])
            .is_empty()
    );
    assert!(
        crate::om::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0x62, 0x01, 0x10,])
            .is_empty()
    );
}

#[test]
fn om_offset_store_abr_lane_requires_sixteen_slots_and_exact_terminator() {
    let mut bytes = vec![0xaa, 0x11];
    bytes.extend_from_slice(&[0xff; 6]);
    bytes.extend_from_slice(&[0x82, 0x83]);
    bytes.extend_from_slice(&[0xff; 9]);
    bytes.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03, 0xbb]);

    let lanes = crate::om::offset_store_abr_reference_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].slots.len(), 16);
    assert_eq!(lanes[0].slots[6], (Some(643), 8));
    assert!(lanes[0]
        .slots
        .iter()
        .enumerate()
        .all(|(slot, (value, _))| slot == 6 || value.is_none()));

    bytes[23] = b'X';
    assert!(crate::om::offset_store_abr_reference_lanes(&bytes).is_empty());
    bytes[23] = b'R';
    bytes.remove(18);
    assert!(crate::om::offset_store_abr_reference_lanes(&bytes).is_empty());
}

#[test]
fn om_sketch_scalar_field_requires_exact_frame_and_finite_shifted_value() {
    let bytes = [
        0xaa, 0x50, 0x59, 0x66, 0x64, 0x00, 0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72, 0xbb,
    ];
    let fields = crate::om::construction_payload_scalar_fields(&bytes);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 1);
    assert_eq!(fields[0].field_code, 0x64);
    assert!((fields[0].value - 38.1).abs() < 2.0e-12);

    let mut malformed = bytes;
    malformed[5] = 1;
    assert!(crate::om::construction_payload_scalar_fields(&malformed).is_empty());
    malformed = bytes;
    malformed[6] = 0x70;
    assert!(crate::om::construction_payload_scalar_fields(&malformed).is_empty());
}

#[test]
fn om_sketch_name_field_decodes_direct_and_extended_compact_type_codes() {
    let bytes = [
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0xaa, 0x66, 0x80, 0x83,
        0x03, 0x07, b'L', b'i', b'n', b'e', b'2', 0x00,
    ];
    let fields = crate::om::construction_payload_named_fields(&bytes);
    assert_eq!(fields.len(), 2);
    assert_eq!(
        (fields[0].offset, fields[0].type_code, fields[0].value),
        (0, Some(0x32), "Point1")
    );
    assert_eq!(
        (fields[1].offset, fields[1].type_code, fields[1].value),
        (12, Some(0x83), "Line2")
    );

    assert!(crate::om::construction_payload_named_fields(&[
        0x66, 0xff, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00,
    ])
    .is_empty());
    assert!(crate::om::construction_payload_named_fields(&[
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't',
    ])
    .is_empty());
}

#[test]
fn om_sketch_name_field_decodes_type_free_payload_leading_form() {
    let fields = crate::om::construction_payload_named_fields(&[
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0x04,
    ]);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 0);
    assert_eq!(fields[0].type_code, None);
    assert!(fields[0].payload_leading);
    assert_eq!(fields[0].value, "Point1");

    assert!(crate::om::construction_payload_named_fields(&[
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1',
    ])
    .is_empty());
}

#[test]
fn om_offset_store_named_point_uses_minimal_consecutive_block_span() {
    let first = [
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'7', 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30,
        0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];
    let second = [
        0x45, 0x04, 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33,
        0x07,
    ];
    let point = crate::om::offset_store_named_point(&[&first, &second]).unwrap();
    assert_eq!(point.name, "Point7");
    assert!(point
        .values
        .iter()
        .all(|value| (*value - 57.15).abs() < 1.0e-12));
    assert_eq!(point.value_offsets, [9, first.len() + 3]);
    assert_eq!(point.block_count, 2);

    let mut same_block = first.to_vec();
    same_block.extend_from_slice(&second);
    assert_eq!(
        crate::om::offset_store_named_point(&[&same_block])
            .unwrap()
            .block_count,
        1
    );
    assert_eq!(
        crate::om::offset_store_named_point(&[&first[..9], &first[9..], &second])
            .unwrap()
            .block_count,
        3
    );
    let mut zero = first;
    zero[7] = b'0';
    assert!(crate::om::offset_store_named_point(&[&zero, &second]).is_none());
}

#[test]
fn sketch_fixed_pair_parser_reads_signed_q1_55_atoms() {
    let bytes = [
        0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let pairs = crate::om::sketch_payload_fixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].values, [0.5, -0.5]);
    assert_eq!(pairs[0].value_offsets, [8, 17]);
    assert_eq!(pairs[0].raw_values[0], [0x40, 0, 0, 0, 0, 0, 0]);

    let mut malformed = bytes;
    malformed[16] = 1;
    assert!(crate::om::sketch_payload_fixed_pairs(&malformed).is_empty());
}

#[test]
fn sketch_named_records_own_fixed_pairs_within_their_intervals() {
    use crate::native::{
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
        payload_offset: 20,
        value_payload_offsets: [28, 37],
        source_offset: 1020,
        value_source_offsets: [1028, 1037],
    };

    let names = [name("first", 0, 10), name("second", 1, 50)];
    let pairs = [pair];
    let records = feature_sketch_payload_named_records(&[payload], &names, &[], &pairs);
    assert_eq!(records[0].fixed_pairs, ["pair"]);
    assert!(records[1].fixed_pairs.is_empty());
    let points = feature_sketch_fixed_points(&records, &names, &pairs);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].name, "Point1");
    assert_eq!(points[0].values, [0.5, -0.5]);
}

#[test]
fn sketch_named_point_block_uses_require_exact_shared_block_identity() {
    use crate::native::{
        feature_sketch_named_point_block_uses, FeatureSketchReference, OffsetStoreNamedPoint,
    };

    let point = OffsetStoreNamedPoint {
        id: "nx:offset-store:named-point#2-10".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["block-10".to_string(), "block-11".to_string()],
        values: [1.0, 2.0],
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
    use crate::native::{
        feature_sketch_preceding_named_point_uses, FeatureSketchReference, OffsetStoreNamedPoint,
    };

    let reference = |ordinal, terminal, block: Option<&str>| FeatureSketchReference {
        id: format!("reference-{ordinal}"),
        operation_label: "nx:feature-history:operation-label#1-4".to_string(),
        ordinal,
        declared_count: 2,
        terminal,
        object_index: 12 + ordinal,
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
    use crate::native::{
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
        &[block_use.clone(), second_block_use.clone()],
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

#[test]
fn sketch_point_blocks_establish_ordered_datum_csys_dependencies() {
    use crate::native::{
        FeatureDatumCsysConstruction, FeatureOperationLabel, FeatureSketchPointUse,
        OffsetStoreNamedPoint,
    };

    let label = |id: &str, ordinal| FeatureOperationLabel {
        id: id.to_string(),
        section_link: "section".to_string(),
        ordinal,
        value: if ordinal == 0 { "SKETCH" } else { "DATUM_CSYS" }.to_string(),
        object_indices: [None; 4],
        source_offset: 100 + u64::from(ordinal),
    };
    let labels = [label("sketch", 0), label("csys", 1)];
    let point = OffsetStoreNamedPoint {
        id: "point".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["point-first".to_string(), "shared".to_string()],
        values: [1.0, 2.0],
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
        data_blocks: blocks,
        source_offsets: [400; 8],
    };

    let dependencies = crate::native::feature_sketch_datum_csys_dependencies(
        &labels,
        std::slice::from_ref(&point),
        std::slice::from_ref(&point_use),
        std::slice::from_ref(&construction),
    );
    assert_eq!(dependencies[0].datum_csys_operation_label, "csys");
    assert_eq!(dependencies[0].sketch_operation_label, "sketch");
    assert_eq!(dependencies[0].sketch_point_use, "point-use");
    assert_eq!(dependencies[0].shared_data_block, "shared");

    let reversed_labels = [label("csys", 0), label("sketch", 1)];
    assert!(crate::native::feature_sketch_datum_csys_dependencies(
        &reversed_labels,
        &[point],
        &[point_use],
        &[construction],
    )
    .is_empty());
}

#[test]
fn nx_sketch_point_names_require_positive_decimal_suffixes() {
    assert_eq!(crate::native::parse_sketch_point_name("Point1"), Some(1));
    assert_eq!(
        crate::native::parse_sketch_point_name("Point2048"),
        Some(2048)
    );
    for malformed in ["Point", "Point0", "point1", "Point-1", "Point1A"] {
        assert_eq!(crate::native::parse_sketch_point_name(malformed), None);
    }
}

#[test]
fn om_simple_hole_lane_requires_two_identical_nonempty_scalar_runs() {
    let shifted = |value: f64| {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        bytes
    };
    let mut payload = Vec::new();
    for value in [508.0, 38.1, 508.0, 38.1] {
        payload.extend_from_slice(&shifted(value));
        payload.push(0x7f);
    }
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X");
    payload.push(0x00);
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 120,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let lane = crate::om::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(lane.values[0], 508.0);
    assert!((lane.values[1] - 38.1).abs() < 2.0e-12);
    assert_eq!(lane.witness_offsets, [vec![200, 209], vec![218, 227]]);

    let mut mismatched = payload.clone();
    mismatched[18 + 7] ^= 1;
    assert!(
        crate::om::simple_hole_repeated_scalar_lane(crate::om::OperationRecord {
            bytes: &mismatched,
            payload: &mismatched,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_simple_hole_lane_accepts_one_repeated_scalar() {
    let mut scalar = 25.4f64.to_be_bytes();
    scalar[0] -= 0x10;
    let mut payload = scalar.to_vec();
    payload.push(0x7f);
    payload.extend_from_slice(&scalar);
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X\0");
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label: crate::om::OperationLabel {
            header_offset: 100,
            offset: 120,
            value: "SIMPLE HOLE",
            object_indices: [None; 4],
            object_index_offsets: [0; 4],
        },
    };
    let lane = crate::om::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(lane.values, [25.4]);
    assert_eq!(lane.witness_offsets, [vec![200], vec![209]]);
}

#[test]
fn om_simple_hole_lane_block_references_follow_both_scalar_runs() {
    let shifted = |value: f64| {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        bytes
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(&shifted(508.0));
    payload.extend_from_slice(&shifted(38.1));
    payload.extend_from_slice(&[0xf0, 0xe7, 0xf0, 0xe8]);
    payload.extend_from_slice(&shifted(508.0));
    payload.extend_from_slice(&shifted(38.1));
    payload.extend_from_slice(&[0xf0, 0xe9, 0xf0, 0xea]);
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X");
    payload.push(0x00);
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 120,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let references = crate::om::simple_hole_repeated_scalar_lane_block_references(record).unwrap();
    assert_eq!(references.first, [231, 232]);
    assert_eq!(references.second, [233, 234]);
    assert_eq!(references.offsets, [[216, 218], [236, 238]]);

    let mut null = payload.clone();
    null[16] = 0xff;
    assert!(
        crate::om::simple_hole_repeated_scalar_lane_block_references(crate::om::OperationRecord {
            bytes: &null,
            payload: &null,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_datum_csys_reference_lane_requires_eight_canonical_indices() {
    let mut payload = vec![
        0x13, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    for value in 42..50 {
        payload.extend_from_slice(&[0xf0, value]);
    }
    payload.extend_from_slice(&[0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    let label = crate::om::OperationLabel {
        header_offset: 10,
        offset: 20,
        value: "DATUM_CSYS",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 10,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    let field = crate::om::datum_csys_references(record).unwrap();
    assert_eq!(field.control, 0x13);
    assert_eq!(
        field.references.map(|reference| reference.object_index),
        [42, 43, 44, 45, 46, 47, 48, 49]
    );
    assert_eq!(
        field.references.map(|reference| reference.offset),
        [114, 116, 118, 120, 122, 124, 126, 128]
    );

    let mut alternate_control = payload.clone();
    alternate_control[0] = 0x1a;
    assert_eq!(
        crate::om::datum_csys_references(crate::om::OperationRecord {
            bytes: &alternate_control,
            payload: &alternate_control,
            ..record
        })
        .unwrap()
        .control,
        0x1a
    );

    let mut malformed = payload.clone();
    malformed[14] = 0x2a;
    assert!(
        crate::om::datum_csys_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_datum_plane_header_requires_common_prefix_and_nontrivial_count() {
    let payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf,
    ];
    let label = crate::om::OperationLabel {
        header_offset: 10,
        offset: 20,
        value: "DATUM_PLANE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 10,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    assert_eq!(
        crate::om::datum_plane_payload_header(record),
        Some(crate::om::DatumPlanePayloadHeader {
            control: 0x22,
            declared_count: 3,
            branch_tag: 0x29,
        })
    );
    let mut malformed = payload;
    malformed[6] = 1;
    assert!(
        crate::om::datum_plane_payload_header(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let branch_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x23, 0x01, 0x02, 0x80, 0x4c, 0x01, 0xf1, 0x02,
        0xbb, 0x00, 0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
    ];
    let branch = crate::om::datum_plane_single_reference_branch(crate::om::OperationRecord {
        bytes: &branch_payload,
        payload: &branch_payload,
        ..record
    })
    .unwrap();
    assert_eq!(branch.descriptor_index, 76);
    assert_eq!(branch.descriptor_offset, 110);
    assert_eq!(branch.object_index, 699);
    assert_eq!(branch.object_offset, 113);

    let double_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x29, 0x01, 0x02, 0xf1, 0x02, 0x77, 0x01, 0x01,
        0x18, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf1, 0x02, 0x78, 0x01, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let double = crate::om::datum_plane_double_reference_branch(crate::om::OperationRecord {
        bytes: &double_payload,
        payload: &double_payload,
        ..record
    })
    .unwrap();
    assert_eq!(
        double.references.map(|reference| reference.object_index),
        [631, 632]
    );
    assert_eq!(
        double.references.map(|reference| reference.offset),
        [110, 124]
    );

    let count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf, 0x01, 0x01,
        0x3a, 0x01, 0x02, 0xf1, 0x02, 0xd0, 0x01, 0x17, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0xff, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let count_three = crate::om::datum_plane_double_reference_branch(crate::om::OperationRecord {
        bytes: &count_three_payload,
        payload: &count_three_payload,
        ..record
    })
    .unwrap();
    assert_eq!(
        count_three
            .references
            .map(|reference| reference.object_index),
        [719, 720]
    );
    assert_eq!(
        count_three.references.map(|reference| reference.offset),
        [110, 118]
    );

    let descriptor_count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x28, 0x01, 0x02, 0x80, 0x4d, 0x01, 0x29, 0x01,
        0x02, 0xf1, 0x02, 0xd1, 0x01, 0x01, 0x07, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff,
        0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let descriptor_count_three =
        crate::om::datum_plane_descriptor_reference_branch(crate::om::OperationRecord {
            bytes: &descriptor_count_three_payload,
            payload: &descriptor_count_three_payload,
            ..record
        })
        .unwrap();
    assert_eq!(descriptor_count_three.descriptor_index, 77);
    assert_eq!(descriptor_count_three.descriptor_offset, 110);
    assert_eq!(descriptor_count_three.object_index, 721);
    assert_eq!(descriptor_count_three.object_offset, 116);
}

#[test]
fn om_datum_plane_object_index_lane_ends_at_logical_payload_boundary() {
    let bytes = [
        0x80, 0xab, 0x01, 0x04, 0x01, 0x01, 0x01, 0x00, 0x12, 0x34, 0x56, 0x78,
    ];
    let lanes = crate::om::datum_plane_object_index_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 2);
    assert_eq!(lanes[0].declared_count, 4);
    assert_eq!(lanes[0].indices, [(1, 4), (1, 5), (1, 6)]);
    assert_eq!(lanes[0].trailer, 0x1234_5678);

    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert!(crate::om::datum_plane_object_index_lanes(&trailing).is_empty());
}

#[test]
fn om_datum_plane_object_scalar_pairs_require_the_complete_discriminator() {
    let mut bytes = vec![0x7f, 0x01, 0x01, 0xff];
    bytes.extend_from_slice(&[
        0x6d, 0x00, 0xf0, 0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x03,
    ]);
    bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let pairs = crate::om::datum_plane_object_scalar_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].offset, 4);
    assert_eq!(pairs[0].value_offsets, [22, 31]);
    assert_eq!(pairs[0].values, [10.0, -20.0]);
    bytes[10] ^= 1;
    assert!(crate::om::datum_plane_object_scalar_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_plane_descriptor_requires_complete_lowercase_hex_identity() {
    let mut bytes = *b"793487222121a5474a9125451b8e31f5?A\xf0\x1e\xff\x02\x01\x33";
    let descriptor = crate::om::datum_plane_descriptor_block(&bytes).unwrap();
    assert_eq!(descriptor.identity, "793487222121a5474a9125451b8e31f5");
    assert_eq!(descriptor.suffix, b"?A\xf0\x1e\xff\x02\x01\x33");
    assert_eq!(descriptor.schema_index, 28_702);
    assert_eq!(descriptor.label, "3");

    let short_bytes = *b"a75c5f0ed880dd1443b3c5c57908aae?A\xf0\x1f\xff\x02\x01\x66\x33";
    let short = crate::om::datum_plane_descriptor_block(&short_bytes).unwrap();
    assert_eq!(short.identity.len(), 31);
    assert_eq!(short.schema_index, 28_703);
    assert_eq!(short.label, "f3");

    bytes[0] = b'G';
    assert!(crate::om::datum_plane_descriptor_block(&bytes).is_none());
    assert!(crate::om::datum_plane_descriptor_block(&bytes[..39]).is_none());
}

#[test]
fn om_datum_csys_scalar_pairs_require_discriminator_and_separator() {
    let mut bytes = vec![0x2f, 0x2f, 0x41, 0x6d, 0x00, 0xf0];
    bytes.extend_from_slice(&[
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ]);
    bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let pairs = crate::om::object_payload_scalar_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].offset, 6);
    assert_eq!(pairs[0].value_offsets, [21, 30]);
    assert_eq!(pairs[0].values, [10.0, -20.0]);
    assert_eq!(pairs[0].discriminator.len(), 15);

    let mut extended = vec![
        0x08, 0x02, 0x03, 0x01, 0x81, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00,
        0x03,
    ];
    extended.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    extended.push(0);
    extended.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let extended_pairs = crate::om::object_payload_scalar_pairs(&extended);
    assert_eq!(extended_pairs.len(), 1);
    assert_eq!(extended_pairs[0].discriminator.len(), 16);
    assert_eq!(extended_pairs[0].value_offsets, [16, 25]);

    bytes[29] = 1;
    assert!(crate::om::object_payload_scalar_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_csys_descriptor_requires_one_maximal_hex_identity() {
    let bytes = b"\x02\x01ae166162820ea2d993e1fdf49091850e?A\x80\xa0\xf0\x26";
    let descriptor = crate::om::datum_csys_descriptor_block(bytes).unwrap();
    assert_eq!(descriptor.prefix, [0x02, 0x01]);
    assert_eq!(descriptor.identity, "ae166162820ea2d993e1fdf49091850e");
    assert_eq!(descriptor.identity_offset, 2);
    assert_eq!(descriptor.suffix, b"?A\x80\xa0\xf0\x26");

    let mut ambiguous = bytes.to_vec();
    ambiguous.extend_from_slice(b"012345678901234567890123456789");
    assert!(crate::om::datum_csys_descriptor_block(&ambiguous).is_none());
}

#[test]
fn nx_datum_plane_csys_identity_uses_join_only_equal_typed_identities() {
    let plane = crate::native::FeatureDatumPlaneDescriptor {
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
    let csys = crate::native::FeatureDatumCsysDescriptor {
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
    let uses = crate::native::feature_datum_plane_csys_identity_uses(&[plane], &[csys]);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].identity, "012345678901234567890123456789");
    assert_eq!(uses[0].datum_plane_operation_label, "operation#4");
    assert_eq!(uses[0].datum_csys_operation_label, "operation#2");
    assert_eq!(uses[0].datum_csys_reference_ordinal, 7);
}

#[test]
fn nx_datum_csys_block_uses_preserve_reference_and_input_order() {
    let construction = crate::native::FeatureDatumCsysConstruction {
        id: "construction".to_string(),
        operation_label: "operation#0".to_string(),
        control: 0x13,
        object_indices: std::array::from_fn(|index| index as u32 + 40),
        data_blocks: std::array::from_fn(|index| format!("block#{}", index + 40)),
        source_offsets: std::array::from_fn(|index| index as u64 + 100),
    };
    let input =
        |id: &str, operation: &str, slot: u8, block: &str| crate::native::FeatureInputBlock {
            id: id.to_string(),
            operation_label: operation.to_string(),
            input_slot: slot,
            object_index: 44,
            data_block: block.to_string(),
            source_offset: 200,
        };
    let uses = crate::native::feature_datum_csys_block_uses(
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
fn om_operation_primary_body_reference_requires_one_complete_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 100,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let bytes = [0x01, 0x02, 0x10, 0x90, 0x19, 0x42, 0xff];
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &bytes,
        payload_offset: 100,
        payload: &bytes,
        label,
    };
    assert_eq!(
        crate::om::operation_body_reference(record),
        Some(crate::om::OperationBodyReference {
            offset: 103,
            object_index: 6466,
        })
    );

    let duplicate = [bytes.as_slice(), bytes.as_slice()].concat();
    assert_eq!(
        crate::om::operation_body_references(crate::om::OperationRecord {
            offset: 100,
            bytes: &duplicate,
            payload_offset: 100,
            payload: &duplicate,
            label,
        }),
        [
            crate::om::OperationBodyReference {
                offset: 103,
                object_index: 6466,
            },
            crate::om::OperationBodyReference {
                offset: 110,
                object_index: 6466,
            },
        ]
    );
    assert!(
        crate::om::operation_body_reference(crate::om::OperationRecord {
            offset: 100,
            bytes: &duplicate,
            payload_offset: 100,
            payload: &duplicate,
            label,
        })
        .is_none()
    );
}

#[test]
fn om_data_block_object_references_require_complete_field_frames() {
    let bytes = [
        0x04, 0x00, 0x2a, 0x02, 0x0b, 0xff, 0x04, 0x00, 0x80, 0xc9, 0x02, 0x0b, 0x04, 0x00, 0x90,
        0x19, 0x42, 0x02, 0x0b,
    ];
    assert_eq!(
        crate::om::data_block_object_references(&bytes),
        [
            crate::om::DataBlockObjectReference {
                offset: 2,
                object_index: 42,
            },
            crate::om::DataBlockObjectReference {
                offset: 8,
                object_index: 201,
            },
            crate::om::DataBlockObjectReference {
                offset: 14,
                object_index: 6466,
            },
        ]
    );
    assert_eq!(
        crate::om::data_block_object_references(&bytes[..bytes.len() - 1]).len(),
        2
    );
}

#[test]
fn feature_body_lineage_excludes_tools_consumed_after_their_latest_writer() {
    use crate::native::{
        FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
    };

    let label = |ordinal: u32, value: &str| FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        source_offset: ordinal as u64,
    };
    let labels = [label(0, "EXTRUDE"), label(1, "EXTRUDE"), label(2, "UNITE")];
    let reference = |operation: &str, body_object_index| FeatureBodyReference {
        id: format!("reference#{body_object_index}"),
        operation_label: operation.to_string(),
        body_object_index,
        source_offset: 0,
    };
    let references = [reference("operation#0", 10), reference("operation#1", 20)];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#2".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 10,
        tool_object_indices: vec![20],
        source_offset: 0,
    }];

    assert_eq!(
        crate::native::terminal_feature_body_indices(&labels, &references, &booleans, &[], &[]),
        Some([10].into_iter().collect())
    );
}

#[test]
fn segment_body_lineage_statuses_cover_every_bound_image() {
    use crate::native::{
        segment_body_lineage_statuses, FeatureBodyReference, FeatureBooleanKind,
        FeatureBooleanOperation, FeatureOperationLabel, SegmentBodyBinding,
    };
    let labels = [
        FeatureOperationLabel {
            id: "operation#0".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "EXTRUDE".to_string(),
            object_indices: [None; 4],
            source_offset: 0,
        },
        FeatureOperationLabel {
            id: "operation#1".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 1,
            value: "UNITE".to_string(),
            object_indices: [None; 4],
            source_offset: 1,
        },
    ];
    let references = [FeatureBodyReference {
        id: "reference#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 10,
        tool_object_indices: vec![21],
        source_offset: 1,
    }];
    let binding = |id: &str, stream_ordinal: u32, body, alias| SegmentBodyBinding {
        id: id.to_string(),
        stream_link: format!("stream#{stream_ordinal}"),
        stream_ordinal,
        stream_kind: "partition".to_string(),
        body_object_index: body,
        body_alias_object_index: alias,
        stream_role: 19,
        source_offset: u64::from(stream_ordinal),
    };
    let statuses = segment_body_lineage_statuses(
        &labels,
        &references,
        &booleans,
        &[],
        &[
            binding("binding#0", 0, 10, 11),
            binding("binding#1", 1, 20, 21),
        ],
    )
    .unwrap();
    assert_eq!(statuses.len(), 2);
    assert!(statuses[0].terminal);
    assert!(!statuses[1].terminal);
}

#[test]
fn feature_body_lineage_treats_segment_tuple_indices_as_one_identity() {
    use crate::native::{
        FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
        SegmentBodyBinding,
    };

    let label = |ordinal: u32, value: &str| FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        source_offset: ordinal as u64,
    };
    let labels = [label(0, "EXTRUDE"), label(1, "UNITE")];
    let references = [FeatureBodyReference {
        id: "reference#150".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 150,
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 10,
        tool_object_indices: vec![94],
        source_offset: 0,
    }];
    let bindings = [SegmentBodyBinding {
        id: "binding#0".to_string(),
        stream_link: "stream#0".to_string(),
        stream_ordinal: 0,
        stream_kind: "partition".to_string(),
        body_object_index: 94,
        body_alias_object_index: 150,
        stream_role: 19,
        source_offset: 0,
    }];

    assert_eq!(
        crate::native::terminal_feature_body_indices(
            &labels,
            &references,
            &booleans,
            &[],
            &bindings,
        ),
        Some(std::collections::BTreeSet::new())
    );
}

#[test]
fn feature_body_lineage_closes_overlapping_alias_pairs_transitively() {
    use crate::native::{
        segment_body_lineage_statuses, FeatureBodyReference, FeatureBooleanKind,
        FeatureBooleanOperation, FeatureOperationLabel, SegmentBodyBinding,
    };

    let label = |ordinal: u32, value: &str| FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        source_offset: u64::from(ordinal),
    };
    let labels = [label(0, "EXTRUDE"), label(1, "UNITE")];
    let references = [FeatureBodyReference {
        id: "reference#30".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 30,
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 99,
        tool_object_indices: vec![10],
        source_offset: 1,
    }];
    let binding = |id: &str, stream_ordinal, body, alias| SegmentBodyBinding {
        id: id.to_string(),
        stream_link: format!("stream#{stream_ordinal}"),
        stream_ordinal,
        stream_kind: "partition".to_string(),
        body_object_index: body,
        body_alias_object_index: alias,
        stream_role: 19,
        source_offset: u64::from(stream_ordinal),
    };
    let bindings = [
        binding("binding#0", 0, 10, 20),
        binding("binding#1", 1, 30, 20),
        binding("binding#2", 2, 40, 20),
    ];

    let statuses =
        segment_body_lineage_statuses(&labels, &references, &booleans, &[], &bindings).unwrap();
    assert_eq!(statuses.len(), 3);
    assert!(statuses.iter().all(|status| !status.terminal));
}

#[test]
fn feature_body_lineage_consumes_segment_bound_sew_operands() {
    use crate::native::{FeatureOperationBodyOperand, FeatureOperationLabel, SegmentBodyBinding};
    let labels = [FeatureOperationLabel {
        id: "operation#0".to_string(),
        section_link: "history#0".to_string(),
        ordinal: 0,
        value: "SEW".to_string(),
        object_indices: [None; 4],
        source_offset: 0,
    }];
    let bindings = [SegmentBodyBinding {
        id: "binding#0".to_string(),
        stream_link: "stream#0".to_string(),
        stream_ordinal: 0,
        stream_kind: "partition".to_string(),
        body_object_index: 20,
        body_alias_object_index: 30,
        stream_role: 0,
        source_offset: 0,
    }];
    let operands = [FeatureOperationBodyOperand {
        id: "operand#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        body_reference_ordinal: 0,
        ordinal: 0,
        operand_object_index: 30,
        segment_body_bindings: vec!["binding#0".to_string()],
        source_offset: 0,
    }];
    assert_eq!(
        crate::native::terminal_feature_body_indices(&labels, &[], &[], &operands, &bindings),
        Some(std::collections::BTreeSet::new())
    );
}

#[test]
fn om_size_frame_bounds_its_type_declarations() {
    let bytes = size_framed_om_section();
    let sections = crate::om::sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].offset, 0);
    assert_eq!(sections[0].byte_len, bytes.len());
    assert_eq!(sections[0].types.len(), 2);
    assert_eq!(sections[0].types[0].name, "UGS::FEATURE_RECORD");
    assert_eq!(
        sections[0].types[0].registry_suffix,
        &[0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06]
    );
    assert_eq!(sections[0].types[1].trailing_code, 0x65);
    assert_eq!(sections[0].fields.len(), 2);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(sections[0].fields[1].trailing_code, 0x81);
    assert_eq!(sections[0].record_area, None);

    let mut truncated = bytes;
    truncated.pop();
    assert!(crate::om::sections(&truncated).is_empty());
}

#[test]
fn om_size_frame_uses_validated_internal_record_area_pointer() {
    let bytes = size_framed_om_section_with_record_area();
    let section = crate::om::sections(&bytes).remove(0);
    let offset = section.record_area_offset.expect("record area");
    assert_eq!(offset, size_framed_om_section().len() + 20);
    assert_eq!(section.record_area.unwrap(), &bytes[offset..]);
    assert_eq!(&bytes[offset + 12..offset + 15], &[0x05, 0x01, 0x0e]);

    let mut invalid = bytes;
    invalid[offset + 12] = 1;
    assert_eq!(crate::om::sections(&invalid)[0].record_area, None);
}

#[test]
fn om_operation_labels_require_the_complete_frame() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x01\x82\x40\x90\x17\xd3\xff\x03\x07UNITE\0\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x02\x03\xff\xff\x03\x08SKETCH\0";
    let labels = crate::om::operation_labels(bytes, 100);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].offset, 122);
    assert_eq!(labels[0].header_offset, 100);
    assert_eq!(labels[0].value, "UNITE");
    assert_eq!(
        labels[0].object_indices,
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[1].value, "SKETCH");
    assert_eq!(labels[1].object_indices, [Some(2), Some(3), None, None]);

    assert!(crate::om::operation_labels(b"\xff\xff\x03\x07UNITE\0", 0).is_empty());
    let mut invalid = bytes.to_vec();
    invalid[15] = 0x91;
    assert_eq!(crate::om::operation_labels(&invalid, 0).len(), 1);
}

#[test]
fn om_operation_records_use_consecutive_validated_headers() {
    let bytes = b"prefix\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x07UNITE\0payload\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x08SKETCH\0tail";
    let records = crate::om::operation_records(bytes, 10);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].offset, 16);
    assert_eq!(records[0].label.value, "UNITE");
    assert!(records[0].bytes.ends_with(b"payload"));
    assert_eq!(records[0].payload, b"payload");
    assert_eq!(records[0].payload_offset, 43);
    assert_eq!(records[1].label.value, "SKETCH");
    assert!(records[1].bytes.ends_with(b"tail"));
    assert_eq!(records[1].payload, b"tail");
}

#[test]
fn om_operation_payload_strings_require_complete_utf8_frames() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x00\x04\x07BLOCK\0\x04\x04\xc3\x97\0\x04\x07BROKEN";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let strings = crate::om::operation_payload_strings(record);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 201);
    assert_eq!(strings[0].value, "BLOCK");
    assert_eq!(strings[1].value, "×");
}

#[test]
fn om_sketch_payload_reference_field_is_counted_ordered_and_canonical() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKETCH",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x00\x01\x05\xf0\xff\xf1\x01\x00\xf1\x01\x01\xf1\x01\x02\x00\x00\xf1\x01\x03\x01\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::sketch_payload_references(record).unwrap();
    assert_eq!(field.declared_count, 5);
    let references: [crate::om::SketchPayloadReference; 5] = field.references.try_into().unwrap();
    assert_eq!(
        references.map(|reference| reference.object_index),
        [255, 256, 257, 258, 259]
    );
    assert_eq!(
        references.map(|reference| reference.offset),
        [204, 206, 209, 212, 217]
    );
    let zero = b"\x01\x00\x00\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = crate::om::sketch_payload_references(crate::om::OperationRecord {
        payload: zero,
        bytes: zero,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 0);
    assert_eq!(field.references.len(), 1);
    assert_eq!(field.references[0].object_index, 0x42);
    let two = b"\x01\x00\x01\x02\xf0\x41\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = crate::om::sketch_payload_references(crate::om::OperationRecord {
        payload: two,
        bytes: two,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 2);
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [0x41, 0x42]
    );

    let mut noncanonical = payload.to_vec();
    noncanonical[7] = 0;
    assert!(
        crate::om::sketch_payload_references(crate::om::OperationRecord {
            payload: &noncanonical,
            bytes: &noncanonical,
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::sketch_payload_references(crate::om::OperationRecord {
            label: crate::om::OperationLabel {
                value: "BLOCK",
                ..label
            },
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_profile_references_require_matching_witness_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x02\x16\x01\x03\xf0\xff\xf1\x01\x00\x01\x03\x79\xaa\x01\x03\xf0\xff\xf1\x01\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::extrude_profile_references(record).unwrap();
    assert!(field.witnessed);
    let references = field.references;
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].object_index, 255);
    assert_eq!(references[0].offset, 205);
    assert_eq!(references[1].object_index, 256);
    assert_eq!(references[1].offset, 207);

    let without_witness = &payload[..14];
    let field = crate::om::extrude_profile_references(crate::om::OperationRecord {
        payload: without_witness,
        bytes: without_witness,
        ..record
    })
    .unwrap();
    assert!(!field.witnessed);
    assert_eq!(field.references.len(), 2);
    assert!(
        crate::om::extrude_profile_references(crate::om::OperationRecord {
            label: crate::om::OperationLabel {
                value: "SKETCH",
                ..label
            },
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_header_decodes_shifted_ieee_scalars() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload =
        b"\x0f\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x2f\xa3\x74\xbc\x6a\x7e\xf9\xdb";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let header = crate::om::extrude_payload_header(record).unwrap();
    assert_eq!(header.offset, 205);
    assert_eq!(header.scalars, [0.04, 0.038]);

    let mut invalid = payload.to_vec();
    invalid[5] = 0xf0;
    assert!(
        crate::om::extrude_payload_header(crate::om::OperationRecord {
            payload: &invalid,
            bytes: &invalid,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_footer_requires_one_complete_terminal_lane() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let footer = crate::om::extrude_payload_footer(record).unwrap();
    assert_eq!(footer.offset, 200);
    assert_eq!(footer.type_indices, [351, 171]);
    assert_eq!(footer.mode_indices, [2, 1]);
    assert_eq!(footer.flags, [1, 2, 1, 1]);
    assert_eq!(footer.trailing_indices, [5, 255]);

    let truncated = &payload[..payload.len() - 1];
    assert!(
        crate::om::extrude_payload_footer(crate::om::OperationRecord {
            payload: truncated,
            bytes: truncated,
            ..record
        })
        .is_none()
    );

    let mut ambiguous = payload[..payload.len() - 1].to_vec();
    ambiguous.extend_from_slice(payload);
    assert!(
        crate::om::extrude_payload_footer(crate::om::OperationRecord {
            payload: &ambiguous,
            bytes: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_body_scalar_lane_decodes_zero_binary32_and_binary64() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x02\x10\x42\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 100,
        payload,
        label,
    };
    let triple = crate::om::extrude_payload_scalar_triple(record).unwrap();
    assert_eq!(
        triple.scalars.map(|scalar| scalar.value),
        [0.0, 3.0, -170.0]
    );
    assert_eq!(
        triple.scalars.map(|scalar| scalar.encoding),
        [
            crate::om::PayloadScalarEncoding::Zero,
            crate::om::PayloadScalarEncoding::Binary32,
            crate::om::PayloadScalarEncoding::Binary64,
        ]
    );
    assert_eq!(triple.scalars.map(|scalar| scalar.offset), [106, 107, 111]);

    let truncated = &payload[..18];
    assert!(
        crate::om::extrude_payload_scalar_triple(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_operation_body_scalar_clauses_preserve_body_order_and_branch() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x1c\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\xaa\x01\x02\x10\x43\xff\x11\x30\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let triples = crate::om::operation_body_scalar_triples(record);
    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].body_reference_ordinal, 0);
    assert_eq!(triples[0].body_object_index, 66);
    assert_eq!(triples[0].branch, 0x1c);
    assert_eq!(
        triples[0].scalars.map(|scalar| scalar.value),
        [0.0, 3.0, -170.0]
    );
    assert_eq!(triples[1].body_reference_ordinal, 1);
    assert_eq!(triples[1].body_object_index, 67);
    assert_eq!(triples[1].branch, 0x11);
    assert_eq!(
        triples[1].scalars.map(|scalar| scalar.value),
        [2.0, 0.0, 0.0]
    );
}

#[test]
fn om_operation_body_branch_11_decodes_wrapped_member_lane_atomically() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SEW",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x03\x2e\x7f\x00\x2e\x80\x01\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let members = crate::om::operation_body_members(record);
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].body_reference_ordinal, 0);
    assert_eq!(members[0].body_object_index, 66);
    assert_eq!(members[0].member_index, 127);
    assert_eq!(members[0].offset, 122);
    assert_eq!(members[1].member_index, 1);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(
        crate::om::operation_body_members(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..record
        })
        .is_empty()
    );
}

#[test]
fn om_trim_body_branch_11_decodes_terminal_continuation_atomically() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let continuations = crate::om::operation_body_11_continuations(record);
    assert_eq!(continuations.len(), 1);
    let continuation = continuations[0];
    assert_eq!(continuation.body_reference_ordinal, 0);
    assert_eq!(continuation.body_object_index, 114);
    assert_eq!(continuation.continuation_index, 67);
    assert_eq!(continuation.continuation_offset, 126);
    assert_eq!(continuation.terminal_object_index, 114);
    assert_eq!(continuation.terminal_offset, 131);

    let mut distinct_terminal = bytes.to_vec();
    distinct_terminal[31] = 0x71;
    assert_eq!(
        crate::om::operation_body_11_continuations(crate::om::OperationRecord {
            bytes: &distinct_terminal,
            payload: &distinct_terminal,
            ..record
        })[0]
            .terminal_object_index,
        113
    );

    let truncated = &bytes[..bytes.len() - 1];
    assert!(
        crate::om::operation_body_11_continuations(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..record
        })
        .is_empty()
    );
}

#[test]
fn om_operation_body_decodes_homogeneous_unwrapped_reference_lanes() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "OFFSET",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let compact = b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: compact,
        payload_offset: 100,
        payload: compact,
        label,
    };
    let lanes = crate::om::operation_body_reference_lanes(record);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].body_object_index, 110);
    assert_eq!(
        lanes[0].encoding,
        crate::om::OperationBodyReferenceLaneEncoding::CompactIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| (value.object_index, value.offset))
            .collect::<Vec<_>>(),
        [(13, 111), (105, 113)]
    );

    let objects =
        b"\x01\x02\x10\x70\xff\x1c\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let object_record = crate::om::OperationRecord {
        bytes: objects,
        payload: objects,
        ..record
    };
    let lanes = crate::om::operation_body_reference_lanes(object_record);
    assert_eq!(
        lanes[0].encoding,
        crate::om::OperationBodyReferenceLaneEncoding::PayloadObjectIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );

    let truncated = &objects[..objects.len() - 1];
    assert!(
        crate::om::operation_body_reference_lanes(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..object_record
        })
        .is_empty()
    );

    let branch_11 =
        b"\x01\x02\x10\x70\xff\x11\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let lanes = crate::om::operation_body_reference_lanes(crate::om::OperationRecord {
        bytes: branch_11,
        payload: branch_11,
        ..record
    });
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].branch, 0x11);
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );
}

#[test]
fn nx_extrude_construction_profile_requires_matching_resolved_encodings() {
    use crate::native::{
        FeatureExtrudeProfileReference, FeatureOperationBodyReferenceLane,
        FeatureOperationBodyReferenceLaneEncoding,
    };

    let references = [10, 11].map(|ordinal| FeatureExtrudeProfileReference {
        id: format!("profile-{ordinal}"),
        operation_label: "operation".to_string(),
        ordinal: ordinal - 10,
        witnessed: true,
        object_index: ordinal + 90,
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
        data_blocks: vec![Some("block-10".to_string()), Some("block-11".to_string())],
        source_offsets: vec![20, 21],
    };
    let profiles = crate::native::feature_extrude_construction_profiles(
        &references,
        std::slice::from_ref(&lane),
    );
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].body_object_index, 42);
    assert_eq!(profiles[0].object_indices, [100, 101]);
    assert_eq!(profiles[0].data_blocks, ["block-10", "block-11"]);

    let mut mismatched = lane;
    mismatched.object_indices[1] = 102;
    assert!(
        crate::native::feature_extrude_construction_profiles(&references, &[mismatched]).is_empty()
    );

    let mut unresolved = FeatureOperationBodyReferenceLane {
        id: "lane".to_string(),
        operation_label: "operation".to_string(),
        body_reference_ordinal: 0,
        body_object_index: 42,
        branch: 0x11,
        encoding: FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex,
        object_indices: vec![100, 101],
        data_blocks: vec![Some("block-10".to_string()), Some("block-11".to_string())],
        source_offsets: vec![20, 21],
    };
    unresolved.data_blocks[1] = None;
    assert!(
        crate::native::feature_extrude_construction_profiles(&references, &[unresolved]).is_empty()
    );
}

#[test]
fn nx_operation_body_operands_require_known_distinct_body_identities() {
    use crate::native::{
        FeatureBodyReferenceOccurrence, FeatureOperationBodyMember, SegmentBodyBinding,
    };
    let member = |ordinal, member_index| FeatureOperationBodyMember {
        id: format!("nx:feature-history:operation-body-member#0-{ordinal}"),
        operation_label: "operation".to_string(),
        body_reference_ordinal: 0,
        body_object_index: 10,
        ordinal,
        member_index,
        source_offset: u64::from(ordinal),
    };
    let members = [member(0, 20), member(1, 30), member(2, 10)];
    let references = [FeatureBodyReferenceOccurrence {
        id: "reference".to_string(),
        operation_label: "earlier".to_string(),
        ordinal: 0,
        body_object_index: 20,
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
    let operands = crate::native::feature_operation_body_operands(&members, &references, &bindings);
    assert_eq!(
        operands
            .iter()
            .map(|operand| operand.operand_object_index)
            .collect::<Vec<_>>(),
        [20, 30]
    );
    assert!(operands[0].segment_body_bindings.is_empty());
    assert_eq!(operands[1].segment_body_bindings, ["binding"]);
}

#[test]
fn om_extrude_body_32_branch_decodes_counted_lanes() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x73\xff\x32\x00\x00\x30\x77\x7e\x14\x7a\xe1\x47\xb3\x01\x03\x3d\x82\x56\x00\x3d\x82\x57\x00\x01\x04\x80\x2b\x80\x2d\x80\x2c\x01\x03\x80\x2e\x80\x77\x00\x01\x73\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let branch = crate::om::extrude_payload_32_branch(record).unwrap();
    assert_eq!(branch.offset, 105);
    assert_eq!(branch.body_object_index, 115);
    assert!(branch.scalar.is_finite());
    assert_eq!(branch.atoms_be, [0x3d82_5600, 0x3d82_5700]);
    assert_eq!(branch.atom_indices, [598, 599]);
    assert_eq!(branch.first_indices, [43, 45, 44]);
    assert_eq!(branch.second_indices, [46, 119]);
    assert_eq!(branch.terminal_object_index, 115);

    let mut invalid = bytes.to_vec();
    invalid[36] = 0xff;
    assert!(
        crate::om::extrude_payload_32_branch(crate::om::OperationRecord {
            bytes: &invalid,
            payload: &invalid,
            ..record
        })
        .is_none()
    );

    let mut invalid_atom = bytes.to_vec();
    invalid_atom[18] = 0x3c;
    assert!(
        crate::om::extrude_payload_32_branch(crate::om::OperationRecord {
            bytes: &invalid_atom,
            payload: &invalid_atom,
            ..record
        })
        .is_none()
    );

    let mut wrong_terminal_body = bytes.to_vec();
    wrong_terminal_body[43] = 0x72;
    assert!(
        crate::om::extrude_payload_32_branch(crate::om::OperationRecord {
            bytes: &wrong_terminal_body,
            payload: &wrong_terminal_body,
            ..record
        })
        .is_none()
    );
}

#[test]
fn nx_extrude_32_construction_requires_resolved_contiguous_profile() {
    let reference = crate::native::FeatureExtrudeProfileReference {
        id: "profile#0".to_string(),
        operation_label: "operation".to_string(),
        ordinal: 0,
        witnessed: false,
        object_index: 100,
        data_block: Some("block#100".to_string()),
        source_offset: 10,
    };
    let branch = crate::native::FeatureExtrudePayload32Branch {
        id: "branch".to_string(),
        operation_label: "operation".to_string(),
        body_object_index: 42,
        scalar: 1.0,
        atoms_be: vec![0x3d80_0100],
        atom_indices: vec![1],
        atom_data_blocks: vec![Some("block#1".to_string())],
        first_indices: vec![2],
        first_data_blocks: vec![Some("block#2".to_string())],
        second_indices: vec![3],
        second_data_blocks: vec![Some("block#3".to_string())],
        terminal_object_index: 42,
        source_offset: 20,
    };
    let constructions = crate::native::feature_extrude_32_constructions(
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

    let mut unresolved = reference;
    unresolved.data_block = None;
    assert!(crate::native::feature_extrude_32_constructions(
        &[unresolved],
        std::slice::from_ref(&branch),
    )
    .is_empty());
    let mut unresolved_lane = branch;
    unresolved_lane.first_data_blocks[0] = None;
    assert!(crate::native::feature_extrude_32_constructions(
        &[crate::native::FeatureExtrudeProfileReference {
            id: "profile#0".to_string(),
            operation_label: "operation".to_string(),
            ordinal: 0,
            witnessed: false,
            object_index: 100,
            data_block: Some("block#100".to_string()),
            source_offset: 10,
        }],
        &[unresolved_lane],
    )
    .is_empty());
}

#[test]
fn om_block_construction_field_decodes_ordered_canonical_references() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "BLOCK",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let mut payload = vec![0x26, 0, 0, 1, 0, 0];
    for value in 1..=18u8 {
        payload.extend([0xf0, value]);
    }
    payload.extend([0x01, 0xf1, 0x01, 0x00]);
    payload.extend([0xff; 11]);
    payload.extend([0; 4]);
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let field = crate::om::block_construction_references(record).unwrap();
    assert_eq!(field.control, 0x26);
    assert_eq!(field.references.len(), 19);
    assert_eq!(field.references[0].object_index, 1);
    assert_eq!(field.references[18].object_index, 256);
    assert_eq!(field.references[0].offset, 206);

    let mut invalid = payload.clone();
    invalid[42] = 0xf0;
    assert!(
        crate::om::block_construction_references(crate::om::OperationRecord {
            bytes: &invalid,
            payload: &invalid,
            ..record
        })
        .is_none()
    );
}

#[test]
fn nx_simple_hole_construction_groups_require_shared_four_block_identity() {
    use crate::native::{
        feature_simple_hole_construction_groups, FeatureSimpleHoleRepeatedScalarLane,
        FeatureSimpleHoleRepeatedScalarLaneBlockReferences,
    };
    let lane = |operation: &str| FeatureSimpleHoleRepeatedScalarLane {
        id: format!("lane-{operation}"),
        operation_label: operation.into(),
        values: vec![25.4],
        first_witness_offsets: vec![1],
        second_witness_offsets: vec![2],
    };
    let reference =
        |operation: &str, last: &str| FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
            id: format!("reference-{operation}"),
            operation_label: operation.into(),
            first_data_blocks: ["block-1".into(), "block-2".into()],
            second_data_blocks: ["block-3".into(), last.into()],
            first_reference_offsets: [3, 4],
            second_reference_offsets: [5, 6],
        };
    let lanes = [
        lane("operation#1-2"),
        lane("operation#1-3"),
        lane("operation#1-4"),
    ];
    let references = [
        reference("operation#1-2", "block-4"),
        reference("operation#1-3", "block-4"),
        reference("operation#1-4", "block-5"),
    ];
    let groups = feature_simple_hole_construction_groups(&lanes, &references);
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].operation_labels,
        ["operation#1-2", "operation#1-3"]
    );
    assert_eq!(
        groups[0].scalar_lanes,
        ["lane-operation#1-2", "lane-operation#1-3"]
    );
    assert_eq!(
        groups[0].block_references,
        ["reference-operation#1-2", "reference-operation#1-3"]
    );
}

#[test]
fn nx_block_construction_requires_complete_resolved_reference_field() {
    let references = (0..19)
        .map(|ordinal| crate::native::FeatureBlockConstructionReference {
            id: format!("reference#{ordinal}"),
            operation_label: "operation".to_string(),
            control: 0x26,
            ordinal,
            terminal: ordinal == 18,
            object_index: ordinal + 100,
            data_block: Some(format!("block#{ordinal}")),
            source_offset: u64::from(ordinal),
        })
        .collect::<Vec<_>>();
    let constructions = crate::native::feature_block_constructions(&references);
    assert_eq!(constructions.len(), 1);
    assert_eq!(constructions[0].control, 0x26);
    assert_eq!(constructions[0].member_references.len(), 18);
    assert_eq!(constructions[0].terminal_reference, "reference#18");
    assert_eq!(constructions[0].terminal_data_block, "block#18");

    let mut unresolved = references;
    unresolved[7].data_block = None;
    assert!(crate::native::feature_block_constructions(&unresolved).is_empty());
}

#[test]
fn nx_block_payload_points_require_exactly_two_named_scalars() {
    use crate::native::{
        feature_block_payload_point_groups, feature_block_payload_points, FeatureBlockPayloadName,
        FeatureBlockPayloadNamedRecord, FeatureBlockPayloadScalar,
    };

    let operation_label = "operation".to_string();
    let construction_payload = "payload".to_string();
    let name = FeatureBlockPayloadName {
        id: "name".to_string(),
        operation_label: operation_label.clone(),
        construction_payload: construction_payload.clone(),
        ordinal: 0,
        type_code: Some(131),
        payload_leading: false,
        value: "Point7".to_string(),
        payload_offset: 10,
        source_offset: 100,
    };
    let scalar = |id: &str, ordinal: u32, value: f64| FeatureBlockPayloadScalar {
        id: id.to_string(),
        operation_label: operation_label.clone(),
        construction_payload: construction_payload.clone(),
        ordinal,
        field_code: 100,
        value,
        payload_offset: 20 + u64::from(ordinal) * 13,
        source_offset: 110 + u64::from(ordinal) * 13,
    };
    let scalars = [scalar("first", 0, 1.25), scalar("second", 1, -2.5)];
    let record = FeatureBlockPayloadNamedRecord {
        id: "record".to_string(),
        operation_label,
        construction_payload,
        name_field: name.id.clone(),
        scalar_fields: scalars.iter().map(|scalar| scalar.id.clone()).collect(),
        payload_start_offset: 10,
        payload_end_offset: 50,
    };

    let points = feature_block_payload_points(
        std::slice::from_ref(&record),
        std::slice::from_ref(&name),
        &scalars,
    );
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].name, "Point7");
    assert_eq!(points[0].coordinates, [1.25, -2.5]);

    let mut duplicate = points[0].clone();
    duplicate.id = "point-2".to_string();
    let groups = feature_block_payload_point_groups(&[points[0].clone(), duplicate]);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].points.len(), 2);
    assert_eq!(groups[0].coordinates, [1.25, -2.5]);

    let mut conflicting = points[0].clone();
    conflicting.id = "conflicting".to_string();
    conflicting.coordinates[1] = f64::from_bits((-2.5_f64).to_bits() + 1);
    assert!(feature_block_payload_point_groups(&[points[0].clone(), conflicting]).is_empty());

    let mut incomplete = record.clone();
    incomplete.scalar_fields.pop();
    assert!(
        feature_block_payload_points(&[incomplete], std::slice::from_ref(&name), &scalars,)
            .is_empty()
    );
    let mut malformed = name;
    malformed.value = "Point0".to_string();
    assert!(feature_block_payload_points(&[record], &[malformed], &scalars).is_empty());
}

#[test]
fn om_boolean_operations_decode_counted_target_and_tools() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x0aSUBTRACT\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x5e\x00\x01\x05\x90\x19\x5f\x90\x19\x44\x90\x19\x43\x90\x19\x60\x00";
    let operations = crate::om::boolean_operations(bytes, 100);
    assert_eq!(operations.len(), 1);
    assert_eq!(
        operations[0].kind,
        crate::om::BooleanOperationKind::Subtract
    );
    assert_eq!(operations[0].target, 6494);
    assert_eq!(operations[0].tools, [6495, 6468, 6467, 6496]);

    let mut invalid = bytes.to_vec();
    *invalid.last_mut().unwrap() = 1;
    assert!(crate::om::boolean_operations(&invalid, 0).is_empty());
}

#[test]
fn om_index_accepts_length_framed_root_version_text() {
    let mut bytes = indexed_om_section();
    let marker = bytes
        .windows(b"\x04\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x04\x01\x0eNX 2027.3102\0")
        .expect("root record");
    bytes[marker + 2] = 0x0f;
    bytes.insert(marker + 3 + 12, b' ');
    let index = bytes
        .windows(4)
        .position(|window| window == 0u32.to_le_bytes())
        .expect("index");
    for ordinal in 2..4 {
        let at = index + ordinal * 4;
        let value = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) + 1;
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert!(sections[0].records[0]
        .bytes
        .starts_with(b"\x04\x01\x0fNX 2027.3102 \0"));
}

#[test]
fn om_store_version_can_follow_control_prefix() {
    let bytes = b"\xff\x00prefix\x04\x01\x0eNX 2027.3102\0tail";
    let version = crate::om::store_version(bytes, 100).expect("store version");
    assert_eq!(version.offset, 108);
    assert_eq!(version.value, "NX 2027.3102");
}

#[test]
fn om_offset_only_index_bounds_storage_blocks() {
    let bytes = offset_only_indexed_om_section();
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 0);
    assert_eq!(sections[0].control.as_ref().unwrap().bytes, &[0, 0, 0, 0]);
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(
        sections[0].column_storage.unwrap(),
        [sections[0].records[0].bytes, sections[0].records[1].bytes].concat()
    );
    assert_eq!(sections[0].records[0].object_id, None);
    assert!(sections[0].records[0].bytes.starts_with(b"\x04\x01\x0eNX "));
    assert_eq!(sections[0].records[1].object_id, None);
    assert!(sections[0].records[1].bytes.ends_with(b"\0"));
    let expressions = sections[0].numeric_expressions();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name, "length");
    assert_eq!(expressions[0].value, Some(25.0));
}

#[test]
fn om_offset_store_control_values_require_complete_zero_prefixed_words() {
    assert_eq!(
        crate::om::offset_store_control_values(&[0, 0x34, 0x12, 0, 0, 0xff, 0xff, 0xff]),
        Some(vec![0x1234, 0x00ff_ffff])
    );
    assert!(crate::om::offset_store_control_values(&[]).is_none());
    assert!(crate::om::offset_store_control_values(&[0, 1, 2]).is_none());
    assert!(crate::om::offset_store_control_values(&[1, 1, 2, 3]).is_none());
}

#[test]
fn om_offset_store_control_class_lane_is_a_distinct_in_range_prefix() {
    let encode = |values: &[u32]| {
        values
            .iter()
            .flat_map(|value| {
                let bytes = value.to_le_bytes();
                [0, bytes[0], bytes[1], bytes[2]]
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        crate::om::offset_store_control_class_ordinals(&encode(&[2, 0, 4, 8]), 4),
        Some(vec![2, 0])
    );
    assert!(crate::om::offset_store_control_class_ordinals(&encode(&[2, 2, 4]), 4).is_none());
    assert!(crate::om::offset_store_control_class_ordinals(&encode(&[2, 4, 1]), 4).is_none());
    assert!(crate::om::offset_store_control_class_ordinals(&encode(&[4, 8]), 4).is_none());
}

#[test]
fn om_offset_store_index_values_end_at_unique_aligned_product_record() {
    let mut bytes = vec![0, 0];
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&0x1020u32.to_le_bytes());
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
    assert_eq!(
        crate::om::offset_store_index_values(&bytes),
        Some((2, vec![7, 0x1020]))
    );

    let mut duplicate = bytes;
    duplicate.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(crate::om::offset_store_index_values(&duplicate).is_none());
    assert_eq!(
        crate::native::control_index_data_block(2, 700, 496).as_deref(),
        Some("nx:om-data-blocks-2:block#496")
    );
    assert!(crate::native::control_index_data_block(2, 700, 700).is_none());
}

#[test]
fn native_catalog_separates_offset_only_blocks_from_object_records() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = container::scan_bytes(file).unwrap();

    assert!(crate::native::object_records(&container).is_empty());
    let blocks = crate::native::data_blocks(&container);
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].block_ordinal, 0);
    assert_eq!(blocks[0].role, crate::native::DataBlockRole::Control);
    assert_eq!(blocks[1].role, crate::native::DataBlockRole::Column);
    assert!(blocks[0].byte_len > 0);
    let control_values = crate::native::data_block_control_values(&container);
    assert_eq!(control_values.len(), 1);
    assert_eq!(control_values[0].data_block, blocks[0].id);
    assert_eq!(control_values[0].ordinal, 0);
    assert_eq!(control_values[0].value, 0);
    assert!(crate::native::string_values(&container).is_empty());
    assert!(crate::native::object_references(&container).is_empty());
    let expressions = crate::native::expressions(&container);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, None);
    assert_eq!(expressions[0].record, None);
}

#[test]
fn native_abr_lane_resolves_nullable_slots_within_its_offset_store() {
    let mut store = offset_only_indexed_om_section();
    let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
    let end_at = index_start + 3 * 4;
    let end = u32::from_le_bytes(store[end_at..end_at + 4].try_into().unwrap()) as usize;
    let mut lane = vec![0x11, 0x02];
    lane.extend_from_slice(&[0xff; 15]);
    lane.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03]);
    store.splice(end..end, lane.iter().copied());
    store[end_at..end_at + 4].copy_from_slice(&((end + lane.len()) as u32).to_le_bytes());
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", store)]);
    let container = container::scan_bytes(file).unwrap();

    let lanes = crate::native::data_block_abr_reference_lanes(&container);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].slot_indices[0], Some(2));
    assert_eq!(
        lanes[0].slot_data_blocks[0].as_deref(),
        Some("nx:om-data-blocks-0:block#2")
    );
    assert!(lanes[0].slot_indices[1..].iter().all(Option::is_none));
    assert_eq!(lanes[0].slot_source_offsets.len(), 16);
    assert_eq!(lanes[0].slot_source_offsets[0], lanes[0].source_offset + 1);
}

#[test]
fn om_registry_uses_length_framing_and_stays_outside_entity_payloads() {
    let mut bytes = indexed_om_section();
    bytes.extend_from_slice(b"\x10UGS::PayloadText");
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].types.len(), 1);
    assert_eq!(sections[0].types[0].name, "UGS::EXP_expression");
    assert_eq!(sections[0].types[0].trailing_code, 0x81);
    assert_eq!(sections[0].types[0].offset, 8);
}

#[test]
fn om_numeric_expression_retains_identity_name_unit_and_value() {
    let bytes = indexed_om_section();
    let section = crate::om::indexed_sections(&bytes).remove(0);
    let expression_records = section.numeric_expression_records();
    assert_eq!(expression_records[0].0, 1);
    let expressions = expression_records
        .iter()
        .map(|(_, expression)| expression)
        .collect::<Vec<_>>();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, Some(0x102));
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].unit, crate::om::ExpressionUnit::Degree);
    assert_eq!(expressions[0].expression, "120");
    assert_eq!(expressions[0].value, Some(120.0));
    let declaration = crate::om::expression_declaration_name(section.records[1].bytes).unwrap();
    assert_eq!(
        declaration.value,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(declaration.parameter_index, 8);
    assert_eq!(
        declaration.qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(declaration.literal, Some("120"));
    let declaration =
        crate::om::expression_declaration_name(b"\x04\x04p1\0\x04\x0a-5.1 * 2\0").unwrap();
    assert_eq!(declaration.value, "p1");
    assert_eq!(declaration.literal, Some("-5.1 * 2"));
    let declaration =
        crate::om::expression_declaration_name(b"\x04\x04p1\0\x04\x055.1\0\x04\x05120\0").unwrap();
    assert_eq!(declaration.literal, None);
    assert!(crate::om::expression_declaration_name(b"\x04\x04p1\0\x04\x04p2\0").is_none());
    assert!(crate::om::expression_declaration_name(b"\x04\x05p1-\0").is_none());
}

#[test]
fn om_numeric_expression_retains_formula_without_literal_value() {
    let text = b"(Number [mm]) p9: p2 * 2 + p7_radius; ";
    let mut bytes = b"hostglobalvariables".to_vec();
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);

    let expressions = crate::om::numeric_expressions(&bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name, "p9");
    assert_eq!(expressions[0].expression, "p2 * 2 + p7_radius");
    assert_eq!(expressions[0].value, None);
    assert_eq!(
        crate::native::expression_parameter_names(expressions[0].expression),
        vec!["p2", "p7_radius"]
    );
}

#[test]
fn om_numeric_expression_evaluates_constant_arithmetic_formula() {
    let text = b"(Number [mm]) p9: (193.94 - 6) / 2 + 1.5e1; ";
    let mut bytes = b"hostglobalvariables".to_vec();
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);

    let expressions = crate::om::numeric_expressions(&bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].expression, "(193.94 - 6) / 2 + 1.5e1");
    assert_eq!(expressions[0].value, Some(108.97));
}

#[test]
fn om_string_value_requires_marker_length_printability_and_terminator() {
    let bytes = b"\x66\x32\x03\x0cSKETCH_001\0\x66\x32\x03\x03A\0\x66\x32\x03\x03A\x01";
    let values = crate::om::string_values(bytes, 100);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].offset, 100);
    assert_eq!(values[0].value, "SKETCH_001");
    assert_eq!(values[1].value, "A");
}

#[test]
fn om_tagged_references_preserve_family_value_order_and_bounds() {
    let bytes = b"\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\xe0\x01";
    let references = crate::om::references(bytes, 20);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 20);
    assert_eq!(
        references[0].kind,
        crate::om::ReferenceKind::PersistentHandle
    );
    assert_eq!(references[0].value, 0x1234_5678);
    assert_eq!(references[1].offset, 25);
    assert_eq!(references[1].kind, crate::om::ReferenceKind::Tagged28);
    assert_eq!(references[1].value, 0x0abc_def0);
}

#[test]
fn om_counted_record_references_require_a_complete_in_bounds_run() {
    let bytes = b"\xff\x01\x03\x90\x00\x02\x90\x00\x04\x01\x02\x90\x00\x05";
    let references = crate::om::counted_record_references(bytes, 100, 5);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 103);
    assert_eq!(
        references[0].kind,
        crate::om::ReferenceKind::RecordOrdinal16
    );
    assert_eq!(references[0].value, 2);
    assert_eq!(references[1].value, 4);
}

#[test]
fn om_record_reference_stream_requires_dense_suffix() {
    let mut dense = b"ordinary-prefix".to_vec();
    for value in 1..=8u32 {
        dense.push(0xe0);
        dense.extend_from_slice(&value.to_be_bytes());
        dense.extend_from_slice(&(0xc000_0000 | value).to_be_bytes());
    }
    let references = crate::om::dense_reference_suffix(&dense, 100);
    assert_eq!(references.len(), 16);
    assert_eq!(references[0].offset, 115);

    let mut sparse = dense;
    sparse.extend_from_slice(&[0x55; 9]);
    assert!(crate::om::dense_reference_suffix(&sparse, 0).is_empty());
}

#[test]
fn om_numeric_expression_table_is_independent_of_entity_indexing() {
    let bytes = b"hostglobalvariables\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00";
    let expressions = crate::om::numeric_expressions(bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, None);
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].value, Some(120.0));
}

/// A synthetic Parasolid partition stream: the `PS 00 00` header, a prologue with
/// a `(partition)` subtype and a schema token, then one POINT, one PLANE, one
/// CYLINDER, and one LINE record laid out back-to-back at their fixed lengths.
fn partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(b"XX: TRANSMIT FILE (partition) created by modeller version 3400176\x00");
    s.extend_from_slice(b"SCH_TEST_1_9999\x00");

    // POINT (type 29): xyz at +16, metres.
    let mut pt = record(0x1d, 40);
    put_vec3(&mut pt, 16, [0.0625, 0.0, 0.0127]); // 62.5, 0, 12.7 mm
    s.extend_from_slice(&pt);

    // PLANE (type 50): origin +19, normal +43, x_axis +67.
    let mut pl = record(0x32, 91);
    pl[18] = b'+';
    put_vec3(&mut pl, 19, [0.0762, 0.0, 0.0]); // 76.2 mm
    put_vec3(&mut pl, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut pl, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&pl);

    // CYLINDER (type 51): origin +19, axis +43, radius +67, x_axis +75.
    let mut cy = record(0x33, 99);
    cy[18] = b'+';
    put_vec3(&mut cy, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, 0.004_05); // 4.05 mm
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&cy);

    // LINE (type 30): point +19, direction +43.
    let mut ln = record(0x1e, 67);
    ln[18] = b'+';
    put_vec3(&mut ln, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut ln, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&ln);

    s
}

/// A complete one-face Parasolid topology. Every ownership and geometry link is
/// a small XMT reference, so this generated fixture exercises the codec's
/// connected-B-rep path without depending on an external CAD file.
fn topology_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(
        b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );

    let mut body = record(12, 24);
    put_ref(&mut body, 2, 2);
    s.extend_from_slice(&body);

    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 3);
    put_ref(&mut shell, 8, 1); // attributes
    put_ref(&mut shell, 10, 2); // body
    put_ref(&mut shell, 12, 1); // next shell
    put_ref(&mut shell, 14, 4); // first face
    put_ref(&mut shell, 16, 1); // sentinel
    put_ref(&mut shell, 18, 1); // sentinel
    put_ref(&mut shell, 20, 12); // region
    put_ref(&mut shell, 22, 1); // sentinel
    s.extend_from_slice(&shell);

    let mut face = record(14, 39);
    put_ref(&mut face, 2, 4);
    put_f64(&mut face, 10, 0.000_2); // 0.2 mm
    put_ref(&mut face, 18, 1); // next face
    put_ref(&mut face, 20, 1); // previous face
    put_ref(&mut face, 22, 5); // loop
    put_ref(&mut face, 24, 3); // shell
    put_ref(&mut face, 26, 6); // plane
    face[28] = b'+';
    s.extend_from_slice(&face);

    let mut loop_ = record(15, 16);
    put_ref(&mut loop_, 2, 5);
    put_ref(&mut loop_, 10, 7); // fin
    put_ref(&mut loop_, 12, 4); // face
    put_ref(&mut loop_, 14, 1); // next loop
    s.extend_from_slice(&loop_);

    let mut fin = record(17, 23);
    put_ref(&mut fin, 2, 7);
    put_ref(&mut fin, 6, 5); // loop
    put_ref(&mut fin, 8, 7); // next (one-fin ring)
    put_ref(&mut fin, 10, 7); // previous
    put_ref(&mut fin, 12, 10); // vertex
    put_ref(&mut fin, 14, 1); // no partner fin
    put_ref(&mut fin, 16, 8); // edge
    put_ref(&mut fin, 18, 9); // curve
    fin[22] = b'+';
    s.extend_from_slice(&fin);

    let mut edge = record(16, 32);
    put_ref(&mut edge, 2, 8);
    put_f64(&mut edge, 10, 0.000_3); // 0.3 mm
    put_ref(&mut edge, 18, 7); // fin
    put_ref(&mut edge, 24, 9); // curve
    s.extend_from_slice(&edge);

    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 6);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&plane);

    let mut line = record(30, 67);
    put_ref(&mut line, 2, 9);
    line[18] = b'+';
    put_vec3(&mut line, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut line, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&line);

    let mut vertex = record(18, 28);
    put_ref(&mut vertex, 2, 10);
    put_ref(&mut vertex, 16, 11); // point
    put_f64(&mut vertex, 18, 0.000_1); // 0.1 mm
    s.extend_from_slice(&vertex);

    let mut region = record(19, 16);
    put_ref(&mut region, 2, 12);
    s.extend_from_slice(&region);

    let mut point = record(29, 40);
    put_ref(&mut point, 2, 11);
    put_vec3(&mut point, 16, [0.01, 0.02, 0.03]);
    s.extend_from_slice(&point);
    s
}

#[test]
fn topology_retains_entity_attribute_list_references() {
    let mut stream = topology_partition_stream();
    for (kind, attribute) in [(14, 41), (15, 42), (17, 43), (16, 44), (18, 45)] {
        let at = stream
            .windows(2)
            .position(|window| window == [0, kind])
            .expect("topology record");
        put_ref(&mut stream, at + if kind == 17 { 4 } else { 8 }, attribute);
    }
    stream.extend_from_slice(&[0, 0x51]);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&41u16.to_be_bytes());
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in [4u16, 1, 1, 1, 1, 42] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.extend_from_slice(&[0, 0x54]);
    stream.extend_from_slice(&8u32.to_be_bytes());
    stream.extend_from_slice(&42u16.to_be_bytes());
    stream.extend_from_slice(b"deadbeef\0");

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(14, 4).unwrap().face_fields().unwrap().attributes,
        41
    );
    assert_eq!(
        graph.get(15, 5).unwrap().loop_fields().unwrap().attributes,
        42
    );
    assert_eq!(
        graph.get(17, 7).unwrap().fin_fields().unwrap().attributes,
        43
    );
    assert_eq!(
        graph.get(16, 8).unwrap().edge_fields().unwrap().attributes,
        44
    );
    assert_eq!(
        graph
            .get(18, 10)
            .unwrap()
            .vertex_fields()
            .unwrap()
            .attributes,
        45
    );

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_partition(&stream)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let references = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidTopologyAttributeListReference>(
            "parasolid_topology_attribute_list_references",
        )
        .unwrap();
    assert_eq!(references.len(), 5);
    assert_eq!(references[0].topology_type, 14);
    assert_eq!(references[0].topology_xmt, 4);
    assert_eq!(references[0].attribute_list_xmt, 41);
    assert!(references[0].attribute_list_record.is_some());
    assert_eq!(result.ir.model.attributes.len(), 1);
    assert_eq!(
        result.ir.model.attributes[0].target,
        cadmpeg_ir::attributes::AttributeTarget::Face(cadmpeg_ir::ids::FaceId(
            "nx:s0:face#4".into()
        ))
    );
    assert_eq!(
        result.ir.model.attributes[0].name,
        "parasolid_type_84_reference_5"
    );
    assert_eq!(
        result.ir.model.attributes[0].values,
        [cadmpeg_ir::attributes::AttributeValue::String(
            "deadbeef".into()
        )]
    );
}

#[test]
fn parasolid_entity_51_records_retain_layout_selected_references() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in 3..=8u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes.extend_from_slice(&[0xaa, 0xbb]);

    let records = crate::parasolid::entity_51_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 0);
    assert_eq!(records[0].byte_len, 26);
    assert_eq!(records[0].xmt, 10);
    assert_eq!(records[0].sequence, 2);
    assert_eq!(records[0].discriminator, 0x21);
    assert_eq!(records[0].references, vec![3, 4, 5, 6, 7, 8]);
}

#[test]
fn parasolid_entity_54_strings_require_exact_length_and_terminator() {
    let mut bytes = vec![0xaa, 0x00, 0x54];
    bytes.extend_from_slice(&8u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(b"deadbeef\0");
    bytes.extend_from_slice(&[0xbb, 0x00, 0x54, 0, 0, 0, 3, 0, 18, b'a', b'b', b'c', 1]);

    let records = crate::parasolid::entity_54_string_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].byte_len, 17);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].value, "deadbeef");
}

#[test]
fn parasolid_entity_52_integers_require_complete_counted_values() {
    let mut bytes = vec![0xaa, 0x00, 0x52];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());

    let records = crate::parasolid::entity_52_integer_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].values, [3, u32::MAX]);
    assert_eq!(records[0].byte_len, 16);
    assert!(crate::parasolid::entity_52_integer_records(&bytes[..bytes.len() - 1]).is_empty());
}

#[test]
fn parasolid_entity_53_doubles_require_complete_finite_values() {
    let mut bytes = vec![0xaa, 0x00, 0x53, 0xff];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&18u16.to_be_bytes());
    bytes.extend_from_slice(&0.001f64.to_be_bytes());
    bytes.extend_from_slice(&0.25f64.to_be_bytes());

    let records = crate::parasolid::entity_53_double_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 18);
    assert_eq!(records[0].values, [0.001, 0.25]);
    assert_eq!(records[0].byte_len, 25);

    let last = bytes.len() - 8;
    bytes[last..].copy_from_slice(&f64::NAN.to_be_bytes());
    assert!(crate::parasolid::entity_53_double_records(&bytes).is_empty());
}

#[test]
fn topology_attribute_class_uses_resolve_one_based_stream_catalog_indices() {
    use crate::native::{
        ParasolidAttributeDefinition, ParasolidEntity51Record,
        ParasolidTopologyAttributeListReference,
    };

    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: 18,
        name: "UG2/PMARK_ATTRIBUTE".into(),
        field_count: 1,
        field_record_xmt: 19,
        field_record_references: [21, 22],
        field_record_header_words: [0, 9000],
        field_descriptor_prefix: [0; 26],
        field_codes: vec![1],
        inflated_offset: 100,
    };
    let entity = ParasolidEntity51Record {
        id: "entity".into(),
        stream_ordinal: 3,
        xmt: 50,
        flags: 1,
        sequence: 7,
        discriminator: 0x21,
        references: vec![60, 61, 1, 62, 63, 64],
        byte_len: 26,
        inflated_offset: 200,
    };
    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: 14,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some(entity.id.clone()),
        inflated_offset: 300,
    };

    let uses = crate::native::parasolid_topology_attribute_class_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&entity),
        std::slice::from_ref(&definition),
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].definition_ordinal, 1);
    assert_eq!(uses[0].attribute_definition, definition.id);

    let mut invalid = entity;
    invalid.references[2] = 0;
    assert!(crate::native::parasolid_topology_attribute_class_uses(
        &[reference],
        &[invalid],
        &[definition],
    )
    .is_empty());
}

#[test]
fn topology_numeric_attribute_values_transfer_in_native_lane_order() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use cadmpeg_ir::ids::{FaceId, LoopId, ShellId};
    use cadmpeg_ir::AnnotationBuilder;

    use crate::native::{
        ParasolidEntity51NumericKind, ParasolidEntity51NumericUse, ParasolidEntity52IntegerRecord,
        ParasolidEntity53DoubleRecord, ParasolidTopologyAttributeListReference,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.shells[0].id = ShellId("nx:s3:shell#58".into());
    ir.model.faces[0].id = FaceId("nx:s3:face#60".into());
    ir.model.loops[0].id = LoopId("nx:s3:loop#59".into());
    let references = [(13, 58), (14, 60), (15, 59)].map(|(topology_type, topology_xmt)| {
        ParasolidTopologyAttributeListReference {
            id: format!("topology-reference-{topology_type}"),
            stream_ordinal: 3,
            topology_type,
            topology_xmt,
            attribute_list_xmt: 50,
            attribute_list_record: Some("entity".into()),
            inflated_offset: 300,
        }
    });
    let integer = ParasolidEntity52IntegerRecord {
        id: "integers".into(),
        stream_ordinal: 3,
        xmt: 70,
        values: vec![4, u32::MAX],
        byte_len: 18,
        inflated_offset: 400,
    };
    let double = ParasolidEntity53DoubleRecord {
        id: "doubles".into(),
        stream_ordinal: 3,
        xmt: 71,
        values: vec![0.25, 7.5],
        byte_len: 26,
        inflated_offset: 500,
    };
    let uses = [
        ParasolidEntity51NumericUse {
            id: "double-use".into(),
            stream_ordinal: 3,
            entity_51_record: "entity".into(),
            reference_ordinal: 4,
            referenced_xmt: 71,
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: double.id.clone(),
            inflated_offset: 200,
        },
        ParasolidEntity51NumericUse {
            id: "integer-use".into(),
            stream_ordinal: 3,
            entity_51_record: "entity".into(),
            reference_ordinal: 3,
            referenced_xmt: 70,
            kind: ParasolidEntity51NumericKind::UnsignedIntegers,
            value_record: integer.id.clone(),
            inflated_offset: 200,
        },
    ];
    let mut annotations = AnnotationBuilder::new();

    crate::decode::attach_parasolid_topology_numeric_attributes(
        &mut ir,
        &crate::decode::ParasolidNumericAttributeSources {
            topology_references: &references,
            numeric_uses: &uses,
            integers: &[integer],
            doubles: &[double],
        },
        &mut annotations,
    );

    let attributes = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.id.0.contains("topology-numeric-attribute"))
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 6);
    assert_eq!(
        attributes[0].target,
        AttributeTarget::Shell(ShellId("nx:s3:shell#58".into()))
    );
    assert_eq!(attributes[0].name, "parasolid_type_integer_reference_3");
    assert_eq!(
        attributes[0].values,
        [
            AttributeValue::Integer(4),
            AttributeValue::Integer(i64::from(u32::MAX))
        ]
    );
    for (attributes, target) in [
        (
            &attributes[0..2],
            AttributeTarget::Shell(ShellId("nx:s3:shell#58".into())),
        ),
        (
            &attributes[2..4],
            AttributeTarget::Face(FaceId("nx:s3:face#60".into())),
        ),
        (
            &attributes[4..6],
            AttributeTarget::Loop(LoopId("nx:s3:loop#59".into())),
        ),
    ] {
        assert!(attributes
            .iter()
            .all(|attribute| attribute.target == target));
        assert_eq!(
            attributes[1].values,
            [AttributeValue::Float(0.25), AttributeValue::Float(7.5)]
        );
    }
}

#[test]
fn topology_rejects_shell_with_broken_face_ownership_chain() {
    let valid = topology_partition_stream();
    let graph = crate::topology::Graph::parse(&valid);
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut broken = valid;
    let face = broken
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut broken, face + 24, 99);
    assert!(crate::topology::Graph::parse(&broken)
        .body_shape_shells()
        .is_empty());

    let mut independent_previous = topology_partition_stream();
    let face = independent_previous
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut independent_previous, face + 20, 99);
    assert_eq!(
        crate::topology::Graph::parse(&independent_previous)
            .body_shape_shells()
            .len(),
        1
    );
}

#[test]
fn topology_retains_shell_body_identity_without_body_record() {
    let mut stream = topology_partition_stream();
    let body = stream
        .windows(4)
        .position(|window| window == [0, 12, 0, 2])
        .expect("body record");
    stream[body..body + 24].fill(0xff);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(12, 2).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.bodies[0].id.0, "nx:s0:body#2");
    assert_eq!(result.ir.model.faces.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn topology_accepts_cached_last_face_and_implicit_region_identity() {
    let mut stream = topology_partition_stream();
    let shell = stream
        .windows(4)
        .position(|window| window == [0, 13, 0, 3])
        .expect("shell record");
    put_ref(&mut stream, shell + 22, 4);
    let region = stream
        .windows(4)
        .position(|window| window == [0, 19, 0, 12])
        .expect("region record");
    stream[region..region + 16].fill(0xff);
    let mut second_face = record(14, 39);
    put_ref(&mut second_face, 2, 20);
    put_f64(&mut second_face, 10, 0.000_2);
    put_ref(&mut second_face, 18, 1);
    put_ref(&mut second_face, 20, 1);
    put_ref(&mut second_face, 22, 1);
    put_ref(&mut second_face, 24, 3);
    put_ref(&mut second_face, 26, 6);
    second_face[28] = b'+';
    stream.extend(second_face);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(19, 12).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);
    assert_eq!(graph.body_shape_face_count(), 2);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.regions[0].id.0, "nx:s0:region#12");
    assert_eq!(result.ir.model.faces.len(), 2);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn topology_rejects_nonreciprocal_fin_ring() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 8, 99);
    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.face_loop_rings(4).is_none());

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.loops.is_empty());
    assert!(result.ir.model.coedges.is_empty());
    assert!(result.ir.model.edges.is_empty());

    let mut broken_partner = topology_partition_stream();
    let fin = broken_partner
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut broken_partner, fin + 14, 99);
    assert!(crate::topology::Graph::parse(&broken_partner)
        .face_loop_rings(4)
        .is_none());
}

#[test]
fn topology_accepts_fixed_record_envelope_escape() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    stream.insert(fin + 2, 0xff);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(17, 7).unwrap().attribute_field_offset(),
        Some(fin + 5)
    );
    assert_eq!(graph.face_loop_rings(4).unwrap().len(), 1);
}

#[test]
fn decode_synthesizes_vertex_for_closed_null_vertex_fin() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 12, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = result.ir.model.edges.first().expect("closed edge");
    assert_eq!(edge.start, edge.end);
    assert!(edge.start.0.contains("closed-edge"));
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn topology_invalid_candidate_cannot_shadow_later_valid_record() {
    let mut stream = record(14, 39);
    put_ref(&mut stream, 2, 4);
    stream.extend(topology_partition_stream());

    let graph = crate::topology::Graph::parse(&stream);
    let face = graph.get(14, 4).expect("valid later FACE");
    assert!(face.pos >= 39);
    assert!(face.face_fields().is_some());
}

#[test]
fn decode_retains_topology_owned_point_at_origin() {
    let mut stream = topology_partition_stream();
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [0.0, 0.0, 0.0]);

    assert!(crate::geometry::points(&stream).is_empty());
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph
            .get(29, 11)
            .and_then(crate::topology::Node::point_position),
        Some(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.bodies[0].transform, None);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn decode_does_not_attach_unreferenced_point_to_solid_topology() {
    let mut stream = topology_partition_stream();
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 77);
    put_vec3(&mut point, 16, [0.04, 0.05, 0.06]);
    stream.extend_from_slice(&point);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.shells[0].free_vertices.len(), 0);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_connected_topology_with_unknown_surface_carrier() {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut stream, face + 26, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == result.ir.model.faces[0].surface)
        .expect("unknown face carrier");
    assert!(matches!(surface.geometry, SurfaceGeometry::Unknown { .. }));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_retains_unknown_non_null_edge_curve_carrier() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let curve = result.ir.model.edges[0]
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| &curve.id == id))
        .expect("unknown edge carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_drops_unknown_carrier_outside_emitted_topology() {
    let mut stream = topology_partition_stream();
    let mut orphan = record(16, 32);
    put_ref(&mut orphan, 2, 88);
    put_f64(&mut orphan, 10, 0.000_3);
    put_ref(&mut orphan, 18, 1);
    put_ref(&mut orphan, 24, 99);
    stream.extend(orphan);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert!(result
        .ir
        .model
        .curves
        .iter()
        .all(|curve| !matches!(curve.geometry, CurveGeometry::Unknown { .. })));
    assert_eq!(result.ir.model.edges.len(), 1);
}

#[test]
fn decode_retains_native_carrierless_edge() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let fin = stream
        .windows(2)
        .position(|window| window == [0, 17])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = &result.ir.model.edges[0];
    assert_eq!(edge.curve, None);
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn tolerant_edge_becomes_a_two_support_procedural_intersection() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge_id = ir.model.edges[0].id.clone();
    ir.model.edges[0].curve = None;
    ir.model.edges[0].param_range = None;
    ir.model.edges[0].tolerance = Some(0.01);
    let mut edges = std::collections::BTreeMap::new();
    edges.insert(12, edge_id.clone());
    let graph = crate::topology::Graph::parse(&[]);
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let stream = annotations.stream("nx:test");

    crate::decode::attach_tolerant_edge_intersections(
        &mut ir,
        &graph,
        &edges,
        "nx:test",
        stream,
        &mut annotations,
    );

    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("tolerant edge");
    assert_eq!(edge.param_range, Some([0.0, 1.0]));
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve.as_ref())
        .expect("procedural carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Procedural { .. }));
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == curve.id)
        .expect("intersection construction");
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &procedural.definition
    else {
        panic!("intersection definition");
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    assert_ne!(context.sides[0].surface, context.sides[1].surface);
}

#[test]
fn intersection_support_completion_requires_one_unique_incident_complement() {
    use cadmpeg_ir::geometry::{
        IntcurveSupportContext, IntcurveSupportSide, Pcurve, ProceduralCurve,
    };
    use cadmpeg_ir::ids::{PcurveId, ProceduralCurveId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = ir.model.edges[0].clone();
    let incident = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == edge.id)
        .filter_map(|coedge| {
            let face = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?
                .face
                .clone();
            ir.model
                .faces
                .iter()
                .find(|candidate| candidate.id == face)
                .map(|face| face.surface.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(incident.len(), 2);
    let curve = edge.curve.expect("cube edge curve");
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("nx:test:intersection#0".into()),
        curve,
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(incident[0].clone()),
                        pcurve: None,
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });

    crate::decode::complete_intersection_supports_from_edge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].surface.as_ref(), Some(&incident[1]));

    let pcurve_id = PcurveId("nx:test:pcurve#0".into());
    let pcurve_geometry = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    ir.model.pcurves.push(Pcurve {
        id: pcurve_id.clone(),
        geometry: pcurve_geometry.clone(),
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some([0.0, 1.0]),
        fit_tolerance: None,
    });
    let second_face = ir
        .model
        .faces
        .iter()
        .find(|face| face.surface == incident[1])
        .expect("second incident face")
        .id
        .clone();
    let second_loop = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.face == second_face)
        .expect("second incident loop")
        .id
        .clone();
    ir.model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge.id && coedge.owner_loop == second_loop)
        .expect("second incident coedge")
        .pcurve = Some(pcurve_id);

    crate::decode::complete_intersection_pcurves_from_coedge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].pcurve.as_ref(), Some(&pcurve_geometry));
}

#[test]
fn decode_attaches_dimension_two_bcurve_through_surface_curve() {
    let stream = pcurve_topology_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result.ir.model.coedges[0].pcurve.as_ref(),
        Some(&result.ir.model.pcurves[0].id)
    );
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &result.ir.model.pcurves[0].geometry
    else {
        panic!("expected NURBS pcurve");
    };
    assert_eq!(*degree, 1);
    assert_eq!(knots, &[0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        control_points,
        &[Point2::new(10.0, 20.0), Point2::new(10.0, 20.0)]
    );
    assert!(weights.is_none());
    assert!(!periodic);
    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, Some(0.01));
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 0.0)
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.findings.is_empty(),
        "findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_omits_surface_curve_missing_tolerance_sentinel() {
    let mut stream = pcurve_topology_partition_stream();
    let surface_curve = stream
        .windows(2)
        .position(|window| window == [0, 137])
        .expect("surface curve");
    put_f64(
        &mut stream,
        surface_curve + 25,
        crate::decode::MISSING_TOLERANCE,
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_multiple_shells_in_one_region() {
    let stream = shared_region_shells_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 2);
    assert_eq!(result.ir.model.regions[0].shells.len(), 2);
    assert_eq!(result.ir.model.bodies[0].regions.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

fn offset_surface_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    put_ref(&mut stream, face + 26, 12);

    let mut offset = record(60, 39);
    put_ref(&mut offset, 2, 12);
    offset[18] = b'+';
    offset[19] = b'V';
    offset[20] = 1;
    put_ref(&mut offset, 21, 6);
    put_f64(&mut offset, 23, 0.002_5);
    stream.extend(offset);
    stream
}

fn surface_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 12);
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 9);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_01);
    stream.extend(surface_curve);
    stream
}

fn pcurve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 25);
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [0.01, 0.02, 0.0]);

    let mut wrapper = record(134, 23);
    put_ref(&mut wrapper, 2, 20);
    wrapper[18] = b'+';
    put_ref(&mut wrapper, 19, 21);
    put_ref(&mut wrapper, 21, 22);
    stream.extend(wrapper);

    let mut descriptor = record(136, 27);
    put_ref(&mut descriptor, 2, 21);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 2);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    put_ref(&mut descriptor, 23, 23);
    put_ref(&mut descriptor, 25, 24);
    stream.extend(descriptor);

    let mut payload = record(135, 15 + 4 * 8);
    put_ref(&mut payload, 2, 22);
    payload[9..13].copy_from_slice(&4u32.to_be_bytes());
    for (index, value) in [0.01, 0.02, 0.01, 0.02].into_iter().enumerate() {
        put_f64(&mut payload, 15 + index * 8, value);
    }
    stream.extend(payload);

    let mut multiplicities = record(127, 12);
    multiplicities[4..6].copy_from_slice(&2u16.to_be_bytes());
    put_ref(&mut multiplicities, 6, 23);
    put_ref(&mut multiplicities, 8, 2);
    put_ref(&mut multiplicities, 10, 2);
    stream.extend(multiplicities);

    let mut knots = record(128, 24);
    knots[4..6].copy_from_slice(&2u16.to_be_bytes());
    put_ref(&mut knots, 6, 24);
    put_f64(&mut knots, 8, 0.0);
    put_f64(&mut knots, 16, 1.0);
    stream.extend(knots);

    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 25);
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 20);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_01);
    stream.extend(surface_curve);
    stream
}

fn shared_region_shells_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 13);
    for (offset, reference) in [
        (8, 1),
        (10, 2),
        (12, 1),
        (14, 14),
        (16, 1),
        (18, 1),
        (20, 12),
        (22, 1),
    ] {
        put_ref(&mut shell, offset, reference);
    }
    stream.extend(shell);

    let mut face = record(14, 39);
    put_ref(&mut face, 2, 14);
    put_f64(&mut face, 10, 0.000_2);
    put_ref(&mut face, 18, 1);
    put_ref(&mut face, 20, 1);
    put_ref(&mut face, 22, 15);
    put_ref(&mut face, 24, 13);
    put_ref(&mut face, 26, 6);
    face[28] = b'+';
    stream.extend(face);

    let mut loop_ = record(15, 16);
    put_ref(&mut loop_, 2, 15);
    put_ref(&mut loop_, 10, 16);
    put_ref(&mut loop_, 12, 14);
    put_ref(&mut loop_, 14, 1);
    stream.extend(loop_);

    let mut fin = record(17, 23);
    put_ref(&mut fin, 2, 16);
    put_ref(&mut fin, 6, 15);
    put_ref(&mut fin, 8, 16);
    put_ref(&mut fin, 10, 16);
    put_ref(&mut fin, 12, 10);
    put_ref(&mut fin, 14, 1);
    put_ref(&mut fin, 16, 17);
    put_ref(&mut fin, 18, 9);
    fin[22] = b'+';
    stream.extend(fin);

    let mut edge = record(16, 32);
    put_ref(&mut edge, 2, 17);
    put_f64(&mut edge, 10, 0.000_3);
    put_ref(&mut edge, 18, 16);
    put_ref(&mut edge, 24, 9);
    stream.extend(edge);
    stream
}

fn blend_surface_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    put_ref(&mut stream, face + 26, 12);

    let mut blend = record(56, 66);
    put_ref(&mut blend, 2, 12);
    blend[18] = b'+';
    blend[19] = b'R';
    put_ref(&mut blend, 20, 6);
    put_ref(&mut blend, 22, 6);
    put_ref(&mut blend, 24, 1);
    put_f64(&mut blend, 26, -0.003);
    put_f64(&mut blend, 34, 0.003);
    put_f64(&mut blend, 42, 1.0);
    put_f64(&mut blend, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut blend, at, 1);
    }
    stream.extend(blend);
    stream
}

fn blend_surface_with_extended_support_reference() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    stream.splice(blend + 20..blend + 22, [0xff, 0xfa, 0x00, 0x00]);
    stream
}

fn blend_surface_with_intersection_spine() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    put_ref(&mut stream, blend + 24, 18);

    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 18);
    put_ref(&mut intersection, 8, 1);
    intersection[18] = b'+';
    for (index, reference) in [6, 6, 1, 1, 1, 1].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);
    stream
}

fn blend_surface_with_forward_blend_support() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("first blend record");
    put_ref(&mut stream, first + 20, 20);

    let mut second = record(56, 66);
    put_ref(&mut second, 2, 20);
    second[18] = b'+';
    second[19] = b'R';
    put_ref(&mut second, 20, 6);
    put_ref(&mut second, 22, 6);
    put_ref(&mut second, 24, 1);
    put_f64(&mut second, 26, -0.003);
    put_f64(&mut second, 34, 0.003);
    put_f64(&mut second, 42, 1.0);
    put_f64(&mut second, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut second, at, 1);
    }
    stream.extend(second);
    stream
}

fn intersection_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 12);
    put_ref(&mut intersection, 8, 1);
    intersection[18] = b'+';
    for (index, reference) in [6, 6, 1, 1, 1, 1].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);
    stream
}

fn charted_intersection_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }

    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 12);
    put_ref(&mut intersection, 8, 1);
    intersection[18] = b'+';
    for (index, reference) in [6, 1, 20, 21, 22, 23].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);

    let mut chart = record(40, 108);
    chart[2..6].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut chart, 6, 20);
    put_f64(&mut chart, 8, 0.0);
    put_f64(&mut chart, 16, 1.0);
    chart[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_f64(&mut chart, 28, 0.000_01);
    put_f64(&mut chart, 36, 0.001);
    put_f64(&mut chart, 44, -31_415_800_000_000.0);
    put_f64(&mut chart, 52, -31_415_800_000_000.0);
    put_vec3(&mut chart, 60, [0.0, 0.0, 0.0]);
    put_vec3(&mut chart, 84, [0.01, 0.0, 0.0]);
    stream.extend(chart);

    for (xmt, point) in [(21, [0.0, 0.0, 0.0]), (22, [0.01, 0.0, 0.0])] {
        let mut term = record(41, 34);
        term[2..6].copy_from_slice(&1u32.to_be_bytes());
        put_ref(&mut term, 6, xmt);
        term[8..10].copy_from_slice(b"L?");
        put_vec3(&mut term, 10, point);
        stream.extend(term);
    }

    let mut uv = record(204, 41);
    uv[2..6].copy_from_slice(&4u32.to_be_bytes());
    put_ref(&mut uv, 6, 23);
    uv[8] = 2;
    for (index, value) in [0.0, 0.0, 0.01, 0.0].into_iter().enumerate() {
        put_f64(&mut uv, 9 + index * 8, value);
    }
    stream.extend(uv);
    stream
}

fn charted_intersection_with_edge_endpoint_witnesses_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let first_fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("first fin record");
    put_ref(&mut stream, first_fin + 8, 13);
    put_ref(&mut stream, first_fin + 10, 13);
    let first_point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("first point record");
    put_vec3(&mut stream, first_point + 16, [0.0, 0.0, 0.0]);

    let mut second_fin = record(17, 23);
    put_ref(&mut second_fin, 2, 13);
    put_ref(&mut second_fin, 6, 5);
    put_ref(&mut second_fin, 8, 7);
    put_ref(&mut second_fin, 10, 7);
    put_ref(&mut second_fin, 12, 14);
    put_ref(&mut second_fin, 14, 1);
    put_ref(&mut second_fin, 16, 8);
    put_ref(&mut second_fin, 18, 12);
    second_fin[22] = b'+';
    stream.extend(second_fin);

    let mut second_vertex = record(18, 28);
    put_ref(&mut second_vertex, 2, 14);
    put_ref(&mut second_vertex, 16, 15);
    put_f64(&mut second_vertex, 18, 0.000_1);
    stream.extend(second_vertex);

    let mut second_point = record(29, 40);
    put_ref(&mut second_point, 2, 15);
    put_vec3(&mut second_point, 16, [0.01, 0.0, 0.0]);
    stream.extend(second_point);
    stream
}

fn charted_intersection_without_uv_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 29, 1);
    stream
}

fn charted_intersection_with_approximated_term_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let end = stream
        .windows(8)
        .position(|window| window == [0, 41, 0, 0, 0, 1, 0, 22])
        .expect("end term record");
    put_f64(&mut stream, end + 10, 0.010_005);
    stream
}

fn ext11_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    let mut entries = vec![0u8; 2 * 11 * 8];
    for (index, point) in [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0]].into_iter().enumerate() {
        let at = index * 88;
        put_vec3(&mut entries, at, point);
        put_vec3(&mut entries, at + 56, [1.0, 0.0, 0.0]);
        put_f64(&mut entries, at + 80, [2.0, 5.0][index]);
    }
    stream.splice(chart + 60..chart + 108, entries);
    stream
}

fn two_support_ext11_charted_intersection_curve_stream(ambiguous: bool) -> Vec<u8> {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 29, 1);

    let second_plane = stream
        .windows(4)
        .position(|window| window == [0, 50, 0, 13])
        .expect("second plane");
    if !ambiguous {
        put_vec3(&mut stream, second_plane + 67, [0.0, 0.0, 1.0]);
    }

    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    let mut entries = vec![0u8; 2 * 11 * 8];
    for (index, x) in [0.0, 0.01].into_iter().enumerate() {
        let at = index * 88;
        put_vec3(&mut entries, at, [x, 0.0, 0.0]);
        let second = if ambiguous { [x, 0.0] } else { [0.0, x] };
        put_f64(&mut entries, at + 24, x);
        put_f64(&mut entries, at + 32, second[0]);
        put_f64(&mut entries, at + 40, 0.0);
        put_f64(&mut entries, at + 48, second[1]);
        put_vec3(&mut entries, at + 56, [1.0, 0.0, 0.0]);
        put_f64(&mut entries, at + 80, x);
    }
    stream.splice(chart + 60..chart + 108, entries);
    stream
}

fn partial_ext11_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = two_support_ext11_charted_intersection_curve_stream(false);
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    for index in 0..2 {
        put_f64(
            &mut stream,
            chart + 60 + index * 88 + 32,
            -31_415_800_000_000.0,
        );
    }
    stream
}

fn two_support_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);

    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 4, 0, 23])
        .expect("UV record");
    stream[uv + 2..uv + 6].copy_from_slice(&8u32.to_be_bytes());
    stream[uv + 8] = 4;
    let mut values = vec![0u8; 8 * 8];
    for (index, value) in [0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.01, 0.0]
        .into_iter()
        .enumerate()
    {
        put_f64(&mut values, index * 8, value);
    }
    stream.splice(uv + 9..uv + 41, values);

    let mut second_plane = record(50, 91);
    put_ref(&mut second_plane, 2, 13);
    second_plane[18] = b'+';
    put_vec3(&mut second_plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut second_plane, 43, [0.0, 1.0, 0.0]);
    put_vec3(&mut second_plane, 67, [1.0, 0.0, 0.0]);
    stream.extend(second_plane);
    stream
}

fn blend_bound_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 14);

    let mut bridge = record(59, 24);
    put_ref(&mut bridge, 2, 14);
    bridge[4..8].copy_from_slice(&9u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut bridge, at, 1);
    }
    bridge[18] = b'+';
    put_ref(&mut bridge, 19, 0);
    put_ref(&mut bridge, 21, 13);
    stream.extend(bridge);
    stream
}

fn inline_descriptor_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 4, 0, 23])
        .expect("UV record");
    let mut inline_uv = b"values\x00\x00\x00\x02\x01\x66\x01".to_vec();
    inline_uv.extend_from_slice(&4u32.to_be_bytes());
    inline_uv.extend_from_slice(&23u16.to_be_bytes());
    inline_uv.push(2);
    for value in [0.0_f64, 0.0, 0.01, 0.0] {
        inline_uv.extend_from_slice(&value.to_be_bytes());
    }
    stream.splice(uv..uv + 41, inline_uv);

    for (xmt, point) in [(22u16, [0.01_f64, 0.0, 0.0]), (21, [0.0, 0.0, 0.0])] {
        let marker = [0, 41, 0, 0, 0, 1, (xmt >> 8) as u8, xmt as u8];
        let term = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("term record");
        let mut inline = b"term_use\x00\x00\x00\x01\x01\x63\x43\x5a".to_vec();
        inline.extend_from_slice(&1u32.to_be_bytes());
        inline.extend_from_slice(&xmt.to_be_bytes());
        inline.extend_from_slice(b"L?");
        for coordinate in point {
            inline.extend_from_slice(&coordinate.to_be_bytes());
        }
        stream.splice(term..term + 34, inline);
    }
    stream
}

fn deltas_intersection_curve_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let subtype = stream
        .windows(b"(partition)".len())
        .position(|window| window == b"(partition)")
        .expect("partition subtype");
    stream.splice(
        subtype..subtype + b"(partition)".len(),
        b"(deltas)".iter().copied(),
    );
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    stream.extend_from_slice(b"intersection_data");
    stream.push(0x5a);
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream
}

fn status_framed_deltas_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    let mut face = Vec::new();
    face.extend_from_slice(&14u16.to_be_bytes());
    face.extend_from_slice(&100u16.to_be_bytes());
    face.extend_from_slice(&7u32.to_be_bytes());
    let push_ref = |record: &mut Vec<u8>, reference: u16| {
        record.extend_from_slice(&reference.to_be_bytes());
        record.push(1);
    };
    push_ref(&mut face, 1);
    face.extend_from_slice(&(-31_415_800_000_000.0f64).to_be_bytes());
    for reference in [1u16; 5] {
        push_ref(&mut face, reference);
    }
    face.push(b'+');
    for reference in [1u16; 5] {
        push_ref(&mut face, reference);
    }
    stream.extend(face);
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&50_000u16.to_be_bytes());
    stream.extend_from_slice(&[0, 1]);
    stream
}

fn variable_status_framed_deltas_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&(-100i16).to_be_bytes());
    stream.extend_from_slice(&0u16.to_be_bytes());
    stream.extend_from_slice(&8u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&101u16.to_be_bytes());
    stream.extend_from_slice(&9u32.to_be_bytes());
    for reference in [1u16, 2] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn status_framed_deltas_point_stream() -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(&29u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&900u32.to_be_bytes());
    for reference in [1u16; 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    for value in [0.0125f64, -0.002, 0.004] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn status_framed_deltas_intersection_stream() -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(&38u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&901u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [6u16, 7, 20, 21, 22, 23] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_point_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend(status_framed_deltas_point_stream());
    stream
}

fn deltas_edge_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&8u16.to_be_bytes());
    stream.extend_from_slice(&901u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_9f64.to_be_bytes());
    for reference in [7u16, 1, 1, 9, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_face_vertex_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&14u16.to_be_bytes());
    stream.extend_from_slice(&4u16.to_be_bytes());
    stream.extend_from_slice(&902u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_8f64.to_be_bytes());
    for reference in [1u16, 1, 5, 3, 6] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }

    stream.extend_from_slice(&18u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&903u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 11] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&0.000_7f64.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream
}

fn deltas_loop_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&5u16.to_be_bytes());
    stream.extend_from_slice(&904u32.to_be_bytes());
    for reference in [1u16, 7, 4, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_shell_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&13u16.to_be_bytes());
    stream.extend_from_slice(&3u16.to_be_bytes());
    stream.extend_from_slice(&905u32.to_be_bytes());
    for reference in [1u16, 2, 1, 4, 1, 1, 12, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_fin_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&7u16.to_be_bytes());
    for reference in [1u16, 5, 7, 7, 10, 1, 8, 9, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'-');
    stream
}

fn deltas_line_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&30u16.to_be_bytes());
    stream.extend_from_slice(&9u16.to_be_bytes());
    stream.extend_from_slice(&906u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.004f64, 0.005, 0.006, 0.0, 1.0, 0.0] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn deltas_plane_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&6u16.to_be_bytes());
    stream.extend_from_slice(&907u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn circle_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (kind, xmt, field) in [(16u8, 8u8, 24usize), (17, 7, 18)] {
        let record = stream
            .windows(4)
            .position(|window| window == [0, kind, 0, xmt])
            .expect("topology record");
        put_ref(&mut stream, record + field, 12);
    }
    let mut circle = record(31, 99);
    put_ref(&mut circle, 2, 12);
    circle[18] = b'+';
    put_vec3(&mut circle, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut circle, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut circle, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut circle, 91, 0.01);
    stream.extend(circle);
    stream
}

fn deltas_circle_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&31u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&908u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.025] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn ellipse_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (kind, xmt, field) in [(16u8, 8u8, 24usize), (17, 7, 18)] {
        let record = stream
            .windows(4)
            .position(|window| window == [0, kind, 0, xmt])
            .expect("topology record");
        put_ref(&mut stream, record + field, 13);
    }
    let mut ellipse = record(32, 107);
    put_ref(&mut ellipse, 2, 13);
    ellipse[18] = b'+';
    put_vec3(&mut ellipse, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut ellipse, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut ellipse, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut ellipse, 91, 0.02);
    put_f64(&mut ellipse, 99, 0.01);
    stream.extend(ellipse);
    stream
}

fn deltas_ellipse_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&32u16.to_be_bytes());
    stream.extend_from_slice(&13u16.to_be_bytes());
    stream.extend_from_slice(&909u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [
        0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.03, 0.012,
    ] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn cylinder_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face");
    put_ref(&mut stream, face + 26, 12);
    let mut cylinder = record(51, 99);
    put_ref(&mut cylinder, 2, 12);
    cylinder[18] = b'+';
    put_vec3(&mut cylinder, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cylinder, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cylinder, 67, 0.01);
    put_vec3(&mut cylinder, 75, [1.0, 0.0, 0.0]);
    stream.extend(cylinder);
    stream
}

fn deltas_cylinder_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&51u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&910u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 0.025, 1.0, 0.0, 0.0] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn cone_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face");
    put_ref(&mut stream, face + 26, 12);
    let mut cone = record(52, 115);
    put_ref(&mut cone, 2, 12);
    cone[18] = b'+';
    put_vec3(&mut cone, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cone, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cone, 67, 0.01);
    put_f64(&mut cone, 75, 0.0);
    put_f64(&mut cone, 83, 1.0);
    put_vec3(&mut cone, 91, [1.0, 0.0, 0.0]);
    stream.extend(cone);
    stream
}

fn deltas_cone_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&52u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&911u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [
        0.001f64,
        0.002,
        0.003,
        0.0,
        1.0,
        0.0,
        0.025,
        0.5,
        3.0f64.sqrt() / 2.0,
        1.0,
        0.0,
        0.0,
    ] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn sphere_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face");
    put_ref(&mut stream, face + 26, 12);
    let mut sphere = record(53, 99);
    put_ref(&mut sphere, 2, 12);
    sphere[18] = b'+';
    put_vec3(&mut sphere, 19, [0.0, 0.0, 0.0]);
    put_f64(&mut sphere, 43, 0.01);
    put_vec3(&mut sphere, 51, [0.0, 0.0, 1.0]);
    put_vec3(&mut sphere, 75, [1.0, 0.0, 0.0]);
    stream.extend(sphere);
    stream
}

fn deltas_sphere_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&53u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&912u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.001f64, 0.002, 0.003, 0.025, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn torus_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face");
    put_ref(&mut stream, face + 26, 12);
    let mut torus = record(54, 107);
    put_ref(&mut torus, 2, 12);
    torus[18] = b'+';
    put_vec3(&mut torus, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut torus, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut torus, 67, 0.03);
    put_f64(&mut torus, 75, 0.01);
    put_vec3(&mut torus, 83, [1.0, 0.0, 0.0]);
    stream.extend(torus);
    stream
}

fn deltas_torus_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&54u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&913u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [
        0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 0.04, 0.015, 1.0, 0.0, 0.0,
    ] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn bspline_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00XX: TRANSMIT FILE (partition)\x00SCH_TEST_1_9999\x00");
    let mut surface = record(124, 23);
    put_ref(&mut surface, 2, 10);
    surface[18] = b'+';
    put_ref(&mut surface, 19, 20);
    put_ref(&mut surface, 21, 21);
    s.extend(surface);

    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 20);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 21);
    s.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 21);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    s.extend(data);

    for (tag, reference, values) in [(127, 30, vec![2u16, 2]), (127, 31, vec![2, 2])] {
        let mut array = record(tag, 8 + values.len() * 2);
        array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
        put_ref(&mut array, 6, reference);
        for (index, value) in values.into_iter().enumerate() {
            put_ref(&mut array, 8 + index * 2, value);
        }
        s.extend(array);
    }
    for reference in [32, 33] {
        let mut array = record(128, 8 + 2 * 8);
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        put_f64(&mut array, 8, 0.0);
        put_f64(&mut array, 16, 1.0);
        s.extend(array);
    }

    let mut curve = record(134, 23);
    put_ref(&mut curve, 2, 50);
    curve[18] = b'+';
    put_ref(&mut curve, 19, 40);
    put_ref(&mut curve, 21, 41);
    s.extend(curve);
    let mut curve_descriptor = record(136, 27);
    put_ref(&mut curve_descriptor, 2, 40);
    put_ref(&mut curve_descriptor, 4, 1);
    put_ref(&mut curve_descriptor, 8, 2);
    put_ref(&mut curve_descriptor, 10, 3);
    put_ref(&mut curve_descriptor, 14, 2);
    curve_descriptor[16] = 5;
    put_ref(&mut curve_descriptor, 23, 42);
    put_ref(&mut curve_descriptor, 25, 43);
    s.extend(curve_descriptor);
    let mut curve_data = record(135, 15 + 6 * 8);
    put_ref(&mut curve_data, 2, 41);
    curve_data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.0, 0.0].into_iter().enumerate() {
        put_f64(&mut curve_data, 15 + index * 8, value);
    }
    s.extend(curve_data);
    for (tag, reference) in [(127, 42), (128, 43)] {
        let mut array = record(tag, if tag == 127 { 12 } else { 24 });
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        if tag == 127 {
            put_ref(&mut array, 8, 2);
            put_ref(&mut array, 10, 2);
        } else {
            put_f64(&mut array, 8, 0.0);
            put_f64(&mut array, 16, 1.0);
        }
        s.extend(array);
    }
    s
}

fn extended_bspline_surface_stream() -> Vec<u8> {
    let descriptor_ref = 40_000u32;
    let payload_ref = 40_001u32;
    let support_refs = [40_010u32, 40_011, 40_012, 40_013];

    let mut stream = Vec::new();
    let mut wrapper = record(124, 19);
    put_ref(&mut wrapper, 2, 10);
    wrapper[18] = b'+';
    stream.extend(wrapper);
    stream.extend(encoded_xmt(descriptor_ref));
    stream.extend(encoded_xmt(payload_ref));

    let xmt = encoded_xmt(descriptor_ref);
    let shift = xmt.len() - 2;
    let mut descriptor = vec![0u8; 58 + shift];
    descriptor[..2].copy_from_slice(&126u16.to_be_bytes());
    descriptor[2..2 + xmt.len()].copy_from_slice(&xmt);
    put_ref(&mut descriptor, 6 + shift, 1);
    put_ref(&mut descriptor, 8 + shift, 1);
    put_ref(&mut descriptor, 12 + shift, 2);
    put_ref(&mut descriptor, 16 + shift, 2);
    descriptor[18 + shift] = 5;
    descriptor[19 + shift] = 5;
    descriptor[20 + shift..24 + shift].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24 + shift..28 + shift].copy_from_slice(&2u32.to_be_bytes());
    let mut at = 34 + shift;
    for reference in [
        40_009,
        support_refs[0],
        support_refs[1],
        support_refs[2],
        support_refs[3],
    ] {
        let encoded = encoded_xmt(reference);
        descriptor[at..at + encoded.len()].copy_from_slice(&encoded);
        at += encoded.len();
    }
    assert_eq!(at, 54 + shift);
    put_ref(&mut descriptor, 54 + shift, 125);
    stream.extend(descriptor);

    let xmt = encoded_xmt(payload_ref);
    let shift = xmt.len() - 2;
    let first = encoded_xmt(40_020);
    let data_at = 95 + shift + first.len();
    let mut payload = vec![0u8; data_at + 12 * 8];
    payload[..2].copy_from_slice(&125u16.to_be_bytes());
    payload[2..2 + xmt.len()].copy_from_slice(&xmt);
    payload[90 + shift] = b'+';
    payload[91 + shift..95 + shift].copy_from_slice(&12u32.to_be_bytes());
    payload[95 + shift..data_at].copy_from_slice(&first);
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut payload, data_at + index * 8, value);
    }
    stream.extend(payload);

    for (tag, reference, values) in [
        (127, support_refs[0], vec![2u16, 2]),
        (127, support_refs[1], vec![2, 2]),
    ] {
        let reference = encoded_xmt(reference);
        let mut array = record(tag, 6 + reference.len() + values.len() * 2);
        array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
        array[6..6 + reference.len()].copy_from_slice(&reference);
        for (index, value) in values.into_iter().enumerate() {
            put_ref(&mut array, 6 + reference.len() + index * 2, value);
        }
        stream.extend(array);
    }
    for reference in [support_refs[2], support_refs[3]] {
        let reference = encoded_xmt(reference);
        let mut array = record(128, 6 + reference.len() + 16);
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        array[6..6 + reference.len()].copy_from_slice(&reference);
        put_f64(&mut array, 6 + reference.len(), 0.0);
        put_f64(&mut array, 14 + reference.len(), 1.0);
        stream.extend(array);
    }
    stream
}

fn bspline_surface_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 60);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 61);
    stream.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 61);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.03, 0.0, 0.015, 0.0, 0.0, 0.015, 0.03, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    stream.extend(data);
    stream
}

fn deltas_bspline_surface_wrapper_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&124u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&914u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [60u16, 61] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn bspline_curve_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(136, 27);
    put_ref(&mut descriptor, 2, 70);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 3);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    put_ref(&mut descriptor, 23, 42);
    put_ref(&mut descriptor, 25, 43);
    stream.extend(descriptor);

    let mut data = record(135, 15 + 6 * 8);
    put_ref(&mut data, 2, 71);
    data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.01, 0.0].into_iter().enumerate() {
        put_f64(&mut data, 15 + index * 8, value);
    }
    stream.extend(data);
    stream
}

fn deltas_bspline_curve_wrapper_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&134u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&915u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [70u16, 71] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 12);
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 12);
    trim[18] = b'+';
    put_ref(&mut trim, 19, 9);
    put_f64(&mut trim, 69, 0.000_25);
    put_f64(&mut trim, 77, 0.000_75);
    // The closed edge's single vertex sits at the trim range's midpoint on the
    // basis line so both trimmed endpoints fall inside the edge's stored
    // 0.3 mm tolerance; the point record is the topology stream's last
    // 40 bytes, before the trim record is appended.
    let point_vec = stream.len() - 40 + 16;
    put_vec3(&mut stream, point_vec, [0.000_5, 0.0, 0.0]);
    stream.extend(trim);
    stream
}

fn mismatched_trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let point_vec = stream.len() - 85 - 40 + 16;
    put_vec3(&mut stream, point_vec, [0.000_5, 0.01, 0.0]);
    stream
}

fn partnered_trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 69, 0.000_75);
    put_f64(&mut stream, trim + 77, 0.000_25);
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("first face");
    put_ref(&mut stream, face + 18, 20);
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("first fin");
    put_ref(&mut stream, fin + 14, 22);
    let first_point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("first point");
    put_vec3(&mut stream, first_point + 16, [0.000_25, 0.0, 0.0]);

    let mut second_face = record(14, 39);
    put_ref(&mut second_face, 2, 20);
    put_f64(&mut second_face, 10, 0.000_2);
    put_ref(&mut second_face, 18, 1);
    put_ref(&mut second_face, 20, 4);
    put_ref(&mut second_face, 22, 21);
    put_ref(&mut second_face, 24, 3);
    put_ref(&mut second_face, 26, 6);
    second_face[28] = b'+';
    stream.extend(second_face);

    let mut second_loop = record(15, 16);
    put_ref(&mut second_loop, 2, 21);
    put_ref(&mut second_loop, 10, 22);
    put_ref(&mut second_loop, 12, 20);
    put_ref(&mut second_loop, 14, 1);
    stream.extend(second_loop);

    let mut second_fin = record(17, 23);
    put_ref(&mut second_fin, 2, 22);
    put_ref(&mut second_fin, 6, 21);
    put_ref(&mut second_fin, 8, 22);
    put_ref(&mut second_fin, 10, 22);
    put_ref(&mut second_fin, 12, 23);
    put_ref(&mut second_fin, 14, 7);
    put_ref(&mut second_fin, 16, 8);
    put_ref(&mut second_fin, 18, 1);
    second_fin[22] = b'-';
    stream.extend(second_fin);

    let mut second_vertex = record(18, 28);
    put_ref(&mut second_vertex, 2, 23);
    put_ref(&mut second_vertex, 16, 24);
    put_f64(&mut second_vertex, 18, 0.000_1);
    stream.extend(second_vertex);

    let mut second_point = record(29, 40);
    put_ref(&mut second_point, 2, 24);
    put_vec3(&mut second_point, 16, [0.000_75, 0.0, 0.0]);
    stream.extend(second_point);
    stream
}

fn forward_trimmed_curve_chain_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("first trimmed curve");
    put_ref(&mut stream, first + 19, 20);

    let mut second = record(133, 85);
    put_ref(&mut second, 2, 20);
    second[18] = b'+';
    put_ref(&mut second, 19, 9);
    put_f64(&mut second, 69, 0.000_25);
    put_f64(&mut second, 77, 0.000_75);
    stream.extend(second);
    stream
}

fn topology_with_extended_edge_curve_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream[edge + 24..edge + 26].copy_from_slice(&(-9i16).to_be_bytes());
    stream.splice(edge + 26..edge + 26, [0, 0]);
    stream
}

fn topology_with_extended_face_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    stream.splice(face + 8..face + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

fn topology_with_extended_edge_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream.splice(edge + 8..edge + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

fn topology_with_extended_internal_topology_references() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(13, 3, 8), (15, 5, 8), (17, 7, 4), (18, 10, 8), (29, 11, 8)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        stream.splice(
            record + offset..record + offset + 2,
            [0xff, 0xff, 0x00, 0x00],
        );
    }
    stream
}

fn topology_with_fully_extended_geometry_headers() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt) in [(50, 6), (30, 9)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("geometry record");
        for index in 0..5 {
            let at = record + 8 + index * 4;
            stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
        }
    }
    stream
}

fn topology_with_escaped_geometry_envelopes() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for marker in [[0, 50, 0, 6], [0, 30, 0, 9]] {
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("geometry record");
        stream.insert(record + 2, 0xff);
    }
    stream
}

fn offset_surface_with_fully_extended_common_header() -> Vec<u8> {
    let mut stream = offset_surface_topology_partition_stream();
    let record = stream
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("offset record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
    stream
}

fn fully_extend_common_header(stream: &mut Vec<u8>, marker: [u8; 4]) {
    let record = stream
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("compact geometry record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
}

fn zlib_compress(raw: &[u8]) -> Vec<u8> {
    // Level 1 emits the `78 01` zlib header NX/Parasolid streams use.
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

fn zlib_compress_at_level(raw: &[u8], level: u32) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(level));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

/// Assemble a synthetic single-part `.prt`: the SPLMSSTR header, a HEADER
/// directory with one `/Root/UG_PART/UG_PART` file entry, and a zlib-compressed
/// Parasolid partition stream.
fn single_part_prt() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06); // version tag
    f.extend_from_slice(&[0x11, 0x22, 0x33]); // u24 file tag
    f.extend_from_slice(&[0, 0, 0, 0]); // +0x0c constant
    f.push(0x00); // +0x10 constant
    f.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // +0x11 footer offset (0 → no footer)
    f.extend_from_slice(&[0, 0]); // pad to 0x19
    assert_eq!(f.len(), 0x19);

    // HEADER directory: one file entry naming the canonical part stream.
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/UG_PART";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    // 16-byte payload: file_offset then size (both u64 LE) — point at the zlib blob.
    let blob = zlib_compress(&partition_stream());
    // The blob will be appended after the directory; compute its offset now.
    let dir_end = f.len() + 16; // after this entry's payload
    let blob_off = dir_end as u64;
    f.extend_from_slice(&blob_off.to_le_bytes());
    f.extend_from_slice(&(blob.len() as u64).to_le_bytes());
    f.extend_from_slice(&blob);
    f
}

fn prt_with_named_payloads(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(0x06);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let mut spans = Vec::new();
    for (name, _) in entries {
        file.extend_from_slice(&(name.len() as u32).to_le_bytes());
        file.extend_from_slice(name.as_bytes());
        spans.push(file.len());
        file.extend_from_slice(&[0; 16]);
    }
    for ((_, payload), span) in entries.iter().zip(spans) {
        let offset = file.len();
        file.extend_from_slice(payload);
        file[span..span + 8].copy_from_slice(&(offset as u64).to_le_bytes());
        file[span + 8..span + 16].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    }
    file
}

fn prt_with_arrangements() -> Vec<u8> {
    let mut arrangements = br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="NO" Name="Exploded"/></Arrangements>"#.to_vec();
    arrangements.push(0);
    let mut attributes = br#"<UgAttributes version="4"><Attribute owner="part" pdmBased="false" utf8title="NX_Arrangement" utf8value="Model" version="3" type="StringAttributeType"/></UgAttributes>"#.to_vec();
    attributes.push(0);
    prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            zlib_compress(&topology_partition_stream()),
        ),
        ("/Root/part/arrangements", arrangements),
        ("/Root/part/attrs", attributes),
    ])
}

fn topology_part_prt() -> Vec<u8> {
    prt_with_partition(&topology_partition_stream())
}

fn topology_with_missing_tolerances() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(14, 4, 10), (16, 8, 10), (18, 10, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_f64(&mut stream, record + offset, -31_415_800_000_000.0);
    }
    stream
}

fn prt_with_partition(stream: &[u8]) -> Vec<u8> {
    let mut f = single_part_prt();
    let compressed = zlib_compress(stream);
    let entry = container::scan_bytes(f.clone()).unwrap().entries.remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, f.len());
    f.truncate(offset as usize);
    f.extend_from_slice(&compressed);
    let size_at = offset as usize - 8;
    f[size_at..size_at + 8].copy_from_slice(&(compressed.len() as u64).to_le_bytes());
    f
}

fn prt_with_streams(streams: &[&[u8]]) -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let payload = streams
        .iter()
        .flat_map(|stream| zlib_compress(stream))
        .collect::<Vec<_>>();
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_indexed_om_section() -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let mut payload = indexed_om_section();
    payload.extend(zlib_compress(&partition_stream()));
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_size_framed_om_section() -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let mut payload = size_framed_om_section();
    payload.extend(zlib_compress(&partition_stream()));
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

fn large_xmt_headers(stream: &[u8]) -> Vec<u8> {
    let marker = b"SCH_TEST_1_9999\x00";
    let start = stream
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len();
    let lengths = [24, 24, 39, 16, 23, 32, 91, 67, 28, 16, 40];
    let mut out = stream[..start].to_vec();
    let mut pos = start;
    for len in lengths {
        let record = &stream[pos..pos + len];
        let xmt = u16::from_be_bytes([record[2], record[3]]);
        out.extend_from_slice(&record[..2]);
        out.extend_from_slice(&(-(i16::try_from(xmt).unwrap())).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&record[4..]);
        pos += len;
    }
    out
}

/// A synthetic assembly `.prt`: SPLMSSTR header, an `ExternalReferences` file
/// entry, and no embedded Parasolid stream.
fn assembly_prt() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0, 0, 0]);
    f.extend_from_slice(&[0, 0, 0, 0]);
    f.push(0x00);
    f.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/ExternalReferences";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    f.extend_from_slice(&[0u8; 16]); // opaque directory payload
    f
}

fn assembly_with_external_paths() -> Vec<u8> {
    let payload = b"EXTREFSTREAM\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/ExternalReferences";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    let offset = f.len() + 16;
    f.extend_from_slice(&(offset as u64).to_le_bytes());
    f.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    f.extend_from_slice(payload);
    f
}

fn rmfastload_prt() -> Vec<u8> {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1..=50u32 {
        payload.extend_from_slice(&id.to_le_bytes());
    }
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(6);
    f.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/FastLoad/RMFastLoad";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    let offset = f.len() + 16;
    f.extend_from_slice(&(offset as u64).to_le_bytes());
    f.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    f.extend(payload);
    f
}

fn many_face_partition_stream(node_id_start: u32) -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(
        b"PS\x00\x00XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );
    let mut body = record(12, 24);
    put_ref(&mut body, 2, 2);
    body[4..8].copy_from_slice(&(node_id_start + 100).to_be_bytes());
    stream.extend(body);
    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 3);
    shell[4..8].copy_from_slice(&(node_id_start + 101).to_be_bytes());
    put_ref(&mut shell, 8, 1);
    put_ref(&mut shell, 10, 2);
    put_ref(&mut shell, 12, 1);
    put_ref(&mut shell, 14, 300);
    put_ref(&mut shell, 16, 1);
    put_ref(&mut shell, 18, 1);
    put_ref(&mut shell, 20, 4);
    put_ref(&mut shell, 22, 1);
    stream.extend(shell);
    let mut region = record(19, 16);
    put_ref(&mut region, 2, 4);
    stream.extend(region);
    for index in 0..50u16 {
        let mut face = record(14, 39);
        put_ref(&mut face, 2, 300 + index);
        face[4..8].copy_from_slice(&(node_id_start + u32::from(index)).to_be_bytes());
        put_f64(&mut face, 10, 0.000_1);
        put_ref(&mut face, 18, if index == 49 { 1 } else { 301 + index });
        put_ref(&mut face, 20, if index == 0 { 1 } else { 299 + index });
        put_ref(&mut face, 22, 1);
        put_ref(&mut face, 24, 3);
        put_ref(&mut face, 26, 500 + index);
        face[28] = b'+';
        stream.extend(face);
    }
    for index in 0..50u16 {
        let mut plane = record(50, 91);
        put_ref(&mut plane, 2, 500 + index);
        plane[18] = b'+';
        put_vec3(&mut plane, 19, [f64::from(index) * 0.001, 0.0, 0.0]);
        put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
        put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
        stream.extend(plane);
    }
    stream
}

fn prt_with_two_bodies_and_rmfastload() -> Vec<u8> {
    let mut part_payload = zlib_compress(&many_face_partition_stream(1_000));
    part_payload.extend(zlib_compress(&many_face_partition_stream(2_000)));
    let mut rm_payload = b"UGS::Solid::Topol".to_vec();
    rm_payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1_000..1_050u32 {
        rm_payload.extend_from_slice(&id.to_le_bytes());
    }

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(6);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let part_name = b"/Root/UG_PART/UG_PART";
    file.extend_from_slice(&(part_name.len() as u32).to_le_bytes());
    file.extend_from_slice(part_name);
    let part_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let rm_name = b"/Root/FastLoad/RMFastLoad";
    file.extend_from_slice(&(rm_name.len() as u32).to_le_bytes());
    file.extend_from_slice(rm_name);
    let rm_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let part_offset = file.len();
    file.extend_from_slice(&part_payload);
    let rm_offset = file.len();
    file.extend_from_slice(&rm_payload);
    file[part_span..part_span + 8].copy_from_slice(&(part_offset as u64).to_le_bytes());
    file[part_span + 8..part_span + 16].copy_from_slice(&(part_payload.len() as u64).to_le_bytes());
    file[rm_span..rm_span + 8].copy_from_slice(&(rm_offset as u64).to_le_bytes());
    file[rm_span + 8..rm_span + 16].copy_from_slice(&(rm_payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_two_active_bodies_and_rmfastload() -> Vec<u8> {
    let mut file = prt_with_two_bodies_and_rmfastload();
    let marker = b"UGS::Solid::Topol";
    let count_at = file
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("RMFastLoad payload")
        + marker.len();
    let ids_at = count_at + 4;
    let tail = file[ids_at + 50 * 4..].to_vec();
    file[count_at..count_at + 4].copy_from_slice(&100u32.to_le_bytes());
    file.truncate(ids_at + 50 * 4);
    for id in 2_000..2_050u32 {
        file.extend_from_slice(&id.to_le_bytes());
    }
    file.extend_from_slice(&tail);
    let directory_size_at = file
        .windows(b"/Root/FastLoad/RMFastLoad".len())
        .position(|window| window == b"/Root/FastLoad/RMFastLoad")
        .expect("RMFastLoad directory")
        + b"/Root/FastLoad/RMFastLoad".len()
        + 8;
    file[directory_size_at..directory_size_at + 8]
        .copy_from_slice(&((marker.len() + 4 + 100 * 4) as u64).to_le_bytes());
    file
}

fn prt_with_missing_active_body_record() -> Vec<u8> {
    let mut active_stream = many_face_partition_stream(1_000);
    let body = active_stream
        .windows(4)
        .position(|window| window == [0, 12, 0, 2])
        .expect("body record");
    active_stream[body..body + 24].fill(0xff);
    let mut part_payload = zlib_compress(&active_stream);
    part_payload.extend(zlib_compress(&many_face_partition_stream(2_000)));
    let mut rm_payload = b"UGS::Solid::Topol".to_vec();
    rm_payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1_000..1_050u32 {
        rm_payload.extend_from_slice(&id.to_le_bytes());
    }

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(6);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let part_name = b"/Root/UG_PART/UG_PART";
    file.extend_from_slice(&(part_name.len() as u32).to_le_bytes());
    file.extend_from_slice(part_name);
    let part_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let rm_name = b"/Root/FastLoad/RMFastLoad";
    file.extend_from_slice(&(rm_name.len() as u32).to_le_bytes());
    file.extend_from_slice(rm_name);
    let rm_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let part_offset = file.len();
    file.extend_from_slice(&part_payload);
    let rm_offset = file.len();
    file.extend_from_slice(&rm_payload);
    file[part_span..part_span + 8].copy_from_slice(&(part_offset as u64).to_le_bytes());
    file[part_span + 8..part_span + 16].copy_from_slice(&(part_payload.len() as u64).to_le_bytes());
    file[rm_span..rm_span + 8].copy_from_slice(&(rm_offset as u64).to_le_bytes());
    file[rm_span + 8..rm_span + 16].copy_from_slice(&(rm_payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_weak_rmfastload_overlap() -> Vec<u8> {
    let mut file = prt_with_two_bodies_and_rmfastload();
    let marker = b"UGS::Solid::Topol";
    let payload = file
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("RMFastLoad payload")
        + marker.len()
        + 4;
    for index in 0..50u32 {
        let id = if index < 5 {
            1_000 + index
        } else {
            10_000 + index
        };
        let at = payload + index as usize * 4;
        file[at..at + 4].copy_from_slice(&id.to_le_bytes());
    }
    file
}

#[test]
fn detect_high_on_magic() {
    assert_eq!(NxCodec.detect(MAGIC), Confidence::High);
    assert_eq!(NxCodec.detect(&single_part_prt()), Confidence::High);
    assert_eq!(NxCodec.detect(b"PK\x03\x04 not nx"), Confidence::No);
    // A Creo/Granite .prt shares the extension but not the magic.
    assert_eq!(NxCodec.detect(b"\xe0\x02\xff\xfeGRANITE"), Confidence::No);
}

#[test]
fn container_parses_header_and_directory() {
    let c = container::scan_bytes(single_part_prt()).unwrap();
    assert_eq!(c.version, 0x06);
    assert_eq!(c.file_tag, 0x33_22_11);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span.is_some()));
}

#[test]
fn inspect_reports_bounded_nx_object_model_entities() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let summary = NxCodec.inspect(&mut cur).unwrap();
    assert!(summary.notes.iter().any(|note| {
        note == "NX object model: 1 indexed section(s), 2 bounded entity record(s)"
    }));
}

#[test]
fn decode_retains_typed_nx_numeric_expression() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let expressions = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::Expression>("expressions")
        .unwrap();
    assert_eq!(result.ir.native.namespace("nx").unwrap().version, 145);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, Some(0x102));
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier.as_deref(),
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].unit, crate::native::ExpressionUnit::Degree);
    assert_eq!(expressions[0].expression, "120");
    assert_eq!(expressions[0].value, Some(120.0));
    assert_eq!(expressions[0].source_entry, "/Root/UG_PART/UG_PART");
    let declarations = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ExpressionDeclaration>("expression_declarations")
        .unwrap();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].object_id, 0x102);
    assert_eq!(declarations[0].parameter_index, 8);
    assert_eq!(declarations[0].literal.as_deref(), Some("120"));
    assert_eq!(
        expressions[0].declaration.as_deref(),
        Some(declarations[0].id.as_str())
    );
    let parameter = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == expressions[0].name)
        .unwrap();
    assert_eq!(
        parameter.properties.get("declaration"),
        Some(&declarations[0].id)
    );
    assert_eq!(
        parameter.properties.get("declaration_object_id"),
        Some(&"258".to_string())
    );
    let om_records = result
        .ir
        .native_unknowns("nx")
        .unwrap()
        .into_iter()
        .filter(|record| record.id.0.starts_with("nx:om-section-"))
        .collect::<Vec<_>>();
    assert_eq!(om_records.len(), 2);
    assert!(om_records.iter().all(|record| {
        record.data.as_ref().is_some_and(|data| {
            data.len() as u64 == record.byte_len
                && cadmpeg_ir::hash::sha256_hex(data) == record.sha256
        })
    }));
    let object_records = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ObjectRecord>("object_records")
        .unwrap();
    assert_eq!(object_records.len(), 2);
    let headers = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::StoreHeader>("store_headers")
        .unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].version, "NX 2027.3102");
    assert_eq!(headers[0].object_id, Some(0x101));
    assert_eq!(object_records[1].object_id, Some(0x102));
    assert_eq!(expressions[0].record.as_ref(), Some(&object_records[1].id));
    assert_eq!(object_records[1].record_ordinal, 1);
    assert_eq!(
        object_records[0].section_offset,
        object_records[1].section_offset
    );
    assert_eq!(object_records[1].byte_len, om_records[1].byte_len);
    assert_eq!(object_records[1].sha256, om_records[1].sha256);
    assert_eq!(
        object_records[1].dependencies,
        vec![object_records[0].id.clone()]
    );
    assert_eq!(
        object_records[0].dependents,
        vec![object_records[1].id.clone()]
    );
    let strings = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::StringValue>("string_values")
        .unwrap();
    assert_eq!(strings.len(), 1);
    assert_eq!(strings[0].record, object_records[1].id);
    assert_eq!(strings[0].object_id, Some(0x102));
    assert_eq!(strings[0].value, "SKETCH_001");
    let references = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ObjectReference>("object_references")
        .unwrap();
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].record, object_records[1].id);
    assert_eq!(references[0].object_id, Some(0x102));
    assert_eq!(references[0].value, 0x1234_5678);
    assert_eq!(references[0].target_record, None);
    assert_eq!(
        references[1].kind,
        crate::native::ObjectReferenceKind::RecordOrdinal16
    );
    assert_eq!(references[1].value, 0);
    assert_eq!(
        references[1].target_record.as_ref(),
        Some(&object_records[0].id)
    );
    let handles = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::PersistentHandle>("persistent_handles")
        .unwrap();
    assert_eq!(handles.len(), 1);
    assert_eq!(handles[0].value, 0x1234_5678);
    assert_eq!(handles[0].records, vec![object_records[1].id.clone()]);
    assert_eq!(handles[0].occurrence_count, 1);
    assert!(handles[0].external_records.is_empty());
    assert_eq!(result.ir.model.features.len(), 1);
    assert!(matches!(
        result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::Equations
        }
    ));
    assert_eq!(result.ir.model.parameters.len(), 1);
    assert_eq!(result.ir.model.parameters[0].expression, "120");
    let parameter = &result.ir.model.parameters[0];
    assert_eq!(parameter.name, expressions[0].name);
    assert!(matches!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(value)
        )) if value == 120_f64.to_radians()
    ));
    assert_eq!(parameter.native_ref.as_ref(), Some(&expressions[0].id));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn nx_part_attributes_require_typed_atomic_xml() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Attribute owner="part" pdmBased="false" title="legacy" utf8title="Material"
    value="legacy-value" utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
</UgAttributes>"#;
    let attributes = crate::native::parse_part_attributes(xml, 7, "/Root/part/attrs", 100)
        .expect("typed attributes");
    assert_eq!(attributes.len(), 1);
    assert_eq!(attributes[0].id, "nx:part-attributes-7:attribute#0");
    assert_eq!(attributes[0].title, "Material");
    assert_eq!(attributes[0].value, "Steel");
    assert_eq!(attributes[0].value_type, "StringAttributeType");
    assert!(!attributes[0].pdm_based);
    assert!(attributes[0].source_offset > 100);

    let mut terminated = xml.to_vec();
    terminated.push(0);
    assert_eq!(
        crate::native::parse_part_attributes(&terminated, 7, "/Root/part/attrs", 100)
            .expect("terminated typed attributes"),
        attributes
    );
    terminated.push(0);
    assert!(
        crate::native::parse_part_attributes(&terminated, 7, "/Root/part/attrs", 100).is_none()
    );

    let malformed = xml
        .windows(b"pdmBased=\"false\"".len())
        .position(|window| window == b"pdmBased=\"false\"")
        .map(|at| {
            let mut malformed = xml.to_vec();
            malformed[at + b"pdmBased=\"".len()..at + b"pdmBased=\"false".len()]
                .copy_from_slice(b"maybe");
            malformed
        })
        .unwrap();
    assert!(crate::native::parse_part_attributes(&malformed, 7, "/Root/part/attrs", 100).is_none());
}

#[test]
fn decode_projects_part_attributes_to_document_attributes() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Attribute owner="part" pdmBased="false" utf8title="Material"
    utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
</UgAttributes>"#;
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/attrs", xml.to_vec()),
    ]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.attributes.len(), 1);
    let attribute = &result.ir.model.attributes[0];
    assert_eq!(attribute.name, "Material");
    assert_eq!(
        attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Document
    );
    assert_eq!(
        attribute.values,
        vec![cadmpeg_ir::attributes::AttributeValue::String(
            "Steel".to_string()
        )]
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_length_framed_nx_class_definition() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let classes = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ClassDefinition>("class_definitions")
        .unwrap();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "UGS::EXP_expression");
    assert_eq!(classes[0].ordinal, 0);
    assert_eq!(classes[0].trailing_code, 0x81);
    assert_eq!(classes[0].source_entry, "/Root/UG_PART/UG_PART");
}

#[test]
fn decode_retains_length_framed_nx_field_definitions() {
    let mut cur = Cursor::new(prt_with_size_framed_om_section());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let fields = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::FieldDefinition>("field_definitions")
        .unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "m_target");
    assert_eq!(fields[0].ordinal, 0);
    assert_eq!(fields[0].registry_suffix, [0x01, 0x02]);
    assert_eq!(fields[1].name, "m_tools");
    assert_eq!(fields[1].trailing_code, 0x81);
    assert!(fields[1].registry_suffix.is_empty());
    assert_eq!(fields[1].source_entry, "/Root/UG_PART/UG_PART");
    let classes = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ClassDefinition>("class_definitions")
        .unwrap();
    assert_eq!(classes[0].layout_prefix, &[0x81, 0x21]);
    assert_eq!(
        classes[0].schema_fingerprint,
        Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
    );
    assert_eq!(classes[0].layout_terminal, Some(0x06));
}

#[test]
fn decode_retains_nx_arrangement_configurations() {
    let mut cur = Cursor::new(prt_with_arrangements());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let configurations = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::Configuration>("configurations")
        .unwrap();
    assert_eq!(configurations.len(), 2);
    assert_eq!(configurations[0].name, "Model");
    assert!(configurations[0].active);
    assert_eq!(configurations[1].name, "Exploded");
    assert!(!configurations[1].active);
    assert_eq!(result.ir.model.configurations.len(), 2);
    assert_eq!(result.ir.model.configurations[0].ordinal, 0);
    assert_eq!(result.ir.model.configurations[0].source_index, Some(0));
    assert_eq!(result.ir.model.configurations[0].name, "Model");
    assert!(result.ir.model.configurations[0].active);
    assert_eq!(
        result.ir.model.configurations[0].bodies,
        result
            .ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(result.ir.model.configurations[1].ordinal, 1);
    assert_eq!(result.ir.model.configurations[1].name, "Exploded");
    assert!(!result.ir.model.configurations[1].active);
    assert!(result.ir.model.configurations[1].bodies.is_empty());
    let uses = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ConfigurationAttributeUse>("configuration_attribute_uses")
        .unwrap();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].configuration, configurations[0].id);
    assert_eq!(uses[0].name, "Model");
    assert_eq!(
        result.ir.model.configurations[0].properties["active_attribute_use"],
        uses[0].id
    );
    let attributes = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::PartAttribute>("part_attributes")
        .unwrap();
    let mut mismatch = attributes.clone();
    mismatch[0].value = "Other".to_string();
    assert!(crate::native::configuration_attribute_uses(&configurations, &mismatch).is_empty());
    let mut duplicate = attributes.clone();
    duplicate.push(attributes[0].clone());
    assert!(crate::native::configuration_attribute_uses(&configurations, &duplicate).is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_exposes_strict_nx_jpeg_preview_metadata() {
    let preview = [
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xb9,
        0x00, 0xf7, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
    ];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/images/preview", preview.to_vec()),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir.source.unwrap().attributes;
    assert_eq!(attributes["jpeg_preview_count"], "1");
    assert_eq!(attributes["jpeg_preview_0_width"], "247");
    assert_eq!(attributes["jpeg_preview_0_height"], "185");
    assert_eq!(attributes["jpeg_preview_0_precision"], "8");
    assert_eq!(attributes["jpeg_preview_0_components"], "3");
    assert_eq!(
        attributes["jpeg_preview_0_byte_len"],
        preview.len().to_string()
    );

    let mut malformed = preview;
    malformed[10..12].copy_from_slice(&16u16.to_be_bytes());
    assert!(crate::decode::jpeg_dimensions(&malformed).is_none());
}

#[test]
fn decode_rejects_ambiguous_nx_arrangement_table_atomically() {
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        (
            "/Root/part/arrangements",
            br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="YES" Name="Exploded"/></Arrangements>"#.to_vec(),
        ),
    ]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(result.ir.native.namespace("nx").is_none_or(|namespace| {
        namespace
            .arena_as::<crate::native::Configuration>("configurations")
            .unwrap()
            .is_empty()
    }));
    assert!(result.ir.model.configurations.is_empty());
}

#[test]
fn decode_rejects_repeated_nx_arrangement_terminators_atomically() {
    let mut arrangements =
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
    arrangements.extend_from_slice(&[0, 0]);
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.configurations.is_empty());
}

#[test]
fn parasolid_extraction_classifies_partition_and_schema() {
    let f = single_part_prt();
    let streams = parasolid::extract_streams(&f);
    let part = streams
        .iter()
        .find(|s| s.kind == StreamKind::Partition)
        .expect("a partition stream");
    assert_eq!(part.schema.as_deref(), Some("SCH_TEST_1_9999"));
    assert!(part.inflated.starts_with(b"PS\x00\x00"));
}

#[test]
fn parasolid_attribute_definition_requires_declared_printable_name_and_field_record() {
    let mut bytes = vec![0xaa, 0x00, 0x4f, 0xff];
    bytes.extend_from_slice(&16u32.to_be_bytes());
    bytes.extend_from_slice(&0x012au16.to_be_bytes());
    bytes.extend_from_slice(b"SDL/TYSA_DENSITY");
    bytes.extend_from_slice(&[0x00, 0x50, 0x00, 0x00, 0x00, 0x01]);
    bytes.extend_from_slice(&0x012bu16.to_be_bytes());
    bytes.extend_from_slice(&0x0030u16.to_be_bytes());
    bytes.extend_from_slice(&0x0031u16.to_be_bytes());
    bytes.extend_from_slice(&[0x00, 0x00, 0x23, 0x28]);
    let descriptor = [
        0x00, 0x00, 0x00, 0x00, 0x03, 0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    ];
    bytes.extend_from_slice(&descriptor);
    bytes.push(1);
    let definitions = crate::parasolid::attribute_definitions(&bytes);
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].offset, 1);
    assert_eq!(definitions[0].xmt, 0x12a);
    assert_eq!(definitions[0].name, "SDL/TYSA_DENSITY");
    assert_eq!(definitions[0].field_count, 1);
    assert_eq!(definitions[0].field_record_xmt, 0x12b);
    assert_eq!(definitions[0].field_record_references, [0x30, 0x31]);
    assert_eq!(definitions[0].field_record_header_words, [0, 0x2328]);
    assert_eq!(definitions[0].field_descriptor_prefix, descriptor);
    assert_eq!(definitions[0].field_codes, [1]);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(crate::parasolid::attribute_definitions(truncated).is_empty());

    bytes[20] = 0;
    assert!(crate::parasolid::attribute_definitions(&bytes).is_empty());
}

#[test]
fn decode_transfers_point_plane_cylinder_line() {
    let mut cur = Cursor::new(single_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    // Point coordinate is scaled metres → millimetres, byte-exact.
    let p = &result.ir.model.points[0].position;
    assert!((p.x - 62.5).abs() < 1e-6 && (p.z - 12.7).abs() < 1e-6);

    // One plane, one cylinder decoded.
    let planes = result
        .ir
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Plane { .. }))
        .count();
    let cyls: Vec<_> = result
        .ir
        .model
        .surfaces
        .iter()
        .filter_map(|s| match &s.geometry {
            SurfaceGeometry::Cylinder { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect();
    assert_eq!(planes, 1);
    assert_eq!(cyls.len(), 1);
    assert!((cyls[0] - 4.05).abs() < 1e-6);
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane {
            u_axis: axis,
            ..
        } if axis == Vector3::new(1.0, 0.0, 0.0)
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder {
            ref_direction: direction,
            ..
        } if direction == Vector3::new(1.0, 0.0, 0.0)
    )));

    // One line decoded, with a unit direction.
    let lines: Vec<_> = result
        .ir
        .model
        .curves
        .iter()
        .filter(|c| matches!(c.geometry, CurveGeometry::Line { .. }))
        .collect();
    assert_eq!(lines.len(), 1);

    // No topology graph is fabricated; the loss is reported as blocking.
    assert!(result.ir.model.faces.is_empty() && result.ir.model.edges.is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.category == cadmpeg_ir::report::LossCategory::Topology
            && l.severity == cadmpeg_ir::report::Severity::Blocking));

    // The Parasolid stream is preserved verbatim.
    let unknowns = result.ir.native_unknowns("nx").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].sha256.len(), 64);
    assert_eq!(
        unknowns[0].links,
        ["nx:s0:surf#0", "nx:s0:surf#1", "nx:s0:crv#0",]
    );
    assert_eq!(
        result.ir.annotations.exactness[&unknowns[0].id.to_string()].fields["links"],
        Exactness::Derived
    );

    // The preserved stream owns partial-decode carriers without fabricating topology.
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn decode_emits_connected_primitive_brep() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(
        result.ir.model.faces[0].loops,
        vec![result.ir.model.loops[0].id.clone()]
    );
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.coedges[0].radial_next,
        result.ir.model.coedges[0].id
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| loss.category != cadmpeg_ir::report::LossCategory::Topology));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_emits_offset_surface_construction() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("offset surface");
    let ProceduralSurfaceDefinition::Offset {
        support,
        distance,
        u_sense,
        v_sense,
        extension_flags,
    } = &procedural.definition
    else {
        panic!("offset definition");
    };
    assert_eq!(*distance, 2.5);
    assert_eq!(*u_sense, 0);
    assert_eq!(*v_sense, 0);
    assert!(extension_flags.is_empty());
    assert_ne!(procedural.surface, *support);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidOffsetSurfaceRecord>("parasolid_offset_surface_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].discriminator, 'V');
    assert!(records[0].true_offset);
    assert_eq!(records[0].support_xmt, 6);
    assert_eq!(records[0].distance, 2.5);
    let carrier = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .expect("offset carrier");
    assert_eq!(
        carrier
            .source_object
            .as_ref()
            .map(|source| &source.object_id),
        Some(&records[0].id)
    );
    assert!(matches!(
        &carrier.geometry,
        SurfaceGeometry::Procedural { construction } if construction == &procedural.id
    ));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn offset_surface_parameter_solver_preserves_support_parameters() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result.ir.model.procedural_surfaces[0].surface.clone();
    let expected = Point2::new(12.0, 7.0);
    let point = cadmpeg_ir::eval::model_surface_point(&result.ir, &surface, expected.u, expected.v)
        .unwrap();

    let actual =
        crate::decode::offset_surface_parameters(&result.ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);
}

#[test]
fn offset_surface_parameter_solver_accepts_a_seed_within_fit_tolerance() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result.ir.model.procedural_surfaces[0].surface.clone();
    let seed = Point2::new(12.0, 7.0);
    let mut point =
        cadmpeg_ir::eval::model_surface_point(&result.ir, &surface, seed.u, seed.v).unwrap();
    point.x += 0.01;

    let actual = crate::decode::offset_surface_parameters_with_tolerance(
        &result.ir,
        &surface,
        point,
        Some(seed),
        Some(0.02),
    )
    .unwrap();

    assert_eq!(actual, seed);
}

#[test]
fn decode_tracks_fully_extended_offset_common_header() {
    let stream = offset_surface_with_fully_extended_common_header();
    assert_eq!(crate::topology::offset_surfaces(&stream).len(), 1);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("offset surface");
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = &procedural.definition
    else {
        panic!("offset definition");
    };
    assert_eq!(*distance, 2.5);
    assert_ne!(procedural.surface, *support);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
}

#[test]
fn decode_tracks_fully_extended_compact_geometry_headers() {
    let mut blend = blend_surface_topology_partition_stream();
    fully_extend_common_header(&mut blend, [0, 56, 0, 12]);
    assert_eq!(crate::topology::blend_surfaces(&blend).len(), 1);

    let mut intersection = intersection_curve_topology_partition_stream();
    fully_extend_common_header(&mut intersection, [0, 38, 0, 12]);
    assert_eq!(crate::topology::composite_curves(&intersection).len(), 1);

    let mut surface_curve = surface_curve_topology_partition_stream();
    fully_extend_common_header(&mut surface_curve, [0, 137, 0, 12]);
    let surface_curves = crate::topology::surface_curves(&surface_curve);
    assert_eq!(surface_curves.len(), 1);
    assert_eq!(surface_curves[0].xmt, 12);
    assert_eq!(surface_curves[0].pcurve, 9);

    let mut trimmed = trimmed_topology_partition_stream();
    fully_extend_common_header(&mut trimmed, [0, 133, 0, 12]);
    let trims = crate::topology::trimmed_curves(&trimmed);
    assert_eq!(trims.len(), 1);
    assert_eq!(trims[0].parameters, [0.000_25, 0.000_75]);

    let mut bspline = bspline_partition_stream();
    fully_extend_common_header(&mut bspline, [0, 124, 0, 10]);
    fully_extend_common_header(&mut bspline, [0, 134, 0, 50]);
    let mut cur = Cursor::new(prt_with_partition(&bspline));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_))));
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Nurbs(_))));
}

#[test]
fn intersection_construction_recovers_one_missing_term_from_unique_edge_endpoints() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let scan = crate::intersection::scan(&stream);
    assert_eq!(scan.constructions.len(), 1);
    assert_eq!(scan.curves.len(), 1);
    assert_eq!(
        scan.rejected,
        crate::intersection::RejectionCounts::default()
    );
}

#[test]
fn intersection_construction_rejects_missing_term_without_topology_endpoint_match() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    put_f64(&mut stream, chart + 60, 0.005);

    let scan = crate::intersection::scan(&stream);
    assert_eq!(scan.constructions.len(), 1);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
}

#[test]
fn decode_resolves_surface_curve_to_its_basis_curve() {
    let stream = surface_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.edges.len(), 1);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidSurfaceCurveRecord>("parasolid_surface_curve_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].surface_xmt, 6);
    assert_eq!(records[0].pcurve_xmt, 9);
    assert_eq!(records[0].original_curve_xmt, 9);
    assert_eq!(records[0].tolerance_to_original, 0.000_01);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_lifts_pcurve_only_fin_carrier_to_its_surface() {
    let mut stream = pcurve_topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let surface_curve = stream
        .windows(4)
        .position(|window| window == [0, 137, 0, 25])
        .expect("surface curve");
    put_ref(&mut stream, surface_curve + 23, 1);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result.ir.model.edges[0]
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| &curve.id == id))
        .expect("lifted carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Procedural { .. }));
    let ProceduralCurveDefinition::SurfaceCurve {
        family: cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric,
        context,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("parametric surface curve");
    };
    assert_eq!(
        context.sides[0].surface,
        Some(result.ir.model.faces[0].surface.clone())
    );
    assert!(context.sides[0].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_rolling_ball_blend_surface() {
    let stream = blend_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("blend surface");
    let ProceduralSurfaceDefinition::Blend {
        supports,
        radius,
        cross_section,
        spine,
        native,
    } = &procedural.definition
    else {
        panic!("blend definition");
    };
    assert_eq!(*cross_section, BlendCrossSection::Circular);
    assert_eq!(*radius, BlendRadiusLaw::Constant { signed_radius: 3.0 });
    assert_eq!(supports[0].as_ref().map(|side| side.reversed), Some(true));
    assert_eq!(supports[1].as_ref().map(|side| side.reversed), Some(false));
    assert!(spine.is_none());
    assert!(native.is_none());
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidBlendSurfaceRecord>("parasolid_blend_surface_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].support_xmts, [6, 6]);
    assert_eq!(records[0].spine_xmt, 1);
    assert_eq!(records[0].offsets, [-3.0, 3.0]);
    assert_eq!(records[0].thumb_weights, [1.0, 1.0]);
    let carrier = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .unwrap();
    assert_eq!(
        carrier
            .source_object
            .as_ref()
            .map(|association| association.object_id.as_str()),
        Some(records[0].id.as_str())
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_blend_with_extended_support_reference() {
    let stream = blend_surface_with_extended_support_reference();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.procedural_surfaces[0].surface
    );
}

#[test]
fn decode_binds_blend_ball_centre_spine() {
    let stream = blend_surface_with_intersection_spine();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let ProceduralSurfaceDefinition::Blend { spine, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend definition");
    };
    assert_eq!(
        spine.as_ref(),
        Some(&result.ir.model.procedural_curves[0].curve)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_blend_support_reference() {
    let stream = blend_surface_with_forward_blend_support();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 2);
    let ProceduralSurfaceDefinition::Blend { supports, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend definition");
    };
    assert_eq!(
        supports[0].as_ref().map(|support| &support.surface),
        Some(&result.ir.model.procedural_surfaces[1].surface)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_intersection_curve_as_connected_carrier() {
    let stream = intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let edge_curve = result.ir.model.edges[0].curve.as_ref().expect("edge curve");
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == edge_curve)
        .expect("intersection carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidIntersectionRecord>("parasolid_intersection_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert!(!records[0].delta_twin);
    assert_eq!(records[0].header_references[0], 1);
    assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
    assert_eq!(
        curve.source_object.as_ref().map(|source| &source.object_id),
        Some(&records[0].id)
    );
    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    assert_eq!(result.ir.model.procedural_curves[0].curve, curve.id);
    assert!(result.report.losses.iter().any(|loss| {
        loss.category == LossCategory::Geometry
            && loss.message.starts_with("1 surface-intersection record(s)")
    }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_deltas_intersection_data_curve() {
    let stream = deltas_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidIntersectionRecord>("parasolid_intersection_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].delta_twin);
    assert_eq!(records[0].header_references[0], 1);
    assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.procedural_curves[0].curve)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_reports_status_framed_deltas_records_and_tombstones() {
    let stream = status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert_eq!(
        attributes.get("deltas.0.full.FACE").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes
            .get("deltas.0.tombstone.EDGE")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes.get("deltas.0.grammar").map(String::as_str),
        Some("status_byte_framed_topology")
    );
}

#[test]
fn decode_accepts_exact_loop_and_rejects_incomplete_fin_deltas() {
    let stream = variable_status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert!(!attributes.contains_key("deltas.0.full.FIN"));
    assert_eq!(
        attributes.get("deltas.0.full.LOOP").map(String::as_str),
        Some("1")
    );
}

#[test]
fn deltas_point_exposes_typed_position_in_model_units() {
    let points = crate::deltas::points(&status_framed_deltas_point_stream());
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].xmt, 50);
    assert_eq!(points[0].node_id, 900);
    assert_eq!(points[0].position.x, 12.5);
    assert_eq!(points[0].position.y, -2.0);
    assert_eq!(points[0].position.z, 4.0);
}

#[test]
fn deltas_point_normalizes_to_partition_record_framing() {
    let record = crate::deltas::walk(&status_framed_deltas_point_stream())
        .records
        .remove(0);
    let mut expected = crate::tests::record(29, 40);
    put_ref(&mut expected, 2, 50);
    expected[4..8].copy_from_slice(&900u32.to_be_bytes());
    for at in [8, 10, 12, 14] {
        put_ref(&mut expected, at, 1);
    }
    put_vec3(&mut expected, 16, [0.0125, -0.002, 0.004]);
    assert_eq!(record.canonical_bytes, expected);
}

#[test]
fn deltas_intersection_normalizes_before_partition_style_decode() {
    let residual = crate::deltas::procedural_residual(&status_framed_deltas_intersection_stream());
    let intersections = crate::topology::composite_curves(&residual);
    assert_eq!(intersections.len(), 1);
    assert_eq!(intersections[0].xmt, 12);
    assert_eq!(intersections[0].references, [6, 7, 20, 21, 22, 23]);
}

#[test]
fn merged_deltas_full_record_replaces_partition_node() {
    let partition = topology_partition_stream();
    let mut deltas = status_framed_deltas_point_stream();
    deltas[2..4].copy_from_slice(&11u16.to_be_bytes());
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    let points = crate::geometry::points(&merged);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].position.x, 12.5);
    assert_eq!(points[0].position.y, -2.0);
    assert_eq!(points[0].position.z, 4.0);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
}

#[test]
fn merged_tombstone_preserves_a_topology_referenced_carrier() {
    let partition = topology_partition_stream();
    let mut tombstone = Vec::new();
    tombstone.extend_from_slice(&29u16.to_be_bytes());
    tombstone.extend_from_slice(&11u16.to_be_bytes());
    tombstone.extend_from_slice(&[0, 1]);
    let census = crate::deltas::walk(&tombstone);
    assert_eq!(census.tombstones.len(), 1);
    assert_eq!(census.tombstones[0].kind, 29);
    assert_eq!(census.tombstones[0].xmt, 11);
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

#[test]
fn merged_exact_key_tombstone_removes_unreferenced_partition_node() {
    let mut partition = record(29, 40);
    put_ref(&mut partition, 2, 11);
    put_vec3(&mut partition, 16, [0.01, 0.02, 0.03]);
    let tombstone = [0, 29, 0, 11, 0, 1];
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_none());
}

#[test]
fn merged_deltas_uses_last_full_or_tombstone_event() {
    let partition = topology_partition_stream();
    let tombstone = [0, 29, 0, 11, 0, 1];
    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&11u16.to_be_bytes());

    let mut delete_then_replace = tombstone.to_vec();
    delete_then_replace.extend_from_slice(&full);
    let merged = crate::deltas::merge_full_records(&partition, &delete_then_replace);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 12.5);

    let mut replace_then_delete = full;
    replace_then_delete.extend_from_slice(&tombstone);
    let merged = crate::deltas::merge_full_records(&partition, &replace_then_delete);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

#[test]
fn decode_emits_point_added_by_deltas_stream() {
    let mut cur = Cursor::new(prt_with_partition(&deltas_point_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_replaces_partition_point_with_same_xmt_deltas_point() {
    let partition = topology_partition_stream();
    let mut deltas = deltas_point_partition_stream();
    let record = deltas
        .windows(2)
        .rposition(|window| window == 29u16.to_be_bytes())
        .expect("deltas POINT");
    deltas[record + 2..record + 4].copy_from_slice(&11u16.to_be_bytes());
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_preserves_partition_edge_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_edge_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_face_and_vertex_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_face_vertex_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_loop_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_loop_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(15, 5)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_shell_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_shell_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(13, 3)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_fin_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_fin_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(
        result.ir.model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_line_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_line_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let CurveGeometry::Line { origin, direction } = result.ir.model.curves[0].geometry else {
        panic!("line");
    };
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0));
    assert_eq!(direction, Vector3::new(0.0, 1.0, 0.0));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_plane_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_plane_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { origin, normal, u_axis }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && normal == Vector3::new(0.0, 1.0, 0.0)
                && u_axis == Vector3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_circle_from_status_framed_deltas() {
    let partition = circle_topology_partition_stream();
    let deltas = deltas_circle_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Circle { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_ellipse_from_status_framed_deltas() {
    let partition = ellipse_topology_partition_stream();
    let deltas = deltas_ellipse_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && major_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 30.0
            && minor_radius == 12.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cylinder_from_status_framed_deltas() {
    let partition = cylinder_topology_partition_stream();
    let deltas = deltas_cylinder_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { origin, axis, ref_direction, radius }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cone_from_status_framed_deltas() {
    let partition = cone_topology_partition_stream();
    let deltas = deltas_cone_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { origin, axis, ref_direction, radius, ratio, half_angle }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
                && ratio == 1.0
                && (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_sphere_from_status_framed_deltas() {
    let partition = sphere_topology_partition_stream();
    let deltas = deltas_sphere_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Sphere { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_torus_from_status_framed_deltas() {
    let partition = torus_topology_partition_stream();
    let deltas = deltas_torus_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 40.0
            && minor_radius == 15.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_charted_surface_intersection_construction() {
    let stream = charted_intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let terms = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidTermUseRecord>("parasolid_term_use_records")
        .unwrap();
    assert_eq!(terms.len(), 2);
    assert_eq!(terms[0].count, 1);
    assert_eq!(terms[0].form, "L?");
    assert_eq!(terms[0].point, [0.0, 0.0, 0.0]);
    assert_eq!(terms[1].point, [10.0, 0.0, 0.0]);
    assert!(terms
        .iter()
        .all(|term| matches!(term.framing, crate::native::ParasolidTermUseFraming::Direct)));
    let support_uv = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidSupportUvRecord>("parasolid_support_uv_records")
        .unwrap();
    assert_eq!(support_uv.len(), 1);
    assert_eq!(support_uv[0].count, 4);
    assert_eq!(support_uv[0].marker, 2);
    assert_eq!(support_uv[0].values, [0.0, 0.0, 0.01, 0.0]);
    assert!(matches!(
        support_uv[0].framing,
        crate::native::ParasolidSupportUvFraming::Direct
    ));
    let charts = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidChartRecord>("parasolid_chart_records")
        .unwrap();
    assert_eq!(charts.len(), 1);
    assert_eq!(charts[0].count, 2);
    assert_eq!(charts[0].base_parameter, 0.0);
    assert_eq!(charts[0].base_scale, 1.0);
    assert_eq!(charts[0].chart_count, 2);
    assert_eq!(charts[0].chordal_error, 0.000_01);
    assert_eq!(charts[0].angular_error, 0.001);
    assert_eq!(charts[0].points, [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
    assert!(matches!(
        charts[0].point_layout,
        crate::native::ParasolidChartPointLayout::Xyz3
    ));

    let procedural = result
        .ir
        .model
        .procedural_curves
        .first()
        .expect("intersection construction");
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .expect("solved chart cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("charted NURBS cache");
    };
    assert_eq!(nurbs.degree, 1);
    assert_eq!(nurbs.control_points[0].x, 0.0);
    assert_eq!(nurbs.control_points[1].x, 10.0);
    assert_eq!(procedural.cache_fit_tolerance, Some(0.01));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &procedural.definition
    else {
        panic!("typed surface intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_none());
    assert_eq!(context.parameter_range, [0.0, 0.01]);
    assert!(result.ir.model.coedges[0].pcurve.is_none());
    assert!(!result.report.losses.iter().any(|loss| {
        loss.category == LossCategory::Geometry
            && loss.message.contains("surface-intersection record(s)")
    }));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn intersection_pcurve_attachment_requires_face_incidence() {
    let ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId("synthetic:cube:edge#0".into());
    let surface = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge && coedge.id.0.contains("bottom"))
        .and_then(|coedge| {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?;
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support surface");
    let pcurve = |end| PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), end],
        weights: None,
        periodic: false,
    };

    assert!(crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 0.0)),
        None,
    ));
    assert!(!crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 5.0)),
        None,
    ));
}

#[test]
fn decode_derives_analytic_support_uv_without_serialized_values() {
    let stream = charted_intersection_without_uv_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("intersection definition");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_accepts_intersection_terms_within_chart_tolerance() {
    let stream = charted_intersection_with_approximated_term_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_ext11_deltas_intersection_chart() {
    let stream = ext11_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let curve_id = &result.ir.model.procedural_curves[0].curve;
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("intersection cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("NURBS chart cache");
    };
    assert_eq!(nurbs.control_points[1].x, 10.0);
    assert_eq!(nurbs.knots, vec![2.0, 2.0, 5.0, 5.0]);
}

#[test]
fn decode_assigns_ext11_uv_lanes_by_unique_surface_evaluation() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let [Some(PcurveGeometry::Nurbs {
        control_points: first,
        ..
    }), Some(PcurveGeometry::Nurbs {
        control_points: second,
        ..
    })] = context.sides.clone().map(|side| side.pcurve)
    else {
        panic!("two ext11 pcurves");
    };
    assert_eq!(first, [Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)]);
    assert_eq!(second, [Point2::new(0.0, 0.0), Point2::new(0.0, 10.0)]);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn ext11_uv_assignment_eliminates_the_complementary_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surfaces = [
        result.ir.model.surfaces[0].id.clone(),
        result.ir.model.surfaces[1].id.clone(),
    ];
    result.ir.model.surfaces[1].geometry = SurfaceGeometry::Unknown { record: None };
    let lanes = [
        Some(vec![[0.0, 0.0], [0.01, 0.0]]),
        Some(vec![[0.0, 0.0], [0.0, 0.01]]),
    ];

    let assigned = crate::decode::assign_ext11_support_uv_to_surfaces(
        &result.ir,
        [&surfaces[0], &surfaces[1]],
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        0.01,
        &lanes,
    )
    .unwrap();

    assert_eq!(assigned, lanes);
}

#[test]
fn decode_replaces_ambiguous_ext11_uv_lanes_from_analytic_supports() {
    let stream = two_support_ext11_charted_intersection_curve_stream(true);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_completes_one_non_sentinel_ext11_uv_lane_analytically() {
    let stream = partial_ext11_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn completed_intersection_support_lane_attaches_after_topology_emission() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId("synthetic:cube:edge#0".into());
    let target = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge && coedge.id.0.contains("bottom"))
        .expect("bottom coedge");
    target.id = cadmpeg_ir::ids::CoedgeId("nx:s0:fin#42".into());
    target.pcurve = None;
    let owner_loop = target.owner_loop.clone();
    let surface = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == owner_loop)
        .and_then(|loop_| {
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support");
    let curve = ir
        .model
        .edges
        .iter()
        .find(|candidate| candidate.id == edge)
        .and_then(|edge| edge.curve.clone())
        .expect("edge curve");
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve {
            id: cadmpeg_ir::ids::ProceduralCurveId("nx:test:intersection#0".into()),
            curve,
            definition: ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext {
                    sides: [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve: Some(PcurveGeometry::Nurbs {
                                degree: 1,
                                knots: vec![0.0, 0.0, 1.0, 1.0],
                                control_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
                                weights: None,
                                periodic: false,
                            }),
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let source_stream = annotations.stream("nx:test");

    crate::decode::attach_completed_intersection_pcurves(
        &mut ir,
        &crate::topology::Graph::parse(&[]),
        "nx:s0",
        source_stream,
        &mut annotations,
    );

    let completed = ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.0.contains("intersection-pcurve-completed"))
        .expect("validated completed support lane attaches");
    assert!(ir
        .model
        .coedges
        .iter()
        .any(|coedge| coedge.pcurve.as_ref() == Some(&completed.id)));
}

#[test]
fn ext11_uv_completion_runs_after_support_incidence_resolution() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [
            Some(vec![[0.0, 0.0], [0.01, 0.0]]),
            Some(vec![[0.0, 0.0], [0.0, 0.01]]),
        ],
    )];

    crate::decode::complete_ext11_support_uv(&mut result.ir, &pending);

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn analytic_uv_completion_fills_missing_intersection_support_lanes() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [None, None],
    )];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn analytic_uv_completion_replaces_a_sentinel_contaminated_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[0].pcurve.as_mut()
    else {
        panic!("NURBS support lane");
    };
    control_points[1] = Point2::new(
        crate::decode::MISSING_TOLERANCE,
        crate::decode::MISSING_TOLERANCE,
    );
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [None, None],
    )];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[0].pcurve.as_ref()
    else {
        panic!("NURBS support lane");
    };
    assert!(control_points.iter().all(|point| {
        point.u.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
            && point.v.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
    }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn equivalent_offset_supports_share_a_complete_parameter_lane() {
    use cadmpeg_ir::geometry::{ProceduralCurve, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let supports = [SurfaceId("support-a".into()), SurfaceId("support-b".into())];
    for support in &supports {
        ir.model.surfaces.push(Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let offsets = [SurfaceId("offset-a".into()), SurfaceId("offset-b".into())];
    for (ordinal, (surface, support)) in offsets.iter().zip(&supports).enumerate() {
        let construction = ProceduralSurfaceId(format!("offset-construction-{ordinal}"));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: construction,
            surface: surface.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: 30.0,
                u_sense: 0,
                v_sense: 0,
                extension_flags: Vec::new(),
            },
            cache_fit_tolerance: None,
        });
    }
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("intersection".into()),
        curve: CurveId("curve".into()),
        definition: ProceduralCurveDefinition::Intersection {
            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                sides: [
                    cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: Some(offsets[0].clone()),
                        pcurve: None,
                    },
                    cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: Some(offsets[1].clone()),
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(1.0, 2.0),
                            direction: Point2::new(3.0, 4.0),
                        }),
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });

    assert!(crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    crate::decode::complete_parameterization_equivalent_support_uv(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[0].pcurve, context.sides[1].pcurve);

    let ProceduralSurfaceDefinition::Offset { distance, .. } =
        &mut ir.model.procedural_surfaces[1].definition
    else {
        unreachable!()
    };
    *distance = 31.0;
    assert!(!crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
}

#[test]
fn nurbs_parameter_solver_inverts_a_rational_surface_point() {
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
        ],
        weights: Some(vec![1.0, 2.0, 3.0, 4.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.37, 0.61);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual = crate::decode::nurbs_parameters(&surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);

    let after_invalid_seed =
        crate::decode::nurbs_parameters(&surface, point, Some(Point2::new(f64::NAN, 0.5))).unwrap();
    assert!((after_invalid_seed.u - expected.u).abs() < 1.0e-10);
    assert!((after_invalid_seed.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn surface_intersection_continuation_corrects_a_chart_selected_branch() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first-intersection-plane".into());
    let second = SurfaceId("synthetic:second-intersection-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let chart = vec![
        Point3::new(1.0e-4, -2.0e-4, 0.0),
        Point3::new(-1.0e-4, 2.0e-4, 2.0),
        Point3::new(2.0e-4, 1.0e-4, 5.0),
    ];
    let lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &chart,
        1.0e-3,
    )
    .unwrap();
    assert_eq!(lanes[0].len(), chart.len());
    for (ordinal, expected_z) in [0.0, 2.0, 5.0].into_iter().enumerate() {
        let first_point = cadmpeg_ir::eval::model_surface_point(
            &ir,
            &first,
            lanes[0][ordinal].u,
            lanes[0][ordinal].v,
        )
        .unwrap();
        let second_point = cadmpeg_ir::eval::model_surface_point(
            &ir,
            &second,
            lanes[1][ordinal].u,
            lanes[1][ordinal].v,
        )
        .unwrap();
        assert!((first_point.x - second_point.x).abs() < 1.0e-10);
        assert!((first_point.y - second_point.y).abs() < 1.0e-10);
        assert!((first_point.z - second_point.z).abs() < 1.0e-10);
        assert!((first_point.z - expected_z).abs() < 1.0e-10);
    }

    let off_branch = [chart[0], Point3::new(1.0, 1.0, 2.0)];
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &off_branch,
        1.0e-3,
    )
    .is_none());
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &first],
        &chart,
        1.0e-3,
    )
    .is_none());

    let cylinder = SurfaceId("synthetic:intersection-cylinder".into());
    let section_plane = SurfaceId("synthetic:intersection-section-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: section_plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let circular_chart =
        [0.0_f64, 0.3, 0.8].map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let circular_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &circular_chart,
        1.0e-3,
    )
    .unwrap();
    for (cylinder_uv, plane_uv) in circular_lanes[0].iter().zip(&circular_lanes[1]) {
        let cylinder_point =
            cadmpeg_ir::eval::model_surface_point(&ir, &cylinder, cylinder_uv.u, cylinder_uv.v)
                .unwrap();
        let plane_point =
            cadmpeg_ir::eval::model_surface_point(&ir, &section_plane, plane_uv.u, plane_uv.v)
                .unwrap();
        assert!((cylinder_point.x - plane_point.x).abs() < 1.0e-8);
        assert!((cylinder_point.y - plane_point.y).abs() < 1.0e-8);
        assert!((cylinder_point.z - plane_point.z).abs() < 1.0e-8);
    }
}

#[test]
fn nurbs_parameter_solver_rejects_a_remote_local_minimum_seed() {
    let mut control_points = Vec::new();
    for (x, z) in [
        (-10.0, 0.0),
        (0.0, 0.0),
        (10.0, 2.0),
        (0.0, 4.0),
        (-10.0, 4.0),
    ] {
        control_points.extend([
            cadmpeg_ir::math::Point3::new(x, 0.0, z),
            cadmpeg_ir::math::Point3::new(x, 10.0, z),
        ]);
    }
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points,
        weights: None,
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.125, 0.3);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual =
        crate::decode::nurbs_parameters(&surface, point, Some(Point2::new(0.875, 0.3))).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn nurbs_curve_closest_parameter_does_not_trust_a_remote_seed() {
    use cadmpeg_ir::geometry::{Curve, NurbsCurve};
    use cadmpeg_ir::ids::CurveId;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let curve = CurveId("synthetic:piecewise-spine".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(-10.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
            ],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });

    let actual = crate::decode::closest_spine_parameter(
        &ir,
        &curve,
        cadmpeg_ir::math::Point3::new(-5.0, 2.0, 0.0),
        Some(0.9),
    )
    .unwrap();

    assert!((actual - 0.25).abs() < 1.0e-10);
}

#[test]
fn spine_contact_pcurve_inverts_linear_and_rational_support_parameters() {
    let pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![2.0, 2.0, 5.0, 9.0, 9.0],
        control_points: vec![
            Point2::new(-1.0, 3.0),
            Point2::new(2.0, 6.0),
            Point2::new(6.0, 4.0),
        ],
        weights: None,
        periodic: false,
    };

    let first = crate::decode::closest_pcurve_parameter(&pcurve, Point2::new(0.5, 4.5)).unwrap();
    let second = crate::decode::closest_pcurve_parameter(&pcurve, Point2::new(5.0, 4.5)).unwrap();

    assert!((first - 3.5).abs() < 1.0e-12);
    assert!((second - 8.0).abs() < 1.0e-12);

    let rational = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        weights: Some(vec![1.0, 2.0]),
        periodic: false,
    };
    let rational_parameter =
        crate::decode::closest_pcurve_parameter(&rational, Point2::new(0.5, 0.0)).unwrap();
    assert!((rational_parameter - 1.0 / 3.0).abs() < 1.0e-10);

    let quadratic = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(2.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let quadratic_parameter =
        crate::decode::closest_pcurve_parameter(&quadratic, Point2::new(1.0, 0.5)).unwrap();
    assert!((quadratic_parameter - 0.5).abs() < 1.0e-10);
}

#[test]
fn blend_contact_offset_requires_the_serialized_range_sign() {
    assert!(crate::decode::blend_contact_offset_matches(
        2.0, 5.0, 3.0, false
    ));
    assert!(crate::decode::blend_contact_offset_matches(
        2.0, -1.0, 3.0, true
    ));
    assert!(!crate::decode::blend_contact_offset_matches(
        2.0, -1.0, 3.0, false
    ));
    assert!(!crate::decode::blend_contact_offset_matches(
        2.0, 5.0, 3.0, true
    ));
    assert!(!crate::decode::blend_contact_offset_matches(
        2.0,
        f64::from_bits(5.0f64.to_bits() + 1),
        3.0,
        false,
    ));
}

#[test]
fn blend_contact_matches_separate_analytic_offset_carriers() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let support = SurfaceId("synthetic:support-cylinder".into());
    let offset = SurfaceId("synthetic:offset-cylinder".into());
    let cylinder = |id, radius| Surface {
        id,
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(-46.75, 0.0, -112.06),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 0.0, -1.0),
            radius,
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        cylinder(support.clone(), 294.0),
        cylinder(offset.clone(), 299.0),
    ]);

    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Cylinder { origin, .. } = &mut ir.model.surfaces[1].geometry else {
        unreachable!()
    };
    origin.y = 1.0;
    assert!(crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0).is_none());
}

#[test]
fn rolling_ball_blend_parameters_invert_the_canal_surface_law() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first-plane".into());
    let second = SurfaceId("synthetic:second-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let spine = CurveId("synthetic:spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let surface = SurfaceId("synthetic:blend".into());
    let construction = ProceduralSurfaceId("synthetic:blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: construction,
        surface: surface.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: first,
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: second,
                    reversed: false,
                }),
            ],
            spine: Some(spine.clone()),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
    });
    let expected = Point2::new(8.0, 0.35);
    let point = crate::decode::blend_surface_point(&ir, &surface, expected.u, expected.v).unwrap();

    assert_eq!(
        crate::decode::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        0.25
    );
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("synthetic:spine-construction".into()),
        curve: spine.clone(),
        definition: ProceduralCurveDefinition::Unknown { record: None },
        cache_fit_tolerance: Some(0.75),
    });
    assert_eq!(
        crate::decode::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        1.0
    );

    let actual = crate::decode::blend_surface_parameters(&ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .unwrap()
        .geometry = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 10.0, 10.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 10.0),
        ],
        weights: None,
        periodic: false,
    });
    let coarse = crate::decode::coarse_blend_surface_parameters(&ir, &surface, point, 0).unwrap();
    let coarse_point =
        crate::decode::blend_surface_point(&ir, &surface, coarse.u, coarse.v).unwrap();
    assert!(
        ((coarse_point.x - point.x).powi(2)
            + (coarse_point.y - point.y).powi(2)
            + (coarse_point.z - point.z).powi(2))
        .sqrt()
            < 1.0
    );

    let refined = crate::decode::refine_blend_surface_parameters(
        &ir,
        &surface,
        point,
        Point2::new(expected.u + 0.5, expected.v + 0.1),
        0,
    )
    .unwrap();
    let refined_point =
        crate::decode::blend_surface_point(&ir, &surface, refined.u, refined.v).unwrap();
    let refined_error = ((refined_point.x - point.x).powi(2)
        + (refined_point.y - point.y).powi(2)
        + (refined_point.z - point.z).powi(2))
    .sqrt();
    assert!(refined_error < 1.0e-9);

    let third = SurfaceId("synthetic:third-plane".into());
    ir.model.surfaces.push(Surface {
        id: third.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 8.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer_spine = CurveId("synthetic:outer-spine".into());
    ir.model.curves.push(Curve {
        id: outer_spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(4.0, 6.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer = SurfaceId("synthetic:outer-blend".into());
    let outer_construction = ProceduralSurfaceId("synthetic:outer-blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: outer.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: outer_construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: outer_construction,
        surface: outer.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface,
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: third,
                    reversed: false,
                }),
            ],
            spine: Some(outer_spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 1.5 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
    });
    let expected = Point2::new(4.0, 0.2);
    let point = crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).unwrap();
    let actual = crate::decode::blend_surface_parameters(&ir, &outer, point, None).unwrap();
    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| candidate.surface == outer)
        .unwrap();
    let ProceduralSurfaceDefinition::Blend { supports, .. } = &mut outer_definition.definition
    else {
        panic!("blend definition");
    };
    supports[0].as_mut().unwrap().surface = outer.clone();
    assert!(crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).is_none());
}

#[test]
fn decode_emits_both_intersection_support_pcurves() {
    let stream = two_support_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_intersection_second_support_through_blend_bound() {
    let stream = blend_bound_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidBlendBoundRecord>("parasolid_blend_bound_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].header_references, [1; 5]);
    assert!(records[0].sense);
    assert_eq!(records[0].boundary_index, 0);
    assert_eq!(records[0].blend_surface_xmt, 13);
    assert!(!records[0].escaped);

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let second = context.sides[1].surface.as_ref().expect("bridged support");
    assert_ne!(context.sides[0].surface.as_ref(), Some(second));
    assert!(context.sides[1].pcurve.is_some());
}

#[test]
fn decode_emits_inline_descriptor_intersection_witnesses() {
    let stream = inline_descriptor_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
    ));
    assert!(matches!(
        result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
            .expect("intersection curve")
            .geometry,
        CurveGeometry::Nurbs(_)
    ));
}

#[test]
fn decode_emits_topology_when_record_xmt_uses_extended_encoding() {
    let stream = large_xmt_headers(&topology_partition_stream());
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_maps_parasolid_tolerance_sentinel_to_none() {
    let stream = topology_with_missing_tolerances();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.vertices[0].tolerance, None);
    assert_eq!(result.ir.model.edges[0].tolerance, None);
    assert_eq!(result.ir.model.faces[0].tolerance, None);
}

#[test]
fn decode_dual_writes_inline_entity_metadata_to_annotations() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let ir = &result.ir;

    macro_rules! assert_arena_annotations {
        ($arena:expr) => {
            for entity in $arena {
                let provenance = ir
                    .annotations
                    .provenance
                    .get(&entity.id.to_string())
                    .expect("annotation provenance");
                assert!(ir.annotations.streams[provenance.stream as usize].starts_with("nx:"));
                assert!(provenance.tag.is_some());
            }
        };
    }

    assert_arena_annotations!(&ir.model.bodies);
    assert_arena_annotations!(&ir.model.regions);
    assert_arena_annotations!(&ir.model.shells);
    assert_arena_annotations!(&ir.model.faces);
    assert_arena_annotations!(&ir.model.loops);
    assert_arena_annotations!(&ir.model.coedges);
    assert_arena_annotations!(&ir.model.edges);
    assert_arena_annotations!(&ir.model.vertices);
    assert_arena_annotations!(&ir.model.points);
    assert_arena_annotations!(&ir.model.surfaces);
    assert_arena_annotations!(&ir.model.curves);
    let unknowns = ir.native_unknowns("nx").unwrap();
    assert_arena_annotations!(&unknowns);

    let point_note = &ir.annotations.exactness[&ir.model.points[0].id.to_string()];
    assert_eq!(point_note.entity, Exactness::ByteExact);
    assert_eq!(point_note.fields["position"], Exactness::Derived);
    let surface_note = &ir.annotations.exactness[&ir.model.surfaces[0].id.to_string()];
    assert_eq!(surface_note.fields["geometry"], Exactness::Derived);
    let curve_note = &ir.annotations.exactness[&ir.model.curves[0].id.to_string()];
    assert_eq!(curve_note.fields["geometry"], Exactness::Derived);
    for id in [
        ir.model.vertices[0].id.to_string(),
        ir.model.edges[0].id.to_string(),
        ir.model.faces[0].id.to_string(),
    ] {
        assert_eq!(
            ir.annotations.exactness[&id].fields["tolerance"],
            Exactness::Derived
        );
    }
}

#[test]
fn decode_transfers_bspline_surface_and_curve() {
    let stream = bspline_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(surface) => Some(surface),
            _ => None,
        })
        .expect("B-spline surface");
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert!((surface.control_points[1].y - 20.0).abs() < 1e-9);
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(curve) => Some(curve),
            _ => None,
        })
        .expect("B-spline curve");
    assert_eq!(curve.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(curve.control_points.len(), 2);
    assert!((curve.control_points[1].x - 20.0).abs() < 1e-9);
}

#[test]
fn nurbs_decodes_extended_xmt_arrays_payload_and_long_surface_descriptor() {
    let surfaces = crate::nurbs::surfaces(&extended_bspline_surface_stream());
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert_eq!(surface.control_points[3].y, 20.0);
}

#[test]
fn nurbs_decodes_escaped_curve_descriptor_and_payload_count() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    stream.insert(descriptor + 2, 0xff);
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    stream.insert(payload + 2, 0xff);
    stream.insert(payload + 10, 0xff);

    let curves = crate::nurbs::curves(&stream);
    assert_eq!(curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    assert_eq!(curve.control_points.len(), 2);
    assert_eq!(curve.control_points[1].x, 20.0);
}

#[test]
fn decode_replaces_partition_bspline_surface_wrapper_from_deltas() {
    let partition = bspline_surface_replacement_partition_stream();
    let deltas = deltas_bspline_surface_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 30.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_bspline_curve_wrapper_from_deltas() {
    let partition = bspline_curve_replacement_partition_stream();
    let deltas = deltas_bspline_curve_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 10.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_trimmed_edge_to_its_basis_curve_and_range() {
    let mut cur = Cursor::new(prt_with_partition(&trimmed_topology_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir.model.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidTrimmedCurveRecord>("parasolid_trimmed_curve_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].basis_xmt, 9);
    assert_eq!(records[0].points, [[0.0; 3]; 2]);
    assert_eq!(records[0].parameters, [0.000_25, 0.000_75]);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_uses_partner_fin_vertex_for_edge_endpoint() {
    let mut cur = Cursor::new(prt_with_partition(
        &partnered_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_ne!(edge.start, edge.end);
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert_eq!(result.ir.model.coedges.len(), 2);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_trimmed_curve_chain() {
    let mut cur = Cursor::new(prt_with_partition(&forward_trimmed_curve_chain_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir.model.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_a_curve_when_its_trim_range_misses_edge_vertices() {
    let mut cur = Cursor::new(prt_with_partition(
        &mismatched_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    let carrier = edge
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| curve.id == *id))
        .expect("edge carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Line { .. }));
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_extended_xmt_reference_inside_edge_record() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_curve_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_extended_face_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_face_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_extended_edge_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_all_extended_topology_reference_shifts() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_internal_topology_references(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.points[0].position.x, 10.0);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_fully_extended_geometry_header_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_fully_extended_geometry_headers(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
}

#[test]
fn decode_tracks_geometry_envelope_escape_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_escaped_geometry_envelopes(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn cylinder_gate_rejects_denormal_radius() {
    // A coincidental byte alignment can present a unit axis and a model-scale
    // origin alongside a denormal (near-zero) double at the radius slot; the radius
    // floor must reject it rather than emit a fabricated zero-radius cylinder.
    let mut cy = record(0x33, 99);
    put_vec3(&mut cy, 19, [0.003_175, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, f64::from_bits(1)); // smallest positive subnormal
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    assert!(crate::geometry::surfaces(&cy).is_empty());
}

#[test]
fn decode_assembly_reports_external_dependency() {
    let mut cur = Cursor::new(assembly_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.message.contains("assembly")));
}

#[test]
fn assembly_metadata_lists_external_child_paths() {
    let mut cur = Cursor::new(assembly_with_external_paths());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attrs = &result.ir.source.expect("source").attributes;
    assert_eq!(
        attrs.get("external_reference.0").map(String::as_str),
        Some("child.prt")
    );
    assert_eq!(
        attrs.get("external_reference.1").map(String::as_str),
        Some("nested/b.prt")
    );
    let references = result
        .ir
        .native
        .namespace("nx")
        .expect("NX native namespace")
        .arena_as::<crate::native::ExternalReference>("external_references")
        .expect("typed external references");
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].ordinal, 0);
    assert_eq!(references[0].path, "child.prt");
    assert_eq!(references[1].ordinal, 1);
    assert_eq!(references[1].path, "nested/b.prt");
    assert!(references[0].source_offset < references[1].source_offset);
}

#[test]
fn external_reference_string_table_is_end_anchored() {
    let table = b"prefix\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let (_, strings) = crate::container::parse_extref_string_table(table).expect("string table");
    assert_eq!(
        strings
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        ["child.prt", "nested/b.prt"]
    );

    let mut trailed = table.to_vec();
    trailed.push(0);
    assert!(crate::container::parse_extref_string_table(&trailed).is_none());
    assert!(crate::container::parse_extref_string_table(b"\x01\xff\xff\xff\xff").is_none());
}

#[test]
fn external_reference_record_parser_requires_sorted_doubled_handle_set() {
    let mut payload = b"EXTREFSTREAM".to_vec();
    payload.extend_from_slice(&3u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.push(0);
    payload.extend_from_slice(&6u32.to_le_bytes());
    payload.extend_from_slice(&41u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(payload.len(), 41);
    payload.extend_from_slice(&[1, 0, 0, 0]);
    payload.extend_from_slice(&2u16.to_be_bytes());
    payload.push(1);
    for value in [8u32, 11, 12, 4] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[1, 4]);
    for handle in [0x1020_3040u32, 0x2030_4050, 0x2030_4050] {
        payload.push(0xe0);
        payload.extend_from_slice(&handle.to_be_bytes());
    }
    payload.push(4);
    payload.extend_from_slice(b"\x01\x01\x00\x00\x00\x09\x00child.prt");

    let records = crate::container::parse_extref_records(&payload);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, 6);
    assert_eq!(records[0].declared_count, 2);
    assert_eq!(records[0].id_slots, [8, 11, 12, 4]);
    assert_eq!(records[0].handles, [0x1020_3040, 0x2030_4050]);
    assert!(records[0].closing_duplicate);
    assert_eq!(records[0].tail_byte_len, 0);

    let duplicate = payload
        .windows(5)
        .rposition(|window| window == [0xe0, 0x20, 0x30, 0x40, 0x50])
        .expect("closing duplicate");
    payload[duplicate + 1] = 0x10;
    assert!(crate::container::parse_extref_records(&payload).is_empty());
}

#[test]
fn persistent_handle_identity_bridges_om_and_external_records() {
    let reference = crate::native::ObjectReference {
        id: "nx:test:reference#0".into(),
        record: "nx:test:om-record#0".into(),
        object_id: Some(1),
        ordinal: 0,
        kind: crate::native::ObjectReferenceKind::PersistentHandle,
        value: 0x1020_3040,
        target_record: None,
        source_entry: "om".into(),
        source_offset: 0,
    };
    let external = crate::native::ExternalReferenceRecord {
        id: "nx:test:external-record#6".into(),
        record_id: 6,
        declared_count: 1,
        id_slots: [0; 4],
        handles: vec![0x1020_3040],
        closing_duplicate: false,
        prefix_byte_len: 31,
        tail_byte_len: 0,
        source_entry: "external".into(),
        source_offset: 10,
    };
    let control = crate::native::DataBlockControlReference {
        id: "nx:test:control-reference#0".into(),
        data_block: "nx:test:control-block#0".into(),
        ordinal: 0,
        kind: crate::native::ObjectReferenceKind::PersistentHandle,
        value: 0x1020_3040,
        source_offset: 20,
    };

    let handles = crate::native::persistent_handles(&[reference], &[control], &[external]);

    assert_eq!(handles.len(), 1);
    assert_eq!(handles[0].records, ["nx:test:om-record#0"]);
    assert_eq!(handles[0].occurrence_count, 2);
    assert_eq!(handles[0].data_blocks, ["nx:test:control-block#0"]);
    assert_eq!(handles[0].external_records, ["nx:test:external-record#6"]);
}

#[test]
fn nx_control_handle_pairs_require_maximal_runs_of_exactly_two() {
    let reference = |ordinal: u32, offset: u64| crate::native::DataBlockControlReference {
        id: format!("reference#{ordinal}"),
        data_block: "block#0".into(),
        ordinal,
        kind: crate::native::ObjectReferenceKind::PersistentHandle,
        value: ordinal + 100,
        source_offset: offset,
    };
    let references = [
        reference(0, 10),
        reference(1, 15),
        reference(2, 30),
        reference(3, 35),
        reference(4, 40),
    ];
    let pairs = crate::native::data_block_control_handle_pairs(&references);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].id, "nx:om-data-block-control:handle-pair#10");
    assert_eq!(pairs[0].first_reference, "reference#0");
    assert_eq!(pairs[0].second_reference, "reference#1");
    assert_eq!(pairs[0].first_handle, 100);
    assert_eq!(pairs[0].second_handle, 101);
}

#[test]
fn container_reads_rmfastload_active_ids() {
    let container = container::scan_bytes(rmfastload_prt()).unwrap();
    assert_eq!(
        container.rmfastload_object_ids(),
        (1..=50).collect::<Vec<_>>()
    );
}

#[test]
fn decode_selects_dominant_rmfastload_body() {
    let mut cur = Cursor::new(prt_with_two_bodies_and_rmfastload());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(result.ir.model.bodies[0].id.0.starts_with("nx:s0:"));
    assert_eq!(result.ir.model.faces.len(), 50);
    assert_eq!(result.ir.model.surfaces.len(), 50);
    assert!(result
        .ir
        .model
        .faces
        .iter()
        .all(|face| face.id.0.starts_with("nx:s0:")));
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.0.starts_with("nx:s0:")));
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("active_body_selector"))
            .map(String::as_str),
        Some("rmfastload_object_id_membership")
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.findings.is_empty(),
        "findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_retains_every_rmfastload_active_body() {
    let mut cur = Cursor::new(prt_with_two_active_bodies_and_rmfastload());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert_eq!(result.ir.model.faces.len(), 100);
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("rmfastload_active_body_count"))
            .map(String::as_str),
        Some("2")
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_selects_active_shell_when_body_record_is_absent() {
    let mut cur = Cursor::new(prt_with_missing_active_body_record());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(result.ir.model.bodies[0].id.0.starts_with("nx:s0:"));
    assert_eq!(result.ir.model.faces.len(), 50);
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_keeps_bodies_when_rmfastload_overlap_is_weak() {
    let mut cur = Cursor::new(prt_with_weak_rmfastload_overlap());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert!(result
        .ir
        .source
        .as_ref()
        .is_none_or(|source| !source.attributes.contains_key("active_body_selector")));
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("sub-body partition")));
}

#[test]
fn container_only_preserves_streams_without_geometry() {
    let mut cur = Cursor::new(single_part_prt());
    let opts = DecodeOptions {
        container_only: true,
    };
    let result = NxCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result.report.container_only);
    assert_eq!(result.ir.native_unknowns("nx").unwrap().len(), 1);
    assert!(result.ir.model.points.is_empty());
}

#[test]
fn inspect_enumerates_streams_and_names_schema() {
    let mut cur = Cursor::new(single_part_prt());
    let summary = NxCodec.inspect(&mut cur).unwrap();
    assert_eq!(summary.format, "nx");
    assert_eq!(summary.container_kind, "splmsstr");
    assert!(summary.entries.iter().any(|e| e.role == "parasolid-stream"));
    assert!(summary.notes.iter().any(|n| n.contains("partition")));
}

#[test]
fn extraction_uses_ug_part_bounds_and_all_standard_zlib_headers() {
    let part = zlib_compress_at_level(&partition_stream(), 6);
    assert_eq!(&part[..2], b"\x78\x9c");

    let mut decoy_stream = partition_stream();
    let schema = b"SCH_TEST_1_9999";
    let decoy = b"SCH_FAKE_1_9999";
    let pos = decoy_stream
        .windows(schema.len())
        .position(|w| w == schema)
        .unwrap();
    decoy_stream[pos..pos + schema.len()].copy_from_slice(decoy);
    let decoy = zlib_compress(&decoy_stream);

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(0x06);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let entries = [
        (b"/Root/UG_PART/UG_PART".as_slice(), part.len()),
        (b"/Root/FastLoad/JT".as_slice(), decoy.len()),
    ];
    let directory_len: usize = entries.iter().map(|(name, _)| 4 + name.len() + 16).sum();
    let mut next_offset = file.len() + directory_len;
    for (name, size) in &entries {
        file.extend_from_slice(&(name.len() as u32).to_le_bytes());
        file.extend_from_slice(name);
        file.extend_from_slice(&(next_offset as u64).to_le_bytes());
        file.extend_from_slice(&(*size as u64).to_le_bytes());
        next_offset += size;
    }
    file.extend_from_slice(&part);
    file.extend_from_slice(&decoy);

    let streams = parasolid::extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].schema.as_deref(), Some("SCH_TEST_1_9999"));
}
