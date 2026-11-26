// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::SldprtCodec;

use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::features::parameters::{
    DesignConfiguration, DesignParameter, ParameterValue, PmiDimensionSubtype,
};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId};
use cadmpeg_ir::scalar::{FiniteReal, Length};

use super::{
    dimension_subtype, exact_count, neutral_parameter_is_count, patch_payload, patch_slots,
};

fn parse_payload(
    payload: &[u8],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Vec<PmiDimension> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("PMI test input fits service policy");
    super::parse_payload(&ctx, payload, losses).expect("PMI test input fits service policy")
}

fn pmi_limit_refusal(
    policy: cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::decode::ResourceLimit {
    let payload = pmi_semantic_payload();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("PMI test input fits the root limit");
    let mut losses = Vec::new();
    let error = super::parse_payload(&ctx, &payload, &mut losses)
        .expect_err("PMI parser must refuse the selected limit");
    assert!(
        losses.is_empty(),
        "resource refusal must not become a PMI loss"
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected a resource refusal");
    };
    limit
}

#[test]
fn pmi_payload_reports_nesting_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth = 2;
    let limit = pmi_limit_refusal(policy);
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RecursionDepth
    );
    assert_eq!(limit.operation, "parse SLDPRT PMI MessagePack");
}

#[test]
fn exact_count_refuses_the_i64_boundary_before_a_saturating_cast() {
    let boundary = 9_223_372_036_854_775_808.0_f64;
    assert_eq!(exact_count(boundary), None);
    let below = f64::from_bits(boundary.to_bits() - 1);
    assert_eq!(exact_count(below), Some(9_223_372_036_854_774_784_i64));
}
use crate::records::PmiDimension;
use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::pmi::fixstr;
use crate::test_support::pmi::pmi_semantic_payload;
use crate::test_support::pmi::pmi_semantic_payload_for;
use crate::test_support::pmi::pmi_semantic_payload_for_with_guid;
use crate::test_support::pmi::pmi_semantic_payload_for_with_guid_and_value;
use crate::test_support::pmi::pmi_semantic_payload_record;
use crate::test_support::pmi::pmi_semantic_payload_record_configured;
use crate::test_support::pmi::pmi_semantic_payload_record_with_items;
use crate::test_support::pmi::push_array_header;
use crate::test_support::pmi::PmiPayloadOptions;

fn dimension(subtype: &str, value: f64) -> PmiDimension {
    PmiDimension {
        id: "dimension".into(),
        parent: "block".into(),
        offset: 0,
        guid: "guid".into(),
        cad_text: "D1@Pattern1".into(),
        item_count: std::num::NonZeroU32::MIN,
        subtype: subtype.into(),
        value: cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite test dimension"),
        value_offset: 0,
        precision: 0,
        precision_offset: 0,
        display_text: None,
        basic: false,
        basic_offset: 0,
        inspection: false,
        inspection_offset: 0,
        reference_only: false,
        reference_only_offset: 0,
    }
}

fn named_feature(id: &str, name: &str) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: Some(name.into()),
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    }
}

#[test]
fn empty_subtype_requires_established_count_semantics() {
    let record = dimension("", 2.0);
    assert_eq!(
        dimension_subtype(&record, false),
        PmiDimensionSubtype::Native(String::new())
    );
    assert_eq!(dimension_subtype(&record, true), PmiDimensionSubtype::Count);
    assert_eq!(
        dimension_subtype(&dimension("", 2.5), true),
        PmiDimensionSubtype::Native(String::new())
    );
}

#[test]
fn linear_pattern_primary_and_secondary_counts_are_count_parameters() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternTransform},
        Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let feature = Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#pattern").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::new(PatternTransform::Linear {
                    direction: None,
                    spacing: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
                    count: 2,
                    second: None,
                    omitted_instances: Vec::new(),
                    symmetric: false,
                })
                .unwrap(),
            }),
        ),
        native_ref: None,
    };

    assert!(neutral_parameter_is_count(&feature, "D1", None));
    assert!(neutral_parameter_is_count(&feature, "D2", None));
    assert!(!neutral_parameter_is_count(&feature, "D3", None));
}

#[test]
fn explicit_keywords_dimension_precedes_pmi_value() {
    let owner = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    let feature = named_feature("synthetic:test:id#feature", "Pattern1");
    let mut parameters = vec![DesignParameter {
        product_definition: None,
        id: ParameterId::mint("synthetic:test:id#keywords-parameter").expect("identity grammar"),
        owner: Some(owner),
        ordinal: 0,
        name: "D1".into(),
        expression: "12mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(12.0).unwrap())),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("keywords-dimension".into()),
    }];
    let record = dimension("Linear", 0.034);

    super::apply_to_parameters(&mut parameters, &[feature], &[record], None, None).unwrap();

    let parameter = &parameters[0];
    assert_eq!(parameter.expression, "12mm");
    assert_eq!(parameter.display, None);
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(12.0).unwrap()))
    );
    assert_eq!(parameter.native_ref.as_deref(), Some("keywords-dimension"));
    assert_eq!(
        parameter.pmi.as_ref().map(|pmi| &pmi.subtype),
        Some(&PmiDimensionSubtype::Linear)
    );
    assert_eq!(
        parameter.pmi.as_ref().map(|pmi| pmi.native_ref.as_str()),
        Some("dimension")
    );
}

/// One Keywords parameter `D1` of the feature `Pattern1` with `expression`
/// and `value`, and the record `metres` of `subtype` for it, applied in a
/// document of `unit` and `places`. The parameter and the lengths refined.
fn bare_keywords_length_in(
    expression: &str,
    value: Option<ParameterValue>,
    subtype: &str,
    metres: f64,
    unit: Option<UnitSymbol>,
    places: Option<u32>,
) -> (DesignParameter, Vec<super::RefinedLength>) {
    let owner = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    let feature = named_feature("synthetic:test:id#feature", "Pattern1");
    let mut parameters = vec![DesignParameter {
        product_definition: None,
        id: ParameterId::mint("synthetic:test:id#keywords-parameter").expect("identity grammar"),
        owner: Some(owner),
        ordinal: 0,
        name: "D1".into(),
        expression: expression.into(),
        display: None,
        value,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }];
    let refined = super::apply_to_parameters(
        &mut parameters,
        &[feature],
        &[dimension(subtype, metres)],
        unit,
        places,
    )
    .unwrap();
    (parameters.remove(0), refined)
}

