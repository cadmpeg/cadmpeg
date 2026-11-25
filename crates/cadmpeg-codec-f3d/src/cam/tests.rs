// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::{with_decode_context, with_decode_policy};
use cadmpeg_core::decode::DecodePolicy;

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

#[test]
fn cam_preserves_parameters_and_all_strategy_families() {
    let text = r#"<document><model>
      <tool id="t1"><Parameters><Parameter name="tool_number" value="1"/><Parameter name="tool_diameter" value="6.00" unit="mm" type="float"/><Parameter name="tool_vendor" value="Example"/></Parameters><holder><Parameters><Parameter name="holder_diameter" value="12" unit="mm"/></Parameters></holder></tool>
      <tool id="t2" number="2" type="drill"/><tool id="t3" number="3" type="ball"/>
      <section type="setup" name="First"><Parameters><Parameter name="job_stockMode" value="fixedbox"/><Parameter name="job_stockSizeX" value="100" unit="mm"/><Parameter name="wcs_orientation_mode" value="axes"/><Parameter name="wcs_origin_mode" value="point"/><Parameter name="wcs_origin_x" value="5" unit="mm"/></Parameters><CadRef id="model-1"/>
        <section strategy="adaptive2d" tool="t1"><Parameters><Parameter name="cuttingFeedrate" value="1200.0" unit="mm/min" type="double"/><Parameter name="stepover" value="tool_diameter * 0.4"/></Parameters><CadContours2D><CadChain><CadRef id="edge-1"/></CadChain></CadContours2D><toolpath valid="true"/></section>
        <section strategy="pocket2d" tool="t1"><toolpath valid="false"/></section>
        <section strategy="contour2d" tool="t1"><toolpath valid="1"/></section>
        <section strategy="drill" tool="t2"><toolpath valid="0"/></section>
      </section>
      <section type="setup" name="Second"><Parameters><Parameter name="job_stockMode" value="relativebox"/><Parameter name="job_stockOffset" value="2" unit="mm"/></Parameters>
        <section strategy="adaptive" tool="t3"><toolpath valid="true"/></section>
        <section strategy="parallel" tool="t3"><toolpath/></section>
        <section strategy="scallop" tool="t3"><toolpath/></section>
      </section>
    </model></document>"#;
    with_decode_context(|ctx| {
        let mut target = Manufacturing::default();
        let mut losses = Vec::new();
        parse(
            ctx,
            View::over_retained(text.as_bytes()),
            "Toolpath/Iron.Document/synthetic.irondoc",
            &mut target,
            &mut losses,
        )
        .unwrap();
        assert_eq!(target.setups.len(), 2);
        assert_eq!(target.tools.len(), 3);
        assert_eq!(target.operations.len(), 7);
        assert_eq!(
            target
                .operations
                .iter()
                .map(|op| op.family)
                .collect::<Vec<_>>(),
            [
                ManufacturingStrategy::Adaptive2d,
                ManufacturingStrategy::Pocket2d,
                ManufacturingStrategy::Contour2d,
                ManufacturingStrategy::Drilling,
                ManufacturingStrategy::Adaptive3d,
                ManufacturingStrategy::Parallel,
                ManufacturingStrategy::Scallop
            ]
        );
        let setup = &target.setups[0];
        assert_eq!(setup.stock.kind.as_deref(), Some("fixedbox"));
        assert_eq!(setup.stock.parameters[1].value, "100");
        assert_eq!(setup.stock.parameters[1].unit.as_deref(), Some("mm"));
        assert_eq!(setup.wcs.orientation[0].value, "axes");
        assert_eq!(setup.wcs.origin_mode.as_deref(), Some("point"));
        assert_eq!(setup.wcs.origin[0].value, "5");
        assert_eq!(setup.stock.dimensions[0].value, "100");
        assert_eq!(target.setups[1].stock.offsets[0].value, "2");
        assert_eq!(setup.wcs.parameters[2].value, "5");
        let op = &target.operations[0];
        assert_eq!(op.setup, setup.id);
        assert_eq!(op.order, 0);
        assert_eq!(target.operations[3].order, 3);
        assert_eq!(target.operations[4].order, 0);
        assert_eq!(op.tool.as_ref(), Some(&target.tools[0].id));
        assert_eq!(op.parameters[0].value, "1200.0");
        assert_eq!(op.parameters[0].kind.as_deref(), Some("double"));
        assert_eq!(op.parameters[1].value, "tool_diameter * 0.4");
        assert_eq!(
            op.geometry[0].native_xml,
            "<CadContours2D><CadChain><CadRef id=\"edge-1\"/></CadChain></CadContours2D>"
        );
        assert!(op.computed_path_present);
        assert_eq!(op.computed_path_valid, Some(true));
        assert_eq!(target.operations[1].computed_path_valid, Some(false));
        assert_eq!(target.tools[0].holder[0].name, "holder_diameter");
        assert_eq!(target.tools[0].vendor.as_deref(), Some("Example"));
        assert_eq!(
            &text[usize::try_from(op.source.start).unwrap()
                ..usize::try_from(op.source.end).unwrap()],
            op.source.native_xml
        );
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::CamSelectionUnresolved.kind()));
    });
}

