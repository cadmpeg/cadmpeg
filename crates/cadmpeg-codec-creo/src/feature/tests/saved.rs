// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::definitions::*;
use super::super::operations::*;
use super::super::rows::*;
use crate::psb;
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
        let (value, next, dimension_driven) =
            decode_variable_scalar(&bytes, 0, bytes.len(), &cache);
        assert_eq!(value, Some(expected));
        assert_eq!(next, bytes.len());
        assert!(!dimension_driven);
    }
}

#[test]
fn decodes_var_arr_negative_subunit_form() {
    let bytes = [0xd5, 0xd9, 0x52, 0xa4, 0x85, 0x40, 0x39];
    let (value, next, dimension_driven) =
        decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());

    assert_eq!(value, Some(-0.395_669_107_559_015_74));
    assert_eq!(next, bytes.len());
    assert!(!dimension_driven);
}

#[test]
fn decodes_var_arr_positive_subunit_form() {
    let bytes = [0x4f, 0xdf, 0x46, 0xa2, 0x52, 0x96, 0xd1];
    let (value, next, dimension_driven) =
        decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());

    assert_eq!(value, Some(0.488_686_161_664_432_46));
    assert_eq!(next, bytes.len());
    assert!(!dimension_driven);
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
    assert_eq!(row.variable_type, 0);
    assert_eq!(row.key, 65);
    assert_eq!(row.value, Some(0.0));
    assert_eq!(row.value_body, [0x18]);
    assert_eq!(row.guess, None);
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
    assert!(row.dimension_driven);
    assert_eq!(
        row.guess_body,
        [0xed, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]
    );
    assert!(row.guess_dimension_driven);
    assert_eq!(row.known, Some(1));
    assert_eq!(row.homogeneity, Some(1));
    assert_eq!(row.uvar_id, Some(9));
}

#[test]
fn var_arr_world_coordinate_2d_is_positive() {
    let bytes = [0x2d, 0x34, 0x43, 0xf5, 0x12, 0xe8, 0x00, 0x45];
    let (value, next, dimension_driven) =
        decode_section_coordinate_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());

    assert_eq!(value, Some(20.265_458_280_220_873));
    assert_eq!(next, bytes.len());
    assert!(!dimension_driven);
    assert_eq!(
        decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()).0,
        Some(-20.265_458_280_220_873)
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
        let (value, next, dimension_driven) =
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());
        assert_eq!(
            value,
            Some(f64::from_be_bytes([
                head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
            ]))
        );
        assert_eq!(next, bytes.len());
        assert!(!dimension_driven);
    }
    let bytes = [0x28, 1, 2, 3, 4, 5, 6, 7];
    assert_eq!(
        decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
        (
            Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])),
            bytes.len(),
            false,
        )
    );
    for prefix in [0x19, 0x32, 0x37, 0x41] {
        let bytes = [prefix, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (
                Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])),
                bytes.len(),
                false,
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
        (None, 3, false)
    );
    assert_eq!(
        decode_section_coordinate_scalar(
            &[0x00, 0x04, 0xa6],
            0,
            3,
            &scalar::ScalarCache::default()
        ),
        (None, 3, false)
    );
    assert_eq!(
        decode_section_coordinate_scalar(
            &[0x01, 0x04, 0xfe, 0xf2],
            0,
            4,
            &scalar::ScalarCache::default()
        ),
        (None, 4, false)
    );
}

#[test]
fn saved_line_accepts_bare_entity_reference_before_coordinates() {
    let payload = b"\xe0\0entity(line)\0\x05\xe2\xf7\x2a\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\xf1\xf7\x2b\xe3";
    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(line.entity_id, 5);
    assert_eq!(line.references, [42, 43]);
    assert_eq!(line.endpoints, [[Some(8.0); 3]; 2]);
    let body_start = b"\xe0\0entity(line)\0".len();
    assert_eq!(line.body, payload[body_start..payload.len() - 1]);
}

