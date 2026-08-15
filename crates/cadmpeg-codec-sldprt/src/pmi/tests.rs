// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};
use cadmpeg_ir::LossTaxonomy;

use crate::test_support::*;
use crate::SldprtCodec;

use cadmpeg_ir::features::{
    DesignParameter, Feature, FeatureDefinition, FeatureId, Length, ParameterId, ParameterValue,
    PmiDimensionSubtype,
};

use super::*;

fn dimension(subtype: &str, value: f64) -> PmiDimension {
    PmiDimension {
        id: "dimension".into(),
        parent: "block".into(),
        offset: 0,
        guid: "guid".into(),
        cad_text: "D1@Pattern1".into(),
        item_count: 1,
        subtype: subtype.into(),
        value,
        value_offset: 0,
        precision: 0,
        precision_offset: 0,
        display_text: None,
        display_text_offset: None,
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
        id: FeatureId(id.into()),
        ordinal: 0,
        name: Some(name.into()),
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::StoredGeometry,
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

    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, Length, PatternKind};

    let feature = Feature {
        id: FeatureId("pattern".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Linear {
                direction: None,
                spacing: Length(10.0),
                count: 2,
                second: None,
            },
        },
        native_ref: None,
    };

    assert!(neutral_parameter_is_count(&feature, "D1", None));
    assert!(neutral_parameter_is_count(&feature, "D2", None));
    assert!(!neutral_parameter_is_count(&feature, "D3", None));
}

#[test]
fn explicit_keywords_dimension_precedes_pmi_value() {
    let owner = FeatureId("feature".into());
    let feature = named_feature("feature", "Pattern1");
    let mut parameters = vec![DesignParameter {
        id: ParameterId("keywords-parameter".into()),
        owner: Some(owner),
        ordinal: 0,
        name: "D1".into(),
        expression: "12mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(12.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("keywords-dimension".into()),
    }];
    let record = dimension("Linear", 0.034);

    apply_to_parameters(&mut parameters, &[feature], &[record]);

    let parameter = &parameters[0];
    assert_eq!(parameter.expression, "12mm");
    assert_eq!(parameter.display, None);
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(12.0))));
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

#[test]
fn conflicting_pmi_dimensions_do_not_bind_a_parameter() {
    let feature = named_feature("feature", "Pattern1");
    let first = dimension("Linear", 0.034);
    let mut second = dimension("Linear", 0.035);
    second.id = "dimension-2".into();
    second.guid = "guid-2".into();
    let mut parameters = Vec::new();

    apply_to_parameters(&mut parameters, &[feature], &[first, second]);

    assert!(parameters.is_empty());
}

#[test]
fn equivalent_pmi_dimensions_bind_once_to_lowest_record_id() {
    let feature = named_feature("feature", "Pattern1");
    let canonical = dimension("Linear", 0.034);
    let mut alias = canonical.clone();
    alias.id = "dimension-2".into();
    alias.guid = "guid-2".into();
    let mut parameters = Vec::new();

    apply_to_parameters(&mut parameters, &[feature], &[canonical, alias]);

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
            parent_source_id: None,
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

    enrich_history_parameters(&mut history, &[first, second]);

    assert!(history[0].features[0].parameters.is_empty());
}

fn fixstr(bytes: &mut Vec<u8>, value: &str) {
    assert!(value.len() < 32);
    bytes.push(0xa0 | value.len() as u8);
    bytes.extend_from_slice(value.as_bytes());
}

fn push_array_header(bytes: &mut Vec<u8>, len: usize) {
    if len < 16 {
        bytes.push(0x90 | len as u8);
    } else {
        bytes.push(0xdc);
        bytes.extend_from_slice(&(len as u16).to_be_bytes());
    }
}

