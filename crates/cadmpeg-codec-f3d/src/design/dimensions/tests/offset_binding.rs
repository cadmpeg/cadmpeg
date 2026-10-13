// SPDX-License-Identifier: Apache-2.0

use crate::design::dimensions::bind_offset_dimension_parameters;
use crate::ids::neutral_parameter_id;
use crate::records::parameters::DesignParameter;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchNativeOperand, SketchOffsetPair,
};

fn offset_binding_fixture() -> (Vec<SketchConstraint>, Vec<DesignParameter>) {
    let parameter =
        DesignParameter::try_from(crate::records::parameters::DesignParameterDraft::<String> {
            id: "f3d:test:parameter#1".into(),
            byte_offset: 0,
            class_tag: "277".to_owned().try_into().unwrap(),
            record_index: 1,
            source_ordinal: 1,
            source: crate::records::parameters::DesignParameterSource::new::<String>(
                "Linear Dimension-4".into(),
                Some(0),
                None,
            )
            .unwrap(),
            expression: "0.5".into(),
            expression_offset: 40,
            source_kind_offset: 60,
            unit: Some(crate::records::identity::RecordedValue {
                value: "cm".into(),
                offset: 70,
            }),
            name: "Distance".into(),
            name_offset: 80,
            evaluated_value: 0.5,
            evaluated_value_offset: 90,
        })
        .expect("generated parameter");
    let parameter_id = neutral_parameter_id(&parameter);
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#1").unwrap();
    let source = cadmpeg_ir::sketches::SketchEntityId::mint("test:model:entity#source").unwrap();
    let result = cadmpeg_ir::sketches::SketchEntityId::mint("test:model:entity#result").unwrap();
    let operand = |kind| SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::try_from(kind).unwrap(),
        field: None,
        object_index: None,
        native_ref: None,
    };
    let native = SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_core::text::NonBlankString::try_from("Linear Dimension-4").unwrap(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
        entities: vec![source.clone()],
        parameter: Some(parameter_id),
        operands: vec![operand("null_locus"), operand("curve")],
    };
    let offset = SketchConstraintDefinitionInput::Offset {
        pairs: vec![SketchOffsetPair {
            source,
            result,
            source_reversed: false,
        }],
        distance: cadmpeg_ir::scalar::Length::new(5.0).unwrap(),
        parameter: None,
    };
    let constraint = |ordinal, definition| SketchConstraint {
        id: cadmpeg_ir::sketches::SketchConstraintId::mint(format!(
            "test:model:constraint#{ordinal}"
        ))
        .unwrap(),
        sketch: sketch.clone(),
        definition: SketchConstraintDefinition::try_from(definition).unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    (
        vec![constraint(1, native), constraint(2, offset)],
        vec![parameter],
    )
}

#[test]
fn offset_dimension_binding_preserves_parameter_and_removes_annotation() {
    let (mut constraints, parameters) = offset_binding_fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    bind_offset_dimension_parameters(&ctx, &mut constraints, &parameters).unwrap();
    assert_eq!(constraints.len(), 1);
    assert!(matches!(constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::Offset { parameter: Some(driving), .. }
            if driving.id == neutral_parameter_id(&parameters[0]) && !driving.negated));
}

fn assert_offset_binding_limit(operation: &'static str, dimension: ResourceDimension) {
    let (constraints, parameters) = offset_binding_fixture();
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        bind_offset_dimension_parameters(ctx, &mut constraints.clone(), &parameters)
    });
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == operation && failure.dimension == dimension));
}

