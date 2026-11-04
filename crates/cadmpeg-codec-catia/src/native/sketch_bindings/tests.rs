#![allow(clippy::unwrap_used)]
use super::bind;
use crate::native::{CatiaDesignObject, CatiaNative, CatiaObjectRecord};
use crate::test_support::test_sketch_bindings::{
    finite_pair, positive_sketch_bindings_source, roster, sketch_bindings_source,
};

fn group(native: &CatiaNative, owner: u32) -> &CatiaDesignObject {
    native
        .design_objects
        .iter()
        .find(|g| g.owner_entity_id == owner)
        .expect("authored group")
}

fn record_mut(native: &mut CatiaNative, identity: u32) -> &mut CatiaObjectRecord {
    native
        .object_graphs
        .iter_mut()
        .flat_map(|g| &mut g.records)
        .find(|r| r.entity_id() == Some(identity))
        .expect("authored object")
}

fn rebind(native: &mut CatiaNative) {
    crate::test_support::with_service_context(|ctx| {
        bind(
            ctx,
            &native.object_graphs,
            &native.entity_records,
            &mut native.design_objects,
        )
    })
    .expect("source-derived bindings");
}

#[test]
fn sketch_bindings_preserve_roster_occurrences_groups_children_and_tail() {
    let native = CatiaNative::decode(&positive_sketch_bindings_source());
    let membership = group(&native, 1)
        .sketch_membership
        .as_ref()
        .expect("counted roster");
    assert_eq!(membership.declared_count, 4);
    assert_eq!(membership.list_payload_offset, 0);
    assert_eq!(
        membership
            .items
            .iter()
            .map(|r| (r.ordinal, r.payload_offset, r.entity_id))
            .collect::<Vec<_>>(),
        [(0, 2, 2), (1, 4, 2), (2, 6, 3), (3, 8, 77)]
    );
    assert_eq!(membership.items[0].target, membership.items[1].target);
    assert!(membership.items[3].target.is_none());
    assert_eq!(membership.groups, [group(&native, 5).id.clone()]);
    assert_eq!(group(&native, 5).fields.len(), 2);
    assert_eq!(
        membership.children,
        [group(&native, 2).id.clone(), group(&native, 3).id.clone()]
    );
    assert_eq!(group(&native, 3).fields.len(), 2);
    let source = native
        .entity_records
        .iter()
        .find(|e| e.entity_id == 1)
        .expect("owner");
    assert_eq!(membership.source_entity, source.id);
    assert_eq!(membership.source_object, source.object_record);
    let owner = native
        .object_graphs
        .iter()
        .flat_map(|g| &g.records)
        .find(|r| r.entity_id() == Some(1))
        .expect("source object");
    assert!(owner.references.iter().any(|r| r.entity_id() == 4));
    assert!(!membership.items.iter().any(|r| r.entity_id == 4));
}

#[test]
fn sketch_bindings_require_complete_initial_reference_list() {
    let value = finite_pair(1.25_f64.to_bits(), 0.0_f64.to_bits());
    for list in [
        vec![0xfe],
        vec![0x3b, 0x84, 0x81, 0x82, 0xfe],
        vec![0x3b, 0x81, 0x80, 0x82, 0xfe],
        vec![0x81, 0x83, 0x3b, 0x81, 0x81, 0x82, 0xfe],
    ] {
        let native = CatiaNative::decode(&sketch_bindings_source(
            &list, &value, false, false, "2DPoint",
        ));
        assert!(group(&native, 1).sketch_membership.is_none(), "{list:?}");
    }
    let native = CatiaNative::decode(&sketch_bindings_source(
        &[0x3b, 0x80, 0xfe],
        &value,
        false,
        false,
        "2DPoint",
    ));
    let membership = group(&native, 1)
        .sketch_membership
        .as_ref()
        .expect("complete empty roster");
    assert_eq!(membership.declared_count, 0);
    assert!(membership.items.is_empty());
}

