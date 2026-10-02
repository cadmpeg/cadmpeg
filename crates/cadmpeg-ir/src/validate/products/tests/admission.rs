// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::document::CadIr;
use crate::products::{Occurrence, OccurrenceParent, PrototypeReference};
use crate::report::check::Finding;
use crate::transform::Transform;

fn occurrence(id: &str, parent: Option<&str>, ordinal: u32) -> Occurrence {
    Occurrence {
        id: id.try_into().unwrap(), prototype: PrototypeReference::Unresolved {},
        parent: parent.map_or(OccurrenceParent::Root {}, |id| OccurrenceParent::Occurrence { occurrence: id.try_into().unwrap() }),
        ordinal, transform: Transform::identity(), linked_prototype: None,
        scale: [crate::scalar::FiniteReal::ONE; 3], name: None, visible: None, link: None, native_ref: None,
    }
}

fn refuses(ir: &CadIr) {
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits, ResourceDimension::RetainedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = super::super::check_products(&ctx, ir, &mut findings) else { panic!("product checker must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

fn missing_body(ir: &mut CadIr) {
    ir.model.product_definitions.push(serde_json::from_value(serde_json::json!({
        "id": "test:model:product#definition", "kind": "part", "bodies": ["test:model:body#missing"]
    })).unwrap());
}

fn missing_joint(ir: &mut CadIr) {
    use crate::products::{AssemblyJoint, JointConnector, JointOperand, PairedJointKind, OperandContainer};
    ir.model.assembly_joints.push(AssemblyJoint::paired(
        "test:model:joint#missing".try_into().unwrap(),
        PairedJointKind::Fixed { angle: None, translation_offset: None, angular_limits: None, linear_limits: None },
        [JointConnector { operand: JointOperand { container: OperandContainer::Occurrence { occurrence: "test:model:occurrence#absent".try_into().unwrap() }, object: "first".into(), subelements: Vec::new() }, frame: Transform::identity(), detached: false },
         JointConnector { operand: JointOperand::root("second", Vec::new()), frame: Transform::identity(), detached: false }],
        None,
    ));
}

#[test]
fn product_body_validation_preserves_index_walk_and_finding_refusals() {
    let mut ir = CadIr::empty();
    missing_body(&mut ir);
    refuses(&ir);
}

#[test]
fn product_sibling_ordinal_validation_preserves_all_refusals() {
    let mut ir = CadIr::empty();
    ir.model.occurrences = vec![occurrence("test:model:occurrence#first", None, 0), occurrence("test:model:occurrence#second", None, 0)];
    refuses(&ir);
}

#[test]
fn product_auxiliary_definition_validation_preserves_all_refusals() {
    let mut ir = CadIr::empty();
    let mut row = occurrence("test:model:occurrence#link", None, 0);
    row.link = crate::products::LinkState::new(Vec::new(), Some("test:model:product#missing".try_into().unwrap()), None, None);
    ir.model.occurrences.push(row);
    refuses(&ir);
}

#[test]
fn product_copy_source_and_group_validation_preserves_all_refusals() {
    for source in [true, false] {
        let mut ir = CadIr::empty();
        let mut row = occurrence("test:model:occurrence#copy", None, 0);
        let reference = PrototypeReference::Local { definition: "test:model:product#missing".try_into().unwrap() };
        row.link = crate::products::LinkState::new(Vec::new(), None, None, Some(crate::products::CopyOnChange {
            policy: crate::products::CopyOnChangePolicy::Enabled,
            source: source.then(|| reference.clone()), group: (!source).then_some(reference), touched: None,
        }));
        ir.model.occurrences.push(row);
        refuses(&ir);
    }
}

#[test]
fn product_joint_validation_preserves_index_walk_and_finding_refusals() {
    let mut ir = CadIr::empty();
    ir.model.occurrences.push(occurrence("test:model:occurrence#present", None, 0));
    missing_joint(&mut ir);
    refuses(&ir);
}

#[test]
fn product_sibling_orders_distinguish_parent_groups_and_release_scopes() {
    let mut ir = CadIr::empty();
    ir.model.occurrences = vec![occurrence("test:model:occurrence#a", None, 0),
        occurrence("test:model:occurrence#b", None, 1),
        occurrence("test:model:occurrence#first", Some("test:model:occurrence#a"), 0),
        occurrence("test:model:occurrence#other", Some("test:model:occurrence#b"), 0)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 32768;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(ctx.reserve_scoped(32768, "product scopes released").unwrap());
    ctx.finish_session().unwrap();
    ir.model.occurrences.push(occurrence("test:model:occurrence#duplicate", Some("test:model:occurrence#a"), 0));
    let ctx = cadmpeg_test_support::service_decode_context();
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].entity.as_deref(), Some("test:model:occurrence#duplicate"));
}

#[test]
fn product_validation_keeps_definition_graph_occurrence_joint_finding_order() {
    let mut ir = CadIr::empty();
    missing_body(&mut ir);
    let mut row = occurrence("test:model:occurrence#bad", Some("test:model:occurrence#absent"), 0);
    row.prototype = PrototypeReference::Local { definition: "test:model:product#absent".try_into().unwrap() };
    ir.model.occurrences.push(row);
    missing_joint(&mut ir);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings: Vec<Finding> = Vec::new();
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(findings.iter().map(|finding| finding.entity.as_deref()).collect::<Vec<_>>(),
        [Some("test:model:product#definition"), Some("model:assembly"), Some("test:model:occurrence#bad"), Some("test:model:joint#missing")]);
    assert_eq!(findings.iter().map(|finding| finding.message.as_str()).collect::<Vec<_>>(),
        ["invalid product body reference", "invalid occurrence parent graph", "invalid occurrence reference, ordinal, or affine transform", "invalid assembly joint operands, frames, or limits"]);
}
