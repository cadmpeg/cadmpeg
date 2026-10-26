// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

mod reference_driving;

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::SldprtCodec;

use cadmpeg_ir::{
    features::{
        DesignParameter, Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
        ParameterValue,
    },
    scalar::Length,
};

use super::{
    dimension_subtype, exact_count, neutral_parameter_is_count, patch_payload, patch_slots,
    DimensionSubtype,
};

fn parse_source(
    payload: &[u8],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> (Vec<PmiDimension>, cadmpeg_ir::SourceFidelity) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("PMI source");
    let parsed =
        super::parse_payload(&ctx, root, losses).expect("PMI test input fits service policy");
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    super::layout::install(&ctx, parsed.pointers, &mut fidelity).expect("PMI source layouts");
    (parsed.records, fidelity)
}

fn parse_payload(
    payload: &[u8],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Vec<PmiDimension> {
    parse_source(payload, losses).0
}

fn pointer(payload: &[u8], record: &PmiDimension, field: &str) -> Option<u64> {
    parse_source(payload, &mut Vec::new())
        .1
        .record_layout()
        .get(&record.id)?
        .pointers()
        .get(field)
        .copied()
}

fn write_with_regenerated_pmi(
    ir: &cadmpeg_ir::CadIr,
    fidelity: &cadmpeg_ir::SourceFidelity,
    output: &mut Vec<u8>,
) {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)
        .expect("semantic PMI is regenerated from the document");
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(
            |loss| loss.code.local_code() == "writer.native-records-omitted"
                && loss.message.contains("native PMI dimension")
        ));
    plan.write_to(output)
        .expect("write solved part and neutral scalars");
}

fn assert_written_scalar_with_pmi(decoded: &cadmpeg_ir::codec::DecodeResult) {
    assert!(!sldprt_native(decoded.ir()).pmi_dimensions.is_empty());
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("neutral scalar survives PMI omission");
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(50.0).unwrap()))
    );
    let id = parameter.pmi.as_ref().expect("PMI link survives synthesis");
    assert!(decoded
        .ir()
        .model
        .pmi
        .iter()
        .any(|annotation| &annotation.id == id));
}

fn pmi_limit_refusal(
    policy: cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::decode::ResourceLimit {
    let payload = pmi_semantic_payload();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("PMI test input fits the root limit");
    let mut losses = Vec::new();
    let error = super::parse_payload(&ctx, root, &mut losses)
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
        guid: "guid".into(),
        cad_text: "D1@Pattern1".into(),
        item_count: std::num::NonZeroU32::MIN,
        subtype: subtype.into(),
        value: cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite test dimension"),
        precision: 0,
        display_text: None,
        basic: false,
        inspection: false,
        reference_only: false,
    }
}

fn named_feature(id: &str, name: &str) -> Feature {
    Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: Some(name.into()),
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
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
        DimensionSubtype::Native(String::new())
    );
    assert_eq!(dimension_subtype(&record, true), DimensionSubtype::Count);
    assert_eq!(
        dimension_subtype(&dimension("", 2.5), true),
        DimensionSubtype::Native(String::new())
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
        id: FeatureId::mint("synthetic:test:id#pattern").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::new(PatternTransform::Linear {
                    direction: None,
                    distance: cadmpeg_ir::scalar::PositiveLength::new(10.0)
                        .unwrap()
                        .into(),
                    count: cadmpeg_ir::Dimension::Literal(2),
                    second: None,
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
        driving: true,
        id: ParameterId::mint("synthetic:test:id#keywords-parameter").expect("identity grammar"),
        owner: Some(owner),
        ordinal: 0,
        name: "D1".into(),
        expression: "12mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(12.0).unwrap())),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("keywords-dimension".into()),
    }];
    let record = dimension("Linear", 0.034);

    let projected = super::apply_to_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &mut parameters,
        &[feature],
        std::slice::from_ref(&record),
    )
    .unwrap();

    let parameter = &parameters[0];
    assert_eq!(parameter.expression.literal_text().as_deref(), Some("12mm"));
    assert_eq!(parameter.display, None);
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(12.0).unwrap()))
    );
    assert_eq!(parameter.native_ref.as_deref(), Some("keywords-dimension"));
    assert_eq!(
        parameter.pmi.as_ref(),
        Some(&super::annotation_id(&record).unwrap())
    );
    assert!(projected.annotations.iter().any(|annotation| matches!(&annotation.definition, cadmpeg_ir::pmi::PmiDefinition::Dimension(dimension) if dimension.kind() == &cadmpeg_ir::pmi::DimensionKind::Size && dimension.nominal().and_then(cadmpeg_ir::Dimension::parameter) == Some(&parameter.id))));
}

