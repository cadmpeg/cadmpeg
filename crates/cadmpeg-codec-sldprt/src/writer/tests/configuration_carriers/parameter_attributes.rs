// SPDX-License-Identifier: Apache-2.0
//! Native dimension attributes and canonical parameter facts.
#![allow(clippy::unwrap_used)]

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

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
    let native = sldprt_native(decoded.ir());
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = &mut ir_edit.model.parameters[0];
        // D1/D11: driving is canonical; EquationId is source spelling only.
        assert!(!parameter.driving);
        assert!(!parameter.properties.contains_key("Driven"));
        assert!(!parameter.properties.contains_key("EquationId"));
        let properties = &native.feature_histories[0].features[0].dimension_properties["Depth"];
        assert_eq!(properties["Driven"], "true");
        assert_eq!(properties["EquationId"], "D1@Boss");
        parameter.expression = "20mm".into();
        parameter.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
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
    let native = sldprt_native(decoded.ir());
    let source_properties = &native.feature_histories[0].features[0].dimension_properties["Depth"];
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = &mut ir_edit.model.parameters[0];
        assert_eq!(parameter.expression, "Width * 2".into());
        assert_eq!(
            parameter.value,
            Some(ParameterValue::Length(Length::new(24.0).unwrap()))
        );
        // D1: the typed cache owns Value; exact native spelling stays native.
        assert!(!parameter.properties.contains_key("Value"));
        assert!(!parameter.properties.contains_key("EquationId"));
        assert_eq!(source_properties["Value"], "24mm");
        assert_eq!(source_properties["EquationId"], "D1@Boss");
        parameter.expression = "Width * 3".into();
        parameter.value = Some(ParameterValue::Length(Length::new(36.0).unwrap()));
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
    let parameter = &regenerated.ir().model.parameters[0];
    assert_eq!(parameter.expression, "Width * 3".into());
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(36.0).unwrap()))
    );
    assert!(!parameter.properties.contains_key("Value"));
    assert!(!parameter.properties.contains_key("EquationId"));
    let native = sldprt_native(regenerated.ir());
    let properties = &native.feature_histories[0].features[0].dimension_properties["Depth"];
    assert_eq!(properties["Value"], "36mm");
    assert_eq!(properties["EquationId"], "D1@Boss");
}
