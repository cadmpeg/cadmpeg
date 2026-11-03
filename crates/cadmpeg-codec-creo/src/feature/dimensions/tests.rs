// SPDX-License-Identifier: Apache-2.0
use super::{dimensions, round_radius, ModelDimension, RoundDimensionRadius};
use cadmpeg_ir::scalar::PositiveReal;

const HEADER: &[u8] = b"\xe0\x00Sld_BasFullData\x00\xe3\xe0\x00dim_array\x00\xf8\x06\xf7\x02\xfb\xe1\xe1";

/// The labeled first element: radius 3.0 of feature 0x59.
const PROTOTYPE: &[u8] = b"\xe3\xe0\x00dtl_named_item\x00\xe2\xe0\x00dtl_item\x00\xe2\xe0\x0aname\x00\xf1\xe1\
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
