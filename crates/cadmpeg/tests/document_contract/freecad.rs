// SPDX-License-Identifier: Apache-2.0
//! `FreeCAD` document edits survive both sidecar artifact flows.

use std::fs;
use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::CadIr;
use serde_json::json;

use super::{decode, workspace, write_decode};

const EXPRESSION: &str = "37 mm";
const CONSTRAINT_EXPRESSION: &str = "31 mm";
const FEATURE_NAME: &str = "AuthoredPad";
const EDIT_COLOR: [u8; 4] = [48, 96, 144, 255];
const EPS_PARAMETER_VALUE: f64 = 1e-9;

fn fixture(name: &str) -> Vec<u8> {
    fs::read(workspace().join("corpus/freecad_fcstd/fixtures").join(name)).unwrap()
}

fn edit_document(ir: &CadIr) -> CadIr {
    use cadmpeg_ir::features::{Expression, ParameterValue};
    use cadmpeg_ir::scalar::Length;

    let mut edited = ir.clone();
    let feature_id = edited
        .model
        .features
        .iter()
        .find(|feature| {
            feature.id.as_str().ends_with("#Pad")
                || feature.id.as_str().ends_with("#DesignedFeature")
        })
        .expect("the fixture holds an extrusion feature")
        .id
        .clone();
    let length = edited
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.id.as_str().ends_with("#Pad:Length")
                || parameter.id.as_str().ends_with("#DesignedFeature:Length")
        })
        .expect("Pad.Length is projected")
        .id
        .clone();
    let feature = edited
        .model
        .features
        .iter()
        .find(|feature| feature.id == feature_id)
        .unwrap();
    let definition = serde_json::to_value(feature.definition()).unwrap();
    let paths = feature
        .definition()
        .dimension_paths()
        .into_iter()
        .filter(|path| {
            definition.pointer(path).is_some_and(|dimension| {
                dimension["kind"] == "parameter"
                    && dimension["value"].as_str() == Some(length.as_str())
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        paths.len(),
        1,
        "the extrusion consumes its one canonical Length parameter"
    );
    let value = ParameterValue::Length(Length::new(37.0).unwrap());
    edited
        .set_parameter_expression(&length, Expression::literal(&value), Some(value))
        .unwrap();
    edited
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id == feature_id)
        .unwrap()
        .name = Some(FEATURE_NAME.into());
    let constraint = edited
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.id.as_str().contains(":constraint:"))
        .expect("the fixture has a dimensional sketch constraint")
        .id
        .clone();
    let value = ParameterValue::Length(Length::new(31.0).unwrap());
    edited
        .set_parameter_expression(&constraint, Expression::literal(&value), Some(value))
        .unwrap();
    assert!(
        edited
            .model
            .features
            .iter()
            .find(|feature| feature.id == feature_id)
            .unwrap()
            .needs_recompute
    );
    let mut value = serde_json::to_value(&edited).unwrap();
    let model = value["model"].as_object_mut().unwrap();
    let body = model["bodies"][0]["id"].clone();
    let appearance_id = "cadir:model:appearance#authored";
    let color = cadmpeg_ir::topology::Color::from_rgba8(
        EDIT_COLOR[0],
        EDIT_COLOR[1],
        EDIT_COLOR[2],
        EDIT_COLOR[3],
    );
    let appearances = model["appearances"].as_array_mut().unwrap();
    if let Some(appearance) = appearances.first_mut() {
        appearance["base_color"] = serde_json::to_value(color).unwrap();
    } else {
        appearances
            .push(json!({"id": appearance_id, "name": "Authored appearance", "base_color": color}));
        model["appearance_bindings"].as_array_mut().unwrap().push(json!({
            "id": "cadir:model:appearance-binding#authored", "target": {"kind": "body", "id": body}, "appearance": appearance_id
        }));
    }
    CadIr::from_json(&value.to_string())
        .expect("the canonical design and display edits form a valid document")
}