/// [`bare_keywords_length_in`] in a document that states no unit or places.
fn bare_keywords_length(
    expression: &str,
    value: Option<ParameterValue>,
    subtype: &str,
    metres: f64,
) -> DesignParameter {
    bare_keywords_length_in(expression, value, subtype, metres, None, None).0
}

fn real(number: f64) -> ParameterValue {
    ParameterValue::Real(FiniteReal::new(number).unwrap())
}

fn length_of(parameter: &DesignParameter) -> Option<f64> {
    match parameter.value {
        Some(ParameterValue::Length(length)) => Some(length.get()),
        _ => None,
    }
}

#[test]
fn a_bare_inch_display_takes_the_length_of_its_record() {
    // 0.08 in is the displayed 2 mm; the display is no reading in millimetres.
    let parameter = bare_keywords_length("0.08", Some(real(0.08)), "Linear", 0.002);
    assert_eq!(parameter.expression, "2mm");
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(2.0).unwrap()))
    );
    // The same text before it was typed: no value yet.
    let parameter = bare_keywords_length("0.79", None, "Linear", 0.02);
    assert_eq!(parameter.expression, "20mm");
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(20.0).unwrap()))
    );
}

#[test]
fn a_bare_millimetre_display_takes_the_length_of_its_record() {
    let parameter = bare_keywords_length("2", Some(ParameterValue::Integer(2)), "Linear", 0.002);
    assert_eq!(parameter.expression, "2mm");
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(2.0).unwrap()))
    );
}

#[test]
fn a_display_that_no_unit_explains_keeps_keywords() {
    // The record's metres do not give the text in any unit.
    let kept = bare_keywords_length("0.08", Some(real(0.08)), "Linear", 0.01);
    assert_eq!(kept.expression, "0.08");
    assert_eq!(kept.value, Some(real(0.08)));
    // A count and a record of another subtype are no length.
    let kept = bare_keywords_length("0.08", Some(real(0.08)), "", 0.002);
    assert_eq!(kept.expression, "0.08");
    // A display of one place shows 3 mm as 0.1 in.
    let parameter = bare_keywords_length("0.1", Some(real(0.1)), "Linear", 0.003);
    assert_eq!(parameter.expression, "3mm");
    // Zero is no unit's reading.
    let kept = bare_keywords_length("0", Some(real(0.0)), "Linear", 0.0);
    assert_eq!(kept.expression, "0");
    // A typed length stays as Keywords gave it.
    let typed = Some(ParameterValue::Length(Length::new(12.0).unwrap()));
    let kept = bare_keywords_length("0.08", typed.clone(), "Linear", 0.002);
    assert_eq!(kept.value, typed);
}

/// Absolute tolerance of a millimetre value derived by one multiplication.
const EPS_TEST_MM: f64 = 1.0e-9;

#[test]
fn an_ordinate_record_types_a_bare_display() {
    // 0.002 m is 2 mm, which is 0.0787 in and shows as 0.08 in.
    let (parameter, refined) =
        bare_keywords_length_in("0.08", Some(real(0.08)), "Ordinate", 0.002, None, None);
    assert_eq!(parameter.expression, "2mm");
    assert!((length_of(&parameter).unwrap() - 2.0).abs() < EPS_TEST_MM);
    assert_eq!(refined.len(), 1);
}

#[test]
fn a_metre_document_reads_the_display_in_metres() {
    // 0.002 m is the display 0.002 in metres.
    let (parameter, refined) = bare_keywords_length_in(
        "0.002",
        Some(real(0.002)),
        "Linear",
        0.002,
        Some(UnitSymbol::M),
        None,
    );
    assert!((length_of(&parameter).unwrap() - 2.0).abs() < EPS_TEST_MM);
    assert!((refined[0].displayed.mm_per_unit - 1000.0).abs() < EPS_TEST_MM);
}

#[test]
fn a_millimetre_document_does_not_read_a_display_in_inches() {
    // 0.0762 m is 76.2 mm, which is 3 in. A millimetre document shows 76.2.
    let declined = bare_keywords_length_in(
        "3",
        Some(ParameterValue::Integer(3)),
        "Linear",
        0.0762,
        Some(UnitSymbol::Mm),
        None,
    );
    assert_eq!(declined.0.expression, "3");
    assert_eq!(declined.0.value, Some(ParameterValue::Integer(3)));
    assert!(declined.1.is_empty());
    // The branch declining leaves the semantic annotation.
    assert_eq!(
        declined.0.pmi.as_ref().map(|pmi| &pmi.subtype),
        Some(&PmiDimensionSubtype::Linear)
    );
    // With no unit stated, only the inch reading shows 3.
    let (parameter, _) = bare_keywords_length_in(
        "3",
        Some(ParameterValue::Integer(3)),
        "Linear",
        0.0762,
        None,
        None,
    );
    assert!((length_of(&parameter).unwrap() - 76.2).abs() < EPS_TEST_MM);
}

#[test]
fn a_raw_metre_scalar_equal_to_the_record_types_in_a_millimetre_document() {
    // The decoded number times 1 is the record's 0.005 m: the exact form,
    // 5 mm, whatever the document's unit.
    let (parameter, refined) = bare_keywords_length_in(
        "0.005",
        Some(real(0.005)),
        "Linear",
        0.005,
        Some(UnitSymbol::Mm),
        Some(2),
    );
    assert_eq!(parameter.expression, "5mm");
    assert!((length_of(&parameter).unwrap() - 5.0).abs() < EPS_TEST_MM);
    // A configuration's 0.006 is 0.006 m, 6 mm.
    assert!((refined[0].displayed.mm_per_unit - 1000.0).abs() < EPS_TEST_MM);
}

