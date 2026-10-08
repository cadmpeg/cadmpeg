// SPDX-License-Identifier: Apache-2.0

use super::*;

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
        spline.endpoint_tangents.as_ref().map(|field| field.value),
        Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
    );
    assert_eq!(
        spline
            .endpoint_tangents
            .as_ref()
            .map(|field| field.body.as_slice()),
        Some(b"\xf9\x02\x03\xe4\x0f\x0f\xe4\x0f\x0f".as_slice())
    );
    assert_eq!(
        spline.parameters.as_ref().map(|field| field.value.clone()),
        Some(vec![0.0, 1.0])
    );
    assert_eq!(
        spline
            .parameters
            .as_ref()
            .map(|field| field.body.as_slice()),
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
    assert_eq!(spline.parameters, None);
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

