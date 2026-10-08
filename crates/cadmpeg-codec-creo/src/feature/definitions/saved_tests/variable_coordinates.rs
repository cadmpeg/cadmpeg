// SPDX-License-Identifier: Apache-2.0

use super::variable_table;
use crate::feature::definitions::{
    decode_section_coordinate_scalar as decode_section_coordinate_scalar_checked,
    decode_variable_scalar as decode_variable_scalar_checked, saved_section_scalar, ScalarLane,
};
use crate::scalar;

#[test]
fn decodes_var_arr_dictionary_sign_pairs() {
    let cache = scalar::ScalarCache::default();
    let cases = [
        (
            [0x97, 0xc3, 0x95, 0x81, 0x06, 0x24, 0xdc],
            3.595_499_999_999_999_5,
        ),
        (
            [0xdd, 0xc3, 0x95, 0x81, 0x06, 0x24, 0xdc],
            -3.595_499_999_999_999_5,
        ),
        (
            [0x80, 0x58, 0x23, 0x8b, 0x27, 0x55, 0x6f],
            1.334_018_271_988_806_7,
        ),
        ([0x7f, 0xa3, 0xd7, 0x0a, 0x3d, 0x70, 0xa4], 1.29),
        ([0xc7, 0xa3, 0xd7, 0x0a, 0x3d, 0x70, 0xa4], -1.29),
        (
            [0xc8, 0x58, 0x23, 0x8b, 0x27, 0x55, 0x6f],
            -1.334_018_271_988_806_7,
        ),
    ];
    for (bytes, expected) in cases {
        let (value, next) = decode_variable_scalar(&bytes, 0, bytes.len(), &cache);
        assert_eq!(value, ScalarLane::Value(expected));
        assert_eq!(next, bytes.len());
    }
}

#[test]
fn decodes_var_arr_negative_subunit_form() {
    let bytes = [0xd5, 0xd9, 0x52, 0xa4, 0x85, 0x40, 0x39];
    let (value, next) =
        decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());

    assert_eq!(value, ScalarLane::Value(-0.395_669_107_559_015_74));
    assert_eq!(next, bytes.len());
}

#[test]
fn decodes_var_arr_positive_subunit_form() {
    let bytes = [0x4f, 0xdf, 0x46, 0xa2, 0x52, 0x96, 0xd1];
    let (value, next) =
        decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());

    assert_eq!(value, ScalarLane::Value(0.488_686_161_664_432_46));
    assert_eq!(next, bytes.len());
}

#[test]
fn variable_row_bounds_an_unresolved_guess_from_its_fixed_suffix() {
    let payload = b"var_arr\0\xf8\x01\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2\
            \x00\x41\x18\x20\x96\x61\x01\x01\x82\x06\xe2";
    let variables = variable_table(payload, 0, payload.len(), &scalar::ScalarCache::default())
        .expect("variable table");
    let [row] = variables.rows.as_slice() else {
        panic!("one structurally complete variable row");
    };

    assert!(variables.is_complete());
    assert_eq!(
        row.variable_type,
        crate::feature::definitions::VariableType::Dimension
    );
    assert_eq!(row.key, 65);
    assert_eq!(row.value, ScalarLane::Value(0.0));
    assert_eq!(row.value_body, [0x18]);
    assert_eq!(row.guess, ScalarLane::Undefined);
    assert_eq!(row.guess_body, [0x20, 0x96, 0x61]);
    assert_eq!(row.known, Some(1));
    assert_eq!(row.homogeneity, Some(1));
    assert_eq!(row.uvar_id, Some(518));
}

#[test]
fn variable_row_classifies_value_and_guess_sentinels_independently() {
    let payload = b"var_arr\0\xf8\x01\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2\
            \x01\x07\xed\x01\x02\x03\x04\x05\x06\x07\x08\
            \xed\x11\x12\x13\x14\x15\x16\x17\x18\x01\x01\x09\xe2";
    let variables = variable_table(payload, 0, payload.len(), &scalar::ScalarCache::default())
        .expect("variable table");
    let [row] = variables.rows.as_slice() else {
        panic!("one structurally complete variable row");
    };

    assert!(variables.is_complete());
    assert_eq!(
        row.value_body,
        [0xed, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
    );
    assert_eq!(row.value, ScalarLane::DimensionDriven);
    assert_eq!(
        row.guess_body,
        [0xed, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]
    );
    assert_eq!(row.guess, ScalarLane::DimensionDriven);
    assert_eq!(row.known, Some(1));
    assert_eq!(row.homogeneity, Some(1));
    assert_eq!(row.uvar_id, Some(9));
}

#[test]
fn var_arr_world_coordinate_2d_is_positive() {
    let bytes = [0x2d, 0x34, 0x43, 0xf5, 0x12, 0xe8, 0x00, 0x45];
    let (value, next) =
        decode_section_coordinate_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());

    assert_eq!(value, ScalarLane::Value(20.265_458_280_220_873));
    assert_eq!(next, bytes.len());
    assert_eq!(
        decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()).0,
        ScalarLane::Value(-20.265_458_280_220_873)
    );
}

