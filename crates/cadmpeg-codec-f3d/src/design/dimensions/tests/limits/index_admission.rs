// SPDX-License-Identifier: Apache-2.0
use super::{fixture, native_fallback_curves, native_fallback_pair, parameter_companion};
use crate::design::dimensions::{
    project_dimension_constraints, project_dimension_constraints_with_presentations,
};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;

fn source_refusal(operation: &'static str) {
    let fixture = fixture();
    let inputs = fixture.inputs();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| project_dimension_constraints(ctx, &inputs, &[], super::EPS_NATIVE_FALLBACK_LINEAR),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn dimension_placement_source_refuses_work_limit() {
    source_refusal("f3d dimension placement source");
}

#[test]
fn dimension_parameter_source_refuses_work_limit() {
    source_refusal("f3d dimension parameter source");
}

#[test]
fn dimension_owner_source_refuses_work_limit() {
    source_refusal("f3d dimension owner source");
}

#[test]
fn dimension_curve_source_refuses_work_limit() {
    source_refusal("f3d dimension curve source");
}

#[test]
fn dimension_native_reference_source_refuses_work_limit() {
    source_refusal("f3d dimension native reference source");
}

#[test]
fn dimension_projected_entity_source_refuses_work_limit() {
    source_refusal("f3d dimension projected entity source");
}

#[test]
fn dimension_native_reference_lookup_refuses_work_limit() {
    source_refusal("f3d dimension native reference lookup");
}

#[test]
fn dimension_secondary_curve_source_refuses_work_limit() {
    source_refusal("f3d dimension secondary curve source");
}

#[test]
fn dimension_point_source_refuses_work_limit() {
    use crate::records::sketch_geometry::{
        SketchPoint, SketchPointClosure, SketchPointCompanion, SketchPointDraft,
        SketchPointRecordForm,
    };
    let fixture = fixture();
    let point = SketchPoint::try_from(SketchPointDraft {
        id: "f3d:Design/BulkStream.dat:sketch-point#40".into(),
        record_index: 40,
        owner_reference: Some(10),
        class_tag: fixture.curve.class_tag.clone(),
        byte_offset: 0,
        coordinate_offset: 0,
        record_form: SketchPointRecordForm::version11(
            40,
            SketchPointClosure::Selector0State0,
            None,
            0.0,
        ),
        companion: SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        paired_reference: 0,
        coordinates: cadmpeg_ir::math::Point2::new(0.0, 0.0),
    })
    .unwrap();
    let mut inputs = fixture.inputs();
    inputs.points = std::slice::from_ref(&point);
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d dimension point source",
        0,
        |ctx| project_dimension_constraints(ctx, &inputs, &[], super::EPS_NATIVE_FALLBACK_LINEAR),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "f3d dimension point source"));
}

