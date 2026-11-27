// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::{with_decode_context, with_decode_policy};
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::report::loss::LossNote;
use std::fmt::Write as _;

#[test]
fn cam_document_member_requires_a_workspace_and_document_directory() {
    assert!(document_member("Toolpath/Iron.Document/a.irondoc"));
    assert!(document_member(
        "part/Toolpath[synthetic]/Iron.Document/a.irondoc"
    ));
    assert!(!document_member("ToolpathOther/Iron.Document/a.irondoc"));
    assert!(!document_member("Toolpath[synthetic]/Iron.Other/a.irondoc"));
    assert!(!document_member("Toolpath[synthetic]/Iron.Document/a.bin"));
}

#[test]
fn cam_cyclic_object_parent_keeps_operation_with_assumed_setup() {
    let text = br#"<document><object id="cut" parent="folder" strategy="face"/><object id="folder" parent="cut" strategy="folder"/></document>"#;
    with_decode_context(|ctx| {
        let mut target = Manufacturing::default();
        let mut losses = Vec::new();
        parse(
            ctx,
            View::over_retained(text),
            "synthetic",
            &mut target,
            &mut losses,
        )
        .unwrap();
        assert_eq!(target.operations.len(), 1);
        assert_eq!(target.setups.len(), 1);
        assert_eq!(target.operations[0].setup, target.setups[0].id);
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::CamSetupAssumed.kind()));
    });
}

const TOOL: &str = r#"<tool guid="g" id="1" type="flat end mill" unit="millimeters" version="1.5"><Parameters><Parameter name="tool_number" value="1" expression="1"/><Parameter name="tool_diameter" value="6" expression="6."/><Parameter name="tool_vendor" value="Example" expression="'Example'"/></Parameters></tool>"#;

const SETUP: &str = r#"<object id="job" displayName="Setup1" strategy="setup"><Parameters><Parameter name="job_stockMode" value="default" expression="'default'"/><Parameter name="job_stockOffsetMode" value="simple" expression="'simple'"/><Parameter name="job_stockOffsetSides" value="2" expression="2mm"/><Parameter name="job_stockOffsetTop" value="3" expression="3mm"/><Parameter name="job_stockOffsetBottom" value="4" expression="4mm"/><Parameter name="wcs_orientation_mode" value="modelOrientation" expression="'modelOrientation'"/><Parameter name="wcs_origin_mode" value="stockPoint" expression="'stockPoint'"/><Parameter name="wcs_origin_boxPoint" value="top center" expression="'top center'"/></Parameters><CadRef id="model-1"/></object>"#;

#[test]
fn cam_object_tree_yields_setups_operations_and_tools() {
    let text = format!(
        r#"<document><model><docObjects>{TOOL}{SETUP}<object id="grp" parent="job" strategy="folder"/><object id="op1" parent="grp" strategy="face" toolId="1" valid="true"><Parameters><Parameter name="tool_feedCutting" value="1000" expression="1000mmpm"/><Parameter name="stepover" value="4.5" expression="4.5mm"/></Parameters><CadContours2D><CadChain><CadRef id="edge-1"/></CadChain></CadContours2D></object><object id="op2" parent="job" strategy="custom" toolId="1" valid="false"/></docObjects></model></document>"#
    );
    let (target, losses) = parse_text(&text);
    assert_eq!(target.setups.len(), 1);
    assert_eq!(target.tools.len(), 1);
    assert_eq!(target.operations.len(), 2);
    let setup = &target.setups[0];
    assert_eq!(setup.name.as_deref(), Some("Setup1"));
    assert_eq!(setup.stock.kind.as_deref(), Some("default"));
    assert_eq!(setup.wcs.orientation[0].value, "modelOrientation");
    assert_eq!(setup.wcs.origin_mode.as_deref(), Some("stockPoint"));
    assert_eq!(setup.stock.offsets.len(), 4);
    assert_eq!(setup.stock.offsets[1].value, "2");
    assert_eq!(setup.stock.offsets[1].unit.as_deref(), Some("mm"));
    let face = &target.operations[0];
    assert_eq!(face.setup, setup.id);
    assert_eq!(face.family, ManufacturingStrategy::Face);
    assert_eq!(face.order, 0);
    assert_eq!(face.tool.as_ref(), Some(&target.tools[0].id));
    assert_eq!(face.computed_path_valid, Some(true));
    assert_eq!(face.parameters[0].unit.as_deref(), Some("mm/min"));
    assert_eq!(face.parameters[1].unit.as_deref(), Some("mm"));
    assert_eq!(
        face.geometry[0].native_xml,
        "<CadContours2D><CadChain><CadRef id=\"edge-1\"/></CadChain></CadContours2D>"
    );
    let custom = &target.operations[1];
    assert_eq!(custom.family, ManufacturingStrategy::Other);
    assert_eq!(custom.order, 1);
    assert_eq!(custom.computed_path_valid, Some(false));
    assert_eq!(target.tools[0].vendor.as_deref(), Some("Example"));
    assert_eq!(target.tools[0].number.as_deref(), Some("1"));
    assert_eq!(
        &text[usize::try_from(face.source.start).unwrap()
            ..usize::try_from(face.source.end).unwrap()],
        face.source.native_xml
    );
    assert!(has_loss(&losses, F3dLossCode::CamSelectionUnresolved));
}

