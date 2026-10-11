use super::*;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId};

#[test]
fn reference_role_value_equality_is_admitted() {
    let mut parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:parameter#reference").unwrap(),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "2".into(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(!is_reference_relation_parameter_in(&ctx, &parameter).unwrap());
    parameter.properties.insert(
        cadmpeg_core::nonblank_literal!("sldprt_relation_parameter_role"),
        "reference".into(),
    );
    assert!(is_reference_relation_parameter_in(&ctx, &parameter).unwrap());
    let CodecError::ResourceLimit(limit) =
        crate::test_support::work_refusal_at("match SLDPRT relation parameter role", |ctx| {
            is_reference_relation_parameter_in(ctx, &parameter)
        })
    else {
        panic!("role value equality must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.additional, 9);
}

fn handle_fixture() -> (Vec<FeatureInputLane>, FeatureInputOperand) {
    let mut lane = super::ownership_tests::relation_lane();
    lane.classes = [
        ("slot-class", "sgSlotHandle", 10),
        ("entity-class", "sgEntHandle", 0),
    ]
    .map(|(id, name, offset)| crate::records::FeatureInputClass {
        id: id.into(),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        name: name.into(),
    })
    .into();
    lane.references.push(crate::records::FeatureInputReference {
        id: "reference".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 1,
        kind: FeatureInputOperandKind::E1,
        class_ref: Some("entity-class".into()),
        object_index: 0,
    });
    let mut marker =
        SketchInputEntity::new("slot", "lane", 0, 20, SketchInputKind::from_handle_code(0));
    marker.feature_ref = Some("feature".into());
    lane.sketch_entities.push(marker);
    let operand = FeatureInputOperand {
        offset: 1,
        reference_ref: "reference".into(),
        kind: FeatureInputOperandKind::E1,
        entity_index: 0,
        entity_ref: Some("slot".into()),
    };
    (vec![lane], operand)
}

#[test]
fn declared_handle_owner_admits_class_name() {
    let (lanes, operand) = handle_fixture();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(
        matches!(declared_entity_handle_owner(&ctx, &lanes, &operand).unwrap(),
        DeclaredEntityHandleOwner::Unique { lane, class } if std::ptr::eq(lane, &raw const lanes[0])
            && std::ptr::eq(class, &raw const lanes[0].classes[1]))
    );
    let CodecError::ResourceLimit(limit) =
        crate::test_support::work_refusal_at("match SLDPRT entity-handle class", |ctx| {
            declared_entity_handle_owner(ctx, &lanes, &operand)
        })
    else {
        panic!("owner class name must refuse");
    };
    assert_eq!(limit.additional, 11);
}

#[test]
fn declared_slot_handle_admits_slot_class_name() {
    let (lanes, operand) = handle_fixture();
    let operation = "match SLDPRT slot-handle class";
    let CodecError::ResourceLimit(limit) =
        crate::test_support::work_refusal_at(operation, |ctx| {
            declared_slot_handle_dimension_center(ctx, &lanes, "feature", &operand, |_, _| {
                Ok(Some([0, 0]))
            })
        })
    else {
        panic!("slot class name must refuse");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 12);
}

#[test]
fn declared_slot_handle_reuses_selected_reference_and_class() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let (mut lanes, operand) = handle_fixture();
    // The first matching reference and class remain authoritative.
    let mut repeated_reference = lanes[0].references[0].clone();
    repeated_reference.class_ref = Some("slot-class".into());
    lanes[0].references.push(repeated_reference);
    let mut repeated_class = lanes[0].classes[1].clone();
    repeated_class.name = "unknown".into();
    lanes[0].classes.push(repeated_class);
    for operation in [
        "find SLDPRT declared slot-handle reference",
        "find SLDPRT declared slot-handle class",
        "match SLDPRT slot entity-handle class",
        "find SLDPRT declared entity-handle reference",
        "find SLDPRT declared entity-handle class",
        "match SLDPRT entity-handle class",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut probe = None;
        assert!(declared_slot_handle_dimension_center(
            &ctx, &lanes, "feature", &operand, |lane, _| {
                assert!(std::ptr::eq(lane, &raw const lanes[0]));
                probe = Some(RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None));
                Ok(Some([0, 0]))
            },
        ).unwrap().is_none());
        assert!(probe.is_some());
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, operation).unwrap_err() else {
            panic!("the ownership proof must not run a second time");
        };
        assert_eq!(limit.additional, 1);
        assert_eq!(limit.operation, operation);
        drop(probe);
    }
}
