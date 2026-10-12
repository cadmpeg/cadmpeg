use super::super::close_sketch_constraint_parameter_references;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchConstraintId, SketchId,
};

#[test]
fn constraint_retain_mut_refuses_work_and_service_keeps_constraint() {
    let constraint = SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:sketch-constraint#retain-mut")
            .expect("constraint ID"),
        sketch: SketchId::mint("synthetic:test:sketch#retain-mut").expect("sketch ID"),
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Disabled {},
        )
        .expect("disabled constraint"),
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
    let mut refused_document = cadmpeg_ir::CadIr::empty();
    refused_document
        .model
        .sketch_constraints
        .push(constraint.clone());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = close_sketch_constraint_parameter_references(&ctx, &mut refused_document)
        .expect_err("one constraint exceeds the zero-work budget");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::WorkUnits
                && refusal.operation == "creo sketch constraint parameter reconciliation"),
        "{error:?}"
    );
    assert_eq!(
        refused_document.model.sketch_constraints.as_slice(),
        std::slice::from_ref(&constraint)
    );

    let mut service_document = cadmpeg_ir::CadIr::empty();
    service_document
        .model
        .sketch_constraints
        .push(constraint.clone());
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
    close_sketch_constraint_parameter_references(&ctx, &mut service_document)
        .expect("service constraint reconciliation");
    assert_eq!(
        service_document.model.sketch_constraints.as_slice(),
        &[constraint]
    );
}

#[test]
fn parameter_closure_moves_native_operands_without_copying_them() {
    let mut document = cadmpeg_ir::CadIr::empty();
    document.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:sketch-constraint#native")
            .expect("constraint ID"),
        sketch: SketchId::mint("synthetic:test:sketch#native").expect("sketch ID"),
        definition: SketchConstraintDefinition::try_from(SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_core::text::NonBlankString::try_from("native")
                .expect("native kind"),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: Vec::new(),
            parameter: Some(
                cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#missing")
                    .expect("parameter ID"),
            ),
            operands: (0..128)
                .map(|index| cadmpeg_ir::sketches::SketchNativeOperand {
                    native_kind: cadmpeg_core::text::NonBlankString::try_from("operand")
                        .expect("operand kind"),
                    field: None,
                    object_index: Some(index),
                    native_ref: Some("synthetic:test:operand#reference".to_owned()),
                })
                .collect(),
        })
        .expect("native constraint"),
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
    });
    let SketchConstraintDefinitionInput::Native { operands, .. } =
        document.model.sketch_constraints[0].definition.kind()
    else {
        panic!("native input");
    };
    let pointer = operands.as_ptr();
    let capacity = operands.capacity();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    close_sketch_constraint_parameter_references(&ctx, &mut document)
        .expect("closure reuses owned payload");
    let SketchConstraintDefinitionInput::Native {
        operands,
        parameter,
        ..
    } = document.model.sketch_constraints[0].definition.kind()
    else {
        panic!("native output");
    };
    assert_eq!(operands.as_ptr(), pointer);
    assert_eq!(operands.capacity(), capacity);
    assert_eq!(operands.len(), 128);
    assert!(parameter.is_none());
    for (index, operand) in operands.iter().enumerate() {
        assert_eq!(
            operand.object_index,
            Some(u32::try_from(index).expect("fixture index"))
        );
        assert_eq!(
            operand.native_ref.as_deref(),
            Some("synthetic:test:operand#reference")
        );
    }
    ctx.finish_session().expect("active closure");
}