#[test]
fn saved_line_expands_compact_basis_triple() {
    let payload = b"\xe0\0entity(line)\0\x05\xe2\x18\xe5\x2f\x20\0\x2f\x20\0\x2f\x20\0\xe3";
    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(
        line.endpoints,
        [
            [Some(0.0), Some(1.0), Some(0.0)],
            [Some(8.0), Some(8.0), Some(8.0)]
        ]
    );
    let body_start = b"\xe0\0entity(line)\0".len();
    assert_eq!(line.body, payload[body_start..payload.len() - 1]);
}

#[test]
fn saved_line_replay_continues_after_point_prototype() {
    let scalar_triple = b"\x2f\x20\0\x2f\x20\0\x2f\x20\0";
    let mut payload = b"\xe0\0entity(line)\0\x05\xe2".to_vec();
    payload.extend_from_slice(scalar_triple);
    payload.extend_from_slice(scalar_triple);
    payload.push(0xe3);
    payload.extend_from_slice(b"\xe0\0entity(point)\0\xe0\x01id\0\x04\xf1\xf7\x2a\xe3\x06\xe2");
    payload.extend_from_slice(scalar_triple);
    payload.extend_from_slice(scalar_triple);
    payload.extend_from_slice(b"\xe0\0entity(arc)\0");

    let entities = saved_line_entities(&payload, 0, payload.len(), &scalar::ScalarCache::default());

    assert_eq!(entities.len(), 2);
    assert_eq!(
        entities
            .iter()
            .filter_map(|entity| match entity {
                FeatureSavedEntity::Line(line) => Some(line.entity_id),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [5, 6]
    );
}

#[test]
fn saved_line_accepts_named_record_boundary() {
    let payload = b"\xe0\0entity(line)\0\x03\xe2\xf1\xf7\x80\xc4\
            \x48\x20\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\
            \x48\x1e\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\x8a\x01\x02\x03\x04\x05\x0f\
            \xe0\0entity(point)\0\xf1\xf7\x2a\xe3\xe0\0entity(arc)\0";
    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(line.entity_id, 3);
    assert_eq!(line.references, [196]);
    let body_start = b"\xe0\0entity(line)\0".len();
    let body_end = payload[body_start..]
        .windows(b"\xe0\0entity(point)\0".len())
        .position(|window| window == b"\xe0\0entity(point)\0")
        .map(|relative| body_start + relative)
        .expect("point boundary");
    assert_eq!(line.body, payload[body_start..body_end]);
}

#[test]
fn saved_line_retains_its_identity_and_coordinate_prefix() {
    let payload = b"\xe0\0entity(line)\0\x07\xe2\x0f\x0f\x0f\
            \xe0\0entity(arc)\0";

    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

    let [FeatureSavedEntity::Line(line)] = entities.as_slice() else {
        panic!("saved line");
    };
    assert_eq!(line.entity_id, 7);
    assert_eq!(
        line.endpoints,
        [[Some(0.0), Some(0.0), Some(0.0)], [None; 3]]
    );
}

#[test]
fn saved_section_retains_an_empty_named_table() {
    let payload = b"\xe0\0p_saved_result\0\xe0\x02local_sys\0";

    let section = saved_section(
        payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        None,
        None,
    )
    .expect("saved section header");

    assert_eq!(section.offset, 0);
    assert!(section.entities.is_empty());
}

#[test]
fn saved_section_41_form_occupies_eight_bytes() {
    let bytes = [0x41, 0xfd, 0x6b, 0xf1, 0xa1, 0xc2, 0x1f, 0xf0];
    let (value, next) =
        saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());
    assert_eq!(next, bytes.len());
    assert_eq!(
        value,
        Some(f64::from_be_bytes([
            0x3f, 0xfd, 0x6b, 0xf1, 0xa1, 0xc2, 0x1f, 0xf0
        ]))
    );
}

#[test]
fn saved_section_zero_does_not_consume_named_record_opener() {
    let mut section = Vec::new();
    for index in 0_u16..=224 {
        section.extend_from_slice(&[0x46, 0x08, (index >> 8) as u8, index as u8, 0, 0, 0, 0]);
    }
    let cache = scalar::ScalarCache::from_section(&section);

    assert_eq!(
        saved_section_scalar(&[0x18, 0xe0], 0, 2, &cache),
        (Some(0.0), 1)
    );
}

#[test]
fn saved_section_consecutive_zero_slots_remain_distinct() {
    let cache = scalar::ScalarCache::default();
    let bytes = [0x18, 0x18, 0x81, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        saved_section_scalar(&bytes, 0, bytes.len(), &cache),
        (Some(0.0), 1)
    );
    assert_eq!(
        saved_section_scalar(&bytes, 1, bytes.len(), &cache),
        (Some(0.0), 2)
    );
}

#[test]
fn saved_section_dd_form_supplies_ieee_high_bytes() {
    let bytes = [0xdd, 0xe6, 0x8a, 0x84, 0x79, 0xd0, 0x62];
    assert_eq!(
        saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
        (
            Some(f64::from_be_bytes([
                0x40, 0x0c, 0xe6, 0x8a, 0x84, 0x79, 0xd0, 0x62,
            ])),
            7,
        )
    );
}

#[test]
fn saved_section_negative_dict_forms_supply_ieee_high_bytes() {
    for (bytes, head) in [
        ([0xb3, 1, 2, 3, 4, 5, 6], [0xbf, 0xe0]),
        ([0xcb, 1, 2, 3, 4, 5, 6], [0xbf, 0xf8]),
        ([0xd6, 1, 2, 3, 4, 5, 6], [0xc0, 0x04]),
    ] {
        assert_eq!(
            saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (
                Some(f64::from_be_bytes([
                    head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                ])),
                7,
            )
        );
    }
}

#[test]
fn saved_arc_negative_dict_forms_supply_ieee_high_bytes() {
    for (bytes, head) in [
        ([0x9b, 1, 2, 3, 4, 5, 6], [0x40, 0x10]),
        ([0x9c, 1, 2, 3, 4, 5, 6], [0x40, 0x11]),
        ([0x9d, 1, 2, 3, 4, 5, 6], [0x40, 0x12]),
        ([0x9e, 1, 2, 3, 4, 5, 6], [0x40, 0x13]),
        ([0x9f, 1, 2, 3, 4, 5, 6], [0x40, 0x14]),
        ([0xa0, 1, 2, 3, 4, 5, 6], [0x40, 0x15]),
        ([0x5e, 1, 2, 3, 4, 5, 6], [0x3f, 0xd3]),
        ([0x60, 1, 2, 3, 4, 5, 6], [0x3f, 0xd5]),
        ([0x64, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
        ([0xad, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
        ([0xcc, 1, 2, 3, 4, 5, 6], [0xbf, 0xf9]),
        ([0xd0, 1, 2, 3, 4, 5, 6], [0xbf, 0xfe]),
        ([0xd2, 1, 2, 3, 4, 5, 6], [0xc0, 0x00]),
        ([0xd5, 1, 2, 3, 4, 5, 6], [0xc0, 0x03]),
        ([0xde, 1, 2, 3, 4, 5, 6], [0xc0, 0x10]),
        ([0xdf, 1, 2, 3, 4, 5, 6], [0xc0, 0x11]),
    ] {
        let expected = f64::from_be_bytes([
            head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
        ]);
        assert_eq!(
            saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (Some(expected), 7)
        );
    }
    let d5 = [0xd5, 1, 2, 3, 4, 5, 6];
    assert_eq!(
        saved_section_scalar(&d5, 0, d5.len(), &scalar::ScalarCache::default()),
        (Some(f64::from_be_bytes([0xbf, 1, 2, 3, 4, 5, 6, 0])), 7)
    );
}

#[test]
fn saved_arc_28_form_supplies_ieee_high_byte() {
    let bytes = [0x28, 1, 2, 3, 4, 5, 6, 7];
    assert_eq!(
        saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
        (Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])), 8)
    );
}

#[test]
fn saved_arc_zero_does_not_consume_arc_scalar_opener() {
    let bytes = [0x18, 0x5e, 1, 2, 3, 4, 5, 6];
    let cache = scalar::ScalarCache::default();
    assert_eq!(
        saved_arc_scalar(&bytes, 0, bytes.len(), &cache),
        (Some(0.0), 1)
    );
    assert_eq!(
        saved_arc_scalar(&bytes, 1, bytes.len(), &cache),
        (Some(f64::from_be_bytes([0x3f, 0xd3, 1, 2, 3, 4, 5, 6])), 8)
    );
}

#[test]
fn saved_circular_entities_retain_ids_and_independent_fields() {
    let payload = b"\xe0\x00entity(arc)\0\
            \xe0\x01id\0\x07\xe0\x02center\0\x0f\x0f\x0f\
            \xe0\x00entity(circle)\0\
            \xe0\x01id\0\x08\xe0\x02radius\0\x0f";

    let entities = saved_circular_entities(
        payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        None,
        None,
    );

    let [FeatureSavedEntity::Arc(arc), FeatureSavedEntity::Circle(circle)] = entities.as_slice()
    else {
        panic!("saved circular entities");
    };
    assert_eq!(arc.entity_id, 7);
    assert_eq!(arc.center, [Some(0.0); 3]);
    assert_eq!(arc.radius, None);
    assert_eq!(arc.endpoints, [[None; 3]; 2]);
    assert_eq!(arc.parameters, [None; 2]);
    let arc_body_start = b"\xe0\x00entity(arc)\0".len();
    let circle_label = b"\xe0\x00entity(circle)\0";
    let circle_offset = payload
        .windows(circle_label.len())
        .position(|window| window == circle_label)
        .expect("circle boundary");
    assert_eq!(arc.body, payload[arc_body_start..circle_offset]);
    assert_eq!(circle.entity_id, 8);
    assert_eq!(circle.center, [None; 3]);
    assert_eq!(circle.radius, Some(0.0));
    assert_eq!(circle.body, payload[circle_offset + circle_label.len()..]);
}

#[test]
fn saved_conic_retains_coefficients_parameters_and_planar_frame() {
    let payload = b"\xe0\x00entity(conic)\0\
            \xe0\x01id\0\x02\xe0\x01type\0\x3a\
            \xe0\x02end1\0\xf8\x03\x18\xe5\
            \xe0\x02end2\0\xf8\x03\x18\xe5\
            \xe0\x02t0\0\x0f\xe0\x02t1\0\xf6\
            \xe0\x02c1\0\xe4\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\
            \xe4\x0f\x0f\x0f\xe4\x18\xe5\x0f\x0f\x0f\x0f\
            \xe0\x01trailing_field\0\x07";

    let entities = saved_conic_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Conic(conic)] = entities.as_slice() else {
        panic!("one saved conic");
    };

    assert_eq!(conic.entity_id, 2);
    assert_eq!(conic.endpoints, [[Some(0.0), Some(1.0), Some(0.0)]; 2]);
    assert_eq!(conic.parameters, [Some(0.0), None]);
    assert_eq!(conic.coefficients, [Some(1.0); 2]);
    assert_eq!(
        conic.local_system,
        Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])
    );
    assert_eq!(conic.body, payload[b"\xe0\x00entity(conic)\0".len()..]);
}

