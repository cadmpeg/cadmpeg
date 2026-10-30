// SPDX-License-Identifier: Apache-2.0
//! Named sketch constraint expression tests.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;

#[test]
fn named_constraint_recipe_bindings() {
    for version in ["0.21.2", "1.0.2", "1.1.4"] {
        for (recipe, expression, cell_name) in [
            ("P12a", "Parameters.plate_w", "plate_w"),
            ("N11a", "Dimensions.w", "w"),
            ("N10a", "Sketch.Constraints.width / 2", ""),
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../corpus/freecad_fcstd/fixtures")
                .join(format!("named_constraint_{version}_{recipe}.FCStd"));
            let bytes = std::fs::read(path).expect("public recipe fixture");
            let result = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("recipe decode");
            let parameters = &result.ir().model.parameters;
            let parameter = parameters
                .iter()
                .find(|p| p.expression == expression)
                .expect("exact expression");
            let dependency = parameters
                .iter()
                .find(|p| {
                    if recipe == "N10a" {
                        p.properties
                            .get("source_name")
                            .is_some_and(|name| name == "width")
                    } else {
                        p.name == cell_name
                    }
                })
                .expect("dependency parameter");
            assert_eq!(
                parameter.dependencies.iter().collect::<Vec<_>>(),
                vec![&dependency.id],
                "{version} {recipe}"
            );
            if recipe != "N10a" {
                assert_eq!(
                    parameter.properties.get("source_name").map(String::as_str),
                    Some("width")
                );
                assert!(parameter.properties.contains_key("expression_native_ref"));
            }
            assert_valid_document(result.ir());
            assert!(crate::test_support::validate_native(result.ir()).is_empty());
        }
    }
}

#[test]
fn named_constraint_path_grammar_and_ambiguity() {
    for (names, paths, bound, loss) in [
        (
            [" Name=\"width\"", ""],
            vec![".Constraints.width"],
            true,
            None,
        ),
        ([" Name=\"width\"", ""], vec!["Constraints[1]"], true, None),
        (["", " Name=\"\""], vec![".Constraints."], false, None),
        ([" Name=\"\"", ""], vec![".Constraints."], false, None),
        (
            [" Name=\"width\"", " Name=\"width\""],
            vec!["Constraints[1]"],
            true,
            Some(crate::loss::FreecadLossCode::SketchConstraintNameAmbiguous),
        ),
        (
            [" Name=\"width\"", ""],
            vec!["Constraints.width"],
            false,
            None,
        ),
        (
            [" Name=\"width\"", ""],
            vec!["..Constraints.width"],
            false,
            None,
        ),
        (
            [" Name=\"width\"", ""],
            vec![".Constraints[1]"],
            false,
            None,
        ),
        (
            [" Name=\"width\"", ""],
            vec!["Sketch.Constraints.width"],
            false,
            None,
        ),
        (
            [" Name=\"width\"", " Name=\"width\""],
            vec![".Constraints.width"],
            false,
            Some(crate::loss::FreecadLossCode::SketchConstraintNameAmbiguous),
        ),
        (
            [" Name=\"width\"", ""],
            vec!["Constraints[1]", ".Constraints.width"],
            false,
            Some(crate::loss::FreecadLossCode::SketchConstraintBindingConflict),
        ),
    ] {
        let expressions = paths.iter().fold(String::new(), |mut out, path| {
            use std::fmt::Write as _;
            write!(out, r#"<Expression path="{path}" expression="Sheet.w"/>"#).unwrap();
            out
        });
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sketch" type="Sketcher::SketchObject"/><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="2">
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="1"><Cell address="A1" alias="w" content="12"/></Cells></Property></Properties></Object>
<Object name="Sketch"><Properties Count="3">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1"><Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="12" EndY="0"/></Geometry></GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="2">
<Constrain Type="2" First="0" FirstPos="0"{}/>
<Constrain Type="6" First="0" FirstPos="1" Second="0" SecondPos="2" Value="12"{}/>
</ConstraintList></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="{}">{expressions}</ExpressionEngine></Property>
</Properties></Object></ObjectData></Document>"#,
            names[1],
            names[0],
            paths.len()
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .expect("synthetic document");
        let parameter = result
            .ir()
            .model
            .parameters
            .iter()
            .find(|p| p.name == "Constraint2")
            .expect("dimension parameter");
        assert_eq!(
            parameter.expression,
            if bound { "Sheet.w" } else { "12" },
            "{paths:?}"
        );
        assert_eq!(
            parameter.dependencies.len(),
            usize::from(bound),
            "{paths:?}"
        );
        assert_eq!(
            parameter.properties.contains_key("expression_native_ref"),
            bound
        );
        if let Some(loss) = loss {
            assert!(result
                .report()
                .losses
                .iter()
                .any(|note| note.code == loss.note("").code));
        } else {
            assert!(result.report().losses.is_empty(), "{paths:?}");
        }
        assert_valid_document(result.ir());
        assert!(crate::test_support::validate_native(result.ir()).is_empty());
    }
}