fn pair_query_refusal(operation: &'static str) {
    let mut fixture = fixture();
    let curves = native_fallback_curves(&mut fixture);
    let pair = native_fallback_pair();
    let mut inputs = fixture.inputs();
    inputs.curves = &curves;
    inputs.pairs = std::slice::from_ref(&pair);
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| project_dimension_constraints(ctx, &inputs, &[], super::EPS_NATIVE_FALLBACK_LINEAR),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn dimension_companion_parameter_lookup_refuses_work_limit() {
    pair_query_refusal("f3d dimension companion parameter lookup");
}

#[test]
fn dimension_parameter_lookup_refuses_work_limit() {
    pair_query_refusal("f3d dimension parameter lookup");
}

#[test]
fn dimension_projected_sketch_source_refuses_work_limit() {
    pair_query_refusal("f3d dimension projected sketch source");
}

#[test]
fn dimension_sketch_entity_lookup_refuses_work_limit() {
    pair_query_refusal("f3d dimension sketch entity lookup");
}

#[test]
fn dimension_projected_sketch_agreement_refuses_work_limit() {
    pair_query_refusal("f3d dimension projected sketch agreement");
}

#[test]
fn dimension_sketch_identity_refuses_work_limit() {
    pair_query_refusal("f3d dimension sketch identity");
}

#[test]
fn dimension_native_owner_source_refuses_work_limit() {
    pair_query_refusal("f3d dimension native owner source");
}

#[test]
fn dimension_native_owner_lookup_refuses_work_limit() {
    pair_query_refusal("f3d dimension native owner lookup");
}

#[test]
fn dimension_native_owner_agreement_refuses_work_limit() {
    pair_query_refusal("f3d dimension native owner agreement");
}

#[test]
fn dimension_owner_sketch_lookup_refuses_work_limit() {
    pair_query_refusal("f3d dimension owner sketch lookup");
}

#[test]
fn native_dimension_operand_source_refuses_work_limit() {
    pair_query_refusal("f3d native dimension operand source");
}

#[test]
fn native_dimension_entity_lookup_refuses_work_limit() {
    pair_query_refusal("f3d native dimension entity lookup");
}

#[test]
fn dimension_native_operand_lookup_refuses_work_limit() {
    pair_query_refusal("f3d dimension native operand lookup");
}

#[test]
fn dimension_native_projected_lookup_refuses_work_limit() {
    pair_query_refusal("f3d dimension native projected lookup");
}

#[test]
fn dimension_indexes_borrow_ids_and_release_storage() {
    let fixture = fixture();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1 << 20;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let output = project_dimension_constraints(
            ctx,
            &fixture.inputs(),
            &[],
            super::EPS_NATIVE_FALLBACK_LINEAR,
        )
        .unwrap();
        assert!(output.is_empty());
        drop(output);
        let _released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released dimension index storage",
            )
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn dimension_native_geometry_index_refuses_materialized_limit() {
    let fixture = fixture();
    let curves: Vec<_> = (0..128)
        .map(|index| {
            let mut curve = fixture.curve.clone();
            curve.record_index = 100 + index;
            curve.id = format!(
                "f3d:Design/BulkStream.dat:sketch-curve#{}",
                curve.record_index
            );
            curve
        })
        .collect();
    let mut inputs = fixture.inputs();
    inputs.curves = &curves;
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::MaterializedBytes,
        "f3d dimension native geometry index",
        0,
        |ctx| project_dimension_constraints(ctx, &inputs, &[], super::EPS_NATIVE_FALLBACK_LINEAR),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "f3d dimension native geometry index"));
}

#[test]
fn dimension_native_reference_collision_uses_last_ordered_record() {
    let mut fixture = fixture();
    let mut curves = native_fallback_curves(&mut fixture);
    curves[1].id = curves[0].id.clone();
    let pair = native_fallback_pair();
    let decode = |curves: &[_]| {
        let mut inputs = fixture.inputs();
        inputs.curves = curves;
        inputs.pairs = std::slice::from_ref(&pair);
        crate::test_support::with_decode_context(|ctx| {
            project_dimension_constraints(ctx, &inputs, &[], super::EPS_NATIVE_FALLBACK_LINEAR)
        })
        .unwrap()
    };
    let first = decode(&curves);
    let second = decode(&[curves[1].clone(), curves[0].clone()]);
    assert_eq!(first, second);
    let [constraint] = first.as_slice() else {
        panic!("expected one native pair dimension");
    };
    let SketchConstraintDefinitionInput::Native {
        entities, operands, ..
    } = constraint.definition.kind()
    else {
        panic!("expected native fallback");
    };
    assert_eq!(entities, &vec![fixture.entity.id().clone()]);
    assert_eq!(operands[0].object_index, Some(30));
    assert_eq!(
        operands[0].native_ref.as_deref(),
        Some(curves[0].id.as_str())
    );
    assert_eq!(operands[1].object_index, Some(31));
    assert!(operands[1].native_ref.is_none());
}

#[test]
fn dimension_scope_index_keeps_the_last_placement() {
    let mut fixture = fixture();
    let mut last = fixture.placement.clone();
    last.entity_id =
        crate::records::identity::DesignEntityId::try_from("Sketch_8".to_owned()).unwrap();
    fixture.entity.sketch = crate::ids::neutral_sketch_id(&last);
    let expected = fixture.entity.sketch.clone();
    let placements = [fixture.placement.clone(), last];
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.placements = &placements;
    inputs.companions = std::slice::from_ref(&companion);
    let output = crate::test_support::with_decode_context(|ctx| {
        project_dimension_constraints(ctx, &inputs, &[], super::EPS_NATIVE_FALLBACK_LINEAR)
    })
    .unwrap();
    let [constraint] = output.as_slice() else {
        panic!("expected one measured companion dimension");
    };
    assert_eq!(constraint.sketch, expected);
    assert!(matches!(
        constraint.definition.kind(),
        SketchConstraintDefinitionInput::DistanceLoci { .. }
    ));
}