#[test]
fn cam_bad_operation_keeps_following_operation() {
    let text = br#"<document><section type="setup"><section><toolpath valid="bad"/></section><section strategy="parallel"><toolpath valid="true"/></section></section></document>"#;
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
        assert_eq!(target.operations.len(), 2);
        assert_eq!(target.operations[0].family, ManufacturingStrategy::Other);
        assert_eq!(target.operations[0].computed_path_valid, None);
        assert_eq!(target.operations[1].family, ManufacturingStrategy::Parallel);
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::CamOperationUnreadable.kind()));
    });
}

#[test]
fn cam_xml_budget_and_depth_are_enforced() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 32;
    with_decode_policy(&policy, |ctx| {
        let text = b"<document><model><section type='setup'/></model></document>";
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
            View::over_retained(b"<document><model><section/></model></document>"),
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
    let text = br#"<document><model><section type="setup"><Parameters><Parameter value="kept"/></Parameters></section><tool id="t"><Parameters><Parameter value="kept"/></Parameters></tool><operation strategy="custom" tool="missing"><Parameters><Parameter name="custom" value="kept"/></Parameters></operation></model></document>"#;
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
        assert_eq!(target.setups.len(), 2);
        assert_eq!(
            target.setups[1].source.exactness,
            cadmpeg_ir::Exactness::Inferred
        );
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::CamSetupAssumed.kind()));
        assert_eq!(target.setups[0].parameters[0].value, "kept");
        assert_eq!(target.tools[0].parameters[0].value, "kept");
        assert_eq!(target.operations.len(), 1);
        assert_eq!(target.operations[0].strategy, "custom");
        assert_eq!(target.operations[0].family, ManufacturingStrategy::Other);
        assert_eq!(target.operations[0].tool, None);
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::CamSetupUnreadable.kind()));
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::CamToolUnreadable.kind()));
    });
}

#[test]
fn cam_xml_element_budget_is_enforced() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 2;
    with_decode_policy(&policy, |ctx| {
        let error = parse(
            ctx,
            View::over_retained(b"<document><model><section type='setup'/></model></document>"),
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
    let text = br#"<document><section type="setup"><Parameters><Parameter name="job_comment" value="a &amp; b"/><Parameter name="job_stockSizeX" unit="mm"><Value>12.000</Value></Parameter></Parameters></section></document>"#;
    with_decode_context(|ctx| {
        let mut target = Manufacturing::default();
        parse(
            ctx,
            View::over_retained(text),
            "synthetic",
            &mut target,
            &mut Vec::new(),
        )
        .unwrap();
        let parameters = &target.setups[0].parameters;
        assert_eq!(parameters[0].value, "a & b");
        assert!(parameters[0].native_xml.contains("a &amp; b"));
        assert_eq!(parameters[1].value, "12.000");
        assert_eq!(parameters[1].unit.as_deref(), Some("mm"));
    });
}

#[test]
fn cam_inline_tool_has_a_typed_operation_reference() {
    let text = br#"<document><section type="setup"><section strategy="drill"><tool><Parameters><Parameter name="tool_number" value="3"/><Parameter name="tool_diameter" value="4" unit="mm"/></Parameters></tool><toolpath valid="true"/></section></section></document>"#;
    with_decode_context(|ctx| {
        let mut target = Manufacturing::default();
        parse(
            ctx,
            View::over_retained(text),
            "synthetic",
            &mut target,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(target.tools.len(), 1);
        assert_eq!(
            target.operations[0].tool.as_ref(),
            Some(&target.tools[0].id)
        );
        assert_eq!(target.tools[0].number.as_deref(), Some("3"));
    });
}

#[test]
fn cam_tool_number_reference_has_a_named_assumption() {
    let text = br#"<document><tool><Parameters><Parameter name="tool_number" value="3"/></Parameters></tool><section type="setup"><section strategy="drill"><Parameters><Parameter name="tool_number" value="3"/></Parameters><toolpath/></section></section></document>"#;
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
        assert_eq!(
            target.operations[0].tool.as_ref(),
            Some(&target.tools[0].id)
        );
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::CamToolNumberReferenceAssumed.kind()));
    });
}
