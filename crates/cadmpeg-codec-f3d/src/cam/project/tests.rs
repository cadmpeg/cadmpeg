// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::cam::records::Manufacturing;
use crate::test_support::with_decode_context;
use cadmpeg_core::decode::View;
use serde_json::Value;

const SETUP: &str = r#"<object id="job" strategy="setup"><Parameters><Parameter name="job_stockMode" value="default" expression="'default'"/><Parameter name="job_stockOffsetMode" value="simple" expression="'simple'"/><Parameter name="job_stockOffsetSides" value="2" expression="2mm"/><Parameter name="job_stockOffsetTop" value="3" expression="3mm"/><Parameter name="job_stockOffsetBottom" value="4" expression="4mm"/><Parameter name="wcs_orientation_mode" value="modelOrientation" expression="'modelOrientation'"/><Parameter name="wcs_origin_mode" value="stockPoint" expression="'stockPoint'"/></Parameters></object>"#;

fn decode(text: &str) -> (CadIr, Vec<LossNote>) {
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    with_decode_context(|ctx| {
        let mut cam = Manufacturing::default();
        crate::cam::parse(
            ctx,
            View::over_retained(text.as_bytes()),
            "Toolpath/Iron.Document/a.irondoc",
            &mut cam,
            &mut losses,
        )
        .unwrap();
        admit(ctx, &mut ir, &cam, &mut losses).unwrap();
    });
    (ir, losses)
}

fn json<T: serde::Serialize>(value: &T) -> Value {
    cadmpeg_core::serde_erase::json::to_value(value).unwrap()
}

/// The id and cached number of the process parameter `name` of `owner`.
fn parameter(ir: &CadIr, owner: &str, name: &str) -> (String, f64) {
    json(&ir.process.parameters)
        .as_array()
        .unwrap()
        .iter()
        .find(|parameter| parameter["name"] == name && parameter["owner"]["kind"] == owner)
        .map(|parameter| {
            (
                parameter["id"].as_str().unwrap().to_owned(),
                parameter["value"]["value"].as_f64().unwrap_or(f64::NAN),
            )
        })
        .unwrap()
}

fn bound(id: &str) -> Value {
    cadmpeg_core::json!({"kind":"parameter", "value":{"kind":"process", "value":id}})
}

fn has_loss(losses: &[LossNote], code: F3dLossCode) -> bool {
    losses.iter().any(|loss| loss.code == code.kind())
}

#[test]
fn setup_choices_name_the_parameters_they_are_read_from() {
    let (ir, losses) = decode(&format!(
        r#"<document>{SETUP}<object parent="job" strategy="face"/></document>"#
    ));
    let setup = json(&ir.process.setups[0]);
    let id = |name| parameter(&ir, "setup", name).0;
    assert_eq!(
        setup["stock"]["parameters"],
        cadmpeg_core::json!([id("job_stockMode"), id("job_stockOffsetMode")])
    );
    assert_eq!(setup["stock"]["value"]["kind"], "relative");
    let sides = bound(&id("job_stockOffsetSides"));
    let value = &setup["stock"]["value"]["value"];
    assert_eq!(value["low"][0], sides);
    assert_eq!(value["low"][1], sides);
    assert_eq!(value["high"][0], sides);
    assert_eq!(value["high"][1], sides);
    assert_eq!(value["low"][2], bound(&id("job_stockOffsetBottom")));
    assert_eq!(value["high"][2], bound(&id("job_stockOffsetTop")));
    assert_eq!(
        setup["wcs"]["orientation"]["parameters"],
        cadmpeg_core::json!([id("wcs_orientation_mode")])
    );
    assert_eq!(setup["wcs"]["orientation"]["value"]["kind"], "model");
    assert_eq!(
        setup["wcs"]["origin"]["parameters"],
        cadmpeg_core::json!([id("wcs_origin_mode")])
    );
    assert!(!has_loss(&losses, F3dLossCode::CamStockOffsetModeUnread));
    assert_eq!(parameter(&ir, "setup", "job_stockOffsetSides").1, 2.0);
}