#[test]
fn saved_section_world_coordinate_2d_is_positive() {
    let bytes = [0x2d, 0x52, 0xa4, 0x0d, 0xb4, 0x1f, 0x70, 0xed];

    assert_eq!(
        saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
        (Some(74.563_336_401_657_31), bytes.len())
    );
}

#[test]
fn decodes_var_arr_positional_dict_lattice() {
    for (bytes, head) in [
        ([0x51, 1, 2, 3, 4, 5, 6], [0x3f, 0xc6]),
        ([0x64, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
        ([0x69, 1, 2, 3, 4, 5, 6], [0x3f, 0xde]),
        ([0x9c, 1, 2, 3, 4, 5, 6], [0x40, 0x11]),
        ([0x9d, 1, 2, 3, 4, 5, 6], [0x40, 0x12]),
        ([0x9f, 1, 2, 3, 4, 5, 6], [0x40, 0x14]),
        ([0xa0, 1, 2, 3, 4, 5, 6], [0x40, 0x15]),
        ([0xa7, 1, 2, 3, 4, 5, 6], [0xbf, 0xd3]),
        ([0xaa, 1, 2, 3, 4, 5, 6], [0xbf, 0xd6]),
        ([0xae, 1, 2, 3, 4, 5, 6], [0xbf, 0xda]),
        ([0xad, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
        ([0xb3, 1, 2, 3, 4, 5, 6], [0xbf, 0xe0]),
        ([0xbd, 1, 2, 3, 4, 5, 6], [0xbf, 0xea]),
        ([0xc3, 1, 2, 3, 4, 5, 6], [0xbf, 0xf0]),
        ([0xc9, 1, 2, 3, 4, 5, 6], [0xbf, 0xf6]),
        ([0xca, 1, 2, 3, 4, 5, 6], [0xbf, 0xf7]),
        ([0xcb, 1, 2, 3, 4, 5, 6], [0xbf, 0xf8]),
        ([0xcc, 1, 2, 3, 4, 5, 6], [0xbf, 0xf9]),
        ([0xcd, 1, 2, 3, 4, 5, 6], [0xbf, 0xfa]),
        ([0xce, 1, 2, 3, 4, 5, 6], [0xbf, 0xfb]),
        ([0xd0, 1, 2, 3, 4, 5, 6], [0xbf, 0xfe]),
        ([0xd2, 1, 2, 3, 4, 5, 6], [0xc0, 0x00]),
        ([0xd4, 1, 2, 3, 4, 5, 6], [0xc0, 0x02]),
        ([0xd6, 1, 2, 3, 4, 5, 6], [0xc0, 0x04]),
        ([0xd8, 1, 2, 3, 4, 5, 6], [0xc0, 0x06]),
        ([0xda, 1, 2, 3, 4, 5, 6], [0xc0, 0x08]),
    ] {
        let (value, next) =
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());
        assert_eq!(
            value,
            ScalarLane::Value(f64::from_be_bytes([
                head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
            ]))
        );
        assert_eq!(next, bytes.len());
    }
    let bytes = [0x28, 1, 2, 3, 4, 5, 6, 7];
    assert_eq!(
        decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
        (
            ScalarLane::Value(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])),
            bytes.len(),
        )
    );
    for prefix in [0x19, 0x32, 0x37, 0x41] {
        let bytes = [prefix, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (
                ScalarLane::Value(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])),
                bytes.len(),
            )
        );
    }
    assert_eq!(
        decode_section_coordinate_scalar(
            &[0x34, 0xd0, 0x00],
            0,
            3,
            &scalar::ScalarCache::default()
        ),
        (ScalarLane::Undefined, 3)
    );
    assert_eq!(
        decode_section_coordinate_scalar(
            &[0x00, 0x04, 0xa6],
            0,
            3,
            &scalar::ScalarCache::default()
        ),
        (ScalarLane::Undefined, 3)
    );
    assert_eq!(
        decode_section_coordinate_scalar(
            &[0x01, 0x04, 0xfe, 0xf2],
            0,
            4,
            &scalar::ScalarCache::default()
        ),
        (ScalarLane::Undefined, 4)
    );
}

fn decode_variable_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (ScalarLane, usize) {
    crate::decode::with_test_decode_ctx(|ctx| {
        decode_variable_scalar_checked(ctx, payload, offset, end, cache)
    })
    .expect("scalar lane admission")
}

fn decode_section_coordinate_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (ScalarLane, usize) {
    crate::decode::with_test_decode_ctx(|ctx| {
        decode_section_coordinate_scalar_checked(ctx, payload, offset, end, cache)
    })
    .expect("scalar lane admission")
}