#[test]
fn a_display_that_rounds_the_record_to_another_number_is_declined() {
    // 0.0026 m is 2.6 mm; a display of no places shows 3, not 2.
    let (parameter, refined) = bare_keywords_length_in(
        "2",
        Some(ParameterValue::Integer(2)),
        "Linear",
        0.0026,
        Some(UnitSymbol::Mm),
        None,
    );
    assert_eq!(parameter.expression, "2");
    assert!(refined.is_empty());
}

#[test]
fn a_display_that_no_two_units_share_is_taken_and_exponent_text_is_not() {
    // 0.0254 m is 25.4 mm: "1" reads only in inches, but "1e0" is no display.
    let (parameter, _) = bare_keywords_length_in("1", None, "Linear", 0.0254, None, None);
    assert!((length_of(&parameter).unwrap() - 25.4).abs() < EPS_TEST_MM);
    let (parameter, refined) = bare_keywords_length_in("1e0", None, "Linear", 0.0254, None, None);
    assert_eq!(parameter.value, None);
    assert!(refined.is_empty());
}

/// `D1` as a bare inch display of `0.08` refined to 2 mm by the record
/// 0.002 m, and configurations holding `values` for it, converted.
fn configuration_values_after_refinement(values: &[ParameterValue]) -> Vec<ParameterValue> {
    let (parameter, refined) = bare_keywords_length_in(
        "0.08",
        Some(real(0.08)),
        "Linear",
        0.002,
        Some(UnitSymbol::In),
        Some(2),
    );
    assert_eq!(refined.len(), 1);
    let mut ir = cadmpeg_ir::CadIr::empty();
    for (ordinal, value) in values.iter().enumerate() {
        ir.model.configurations.push(DesignConfiguration {
            id: cadmpeg_ir::features::ConfigurationId::mint(format!(
                "synthetic:test:id#configuration{ordinal}"
            ))
            .expect("identity grammar"),
            ordinal: u32::try_from(ordinal).unwrap(),
            active: false,
            source_index: None,
            name: None,
            material: None,
            properties: BTreeMap::new(),
            bodies: None,
            parameter_values: BTreeMap::from([(parameter.id.clone(), value.clone())]),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        });
    }
    ir.model.parameters.push(parameter.clone());
    crate::history::configuration::convert_refined_configuration_values(&mut ir, &refined);
    ir.model
        .configurations
        .iter()
        .map(|configuration| configuration.parameter_values[&parameter.id].clone())
        .collect()
}

#[test]
fn a_configuration_value_of_a_refined_parameter_converts_from_the_display_unit() {
    let [equal, other, integer] = configuration_values_after_refinement(&[
        ParameterValue::Real(FiniteReal::new(0.08).unwrap()),
        ParameterValue::Real(FiniteReal::new(0.1).unwrap()),
        ParameterValue::Integer(2),
    ])
    .try_into()
    .unwrap();
    let mm = |value: ParameterValue| match value {
        ParameterValue::Length(length) => length.get(),
        other => panic!("not a length: {other:?}"),
    };
    // The displayed 0.08 in is the refined 2 mm, not 0.08 * 25.4 = 2.032.
    assert!((mm(equal) - 2.0).abs() < EPS_TEST_MM);
    // 0.1 in is 0.1 * 25.4 = 2.54 mm.
    assert!((mm(other) - 2.54).abs() < EPS_TEST_MM);
    // 2 in is 50.8 mm.
    assert!((mm(integer) - 50.8).abs() < EPS_TEST_MM);
}

#[test]
fn conflicting_pmi_dimensions_do_not_bind_a_parameter() {
    let feature = named_feature("synthetic:test:id#feature", "Pattern1");
    let first = dimension("Linear", 0.034);
    let mut second = dimension("Linear", 0.035);
    second.id = "dimension-2".into();
    second.guid = "guid-2".into();
    let mut parameters = Vec::new();

    super::apply_to_parameters(&mut parameters, &[feature], &[first, second], None, None).unwrap();

    assert!(parameters.is_empty());
}

#[test]
fn equivalent_pmi_dimensions_bind_once_to_lowest_record_id() {
    let feature = named_feature("synthetic:test:id#feature", "Pattern1");
    let canonical = dimension("Linear", 0.034);
    let mut alias = canonical.clone();
    alias.id = "dimension-2".into();
    alias.guid = "guid-2".into();
    let mut parameters = Vec::new();

    super::apply_to_parameters(&mut parameters, &[feature], &[canonical, alias], None, None)
        .unwrap();

    let [parameter] = parameters.as_slice() else {
        panic!("one PMI-backed parameter");
    };
    assert_eq!(parameter.expression, "34mm");
    assert_eq!(
        parameter.pmi.as_ref().map(|pmi| pmi.native_ref.as_str()),
        Some("dimension")
    );
}

#[test]
fn conflicting_pmi_metadata_do_not_enrich_history() {
    use crate::records::FeatureHistory;

    let mut history = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::records::Feature {
            id: "feature".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: None,
            ordinal: 0,
            name: "Pattern1".into(),
            kind: "StoredGeometry".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }];
    let first = dimension("Linear", 0.034);
    let mut second = first.clone();
    second.id = "dimension-2".into();
    second.guid = "guid-2".into();
    second.basic = true;

    super::enrich_history_parameters(&mut history, &[first, second]);

    assert!(history[0].features[0].parameters.is_empty());
}

fn push_map_header(bytes: &mut Vec<u8>, len: usize) {
    bytes.push(0x80 | u8::try_from(len).unwrap());
}

