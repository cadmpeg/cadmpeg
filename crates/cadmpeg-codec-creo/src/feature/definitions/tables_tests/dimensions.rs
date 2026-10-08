// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn named_dimension_value_body_refuses_before_copy() {
    assert!(
        matches!(named_dimension_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo dimension value body"), |cap| named_dimension_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension value body")
    );
    assert_eq!(
        named_dimension_with_limits(u64::MAX, crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| named_dimension_with_limits(u64::MAX, cap)
            ))
        .expect("dimension admitted")
        .expect("dimension present")
        .rows
        .len(),
        1
    );
}

#[test]
fn named_dimension_auxiliary_body_refuses_before_copy() {
    assert!(
        matches!(named_dimension_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo dimension auxiliary body"), |cap| named_dimension_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension auxiliary body")
    );
    assert_eq!(
        named_dimension_with_limits(u64::MAX, crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| named_dimension_with_limits(u64::MAX, cap)
            ))
        .expect("dimension admitted")
        .expect("dimension present")
        .rows
        .len(),
        1
    );
}

#[test]
fn positional_dimension_unresolved_token_refuses_before_copy() {
    let run = |limit| {
        with_dimension_limits(POSITIONAL_DIMENSION_LIMIT_INPUT, u64::MAX, limit, |ctx| {
            parse_positional_dimension(
                ctx,
                POSITIONAL_DIMENSION_LIMIT_INPUT,
                0,
                POSITIONAL_DIMENSION_LIMIT_INPUT.len(),
                &scalar::ScalarCache::default(),
            )
        })
    };
    assert!(matches!(run(crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo dimension unresolved token"), run)),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension unresolved token"));
    assert_eq!(
        run(u64::MAX)
            .expect("dimension admitted")
            .expect("row present")
            .value
            .unresolved_token(),
        Some(&[0x00, 0x04, 0xa6][..])
    );
}

#[test]
fn positional_dimension_rows_refuse_before_each_append() {
    crate::test_support::assert_refusal_order(ResourceDimension::CollectionItems,
        &["creo dimension rows", "creo dimension rows"], positional_dimension_table_with_limit);
    let table = positional_dimension_table_with_limit(u64::MAX)
        .expect("dimension table admitted")
        .expect("table present");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0].external_id, 43);
    assert_eq!(table.rows[1].external_id, 44);
}

