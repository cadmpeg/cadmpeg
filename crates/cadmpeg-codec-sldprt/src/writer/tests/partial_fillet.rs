// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{
    edge_treatments::{RadiusForm, RadiusSpec},
    FeatureDefinition, FeatureOperation,
};

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn source_less_partial_variable_fillet_retains_its_radius_form() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Partial" Type="Fillet" id="41"><Dimension Name="Radius0">4mm</Dimension></Fillet></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let is_partial = |definition: &FeatureDefinition| {
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(groups.as_slice()[0].radius, RadiusSpec::Unresolved { form: Some(RadiusForm::Variable) }))
    };
    assert!(is_partial(
        decoded.ir().model.features[0].evaluation.definition()
    ));
    let mut ir = decoded.ir().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    for feature in &mut ir.model.features {
        feature.native_ref = None;
    }
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let feature = output
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Partial"))
        .unwrap();
    assert!(is_partial(feature.evaluation.definition()));
    assert!(output.ir().model.parameters.iter().any(|parameter| {
        parameter.owner.as_ref() == Some(&feature.id)
            && parameter.name == "Radius0"
            && parameter.value.as_ref().is_some_and(|value| matches!(value, cadmpeg_ir::features::ParameterValue::Length(length) if length.get() == 4.0))
    }));
}
