// SPDX-License-Identifier: Apache-2.0

use super::super::{FeatureChoice, FeatureFieldValue, FeatureRow};

fn row() -> FeatureRow {
    let body = b"\xe0\x01blend_choice\0\x00".to_vec();
    FeatureRow {
        feature_id: 7,
        root_schema_class: None,
        stream_offset: 0,
        body: body.try_into().expect("two-byte row body"),
        body_offset: 0,
        offset: 0,
    }
}

fn field_choice(payload: Vec<u8>) -> FeatureChoice {
    FeatureChoice {
        feature_id: 7,
        label: "blend_choice".to_string(),
        type_byte: Some(1),
        payload,
        payload_offset: 0,
        offset: 0,
    }
}

#[test]
fn choices_decode_one_recognized_label() {
    let row = row();
    assert_eq!(
        super::super::choices(std::slice::from_ref(&row))
            .expect("one choice decodes")
            .len(),
        1
    );
}

#[test]
fn scalar_feature_field_decodes_zero_value() {
    let payload = [0xf9, 0x01, 0x01, 0x0f];
    let value = super::super::field_value(&payload);
    assert!(
        matches!(value, FeatureFieldValue::ScalarArray { decoded_values: Some(values), .. } if values == [0.0])
    );
}

#[test]
fn choice_fields_decode_one_field() {
    let choice = field_choice(b"\xe0\x01foo\0\xf8\x01\x07".to_vec());
    assert_eq!(
        super::super::choice_fields(std::slice::from_ref(&choice))
            .expect("one choice field decodes")
            .len(),
        1
    );
}

fn named_datum_row() -> FeatureRow {
    FeatureRow {
        feature_id: 7,
        root_schema_class: None,
        stream_offset: 10,
        body: b"\xe0\x00dtm_id_tab\0\xf2\xf8\x01\xf7\x57\xfb\xe2\
            \xe0\x01dtm_id\0\x2a\xe0\x01dim_id\0\xf6"
            .to_vec()
            .try_into()
            .expect("two-byte datum row"),
        body_offset: 100,
        offset: 98,
    }
}

#[test]
fn named_datum_row_decodes_one_geometry_table() {
    let row = named_datum_row();
    assert_eq!(
        super::super::geometry_tables(std::slice::from_ref(&row))
            .expect("named datum table decodes")
            .len(),
        1
    );
}

fn affected_row() -> FeatureRow {
    FeatureRow {
        feature_id: 7,
        root_schema_class: None,
        stream_offset: 0,
        body: b"\xe0\x01geoms_affected\0\xf8\x01\x2a"
            .to_vec()
            .try_into()
            .expect("two-byte affected row"),
        body_offset: 100,
        offset: 98,
    }
}

#[test]
fn affected_ids_decode_one_record() {
    let row = affected_row();
    assert_eq!(
        super::super::affected_ids(std::slice::from_ref(&row)).len(),
        1
    );
}

#[test]
fn round_replay_scalars_decode_one_scalar() {
    let body = b"\xf2\xf7\x80\xa0\x01\xf6\x29\xc9\x99\xf3\xf7\x80\x97\xe2";
    let row = FeatureRow {
        feature_id: 17,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: body.to_vec().try_into().expect("two-byte round row"),
        body_offset: 0,
        offset: 0,
    };
    assert_eq!(
        super::super::round_replay_scalars(std::slice::from_ref(&row)).len(),
        1
    );
}

#[test]
fn loop_restore_directions_decode_one_direction() {
    let body = b"lo_restore\0\xe0\x01direction\0\x01";
    let row = FeatureRow {
        feature_id: 17,
        root_schema_class: None,
        stream_offset: 0,
        body: body.to_vec().try_into().expect("two-byte restore row"),
        body_offset: 0,
        offset: 0,
    };
    assert_eq!(
        super::super::loop_restore_directions(std::slice::from_ref(&row)).len(),
        1
    );
}

