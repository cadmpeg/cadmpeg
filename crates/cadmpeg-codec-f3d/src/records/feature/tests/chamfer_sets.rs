// SPDX-License-Identifier: Apache-2.0
use crate::records::feature::chamfer_set::DesignChamferSetList;
use cadmpeg_core::serde_erase::json;

fn wire(entry_count: u32, sets: &serde_json::Value) -> serde_json::Value {
    cadmpeg_core::json!({"form": "current", "corner_type": "miter", "entry_count": entry_count, "sets": sets})
}

fn set() -> serde_json::Value {
    cadmpeg_core::json!({
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
    let value = wire(1, &cadmpeg_core::json!([set()]));
    let list: DesignChamferSetList = json::from_value(value.clone()).unwrap();
    assert_eq!(json::to_value(&list).unwrap(), value);
    assert!(json::from_value::<DesignChamferSetList>(wire(1, &cadmpeg_core::json!([]))).is_err());
    assert!(
        json::from_value::<DesignChamferSetList>(wire(0, &cadmpeg_core::json!([set()]))).is_err()
    );
    let mut extra = set();
    extra["distance"] = cadmpeg_core::json!(1.0);
    assert!(
        json::from_value::<DesignChamferSetList>(wire(1, &cadmpeg_core::json!([extra]))).is_err()
    );
}

#[test]
fn distance_angle_reversal_round_trips_and_defaults_to_false() {
    for reversed in [false, true] {
        let mut value = set();
        if reversed {
            value["distance_reversed"] = cadmpeg_core::json!(true);
        }
        let value = wire(1, &cadmpeg_core::json!([value]));
        let list: DesignChamferSetList = json::from_value(value.clone()).unwrap();
        assert_eq!(list.sets[0].distance_reversed, reversed);
        assert_eq!(json::to_value(&list).unwrap(), value);
    }
}