#[test]
fn saved_arc_replay_uses_order_table_row_boundaries() {
    let mut payload = vec![0xe3, 7, 0xe2];
    payload.extend([0x0f; 12]);
    payload.push(0xe3);
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 42,
            internal_id: 7,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![FeatureSegment {
            kind: FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            body: Vec::new(),
            offset: 0,
        }],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 0,
    };

    let entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Arc(arc) = &entities[0] else {
        panic!("expected saved arc");
    };
    assert_eq!(arc.entity_id, 7);
    assert_eq!(arc.center, [Some(0.0); 3]);
    assert_eq!(arc.radius, Some(0.0));
    assert_eq!(arc.body, payload[1..payload.len() - 1]);
    let section = positional_saved_section(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    )
    .expect("positional saved section");
    assert_eq!(section.entities.len(), 1);
    assert_eq!(section.offset, 1);

    let named_prefix = b"\xe0\x00entity(arc)\0\xe0\x01id\0\x09";
    let mut named_payload = named_prefix.to_vec();
    named_payload.extend_from_slice(&payload);
    let named_entities = saved_circular_entities(
        &named_payload,
        0,
        named_payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );
    let [FeatureSavedEntity::Arc(named), FeatureSavedEntity::Arc(replay)] =
        named_entities.as_slice()
    else {
        panic!("named arc and replay");
    };
    assert_eq!(
        named.body, b"\xe0\x01id\0\x09",
        "named body must stop before the replay separator"
    );
    assert_eq!(replay.body, payload[1..payload.len() - 1]);
}

