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

    // Replacing the canonical operand must take precedence over the original
    // signed source control, while that authored driver's formula stays held.
    let mut edited = plain.clone();
    let feature = edited
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Thickness"))
        .unwrap();
    let owner = feature.id.clone();
    let mut definition = feature.definition().clone();
    let operation = match &mut definition {
        cadmpeg_ir::features::FeatureDefinition::Operation(operation)
        | cadmpeg_ir::features::FeatureDefinition::PostProcess { operation, .. } => operation,
    };
    let independent_id =
        cadmpeg_ir::features::ParameterId::mint("test:parameter:independent#0").unwrap();
    let FeatureOperation::Shell { thickness, .. } = operation else {
        panic!("shell");
    };
    *thickness = Some(cadmpeg_ir::Dimension::Parameter(independent_id.clone()));
    let value = ParameterValue::Length(cadmpeg_ir::scalar::Length::new(3.0).unwrap());
    let ordinal = edited
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(&owner))
        .map(|parameter| parameter.ordinal)
        .max()
        .unwrap()
        + 1;
    edited
        .model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: independent_id,
            owner: Some(owner.clone()),
            ordinal,
            name: "Independent".into(),
            expression: cadmpeg_ir::features::Expression::literal(&value),
            driving: true,
            display: None,
            value: Some(value),
            properties: Default::default(),
            pmi: None,
            native_ref: None,
        });
    edited.set_feature_definition(&owner, definition).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&edited, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let feature = reread
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Thickness"))
        .unwrap();
    assert!(
        matches!(feature.definition().operation(), FeatureOperation::Shell { thickness: Some(dimension), outward: Some(false), .. }
        if reread.ir().dimension_value(dimension).unwrap().get() == 3.0)
    );
    let raw = reread
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "Value"
        })
        .unwrap();
    assert!(matches!(raw.value, Some(ParameterValue::Length(value)) if value.get() == -2.0));
    assert_eq!(raw.expression.dependencies().count(), 1);
    let driver = reread
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "Driver"
        })
        .unwrap();
    assert_eq!(
        raw.expression.tokens,
        vec![
            ExpressionToken::Text("-2 mm * ".into()),
            ExpressionToken::Parameter(driver.id.clone())
        ]
    );

    let mut literal = edited.clone();
    let mut definition = literal
        .model
        .features
        .iter()
        .find(|feature| feature.id == owner)
        .unwrap()
        .definition()
        .clone();
    let operation = match &mut definition {
        cadmpeg_ir::features::FeatureDefinition::Operation(operation)
        | cadmpeg_ir::features::FeatureDefinition::PostProcess { operation, .. } => operation,
    };
    let FeatureOperation::Shell { thickness, .. } = operation else {
        panic!("shell");
    };
    *thickness = Some(cadmpeg_ir::Dimension::Literal(
        cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
    ));
    literal.set_feature_definition(&owner, definition).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&literal, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let feature = reread
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Thickness"))
        .unwrap();
    assert!(
        matches!(feature.definition().operation(), FeatureOperation::Shell { thickness: Some(dimension), outward: Some(false), .. }
        if reread.ir().dimension_value(dimension).unwrap().get() == 4.0)
    );
    assert!(reread
        .ir()
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.owner.as_ref() == Some(&feature.id)
            && parameter.name == "Value"
            && matches!(parameter.value, Some(ParameterValue::Length(value)) if value.get() == -2.0)
            && parameter.expression.dependencies().count() == 1));
}
