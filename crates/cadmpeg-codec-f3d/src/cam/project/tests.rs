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
    serde_json::to_value(value).unwrap()
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
    serde_json::json!({"kind":"parameter", "value":{"kind":"process", "value":id}})
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
        serde_json::json!([id("job_stockMode"), id("job_stockOffsetMode")])
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
        serde_json::json!([id("wcs_orientation_mode")])
    );
    assert_eq!(setup["wcs"]["orientation"]["value"]["kind"], "model");
    assert_eq!(
        setup["wcs"]["origin"]["parameters"],
        serde_json::json!([id("wcs_origin_mode")])
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
        serde_json::json!({"kind":"native","value":"default"})
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
        serde_json::json!({"kind":"native","value":"fixedbox"})
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
        serde_json::json!({"kind":"per_time","value":bound(&cutting)})
    );
    let (speed, value) = parameter(&ir, "operation", "tool_spindleSpeed");
    assert!((value - 9000.0 * std::f64::consts::TAU / 60.0).abs() < 1e-9);
    assert_eq!(
        technology["spindle"],
        serde_json::json!({"kind":"rotation","value":bound(&speed)})
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
fn definitions_that_do_not_project_are_a_loss_and_leave_no_process_records() {
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
        cam.setups.clear();
        admit(ctx, &mut ir, &cam, &mut losses).unwrap();
    });
    assert!(has_loss(&losses, F3dLossCode::CamDocumentUnreadable));
    assert!(ir.process.setups.is_empty());
    assert!(ir.process.operations.is_empty());
    assert!(ir.process.tools.is_empty());
    assert!(ir.process.parameters.is_empty());
}

#[test]
fn projection_without_a_document_definition_is_malformed() {
    let mut ir = CadIr::empty();
    with_decode_context(|ctx| {
        let error = project(ctx, &mut ir, &Manufacturing::default(), &mut Vec::new()).unwrap_err();
        assert!(matches!(error, CodecError::Malformed(_)));
    });
}