#[test]
fn revolution_extents_decode_one_extent() {
    let body = b"\xe3\xf6\x83\x95\xe1\x02\x83\xdf\xf6\xe3\
        \x00\x00\xea\x44\x00\x00\xf6\xf6\xf6\x00\x00\x00\x00";
    let row = FeatureRow {
        feature_id: 17,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: body.to_vec().try_into().expect("two-byte revolution row"),
        body_offset: 0,
        offset: 0,
    };
    assert_eq!(
        super::super::revolution_extents(std::slice::from_ref(&row)).len(),
        1
    );
}

#[test]
fn replay_ids_decode_two_ids() {
    let bytes = [10, 11];
    assert!(matches!(
        super::super::replay_ids(&bytes, 2, 0),
        Some(Ok(_))
    ));
}

#[test]
fn explicit_replay_pair_decodes_before_suffix() {
    let row = super::unanchored_replay_row(1, 40, None, &[0xf8, 1, 10, 0xf8, 1, 20]);
    let suffix = row
        .body
        .windows(2)
        .position(|bytes| bytes == [0xe1, 0xe1])
        .expect("replay suffix");
    assert!(matches!(
        super::super::explicit_replay_pair_before_suffix(&row, suffix),
        Some(Ok(_))
    ));
}

#[test]
fn unanchored_replay_row_decodes_one_candidate() {
    let row = super::unanchored_replay_row(1, 40, None, &[0xf8, 1, 10, 0xf8, 1, 20]);
    assert!(matches!(
        super::super::unique_unanchored_replay_pair(&row, [None; 2]),
        Some(Ok(_))
    ));
}

#[test]
fn replay_affected_ids_decode_one_record() {
    let row = super::replay_row(1, &[0xf8, 1, 10, 0xf8, 1, 20]);
    assert_eq!(
        super::super::replay_affected_ids(std::slice::from_ref(&row))
            .expect("one replay record decodes")
            .len(),
        1
    );
}

fn surface_merge_row() -> FeatureRow {
    super::surface_merge_row(
        1,
        40,
        &[
            0xf8, 1, 10, 0xf8, 1, 20, 0xf0, 0xf7, 0x80, 0x99, 0xf8, 1, 30,
        ],
    )
}

#[test]
fn positional_surface_merge_decodes_nested_arrays() {
    let row = surface_merge_row();
    assert!(matches!(
        super::super::positional_surface_merge_affected_ids(&row, [None; 3]),
        Some(Ok(_))
    ));
}

#[test]
fn surface_merge_replay_affected_ids_decode_one_record() {
    let row = surface_merge_row();
    assert_eq!(
        super::super::surface_merge_replay_affected_ids(std::slice::from_ref(&row), &[])
            .expect("one surface merge record decodes")
            .len(),
        1
    );
}

#[test]
fn loop_history_roster_decodes_one_row() {
    let body = [42, 1, 2, 3, 4, 0xe3];
    assert!(matches!(
        super::super::loop_history_roster(&body, 0, 1),
        Some(Ok(_))
    ));
}

#[test]
fn loop_history_entries_decode_one_result() {
    let body = b"\xe0\x00lo_id_tab_ptr\0\xf8\x01\xf7\x60\xfb\xe3\
                 \xe0\x01lo_hist\0\xf8\x06\x2a\x01\x02\x03\x04\xe3";
    let row = FeatureRow {
        feature_id: 7,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 10,
        body: body.to_vec().try_into().expect("loop history row"),
        body_offset: 1000,
        offset: 998,
    };
    let table = super::super::FeatureGeometryTable {
        feature_id: 7,
        kind: super::super::FeatureGeometryTableKind::LoopIds,
        count: 1,
        entity_class: 96,
        offset: 1000,
    };
    assert_eq!(
        super::super::loop_history_entries(
            std::slice::from_ref(&row),
            std::slice::from_ref(&table),
        )
        .expect("one loop history result decodes")
        .len(),
        1
    );
}
