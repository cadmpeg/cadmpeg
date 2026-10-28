// SPDX-License-Identifier: Apache-2.0

use super::variable_table;
use crate::feature::definitions::{
    decode_section_coordinate_scalar, decode_variable_scalar, saved_section_scalar, ScalarLane,
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
fn variable_row_decodes_independent_full_width_value_and_guess() {
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
    assert_eq!(
        row.value,
        ScalarLane::Value(f64::from_bits(0x0102_0304_0506_0708))
    );
    assert_eq!(
        row.guess_body,
        [0xed, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]
    );
    assert_eq!(
        row.guess,
        ScalarLane::Value(f64::from_bits(0x1112_1314_1516_1718))
    );
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

#[test]
fn typed_full_width_coordinate_retains_nonfinite_and_rejects_truncation() {
    use crate::feature::definitions::{
        decode_typed_variable_field, decode_variable_scalar, VariableType,
    };
    let cache = scalar::ScalarCache::default();
    let expected = -13.25_f64;
    let mut body = vec![0xed];
    body.extend_from_slice(&expected.to_be_bytes());
    assert_eq!(
        decode_typed_variable_field(&body, 0, body.len(), &cache, VariableType::U),
        (ScalarLane::Value(expected), 9)
    );
    assert_eq!(
        decode_variable_scalar(&body, 0, body.len(), &cache),
        (ScalarLane::DimensionDriven, 9)
    );
    for end in 1..9 {
        assert_eq!(
            decode_typed_variable_field(&body, 0, end, &cache, VariableType::U),
            (ScalarLane::Undefined, 0)
        );
    }
    let nonfinite = [0xed, 0x7f, 0xf0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        decode_typed_variable_field(&nonfinite, 0, 9, &cache, VariableType::U),
        (ScalarLane::OpaqueFullWidth, 9)
    );
    let short = [0x00, 0x12, 0x34];
    assert_eq!(
        decode_typed_variable_field(&short, 0, 3, &cache, VariableType::U),
        (ScalarLane::Undefined, 3)
    );
}

#[test]
fn full_width_variable_field_requires_coordinate_role_for_numeric_admission() {
    use crate::feature::definitions::{decode_typed_variable_field, VariableType};
    let mut image = vec![0xed];
    image.extend_from_slice(&(-13.25_f64).to_be_bytes());
    let cache = scalar::ScalarCache::default();
    for role in [1, 2] {
        assert_eq!(
            decode_typed_variable_field(&image, 0, image.len(), &cache, VariableType::from(role)),
            (ScalarLane::Value(-13.25), 9)
        );
    }
    for role in [0, 3, 4, 5, 6, 7, u32::MAX] {
        assert_eq!(
            decode_typed_variable_field(&image, 0, image.len(), &cache, VariableType::from(role)),
            (ScalarLane::OpaqueFullWidth, 9)
        );
        for end in 1..9 {
            assert_eq!(
                decode_typed_variable_field(&image, 0, end, &cache, VariableType::from(role)),
                (ScalarLane::Undefined, 0)
            );
        }
    }
}

#[test]
fn named_variable_prototype_frames_scalar_payload_before_names_and_close() {
    let value_word = [0x3f, b'g', b'u', b'e', b's', b's', 0, 1];
    let guess_word = [0x3f, 0xf1, 0xf7, 0xe2, 0xe3, 0x11, 0x22, 0x33];
    let mut payload =
        b"var_arr\0\xf8\x01\xf7\x71\xfb\xe2\xe0\x05type\0\x01\xe0\x08key\0\x09\xe0\x02value\0\xed"
            .to_vec();
    payload.extend(value_word);
    payload.extend_from_slice(b"\xe0\x02guess\0\xed");
    payload.extend(guess_word);
    payload.extend_from_slice(
        b"\xe0\x07known\0\x00\xe0\x07homogeneity\0\x01\xe0\x08uvar_id\0\x09\xf1\xf7\x71\xe2",
    );
    let table = variable_table(&payload, 0, payload.len(), &scalar::ScalarCache::default())
        .expect("complete prototype");
    assert!(table.is_complete());
    let [row] = table.rows.as_slice() else {
        panic!("one owned row");
    };
    assert_eq!(row.value, ScalarLane::Value(f64::from_be_bytes(value_word)));
    assert_eq!(row.guess, ScalarLane::Value(f64::from_be_bytes(guess_word)));
    assert_eq!(row.value_body.len(), 9);
    assert_eq!(row.guess_body.len(), 9);
    assert_eq!(
        (row.key, row.known, row.homogeneity, row.uvar_id),
        (9, Some(0), Some(1), Some(9))
    );
    let mut wrong_class = payload.clone();
    let class_offset = wrong_class.len() - 2;
    wrong_class[class_offset] = 0x72;
    assert!(variable_table(
        &wrong_class,
        0,
        wrong_class.len(),
        &scalar::ScalarCache::default()
    )
    .is_none());
    assert!(variable_table(
        &payload,
        0,
        payload.len() - 2,
        &scalar::ScalarCache::default()
    )
    .is_none());
    let mut truncated = payload.clone();
    let close = truncated.len() - 4;
    truncated.truncate(close);
    assert!(variable_table(
        &truncated,
        0,
        truncated.len(),
        &scalar::ScalarCache::default()
    )
    .is_none());
}

#[test]
fn nonfinite_full_width_coordinate_is_opaque_and_cannot_use_duplicate_value() {
    use crate::feature::definitions::{
        decode_typed_variable_field, FeatureVariableRow, FeatureVariableTable, VariableType,
    };
    let mut image = vec![0xed];
    image.extend_from_slice(&f64::INFINITY.to_be_bytes());
    let (lane, end) = decode_typed_variable_field(
        &image,
        0,
        image.len(),
        &scalar::ScalarCache::default(),
        VariableType::U,
    );
    assert_eq!((lane, end), (ScalarLane::OpaqueFullWidth, 9));
    let row = FeatureVariableRow {
        variable_type: VariableType::U,
        key: 9,
        value: lane,
        value_body: image,
        guess: ScalarLane::Undefined,
        guess_body: Vec::new(),
        known: None,
        homogeneity: None,
        uvar_id: None,
        offset: 0,
    };
    let mut duplicate = row.clone();
    duplicate.value = ScalarLane::Value(3.5);
    let table = FeatureVariableTable {
        declared_count: 2,
        entity_ref: None,
        rows: vec![row, duplicate],
        offset: 0,
    };
    assert!(table.reconciled_points().ambiguous.contains(&9));
    assert!(!table.reconciled_points().points.contains_key(&9));
}

#[test]
fn variable_rows_require_complete_full_width_value_and_guess_images() {
    use crate::feature::definitions::{positional_variable_table, VariableType};
    let cache = scalar::ScalarCache::default();
    // Authored payload includes row-looking bytes and a compact-looking
    // suffix before e2. Its marker bytes remain inside the scalar image.
    let image = [0xed, 0x3f, 0, 1, 1, 0xe2, 0xf7, 0xe3, 0x0f];
    let table = |row: &[u8], positional: bool| {
        let mut bytes = if positional {
            b"\xf8\x01\xf7\x71\xfb\xe2\xf7\x72".to_vec()
        } else {
            b"var_arr\0\xf8\x01\xf7\x71\xfb\xe2\xf1\xf7\x71\xe2".to_vec()
        };
        bytes.extend_from_slice(row);
        if positional {
            positional_variable_table(&bytes, 0, bytes.len(), 0x71, &cache)
        } else {
            variable_table(&bytes, 0, bytes.len(), &cache)
        }
        .expect("owned variable array")
    };
    for positional in [false, true] {
        for kind in [1_u8, 6] {
            let mut row = vec![kind, 9];
            row.extend_from_slice(&image);
            row.extend_from_slice(&image);
            row.extend_from_slice(&[0, 1, 1, 0xe2]);
            let complete = table(&row, positional);
            assert!(complete.is_complete());
            assert_eq!(
                complete.rows[0].variable_type,
                VariableType::from(u32::from(kind))
            );
            assert_eq!(complete.rows[0].value_body, image);
            assert_eq!(complete.rows[0].guess_body, image);
            assert_eq!(complete.rows[0].known, Some(0));
            assert_eq!(complete.rows[0].homogeneity, Some(1));
            assert_eq!(complete.rows[0].uvar_id, Some(1));
            if kind == 6 {
                assert_eq!(complete.rows[0].value, ScalarLane::OpaqueFullWidth);
                assert_eq!(complete.rows[0].guess, ScalarLane::OpaqueFullWidth);
            }
            for cut in 1..image.len() {
                for truncated_guess in [false, true] {
                    let mut row = vec![kind, 9];
                    if truncated_guess {
                        row.extend_from_slice(&image);
                    }
                    row.extend_from_slice(&image[..cut]);
                    let incomplete = table(&row, positional);
                    assert!(
                        !incomplete.is_complete(),
                        "positional={positional} kind={kind} cut={cut} guess={truncated_guess}"
                    );
                    assert!(incomplete.rows.is_empty());
                }
            }
        }
    }
}

#[test]
fn real_named_variable_table_retains_full_width_coordinate_fields() {
    // Exact wine-glass.prt excerpt at38624..39773, including the following
    // eqtn_arr opener as boundary context. No isolated-table EOF claim.
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wine-variable-table-with-next-opener.bin"
    ));
    let table = variable_table(bytes, 0, bytes.len(), &scalar::ScalarCache::default())
        .expect("real seven-field named prototype");
    assert!(table.is_complete());
    assert_eq!(table.declared_count, 72);
    assert_eq!(table.entity_ref, Some(151));
    assert_eq!(table.rows.len(), 72);
    let prototype = &table.rows[0];
    assert_eq!(prototype.variable_type.code(), 5);
    assert_eq!(prototype.key, 0);
    assert_eq!(prototype.value_body, [0x18]);
    assert_eq!(prototype.guess_body, [0x18]);
    assert_eq!(
        (prototype.known, prototype.homogeneity, prototype.uvar_id),
        (Some(1), Some(0), Some(0))
    );
    let value_word = [0xba, 0x10, 0x0c, 0x8d, 0xee, 0x90, 0xb4, 0x0c];
    let guess_word = [0x3a, 0x50, 0x22, 0xce, 0xe9, 0x91, 0x0f, 0x6b];
    for (ordinal, key, uvar) in [(8, 1, 1), (9, 2, 2), (47, 20, 28)] {
        let row = &table.rows[ordinal];
        assert_eq!(row.variable_type.code(), 1);
        assert_eq!(row.key, key);
        assert_eq!(row.value_body[0], 0xed);
        assert_eq!(row.guess_body[0], 0xed);
        assert_eq!(row.value_body[1..], value_word);
        assert_eq!(row.guess_body[1..], guess_word);
        assert_eq!(row.value_body.len(), 9);
        assert_eq!(row.guess_body.len(), 9);
        assert_eq!(
            row.value.value().expect("stored IEEE value").to_bits(),
            u64::from_be_bytes(value_word)
        );
        assert_eq!(
            row.guess.value().expect("stored IEEE guess").to_bits(),
            u64::from_be_bytes(guess_word)
        );
        assert_eq!(
            (row.known, row.homogeneity, row.uvar_id),
            (Some(0), Some(1), Some(uvar))
        );
    }
}
