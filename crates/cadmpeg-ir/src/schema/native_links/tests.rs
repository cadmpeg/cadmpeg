// SPDX-License-Identifier: Apache-2.0
use super::{linking_record, visit_native_references};
use crate::appearance::AppearanceTarget;
use crate::attributes::AttributeTarget;
use crate::document::ArenaName;

#[test]
fn native_links_use_borrowed_registered_records_without_schema_generation() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let native = "test:native:object#Body";
    ir.model.bodies[0].native_ref = Some(native.into());
    let body = &ir.model.bodies[0];
    let mut count = 0;
    let mut found = false;
    visit_native_references(&ir.model, &mut |arena, owner, id, reference| {
        count += 1;
        if arena == "bodies" {
            assert_eq!(owner, std::any::type_name_of_val(body));
            assert_eq!(id, body.id.as_str());
            assert_eq!(reference, Some(native));
            assert_eq!(id.as_ptr(), body.id.as_str().as_ptr());
            assert_eq!(
                reference.unwrap().as_ptr(),
                body.native_ref.as_deref().unwrap().as_ptr()
            );
            found = true;
        }
    });
    assert!(found);
    assert_eq!(count, ir.model.entity_count());
}

#[test]
fn linking_record_prefers_model_class_then_registry_order_then_id() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let owner = "test:native:object#1";
    ir.model.bodies[0].native_ref = Some(owner.into());
    ir.model.faces[0].native_ref = Some(owner.into());
    ir.model.faces[1].native_ref = Some(owner.into());
    let (arena, id) = linking_record(&ir.model, owner).expect("linked owner resolves");
    assert_eq!(arena, ArenaName::parse("bodies").unwrap());
    assert_eq!(id, ir.model.bodies[0].id.as_str());
    ir.model.bodies[0].native_ref = None;
    let (arena, id) = linking_record(&ir.model, owner).expect("linked owner resolves");
    assert_eq!(arena, ArenaName::parse("faces").unwrap());
    let first = ir.model.faces[0]
        .id
        .as_str()
        .min(ir.model.faces[1].id.as_str());
    assert_eq!(id, first);
    assert!(linking_record(&ir.model, "test:native:object#absent").is_none());
}

#[test]
fn record_targets_name_the_first_linked_neutral_record() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let owner = "test:native:object#2";
    ir.model.faces[0].native_ref = Some(owner.into());
    let AttributeTarget::Record { arena, id } =
        AttributeTarget::record_for(&ir.model, owner).expect("linked owner resolves")
    else {
        panic!("attribute target names a neutral record");
    };
    assert_eq!(arena, ArenaName::parse("faces").unwrap());
    assert_eq!(id, ir.model.faces[0].id.as_str());
    let AppearanceTarget::Record { arena, id } =
        AppearanceTarget::record_for(&ir.model, owner).expect("linked owner resolves")
    else {
        panic!("appearance target names a neutral record");
    };
    assert_eq!(arena, ArenaName::parse("faces").unwrap());
    assert_eq!(id, ir.model.faces[0].id.as_str());
    assert!(AttributeTarget::record_for(&ir.model, "test:native:object#absent").is_none());
    assert!(AppearanceTarget::record_for(&ir.model, "test:native:object#absent").is_none());
}

#[test]
fn input_operand_links_resolve_to_the_containing_topology() {
    use crate::features::{
        FeatureId, FeatureInputOperand, FeatureInputOperandDefinition, FeatureInputTopology,
        SelectionReference,
    };
    use crate::ids::{FeatureInputOperandId, FeatureInputTopologyId};

    let mut model = crate::document::Model::default();
    let operand_id = FeatureInputOperandId::mint("test:model:input-operand#one").unwrap();
    let topology_id = FeatureInputTopologyId::mint("test:model:input-topology#one").unwrap();
    let native = "test:native:operand#one";
    model.feature_input_topologies.push(FeatureInputTopology {
        id: topology_id.clone(),
        input_of: FeatureId::mint("test:model:feature#one").unwrap(),
        bodies: crate::features::DistinctMembers::default(),
        faces: crate::features::DistinctMembers::default(),
        edges: crate::features::DistinctMembers::default(),
        vertices: crate::features::DistinctMembers::default(),
        operands: vec![FeatureInputOperand {
            id: operand_id.clone(),
            definition: FeatureInputOperandDefinition::Native,
            native_ref: SelectionReference::new(native.into()).unwrap(),
        }]
        .try_into()
        .unwrap(),
        edge_geometry: Vec::new(),
        native_ref: None,
    });
    let mut links = Vec::new();
    visit_native_references(&model, &mut |_, _, id, target| {
        if target == Some(native) {
            links.push(id.to_owned());
        }
    });
    assert_eq!(links, [operand_id.to_string()]);
    assert_eq!(
        linking_record(&model, native),
        Some((
            ArenaName::parse("feature_input_topologies").unwrap(),
            topology_id.to_string()
        ))
    );
}
