// SPDX-License-Identifier: Apache-2.0
use super::*;

fn value(order: usize, parent: Option<usize>) -> ValueRecord {
    ValueRecord {
        item_index: None,
        link_index: None,
        tag: "String".into(),
        order,
        parent,
        attributes: Default::default(),
        decoded_json: None,
        text: Some("held".into()),
    }
}

#[test]
fn null_positions_and_trailing_absence_roundtrip_without_placeholder_records() {
    let values = ValueSlots::new(vec![value(0, None), value(2, Some(0))], 4).unwrap();
    let wire = serde_json::to_value(&values).unwrap();
    assert!(wire[1].is_null());
    assert!(wire[3].is_null());
    assert_eq!(wire.as_array().unwrap().len(), 4);
    let decoded: ValueSlots = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded, values);
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[1].order, 2);
    assert_eq!(decoded.slot_count(), 4);
}

#[test]
fn missing_parent_and_wrong_wire_position_are_rejected() {
    assert!(ValueSlots::new(vec![value(2, Some(1))], 3).is_err());
    assert!(ValueSlots::new(vec![value(2, None), value(1, None)], 3).is_err());
    assert!(serde_json::from_value::<ValueSlots>(serde_json::json!([value(2, None)])).is_err());
}

#[test]
fn source_xlink_indices_are_optional_unique_and_bounded_by_their_actual_container() {
    let mut root = value(0, None);
    root.tag = "XLinks".into();
    root.attributes.insert("count".into(), "3".into());
    let mut first = value(1, Some(0));
    first.tag = "XLink".into();
    first.link_index = Some(0);
    let mut last = value(3, Some(0));
    last.tag = "XLink".into();
    last.link_index = Some(2);
    let slots = ValueSlots::new(vec![root.clone(), first.clone(), last.clone()], 4).unwrap();
    let wire = serde_json::to_value(&slots).unwrap();
    assert!(wire[2].is_null());
    assert_eq!(wire[3]["link_index"], 2);
    assert!(wire[0].get("link_index").is_none());
    assert_eq!(serde_json::from_value::<ValueSlots>(wire).unwrap(), slots);
    last.link_index = Some(0);
    assert!(ValueSlots::new(vec![root.clone(), first.clone(), last.clone()], 4).is_err());
    last.link_index = Some(3);
    assert!(ValueSlots::new(vec![root.clone(), first.clone(), last], 4).is_err());
    first.tag = "Cell".into();
    assert!(ValueSlots::new(vec![root, first], 2).is_err());
}

#[test]
fn enum_item_indices_are_semantic_sibling_positions_with_null_peers() {
    let mut root = value(0, None);
    root.tag = "CustomEnumList".into();
    root.attributes.insert("count".into(), "3".into());
    let mut first = value(1, Some(0));
    first.tag = "Enum".into();
    first.item_index = Some(0);
    let mut last = value(3, Some(0));
    last.tag = "Enum".into();
    last.item_index = Some(2);
    let held = ValueSlots::new(vec![root.clone(), first.clone(), last.clone()], 4).unwrap();
    let wire = serde_json::to_value(&held).unwrap();
    assert!(wire[2].is_null());
    assert_eq!(wire[3]["item_index"], 2);
    assert_eq!(serde_json::from_value::<ValueSlots>(wire).unwrap(), held);
    last.item_index = Some(0);
    assert!(ValueSlots::new(vec![root.clone(), first.clone(), last.clone()], 4).is_err());
    last.item_index = Some(3);
    assert!(ValueSlots::new(vec![root.clone(), first.clone(), last.clone()], 4).is_err());
    last.item_index = Some(2);
    last.tag = "String".into();
    assert!(ValueSlots::new(vec![root, first, last], 4).is_err());
}
