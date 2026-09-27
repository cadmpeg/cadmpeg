// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{FeatureChoice, FeatureFieldValue, FeatureRow};

fn run<T>(
    input: &[u8],
    items: u64,
    retained: u64,
    parse: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
        .expect("root choice input is admitted");
    parse(&ctx)
}

fn item(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
}

fn retained(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == operation));
}

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
fn choice_hit_refuses_before_vec_growth() {
    let row = row();
    item(
        run(&row.body, 0, u64::MAX, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect_err("one recognized label needs a hit item"),
        "creo choice label hits",
    );
}

#[test]
fn choice_label_refuses_before_retained_text_copy() {
    let row = row();
    retained(
        run(&row.body, 2, 0, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect_err("choice label needs retained text"),
        "creo feature choice label",
    );
}

#[test]
fn choice_payload_refuses_before_retained_byte_copy() {
    let row = row();
    retained(
        run(&row.body, 2, "blend_choice".len() as u64, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect_err("choice payload needs retained bytes"),
        "creo feature choice payload",
    );
}

#[test]
fn choice_record_refuses_before_vec_growth() {
    let row = row();
    assert_eq!(
        run(&row.body, 2, u64::MAX, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect("one choice admitted")
        .len(),
        1
    );
    item(
        run(&row.body, 1, u64::MAX, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect_err("choice result needs another item"),
        "creo feature choices",
    );
}

#[test]
fn raw_feature_field_refuses_before_retained_copy() {
    let payload = [0xff, 0x00];
    retained(
        run(&payload, 0, 0, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("raw bytes need retained admission"),
        "creo feature raw field",
    );
}

#[test]
fn scalar_feature_field_cache_refuses_before_hashset_growth() {
    let payload = [0xf9, 0x01, 0x01, 0x46, 0, 0, 0, 0, 0, 0, 0];
    item(
        run(&payload, 0, u64::MAX, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("scalar image requires a cache item"),
        "creo scalar cache unique images",
    );
}

#[test]
fn scalar_feature_values_refuse_before_vec_growth() {
    let payload = [0xf9, 0x01, 0x01, 0x0f];
    let value = run(&payload, 1, u64::MAX, |ctx| {
        super::super::field_value(ctx, &payload)
    })
    .expect("one scalar value admitted");
    assert!(
        matches!(value, FeatureFieldValue::ScalarArray { decoded_values: Some(values), .. } if values == [0.0])
    );
    item(
        run(&payload, 0, u64::MAX, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("decoded scalar needs one item"),
        "creo feature scalar values",
    );
}

#[test]
fn scalar_feature_body_refuses_before_retained_copy() {
    let payload = [0xf9, 0x01, 0x01, 0x0f];
    retained(
        run(&payload, 1, 0, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("scalar body needs retained bytes"),
        "creo feature scalar field body",
    );
}

#[test]
fn compact_feature_integer_refuses_before_vec_growth() {
    let payload = [0xf8, 0x01, 0x07];
    item(
        run(&payload, 0, u64::MAX, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("compact array needs one value item"),
        "creo feature compact integer values",
    );
}

#[test]
fn choice_field_header_refuses_before_vec_growth() {
    let choice = field_choice(b"\xe0\x01foo\0\xf8\x01\x07".to_vec());
    item(
        run(&choice.payload, 0, u64::MAX, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect_err("field header needs one item"),
        "creo choice field headers",
    );
}

#[test]
fn choice_field_label_refuses_before_retained_text_copy() {
    let choice = field_choice(b"\xe0\x01foo\0\xf8\x01\x07".to_vec());
    retained(
        run(&choice.payload, 3, 0, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect_err("copied choice label needs retained bytes"),
        "creo choice field label",
    );
}

#[test]
fn choice_field_name_refuses_before_retained_text_copy() {
    let choice = field_choice(b"\xe0\x01foo\0\xf8\x01\x07".to_vec());
    retained(
        run(&choice.payload, 3, choice.label.len() as u64, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect_err("field name needs retained bytes"),
        "creo choice field name",
    );
}

#[test]
fn choice_field_record_refuses_before_vec_growth() {
    let choice = field_choice(b"\xe0\x01foo\0\xf8\x01\x07".to_vec());
    assert_eq!(
        run(&choice.payload, 3, u64::MAX, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect("one choice field admitted")
        .len(),
        1
    );
    item(
        run(&choice.payload, 2, u64::MAX, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect_err("choice field result needs one item"),
        "creo choice fields",
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
fn named_datum_ids_refuse_before_vec_growth() {
    let row = named_datum_row();
    item(
        run(&row.body, 0, u64::MAX, |ctx| {
            super::super::geometry_tables(ctx, std::slice::from_ref(&row))
        })
        .expect_err("named datum id needs one item"),
        "creo named datum ids",
    );
}

#[test]
fn feature_geometry_table_refuses_before_vec_growth() {
    let row = named_datum_row();
    item(
        run(&row.body, 1, u64::MAX, |ctx| {
            super::super::geometry_tables(ctx, std::slice::from_ref(&row))
        })
        .expect_err("geometry table needs one result item"),
        "creo feature geometry tables",
    );
}

#[test]
fn datum_class_stream_refuses_before_btree_insertion() {
    let row = named_datum_row();
    assert_eq!(
        run(&row.body, u64::MAX, u64::MAX, |ctx| {
            super::super::geometry_tables(ctx, std::slice::from_ref(&row))
        })
        .expect("named datum table admitted")
        .len(),
        1
    );
    item(
        run(&row.body, 2, u64::MAX, |ctx| {
            super::super::geometry_tables(ctx, std::slice::from_ref(&row))
        })
        .expect_err("datum stream class needs one node"),
        "creo datum class by stream",
    );
}

#[test]
fn positional_datum_ids_refuse_before_counted_vec_reserve() {
    let body = [
        0x00, 0xf8, 0x02, 0xf7, 0x57, 0xfb, 0xe2, 0xf7, 0x58, 0x80, 0x91, 0xf6, 0xf1, 0xf7, 0x57,
        0xe2, 0x80, 0x92, 0xf6, 0xe3,
    ];
    item(
        run(&body, 1, u64::MAX, |ctx| {
            super::super::positional_datum_geometry_table_at(ctx, &body, 1, 87)
                .transpose()
                .map(|decoded| decoded.map(|(_, ids)| ids))
        })
        .expect_err("two positional ids need two items"),
        "creo positional datum ids",
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
fn affected_ids_refuse_before_counted_vec_reserve() {
    let row = affected_row();
    item(
        run(&row.body, 0, u64::MAX, |ctx| {
            super::super::affected_ids(ctx, std::slice::from_ref(&row))
        })
        .expect_err("one affected id needs one item"),
        "creo affected ids",
    );
}

#[test]
fn affected_id_record_refuses_before_vec_growth() {
    let row = affected_row();
    assert_eq!(
        run(&row.body, 2, u64::MAX, |ctx| {
            super::super::affected_ids(ctx, std::slice::from_ref(&row))
        })
        .expect("one affected-id record admitted")
        .len(),
        1
    );
    item(
        run(&row.body, 1, u64::MAX, |ctx| {
            super::super::affected_ids(ctx, std::slice::from_ref(&row))
        })
        .expect_err("affected-id record needs another item"),
        "creo affected-id records",
    );
}

#[test]
fn round_replay_scalar_refuses_before_vec_growth() {
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
        run(body, 1, u64::MAX, |ctx| {
            super::super::round_replay_scalars(ctx, std::slice::from_ref(&row))
        })
        .expect("one round scalar admitted")
        .len(),
        1
    );
    item(
        run(body, 0, u64::MAX, |ctx| {
            super::super::round_replay_scalars(ctx, std::slice::from_ref(&row))
        })
        .expect_err("round scalar needs one item"),
        "creo round replay scalars",
    );
}

#[test]
fn loop_restore_direction_refuses_before_vec_growth() {
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
        run(body, 1, u64::MAX, |ctx| {
            super::super::loop_restore_directions(ctx, std::slice::from_ref(&row))
        })
        .expect("one restore direction admitted")
        .len(),
        1
    );
    item(
        run(body, 0, u64::MAX, |ctx| {
            super::super::loop_restore_directions(ctx, std::slice::from_ref(&row))
        })
        .expect_err("restore direction needs one item"),
        "creo loop restore directions",
    );
}

#[test]
fn feature_revolution_extent_refuses_before_vec_growth() {
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
        run(body, 1, u64::MAX, |ctx| {
            super::super::revolution_extents(ctx, std::slice::from_ref(&row))
        })
        .expect("one revolution extent admitted")
        .len(),
        1
    );
    item(
        run(body, 0, u64::MAX, |ctx| {
            super::super::revolution_extents(ctx, std::slice::from_ref(&row))
        })
        .expect_err("revolution extent needs one item"),
        "creo feature revolution extents",
    );
}

#[test]
fn replay_id_array_refuses_before_nested_vec_growth() {
    let bytes = [10, 11];
    item(
        run(&bytes, 1, u64::MAX, |ctx| {
            super::super::replay_ids(ctx, &bytes, 2, 0)
                .transpose()?
                .ok_or_else(|| CodecError::malformed("replay ids"))
                .map(|_| ())
        })
        .expect_err("two replay IDs need two items"),
        "creo replay affected ids",
    );
    assert!(run(&bytes, 2, u64::MAX, |ctx| {
        super::super::replay_ids(ctx, &bytes, 2, 0)
            .transpose()?
            .ok_or_else(|| CodecError::malformed("replay ids"))
    })
    .is_ok());
}

#[test]
fn explicit_replay_array_record_refuses_before_outer_vec_growth() {
    let row = super::unanchored_replay_row(1, 40, None, &[0xf8, 1, 10, 0xf8, 1, 20]);
    let suffix = row
        .body
        .windows(2)
        .position(|bytes| bytes == [0xe1, 0xe1])
        .expect("replay suffix");
    item(
        run(&row.body, 1, u64::MAX, |ctx| {
            super::super::explicit_replay_pair_before_suffix(ctx, &row, suffix)
                .transpose()?
                .ok_or_else(|| CodecError::malformed("replay pair"))
                .map(|_| ())
        })
        .expect_err("the first explicit array needs an outer item"),
        "creo explicit replay arrays",
    );
    assert!(run(&row.body, 4, u64::MAX, |ctx| {
        super::super::explicit_replay_pair_before_suffix(ctx, &row, suffix)
            .transpose()?
            .ok_or_else(|| CodecError::malformed("replay pair"))
    })
    .is_ok());
}

#[test]
fn unanchored_replay_candidate_refuses_before_vec_growth() {
    let row = super::unanchored_replay_row(1, 40, None, &[0xf8, 1, 10, 0xf8, 1, 20]);
    item(
        run(&row.body, 4, u64::MAX, |ctx| {
            super::super::unique_unanchored_replay_pair(ctx, &row, [None; 2])
                .transpose()?
                .ok_or_else(|| CodecError::malformed("replay candidate"))
                .map(|_| ())
        })
        .expect_err("one candidate needs one more item"),
        "creo replay candidates",
    );
    assert!(run(&row.body, 5, u64::MAX, |ctx| {
        super::super::unique_unanchored_replay_pair(ctx, &row, [None; 2])
            .transpose()?
            .ok_or_else(|| CodecError::malformed("replay candidate"))
    })
    .is_ok());
}

#[test]
fn replay_extent_state_refuses_before_btree_insertion() {
    let row = super::replay_row(1, &[0xf8, 1, 10, 0xf8, 1, 20]);
    item(
        run(&row.body, 0, u64::MAX, |ctx| {
            super::super::replay_affected_ids(ctx, std::slice::from_ref(&row))
        })
        .expect_err("one extent state needs one item"),
        "creo replay extent states",
    );
}

#[test]
fn replay_affected_record_refuses_before_vec_growth() {
    let row = super::replay_row(1, &[0xf8, 1, 10, 0xf8, 1, 20]);
    item(
        run(&row.body, 3, u64::MAX, |ctx| {
            super::super::replay_affected_ids(ctx, std::slice::from_ref(&row))
        })
        .expect_err("one replay record needs a fourth item"),
        "creo replay affected-id records",
    );
    assert_eq!(
        run(&row.body, 4, u64::MAX, |ctx| {
            super::super::replay_affected_ids(ctx, std::slice::from_ref(&row))
        })
        .expect("one replay record admitted")
        .len(),
        1
    );
}

#[test]
fn surface_merge_extent_state_refuses_before_btree_insertion() {
    let row = super::surface_merge_row(1, 40, &[]);
    item(
        run(&row.body, 0, u64::MAX, |ctx| {
            super::super::surface_merge_replay_affected_ids(ctx, std::slice::from_ref(&row), &[])
        })
        .expect_err("one surface merge state needs one item"),
        "creo surface merge extent states",
    );
}

#[test]
fn surface_merge_nested_arrays_refuse_before_each_vec_growth() {
    let row = super::surface_merge_row(
        1,
        40,
        &[
            0xf8, 1, 10, 0xf8, 1, 20, 0xf0, 0xf7, 0x80, 0x99, 0xf8, 1, 30,
        ],
    );
    for admitted in 0..4 {
        item(
            run(&row.body, admitted, u64::MAX, |ctx| {
                super::super::positional_surface_merge_affected_ids(ctx, &row, [None; 3])
                    .transpose()?
                    .ok_or_else(|| CodecError::malformed("surface merge arrays"))
            })
            .expect_err("one of four nested arrays needs another item"),
            "creo replay affected ids",
        );
    }
    assert!(run(&row.body, 4, u64::MAX, |ctx| {
        super::super::positional_surface_merge_affected_ids(ctx, &row, [None; 3])
            .transpose()?
            .ok_or_else(|| CodecError::malformed("surface merge arrays"))
    })
    .is_ok());
}

#[test]
fn surface_merge_record_refuses_before_vec_growth() {
    let row = super::surface_merge_row(
        1,
        40,
        &[
            0xf8, 1, 10, 0xf8, 1, 20, 0xf0, 0xf7, 0x80, 0x99, 0xf8, 1, 30,
        ],
    );
    item(
        run(&row.body, 5, u64::MAX, |ctx| {
            super::super::surface_merge_replay_affected_ids(ctx, std::slice::from_ref(&row), &[])
        })
        .expect_err("one surface merge record needs another item"),
        "creo surface merge affected-id records",
    );
    assert_eq!(
        run(&row.body, 6, u64::MAX, |ctx| {
            super::super::surface_merge_replay_affected_ids(ctx, std::slice::from_ref(&row), &[])
        })
        .expect("one surface merge record admitted")
        .len(),
        1
    );
}

#[test]
fn loop_history_roster_refuses_before_counted_vec_growth() {
    let body = [42, 1, 2, 3, 4, 0xe3];
    item(
        run(&body, 0, u64::MAX, |ctx| {
            super::super::loop_history_roster(ctx, &body, 0, 1)
                .transpose()?
                .ok_or_else(|| CodecError::malformed("loop history roster"))
                .map(|_| ())
        })
        .expect_err("one roster row needs one item"),
        "creo loop history roster",
    );
    assert!(run(&body, 1, u64::MAX, |ctx| {
        super::super::loop_history_roster(ctx, &body, 0, 1)
            .transpose()?
            .ok_or_else(|| CodecError::malformed("loop history roster"))
    })
    .is_ok());
}

#[test]
fn loop_history_fields_refuse_before_each_retained_copy() {
    let body = [42, 1, 2, 3, 4, 0xe3];
    for admitted in 0..4 {
        retained(
            run(&body, 1, admitted, |ctx| {
                super::super::loop_history_roster(ctx, &body, 0, 1)
                    .transpose()?
                    .ok_or_else(|| CodecError::malformed("loop history fields"))
                    .map(|_| ())
            })
            .expect_err("one field byte needs retained admission"),
            "creo loop history field bytes",
        );
    }
}

#[test]
fn loop_history_trailing_field_refuses_before_retained_copy() {
    let body = b"\x2a\x01\x02\x03\x04\x07\xe0\x00next\0";
    retained(
        run(body, 1, 4, |ctx| {
            super::super::loop_history_roster(ctx, body, 0, 1)
                .transpose()?
                .ok_or_else(|| CodecError::malformed("loop history trailing field"))
                .map(|_| ())
        })
        .expect_err("trailing field needs retained admission"),
        "creo loop history trailing bytes",
    );
    assert!(run(body, 1, 5, |ctx| {
        super::super::loop_history_roster(ctx, body, 0, 1)
            .transpose()?
            .ok_or_else(|| CodecError::malformed("loop history trailing field"))
    })
    .is_ok());
}

#[test]
fn loop_history_result_refuses_before_vec_growth() {
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
    item(
        run(body, 1, u64::MAX, |ctx| {
            super::super::loop_history_entries(
                ctx,
                std::slice::from_ref(&row),
                std::slice::from_ref(&table),
            )
        })
        .expect_err("one result row needs another item"),
        "creo loop history entries",
    );
    assert_eq!(
        run(body, 2, u64::MAX, |ctx| {
            super::super::loop_history_entries(
                ctx,
                std::slice::from_ref(&row),
                std::slice::from_ref(&table),
            )
        })
        .expect("one loop history result admitted")
        .len(),
        1
    );
}