#[test]
fn conflicting_pmi_dimensions_do_not_bind_a_parameter() {
    let feature = named_feature("synthetic:test:id#feature", "Pattern1");
    let first = dimension("Linear", 0.034);
    let mut second = dimension("Linear", 0.035);
    second.id = "dimension-2".into();
    second.guid = "guid-2".into();
    let mut parameters = Vec::new();

    super::apply_to_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &mut parameters,
        &[feature],
        &[first, second],
    )
    .unwrap();

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

    let canonical_id = super::annotation_id(&canonical).unwrap();
    super::apply_to_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &mut parameters,
        &[feature],
        &[canonical, alias],
    )
    .unwrap();

    let [parameter] = parameters.as_slice() else {
        panic!("one PMI-backed parameter");
    };
    assert_eq!(parameter.expression.literal_text().as_deref(), Some("34mm"));
    assert_eq!(parameter.pmi.as_ref(), Some(&canonical_id));
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
    let [loss] = losses.as_slice() else {
        panic!("one additional-item loss: {losses:?}");
    };
    assert_eq!(
        loss.code,
        crate::loss::SldprtLossCode::PmiSemanticRecordUntyped.kind()
    );
    assert!(loss
        .message
        .contains("contains 15 additional semantic dimension item(s) without typed payloads"));
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
    let basic = patch_slots::field_offset(
        &payload,
        pointer(&payload, &record, "offset").expect("source offset"),
        "isBasic",
    )
    .unwrap();
    let inspection = patch_slots::field_offset(
        &payload,
        pointer(&payload, &record, "offset").expect("source offset"),
        "isInspection",
    )
    .unwrap();
    let mut reordered = payload.clone();
    let start = basic - (1 + "isBasic".len());
    reordered[start..=inspection].rotate_left(1 + "isBasic".len() + 1);
    let other = parse_payload(&reordered, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    let mut left = serde_json::to_value(&original[0]).unwrap();
    let mut right = serde_json::to_value(&other[0]).unwrap();
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
        patch_slots::field_offset(
            &reordered,
            pointer(&reordered, &record, "offset").expect("source offset"),
            "isBasic"
        )
        .unwrap()
    );
    assert_ne!(
        inspection,
        patch_slots::field_offset(
            &reordered,
            pointer(&reordered, &record, "offset").expect("source offset"),
            "isInspection"
        )
        .unwrap()
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
    let text_off = usize::try_from(
        pointer(&payload, &record, "display_text_offset").expect("display text offset"),
    )
    .unwrap();
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
    let start = usize::try_from(pointer(&payload, &record, "value_offset").expect("source offset"))
        .unwrap();
    let edited = 0.05_f64;
    patched[start..start + 8].copy_from_slice(&edited.to_be_bytes());
    patched[usize::try_from(
        pointer(&payload, &record, "precision_offset").expect("source offset"),
    )
    .unwrap()] = 4;
    patched[usize::try_from(pointer(&payload, &record, "basic_offset").expect("source offset"))
        .unwrap()] = 0xc2;
    patched[usize::try_from(
        pointer(&payload, &record, "inspection_offset").expect("source offset"),
    )
    .unwrap()] = 0xc3;
    patched[usize::try_from(
        pointer(&payload, &record, "reference_only_offset").expect("source offset"),
    )
    .unwrap()] = 0xc2;
    let text_off =
        usize::try_from(pointer(&payload, &record, "display_text_offset").expect("display text"))
            .unwrap();
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
    assert_eq!(
        pointer(&patched, &edited_record, "value_offset"),
        pointer(&payload, &record, "value_offset")
    );
    assert_eq!(
        pointer(&patched, &edited_record, "precision_offset"),
        pointer(&payload, &record, "precision_offset")
    );
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
        decoded.source_fidelity().record_layout()[&dimension.id]
            .pointers()
            .get("offset")
            .copied()
            .expect("source offset")
    );
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("PMI-backed parameter");
    assert_eq!(parameter.expression.literal_text().as_deref(), Some("25mm"));
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(25.0).unwrap()
        ))
    );
    let semantic = parameter.pmi.as_ref().expect("PMI semantics");
    assert_eq!(semantic, &super::annotation_id(dimension).unwrap());
    assert!(decoded.ir().model.pmi.iter().any(|annotation| &annotation.id == semantic && matches!(&annotation.definition, cadmpeg_ir::pmi::PmiDefinition::Dimension(dimension) if dimension.kind() == &cadmpeg_ir::pmi::DimensionKind::Size && dimension.nominal().and_then(cadmpeg_ir::Dimension::parameter) == Some(&parameter.id))));
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap();

    {
        let id = parameter.id.clone();
        let semantic = semantic.clone();
        let mut ir = decoded.ir_mut();
        ir.set_parameter_expression(
            &id,
            "50mm".into(),
            Some(ParameterValue::Length(Length::new(50.0).unwrap())),
        )
        .unwrap();
        for attribute in &mut ir.model.attributes {
            if attribute.target != cadmpeg_ir::attributes::AttributeTarget::Pmi(semantic.clone()) {
                continue;
            }
            attribute.values = vec![match attribute.name.as_str() {
                "precision" => Some(cadmpeg_ir::attributes::AttributeValue::Integer(4)),
                "basic" | "reference_only" => {
                    Some(cadmpeg_ir::attributes::AttributeValue::Boolean(false))
                }
                "inspection" => Some(cadmpeg_ir::attributes::AttributeValue::Boolean(true)),
                _ => continue,
            }];
        }
        for annotation in &mut ir.model.pmi {
            if let cadmpeg_ir::pmi::PmiDefinition::Presentation {
                text, semantics, ..
            } = &mut annotation.definition
            {
                if semantics.contains(&semantic) {
                    *text = Some("50.000 mm".into());
                }
            }
        }
    }

    let mut encoded = Vec::new();
    write_with_regenerated_pmi(decoded.ir(), decoded.source_fidelity(), &mut encoded);
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_written_scalar_with_pmi(&regenerated);
    let native = sldprt_native(regenerated.ir());
    let record = &native.pmi_dimensions[0];
    assert_eq!(record.value.get(), 0.05);
    assert_eq!(record.precision, 4);
    assert_eq!(record.display_text(), Some("50.000 mm"));
    assert!(!record.basic);
    assert!(record.inspection);
    assert!(!record.reference_only);
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
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
        ));
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
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
fn duplicate_pmi_records_share_one_parameter_and_report_annotation_loss() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    for guid in [
        "01234567-89ab-cdef-0123-456789abcdef",
        "fedcba98-7654-3210-fedc-ba9876543210",
    ] {
        source.extend(make_block(
            0x49,
            "Contents/PMISemanticDataDB",
            &pmi_semantic_payload_for_with_guid("D1@Sketch1", guid),
        ));
    }

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
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
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
        ));
    }

    let mut encoded = Vec::new();
    write_with_regenerated_pmi(decoded.ir(), decoded.source_fidelity(), &mut encoded);
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_written_scalar_with_pmi(&regenerated);
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
fn ordinate_pmi_values_survive_reported_annotation_omission() {
    use cadmpeg_ir::{features::ParameterValue, scalar::Length};

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
        let ordinate = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "D1")
            .expect("ordinate parameter");
        assert_eq!(
            ordinate.value,
            Some(ParameterValue::Length(Length::new(25.0).unwrap()))
        );
        let annotation = decoded
            .ir()
            .model
            .pmi
            .iter()
            .find(|annotation| Some(&annotation.id) == ordinate.pmi.as_ref())
            .unwrap();
        assert!(
            matches!(&annotation.definition, cadmpeg_ir::pmi::PmiDefinition::Dimension(dimension) if dimension.kind() == &cadmpeg_ir::pmi::DimensionKind::Location)
        );
        let id = ordinate.id.clone();
        decoded
            .ir_mut()
            .set_parameter_expression(
                &id,
                "50mm".into(),
                Some(ParameterValue::Length(Length::new(50.0).unwrap())),
            )
            .unwrap();
    }

    let mut encoded = Vec::new();
    write_with_regenerated_pmi(decoded.ir(), decoded.source_fidelity(), &mut encoded);
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_written_scalar_with_pmi(&regenerated);
}