fn dim_sem_item(bytes: &mut Vec<u8>, subtype: &str, value: f64) {
    bytes.push(0x87);
    fixstr(bytes, "class");
    fixstr(bytes, "DimSemData");
    fixstr(bytes, "dimSubType");
    fixstr(bytes, subtype);
    fixstr(bytes, "isBasic");
    bytes.push(0xc3);
    fixstr(bytes, "isInspection");
    bytes.push(0xc2);
    fixstr(bytes, "isReferenceOnly");
    bytes.push(0xc3);
    fixstr(bytes, "valPrecision");
    bytes.push(3);
    fixstr(bytes, "value");
    bytes.push(0xcb);
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn fixture_payload(
    cad_text: &str,
    guid: &str,
    items: &[(&str, f64)],
    display_text: &str,
    reorder_and_extra_key: bool,
    key_like_in_dim_text: bool,
    truncate_after_dim_items_key: bool,
) -> Vec<u8> {
    assert_eq!(guid.len(), 36);
    let mut payload = b"unqlite".to_vec();
    payload.extend_from_slice(&[0; 57]);
    payload.extend_from_slice(guid.as_bytes());
    let outer_len = if reorder_and_extra_key { 8 } else { 7 };
    push_map_header(&mut payload, outer_len);
    if reorder_and_extra_key {
        fixstr(&mut payload, "cadText");
        fixstr(&mut payload, cad_text);
        fixstr(&mut payload, "extraKey");
        fixstr(&mut payload, "ignored");
        fixstr(&mut payload, "annoType");
        payload.push(1);
    } else {
        fixstr(&mut payload, "annoType");
        payload.push(1);
        fixstr(&mut payload, "cadText");
        fixstr(&mut payload, cad_text);
    }
    fixstr(&mut payload, "dimItems");
    if truncate_after_dim_items_key {
        push_array_header(&mut payload, 1);
        return payload;
    }
    push_array_header(&mut payload, items.len());
    for (subtype, value) in items {
        dim_sem_item(&mut payload, subtype, *value);
    }
    fixstr(&mut payload, "dimText");
    if key_like_in_dim_text {
        fixstr(&mut payload, "cadText");
    } else {
        fixstr(&mut payload, display_text);
    }
    fixstr(&mut payload, "dimType");
    payload.push(0);
    fixstr(&mut payload, "iDString");
    fixstr(&mut payload, "native-id");
    fixstr(&mut payload, "reserved");
    payload.push(0xc0);
    payload
}

#[test]
fn parses_array16_dim_items() {
    let items = vec![("Linear", 0.025); 16];
    let payload = fixture_payload(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &items,
        "25.000 mm",
        false,
        false,
        false,
    );
    let mut losses = Vec::new();
    let records = parse_payload(&payload, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].item_count.get(), 16);
    assert_eq!(records[0].value.get(), 0.025);
}

#[test]
fn identical_dimension_fields_can_have_different_patch_positions() {
    let payload = fixture_payload(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &[("Linear", 0.025)],
        "25.000 mm",
        false,
        false,
        false,
    );
    let mut losses = Vec::new();
    let original = parse_payload(&payload, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(original.len(), 1);
    let record = &original[0];
    let basic = patch_slots::field_offset(&payload, record.offset, "isBasic").unwrap();
    let inspection = patch_slots::field_offset(&payload, record.offset, "isInspection").unwrap();
    let mut reordered = payload.clone();
    let start = basic - (1 + "isBasic".len());
    reordered[start..=inspection].rotate_left(1 + "isBasic".len() + 1);
    let other = parse_payload(&reordered, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    let mut left = cadmpeg_core::serde_erase::json::to_value(&original[0]).unwrap();
    let mut right = cadmpeg_core::serde_erase::json::to_value(&other[0]).unwrap();
    for value in [&mut left, &mut right] {
        for field in [
            "value_offset",
            "precision_offset",
            "basic_offset",
            "inspection_offset",
            "reference_only_offset",
        ] {
            value.as_object_mut().unwrap().remove(field);
        }
    }
    assert_eq!(left, right);
    assert_ne!(
        basic,
        patch_slots::field_offset(&reordered, record.offset, "isBasic").unwrap()
    );
    assert_ne!(
        inspection,
        patch_slots::field_offset(&reordered, record.offset, "isInspection").unwrap()
    );
}

#[test]
fn parses_reordered_map_with_extra_key() {
    let payload = fixture_payload(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &[("Linear", 0.025)],
        "25.000 mm",
        true,
        false,
        false,
    );
    let mut losses = Vec::new();
    let records = parse_payload(&payload, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].cad_text, "D1@Sketch1");
}

#[test]
fn key_like_string_inside_value_does_not_steal_field_spans() {
    let payload = fixture_payload(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &[("Linear", 0.025)],
        "25.000 mm",
        false,
        true,
        false,
    );
    let mut losses = Vec::new();
    let records = parse_payload(&payload, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    let [record] = records.as_slice() else {
        panic!("one record");
    };
    assert_eq!(record.display_text(), Some("cadText"));
    assert_eq!(record.cad_text, "D1@Sketch1");
    let text_off =
        usize::try_from(record.display_text_offset().expect("display text offset")).unwrap();
    assert_eq!(&payload[text_off..text_off + 7], b"cadText");
}

#[test]
fn invalid_utf8_auxiliary_string_does_not_drop_dimension() {
    let mut payload = fixture_payload(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &[("Linear", 0.025)],
        "25.000 mm",
        false,
        false,
        false,
    );
    let start = payload
        .windows(b"native-id".len())
        .position(|window| window == b"native-id")
        .expect("auxiliary iDString value");
    payload[start..start + b"native-id".len()]
        .copy_from_slice(&[0xff, 0xfe, 0xfd, b'n', b'a', b't', b'i', b'v', b'e']);

    let mut losses = Vec::new();
    let records = parse_payload(&payload, &mut losses);

    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].cad_text, "D1@Sketch1");
    assert_eq!(records[0].value.get(), 0.025);
}

