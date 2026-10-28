// SPDX-License-Identifier: Apache-2.0

use super::{model_dimensions, ModelDimension, ANGULAR_DIMENSION_TYPE};

fn with_label(elements: &[u8]) -> Vec<u8> {
    let mut payload = b"\xe0\x00dim_array\0\xf8\x04\xf7\x02\xfb\xe1".to_vec();
    payload.extend_from_slice(elements);
    payload
}

#[test]
fn reads_detail_item_and_plain_element_heads() {
    let elements = b"\xe3\xf7\x03\xe2\xe2\xe1\x08\x15\x82\x9b\x81\x81\x20\xe3\x2f\x76\x80\x2f\x76\x80\xe3\x09\
        \xe3\x08\x02\x80\xf7\xf5\xeb\x7f\xff\xfe\xdf\xeb\x40\x00\x00\x20\xe3\x2f\x50\xc0\x2f\x50\xc0\xe3";
    let payload = with_label(elements);
    let first = payload
        .iter()
        .position(|&b| b == 0x08)
        .expect("first element type");
    let second = payload[first + 1..]
        .windows(2)
        .position(|w| w == [0xe3, 0x08])
        .expect("second element opener")
        + first
        + 2;

    assert_eq!(
        model_dimensions(&payload),
        vec![
            ModelDimension {
                dimension_type: ANGULAR_DIMENSION_TYPE,
                feature_id: 667,
                value: Some(360.0),
                offset: first,
            },
            ModelDimension {
                dimension_type: 2,
                feature_id: 247,
                value: Some(67.0),
                offset: second,
            },
        ]
    );
}

#[test]
fn class_referenced_element_and_three_byte_attribute_are_read() {
    let payload = with_label(b"\xe3\xf7\x08\x08\x0a\x8a\xbf\x81\x21\xc0\x80\x20\xe3\x2f\x3f\x00");
    let dimensions = model_dimensions(&payload);
    assert_eq!(dimensions.len(), 1);
    assert_eq!(dimensions[0].dimension_type, 10);
    assert_eq!(dimensions[0].feature_id, 2751);
    assert_eq!(dimensions[0].value, Some(31.0));
}

#[test]
fn element_heads_require_the_dimension_array_and_an_element_opener() {
    let head = b"\x08\x15\x82\x9b\x81\x81\x20\xe3\x2f\x76\x80";
    let mut before_label = b"\xe3".to_vec();
    before_label.extend_from_slice(head);
    before_label.extend_from_slice(b"\xe0\x00dim_array\0");
    assert!(model_dimensions(&before_label).is_empty());

    let mut unopened = with_label(b"\x00");
    unopened.extend_from_slice(head);
    assert!(model_dimensions(&unopened).is_empty());
}

#[test]
fn non_short_form_value_remains_unresolved() {
    let payload =
        with_label(b"\xe3\x08\x01\x82\x9b\x81\x01\x20\xe3\x2d\x1e\xae\x14\x7a\xe1\x47\xae");
    let dimensions = model_dimensions(&payload);
    assert_eq!(dimensions.len(), 1);
    assert_eq!(dimensions[0].value, None);
}
