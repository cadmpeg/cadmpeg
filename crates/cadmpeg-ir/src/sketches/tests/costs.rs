use crate::features::ParameterId;
use crate::sketches::{
    NativeOperandField, SketchConstraintDefinitionInput, SketchEntityId, SketchNativeOperand,
};
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure,
};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn native_constraint() -> SketchConstraintDefinitionInput {
    SketchConstraintDefinitionInput::Native {
        native_kind: NonBlankString::try_from("native").unwrap(),
        native_state: Some(7),
        native_flags: None,
        native_properties: BTreeMap::from([("key".to_owned(), "value".to_owned())]),
        entities: vec![SketchEntityId::mint("synthetic:test:entity#e").unwrap()],
        parameter: Some(ParameterId::mint("synthetic:test:parameter#p").unwrap()),
        operands: vec![SketchNativeOperand {
            native_kind: NonBlankString::try_from("operand").unwrap(),
            field: Some(NativeOperandField {
                name: NonBlankString::try_from("field").unwrap(),
                role: Some(2),
            }),
            object_index: Some(3),
            native_ref: Some("ref".to_owned()),
        }],
    }
}

#[test]
fn sketch_constraint_cost_counts_native_children() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One entity, one native operand, and one property entry are measured.
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        native_constraint()
            .decode_cost(&ctx, "measure constraint")
            .unwrap(),
        102
    );
    let error = ctx.charge_work(1, "after measurement").unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.reason == ResourceFailure::BudgetExceeded
            && limit.used == 3 && limit.additional == 1 && limit.limit == 3
            && limit.operation == "after measurement"));
}

#[test]
fn sketch_constraint_cost_refuses_child_traversal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = native_constraint()
        .decode_cost(&ctx, "measure constraint")
        .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.reason == ResourceFailure::BudgetExceeded
            && limit.used == 0 && limit.additional == 1 && limit.limit == 0
            && limit.operation == "measure constraint"));
}
