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
        &[constraint.clone()]
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