#[test]
fn decode_uses_pmi_dimension_to_project_sparse_extrusion() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureOperation,
        LinearTermination, PlanarProfileRef, ProfileRef,
    };

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
        }) if decoded.ir().dimension_value(actual_length).unwrap().get() == 25.0
    ));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&decoded.ir().model.features[0].id))
        .expect("PMI extrusion parameter");
    assert_eq!(parameter.name, "D1");
    assert_eq!(parameter.expression.literal_text().as_deref(), Some("25mm"));
    assert!(parameter.pmi.is_some());
}

#[test]
fn u16_precision_survives_plain_patch_round_trip() {
    let mut payload = pmi_semantic_payload();
    let record = parse_payload(&payload, &mut Vec::new()).remove(0);
    let offset =
        usize::try_from(pointer(&payload, &record, "precision_offset").expect("source offset"))
            .unwrap();
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
    patch_payload(
        decoded.ir(),
        &record.parent,
        &mut payload,
        decoded.source_fidelity(),
    )
    .unwrap();
    assert_eq!(&payload[offset..offset + 3], &[0xcd, 0x00, 0x03]);
    assert_eq!(payload, original);
}

#[test]
fn pmi_parameter_ordinal_overflow_is_refused_not_saturated() {
    let owner = FeatureId::mint("synthetic:test:id#feature").expect("identity grammar");
    let feature = named_feature("synthetic:test:id#feature", "Pattern1");
    let mut parameters = vec![DesignParameter {
        driving: true,
        id: ParameterId::mint("synthetic:test:id#other-parameter").expect("identity grammar"),
        owner: Some(owner),
        ordinal: u32::MAX,
        name: "D9".into(),
        expression: "12mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(12.0).unwrap())),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("other-dimension".into()),
    }];
    let error = super::apply_to_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &mut parameters,
        &[feature],
        &[dimension("Linear", 0.034)],
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert_eq!(parameters.len(), 1);
}

