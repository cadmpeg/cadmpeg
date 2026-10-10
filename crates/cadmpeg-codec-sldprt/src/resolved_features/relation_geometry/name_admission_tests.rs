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
    parameter.properties.insert(cadmpeg_core::nonblank_literal!("sldprt_relation_parameter_role"), "reference".into());
    assert!(is_reference_relation_parameter_in(&ctx, &parameter).unwrap());
    let CodecError::ResourceLimit(limit) = crate::test_support::work_refusal_at(
        "match SLDPRT relation parameter role",
        |ctx| is_reference_relation_parameter_in(ctx, &parameter),
    ) else { panic!("role value equality must refuse"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.additional, 9);
}

fn handle_fixture() -> (Vec<FeatureInputLane>, FeatureInputOperand) {
    let mut lane = super::ownership_tests::relation_lane();
    lane.classes = [("entity-class", "sgEntHandle", 0), ("slot-class", "sgSlotHandle", 10)]
        .map(|(id, name, offset)| crate::records::FeatureInputClass {
            id: id.into(), parent: "lane".into(), ordinal: 0, offset, name: name.into(),
        }).into();
    lane.references.push(crate::records::FeatureInputReference {
        id: "reference".into(), parent: "lane".into(), feature_ref: Some("feature".into()),
        ordinal: 0, offset: 1, kind: FeatureInputOperandKind::E1,
        class_ref: Some("entity-class".into()), object_index: 0,
    });
    let mut marker = SketchInputEntity::new("slot", "lane", 0, 20, SketchInputKind::from_handle_code(0));
    marker.feature_ref = Some("feature".into());
    lane.sketch_entities.push(marker);
    let operand = FeatureInputOperand {
        offset: 1, reference_ref: "reference".into(), kind: FeatureInputOperandKind::E1,
        entity_index: 0, entity_ref: Some("slot".into()),
    };
    (vec![lane], operand)
}

#[test]
fn declared_handle_owner_admits_class_name() {
    let (lanes, operand) = handle_fixture();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(matches!(declared_entity_handle_owner(&ctx, &lanes, &operand).unwrap(),
        DeclaredEntityHandleOwner::Unique(lane) if std::ptr::eq(lane, &lanes[0])));
    let CodecError::ResourceLimit(limit) = crate::test_support::work_refusal_at(
        "match SLDPRT entity-handle class",
        |ctx| declared_entity_handle_owner(ctx, &lanes, &operand),
    ) else { panic!("owner class name must refuse"); };
    assert_eq!(limit.additional, 11);
}

#[test]
fn declared_slot_handle_admits_both_class_names() {
    let (lanes, operand) = handle_fixture();
    for operation in ["match SLDPRT slot entity-handle class", "match SLDPRT slot-handle class"] {
        let CodecError::ResourceLimit(limit) = crate::test_support::work_refusal_at(
            operation,
            |ctx| declared_slot_handle_dimension_center(ctx, &lanes, "feature", &operand,
                |_, _| Ok(Some([0, 0]))),
        ) else { panic!("slot class name must refuse"); };
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.additional, 11);
    }
}
