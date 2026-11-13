// SPDX-License-Identifier: Apache-2.0
use crate::records::feature::fillet_set::DesignFilletSetList;

fn wire(sets: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({"form": "compact", "rolling_ball_corner": true, "sets": sets})
}

fn set() -> serde_json::Value {
    serde_json::json!({
        "record_index": 283,
        "byte_offset": 35020,
        "frame_length": 93,
        "group_record_index": 284,
        "law": {"kind": "chordal", "chord_length_owner_record_index": 295},
        "tangency_weight_owner_record_index": 292,
        "tangent_chain": true
    })
}

#[test]
fn fillet_set_list_round_trips() {
    let value = wire(&serde_json::json!([set()]));
    let list: DesignFilletSetList = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&list).unwrap(), value);
}

#[test]
fn fillet_set_list_refuses_an_empty_set_run_and_unknown_keys() {
    assert!(serde_json::from_value::<DesignFilletSetList>(wire(&serde_json::json!([]))).is_err());
    let mut extra = set();
    extra["radius"] = serde_json::json!(1.0);
    assert!(
        serde_json::from_value::<DesignFilletSetList>(wire(&serde_json::json!([extra]))).is_err()
    );
}

#[test]
fn a_set_list_takes_precedence_over_fixed_lanes() {
    use crate::records::feature::fillet_set::DesignFilletScope;
    let list: DesignFilletSetList =
        serde_json::from_value(wire(&serde_json::json!([set()]))).unwrap();
    let fixed = serde_json::from_value(serde_json::json!({"groups": [{
        "radii": [0.2], "radius_offsets": [2020], "radius_record_indexes": [202]
    }]}))
    .unwrap();
    let both = DesignFilletScope::admit(Some(fixed), Some(list.clone()));
    assert!(both.fixed.is_none());
    assert_eq!(both.sets, Some(list));
    let fixed = serde_json::from_value(serde_json::json!({"groups": [{
        "radii": [0.2], "radius_offsets": [2020], "radius_record_indexes": [202]
    }]}))
    .unwrap();
    assert!(DesignFilletScope::admit(Some(fixed), None).fixed.is_some());
}
