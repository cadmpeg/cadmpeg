// SPDX-License-Identifier: Apache-2.0
//! Parameter edits and dimension metadata preservation.

use super::*;

#[test]
fn semantic_writer_applies_neutral_parameter_edits() {
    use cadmpeg_ir::{features::ParameterValue, scalar::Length};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Depth")
            .unwrap();
        parameter.expression = "20mm".into();
        parameter.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_history_after_refusal(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Depth"],
        "20mm"
    );
    assert_eq!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Depth")
            .unwrap()
            .expression,
        "20mm"
    );
}

#[test]
fn semantic_writer_preserves_dimension_attributes() {
    use cadmpeg_ir::{features::ParameterValue, scalar::Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth" Driven="true" EquationId="D1@Boss">12mm</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = &mut ir_edit.model.parameters[0];
        assert_eq!(parameter.properties["Driven"], "true");
        assert_eq!(parameter.properties["EquationId"], "D1@Boss");
        parameter.expression = "20mm".into();
        parameter.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_history_after_refusal(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Depth"], "20mm");
    assert_eq!(feature.dimension_properties["Depth"]["Driven"], "true");
    assert_eq!(
        feature.dimension_properties["Depth"]["EquationId"],
        "D1@Boss"
    );
}

#[test]
fn semantic_writer_preserves_evaluated_equation_values() {
    use cadmpeg_ir::{features::ParameterValue, scalar::Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth" Value="24mm" EquationId="D1@Boss">Width * 2</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = &mut ir_edit.model.parameters[0];
        assert_eq!(parameter.expression, "Width * 2");
        assert_eq!(
            parameter.value,
            Some(ParameterValue::Length(Length::new(24.0).unwrap()))
        );
        assert_eq!(parameter.properties["Value"], "24mm");
        parameter.expression = "Width * 3".into();
        parameter.value = Some(ParameterValue::Length(Length::new(36.0).unwrap()));
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_history_after_refusal(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = &regenerated.ir().model.parameters[0];
    assert_eq!(parameter.expression, "Width * 3");
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(36.0).unwrap()))
    );
    assert_eq!(parameter.properties["Value"], "36mm");
    assert_eq!(parameter.properties["EquationId"], "D1@Boss");
}