#[test]
fn saved_arc_replay_retains_a_structurally_terminated_scalar_prefix() {
    let mut payload = vec![0xe3, 7, 0xe2];
    payload.extend([0x0f; 6]);
    payload.push(0xe3);
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 42,
            internal_id: 7,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![FeatureSegment {
            kind: FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            body: Vec::new(),
            offset: 0,
        }],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 0,
    };

    let entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );

    let [FeatureSavedEntity::Arc(arc)] = entities.as_slice() else {
        panic!("expected saved arc");
    };
    assert_eq!(arc.entity_id, 7);
    assert_eq!(arc.center, [Some(0.0); 3]);
    assert_eq!(arc.radius, Some(0.0));
    assert_eq!(arc.endpoints[0], [Some(0.0), Some(0.0), None]);
    assert_eq!(arc.endpoints[1], [None; 3]);
    assert_eq!(arc.parameters, [None; 2]);
}

#[test]
fn saved_generated_line_requires_its_orientation_invariant() {
    let payload = [0xe3, 8, 0xe2, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe3];
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 43,
            internal_id: 8,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![FeatureSegment {
            kind: FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: None,
            arc_orientation: Some(0),
            vertical_horizontal: Some(1),
            radius_ref: None,
            radius2_ref: None,
            external_id: 43,
            body: Vec::new(),
            offset: 0,
        }],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 0,
    };

    let entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(line.entity_id, 8);
    assert_eq!(line.endpoints[0], [Some(0.0); 3]);
    assert_eq!(line.endpoints[1], [Some(1.0), Some(0.0), Some(0.0)]);
    assert_eq!(line.body, payload[1..payload.len() - 1]);
}

