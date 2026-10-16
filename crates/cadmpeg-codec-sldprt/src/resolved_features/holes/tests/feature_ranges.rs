//! Native feature-object range tests.

use super::{lane_with_position_reference, native_history};
use crate::resolved_features::holes::feature_object_byte_ranges;

#[test]
fn feature_object_ranges_preserve_named_object_bounds() {
    let histories = [native_history()];
    let lane = lane_with_position_reference(7);
    let ranges = feature_object_byte_ranges(&histories, &lane);
    assert_eq!(
        ranges.get("native-hole"),
        Some(&(0, 0, lane.native_payload.len()))
    );
}

#[test]
fn feature_object_ranges_preserve_equal_offset_history_order() {
    let mut history = native_history();
    let mut second = history.features[0].clone();
    second.id = "second-hole".into();
    history.features.push(second);
    let lane = lane_with_position_reference(7);
    let histories = [history];
    let ranges = feature_object_byte_ranges(&histories, &lane);
    assert_eq!(ranges.get("native-hole"), Some(&(0, 0, 0)));
    assert_eq!(
        ranges.get("second-hole"),
        Some(&(0, 0, lane.native_payload.len()))
    );
}
