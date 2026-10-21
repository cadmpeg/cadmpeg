// SPDX-License-Identifier: Apache-2.0
//! Signed native drivers retain their canonical computation graph in synthesis.

use crate::{test_support::test_archive::archive, FcstdCodec};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::features::{ExpressionToken, FeatureOperation, ParameterValue};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};
use std::io::Cursor;

fn assert_thickness_graph(ir: &CadIr) {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Thickness"))
        .unwrap();
    let parameters = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(&feature.id))
        .collect::<Vec<_>>();
    assert_eq!(
        parameters.len(),
        3,
        "one raw driver, one formula input, one semantic magnitude"
    );
    let parameter = |name| {
        *parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap()
    };
    let input = parameter("Driver");
    assert!(matches!(input.value, Some(ParameterValue::Real(value)) if value.get() == 1.0));
    let raw = parameter("Value");
    assert!(matches!(raw.value, Some(ParameterValue::Length(value)) if value.get() == -2.0));
    assert_eq!(
        raw.expression.tokens,
        vec![
            ExpressionToken::Text("-2 mm * ".into()),
            ExpressionToken::Parameter(input.id.clone())
        ]
    );
    let magnitude = parameter("Value_magnitude");
    assert!(!magnitude.driving);
    assert!(matches!(magnitude.value, Some(ParameterValue::Length(value)) if value.get() == 2.0));
    assert_eq!(
        magnitude.expression.tokens,
        vec![
            ExpressionToken::Text("abs(".into()),
            ExpressionToken::Parameter(raw.id.clone()),
            ExpressionToken::Text(")".into())
        ]
    );
    assert!(
        matches!(feature.definition().operation(), FeatureOperation::Shell { thickness: Some(dimension), outward: Some(false), .. }
        if dimension.parameter() == Some(&magnitude.id) && ir.dimension_value(dimension).unwrap().get() == 2.0)
    );
}

#[test]
fn source_less_signed_thickness_keeps_raw_value_formula_and_magnitude_driver() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Box" name="Base"/><Object type="Part::Thickness" name="Thickness"/></Objects>
<ObjectData Count="2">
<Object name="Base"><Properties Count="3">
<Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
<Property name="Width" type="App::PropertyLength"><Float value="10"/></Property>
<Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
</Properties></Object>
<Object name="Thickness"><Properties Count="8">
<Property name="Faces" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Face1"/></LinkSub></Property>
<Property name="Driver" type="App::PropertyFloat" group="Parameters"><Float value="1"/></Property>
<Property name="Value" type="App::PropertyLength"><Float value="-2"/></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Value" expression="-2 mm * Driver"/></ExpressionEngine></Property>
<Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
<Property name="Join" type="App::PropertyEnumeration"><Integer value="0"/></Property>
<Property name="Intersection" type="App::PropertyBool"><Bool value="false"/></Property>
<Property name="SelfIntersection" type="App::PropertyBool"><Bool value="false"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_thickness_graph(decoded.ir());
    // Persist only the public document; no source image or SourceFidelity can
    // participate in this synthesis or recover the driver's authored name.
    let plain: CadIr = serde_json::from_value(serde_json::to_value(decoded.ir()).unwrap()).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&plain, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_thickness_graph(reread.ir());
    let objects = reread
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    assert!(objects
        .iter()
        .any(|object| object.name() == "Thickness" && object.type_name == "Part::Thickness"));
}