#[test]
fn sketch_bindings_require_exact_owner_declaration() {
    let value = finite_pair(1.25_f64.to_bits(), 0.0_f64.to_bits());
    let native = CatiaNative::decode(&sketch_bindings_source(
        &roster(),
        &value,
        true,
        false,
        "2DPoint",
    ));
    assert!(group(&native, 1).sketch_membership.is_none());
    for control in [
        "class-name",
        "class-entry",
        "owner-record",
        "head-owner",
        "head-class",
    ] {
        let mut native = CatiaNative::decode(&positive_sketch_bindings_source());
        match control {
            "class-name" => {
                record_mut(&mut native, 1)
                    .class
                    .as_mut()
                    .expect("class")
                    .name = Some("PRTSketch".to_owned());
            }
            "class-entry" => {
                record_mut(&mut native, 1)
                    .class
                    .as_mut()
                    .expect("class")
                    .entry = None;
            }
            "owner-record" => {
                native
                    .design_objects
                    .iter_mut()
                    .find(|g| g.owner_entity_id == 1)
                    .expect("group")
                    .owner_record = None;
            }
            "head-owner" => {
                record_mut(&mut native, 1).head[2] = crate::object_graph::HeadToken::Reference(77);
            }
            "head-class" => {
                record_mut(&mut native, 1).head[3] = crate::object_graph::HeadToken::Reference(5);
            }
            _ => unreachable!("named control"),
        }
        rebind(&mut native);
        assert!(group(&native, 1).sketch_membership.is_none(), "{control}");
    }
}

#[test]
fn sketch_bindings_require_unique_pairs_and_complete_groups() {
    for control in [
        "source-missing",
        "source-duplicate",
        "source-graph",
        "source-pair",
        "source-object-duplicate",
        "target-missing",
        "target-duplicate",
        "target-graph",
        "target-pair",
        "target-object-duplicate",
        "group-duplicate",
        "group-omitted-field",
        "group-foreign-field",
    ] {
        let mut native = CatiaNative::decode(&positive_sketch_bindings_source());
        let identity = if control.starts_with("source-") { 1 } else { 2 };
        let index = native
            .entity_records
            .iter()
            .position(|e| e.entity_id == identity)
            .expect("entity");
        match control {
            "source-missing" | "target-missing" => {
                native.entity_records.remove(index);
            }
            "source-duplicate" | "target-duplicate" => native
                .entity_records
                .push(native.entity_records[index].clone()),
            "source-graph" | "target-graph" => {
                native.entity_records[index].object_graph.push_str(":other");
            }
            "source-pair" | "target-pair" => native.entity_records[index]
                .object_record
                .push_str(":other"),
            "source-object-duplicate" | "target-object-duplicate" => {
                let duplicate = record_mut(&mut native, identity).clone();
                native.object_graphs[0].records.push(duplicate);
            }
            "group-duplicate" => {
                let mut duplicate = group(&native, 5).clone();
                duplicate.id.push_str(":other");
                native.design_objects.push(duplicate);
            }
            "group-omitted-field" => {
                native
                    .design_objects
                    .iter_mut()
                    .find(|g| g.owner_entity_id == 5)
                    .expect("group")
                    .fields
                    .pop();
            }
            "group-foreign-field" => {
                native
                    .design_objects
                    .iter_mut()
                    .find(|g| g.owner_entity_id == 5)
                    .expect("group")
                    .fields[0]
                    .push_str(":other");
            }
            _ => unreachable!("named control"),
        }
        rebind(&mut native);
        let membership = group(&native, 1).sketch_membership.as_ref();
        if control.starts_with("source-") {
            assert!(membership.is_none(), "{control}");
        } else {
            let membership = membership.expect("source roster is independent of a target refusal");
            assert!(
                membership
                    .items
                    .iter()
                    .filter(|r| r.entity_id == 2)
                    .all(|r| r.target.is_none()),
                "{control}"
            );
        }
    }
}

#[test]
fn sketch_bindings_keep_group_when_child_ownership_is_ambiguous() {
    let mut native = CatiaNative::decode(&positive_sketch_bindings_source());
    let mut duplicate = group(&native, 2).clone();
    duplicate.id.push_str(":duplicate");
    native.design_objects.push(duplicate);
    rebind(&mut native);
    let membership = group(&native, 1)
        .sketch_membership
        .as_ref()
        .expect("source roster");
    assert_eq!(membership.groups, [group(&native, 5).id.clone()]);
    for item in membership.items.iter().filter(|r| r.entity_id == 2) {
        assert!(item
            .target
            .as_ref()
            .expect("containing group stays bound")
            .child
            .is_none());
    }
    assert_eq!(membership.children, [group(&native, 3).id.clone()]);
}