#[test]
fn cam_bad_operation_keeps_following_operation() {
    let (target, losses) = parse_text(
        r#"<document><object id="job" strategy="setup"/><object parent="job" strategy="face" valid="bad"/><object parent="job" strategy="face" valid="true"/></document>"#,
    );
    assert_eq!(target.operations.len(), 2);
    assert_eq!(target.operations[0].computed_path_valid, None);
    assert_eq!(target.operations[1].computed_path_valid, Some(true));
    assert!(has_loss(&losses, F3dLossCode::CamOperationUnreadable));
}

#[test]
fn cam_xml_budget_and_depth_are_enforced() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 32;
    with_decode_policy(&policy, |ctx| {
        let text = b"<document><model><object strategy='setup'/></model></document>";
        let error = parse(
            ctx,
            View::over_retained(text),
            "synthetic",
            &mut Manufacturing::default(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(error, CodecError::NotImplemented(_)));
    });
    policy.limits.max_decompressed_bytes_per_expand = 1024 * 1024;
    policy.limits.max_recursion_depth = 2;
    with_decode_policy(&policy, |ctx| {
        let error = parse(
            ctx,
            View::over_retained(b"<document><model><object/></model></document>"),
            "synthetic",
            &mut Manufacturing::default(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    });
}

#[test]
fn cam_bad_setup_and_tool_keep_native_values_and_orphan_operation() {
    let (target, losses) = parse_text(
        r#"<document><object id="job" strategy="setup"><Parameters><Parameter value="kept"/></Parameters></object><tool id="t"><Parameters><Parameter value="kept"/></Parameters></tool><object id="o" parent="missing" strategy="custom" toolId="missing"><Parameters><Parameter name="custom" value="kept"/></Parameters></object></document>"#,
    );
    assert_eq!(target.setups.len(), 2);
    assert_eq!(
        target.setups[1].source.exactness,
        cadmpeg_ir::Exactness::Inferred
    );
    assert!(has_loss(&losses, F3dLossCode::CamSetupAssumed));
    assert_eq!(target.setups[0].parameters[0].value, "kept");
    assert_eq!(target.tools[0].parameters[0].value, "kept");
    assert_eq!(target.operations.len(), 1);
    assert_eq!(target.operations[0].strategy, "custom");
    assert_eq!(target.operations[0].family, ManufacturingStrategy::Other);
    assert_eq!(target.operations[0].tool, None);
    assert!(has_loss(&losses, F3dLossCode::CamSetupUnreadable));
    assert!(has_loss(&losses, F3dLossCode::CamToolUnreadable));
}

#[test]
fn cam_xml_element_budget_is_enforced() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 2;
    with_decode_policy(&policy, |ctx| {
        let error = parse(
            ctx,
            View::over_retained(b"<document><model><object strategy='setup'/></model></document>"),
            "synthetic",
            &mut Manufacturing::default(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    });
}

#[test]
fn cam_parameter_text_keeps_xml_spelling() {
    let (target, _) = parse_text(
        r#"<document><object id="job" strategy="setup"><Parameters><Parameter name="job_comment" value="a &amp; b" expression="'a &amp; b'"/><Parameter name="job_stockOffsetTop" value="12" expression="12mm"/></Parameters></object></document>"#,
    );
    let parameters = &target.setups[0].parameters;
    assert_eq!(parameters[0].value, "a & b");
    assert!(parameters[0].native_xml.contains("a &amp; b"));
    assert_eq!(parameters[0].unit, None);
    assert_eq!(parameters[1].value, "12");
    assert_eq!(parameters[1].unit.as_deref(), Some("mm"));
}

#[test]
fn cam_tool_id_selects_the_tool_record() {
    let (target, _) = parse_text(
        r#"<document><tool id="3"><Parameters><Parameter name="tool_number" value="3"/></Parameters></tool><object id="job" strategy="setup"/><object parent="job" strategy="face" toolId="3"/></document>"#,
    );
    assert_eq!(
        target.operations[0].tool.as_ref(),
        Some(&target.tools[0].id)
    );
    assert_eq!(target.tools[0].number.as_deref(), Some("3"));
}

#[test]
fn cam_tool_number_reference_has_a_named_assumption() {
    let (target, losses) = parse_text(
        r#"<document><tool id="x"><Parameters><Parameter name="tool_number" value="3"/></Parameters></tool><object id="job" strategy="setup"/><object parent="job" strategy="face" toolId=""><Parameters><Parameter name="tool_number" value="3"/></Parameters></object></document>"#,
    );
    assert_eq!(
        target.operations[0].tool.as_ref(),
        Some(&target.tools[0].id)
    );
    assert!(has_loss(
        &losses,
        F3dLossCode::CamToolNumberReferenceAssumed
    ));
}

fn parse_text(text: &str) -> (Manufacturing, Vec<LossNote>) {
    let mut target = Manufacturing::default();
    let mut losses = Vec::new();
    with_decode_context(|ctx| {
        parse(
            ctx,
            View::over_retained(text.as_bytes()),
            "synthetic",
            &mut target,
            &mut losses,
        )
        .unwrap();
    });
    (target, losses)
}

fn has_loss(losses: &[LossNote], code: F3dLossCode) -> bool {
    losses.iter().any(|loss| loss.code == code.kind())
}

#[test]
fn cam_toolpath_of_an_operation_object_is_not_an_operation() {
    let (target, losses) = parse_text(
        r#"<document><object id="job" strategy="setup"/><object id="o" parent="job" strategy="face"><toolpath valid="true"/></object></document>"#,
    );
    assert_eq!(target.setups.len(), 1);
    assert_eq!(target.operations.len(), 1);
    assert_eq!(target.operations[0].setup, target.setups[0].id);
    assert_eq!(target.operations[0].computed_path_valid, Some(true));
    assert!(!has_loss(&losses, F3dLossCode::CamSetupAssumed));
}

#[test]
fn cam_long_parent_chain_with_many_operations_completes_under_a_small_budget() {
    let groups = 4000;
    let mut text = String::from(r#"<document><object id="job" strategy="setup"/>"#);
    for group in 0..groups {
        let parent = if group == 0 {
            "job".to_owned()
        } else {
            format!("g{}", group - 1)
        };
        write!(
            text,
            r#"<object id="g{group}" parent="{parent}" strategy="folder"/>"#
        )
        .unwrap();
    }
    let operations = 4000;
    for _ in 0..operations {
        write!(
            text,
            r#"<object parent="g{}" strategy="face"><toolpath valid="true"/></object>"#,
            groups - 1
        )
        .unwrap();
    }
    text.push_str("</document>");
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 4 * u64::try_from(groups + operations).unwrap() + 16;
    let started = std::time::Instant::now();
    with_decode_policy(&policy, |ctx| {
        let mut target = Manufacturing::default();
        parse(
            ctx,
            View::over_retained(text.as_bytes()),
            "synthetic",
            &mut target,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(target.setups.len(), 1);
        assert_eq!(target.operations.len(), operations);
        assert!(target
            .operations
            .iter()
            .all(|operation| operation.setup == target.setups[0].id));
    });
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

#[test]
fn cam_oversized_document_is_a_loss_and_decoding_continues() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 32;
    with_decode_policy(&policy, |ctx| {
        let mut target = Manufacturing::default();
        let mut losses = Vec::new();
        read_entry(
            ctx,
            View::over_retained(
                b"<document><object id='job' strategy='setup'/><object parent='job' strategy='face'/></document>",
            ),
            "Toolpath/Iron.Document/a.irondoc",
            &mut target,
            &mut losses,
        )
        .unwrap();
        assert!(target.setups.is_empty());
        assert!(has_loss(&losses, F3dLossCode::CamDocumentUnreadable));
    });
}

#[test]
fn cam_empty_tool_id_is_no_reference() {
    let (target, losses) = parse_text(
        r#"<document><tool id="t"/><object id="job" strategy="setup"/><object parent="job" strategy="face" toolId=""/></document>"#,
    );
    assert_eq!(target.operations[0].tool, None);
    assert!(!has_loss(&losses, F3dLossCode::CamToolUnreadable));
}

#[test]
fn cam_unstated_record_forms_stay_native_with_a_loss() {
    let (target, losses) = parse_text(
        r#"<document><section type="setup" name="S"><section strategy="face"><toolpath valid="true"/></section></section><setup/><job/><operation strategy="face"/></document>"#,
    );
    assert!(target.setups.is_empty());
    assert!(target.operations.is_empty());
    assert!(has_loss(&losses, F3dLossCode::CamRecordFormUnread));
}

#[test]
fn cam_unstated_strategy_name_stays_native_with_a_loss() {
    let (target, losses) = parse_text(
        r#"<document><object id="job" strategy="setup"/><object parent="job" strategy="adaptive2d"/><object parent="job" strategy="face"/></document>"#,
    );
    assert_eq!(target.operations[0].strategy, "adaptive2d");
    assert_eq!(target.operations[0].family, ManufacturingStrategy::Other);
    assert_eq!(target.operations[1].family, ManufacturingStrategy::Face);
    assert!(has_loss(&losses, F3dLossCode::CamRecordFormUnread));
}

#[test]
fn cam_unstated_unit_suffix_names_no_unit_and_is_a_loss() {
    let (target, losses) = parse_text(
        r#"<document><object id="job" strategy="setup"><Parameters><Parameter name="job_stockOffsetTop" value="1" expression="1in"/><Parameter name="job_stockOffsetSides" value="2" expression="2mm"/></Parameters></object></document>"#,
    );
    let parameters = &target.setups[0].parameters;
    assert_eq!(parameters[0].unit, None);
    assert_eq!(parameters[1].unit.as_deref(), Some("mm"));
    assert!(has_loss(&losses, F3dLossCode::CamParameterUnitUnread));
}

#[test]
fn cam_tool_parameters_are_the_tools_own_not_every_descendant_attribute() {
    let (target, _) = parse_text(
        r#"<document><tool id="t" type="flat end mill" unit="millimeters"><nc number="1"/><body diameter="6"/><Parameters><Parameter name="a" value="1" unit="mm"/><Parameter name="b" value="2"/><Parameter name="c" value="3"/></Parameters></tool></document>"#,
    );
    let parameters = &target.tools[0].parameters;
    let named: Vec<_> = parameters
        .iter()
        .filter(|parameter| !parameter.name.contains('.'))
        .map(|parameter| parameter.name.as_str())
        .collect();
    assert_eq!(named, ["a", "b", "c"]);
    assert!(parameters
        .iter()
        .all(|parameter| !parameter.name.starts_with("Parameter.")));
    let dotted: Vec<_> = parameters
        .iter()
        .filter(|parameter| parameter.name.contains('.'))
        .map(|parameter| (parameter.name.as_str(), parameter.value.as_str()))
        .collect();
    assert_eq!(
        dotted,
        [
            ("tool.id", "t"),
            ("tool.type", "flat end mill"),
            ("tool.unit", "millimeters"),
            ("nc.number", "1"),
            ("body.diameter", "6")
        ]
    );
    assert!(parameters
        .iter()
        .filter(|parameter| parameter.name.contains('.'))
        .all(|parameter| parameter.native_xml.len() < 60));
}