#[test]
fn decodes_mdlstatus_recipe_discriminators_within_their_records() {
    let payload = b"\xe3icon\0protextrude\0Protrusion id 40\0\xe2\xe3\
            icon\0protrevolve\0Revolve id 41\0\xe2\xe3\
            icon\0cutextrude\0Cut id 42\0\xe2\xe3\
            icon\0cutrevolve\0Cut id 43\0\xe2\xe3Datum Plane id 44\0\xe3K\xc3\xb6rper ID 45\0";
    let operations = operations(payload);
    assert_eq!(operations.len(), 6);
    assert_eq!(operations[0].recipe, Some(FeatureRecipe::ProtrudeExtrude));
    assert_eq!(operations[1].recipe, Some(FeatureRecipe::ProtrudeRevolve));
    assert_eq!(operations[2].recipe, Some(FeatureRecipe::CutExtrude));
    assert_eq!(operations[3].recipe, Some(FeatureRecipe::CutRevolve));
    assert_eq!(operations[4].recipe, None);
    assert_eq!(operations[5].kind, "Körper");
    assert_eq!(operations[5].feature_id, 45);
}

#[test]
fn preserves_mdlstatus_name_prefixes_without_using_them_as_state_selectors() {
    let payload = b"\xe3oExtrude id 7\0\xe3xExtrude id 7\0\xe3yExtrude id 7\0\xe3zExtrude ID 7\0";

    let states = operation_states(payload);
    assert_eq!(states.len(), 4);
    for (state, (prefix, expected_name)) in states.iter().zip([
        (b'o', "oExtrude id 7"),
        (b'x', "xExtrude id 7"),
        (b'y', "yExtrude id 7"),
        (b'z', "zExtrude ID 7"),
    ]) {
        assert_eq!(state.feature_id, 7);
        assert_eq!(state.kind, "Extrude");
        assert_eq!(state.stored_name_prefix, Some(prefix));
        assert!(state.display_state_conflict);
        assert_eq!(state.state_offset + 1, state.offset);
        assert_eq!(state.stored_name.as_deref(), Some(expected_name));
    }
    assert_eq!(states[3].identifier_keyword.as_deref(), Some("ID"));

    let current_operations = operations(payload);
    let [current] = current_operations.as_slice() else {
        panic!("one current operation");
    };
    assert_eq!(current.kind, "Extrude");
    assert!(!current.display_name_stored);
    assert_eq!(current.stored_name, None);
    assert_eq!(current.stored_name_bytes, None);
    assert_eq!(current.identifier_keyword, None);
    assert_eq!(current.stored_name_prefix, None);
    assert!(current.display_state_conflict);
}

