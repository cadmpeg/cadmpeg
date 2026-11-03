// SPDX-License-Identifier: Apache-2.0
use super::{
    dimensions, round_radius, ModelDimension, RoundDimensionRadius, ANGULAR_DIMENSION_TYPE,
};
use cadmpeg_ir::scalar::PositiveReal;

const HEADER: &[u8] =
    b"\xe0\x00Sld_BasFullData\x00\xe3\xe0\x00dim_array\x00\xf8\x06\xf7\x02\xfb\xe1\xe1";

/// The labeled first element: radius 3.0 of feature 0x59.
const PROTOTYPE: &[u8] =
    b"\xe3\xe0\x00dtl_named_item\x00\xe2\xe0\x00dtl_item\x00\xe2\xe0\x0aname\x00\xf1\xe1\
\xe0\x01type\x00\xf1\x08\xe0\x01dim_type\x00\x03\xe0\x01feat_id\x00\x59\
\xe0\x01attr_arr\x00\xf8\x03\xf5\xeb\x7f\xff\xfe\xdf\xeb\x40\x00\x00\x20\x02\
\xe0\x00dim_dat_ptr\x00\xe3\xe0\x02value\x00\x2f\x08\x00\xe0\x02bck_value\x00\x2f\x08\x00\xe3";

fn section(elements: &[&[u8]]) -> Vec<u8> {
    let mut bytes = HEADER.to_vec();
    for element in elements {
        bytes.extend_from_slice(element);
    }
    bytes
}

fn positive(value: f64) -> PositiveReal {
    PositiveReal::new(value).expect("positive fixture radius")
}

#[test]
fn prototype_and_positional_rows_share_field_order() {
    let bytes = section(&[
        PROTOTYPE,
        b"\xe2\xe2\xe1\x08\x3e\x64\x81\x81\x20\x02\xe3\x2f\x2e\x00\x2f\x2e\x00\xe3",
        b"\xe2\xe2\xe1\x08\x03\x88\xc5\x20\xe4\x02\xe3\x8b\x00\x00\x00\x00\x00\x00\xe3",
    ]);
    let rows = dimensions(&bytes);
    let prototype = HEADER.len();
    assert_eq!(
        rows,
        vec![
            ModelDimension {
                offset: prototype + 1,
                dim_type: 3,
                feature_id: Some(0x59),
                value: Some(3.0),
            },
            ModelDimension {
                offset: prototype + PROTOTYPE.len(),
                dim_type: 62,
                feature_id: Some(0x64),
                value: Some(15.0),
            },
            ModelDimension {
                offset: prototype + PROTOTYPE.len() + 18,
                dim_type: 3,
                feature_id: Some(0x8c5),
                value: Some(2.0),
            },
        ]
    );
}

#[test]
fn null_feature_and_three_byte_attribute_are_read() {
    let bytes = section(&[
        PROTOTYPE,
        b"\xe2\xe2\xe1\x08\x1b\xf6\xc2\x11\x03\x22\x00\xe3\x2f\x18\x00",
    ]);
    let rows = dimensions(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].dim_type, 0x1b);
    assert_eq!(rows[1].feature_id, None);
    assert_eq!(rows[1].value, Some(6.0));
}

#[test]
fn value_outside_the_frame_lane_has_no_value() {
    let bytes = section(&[PROTOTYPE, b"\xe2\xe2\xe1\x08\x03\x40\x02\xe3\x0e\x0e\xe3"]);
    let rows = dimensions(&bytes);
    assert_eq!(rows[1].feature_id, Some(0x40));
    assert_eq!(rows[1].value, None);
}

#[test]
fn other_element_types_are_not_dimensions() {
    let bytes = section(&[b"\xe2\xe2\xe1\x09\x03\x40\x02\xe3\x2f\x08\x00"]);
    assert!(dimensions(&bytes).is_empty());
}

#[test]
fn a_second_dim_array_field_withholds_every_row() {
    let mut bytes = section(&[PROTOTYPE]);
    bytes.extend_from_slice(b"\xe0\x00dim_array\x00\xf8\x00\xf7\x02\xfb");
    assert!(dimensions(&bytes).is_empty());
}