#[test]
fn patching_a_count_beyond_float_precision_is_refused_not_rounded() {
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
        br#"<Keywords><Pattern Name="Sketch1" Type="LinearPattern" id="7" Direction="1,0,0"><Dimension Name="D1">2</Dimension><Dimension Name="D3">1mm</Dimension></Pattern></Keywords>"#,
    ));
    source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parent = sldprt_native(decoded.ir()).pmi_dimensions[0].parent.clone();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let id = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "D1")
            .unwrap()
            .id
            .clone();
        decoded
            .ir_mut()
            .set_parameter_expression(
                &id,
                ((1_i64 << 53) + 1).to_string().into(),
                Some(ParameterValue::Integer((1_i64 << 53) + 1)),
            )
            .unwrap();
    }

    let mut patched = payload;
    let error = patch_payload(
        decoded.ir(),
        &parent,
        &mut patched,
        decoded.source_fidelity(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::UnsupportedContent(_)
    ));
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
    let mut within = vec![0x91; 16];
    within.push(0xc0);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &within,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let work = cadmpeg_core::decode::WorkBudget::new(usize::MAX);
    let parse = super::Parse {
        shape: super::Shape::Record,
        work: &work,
    };
    assert!(super::parse_value(&ctx, root, &mut 0, 0, parse)
        .unwrap()
        .is_some());
    let mut beyond = vec![0x91; 18];
    beyond.push(0xc0);
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &beyond,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    assert!(super::parse_value(&ctx, root, &mut 0, 0, parse)
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
    let namespace = ir.native.namespace_mut("sldprt");
    namespace
        .arenas_mut()
        .insert("pmi_dimensions".into(), vec![record]);
    // The fixture is not a complete dimension record; only the absence of a resource refusal
    // is asserted.
    if let Err(error) = crate::native::SldprtNative::load(namespace) {
        assert!(!matches!(
            cadmpeg_core::CodecError::from(error),
            cadmpeg_core::CodecError::ResourceLimit(_)
        ));
    }
    assert!(!matches!(
        patch_payload(
            &ir,
            "block",
            &mut [],
            &cadmpeg_ir::SourceFidelity::default()
        ),
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

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let work = cadmpeg_core::decode::WorkBudget::new(usize::MAX);
    let parse = super::Parse {
        shape: super::Shape::Record,
        work: &work,
    };
    let mut cursor =
        usize::try_from(pointer(&payload, &records[0], "offset").expect("source offset")).unwrap();
    let outer = super::parse_value(&ctx, root, &mut cursor, 0, parse)
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

    let (moderate, moderate_stride) = overlapping_candidates(8, 2048);
    let mut payload = record.clone();
    payload.extend_from_slice(&moderate);
    let mut losses = Vec::new();
    assert_eq!(parse_payload(&payload, &mut losses), original);
    assert_eq!(losses.len(), 8);
    assert_untyped_candidate_maps(&losses, record.len(), moderate_stride, 8);

    let (chain, stride) = overlapping_candidates(CANDIDATES, 2048);
    let mut payload = record.clone();
    payload.extend_from_slice(&chain);
    let mut losses = Vec::new();
    assert_eq!(parse_payload(&payload, &mut losses), original);
    let cap_losses = losses
        .iter()
        .filter(|loss| loss.code == crate::loss::SldprtLossCode::PmiSemanticParseCapped.kind())
        .collect::<Vec<_>>();
    let [loss] = cap_losses.as_slice() else {
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
    assert_eq!(losses.len(), CANDIDATES - skipped + 1);
    assert_untyped_candidate_maps(&losses, record.len(), stride, CANDIDATES - skipped);
    assert_eq!(
        offset,
        record.len() + (CANDIDATES - skipped) * stride + 36,
        "the loss starts at the candidate the budget ran out in"
    );
}

fn assert_untyped_candidate_maps(
    losses: &[cadmpeg_ir::report::loss::LossNote],
    base: usize,
    stride: usize,
    count: usize,
) {
    let untyped = losses
        .iter()
        .filter(|loss| loss.code == crate::loss::SldprtLossCode::PmiSemanticRecordUntyped.kind())
        .collect::<Vec<_>>();
    assert_eq!(untyped.len(), count);
    for (index, loss) in untyped.iter().enumerate() {
        let offset = base + index * stride + 36;
        let guid = format!("{index:08x}-0000-4000-8000-{index:012x}");
        assert_eq!(loss.message, format!("PMISemanticDataDB map at offset {offset} (guid {guid}) has no supported typed semantic record"));
    }
}

#[test]
fn duplicate_member_identity_replays_but_refuses_semantic_edit() {
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
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
        ));
    }
    // A declined patch selects synthesis (#237 item 1).
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    assert!(matches!(plan.report().synthesis_cause(),
        Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { reason }) if !reason.is_empty()));
    plan.write_to(&mut Vec::new()).unwrap();
}