#[test]
fn binds_depdb_recipe_records_to_compact_feature_ids() {
    let payload = b"\xe3K\xc3\xb6rper ID 247\0\xe3\
            \xf7\x3b\x80\xf7\x83\x95\xf6\x20Drehen 1\0\xf6\0protrevolve\0\
            \xe3Body ID 8053\0\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

    let operations = operations(payload);
    assert_eq!(operations.len(), 2);
    assert_eq!(operations[0].feature_id, 247);
    assert_eq!(operations[0].recipe, Some(FeatureRecipe::ProtrudeRevolve));
    assert_eq!(operations[0].root_schema_class, Some(917));
    assert_eq!(operations[0].parent_feature_id, Some(32));
    assert_eq!(operations[1].feature_id, 8053);
    assert_eq!(operations[1].recipe, Some(FeatureRecipe::ProtrudeExtrude));
    assert_eq!(operations[1].root_schema_class, Some(917));
    assert_eq!(operations[1].parent_feature_id, Some(8051));
}

#[test]
fn preserves_competing_depdb_recipe_bindings() {
    let payload = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0";

    let states = operation_states(payload);
    assert_eq!(states.len(), 2);
    assert_eq!(states[0].feature_id, 8053);
    assert_eq!(states[0].recipe, Some(FeatureRecipe::ProtrudeExtrude));
    assert!(states[0].recipe_conflict);
    assert_eq!(states[0].root_schema_class, Some(917));
    assert_eq!(states[1].feature_id, 8053);
    assert_eq!(states[1].recipe, Some(FeatureRecipe::CutExtrude));
    assert!(states[1].recipe_conflict);
    assert_eq!(states[1].root_schema_class, Some(916));

    let current = operations(payload);
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].feature_id, 8053);
    assert_eq!(current[0].kind, "Native Feature");
    assert_eq!(current[0].recipe, None);
    assert!(current[0].recipe_conflict);
    assert_eq!(current[0].root_schema_class, None);
    assert_eq!(current[0].parent_feature_id, None);

    let repeated = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 2\0\xf6\0protextrude\0";
    let repeated_states = operation_states(repeated);
    assert_eq!(repeated_states.len(), 2);
    assert_eq!(repeated_states[0].recipe, repeated_states[1].recipe);
    assert_ne!(repeated_states[0].offset, repeated_states[1].offset);
    let repeated_current = operations(repeated);
    assert_eq!(repeated_current.len(), 1);
    assert_eq!(repeated_current[0].kind, "Extrude");
    assert_eq!(
        repeated_current[0].recipe,
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(repeated_current[0].root_schema_class, Some(917));
    assert_eq!(repeated_current[0].parent_feature_id, Some(8051));
}

#[test]
fn conflicting_bindings_do_not_use_an_inline_recipe_fallback() {
    let payload = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0\
            \xe3icon\0protextrude\0Extrude id 8053\0";

    let states = operation_states(payload);
    let display = states
        .iter()
        .find(|state| state.display_name_stored)
        .expect("stored display state");
    assert_eq!(display.kind, "Extrude");
    assert_eq!(display.recipe, None);
    assert!(display.recipe_conflict);
    assert_eq!(display.root_schema_class, None);
    assert_eq!(display.parent_feature_id, None);

    let current = operations(payload);
    let [current] = current.as_slice() else {
        panic!("one current operation");
    };
    assert_eq!(current.kind, "Extrude");
    assert!(current.display_name_stored);
    assert_eq!(current.recipe, None);
    assert!(current.recipe_conflict);
}

