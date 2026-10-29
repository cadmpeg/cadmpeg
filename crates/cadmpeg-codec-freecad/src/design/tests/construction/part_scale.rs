// SPDX-License-Identifier: Apache-2.0
//! Part scale construction tests.

use super::*;

#[test]
fn transfers_uniform_and_anisotropic_part_scale() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Box" name="Source" id="1"/>
 <Object type="Part::Scale" name="Uniform" id="2"/>
 <Object type="Part::Scale" name="Anisotropic" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Source"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="1"/></Property>
 </Properties></Object>
 <Object name="Uniform"><Properties Count="3">
  <Property name="Base" type="App::PropertyLink"><Link value="Source"/></Property>
  <Property name="Uniform" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="UniformScale" type="App::PropertyFloat"><Float value="-2"/></Property>
 </Properties></Object>
 <Object name="Anisotropic"><Properties Count="5">
  <Property name="Base" type="App::PropertyLink"><Link value="Source"/></Property>
  <Property name="Uniform" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="XScale" type="App::PropertyFloat"><Float value="2"/></Property>
  <Property name="YScale" type="App::PropertyFloat"><Float value="3"/></Property>
  <Property name="ZScale" type="App::PropertyFloat"><Float value="4"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part scale");
    let definition = |name: &str| {
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .evaluation
            .definition()
    };
    assert!(matches!(
        definition("Uniform"),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Scale {
            center: Some(cadmpeg_ir::features::ScaleCenter::ModelOrigin),
            factors: cadmpeg_ir::features::ScaleFactors::Uniform { factor },
            ..
        }) if result.ir().dimension_value(factor).expect("resolved dimension").get() == -2.0
    ));
    assert!(matches!(
        definition("Anisotropic"),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Scale {
            factors: cadmpeg_ir::features::ScaleFactors::PerAxis { factors },
            ..
        }) if factors.each_ref().map(|value| result.ir().dimension_value(value).unwrap().get()) == [2.0, 3.0, 4.0]
    ));
}

#[test]
fn distinguishes_absent_and_malformed_part_scale_uniform_flag() {
    fn document(uniform_property: Option<&str>) -> String {
        let uniform = uniform_property.unwrap_or_default();
        let count = 5 + usize::from(!uniform.is_empty());
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Box" name="Source"/><Object type="Part::Scale" name="Scale"/></Objects>
<ObjectData Count="2"><Object name="Source"><Properties Count="3">
<Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
<Property name="Width" type="App::PropertyLength"><Float value="1"/></Property>
<Property name="Height" type="App::PropertyLength"><Float value="1"/></Property>
</Properties></Object>
<Object name="Scale"><Properties Count="{count}">
<Property name="Base" type="App::PropertyLink"><Link value="Source"/></Property>
{uniform}
<Property name="UniformScale" type="App::PropertyFloat"><Float value="2"/></Property>
<Property name="XScale" type="App::PropertyFloat"><Float value="3"/></Property>
<Property name="YScale" type="App::PropertyFloat"><Float value="4"/></Property>
<Property name="ZScale" type="App::PropertyFloat"><Float value="5"/></Property>
</Properties></Object></ObjectData></Document>"#
        )
    }

    let absent = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document(None))),
            &DecodeOptions::default(),
        )
        .expect("absent Part scale flag");
    assert!(matches!(
        definition(&absent, "Scale"),
        FeatureDefinition::Operation(FeatureOperation::Scale {
            factors: cadmpeg_ir::features::ScaleFactors::Uniform { factor },
            ..
        }) if absent.ir().dimension_value(factor).expect("resolved dimension").get() == 2.0
    ));
    assert_valid_document(absent.ir());

    let valid =
        r#"<Property name="Uniform" type="App::PropertyBool"><Bool value="false"/></Property>"#;
    let valid = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document(Some(valid)))),
            &DecodeOptions::default(),
        )
        .expect("valid Part scale flag");
    assert!(matches!(
        definition(&valid, "Scale"),
        FeatureDefinition::Operation(FeatureOperation::Scale {
            factors: cadmpeg_ir::features::ScaleFactors::PerAxis { factors },
            ..
        }) if factors.each_ref().map(|value| valid.ir().dimension_value(value).unwrap().get()) == [3.0, 4.0, 5.0]
    ));
    assert_valid_document(valid.ir());

    let malformed_values = [
        r#"<Property name="TARGET" type="App::PropertyString"><String value="false"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyInteger"><Integer value="0"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Bool value="1"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Wrapper><Bool value="false"/></Wrapper></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Bool value="false"/><Bool value="true"/></Property>"#,
    ];
    for malformed in malformed_values {
        let replacement = malformed.replace("TARGET", "Uniform");
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document(Some(&replacement)))),
                &DecodeOptions::default(),
            )
            .expect("malformed Part scale flag");
        assert!(matches!(
            definition(&result, "Scale"),
            FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) if kind.as_str() == "Part::Scale"
        ));
        crate::design::tests::assert_native_retention(&result, 1);
        assert_valid_document(result.ir());
    }
}
