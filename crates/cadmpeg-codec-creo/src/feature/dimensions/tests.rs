// SPDX-License-Identifier: Apache-2.0
use super::{
    dimensions, round_radius, ModelDimension, RoundDimensionRadius, ANGULAR_DIMENSION_TYPE,
};
use crate::test_support::{dim_array, DimSlot};
use cadmpeg_ir::scalar::PositiveReal;

/// The labeled first element: radius 3.0 of feature 0x59 with three attributes.
const PROTOTYPE: &[u8] =
    b"\xe3\xe0\x00dtl_named_item\x00\xe2\xe0\x00dtl_item\x00\xe2\xe0\x0aname\x00\xf1\xe1\
\xe0\x01type\x00\xf1\x08\xe0\x01dim_type\x00\x03\xe0\x01feat_id\x00\x59\
\xe0\x01attr_arr\x00\xf8\x03\xf5\xeb\x7f\xff\xfe\xdf\xeb\x40\x00\x00\x20\x02\
\xe0\x00dim_dat_ptr\x00\xe3\xe0\x02value\x00\x2f\x08\x00\xe0\x02bck_value\x00\x2f\x08\x00";

/// A later element: dimension type 62 of feature 0x64 with value 15.0.
const LATER: &[u8] = b"\xe3\xe2\xe2\xe1\x08\x3e\x64\x81\x81\x20\x02\xe3\x2f\x2e\x00\x2f\x2e\x00";

/// A later element: radius 2.0 of feature 0x8c5, with a repeated attribute.
const LATER_RADIUS: &[u8] =
    b"\xe3\xe2\xe2\xe1\x08\x03\x88\xc5\x20\xe4\x02\xe3\x8b\x00\x00\x00\x00\x00\x00";

fn positive(value: f64) -> PositiveReal {
    PositiveReal::new(value).expect("positive fixture radius")
}

#[test]
fn prototype_and_positional_rows_share_field_order() {
    let (bytes, starts) = dim_array(
        b"",
        &[
            DimSlot::Null,
            DimSlot::Null,
            DimSlot::Prototype(PROTOTYPE),
            DimSlot::Later(LATER),
            DimSlot::Later(LATER_RADIUS),
            DimSlot::Null,
        ],
    );
    assert_eq!(
        dimensions(&bytes),
        vec![
            ModelDimension {
                offset: starts[2] + 1,
                dim_type: 3,
                feature_id: Some(0x59),
                value: Some(3.0),
            },
            ModelDimension {
                offset: starts[3] + 1,
                dim_type: 62,
                feature_id: Some(0x64),
                value: Some(15.0),
            },
            ModelDimension {
                offset: starts[4] + 1,
                dim_type: 3,
                feature_id: Some(0x8c5),
                value: Some(2.0),
            },
        ]
    );
}

#[test]
fn null_feature_and_three_byte_attribute_are_read() {
    let (bytes, _) = dim_array(
        b"",
        &[
            DimSlot::Prototype(PROTOTYPE),
            DimSlot::Later(b"\xe3\xe2\xe2\xe1\x08\x1b\xf6\xc2\x11\x03\x22\x00\xe3\x2f\x18\x00"),
        ],
    );
    let rows = dimensions(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].dim_type, 0x1b);
    assert_eq!(rows[1].feature_id, None);
    assert_eq!(rows[1].value, Some(6.0));
}

#[test]
fn value_outside_the_frame_lane_has_no_value() {
    let (bytes, _) = dim_array(
        b"",
        &[
            DimSlot::Prototype(PROTOTYPE),
            DimSlot::Later(b"\xe3\xe2\xe2\xe1\x08\x03\x40\x02\x02\x02\xe3\x0e\x0e"),
        ],
    );
    let rows = dimensions(&bytes);
    assert_eq!(rows[1].feature_id, Some(0x40));
    assert_eq!(rows[1].value, None);
}

#[test]
fn an_element_whose_fields_do_not_read_yields_no_row_and_ends_at_its_symbol() {
    let two_attributes = b"\xe3\xe2\xe2\xe1\x08\x03\x40\x02\x02\xe3\x2f\x08\x00";
    let (bytes, starts) = dim_array(
        b"",
        &[
            DimSlot::Prototype(PROTOTYPE),
            DimSlot::Later(two_attributes),
            DimSlot::Later(LATER),
        ],
    );
    let rows = dimensions(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].offset, starts[2] + 1);
    assert_eq!(rows[1].dim_type, 62);
}

#[test]
fn class_references_can_precede_a_slot() {
    let (mut bytes, starts) =
        dim_array(b"", &[DimSlot::Prototype(PROTOTYPE), DimSlot::Later(LATER)]);
    bytes.splice(starts[1]..starts[1], *b"\xf7\x08\xf7\x09");
    let rows = dimensions(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].offset, starts[1] + 5);
    assert_eq!(rows[1].dim_type, 62);
}

