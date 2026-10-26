// SPDX-License-Identifier: Apache-2.0
//! PMI-only dimensions carry their source driving role into canonical parameters.

use super::*;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::pmi::PmiDefinition;

fn assert_parameter_and_pmi(
    decoded: &cadmpeg_ir::codec::DecodeResult,
    driving: bool,
    reference_only: bool,
) {
    let native = sldprt_native(decoded.ir());
    let [record] = native.pmi_dimensions.as_slice() else {
        panic!("one unchanged PMI source dimension");
    };
    assert_eq!(record.cad_text, "D1@Sketch1");
    assert_eq!(record.guid, "01234567-89ab-cdef-0123-456789abcdef");
    assert_eq!(record.subtype, "Linear");
    assert_eq!(record.value.get(), 0.025);
    assert_eq!(record.precision, 3);
    assert_eq!(record.display_text(), Some("25.000 mm"));
    assert!(record.basic);
    assert!(!record.inspection);
    assert_eq!(record.reference_only, reference_only);
    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one canonical dimension");
    };
    assert_eq!(parameter.name, "D1");
    assert_eq!(parameter.driving, driving);
    assert_eq!(parameter.expression, "25mm".into());
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(25.0).unwrap()))
    );
    let id = parameter.pmi.as_ref().expect("canonical PMI link");
    let annotation = decoded
        .ir()
        .model
        .pmi
        .iter()
        .find(|pmi| &pmi.id == id)
        .unwrap();
    let PmiDefinition::Dimension(dimension) = &annotation.definition else {
        panic!("semantic dimension");
    };
    assert_eq!(
        dimension
            .nominal()
            .and_then(cadmpeg_ir::Dimension::parameter),
        Some(&parameter.id)
    );
    assert_eq!(
        dimension
            .resolved_nominal(decoded.ir())
            .unwrap()
            .value
            .get(),
        25.0
    );
    assert!(decoded.ir().model.pmi.iter().any(|pmi| {
        matches!(&pmi.definition, PmiDefinition::Presentation { text, semantics, .. }
            if text.as_deref() == Some("25.000 mm") && semantics.as_slice() == std::slice::from_ref(id))
    }));
    for (name, value) in [
        ("basic", Some(AttributeValue::Boolean(true))),
        ("inspection", Some(AttributeValue::Boolean(false))),
        ("precision", Some(AttributeValue::Integer(3))),
        (
            "reference_only",
            Some(AttributeValue::Boolean(reference_only)),
        ),
    ] {
        assert!(decoded.ir().model.attributes.iter().any(|attribute| {
            matches!(&attribute.target, AttributeTarget::Pmi(target) if target == id)
                && attribute.name == name
                && attribute.values.as_slice() == std::slice::from_ref(&value)
        }));
    }
}

fn authored_control(keywords: &[u8], reference_only: bool) -> cadmpeg_ir::codec::DecodeResult {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", keywords));
    let mut payload = pmi_semantic_payload();
    if !reference_only {
        let parsed = parse_payload(&payload, &mut Vec::new());
        let offset =
            usize::try_from(super::pointer(&payload, &parsed[0], "reference_only_offset").unwrap())
                .unwrap();
        assert_eq!(payload[offset], 0xc3);
        payload[offset] = 0xc2;
    }
    source.extend(make_block(0x49, "Contents/PMISemanticDataDB", &payload));
    SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap()
}

#[test]
fn frozen_reference_only_pmi_parameter_is_not_driving() {
    let source = include_bytes!("../../../tests/golden/fixtures/authored_pmi_dimension.sldprt");
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_parameter_and_pmi(&decoded, false, true);
    let native = sldprt_native(decoded.ir());
    let feature = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|feature| feature.name == "Sketch1")
        .unwrap();
    assert!(feature.parameters.is_empty());
    assert!(feature.dimension_properties.is_empty());
}

#[test]
fn inferred_non_reference_pmi_parameter_is_driving() {
    let decoded = authored_control(
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
        false,
    );
    assert_parameter_and_pmi(&decoded, true, false);
    assert!(sldprt_native(decoded.ir()).feature_histories[0].features[0]
        .parameters
        .is_empty());
}

#[test]
fn actual_keywords_driven_fact_keeps_its_independent_role() {
    for (keywords, reference_only, driving, driven) in [
        (
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D1" Driven="false">25mm</Dimension></Sketch></Keywords>"#.as_slice(),
            true, true, "false",
        ),
        (
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D1" Driven="true">25mm</Dimension></Sketch></Keywords>"#.as_slice(),
            false, false, "true",
        ),
    ] {
        let decoded = authored_control(keywords, reference_only);
        assert_parameter_and_pmi(&decoded, driving, reference_only);
        let native = sldprt_native(decoded.ir());
        let feature = &native.feature_histories[0].features[0];
        assert_eq!(feature.parameters["D1"], "25mm");
        assert_eq!(feature.dimension_properties["D1"]["Driven"], driven);
    }
}
