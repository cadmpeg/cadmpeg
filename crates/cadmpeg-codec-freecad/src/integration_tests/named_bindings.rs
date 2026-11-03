// SPDX-License-Identifier: Apache-2.0
//! Named sketch constraint expression tests.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;

#[test]
fn named_constraint_recipe_bindings() {
    use cadmpeg_ir::features::ExpressionToken;
    for version in ["0.21.2", "1.0.2", "1.1.4"] {
        for (recipe, cell_name) in [("P12a", "plate_w"), ("N11a", "w"), ("N10a", "width")] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../corpus/freecad_fcstd/fixtures")
                .join(format!("named_constraint_{version}_{recipe}.FCStd"));
            let bytes = std::fs::read(path).expect("public recipe fixture");
            let result = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("recipe decode");
            let parameters = &result.ir().model.parameters;
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
            let expected_tail: Option<&str> = if recipe == "N10a" { Some(" / 2") } else { None };
            let alias = parameters.iter().find(|p| {
                let mut tokens = p.expression.tokens.iter();
                if tokens.next() != Some(&ExpressionToken::Parameter(dependency.id.clone())) {
                    return false;
                }
                match (tokens.next(), expected_tail) {
                    (None, None) => true,
                    (Some(ExpressionToken::Text(tail)), Some(expected)) => {
                        tail == expected && tokens.next().is_none()
                    }
                    _ => false,
                }
            });
            if recipe == "N10a" {
                // The halved Length has no diameter-kind consumer, so #224
                // D31 keeps its verified alias: the original shape holds.
                let parameter = alias.expect("referring parameter");
                assert_eq!(
                    parameter.expression.dependencies().collect::<Vec<_>>(),
                    vec![&dependency.id],
                    "{version} {recipe}"
                );
            } else {
                // #224 D31: the sketch constraint dimension is driven by the
                // current ExpressionEngine, so the verified bridge removes
                // the alias and binds the constraint directly at the cell.
                // The halved Pad Length keeps its alias (see N10a).
                assert!(
                    alias.is_none(),
                    "{version} {recipe}: driven alias is bridged away"
                );
                let mut stack = Vec::new();
                let mut bound = false;
                for constraint in &result.ir().model.sketch_constraints {
                    stack.push(
                        serde_json::to_value(&constraint.definition)
                            .expect("constraint definition"),
                    );
                }
                while let Some(value) = stack.pop() {
                    match value {
                        serde_json::Value::Object(fields) => {
                            if fields.get("kind").and_then(serde_json::Value::as_str)
                                == Some("parameter")
                                && fields.get("value").and_then(serde_json::Value::as_str)
                                    == Some(dependency.id.as_str())
                            {
                                bound = true;
                            }
                            stack.extend(fields.values().cloned());
                        }
                        serde_json::Value::Array(items) => {
                            stack.extend(items.into_iter());
                        }
                        _ => {}
                    }
                }
                assert!(
                    bound,
                    "{version} {recipe}: constraint names the canonical parameter"
                );
                let namespace = result.ir().native.namespace("fcstd").expect("fcstd");
                let objects = namespace
                    .arena_as::<crate::native::ObjectRecord>("objects")
                    .expect("objects");
                let pad = objects
                    .iter()
                    .find(|object| object.type_name == "PartDesign::Pad")
                    .expect("pad object");
                let properties = namespace
                    .arena_as::<crate::native::PropertyRecord>("properties")
                    .expect("properties");
                let length = properties
                    .iter()
                    .find(|property| {
                        property.owner == pad.id().as_str() && property.name == "Length"
                    })
                    .expect("pad length property");
                let length_parameter = parameters
                    .iter()
                    .find(|parameter| parameter.native_ref.as_deref() == Some(length.id.as_str()))
                    .expect("pad length parameter");
                if recipe == "P12a" {
                    let mut tokens = length_parameter.expression.tokens.iter();
                    assert_eq!(
                        tokens.next(),
                        Some(&ExpressionToken::Parameter(dependency.id.clone())),
                        "{version} {recipe}: length still halves the cell"
                    );
                    assert_eq!(
                        tokens.next().and_then(|token| match token {
                            ExpressionToken::Text(tail) => Some(tail.as_str()),
                            _ => None,
                        }),
                        Some(" / 2"),
                        "{version} {recipe}"
                    );
                    assert!(tokens.next().is_none(), "{version} {recipe}");
                } else {
                    assert!(
                        length_parameter.expression.dependencies().next().is_none(),
                        "{version} {recipe}: length stays literal"
                    );
                }
            }
            assert!(
                result.report().losses.iter().all(|loss| {
                    !(loss.code.namespace() == "fcstd"
                        && loss.code.local_code() == "expression.reference-unresolved")
                }),
                "{version} {recipe}: verified bridge charges no loss: {:?}",
                result.report().losses
            );
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
        if bound {
            assert_eq!(parameter.expression.dependencies().count(), 1, "{paths:?}");
            assert!(parameter.expression.literal_text().is_none(), "{paths:?}");
        } else {
            assert_eq!(
                parameter.expression.literal_text().as_deref(),
                Some("12 mm"),
                "{paths:?}"
            );
            assert_eq!(parameter.expression.dependencies().count(), 0, "{paths:?}");
        }
        // #224 ruling 2: the ExpressionEngine link lives in the typed
        // `expression`, never in `properties`.
        assert!(!parameter.properties.contains_key("expression_native_ref"));
        if let Some(loss) = loss {
            assert!(result
                .report()
                .losses
                .iter()
                .any(|note| note.code == loss.note("").code));
        } else if bound {
            // #224 D31: the cell holds unitless Real while the constraint is
            // Length, so the kept quantity check leaves the driven control
            // explicit with its loss. Scope still reports only the missing
            // model class.
            assert!(result.ir().scope.design);
            assert!(!result.ir().scope.model);
            let losses = crate::test_support::non_placement_losses(result.report());
            assert_eq!(losses.len(), 2, "{losses:?}");
            assert!(losses.iter().any(|note| note.code.namespace() == "fcstd"
                && note.code.local_code() == "expression.reference-unresolved"));
            assert!(losses
                .iter()
                .any(|note| note.code.local_code() == "depth.not-delivered"));
        } else {
            crate::test_support::assert_design_fragment_scope(&result);
        }
        assert_valid_document(result.ir());
        assert!(crate::test_support::validate_native(result.ir()).is_empty());
    }
}
