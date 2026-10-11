// SPDX-License-Identifier: Apache-2.0
//! Allocation graph wire and native incidence invariants.

#[test]
fn allocation_bindings_reject_cross_record_and_side_substitution() {
    let bytes = crate::test_support::test_zero_entity::zero_entity_allocation_stream();
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.zero_entity_allocation_edges.len(), 3);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("native source graph");
    let roundtrip = crate::native::CatiaNative::load(&namespace).expect("valid complete graph");
    assert_eq!(
        roundtrip.zero_entity_allocation_edges,
        native.zero_entity_allocation_edges
    );
    for field in [
        "oriented_use",
        "support_record",
        "face_record",
        "incidence_record",
        "vertex_record",
    ] {
        let mut changed =
            serde_json::to_value(&native.zero_entity_allocation_edges[0]).expect("wire");
        changed["uses"][0][field] = serde_json::json!("catia:zero-entity:record#1");
        let mut mutant = native.clone();
        mutant.zero_entity_allocation_edges[0] =
            serde_json::from_value(changed).expect("closed wire");
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        mutant.store(&mut namespace).expect("store mutant");
        assert!(
            crate::native::CatiaNative::load(&namespace).is_err(),
            "{field}"
        );
    }
    let mut swapped = native.clone();
    swapped.zero_entity_allocation_edges[0].uses.swap(0, 1);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    swapped
        .store(&mut namespace)
        .expect("store swapped source sides");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn allocation_wire_requires_two_complete_sides_and_rejects_unknown_fields() {
    let native = crate::native::CatiaNative::decode(
        &crate::test_support::test_zero_entity::zero_entity_allocation_stream(),
    );
    let wire = serde_json::to_value(&native.zero_entity_allocation_edges[0]).expect("wire");
    for uses in [
        serde_json::json!([]),
        serde_json::json!([wire["uses"][0].clone()]),
        serde_json::json!([
            wire["uses"][0].clone(),
            wire["uses"][1].clone(),
            wire["uses"][1].clone()
        ]),
    ] {
        let mut changed = wire.clone();
        changed["uses"] = uses;
        assert!(serde_json::from_value::<super::CatiaZeroEntityAllocationEdge>(changed).is_err());
    }
    for side in [false, true] {
        let mut changed = wire.clone();
        if side {
            changed["uses"][0]["unknown"] = serde_json::json!(0);
        } else {
            changed["unknown"] = serde_json::json!(0);
        }
        assert!(serde_json::from_value::<super::CatiaZeroEntityAllocationEdge>(changed).is_err());
    }
}