#[test]
fn stock_offsets_apply_only_in_the_simple_offset_mode() {
    let (ir, losses) = decode(
        r#"<document><object id="job" strategy="setup"><Parameters><Parameter name="job_stockMode" value="default"/><Parameter name="job_stockOffsetMode" value="absolute"/><Parameter name="job_stockOffsetSides" value="2" expression="2mm"/></Parameters></object></document>"#,
    );
    let setup = json(&ir.process.setups[0]);
    assert_eq!(
        setup["stock"]["value"],
        cadmpeg_core::json!({"kind":"native","value":"default"})
    );
    assert!(has_loss(&losses, F3dLossCode::CamStockOffsetModeUnread));
}

#[test]
fn unstated_stock_modes_keep_their_source_name() {
    let (ir, _) = decode(
        r#"<document><object id="job" strategy="setup"><Parameters><Parameter name="job_stockMode" value="fixedbox"/></Parameters></object></document>"#,
    );
    assert_eq!(
        json(&ir.process.setups[0])["stock"]["value"],
        cadmpeg_core::json!({"kind":"native","value":"fixedbox"})
    );
}

fn operation_with(parameters: &str) -> (CadIr, Value, Vec<LossNote>) {
    let (ir, losses) = decode(&format!(
        r#"<document><object id="job" strategy="setup"/><object id="op" parent="job" strategy="face"><Parameters>{parameters}</Parameters></object></document>"#
    ));
    let operation = json(&ir.process.operations[0]);
    (ir, operation, losses)
}

#[test]
fn technology_binds_parameters_of_the_stated_units() {
    let (ir, operation, losses) = operation_with(
        r#"<Parameter name="tool_feedCutting" value="1200" expression="1200mmpm"/><Parameter name="tool_spindleSpeed" value="9000" expression="9000rpm"/><Parameter name="stepover" value="4.5" expression="4.5mm"/>"#,
    );
    let technology = &operation["technology"];
    let (cutting, value) = parameter(&ir, "operation", "tool_feedCutting");
    assert!((value - 1200.0 / 60.0).abs() < 1e-9);
    let feeds = technology["feeds"].as_array().unwrap();
    assert_eq!(feeds.len(), 1);
    assert_eq!(feeds[0][0]["kind"], "cutting");
    assert_eq!(
        feeds[0][1],
        cadmpeg_core::json!({"kind":"per_time","value":bound(&cutting)})
    );
    let (speed, value) = parameter(&ir, "operation", "tool_spindleSpeed");
    assert!((value - 9000.0 * std::f64::consts::TAU / 60.0).abs() < 1e-9);
    assert_eq!(
        technology["spindle"],
        cadmpeg_core::json!({"kind":"rotation","value":bound(&speed)})
    );
    let (stepover, value) = parameter(&ir, "operation", "stepover");
    assert!((value - 4.5).abs() < 1e-9);
    assert_eq!(technology["stepover"], bound(&stepover));
    assert!(!has_loss(&losses, F3dLossCode::CamParameterUnitUnread));
}

#[test]
fn technology_values_are_canonical_whatever_unit_the_expression_types() {
    let (ir, operation, losses) = operation_with(
        r#"<Parameter name="tool_feedCutting" value="1000" expression="100cmpm"/><Parameter name="tool_feedPlunge" value="999.99999999600004" expression="39.37007874inpm"/><Parameter name="stepover" value="4.5" expression="0.45cm"/><Parameter name="tolerance" value="0.1" expression="0.1m"/>"#,
    );
    let technology = &operation["technology"];
    assert_eq!(technology["feeds"].as_array().unwrap().len(), 2);
    let (_, cutting) = parameter(&ir, "operation", "tool_feedCutting");
    assert!((cutting - 1000.0 / 60.0).abs() < 1e-9);
    let (_, plunge) = parameter(&ir, "operation", "tool_feedPlunge");
    assert!((plunge - 1000.0 / 60.0).abs() < 1e-6);
    let (stepover, value) = parameter(&ir, "operation", "stepover");
    assert!((value - 4.5).abs() < 1e-9);
    assert_eq!(technology["stepover"], bound(&stepover));
    assert!(technology.get("tolerance").is_none());
    assert!(has_loss(&losses, F3dLossCode::CamParameterUnitUnread));
}

