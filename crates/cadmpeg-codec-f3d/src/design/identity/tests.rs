// SPDX-License-Identifier: Apache-2.0
use super::neutral_configuration_id;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn configuration_identifier_refuses_retained_limit() {
    let entry = "asset/encoded:#% \u{2003}.dsgcfg";
    let name = "wide \u{a0} variant";
    let expected = crate::ids::neutral_configuration_id(entry, name);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(expected.as_str().len()).unwrap() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(neutral_configuration_id(Some(&ctx), entry, name), Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes && failure.operation == "f3d configuration identifier"));
}

#[test]
fn configuration_identifier_preserves_encoded_bytes() {
    for entry in ["plain.dsgcfg", "asset/a:#% b\u{2003}ç.dsgcfg"] {
        for name in ["Small", "v:#%\u{a0}ç"] {
            let expected = crate::ids::neutral_configuration_id(entry, name);
            let arena = DecodeArena::new();
            let policy = DecodePolicy::default();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert_eq!(neutral_configuration_id(Some(&ctx), entry, name).unwrap(), expected);
        }
    }
}

#[test]
fn projected_configuration_identifier_refuses_retained_limit() {
    let table = crate::records::configuration::DesignConfiguration::try_new(
        "asset/a:#% b\u{2003}ç.dsgcfg".to_owned(),
        crate::records::configuration::DesignConfigurationKind::Table,
        vec!["v:#%\u{a0}ç".to_owned()],
        serde_json::json!({"configurations":{"v:#%\u{a0}ç":{}}}).as_object().unwrap().clone(),
    ).unwrap();
    let name_bytes = u64::try_from(table.variants()[0].0.len()).unwrap();
    let id = crate::ids::neutral_configuration_id(table.entry_name(), &table.variants()[0].0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = name_bytes + u64::try_from(id.as_str().len()).unwrap() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(crate::design::configurations::project_configurations(Some(&ctx), &[table]), Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes && failure.operation == "f3d configuration identifier"));
}

fn assert_identity_budget<T: std::fmt::Display + std::fmt::Debug + PartialEq>(
    expected: T,
    operation: &'static str,
    construct: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let length = u64::try_from(expected.to_string().len()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = length - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(construct(&ctx), Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == operation && failure.used == 0 && failure.additional == length));
    policy.limits.max_retained_bytes = length;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(construct(&ctx).unwrap(), expected);
}

fn encoded_scope() -> crate::records::feature::scope::DesignParameterScope {
    crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:asset/a %\u{2003}ç:parameter-scope#42",
        crate::records::feature::scope::DesignFeatureKind::try_from("Native:#% \u{a0}ç".to_owned()).unwrap(),
        42,
    )
}

fn encoded_placement() -> crate::records::sketch_placement::DesignSketchPlacement {
    crate::records::sketch_placement::DesignSketchPlacement {
        id: "f3d:asset/a %\u{2003}ç:design-sketch-placement#0".to_owned(),
        scope_record_index: Some(42),
        entity_id: "sketch_100".to_owned().try_into().unwrap(),
        visibility: None,
        class_tag: "356".to_owned().try_into().unwrap(),
        record_index: 43,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0, crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact).unwrap(),
    }
}

#[test]
fn neutral_feature_id_refuses_before_allocation_and_preserves_bytes() {
    let scope = encoded_scope();
    assert_identity_budget(crate::ids::neutral_feature_id(&scope), "f3d feature identifier", |ctx| super::neutral_feature_id(Some(ctx), &scope));
}

#[test]
fn neutral_component_insert_occurrence_id_refuses_before_allocation_and_preserves_bytes() {
    let scope = encoded_scope();
    assert_identity_budget(crate::ids::neutral_component_insert_occurrence_id(&scope), "f3d component insert occurrence identifier", |ctx| super::neutral_component_insert_occurrence_id(Some(ctx), &scope));
}

#[test]
fn neutral_assembly_joint_id_refuses_before_allocation_and_preserves_bytes() {
    let scope = encoded_scope();
    assert_identity_budget(crate::ids::neutral_assembly_joint_id(&scope), "f3d assembly joint identifier", |ctx| super::neutral_assembly_joint_id(Some(ctx), &scope));
}

#[test]
fn neutral_sketch_id_refuses_before_allocation_and_preserves_bytes() {
    let placement = encoded_placement();
    assert_identity_budget(crate::ids::neutral_sketch_id(&placement), "f3d sketch identifier", |ctx| super::neutral_sketch_id(Some(ctx), &placement));
}

#[test]
fn neutral_spatial_sketch_id_refuses_before_allocation_and_preserves_bytes() {
    let placement = encoded_placement();
    assert_identity_budget(crate::ids::neutral_spatial_sketch_id(&placement), "f3d spatial sketch identifier", |ctx| super::neutral_spatial_sketch_id(Some(ctx), &placement));
}

#[test]
fn neutral_parameter_id_refuses_before_allocation_and_preserves_bytes() {
    let parameter = crate::design::decode::parameters::parse_design_parameter_record(&crate::design::test_support::parameter_record(Some(40), "1 cm", "AlongDistance", Some("cm"), "d9", 1.0)).unwrap();
    assert_identity_budget(crate::ids::neutral_parameter_id(&parameter), "f3d parameter identifier", |ctx| super::neutral_parameter_id(Some(ctx), &parameter));
}

#[test]
fn neutral_sketch_point_id_refuses_before_allocation_and_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::neutral_sketch_point_id(&sketch, u64::MAX), "f3d sketch point identifier", |ctx| super::neutral_sketch_point_id(Some(ctx), &sketch, u64::MAX));
}