fn push_map_header(bytes: &mut Vec<u8>, len: usize) {
    bytes.push(0x80 | len as u8);
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
    assert_eq!(records[0].item_count, 16);
    assert_eq!(records[0].value, 0.025);
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
    assert_eq!(record.display_text.as_deref(), Some("cadText"));
    assert_eq!(record.cad_text, "D1@Sketch1");
    let text_off = record.display_text_offset.expect("display text offset") as usize;
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
    assert_eq!(records[0].value, 0.025);
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
    let start = record.value_offset as usize;
    let edited = 0.05_f64;
    patched[start..start + 8].copy_from_slice(&edited.to_be_bytes());
    patched[record.precision_offset as usize] = 4;
    patched[record.basic_offset as usize] = 0xc2;
    patched[record.inspection_offset as usize] = 0xc3;
    patched[record.reference_only_offset as usize] = 0xc2;
    let text_off = record.display_text_offset.expect("display text") as usize;
    patched[text_off..text_off + 9].copy_from_slice(b"50.000 mm");
    let mut again_losses = Vec::new();
    let again = parse_payload(&patched, &mut again_losses);
    assert!(again_losses.is_empty(), "{again_losses:?}");
    let [edited_record] = again.as_slice() else {
        panic!("one edited record");
    };
    assert_eq!(edited_record.value, 0.05);
    assert_eq!(edited_record.precision, 4);
    assert!(!edited_record.basic);
    assert!(edited_record.inspection);
    assert!(!edited_record.reference_only);
    assert_eq!(edited_record.display_text.as_deref(), Some("50.000 mm"));
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

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one PMI dimension");
    };
    assert_eq!(dimension.guid, "01234567-89ab-cdef-0123-456789abcdef");
    assert_eq!(dimension.cad_text, "D1@Sketch1");
    assert_eq!(dimension.subtype, "Linear");
    assert_eq!(dimension.value, 0.025);
    assert_eq!(dimension.precision, 3);
    assert_eq!(dimension.display_text.as_deref(), Some("25.000 mm"));
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
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let semantic = parameter.pmi.as_ref().expect("PMI semantics");
    assert_eq!(
        semantic.subtype,
        cadmpeg_ir::features::PmiDimensionSubtype::Linear
    );
    assert_eq!(semantic.native_ref, dimension.id);
    SldprtCodec
        .write_preserved_with_source_fidelity(
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
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(50.0),
        ));
        let semantic = parameter.pmi.as_mut().expect("editable PMI semantics");
        semantic.precision = 4;
        semantic.display_text = Some("50.000 mm".into());
        semantic.basic = false;
        semantic.inspection = true;
        semantic.reference_only = false;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one regenerated PMI dimension");
    };
    assert_eq!(dimension.value, 0.05);
    assert_eq!(dimension.precision, 4);
    assert_eq!(dimension.display_text.as_deref(), Some("50.000 mm"));
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
        assert_eq!(dimension.value, value);
        assert_eq!(dimension.item_count, item_count);
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
    assert_eq!(dimension.item_count, 2);
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

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
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
            cadmpeg_ir::features::Length(50.0),
        ));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    assert_eq!(native.pmi_dimensions.len(), 2);
    assert!(native
        .pmi_dimensions
        .iter()
        .all(|dimension| dimension.value == 0.05));
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
    use cadmpeg_ir::features::{Length, ParameterValue, PmiDimensionSubtype};

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

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    {
        let mut ir = decoded.ir_mut();
        let ordinate = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "D1")
            .expect("ordinate parameter");
        assert_eq!(ordinate.value, Some(ParameterValue::Length(Length(25.0))));
        assert_eq!(
            ordinate.pmi.as_ref().map(|pmi| &pmi.subtype),
            Some(&PmiDimensionSubtype::Ordinate)
        );
        ordinate.expression = "50mm".into();
        ordinate.value = Some(ParameterValue::Length(Length(50.0)));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
            .map(|dimension| dimension.value),
        Some(0.05)
    );
}

#[test]
fn decode_uses_pmi_dimension_to_project_sparse_extrusion() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ProfileRef, Termination,
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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(25.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
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