fn assert_edits(ir: &CadIr) {
    assert!(ir
        .model
        .features
        .iter()
        .any(|feature| feature.name.as_deref() == Some(FEATURE_NAME)));
    let length = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.expression.literal_text().as_deref() == Some(EXPRESSION))
        .expect("the edited canonical length expression survives");
    assert!(
        matches!(&length.value, Some(cadmpeg_ir::features::ParameterValue::Length(value)) if (value.get()-37.0).abs() <= EPS_PARAMETER_VALUE)
    );
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(FEATURE_NAME))
        .unwrap();
    let definition = serde_json::to_value(feature.definition()).unwrap();
    assert!(
        feature
            .definition()
            .dimension_paths()
            .iter()
            .any(|path| definition.pointer(path).is_some_and(|dimension| {
                dimension["kind"] == "parameter"
                    && dimension["value"].as_str() == Some(length.id.as_str())
            })),
        "the extrusion keeps its canonical parameter binding"
    );
    assert!(
        feature.needs_recompute,
        "FreeCAD must retain the edited feature's Touched state"
    );
    let dimensional = ir
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.expression.literal_text().as_deref() == Some(CONSTRAINT_EXPRESSION)
        })
        .expect("the edited constraint expression survives");
    assert!(
        matches!(&dimensional.value,Some(cadmpeg_ir::features::ParameterValue::Length(value)) if (value.get()-31.0).abs()<=EPS_PARAMETER_VALUE),
        "the edited constraint's numeric value must also survive"
    );
    let color = cadmpeg_ir::topology::Color::from_rgba8(
        EDIT_COLOR[0],
        EDIT_COLOR[1],
        EDIT_COLOR[2],
        EDIT_COLOR[3],
    );
    assert!(ir.model.appearance_bindings.iter().any(|binding| {
        ir.model.appearances.iter().any(|appearance|appearance.id==binding.appearance && appearance.base_color==Some(color))
            && matches!(&binding.target,cadmpeg_ir::appearance::AppearanceTarget::Body(body) if ir.model.bodies.iter().any(|candidate|candidate.id==*body))
    }),"the edited color must remain bound to a written body");
}

fn counts(ir: &CadIr) -> [usize; 9] {
    [
        ir.model.features.len(),
        ir.model.sketches.len(),
        ir.model.sketch_entities.len(),
        ir.model.sketch_constraints.len(),
        ir.model.parameters.len(),
        ir.model.bodies.len(),
        ir.model.spreadsheets.len(),
        ir.model.product_definitions.len(),
        ir.model.occurrences.len(),
    ]
}

#[test]
fn unedited_patch_supplies_no_content_beyond_the_synthesized_written_arenas() {
    for name in ["design_history.FCStd", "core_design_product.FCStd"] {
        let result = decode(&FcstdCodec, &fixture(name));
        let (patched, patch_report) = write_decode(
            &FcstdCodec,
            &FcstdCodec,
            result.ir(),
            Some(result.source_fidelity()),
            None,
        );
        let (synthesized, synth_report) =
            write_decode(&FcstdCodec, &FcstdCodec, result.ir(), None, None);
        assert!(matches!(
            patch_report.write_path(),
            WritePath::Patched { .. }
        ));
        assert!(matches!(
            synth_report.write_path(),
            WritePath::Synthesized { .. }
        ));
        cadmpeg_test_support::contract::assert_written_arenas_agree(
            &synth_report,
            patched.ir(),
            synthesized.ir(),
        );
    }
}

#[test]
fn design_history_full_edit_synthesizes_all_canonical_values() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};

    let source = decode(&FcstdCodec, &fixture("design_history.FCStd"));
    let edited = edit_document(source.ir());
    let plan = FcstdCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let (written, report) = write_decode(&FcstdCodec, &FcstdCodec, &edited, None, None);
    assert_edits(written.ir());
    assert_eq!(counts(written.ir()), counts(&edited));
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &edited, written.ir());
}

#[test]
fn product_feature_parameter_edit_survives_synthesis() {
    use cadmpeg_ir::features::{Dimension, Expression, ParameterValue};
    use cadmpeg_ir::scalar::Length;

    let source = decode(&FcstdCodec, &fixture("core_design_product.FCStd"));
    let mut edited = source.ir().clone();
    let parameter = edited
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.id.as_str().ends_with("#DesignedFeature:Length"))
        .unwrap()
        .id
        .clone();
    let owner = edited
        .model
        .parameters
        .iter()
        .find(|row| row.id == parameter)
        .unwrap()
        .owner
        .clone()
        .unwrap();
    let value = ParameterValue::Length(Length::new(37.0).unwrap());
    edited
        .set_parameter_expression(&parameter, Expression::literal(&value), Some(value))
        .unwrap();
    let feature = edited
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id == owner)
        .unwrap();
    feature.name = Some("AuthoredProductFeature".into());
    assert!(feature.needs_recompute);
    edited
        .model
        .product_definitions
        .iter_mut()
        .find(|definition| {
            definition
                .id
                .as_str()
                .ends_with("#DesignedFeature:definition")
        })
        .unwrap()
        .label = Some("AuthoredProductFeature".into());
    let (written, report) = write_decode(&FcstdCodec, &FcstdCodec, &edited, None, None);
    let after = written.ir();
    let feature = after
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("AuthoredProductFeature"))
        .unwrap();
    let parameter = after
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "Length"
        })
        .unwrap();
    assert!(
        (after
            .dimension_value(&Dimension::<Length>::Parameter(parameter.id.clone()))
            .unwrap()
            .get()
            - 37.0)
            .abs()
            <= EPS_PARAMETER_VALUE
    );
    assert!(feature.needs_recompute);
    assert_eq!(counts(after), counts(&edited));
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &edited, after);
}