#[test]
fn offset_parameter_value_index_refuses_collection_limit() {
    assert_offset_binding_limit(
        "f3d offset parameter value index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn offset_parameter_value_lookup_refuses_work_limit() {
    assert_offset_binding_limit(
        "f3d offset parameter value lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn offset_dimension_binding_refuses_collection_limit() {
    assert_offset_binding_limit(
        "f3d offset dimension binding",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn offset_binding_count_refuses_collection_limit() {
    assert_offset_binding_limit(
        "f3d offset binding count",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn offset_driving_parameter_id_refuses_retained_limit() {
    assert_offset_binding_limit(
        "f3d offset driving parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn offset_removed_dimension_refuses_collection_limit() {
    assert_offset_binding_limit(
        "f3d offset removed dimension",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn offset_ambiguous_target_copies_no_parameter_and_releases_scratch() {
    let (mut constraints, parameters) = offset_binding_fixture();
    constraints.push(constraints[1].clone());
    let offset = constraints[1].clone();
    constraints.extend((0..32).map(|_| offset.clone()));
    let before = constraints.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        bind_offset_dimension_parameters(ctx, &mut constraints, &parameters).unwrap();
        assert_eq!(constraints, before);
        let _released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released ambiguous offset binding",
            )
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn offset_multiple_drivers_copy_no_parameter_and_release_scratch() {
    let (mut constraints, parameters) = offset_binding_fixture();
    constraints.push(constraints[0].clone());
    let before = constraints.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        bind_offset_dimension_parameters(ctx, &mut constraints, &parameters).unwrap();
        assert_eq!(constraints, before);
        let _released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released duplicate offset drivers",
            )
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn offset_selected_driver_copies_the_parameter_once() {
    let (mut constraints, parameters) = offset_binding_fixture();
    let parameter = neutral_parameter_id(&parameters[0]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(parameter.as_str().len()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        bind_offset_dimension_parameters(ctx, &mut constraints, &parameters).unwrap();
        assert_eq!(constraints.len(), 1);
        assert!(matches!(constraints[0].definition.kind(),
            SketchConstraintDefinitionInput::Offset { parameter: Some(driving), .. }
                if driving.id == parameter && !driving.negated));
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn offset_binding_admits_original_sources_and_queries() {
    let (constraints, parameters) = offset_binding_fixture();
    for operation in [
        "f3d offset parameter source",
        "f3d offset dimension source",
        "f3d offset parameter value lookup",
        "f3d offset first source",
        "f3d offset candidate source",
        "f3d offset candidate pair source",
        "f3d offset candidate duplicate",
        "f3d offset candidate index",
        "f3d offset candidate lookup",
        "f3d offset unique suffix",
        "f3d offset binding count source",
        "f3d offset binding count lookup",
        "f3d offset binding uniqueness source",
        "f3d offset binding mutation source",
        "f3d offset constraint removal source",
        "f3d offset removed dimension lookup",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| bind_offset_dimension_parameters(ctx, &mut constraints.clone(), &parameters),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
    }
}

#[test]
fn offset_parameter_identity_refuses_materialized_limit() {
    assert_offset_binding_limit(
        "f3d parameter identifier",
        ResourceDimension::MaterializedBytes,
    );
}

#[test]
fn offset_index_stays_absent_without_an_eligible_dimension() {
    let (mut constraints, _) = offset_binding_fixture();
    constraints.remove(0);
    let before = constraints.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        bind_offset_dimension_parameters(ctx, &mut constraints, &[]).unwrap();
        assert_eq!(constraints, before);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn offset_duplicate_parameter_identity_keeps_the_last_value() {
    let (mut constraints, mut parameters) = offset_binding_fixture();
    let mut later = crate::design::decode::parameters::parse_design_parameter_record(
        &crate::design::test_support::parameter_record(
            Some(0),
            "0.6 cm",
            "Linear Dimension-4",
            Some("cm"),
            "Distance",
            0.6,
        ),
    )
    .unwrap();
    later.id = parameters[0].id.clone();
    later.record_index = parameters[0].record_index;
    parameters.push(later);
    let before = constraints.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        bind_offset_dimension_parameters(ctx, &mut constraints, &parameters).unwrap();
        assert_eq!(constraints, before);
        let _released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released duplicate offset parameter identity",
            )
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}