#[test]
fn malformed_pmi_map_emits_attributed_loss() {
    let payload = fixture_payload(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &[("Linear", 0.025)],
        "25.000 mm",
        false,
        false,
        true,
    );
    let mut losses = Vec::new();
    let records = parse_payload(&payload, &mut losses);
    assert!(records.is_empty());
    assert_eq!(losses.len(), 1);
    assert!(
        losses[0]
            .message
            .contains("01234567-89ab-cdef-0123-456789abcdef"),
        "{}",
        losses[0].message
    );
    assert!(
        losses[0].message.contains("failed to parse"),
        "{}",
        losses[0].message
    );
}

#[test]
fn malformed_uppercase_pmi_guid_keeps_normalized_loss_text() {
    let payload = fixture_payload(
        "D1@Sketch1",
        "ABCDEF01-89AB-CDEF-0123-456789ABCDEF",
        &[("Linear", 0.025)],
        "25.000 mm",
        false,
        false,
        true,
    );
    let mut losses = Vec::new();
    let records = parse_payload(&payload, &mut losses);
    assert!(records.is_empty());
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("abcdef01-89ab-cdef-0123-456789abcdef"));
}

#[test]
fn patch_payload_offsets_round_trip_through_reparse() {
    let payload = fixture_payload(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &[("Linear", 0.025)],
        "25.000 mm",
        false,
        false,
        false,
    );
    let mut losses = Vec::new();
    let records = parse_payload(&payload, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    let [record] = records.as_slice() else {
        panic!("one record");
    };
    let mut patched = payload.clone();
    let start = usize::try_from(record.value_offset).unwrap();
    let edited = 0.05_f64;
    patched[start..start + 8].copy_from_slice(&edited.to_be_bytes());
    patched[usize::try_from(record.precision_offset).unwrap()] = 4;
    patched[usize::try_from(record.basic_offset).unwrap()] = 0xc2;
    patched[usize::try_from(record.inspection_offset).unwrap()] = 0xc3;
    patched[usize::try_from(record.reference_only_offset).unwrap()] = 0xc2;
    let text_off = usize::try_from(record.display_text_offset().expect("display text")).unwrap();
    patched[text_off..text_off + 9].copy_from_slice(b"50.000 mm");
    let mut again_losses = Vec::new();
    let again = parse_payload(&patched, &mut again_losses);
    assert!(again_losses.is_empty(), "{again_losses:?}");
    let [edited_record] = again.as_slice() else {
        panic!("one edited record");
    };
    assert_eq!(edited_record.value.get(), 0.05);
    assert_eq!(edited_record.precision, 4);
    assert!(!edited_record.basic);
    assert!(edited_record.inspection);
    assert!(!edited_record.reference_only);
    assert_eq!(edited_record.display_text(), Some("50.000 mm"));
    assert_eq!(edited_record.value_offset, record.value_offset);
    assert_eq!(edited_record.precision_offset, record.precision_offset);
}

#[test]
fn decode_extracts_pmi_semantic_dimension() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload(),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let native = sldprt_native(decoded.ir());
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one PMI dimension");
    };
    assert_eq!(dimension.guid, "01234567-89ab-cdef-0123-456789abcdef");
    assert_eq!(dimension.cad_text, "D1@Sketch1");
    assert_eq!(dimension.subtype, "Linear");
    assert_eq!(dimension.value.get(), 0.025);
    assert_eq!(dimension.precision, 3);
    assert_eq!(dimension.display_text(), Some("25.000 mm"));
    assert!(dimension.basic);
    assert!(!dimension.inspection);
    assert!(dimension.reference_only);
    assert_eq!(
        decoded.source_fidelity().annotations.provenance[&dimension.id].offset,
        dimension.offset
    );
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("PMI-backed parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::parameters::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(25.0).unwrap()
        ))
    );
    let semantic = parameter.pmi.as_ref().expect("PMI semantics");
    assert_eq!(
        semantic.subtype,
        cadmpeg_ir::features::parameters::PmiDimensionSubtype::Linear
    );
    assert_eq!(semantic.native_ref, dimension.id);
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap();

    {
        let mut ir = decoded.ir_mut();
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "D1")
            .expect("editable PMI-backed parameter");
        parameter.expression = "50mm".into();
        parameter.value = Some(cadmpeg_ir::features::parameters::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
        ));
        let semantic = parameter.pmi.as_mut().expect("editable PMI semantics");
        semantic.precision = 4;
        semantic.display_text = Some("50.000 mm".into());
        semantic.basic = false;
        semantic.inspection = true;
        semantic.reference_only = false;
    }

    let mut encoded = Vec::new();
    crate::test_support::write_history_over_stale_solid(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one regenerated PMI dimension");
    };
    assert_eq!(dimension.value.get(), 0.05);
    assert_eq!(dimension.precision, 4);
    assert_eq!(dimension.display_text(), Some("50.000 mm"));
    assert!(!dimension.basic);
    assert!(dimension.inspection);
    assert!(!dimension.reference_only);
}

#[test]
fn decode_extracts_array16_and_reordered_pmi_maps() {
    let items = vec![("Linear", 0.025); 16];
    let array16 = pmi_semantic_payload_record_with_items(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &items,
        "25.000 mm",
    );
    let reordered = pmi_semantic_payload_record_configured(
        "D1@Sketch1",
        "fedcba98-7654-3210-fedc-ba9876543210",
        &[("Linear", 0.030)],
        "30.000 mm",
        PmiPayloadOptions {
            reorder_and_extra_key: true,
            ..PmiPayloadOptions::default()
        },
    );
    for (payload, guid, value, item_count) in [
        (
            array16,
            "01234567-89ab-cdef-0123-456789abcdef",
            0.025,
            16_u32,
        ),
        (reordered, "fedcba98-7654-3210-fedc-ba9876543210", 0.030, 1),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(decoded.ir());
        let dimension = native
            .pmi_dimensions
            .iter()
            .find(|record| record.guid == guid)
            .expect("PMI dimension");
        assert_eq!(dimension.value.get(), value);
        assert_eq!(dimension.item_count.get(), item_count);
        assert!(decoded.report().losses.iter().all(|loss| {
            !loss.message.contains("semantic-record-malformed")
                && !loss.message.contains("failed to parse MessagePack map")
        }));
    }
}