#[test]
fn leaves_inline_recipe_conflicts_unresolved() {
    let payload = b"\xe3icon\0protextrude\0cutextrude\0Extrude id 9\0";

    let states = operation_states(payload);
    let [state] = states.as_slice() else {
        panic!("one operation state");
    };
    assert_eq!(state.feature_id, 9);
    assert_eq!(state.kind, "Extrude");
    assert_eq!(state.recipe, None);
    assert!(state.recipe_conflict);
    assert_eq!(state.root_schema_class, None);
    assert_eq!(state.parent_feature_id, None);
}

#[test]
fn promotes_depdb_recipe_without_operation_display_name() {
    let payload = b"\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

    let operations = operations(payload);
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].feature_id, 8053);
    assert_eq!(operations[0].kind, "Extrude");
    assert_eq!(operations[0].recipe, Some(FeatureRecipe::ProtrudeExtrude));
    assert_eq!(operations[0].root_schema_class, Some(917));
    assert_eq!(operations[0].parent_feature_id, Some(8051));
    assert_eq!(operations[0].offset, 1);
}

#[test]
fn decodes_count_bounded_saved_spline_interpolation_points() {
    let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x02\x03\
            \xe4\x0f\x0d\x0f\xe4\x0f\
            \xe0\x02end_tangts\0\xf9\x02\x03\
            \xe4\x0f\x0f\xe4\x0f\x0f\
            \xe0\x02params\0\xf8\x02\x0f\xe4\
            \xe0\x01tan_cond\0\x00";

    let entities =
        saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };
    assert_eq!(spline.entity_id, Some(7));
    assert_eq!(spline.declared_point_count, Some(2));
    assert_eq!(
        spline.interpolation_points,
        [[1.0, 0.0, -1.0], [0.0, 1.0, 0.0]]
    );
    assert_eq!(
        spline.interpolation_points_body,
        b"\xf9\x02\x03\xe4\x0f\x0d\x0f\xe4\x0f"
    );
    assert_eq!(
        spline.endpoint_tangents,
        Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
    );
    assert_eq!(
        spline.endpoint_tangents_body.as_deref(),
        Some(b"\xf9\x02\x03\xe4\x0f\x0f\xe4\x0f\x0f".as_slice())
    );
    assert_eq!(spline.parameters, Some(vec![0.0, 1.0]));
    assert_eq!(
        spline.parameters_body.as_deref(),
        Some(b"\xf8\x02\x0f\xe4".as_slice())
    );
}

#[test]
fn decodes_compact_saved_spline_point_count() {
    let mut payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x80\x88\x03"
        .to_vec();
    payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

    let entities =
        saved_spline_entities(&payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };
    assert_eq!(spline.declared_point_count, Some(136));
    assert_eq!(spline.interpolation_points.len(), 136);
    assert_eq!(
        spline.interpolation_points_body,
        payload[b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x01id\0\x07\xe0\x02i_pnts\0".len()..]
    );
    assert!(spline
        .interpolation_points
        .iter()
        .all(|point| *point == [0.0; 3]));
}

#[test]
fn saved_spline_retains_its_declared_count_and_complete_point_prefix() {
    let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x02\x03\
            \x0f\x0f\x0f\xe0\x01tan_cond\0\x00";

    let entities =
        saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };

    assert_eq!(spline.entity_id, Some(7));
    assert_eq!(spline.declared_point_count, Some(2));
    assert_eq!(spline.interpolation_points, [[0.0; 3]]);
    assert_eq!(
        spline.interpolation_points_body,
        b"\xf9\x02\x03\x0f\x0f\x0f"
    );
    assert_eq!(spline.endpoint_tangents, None);
    assert_eq!(spline.endpoint_tangents_body, None);
    assert_eq!(spline.parameters, None);
    assert_eq!(spline.parameters_body, None);
}