#[test]
fn missing_setup_omits_operation_and_keeps_tool() {
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    with_decode_context(|ctx| {
        let mut cam = Manufacturing::default();
        crate::cam::parse(
            ctx,
            View::over_retained(
                br#"<document><tool id="t"/><object id="job" strategy="setup"/><object parent="job" strategy="face" toolId="t"/></document>"#,
            ),
            "Toolpath/Iron.Document/a.irondoc",
            &mut cam,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(cam.operations.len(), 1);
        cam.setups.clear();
        admit(ctx, &mut ir, &cam, &mut losses).unwrap();
    });
    assert!(has_loss(&losses, F3dLossCode::CamOperationUnreadable));
    assert!(ir.process.setups.is_empty());
    assert!(ir.process.operations.is_empty());
    assert_eq!(ir.process.tools.len(), 1);
    assert_eq!(ir.process.parameters.len(), 1);
    assert_eq!(ir.process.parameters[0].name.as_str(), "tool.id");
    assert!(matches!(&ir.process.parameters[0].owner,
        cadmpeg_ir::process::groups::ProcessOwner::Tool(id) if *id == ir.process.tools[0].id));
}

#[test]
fn projection_without_a_document_definition_is_malformed() {
    let mut ir = CadIr::empty();
    with_decode_context(|ctx| {
        let error = project(ctx, &mut ir, &Manufacturing::default(), &mut Vec::new()).unwrap_err();
        assert!(matches!(error, CodecError::Malformed(_)));
    });
}

#[test]
fn cam_unresolved_setup_scope_keeps_document_and_parameters() {
    let (mut ir, _) = decode(&format!("<document>{SETUP}</document>"));
    let parameters = ir.process.parameters.clone();
    let definitions = ir.model.product_definitions.clone();
    ir.process.setups[0].scope =
        cadmpeg_ir::ids::ProductDefinitionId::mint("f3d:model:product_definition#missing").unwrap();
    let mut losses = Vec::new();
    with_decode_context(|ctx| ensure_scopes(ctx, &mut ir, &mut losses).unwrap());
    assert_eq!(ir.model.product_definitions, definitions);
    assert_eq!(ir.process.parameters, parameters);
    assert_eq!(ir.process.setups.len(), 1);
    assert_eq!(ir.process.setups[0].scope, definitions[0].id);
    assert!(has_loss(&losses, F3dLossCode::CamSetupScopeUnresolved));
    ir.model.product_definitions.clear();
    ir.design.scopes.clear();
    with_decode_context(|ctx| ensure_scopes(ctx, &mut ir, &mut losses).unwrap());
    assert!(ir.process.setups.is_empty());
    assert!(ir.process.parameters.is_empty());
}

#[test]
fn cam_missing_operation_setup_keeps_other_records() {
    with_decode_context(|ctx| {
        let mut cam = Manufacturing::default();
        let mut losses = Vec::new();
        crate::cam::parse(
            ctx,
            View::over_retained(
                b"<document><object parent=\"missing\" strategy=\"face\"/></document>",
            ),
            "synthetic",
            &mut cam,
            &mut losses,
        )
        .unwrap();
        assert_eq!(cam.operations.len(), 1);
        cam.setups.clear();
        let mut ir = CadIr::empty();
        crate::tree::state_root_definition(ctx, &mut ir).unwrap();
        project(ctx, &mut ir, &cam, &mut losses).unwrap();
        assert!(ir.process.operations.is_empty());
        assert!(!ir.model.product_definitions.is_empty());
        assert!(has_loss(&losses, F3dLossCode::CamOperationUnreadable));
    });
}

#[test]
fn tool_canonical_dimensions_match_parameters_in_an_inch_table() {
    let (ir, losses) = decode(
        r#"<document><tool id="t" unit="inches" type="flat end mill"><Parameters><Parameter name="tool_diameter" value="6" expression="6mm"/></Parameters><body diameter="0.2362204724409449" flute-length="1"/></tool></document>"#,
    );
    let cutter = &json(&ir.process.tools[0])["cutter"];
    let (_, diameter) = parameter(&ir, "tool", "tool_diameter");
    assert_eq!(diameter, 6.0);
    assert_eq!(
        cutter["diameter"],
        cadmpeg_core::json!({"kind":"literal","value":diameter})
    );
    assert_eq!(
        cutter["flute_length"],
        cadmpeg_core::json!({"kind":"literal","value":25.4})
    );
    assert!(!has_loss(&losses, F3dLossCode::CamParameterUnitUnread));
}

#[test]
fn descriptor_sibling_orders_bind_toolpath_feed() {
    let frame = r#"<toolpathToolView><Parameters><Parameter name="stepover" value="2" expression="2mm"/></Parameters></toolpathToolView>"#;
    let path = r#"<toolpath valid="true"><Parameters><Parameter name="tool_feedCutting" value="1200" expression="1200mmpm"/></Parameters></toolpath>"#;
    for descriptors in [format!("{frame}{path}"), format!("{path}{frame}")] {
        let (ir, losses) = decode(&format!(
            r#"<document><object id="job" strategy="setup"/><object parent="job" strategy="face">{descriptors}</object></document>"#
        ));
        let operation = json(&ir.process.operations[0]);
        let (feed, value) = parameter(&ir, "operation", "tool_feedCutting");
        assert_eq!(value, 20.0);
        assert_eq!(
            operation["technology"]["feeds"][0][1],
            cadmpeg_core::json!({"kind":"per_time","value":bound(&feed)})
        );
        let (stepover, _) = parameter(&ir, "operation", "stepover");
        assert_eq!(operation["technology"]["stepover"], bound(&stepover));
        assert_eq!(ir.process.parameters.len(), 2);
        assert!(!has_loss(&losses, F3dLossCode::CamParameterUnitUnread));
    }
}

#[test]
fn exponent_literals_keep_canonical_quantities_and_bindings() {
    for exponent in ["e+3", "e-3"] {
        for (name, suffixes, scale) in [
            ("stepover", &["mm", "cm", "in"][..], 1.0),
            (
                "tool_feedCutting",
                &["mmpm", "cmpm", "inpm"][..],
                1.0 / 60.0,
            ),
            (
                "tool_spindleSpeed",
                &["rpm"][..],
                std::f64::consts::TAU / 60.0,
            ),
        ] {
            for suffix in suffixes {
                for mantissa in ["1.5", "1,5"] {
                    let (ir, operation, losses) = operation_with(&format!(
                        r#"<Parameter name="{name}" value="2" expression="{mantissa}{exponent}{suffix}"/>"#
                    ));
                    let (id, value) = parameter(&ir, "operation", name);
                    assert_eq!(value, 2.0 * scale);
                    let technology = &operation["technology"];
                    match name {
                        "stepover" => assert_eq!(technology["stepover"], bound(&id)),
                        "tool_feedCutting" => assert_eq!(
                            technology["feeds"][0][1],
                            cadmpeg_core::json!({"kind":"per_time","value":bound(&id)})
                        ),
                        _ => assert_eq!(
                            technology["spindle"],
                            cadmpeg_core::json!({"kind":"rotation","value":bound(&id)})
                        ),
                    }
                    assert!(!has_loss(&losses, F3dLossCode::CamParameterUnitUnread));
                }
            }
        }
    }
}