#[test]
fn round_radius_requires_bit_identical_positive_values() {
    let row = |feature_id, dim_type, value| ModelDimension {
        offset: 0,
        dim_type,
        feature_id: Some(feature_id),
        value,
    };
    let rows = [
        row(7, 3, Some(2.0)),
        row(7, 3, Some(2.0)),
        row(7, 62, Some(9.0)),
        row(8, 3, Some(2.0)),
        row(8, 3, Some(2.5)),
        row(9, 3, None),
        row(10, 3, Some(-1.0)),
    ];
    assert_eq!(
        round_radius(&rows, 7),
        RoundDimensionRadius::Constant(positive(2.0))
    );
    assert_eq!(round_radius(&rows, 8), RoundDimensionRadius::Ambiguous);
    assert_eq!(round_radius(&rows, 9), RoundDimensionRadius::Ambiguous);
    assert_eq!(round_radius(&rows, 10), RoundDimensionRadius::Ambiguous);
    assert_eq!(round_radius(&rows, 11), RoundDimensionRadius::NotPresent);
}

/// The labeled first element of a `dim_array` whose elements start at `type`:
/// angle 360.0 of feature 667.
const BARE_PROTOTYPE: &[u8] = b"\xe3\xe0\x01type\x00\x08\xe0\x01dim_type\x00\x15\
\xe0\x01feat_id\x00\x82\x9b\xe0\x01attr_arr\x00\xf8\x02\x81\x81\x20\
\xe0\x00dim_dat_ptr\x00\xe3\xe0\x02value\x00\x2f\x76\x80\xe0\x02bck_value\x00\x2f\x76\x80\xe3\x09";

fn bare_section(token: &[u8], elements: &[&[u8]]) -> (Vec<u8>, usize) {
    let mut bytes = b"\xe0\x00dim_array\x00".to_vec();
    bytes.extend_from_slice(token);
    bytes.extend_from_slice(b"\xf8\x04\xf7\x02\xfb\xe1");
    let first = bytes.len();
    for element in elements {
        bytes.extend_from_slice(element);
    }
    (bytes, first)
}

#[test]
fn bare_elements_open_with_an_optional_class_reference() {
    let plain = b"\xe3\x08\x02\x80\xf7\xf5\xeb\x7f\xff\xfe\xdf\xeb\x40\x00\x00\x20\xe3\x2f\x50\xc0\x2f\x50\xc0\xe3";
    let referenced = b"\xe3\xf7\x08\x08\x0a\x8a\xbf\x81\x21\xc0\x80\x20\xe3\x2f\x3f\x00";
    let (bytes, first) = bare_section(b"", &[BARE_PROTOTYPE, plain, referenced]);
    let second = first + BARE_PROTOTYPE.len();
    assert_eq!(
        dimensions(&bytes),
        vec![
            ModelDimension {
                offset: first + 1,
                dim_type: ANGULAR_DIMENSION_TYPE,
                feature_id: Some(667),
                value: Some(360.0),
            },
            ModelDimension {
                offset: second + 1,
                dim_type: 2,
                feature_id: Some(247),
                value: Some(67.0),
            },
            ModelDimension {
                offset: second + plain.len() + 3,
                dim_type: 10,
                feature_id: Some(2751),
                value: Some(31.0),
            },
        ]
    );
}

#[test]
fn array_token_before_the_count_is_skipped() {
    for token in [&b"\xf1"[..], b"\xf3", b"\xf4\x04"] {
        let (bytes, _) = bare_section(token, &[BARE_PROTOTYPE]);
        let rows = dimensions(&bytes);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].feature_id, Some(667));
    }
}

#[test]
fn named_prototype_can_reference_the_item_class() {
    let referenced = [
        &b"\xe3\xe0\x00dtl_named_item\x00\xe2\xf7\x04\xe2\xe1"[..],
        &PROTOTYPE[PROTOTYPE
            .windows(7)
            .position(|window| window == b"\xe0\x01type\x00")
            .expect("type label")..],
    ]
    .concat();
    let bytes = section(&[
        &referenced,
        b"\xe2\xe2\xe1\x08\x3e\x64\x81\x81\x20\x02\xe3\x2f\x2e\x00",
    ]);
    let rows = dimensions(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].feature_id, Some(0x59));
    assert_eq!(rows[0].value, Some(3.0));
    assert_eq!(rows[1].dim_type, 62);
}

#[test]
fn rows_need_a_labeled_prototype() {
    let (bytes, _) = bare_section(b"", &[b"\xe3\x08\x15\x82\x9b\x81\x81\x20\xe3\x2f\x76\x80"]);
    assert!(dimensions(&bytes).is_empty());
}