#[test]
fn decode_reports_malformed_pmi_semantic_map() {
    let payload = pmi_semantic_payload_record_configured(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        &[("Linear", 0.025)],
        "25.000 mm",
        PmiPayloadOptions {
            truncate_after_dim_items_key: true,
            ..PmiPayloadOptions::default()
        },
    );
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(sldprt_native(decoded.ir()).pmi_dimensions.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("01234567-89ab-cdef-0123-456789abcdef")
            && loss.message.contains("failed to parse MessagePack map")
    }));
}

#[test]
fn multi_item_pmi_dimension_is_not_bound() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_record_with_items(
            "D1@Sketch1",
            "01234567-89ab-cdef-0123-456789abcdef",
            &[("Linear", 0.025), ("Linear", 0.025)],
            "25.000 mm",
        ),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one native PMI dimension");
    };
    assert_eq!(dimension.item_count.get(), 2);
    assert!(!decoded
        .ir()
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "D1" && parameter.pmi.is_some()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "1 semantic dimension record(s) are not bound to parameters; 0 parameter dimension(s) retain native subtypes."
    }));
}

#[test]
fn decode_reports_unbound_pmi_semantic_dimension() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_for("D1@MissingFeature"),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "1 semantic dimension record(s) are not bound to parameters; 0 parameter dimension(s) retain native subtypes."
    }));
}

#[test]
fn duplicate_pmi_records_share_one_parameter_and_round_trip_edits() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    for (time_date, guid) in [
        (0x0021_0049, "01234567-89ab-cdef-0123-456789abcdef"),
        (0x0021_004a, "fedcba98-7654-3210-fedc-ba9876543210"),
    ] {
        source.extend(make_block(
            time_date,
            "Contents/PMISemanticDataDB",
            &pmi_semantic_payload_for_with_guid("D1@Sketch1", guid),
        ));
    }

    let pmi_dates: Vec<_> = crate::test_support::container::scan(&source)
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some("Contents/PMISemanticDataDB"))
        .map(|block| block.type_id)
        .collect();
    assert_eq!(pmi_dates, [0x0021_0049, 0x0021_004a]);

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                &(source),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert_eq!(sldprt_native(decoded.ir()).pmi_dimensions.len(), 2);
    assert_eq!(decoded.ir().model.parameters.len(), 1);
    assert!(decoded.report().losses.iter().all(|loss| !loss
        .message
        .contains("semantic dimension record(s) are not bound")));

    {
        let mut ir = decoded.ir_mut();
        let parameter = &mut ir.model.parameters[0];
        parameter.expression = "50mm".into();
        parameter.value = Some(cadmpeg_ir::features::parameters::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
        ));
    }

    let mut encoded = Vec::new();
    crate::test_support::write_history_over_stale_solid(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    assert_eq!(native.pmi_dimensions.len(), 2);
    assert!(native
        .pmi_dimensions
        .iter()
        .all(|dimension| dimension.value.get() == 0.05));
}

#[test]
fn semantically_distinct_pmi_records_remain_unbound() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    for (guid, value) in [
        ("01234567-89ab-cdef-0123-456789abcdef", 0.025),
        ("fedcba98-7654-3210-fedc-ba9876543210", 0.030),
    ] {
        source.extend(make_block(
            0x49,
            "Contents/PMISemanticDataDB",
            &pmi_semantic_payload_for_with_guid_and_value("D1@Sketch1", guid, value),
        ));
    }

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "2 semantic dimension record(s) are not bound to parameters; 0 parameter dimension(s) retain native subtypes."
    }));
}

#[test]
fn ordinate_pmi_dimensions_round_trip_typed_values() {
    use cadmpeg_ir::features::parameters::{ParameterValue, PmiDimensionSubtype};
    use cadmpeg_ir::scalar::Length;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let payload = pmi_semantic_payload_record(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        "Ordinate",
        0.025,
        "<DIM>",
    );
    source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let ordinate = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "D1")
            .expect("ordinate parameter");
        assert_eq!(
            ordinate.value,
            Some(ParameterValue::Length(Length::new(25.0).unwrap()))
        );
        assert_eq!(
            ordinate.pmi.as_ref().map(|pmi| &pmi.subtype),
            Some(&PmiDimensionSubtype::Ordinate)
        );
        ordinate.expression = "50mm".into();
        ordinate.value = Some(ParameterValue::Length(Length::new(50.0).unwrap()));
    }

    let mut encoded = Vec::new();
    crate::test_support::write_history_over_stale_solid(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    assert_eq!(
        native
            .pmi_dimensions
            .iter()
            .find(|dimension| dimension.cad_text == "D1@Sketch1")
            .map(|dimension| dimension.value.get()),
        Some(0.05)
    );
}

#[test]
fn decode_uses_pmi_dimension_to_project_sparse_extrusion() {
    use cadmpeg_ir::features::extents::{ExtrudeExtent, ExtrudeSide, LinearTermination};
    use cadmpeg_ir::features::operation_settings::BooleanOp;
    use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="Localized" id="42"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moExtrusion_c", "Boss", 42)]),
    ));
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_for("D1@Boss"),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(_)),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }) if actual_length.get() == 25.0
    ));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&decoded.ir().model.features[0].id))
        .expect("PMI extrusion parameter");
    assert_eq!(parameter.name, "D1");
    assert_eq!(parameter.expression, "25mm");
    assert!(parameter.pmi.is_some());
}