#[test]
fn positional_dimension_value_body_refuses_before_copy() {
    assert!(matches!(crate::test_support::last_refusal_at(POSITIONAL_DIMENSION_LIMIT_INPUT, ResourceDimension::RetainedBytes, "creo dimension value body", |ctx| {
            parse_positional_dimension(ctx, POSITIONAL_DIMENSION_LIMIT_INPUT, 0,
                POSITIONAL_DIMENSION_LIMIT_INPUT.len(), &scalar::ScalarCache::default())
        }),
CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension value body"));
}

#[test]
fn positional_dimension_auxiliary_body_refuses_before_copy() {
    assert!(matches!(crate::test_support::last_refusal_at(POSITIONAL_DIMENSION_LIMIT_INPUT, ResourceDimension::RetainedBytes, "creo dimension auxiliary body", |ctx| {
            parse_positional_dimension(ctx, POSITIONAL_DIMENSION_LIMIT_INPUT, 0,
                POSITIONAL_DIMENSION_LIMIT_INPUT.len(), &scalar::ScalarCache::default())
        }),
CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension auxiliary body"));
}

#[test]
fn named_section_dimension_id_refuses_before_vec_growth() {
    let run = |limit| {
        with_trim_limits(limit, u64::MAX, |ctx| {
            parse_section_3d(
                ctx,
                NAMED_SECTION_LIMIT_INPUT,
                0,
                NAMED_SECTION_LIMIT_INPUT.len(),
            )
        })
    };
    assert!(matches!(run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo section dimension IDs"), run)),
Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo section dimension IDs"));
    assert_eq!(
        run(u64::MAX)
            .expect("section admitted")
            .expect("section present")
            .dimension_ids,
        [42]
    );
}

#[test]
fn positional_dimension_table_uses_the_inherited_table_class() {
    let mut payload = b"prefix\xf8\x02\xf7\x58\xfb\xe2\xf7\x59".to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
    payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
    payload.extend_from_slice(&[10, 0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f, 0, 0x18, 44]);
    let cache = scalar::ScalarCache::from_section(&payload);

    let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
        .expect("positional dimtab");

    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 2);
    assert_eq!(dimensions.rows[0].value.resolved(), Some(3.0));
    assert_eq!(
        dimensions.rows[0].value_body,
        [0x46, 0x08, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(dimensions.rows[0].auxiliary_body, [0x18]);
    assert_eq!(dimensions.rows[0].external_id, 43);
    assert_eq!(dimensions.rows[1].dimension_type, 10);
    assert_eq!(
        dimensions.rows[1].value_body,
        [0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f]
    );
    assert_eq!(dimensions.rows[1].external_id, 44);
}

#[test]
fn named_dimension_retains_nested_dimension_references() {
    let payload = b"dimtab_ptr\0\xf3\xf8\x01\xf7\x58\xfb\xe2\
            \xe0\x01type\0\x02\xe0\x02value\0\x18\xe0\x01direct\0\x00\
            \xe0\x02aux_value\0\x18\xe0\x01ext_id\0\x02\
            dim_ref\0\xf1\xf8\x02\xf7\x60\xfb\xe2\
            \xe0\x01item_id\0\x0d\xe0\x01sense\0\x00\
            \xe0\x01point\0\xf8\x02\x03\xe4\
            \xf1\xf7\x60\xe2\x02\x02\x14\xe4\xf3\xf7\x58\xe2";
    let cache = scalar::ScalarCache::from_section(payload);

    let dimensions =
        dimension_table(payload, 0, payload.len(), &cache).expect("named dimension table");
    let references = dimensions.rows[0]
        .references
        .as_ref()
        .expect("nested dimension references");

    assert_eq!(references.declared_count, 2);
    assert_eq!(references.entity_ref, Some(0x60));
    assert_eq!(references.rows.len(), 2);
    assert_eq!(
        references.rows[0],
        FeatureDimensionReference {
            item_id: Some(13),
            sense: Some(0),
            point: [Some(3), Some(1)],
            offset: payload
                .windows(b"item_id\0".len())
                .position(|window| window == b"item_id\0")
                .expect("item_id offset"),
        }
    );
    assert_eq!(references.rows[1].item_id, Some(2));
    assert_eq!(references.rows[1].sense, Some(2));
    assert_eq!(references.rows[1].point, [Some(20), Some(1)]);
}

#[test]
fn positional_dimension_table_is_self_describing_when_multiple_rows_close() {
    let mut payload = b"prefix\xf8\x04\xf7\x58\xfb\xe2\xf7\x59".to_vec();
    for (index, row) in [
        [1, 0xe4, 0, 0x18, 2],
        [2, 0x0e, 0, 0x18, 0],
        [2, 0xe4, 0, 0x18, 3],
        [2, 0xe4, 0, 0x18, 1],
    ]
    .into_iter()
    .enumerate()
    {
        payload.extend_from_slice(&row);
        if index < 3 {
            payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
        }
    }
    let cache = scalar::ScalarCache::from_section(&payload);

    let dimensions = self_described_positional_dimension_table(&payload, 0, payload.len(), &cache)
        .expect("self-described dimension table");

    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 4);
    assert_eq!(dimensions.rows[0].external_id, 2);
    assert_eq!(dimensions.rows[1].value.resolved(), Some(-0.5));
}

#[test]
fn one_row_positional_table_does_not_self_identify_as_dimensions() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\x01\xe4\x00\x18\x02";
    assert_eq!(
        self_described_positional_dimension_table(
            payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
        ),
        None
    );
}

#[test]
fn positional_dimension_table_retains_bounded_opaque_values() {
    let mut payload = b"prefix\xf8\x03\xf7\x58\xfb\xe2\xf7\x59".to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
    payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
    payload.extend_from_slice(&[1, 0x00, 0x04, 0xa6, 0, 0x18, 44]);
    payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
    payload.extend_from_slice(&[5, 0x0d, 0, 0x18, 45]);
    let cache = scalar::ScalarCache::from_section(&payload);

    let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
        .expect("positional dimtab");

    assert_eq!(dimensions.rows.len(), 3);
    assert_eq!(dimensions.rows[1].value.resolved(), None);
    assert_eq!(
        dimensions.rows[1].value.unresolved_token(),
        Some(&[0x00, 0x04, 0xa6][..])
    );
    assert_eq!(dimensions.rows[1].value_body, [0x00, 0x04, 0xa6]);
    assert_eq!(dimensions.rows[1].auxiliary_body, [0x18]);
    assert_eq!(dimensions.rows[1].external_id, 44);
    assert_eq!(dimensions.rows[2].value.resolved(), Some(-1.0));
    assert_eq!(dimensions.rows[2].external_id, 45);
}

#[test]
fn positional_dimensions_decode_the_positive_dict_lattice_and_bounded_opaque_forms() {
    let positive = [1, 0x53, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f, 0, 0x18, 46];
    let opaque_three = [1, 0x00, 0x04, 0xa6, 0, 0x18, 47];
    let opaque_four = [1, 0x01, 0x04, 0xfe, 0xf2, 0, 0x18, 48];
    let zero = [2, 0x18, 0, 0x18, 49];
    let negative_half = [1, 0x0e, 0, 0x18, 50];
    let cache = scalar::ScalarCache::default();

    let positive_row = positional_dimension(&positive, 0, positive.len(), &cache)
        .expect("positive dictionary dimension");
    assert_eq!(
        positive_row.value.resolved(),
        Some(f64::from_be_bytes([
            0x3f, 0xc8, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f,
        ]))
    );
    assert_eq!(positive_row.direction_byte, 0);
    assert_eq!(positive_row.auxiliary_value, Some(0.0));
    assert_eq!(positive_row.value_body, positive[1..8]);
    assert_eq!(positive_row.auxiliary_body, [0x18]);
    assert_eq!(positive_row.external_id, 46);
    for (body, external_id, token) in [
        (&opaque_three[..], 47, &[0x00, 0x04, 0xa6][..]),
        (&opaque_four[..], 48, &[0x01, 0x04, 0xfe, 0xf2][..]),
    ] {
        let row =
            positional_dimension(body, 0, body.len(), &cache).expect("bounded opaque dimension");
        assert_eq!(row.value.resolved(), None);
        assert_eq!(row.value.unresolved_token(), Some(token));
        assert_eq!(row.external_id, external_id);
    }
    let zero_row = positional_dimension(&zero, 0, zero.len(), &cache).expect("zero dimension");
    assert_eq!(zero_row.value.resolved(), Some(0.0));
    assert_eq!(zero_row.external_id, 49);
    let negative_half_row = positional_dimension(&negative_half, 0, negative_half.len(), &cache)
        .expect("negative half dimension");
    assert_eq!(negative_half_row.value.resolved(), Some(-0.5));
    assert_eq!(negative_half_row.external_id, 50);
}

#[test]
fn positional_dimension_seven_byte_positive_value_preserves_field_alignment() {
    let body = [2, 0x31, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0, 0x18, 27];
    let row = positional_dimension(&body, 0, body.len(), &scalar::ScalarCache::default())
        .expect("seven-byte positive dimension");

    assert_eq!(
        row.value.resolved(),
        Some(f64::from_be_bytes([
            0x40, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0,
        ]))
    );
    assert_eq!(row.direction_byte, 0);
    assert_eq!(row.auxiliary_value, Some(0.0));
    assert_eq!(row.external_id, 27);
}

#[test]
fn dimension_tables_retain_extents_without_decoded_rows() {
    let named = b"dimtab_ptr\0\xf8\x02\xf7\x58\xfb\xe2";
    let cache = scalar::ScalarCache::from_section(named);
    let dimensions = dimension_table(named, 0, named.len(), &cache).expect("named dimtab header");
    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(88));
    assert!(dimensions.rows.is_empty());

    let positional = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59";
    let cache = scalar::ScalarCache::from_section(positional);
    let dimensions = positional_dimension_table(positional, 0, positional.len(), 88, &cache)
        .expect("positional dimtab header");
    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(88));
    assert!(dimensions.rows.is_empty());
}

#[test]
fn positional_definition_inherits_the_labeled_dimension_table_class() {
    let mut payload = b"feat_defs_917\0dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe0\x01feat_id\0\x2a\xe0\x00ref_model_info\0\xe3S2D0004\0\
            \xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
        .to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

    let decoded = crate::decode::with_test_decode_ctx(|ctx| definitions(ctx, &payload))
        .expect("definitions admitted");
    let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

    assert_eq!(decoded[1].identity.owner_feature_id(), Some(42));
    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 1);
    assert_eq!(dimensions.rows[0].value.resolved(), Some(3.0));
    assert_eq!(dimensions.rows[0].external_id, 43);
}