#[test]
fn a_symbol_for_another_slot_withholds_every_row() {
    let (mut bytes, starts) =
        dim_array(b"", &[DimSlot::Prototype(PROTOTYPE), DimSlot::Later(LATER)]);
    let symbol_index = starts[1] + LATER.len() + 2;
    assert_eq!(bytes[symbol_index], 1);
    bytes[symbol_index] = 2;
    assert!(dimensions(&bytes).is_empty());
}

#[test]
fn rows_after_the_declared_slots_are_not_read() {
    let (mut bytes, _) = dim_array(b"", &[DimSlot::Prototype(PROTOTYPE), DimSlot::Later(LATER)]);
    for index in 2..66 {
        bytes.extend_from_slice(LATER_RADIUS);
        bytes.extend_from_slice(&[0xe2, 0x01, index]);
    }
    let rows = dimensions(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(round_radius(&rows, 0x8c5), RoundDimensionRadius::NotPresent);
}

#[test]
fn a_count_beyond_the_payload_withholds_every_row() {
    let (mut bytes, _) = dim_array(b"", &[DimSlot::Prototype(PROTOTYPE)]);
    let count = bytes
        .windows(4)
        .position(|window| window == b"\xf8\x01\xf7\x02")
        .expect("slot count");
    bytes.splice(count + 1..count + 2, [0xbf, 0xff]);
    assert!(dimensions(&bytes).is_empty());
}

#[test]
fn an_unclosed_element_stops_the_walk_in_one_pass() {
    let (mut bytes, starts) = dim_array(b"", &[DimSlot::Prototype(BARE_PROTOTYPE), DimSlot::Null]);
    bytes.truncate(starts[1]);
    for _ in 0..200_000 {
        bytes.extend_from_slice(b"\xe3\x08\x01\x01\xc0");
    }
    assert!(dimensions(&bytes).is_empty());
}

#[test]
fn other_element_types_are_not_dimensions() {
    let (bytes, _) = dim_array(
        b"",
        &[DimSlot::Later(
            b"\xe3\xe2\xe2\xe1\x09\x03\x40\x02\xe3\x2f\x08\x00",
        )],
    );
    assert!(dimensions(&bytes).is_empty());
}

#[test]
fn a_second_dim_array_field_withholds_every_row() {
    let (mut bytes, _) = dim_array(b"", &[DimSlot::Prototype(PROTOTYPE)]);
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
\xe0\x00dim_dat_ptr\x00\xe3\xe0\x02value\x00\x2f\x76\x80\xe0\x02bck_value\x00\x2f\x76\x80";

#[test]
fn bare_elements_open_with_an_optional_class_reference() {
    let plain = b"\xe3\x08\x02\x80\xf7\xf5\xeb\x7f\xff\xfe\xdf\xeb\x40\x00\x00\x20\xe3\x2f\x50\xc0\x2f\x50\xc0";
    let referenced = b"\xe3\xf7\x08\x08\x0a\x8a\xbf\x81\x21\xc0\x80\x20\xe3\x2f\x3f\x00";
    let (bytes, starts) = dim_array(
        b"",
        &[
            DimSlot::Null,
            DimSlot::Prototype(BARE_PROTOTYPE),
            DimSlot::Later(plain),
            DimSlot::Later(referenced),
        ],
    );
    assert_eq!(
        dimensions(&bytes),
        vec![
            ModelDimension {
                offset: starts[1] + 1,
                dim_type: ANGULAR_DIMENSION_TYPE,
                feature_id: Some(667),
                value: Some(360.0),
            },
            ModelDimension {
                offset: starts[2] + 1,
                dim_type: 2,
                feature_id: Some(247),
                value: Some(67.0),
            },
            ModelDimension {
                offset: starts[3] + 3,
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
        let (bytes, _) = dim_array(token, &[DimSlot::Prototype(BARE_PROTOTYPE)]);
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
    let (bytes, _) = dim_array(
        b"",
        &[DimSlot::Prototype(&referenced), DimSlot::Later(LATER)],
    );
    let rows = dimensions(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].feature_id, Some(0x59));
    assert_eq!(rows[0].value, Some(3.0));
    assert_eq!(rows[1].dim_type, 62);
}

#[test]
fn rows_need_a_labeled_prototype() {
    let (bytes, _) = dim_array(
        b"",
        &[DimSlot::Later(
            b"\xe3\x08\x15\x82\x9b\x81\x81\x20\xe3\x2f\x76\x80",
        )],
    );
    assert!(dimensions(&bytes).is_empty());
}