#[test]
fn local_point_pairs_preserve_bits_slots_and_shared_owners() {
    let native = CatiaNative::decode(&positive_sketch_bindings_source());
    let pairs = &group(&native, 3).local_point_pairs;
    assert_eq!(pairs.len(), 2);
    assert_ne!(pairs[0].point_entity, pairs[1].point_entity);
    assert_ne!(pairs[0].point_object, pairs[1].point_object);
    assert_eq!(pairs[0].coordinate_entity, pairs[1].coordinate_entity);
    let owner = native
        .entity_records
        .iter()
        .find(|e| e.entity_id == 3)
        .expect("coordinate owner");
    for (pair, identity) in pairs.iter().zip([4, 7]) {
        let point = native
            .entity_records
            .iter()
            .find(|e| e.entity_id == identity)
            .expect("point");
        assert_eq!(pair.point_entity, point.id);
        assert_eq!(pair.point_object, point.object_record);
        assert_eq!(pair.coordinate_entity, owner.id);
        assert_eq!(pair.coordinate_object, owner.object_record);
        assert_eq!(
            pair.slots
                .iter()
                .map(|s| (s.bits, s.payload_offset))
                .collect::<Vec<_>>(),
            [(1.25_f64.to_bits(), 8), ((-0.0_f64).to_bits(), 17)]
        );
        assert!(point.numeric_pair().is_none());
    }
}

#[test]
fn local_point_pairs_require_complete_finite_owner_values() {
    let finite = finite_pair(1.25_f64.to_bits(), 0.0_f64.to_bits());
    let mut nullable = finite[..17].to_vec();
    nullable.extend([0xe8, 0xfe, 0xfe]);
    let mut trailing = finite.clone();
    trailing.push(0x80);
    for value in [
        vec![0xfe],
        finite[..finite.len() - 1].to_vec(),
        trailing,
        nullable,
        finite_pair(f64::NAN.to_bits(), 0.0_f64.to_bits()),
        finite_pair(1.25_f64.to_bits(), f64::INFINITY.to_bits()),
        finite_pair(f64::NEG_INFINITY.to_bits(), 0.0_f64.to_bits()),
    ] {
        let mut native = CatiaNative::decode(&sketch_bindings_source(
            &roster(),
            &value,
            false,
            false,
            "2DPoint",
        ));
        let point = native
            .entity_records
            .iter_mut()
            .find(|e| e.entity_id == 4)
            .expect("point");
        if let crate::native::entity_record::CatiaEntityRecordBody::Nested {
            value_payload, ..
        } = &mut point.body
        {
            *value_payload = finite.clone();
        }
        rebind(&mut native);
        assert!(group(&native, 3).local_point_pairs.is_empty(), "{value:?}");
    }
}

#[test]
fn local_point_pairs_reject_compact_or_unresolved_class_aliases() {
    let value = finite_pair(1.25_f64.to_bits(), 0.0_f64.to_bits());
    for (compact, name) in [(true, "2DPoint"), (false, "Point"), (false, "2DPointAlias")] {
        let native = CatiaNative::decode(&sketch_bindings_source(
            &roster(),
            &value,
            false,
            compact,
            name,
        ));
        assert!(group(&native, 3).local_point_pairs.is_empty());
    }
    for identity in [4, 7] {
        let mut native = CatiaNative::decode(&positive_sketch_bindings_source());
        record_mut(&mut native, identity)
            .class
            .as_mut()
            .expect("class")
            .entry = None;
        rebind(&mut native);
        assert_eq!(group(&native, 3).local_point_pairs.len(), 1);
    }
}

#[test]
fn local_point_pairs_require_unique_same_graph_ownership() {
    for control in [
        "point-missing",
        "point-duplicate",
        "point-graph",
        "point-pair",
        "point-object-duplicate",
        "owner-missing",
        "owner-duplicate",
        "owner-graph",
        "owner-pair",
        "owner-object-duplicate",
        "group-owner-record",
        "group-fields",
        "group-duplicate",
        "head-owner",
    ] {
        let mut native = CatiaNative::decode(&positive_sketch_bindings_source());
        let rejected_point = native
            .entity_records
            .iter()
            .find(|e| e.entity_id == 4)
            .expect("point")
            .id
            .clone();
        let identity = if control.starts_with("owner-") { 3 } else { 4 };
        let index = native
            .entity_records
            .iter()
            .position(|e| e.entity_id == identity)
            .expect("entity");
        match control {
            "point-missing" | "owner-missing" => {
                native.entity_records.remove(index);
            }
            "point-duplicate" | "owner-duplicate" => native
                .entity_records
                .push(native.entity_records[index].clone()),
            "point-graph" | "owner-graph" => {
                native.entity_records[index].object_graph.push_str(":other");
            }
            "point-pair" | "owner-pair" => native.entity_records[index]
                .object_record
                .push_str(":other"),
            "point-object-duplicate" | "owner-object-duplicate" => {
                let duplicate = record_mut(&mut native, identity).clone();
                native.object_graphs[0].records.push(duplicate);
            }
            "group-owner-record" => {
                native
                    .design_objects
                    .iter_mut()
                    .find(|g| g.owner_entity_id == 3)
                    .expect("group")
                    .owner_record = None;
            }
            "group-fields" => {
                native
                    .design_objects
                    .iter_mut()
                    .find(|g| g.owner_entity_id == 3)
                    .expect("group")
                    .fields
                    .pop();
            }
            "group-duplicate" => {
                let mut duplicate = group(&native, 3).clone();
                duplicate.id.push_str(":other");
                native.design_objects.push(duplicate);
            }
            "head-owner" => {
                record_mut(&mut native, 4).head[2] = crate::object_graph::HeadToken::Reference(77);
            }
            _ => unreachable!("named control"),
        }
        rebind(&mut native);
        assert!(
            group(&native, 3)
                .local_point_pairs
                .iter()
                .all(|p| p.point_entity != rejected_point),
            "{control}"
        );
        let expected_max = usize::from(control.starts_with("point-") || control == "head-owner");
        assert!(
            group(&native, 3).local_point_pairs.len() <= expected_max,
            "{control}"
        );
    }
}

