// SPDX-License-Identifier: Apache-2.0
use crate::records::feature::chamfer_set::{DesignChamferScope, DesignChamferSetList};

fn wire(entry_count: u32, sets: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({"form": "current", "corner_type": "miter", "entry_count": entry_count, "sets": sets})
}

fn set() -> serde_json::Value {
    serde_json::json!({
        "record_index": 290,
        "byte_offset": 34500,
        "frame_length": 93,
        "group_record_index": 283,
        "spec": {"kind": "distance_angle", "distance_owner_record_index": 292, "angle_owner_record_index": 295},
        "auxiliary_record_index": 297,
        "entry_count": 1,
        "tangent_chain": true
    })
}

#[test]
fn chamfer_set_list_round_trips_and_refuses_empty_runs_and_unknown_keys() {
    let value = wire(1, &serde_json::json!([set()]));
    let list: DesignChamferSetList = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&list).unwrap(), value);
    assert!(
        serde_json::from_value::<DesignChamferSetList>(wire(1, &serde_json::json!([]))).is_err()
    );
    assert!(
        serde_json::from_value::<DesignChamferSetList>(wire(0, &serde_json::json!([set()])))
            .is_err()
    );
    let mut extra = set();
    extra["distance"] = serde_json::json!(1.0);
    assert!(
        serde_json::from_value::<DesignChamferSetList>(wire(1, &serde_json::json!([extra])))
            .is_err()
    );
}

#[test]
fn a_chamfer_set_list_retains_available_fixed_lanes() {
    let list: DesignChamferSetList =
        serde_json::from_value(wire(1, &serde_json::json!([set()]))).unwrap();
    let fixed = || {
        serde_json::from_value(serde_json::json!({
            "kind": "equal_distance",
            "distance": {"value": 0.2, "record_index": 202, "value_offset": 2020}
        }))
        .unwrap()
    };
    let both = DesignChamferScope::admit(Some(fixed()), Some(list.clone()));
    assert!(
        matches!(both.fixed, Some(crate::records::feature::fixed_parameters::DesignFixedChamferParameters::EqualDistance { distance }) if (distance.value.get() - 0.2).abs() < 1e-12)
    );
    assert_eq!(both.sets, Some(list));
    assert!(DesignChamferScope::admit(Some(fixed()), None)
        .fixed
        .is_some());
}
