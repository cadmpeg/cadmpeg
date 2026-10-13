use super::*;
use cadmpeg_core::CodecError;

#[test]
fn dimensioned_carrier_reuses_one_declared_owner_proof() {
    let reference = "reference".repeat(8192);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: vec![crate::records::FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            name: "sgEntHandle".into(),
        }],
        references: vec![crate::records::FeatureInputReference {
            id: reference.clone(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 0,
            kind: FeatureInputOperandKind::E1,
            class_ref: Some("class".into()),
            object_index: 0,
        }],
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let lanes = [lane];
    let operand = FeatureInputOperand {
        offset: 0,
        reference_ref: reference,
        kind: FeatureInputOperandKind::E1,
        entity_index: 0,
        entity_ref: None,
    };
    let CodecError::ResourceLimit(single) =
        crate::test_support::work_refusal_at("test declared owner boundary", |ctx| {
            assert!(matches!(
                declared_entity_handle_owner(ctx, &lanes, &operand)?,
                DeclaredEntityHandleOwner::Unique { .. }
            ));
            ctx.charge_work(1, "test declared owner boundary")
        })
    else {
        panic!("the work oracle must locate the owner proof boundary");
    };
    let index_ctx = cadmpeg_test_support::service_decode_context();
    let index = LaneMarkerIndex::new(&index_ctx, &lanes).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = single.used.checked_mul(2).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        dimensioned_relation_carrier(&ctx, &index, "feature", &operand, 1.0)
            .unwrap()
            .is_none()
    );
    ctx.finish_session().unwrap();
}