#[test]
fn saved_spline_retains_its_identity_without_a_point_table() {
    let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x01id\0\x07";

    let entities =
        saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };

    assert_eq!(spline.entity_id, Some(7));
    assert_eq!(spline.declared_point_count, None);
    assert!(spline.interpolation_points.is_empty());
    assert!(spline.interpolation_points_body.is_empty());
}

#[test]
fn saved_spline_retains_a_valid_point_wrapper_when_allocation_is_rejected() {
    let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x02i_pnts\0\xf9\xbf\xff\x03";

    let entities =
        saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };

    assert_eq!(spline.declared_point_count, Some(16_383));
    assert!(spline.interpolation_points.is_empty());
    assert_eq!(spline.interpolation_points_body, b"\xf9\xbf\xff\x03");
}

#[test]
fn decodes_compact_feature_scalar_array_extents() {
    let mut payload = vec![psb::token::SCALAR_BODY, 0x80, 0x88, 0x03];
    payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

    let FeatureFieldValue::ScalarArray {
        dimensions,
        count,
        body,
        decoded_values,
    } = field_value(&payload)
    else {
        panic!("scalar array");
    };
    assert_eq!(dimensions, 136);
    assert_eq!(count, 3);
    assert_eq!(body.len(), 408);
    assert_eq!(decoded_values, Some(vec![0.0; 408]));
}

#[test]
fn decodes_saved_spline_chord_parameter_lane() {
    let body = [
        0x18, 0x6d, 0x31, 0xd2, 0x2a, 0x7f, 0x68, 0x39, 0x85, 0x06, 0x5f, 0x25, 0x83, 0xf4, 0x6c,
        0x93, 0xd8, 0xd4, 0xfb, 0x45, 0xbc, 0x38, 0x9e, 0x51, 0xef, 0x1e, 0x96, 0xe2, 0x6c, 0x2d,
        0x1a, 0xfc, 0x59, 0x51, 0xbd, 0x0a, 0x38,
    ];
    let cache = scalar::ScalarCache::default();
    let expected = [
        0.0,
        0.568_581_660_273_827_7,
        1.626_555_582_565_994_3,
        3.105_874_980_035_448_4,
        4.830_013_730_963_952,
        6.746_434_476_054_269,
    ];
    let mut cursor = 0;
    for expected in expected {
        let (value, next) = saved_spline_parameter(&body, cursor, &cache).expect("parameter");
        assert_eq!(value, expected);
        cursor = next;
    }
    assert_eq!(cursor, body.len());
}

#[test]
fn decodes_zero_offset_positional_placement_instruction() {
    let payload = b"place_instruction_ptrs\0\xf8\x03\xf7\x0b\xfb\xe3\
            \xf1\xf7\x0b\xe3\xc0\x4e\x9f\x18\xf6\xf6\x02\xf6\x00\x00\x00\xe6";
    let rows = placement_instruction_rows(payload, 1000);
    let [row] = rows.as_slice() else {
        panic!("placement row");
    };
    assert_eq!(row.kind, 20_127);
    assert!(row.zero_offset);
    assert_eq!(row.dimension_id, None);
    assert_eq!(row.reference_id, None);
    assert_eq!(row.geometry1_id, Some(2));
    assert_eq!(row.geometry2_id, None);
    assert_eq!([row.member1, row.member2], [0, 0]);
    assert_eq!(row.offset, 1029);
}

#[test]
fn model_reference_entry_joins_feature_name_to_feature_id() {
    let payload = b"\0\xf7\x71\x2a\x05\x29Datum Plane id 41\0\x2a\x2a\x10\0\
            \xf7\x71\x30\x05\x2fBroken\0\x30\x31";

    assert_eq!(
        reference_names(payload),
        [FeatureReferenceName {
            feature_id: 41,
            name: "Datum Plane id 41".to_string(),
            name_bytes: b"Datum Plane id 41".to_vec(),
            own_reference_id: 42,
            reference_type: 5,
            offset: 1,
        }]
    );
}