#[test]
fn u16_precision_survives_plain_patch_round_trip() {
    let mut payload = pmi_semantic_payload();
    let record = parse_payload(&payload, &mut Vec::new()).remove(0);
    let offset = usize::try_from(record.precision_offset).unwrap();
    payload.splice(offset..=offset, [0xcd, 0x00, 0x03]);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let record = &native.pmi_dimensions[0];
    assert_eq!(record.precision, 3);
    assert!(decoded
        .ir()
        .model
        .parameters
        .iter()
        .any(|p| p.pmi.is_some()));
    let original = payload.clone();
    patch_payload(decoded.ir(), &record.parent, &mut payload, &|_| true).unwrap();
    assert_eq!(&payload[offset..offset + 3], &[0xcd, 0x00, 0x03]);
    assert_eq!(payload, original);
}

#[test]
fn pmi_parameter_ordinal_overflow_is_refused_not_saturated() {
    let owner = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    let feature = named_feature("synthetic:test:id#feature", "Pattern1");
    let mut parameters = vec![DesignParameter {
        product_definition: None,
        id: ParameterId::mint("synthetic:test:id#other-parameter").expect("identity grammar"),
        owner: Some(owner),
        ordinal: u32::MAX,
        name: "D9".into(),
        expression: "12mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(12.0).unwrap())),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("other-dimension".into()),
    }];
    let error = super::apply_to_parameters(
        &mut parameters,
        &[feature],
        &[dimension("Linear", 0.034)],
        None,
        None,
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert_eq!(parameters.len(), 1);
}

#[test]
fn a_count_beyond_float_precision_keeps_the_stored_value_and_is_named() {
    let payload = crate::test_support::pmi::pmi_semantic_payload_record(
        "D1@Sketch1",
        "01234567-89ab-cdef-0123-456789abcdef",
        "",
        2.0,
        "2",
    );
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parent = sldprt_native(decoded.ir()).pmi_dimensions[0].parent.clone();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let parameter = &mut ir.model.parameters[0];
        parameter.value = Some(ParameterValue::Integer((1_i64 << 53) + 1));
        parameter.pmi.as_mut().unwrap().subtype = PmiDimensionSubtype::Count;
    }
    let mut patched = payload.clone();
    let kept = patch_payload(decoded.ir(), &parent, &mut patched, &|_| true).unwrap();
    assert_eq!(patched, payload);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(crate::test_support::is_edit_dropped(&kept[0].code));
    assert!(
        kept[0].message.contains("beyond exact float range"),
        "{}",
        kept[0].message
    );
}

#[test]
fn pmi_alias_comparison_counts_long_distinct_semantic_strings() {
    let mut first = dimension("linear", 1.0);
    first.id = "bound".into();
    first.cad_text = "x".repeat(1024);
    let mut second = first.clone();
    second.id = "unbound".into();
    second.cad_text.push('y');
    let records = [first, second];
    let bound = std::collections::HashSet::from(["bound"]);
    assert_eq!(super::unbound_dimension_count(&records, &bound), 1);
}