#[test]
fn neutral_sketch_record_id_refuses_before_allocation_and_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::neutral_sketch_record_id(&sketch, u32::MAX), "f3d sketch record identifier", |ctx| super::neutral_sketch_record_id(Some(ctx), &sketch, u32::MAX));
}

#[test]
fn neutral_sketch_text_id_refuses_before_allocation_and_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::neutral_sketch_text_id(&sketch, u64::MAX), "f3d sketch text identifier", |ctx| super::neutral_sketch_text_id(Some(ctx), &sketch, u64::MAX));
}

#[test]
fn neutral_sketch_curve_id_refuses_before_allocation_and_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::neutral_sketch_curve_id(&sketch, u64::MAX, u64::MAX), "f3d sketch curve identifier", |ctx| super::neutral_sketch_curve_id(Some(ctx), &sketch, u64::MAX, u64::MAX));
}

#[test]
fn neutral_spatial_sketch_point_id_refuses_before_allocation_and_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SpatialSketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::neutral_spatial_sketch_point_id(&sketch, u64::MAX), "f3d spatial sketch point identifier", |ctx| super::neutral_spatial_sketch_point_id(Some(ctx), &sketch, u64::MAX));
}

#[test]
fn neutral_spatial_sketch_record_id_refuses_before_allocation_and_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SpatialSketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::neutral_spatial_sketch_record_id(&sketch, u32::MAX), "f3d spatial sketch record identifier", |ctx| super::neutral_spatial_sketch_record_id(Some(ctx), &sketch, u32::MAX));
}

#[test]
fn neutral_spatial_sketch_surface_id_refuses_before_allocation_and_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SpatialSketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::neutral_spatial_sketch_surface_id(&sketch, u64::MAX), "f3d spatial sketch surface identifier", |ctx| super::neutral_spatial_sketch_surface_id(Some(ctx), &sketch, u64::MAX));
}

#[test]
fn neutral_spatial_sketch_curve_id_refuses_before_allocation_and_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SpatialSketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::neutral_spatial_sketch_curve_id(&sketch, u64::MAX, u64::MAX), "f3d spatial sketch curve identifier", |ctx| super::neutral_spatial_sketch_curve_id(Some(ctx), &sketch, u64::MAX, u64::MAX));
}

#[test]
fn neutral_sketch_constraint_id_refuses_before_allocation_and_preserves_bytes() {
    let native = "f3d:asset/a %\u{2003}ç:relation#42";
    assert_identity_budget(crate::ids::neutral_sketch_constraint_id(native, u32::MAX), "f3d sketch constraint identifier", |ctx| super::neutral_sketch_constraint_id(Some(ctx), native, u32::MAX));
}

#[test]
fn neutral_dimension_constraint_id_refuses_before_allocation_and_preserves_bytes() {
    let parameter = cadmpeg_ir::features::ParameterId::mint("test:model:parameter#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::neutral_dimension_constraint_id(&parameter, "a:#% \u{2003}ç"), "f3d dimension constraint identifier", |ctx| super::neutral_dimension_constraint_id(Some(ctx), &parameter, "a:#% \u{2003}ç"));
}

#[test]
fn configuration_entry_id_refuses_before_allocation_and_preserves_bytes() {
    let entry = "asset/a:#% \u{2003}ç.dsgcfg";
    assert_identity_budget(crate::ids::configuration_entry_id(entry, &cadmpeg_ir::identity_component!("configuration")), "f3d configuration native identifier", |ctx| super::configuration_entry_id(Some(ctx), entry));
}

#[test]
fn history_input_prefix_refuses_before_allocation_and_preserves_bytes() {
    let key = cadmpeg_ir::identity_key!("encoded%20ç");
    assert_identity_budget(crate::ids::history_input_prefix(&key, i64::MIN), "f3d history input prefix", |ctx| super::history_input_prefix(Some(ctx), key.as_str(), i64::MIN));
}

#[test]
fn feature_input_topology_id_refuses_before_allocation_and_preserves_bytes() {
    let feature = cadmpeg_ir::features::FeatureId::mint("test:model:feature#encoded%20ç").unwrap();
    assert_identity_budget(crate::ids::feature_input_topology_id(&feature, i64::MIN), "f3d feature input topology identifier", |ctx| super::feature_input_topology_id(Some(ctx), &feature, i64::MIN));
}

#[test]
fn history_input_edge_id_refuses_before_allocation_and_preserves_bytes() {
    let prefix = cadmpeg_ir::identity_key!("encoded%20ç");
    assert_identity_budget(crate::ids::history_input_edge_id(&prefix, i64::MIN), "f3d historical edge identifier", |ctx| super::history_input_edge_id(Some(ctx), &prefix, i64::MIN, "f3d historical edge identifier"));
}

#[test]
fn history_input_face_id_refuses_before_allocation_and_preserves_bytes() {
    let prefix = cadmpeg_ir::identity_key!("encoded%20ç");
    assert_identity_budget(crate::ids::history_input_face_id(&prefix, i64::MIN), "f3d historical face identifier", |ctx| super::history_input_face_id(Some(ctx), &prefix, i64::MIN, "f3d historical face identifier"));
}

#[test]
fn history_input_vertex_id_refuses_before_allocation_and_preserves_bytes() {
    let prefix = cadmpeg_ir::identity_key!("encoded%20ç");
    assert_identity_budget(crate::ids::history_input_vertex_id(&prefix, i64::MIN), "f3d historical vertex identifier", |ctx| super::history_input_vertex_id(Some(ctx), &prefix, i64::MIN, "f3d historical vertex identifier"));
}
