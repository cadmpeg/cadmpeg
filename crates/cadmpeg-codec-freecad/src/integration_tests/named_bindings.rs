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
#[test]
fn named_constraint_recipe_profiles_constraints_and_body_measures() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, FeatureDefinition, FeatureOperation, LinearTermination,
        ParameterValue, PlanarProfileRef, ProfileRef,
    };
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchGeometryDefinition};
    use cadmpeg_measure::test_support::round_trip::measured;

    const LENGTH_TOLERANCE: f64 = 1.0e-12;
    const VOLUME_TOLERANCE: f64 = 1.0e-6;
    const AREA_TOLERANCE: f64 = 1.0e-6;
    const BOX_TOLERANCE: f64 = 1.0e-6;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures");
    for version in ["0.21.2", "1.0.2", "1.1.4"] {
        for (recipe, x, y, width, height, depth) in [
            ("P12a", -10.0, -15.0, 20.0, 30.0, 10.0),
            ("N11a", 0.0, 0.0, 30.0, 20.0, 10.0),
            ("N10a", 0.0, 0.0, 40.0, 20.0, 20.0),
        ] {
            let label = format!("{version} {recipe}");
            let bytes =
                std::fs::read(root.join(format!("named_constraint_{version}_{recipe}.FCStd")))
                    .unwrap();
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let model = &decoded.ir().model;
            let [sketch] = model.sketches.as_slice() else {
                panic!("{label}: expected one sketch")
            };
            let lines = model
                .sketch_entities
                .iter()
                .filter(|entity| {
                    entity.sketch == sketch.id
                        && matches!(
                            entity.geometry.definition(),
                            SketchGeometryDefinition::Line { .. }
                        )
                })
                .collect::<Vec<_>>();
            assert_eq!(lines.len(), 4, "{label}");
            let expected = [
                ([x, y], [x + width, y]),
                ([x + width, y], [x + width, y + height]),
                ([x + width, y + height], [x, y + height]),
                ([x, y + height], [x, y]),
            ];
            for (line, (start, end)) in lines.iter().zip(expected) {
                let SketchGeometryDefinition::Line {
                    start: actual_start,
                    end: actual_end,
                } = line.geometry.definition()
                else {
                    unreachable!()
                };
                for (actual, expected) in [
                    actual_start.get().u,
                    actual_start.get().v,
                    actual_end.get().u,
                    actual_end.get().v,
                ]
                .into_iter()
                .zip(start.into_iter().chain(end))
                {
                    assert!(
                        (actual - expected).abs() < LENGTH_TOLERANCE,
                        "{label}: line coordinate"
                    );
                }
            }
            let constraints = model
                .sketch_constraints
                .iter()
                .filter(|constraint| constraint.sketch == sketch.id)
                .collect::<Vec<_>>();
            assert_eq!(constraints.len(), 11, "{label}");
            for (kind, expected_count) in [
                ("horizontal", 2),
                ("vertical", 2),
                ("distance", 2),
                ("coincident_loci", if recipe == "P12a" { 4 } else { 5 }),
            ] {
                let count = constraints
                    .iter()
                    .filter(|constraint| match kind {
                        "horizontal" => matches!(
                            constraint.definition.kind(),
                            SketchConstraintDefinitionInput::Horizontal { .. }
                        ),
                        "vertical" => matches!(
                            constraint.definition.kind(),
                            SketchConstraintDefinitionInput::Vertical { .. }
                        ),
                        "distance" => matches!(
                            constraint.definition.kind(),
                            SketchConstraintDefinitionInput::Distance { .. }
                        ),
                        "coincident_loci" => matches!(
                            constraint.definition.kind(),
                            SketchConstraintDefinitionInput::CoincidentLoci { .. }
                        ),
                        _ => unreachable!(),
                    })
                    .count();
                assert_eq!(count, expected_count, "{label}: {kind}");
            }
            if recipe == "P12a" {
                assert!(
                    constraints.iter().any(|constraint| matches!(
                        constraint.definition.kind(),
                        SketchConstraintDefinitionInput::PointSymmetric { .. }
                    )),
                    "{label}: symmetry"
                );
            }
            for (name, entity, expected) in
                [("width", lines[0], width), ("height", lines[1], height)]
            {
                let constraint = constraints
                    .iter()
                    .find(|constraint| constraint.name.as_deref() == Some(name))
                    .unwrap();
                let SketchConstraintDefinitionInput::Distance {
                    entities,
                    parameter,
                } = constraint.definition.kind()
                else {
                    panic!("{label}: {name} constraint")
                };
                assert_eq!(entities.as_slice(), [entity.id().clone()], "{label}");
                let parameter = model
                    .parameters
                    .iter()
                    .find(|p| &p.id == parameter)
                    .unwrap();
                let Some(ParameterValue::Length(value)) = &parameter.value else {
                    panic!("{label}: {name} value")
                };
                assert!((value.get() - expected).abs() < LENGTH_TOLERANCE, "{label}");
            }
            let pad = model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some("Pad"))
                .unwrap();
            let (FeatureDefinition::Operation(operation)
            | FeatureDefinition::PostProcess { operation, .. }) = pad.evaluation.definition();
            let FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(profile)),
                extent: ExtrudeExtent::OneSided { side },
                op: BooleanOp::Join,
                ..
            } = operation
            else {
                panic!("{label}: expected extrusion")
            };
            assert_eq!(profile, &sketch.id, "{label}");
            let LinearTermination::Blind { length } = side.termination else {
                panic!("{label}: expected blind pad")
            };
            assert!((length.get() - depth).abs() < LENGTH_TOLERANCE, "{label}");
            let [body_id] = pad.evaluation.outputs().as_slice() else {
                panic!("{label}: expected one pad result")
            };
            let report = measured(&label, decoded.ir()).unwrap();
            let body = report
                .bodies()
                .iter()
                .find(|body| body.body() == body_id.as_str())
                .unwrap();
            let volume = body
                .volume()
                .as_ref()
                .unwrap()
                .measured()
                .unwrap()
                .value()
                .value
                .get();
            let area = body
                .area()
                .as_ref()
                .unwrap()
                .measured()
                .unwrap()
                .value()
                .value
                .get();
            assert!(
                (volume - width * height * depth).abs() < VOLUME_TOLERANCE,
                "{label}: volume {volume}"
            );
            assert!(
                (area - 2.0 * (width * height + width * depth + height * depth)).abs()
                    < AREA_TOLERANCE,
                "{label}: area {area}"
            );
            let bbox = body.bbox().measured().unwrap().value();
            for (actual, expected) in bbox
                .min
                .coordinates()
                .into_iter()
                .chain(bbox.max.coordinates())
                .zip([x, y, 0.0, x + width, y + height, depth])
            {
                assert!(
                    (actual.get() - expected).abs() < BOX_TOLERANCE,
                    "{label}: bbox"
                );
            }
        }
    }
}