#[test]
fn sketch_bindings_roundtrip_and_reject_forged_native_metadata() {
    let native = CatiaNative::decode(&positive_sketch_bindings_source());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store Native");
    assert_eq!(
        CatiaNative::load(&namespace)
            .expect("typed test roundtrip")
            .design_objects,
        native.design_objects
    );
    for control in [
        "omit-membership",
        "item-offset",
        "group-summary",
        "omit-pairs",
        "slot-bits",
        "point-owner",
    ] {
        let mut changed = native.clone();
        for g in &mut changed.design_objects {
            if g.owner_entity_id == 1 {
                match control {
                    "omit-membership" => g.sketch_membership = None,
                    "item-offset" => {
                        g.sketch_membership.as_mut().expect("membership").items[0]
                            .payload_offset += 1;
                    }
                    "group-summary" => g
                        .sketch_membership
                        .as_mut()
                        .expect("membership")
                        .groups
                        .clear(),
                    _ => (),
                }
            }
            if g.owner_entity_id == 3 {
                match control {
                    "omit-pairs" => g.local_point_pairs.clear(),
                    "slot-bits" => g.local_point_pairs[0].slots[0].bits ^= 1,
                    "point-owner" => g.local_point_pairs[0].coordinate_entity.push_str(":other"),
                    _ => (),
                }
            }
        }
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        changed
            .store(&mut namespace)
            .expect("store forged test view");
        assert!(CatiaNative::load(&namespace).is_err(), "{control}");
    }
}

#[test]
fn sketch_bindings_charge_retained_metadata_before_copying() {
    let mut native = CatiaNative::decode(&positive_sketch_bindings_source());
    let error = crate::test_support::with_retained_limit(0, |ctx| {
        bind(
            ctx,
            &native.object_graphs,
            &native.entity_records,
            &mut native.design_objects,
        )
    })
    .expect_err("source metadata needs retained space");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
        && limit.operation == "retain CATIA sketch source bindings")
    );
}

#[test]
fn sketch_bindings_abstain_without_eligible_source_declarations() {
    let mut native = CatiaNative::decode(&sketch_bindings_source(
        &roster(),
        &[0xfe],
        true,
        true,
        "2DPoint",
    ));
    crate::test_support::with_retained_limit(0, |ctx| {
        bind(
            ctx,
            &native.object_graphs,
            &native.entity_records,
            &mut native.design_objects,
        )
    })
    .expect("no admitted source metadata");
    assert!(native
        .design_objects
        .iter()
        .all(|g| g.sketch_membership.is_none() && g.local_point_pairs.is_empty()));
}

#[test]
fn sketch_target_optional_keys_have_one_absence_spelling() {
    let target = serde_json::json!({"entity":"entity", "object":"object", "group":"group"});
    let parsed: super::CatiaSketchMemberTarget = serde_json::from_value(target.clone()).unwrap();
    assert!(parsed.child.is_none());
    let mut stated = target.clone();
    stated["child"] = serde_json::json!("child");
    assert!(serde_json::from_value::<super::CatiaSketchMemberTarget>(stated.clone()).is_ok());
    stated["child"] = serde_json::Value::Null;
    assert!(
        serde_json::from_value::<super::CatiaSketchMemberTarget>(stated)
            .unwrap_err()
            .to_string()
            .contains("child")
    );
    let mut member = serde_json::json!({"ordinal":0,"payload_offset":0,"entity_id":1});
    assert!(
        serde_json::from_value::<super::CatiaSketchMember>(member.clone())
            .unwrap()
            .target
            .is_none()
    );
    member["target"] = target;
    assert!(serde_json::from_value::<super::CatiaSketchMember>(member.clone()).is_ok());
    member["target"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<super::CatiaSketchMember>(member)
        .unwrap_err()
        .to_string()
        .contains("target"));
}