#[test]
fn pmi_nested_messagepack_depth_cap_rejects_the_value() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut within = vec![0x91; 16];
    within.push(0xc0);
    let work = cadmpeg_core::decode::WorkBudget::new(usize::MAX);
    let parse = super::Parse {
        shape: super::Shape::Record,
        work: &work,
    };
    assert!(super::parse_value(&ctx, &within, &mut 0, 0, parse)
        .unwrap()
        .is_some());
    let mut beyond = vec![0x91; 18];
    beyond.push(0xc0);
    assert!(super::parse_value(&ctx, &beyond, &mut 0, 0, parse)
        .unwrap()
        .is_none());
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn pmi_patch_admits_records_at_the_maximum_field_depth() {
    // NativeRecord admits fields nested 256 deep; loading such a record stays within the depth
    // limit, so neither the load nor the patch refuses on resources.
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut nested = serde_json::Value::Null;
    for _ in 0..256 {
        nested = serde_json::Value::Array(vec![nested]);
    }
    let mut fields = serde_json::Map::new();
    fields.insert("extra".into(), nested);
    let record = cadmpeg_ir::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new("sldprt:pmi:dimension#0").unwrap(),
        fields,
    )
    .unwrap();
    let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("sldprt"));
    namespace.arenas_mut().insert(
        cadmpeg_core::nonblank_literal!("pmi_dimensions"),
        vec![record],
    );
    // The fixture is not a complete dimension record; only the absence of a resource refusal
    // is asserted.
    if let Err(error) = crate::native::SldprtNative::load(
        namespace,
        cadmpeg_ir::units::Tolerances::default().linear,
    ) {
        assert!(!matches!(
            cadmpeg_core::CodecError::from(error),
            cadmpeg_core::CodecError::ResourceLimit(_)
        ));
    }
    assert!(!matches!(
        patch_payload(&ir, "block", &mut [], &|_| true),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

/// The default record with one more outer-map entry `key: value` after its
/// fields, so every field keeps its offset.
fn record_with_trailing_entry(key: &str, value: &[u8]) -> Vec<u8> {
    const GUID: &str = "01234567-89ab-cdef-0123-456789abcdef";
    let mut payload = pmi_semantic_payload();
    let header = payload
        .windows(GUID.len())
        .position(|window| window == GUID.as_bytes())
        .unwrap()
        + GUID.len();
    assert_eq!(payload[header], 0x87);
    payload[header] = 0x88;
    fixstr(&mut payload, key);
    payload.extend_from_slice(value);
    payload
}

#[test]
fn pmi_skipped_values_are_validated_but_not_kept() {
    const BULK_VALUES: usize = 1 << 20;
    let mut bulk = vec![0xdd];
    bulk.extend_from_slice(&u32::try_from(BULK_VALUES).unwrap().to_be_bytes());
    bulk.extend_from_slice(&vec![0; BULK_VALUES]);
    let original = parse_payload(&pmi_semantic_payload(), &mut Vec::new());
    let payload = record_with_trailing_entry("bulk", &bulk);
    let mut losses = Vec::new();
    let records = parse_payload(&payload, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(records, original);

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let work = cadmpeg_core::decode::WorkBudget::new(usize::MAX);
    let parse = super::Parse {
        shape: super::Shape::Record,
        work: &work,
    };
    let mut cursor = usize::try_from(records[0].offset).unwrap();
    let outer = super::parse_value(&ctx, &payload, &mut cursor, 0, parse)
        .unwrap()
        .unwrap();
    let super::ValueKind::Map(fields) = outer.kind else {
        panic!("the record is a map");
    };
    assert_eq!(
        fields.keys().copied().collect::<Vec<_>>(),
        ["cadText", "dimItems", "dimText"]
    );
    assert_eq!(cursor, payload.len());
    assert!(work.consumed() > BULK_VALUES);

    let mut losses = Vec::new();
    let integer_key = record_with_trailing_entry("bulk", &[0x91, 0x81, 0x01, 0x02]);
    assert!(parse_payload(&integer_key, &mut losses).is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one malformed-record loss: {losses:?}");
    };
    assert_eq!(
        loss.code,
        crate::loss::SldprtLossCode::PmiSemanticRecordMalformed.kind()
    );
}

/// `count` candidate maps whose arrays run to the end of the payload. Each
/// later header sits inside a `str8` value of every earlier parse: the `0xd9`
/// before a GUID turns its first character, `0`, into a 48-byte length.
fn overlapping_candidates(count: usize, filler: usize) -> (Vec<u8>, usize) {
    let mut payload = Vec::new();
    let mut length_slots = Vec::new();
    let stride = 36 + 8 + filler + 1;
    for index in 0..count {
        let guid = format!("{index:08x}-0000-4000-8000-{index:012x}");
        payload.extend_from_slice(guid.as_bytes());
        payload.extend_from_slice(&[0x81, 0xa1, b'x', 0xdd]);
        length_slots.push(payload.len());
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&vec![0; filler]);
        payload.push(0xd9);
    }
    for slot in length_slots {
        let remaining = u32::try_from(payload.len() - slot - 4).unwrap();
        payload[slot..slot + 4].copy_from_slice(&remaining.to_be_bytes());
    }
    (payload, stride)
}

#[test]
fn pmi_parse_cap_skips_the_remaining_candidates_and_names_the_bound() {
    const CANDIDATES: usize = 64;
    let record = pmi_semantic_payload();
    let original = parse_payload(&record, &mut Vec::new());

    let (moderate, _) = overlapping_candidates(8, 2048);
    let mut payload = record.clone();
    payload.extend_from_slice(&moderate);
    let mut losses = Vec::new();
    assert_eq!(parse_payload(&payload, &mut losses), original);
    assert!(losses.is_empty(), "{losses:?}");

    let (chain, stride) = overlapping_candidates(CANDIDATES, 2048);
    let mut payload = record.clone();
    payload.extend_from_slice(&chain);
    let mut losses = Vec::new();
    assert_eq!(parse_payload(&payload, &mut losses), original);
    let [loss] = losses.as_slice() else {
        panic!("one parse-cap loss: {losses:?}");
    };
    assert_eq!(
        loss.code,
        crate::loss::SldprtLossCode::PmiSemanticParseCapped.kind()
    );
    let prefix = "PMISemanticDataDB section sldprt:block#pmi-payload: ";
    let suffix = format!(
        " were not parsed because the section reached the local work cap \
         PMI_PARSE_WORK_PER_PAYLOAD_BYTE (16) times its {}-byte payload.",
        payload.len()
    );
    let counts = loss
        .message
        .strip_prefix(prefix)
        .and_then(|message| message.strip_suffix(&suffix))
        .unwrap_or_else(|| panic!("unexpected message: {}", loss.message));
    let (skipped, offset) = counts.split_once(" candidate map(s) from offset ").unwrap();
    let skipped = skipped.parse::<usize>().unwrap();
    let offset = offset.parse::<usize>().unwrap();
    assert!(0 < skipped && skipped < CANDIDATES, "{skipped}");
    assert_eq!(
        offset,
        record.len() + (CANDIDATES - skipped) * stride + 36,
        "the loss starts at the candidate the budget ran out in"
    );
}

#[test]
fn duplicate_member_identity_replays_and_names_a_semantic_edit_over_the_stored_solid() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x0021_0042,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    for guid in [
        "01234567-89ab-cdef-0123-456789abcdef",
        "fedcba98-7654-3210-fedc-ba9876543210",
    ] {
        source.extend(make_block(
            0x0021_0049,
            "Contents/PMISemanticDataDB",
            &pmi_semantic_payload_for_with_guid("D1@Sketch1", guid),
        ));
    }
    let source = crate::test_support::container::native_envelope_fixture(&source);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert_eq!(sldprt_native(decoded.ir()).pmi_dimensions.len(), 2);
    assert_eq!(decoded.ir().model.parameters.len(), 1);
    assert!(decoded
        .source_fidelity()
        .retained_record(crate::SOURCE_IMAGE_ID)
        .unwrap()
        .data()
        .is_some());
    let mut unchanged = Vec::new();
    let path = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut unchanged,
    )
    .unwrap();
    assert!(matches!(
        path,
        cadmpeg_ir::report::export::WritePath::VerbatimReplay { .. }
    ));
    assert_eq!(unchanged, source);
    {
        let mut ir = decoded.ir_mut();
        let parameter = &mut ir.model.parameters[0];
        parameter.expression = "50mm".into();
        parameter.value = Some(cadmpeg_ir::features::parameters::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
        ));
    }
    let report = cadmpeg_ir::codec::write::Encoder::plan(
        &SldprtCodec,
        cadmpeg_ir::codec::write::EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
        cadmpeg_ir::codec::write::target::TargetRequest::Inherit,
    )
    .unwrap()
    .write_to(&mut Vec::new())
    .unwrap();
    let parameter = decoded.ir().model.parameters[0].id.as_str();
    assert!(
        report.losses.iter().any(|loss| loss.code
            == crate::loss::SldprtLossCode::WriterSolidStaleAfterEdit.kind()
            && loss.message.contains(parameter)),
        "{:#?}",
        report.losses
    );
}