fn presentation(index: u32) -> crate::records::dimensions::DesignDimensionPresentationFrame {
    crate::records::dimensions::DesignDimensionPresentationFrame {
        id: format!("f3d:Design/BulkStream.dat:dimension-presentation#{index}"),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("314".to_owned()).unwrap(),
        record_index: index,
        frame_length: 0,
        operands: Vec::new(),
        presentation_bytes: Vec::new(),
        presentation_byte_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("281".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
        owner_reference: 7,
        owner_reference_offset: 0,
        governing_owner_record_index: 21,
        governing_parameter_record_index: 20,
        governing_companion_record_index: 22,
    }
}

fn presentation_refusal(
    frames: &[crate::records::dimensions::DesignDimensionPresentationFrame],
    operation: &'static str,
) -> cadmpeg_core::decode::ResourceLimit {
    let fixture = fixture();
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            project_dimension_constraints_with_presentations(
                ctx,
                &inputs,
                frames,
                &[],
                super::EPS_NATIVE_FALLBACK_LINEAR,
            )
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected work limit");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit
}

#[test]
fn dimension_presentation_source_refuses_work_limit() {
    presentation_refusal(&[presentation(50)], "f3d dimension presentation source");
}

#[test]
fn dimension_presentation_stream_refuses_work_limit() {
    presentation_refusal(&[presentation(50)], "f3d dimension presentation stream");
}

#[test]
fn dimension_ambiguous_presentations_stop_at_the_second_match() {
    let prefix = [presentation(50), presentation(51)];
    let complete = [presentation(50), presentation(51), presentation(52)];
    let first = presentation_refusal(&prefix, "f3d companion constraint native reference");
    let second = presentation_refusal(&complete, "f3d companion constraint native reference");
    assert_eq!(first.used, second.used);
    assert_eq!(first.additional, second.additional);
}

#[test]
fn dimension_companion_source_refuses_work_limit() {
    presentation_refusal(&[], "f3d dimension companion source");
}

#[test]
fn dimension_companion_projection_source_refuses_work_limit() {
    presentation_refusal(&[], "f3d dimension companion projection source");
}

#[test]
fn dimension_companion_owner_lookup_refuses_work_limit() {
    presentation_refusal(&[], "f3d dimension companion owner lookup");
}

#[test]
fn dimension_companion_sketch_lookup_refuses_work_limit() {
    presentation_refusal(&[], "f3d dimension companion sketch lookup");
}

#[test]
fn dimension_container_only_lookup_refuses_work_limit() {
    presentation_refusal(&[], "f3d container-only dimension lookup");
}

#[test]
fn dimension_companion_owner_source_refuses_work_limit() {
    source_refusal("f3d dimension companion owner source");
}

#[test]
fn dimension_recipe_record_source_refuses_work_limit() {
    super::assert_recipe_projection_refusal(
        "f3d dimension recipe record source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_recipe_sort_source_refuses_work_limit() {
    super::assert_recipe_projection_refusal(
        "f3d dimension recipe sort source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_recipe_group_source_refuses_work_limit() {
    super::assert_recipe_projection_refusal(
        "f3d dimension recipe group source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_recipe_companion_lookup_refuses_work_limit() {
    super::assert_recipe_projection_refusal(
        "f3d dimension recipe companion lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_recipe_owner_lookup_refuses_work_limit() {
    super::assert_recipe_projection_refusal(
        "f3d dimension recipe owner lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_recipe_sketch_lookup_refuses_work_limit() {
    super::assert_recipe_projection_refusal(
        "f3d dimension recipe sketch lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_recipe_operand_source_refuses_work_limit() {
    super::assert_recipe_projection_refusal(
        "f3d dimension recipe operand source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_projected_companion_lookup_refuses_work_limit() {
    super::assert_recipe_projection_refusal(
        "f3d dimension projected companion lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn projected_dimension_constraint_source_refuses_work_limit() {
    pair_query_refusal("f3d projected dimension constraint source");
}

#[test]
fn projected_dimension_parameter_lookup_refuses_work_limit() {
    pair_query_refusal("f3d projected dimension parameter lookup");
}
