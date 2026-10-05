use super::super::reconcile_constraint_entity_references;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntityId};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn constraint_entity_retention_refuses_and_service_preserves_membership() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let entity = SketchEntityId::mint("creo:test:entity#1").expect("entity");
    let emitted = BTreeSet::from([entity.clone()]);
    let make_definition = || SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_core::text::NonBlankString::try_from("native")
            .expect("nonblank kind"),
        native_state: None,
        native_flags: None,
        native_properties: BTreeMap::new(),
        entities: vec![entity.clone()],
        parameter: None,
        operands: Vec::new(),
    };
    let mut definition = make_definition();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");

    let error = reconcile_constraint_entity_references(&ctx, &mut definition, &emitted)
        .expect_err("retention requires work");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo constraint emitted entity retention"));

    let mut admitted = make_definition();
    assert!(crate::decode::with_test_decode_ctx(|ctx|
        reconcile_constraint_entity_references(ctx, &mut admitted, &emitted)
    )
    .expect("service entity retention admitted"));
    assert!(matches!(admitted, SketchConstraintDefinitionInput::Native { entities, .. }
        if entities == vec![entity]));
}
