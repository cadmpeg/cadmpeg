// SPDX-License-Identifier: Apache-2.0
//! Kernel tolerance modes and authored parameter admission.

use crate::design::tests::definition;
use crate::test_support::test_archive::archive;
use crate::FcstdCodec;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn tolerance_modes_do_not_create_parameters_but_authored_drivers_do() {
    for (value, declaration, expression, expected_driver) in [
        ("0", "", "", false),
        ("-1", "", "", false),
        ("0.01", "", "", true),
        ("0", " group=\"Parameters\"", "", true),
        (
            "0",
            "",
            r#"<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="FuzzyTolerance" expression="0 mm"/></ExpressionEngine></Property>"#,
            true,
        ),
    ] {
        let count = if expression.is_empty() { 4 } else { 5 };
        let document = format!(
            r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="PartDesign::AdditiveBox" name="Box"/></Objects><ObjectData Count="1"><Object name="Box"><Properties Count="{count}">
<Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
<Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
<Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
<Property name="FuzzyTolerance" type="App::PropertyFloatConstraint"{declaration}><Float value="{value}"/></Property>{expression}
</Properties></Object></ObjectData></Document>"#
        );
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            decoded.ir().model.parameters.len(),
            3 + usize::from(expected_driver)
        );
        let driver = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "FuzzyTolerance");
        assert_eq!(driver.is_some(), expected_driver, "{document}");
        if let Some(driver) = driver {
            assert!(
                matches!(&driver.value, Some(cadmpeg_ir::features::ParameterValue::Length(length)) if length.get() == value.parse::<f64>().unwrap())
            );
            assert!(driver.driving);
            if !expression.is_empty() {
                assert_eq!(driver.expression.literal_text().as_deref(), Some("0 mm"));
            }
        }
        assert!(
            matches!(definition(&decoded, "Box"), FeatureDefinition::PostProcess { fuzzy_tolerance, .. }
            if match value {
                "0" => matches!(fuzzy_tolerance, cadmpeg_ir::features::FuzzyTolerance::KernelDefault),
                "-1" => matches!(fuzzy_tolerance, cadmpeg_ir::features::FuzzyTolerance::Automatic),
                _ => matches!(fuzzy_tolerance, cadmpeg_ir::features::FuzzyTolerance::Explicit(dimension) if decoded.ir().dimension_value(dimension).unwrap().get() == 0.01),
            })
        );
        let properties = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let source = properties
            .iter()
            .find(|property| property.name == "FuzzyTolerance")
            .unwrap();
        assert_eq!(source.values()[0].attributes["value"], value);
    }
}
