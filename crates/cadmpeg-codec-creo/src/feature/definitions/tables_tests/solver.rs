// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn relation_operands_refuse_before_retained_copy() {
    assert!(
        matches!(positional_relation_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo relation operands"), |cap| positional_relation_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo relation operands")
    );
    let table = positional_relation_with_limits(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| positional_relation_with_limits(u64::MAX, cap),
        ),
    )
    .expect("relation admitted")
    .expect("table present");
    assert_eq!(table.rows[0].operands.len(), 12);
}

#[test]
fn relation_row_body_refuses_before_retained_copy() {
    assert!(
        matches!(positional_relation_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo relation row body"), |cap| positional_relation_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo relation row body")
    );
    let table = positional_relation_with_limits(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| positional_relation_with_limits(u64::MAX, cap),
        ),
    )
    .expect("relation admitted")
    .expect("table present");
    assert_eq!(table.rows[0].body.len(), 17);
}

#[test]
fn relation_row_refuses_before_vec_growth() {
    assert!(
        matches!(positional_relation_with_limits(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo relation rows"), |cap| positional_relation_with_limits(cap, u64::MAX)), u64::MAX),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo relation rows")
    );
    assert_eq!(
        positional_relation_with_limits(
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| positional_relation_with_limits(u64::MAX, cap)
            )
        )
        .expect("relation admitted")
        .expect("table present")
        .rows
        .len(),
        1
    );
}

#[test]
fn depdb_gsec2d_definition_anchors_positional_table_replay() {
    let mut payload = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0\
            dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe3S2D0003\0\xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
        .to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

    let decoded = crate::decode::with_test_decode_ctx(|ctx| depdb_definitions(ctx, &payload))
        .expect("definitions admitted");
    let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].identity.id(), 2);
    assert_eq!(decoded[1].identity.id(), 2);
    assert!(decoded
        .iter()
        .all(|definition| definition.identity.owner_feature_id().is_none()));
    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 1);
    assert_eq!(dimensions.rows[0].value.resolved(), Some(3.0));
    assert_eq!(dimensions.rows[0].external_id, 43);
}

#[test]
fn equation_table_replays_direct_and_counted_rows() {
    let payload = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\
            \xe0\x05fcn_id\0\x02\
            \xe0\x08arg_arr\0\xf8\x02\x2f\x08\
            \xe0\x01aux_data\0\xf6\
            \xf1\xf7\x80\x9f\xe2\
            \x01\x04\x11\x12\xf6\xe2\
            \x02\x05\xf8\x04\x13\xe4\xe5\xf6\xe2\
            \x03\x06\xf8\x02\xf6\x14\xf6\xe2\
            \xe0\x02scale\0\x99\x88"
        .to_vec();

    let table = equation_table(&payload, 0, payload.len()).expect("eqtn_arr table");

    assert_eq!(table.declared_count, 4);
    assert_eq!(table.entity_ref, Some(159));
    assert_eq!(table.offset, 0);
    assert_eq!(table.rows.len(), 3);
    assert!(table.prototype_body.starts_with(b"\xe0\x01id\0"));
    assert!(table.prototype_body.ends_with(b"\xf1\xf7\x80\x9f\xe2"));

    assert_eq!(table.rows[0].equation_id, 1);
    assert_eq!(table.rows[0].function_id, 4);
    assert_eq!(table.rows[0].explicit_argument_count, None);
    assert_eq!(table.rows[0].arguments, [Some(17), Some(18)]);
    assert_eq!(table.rows[0].arguments_body, [0x11, 0x12]);
    assert_eq!(table.rows[0].auxiliary_body, [0xf6]);
    assert_eq!(table.rows[0].body, [1, 4, 0x11, 0x12, 0xf6, 0xe2]);

    assert_eq!(table.rows[1].equation_id, 2);
    assert_eq!(table.rows[1].function_id, 5);
    assert_eq!(table.rows[1].explicit_argument_count, Some(4));
    assert_eq!(
        table.rows[1].arguments,
        [Some(19), Some(1), Some(0), Some(0)]
    );
    assert_eq!(table.rows[1].arguments_body, [0x13, 0xe4, 0xe5]);
    assert_eq!(table.rows[1].auxiliary_body, [0xf6]);
    assert!(table.rows[1].body.ends_with(&[0xf6, 0xe2]));

    assert_eq!(table.rows[2].equation_id, 3);
    assert_eq!(table.rows[2].function_id, 6);
    assert_eq!(table.rows[2].explicit_argument_count, Some(2));
    assert_eq!(table.rows[2].arguments, [None, Some(20)]);
    assert_eq!(table.rows[2].arguments_body, [0xf6, 0x14]);
    assert_eq!(table.rows[2].auxiliary_body, [0xf6]);
}

#[test]
fn equation_table_accepts_final_row_at_table_separator() {
    let payload = b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\
            \xe0\x05fcn_id\0\x02\
            \xe0\x08arg_arr\0\xf8\x02\x11\x12\
            \xe0\x01aux_data\0\xf6\
            \xf1\xf7\x80\x9f\xe2\
            \x01\x04\x11\x12\xf6\xf2\xf7\x39\x99\x88\
            \xe0\x02scale\0\x99\x88";

    let table = equation_table(payload, 0, payload.len()).expect("eqtn_arr table");

    assert_eq!(table.declared_count, 2);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].equation_id, 1);
    assert_eq!(table.rows[0].body, [1, 4, 0x11, 0x12, 0xf6]);
}

#[test]
fn equation_prototype_body_refuses_before_retained_copy() {
    assert!(
        matches!(equation_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo equation prototype body"), |cap| equation_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo equation prototype body")
    );
}

#[test]
fn equation_arguments_refuse_before_vec_growth() {
    assert!(
        matches!(equation_with_limits(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo equation arguments"), |cap| equation_with_limits(cap, u64::MAX)), u64::MAX),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo equation arguments")
    );
}

#[test]
fn equation_argument_body_refuses_before_retained_copy() {
    assert!(
        matches!(equation_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo equation argument body"), |cap| equation_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo equation argument body")
    );
}

#[test]
fn equation_auxiliary_body_refuses_before_retained_copy() {
    assert!(
        matches!(equation_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo equation auxiliary body"), |cap| equation_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo equation auxiliary body")
    );
}

#[test]
fn equation_row_body_refuses_before_retained_copy() {
    assert!(
        matches!(equation_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo equation row body"), |cap| equation_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo equation row body")
    );
}

#[test]
fn equation_row_refuses_before_vec_growth() {
    assert!(
        matches!(equation_with_limits(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo equation rows"), |cap| equation_with_limits(cap, u64::MAX)), u64::MAX),
Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo equation rows")
    );
    let table = equation_with_limits(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| equation_with_limits(u64::MAX, cap),
        ),
    )
    .expect("equation admitted")
    .expect("table present");
    assert_eq!(table.rows[0].arguments, [Some(17), Some(18)]);
}

#[test]
fn positional_relation_table_replays_rows_after_its_prototype() {
    let payload = b"prefix\xf8\x03\xf7\x64\xfb\xe2\xf7\x65\
            prototype\xf1\xf7\x64\xe2\
            \x08\x00\x03\x0f\xf6\xe4\x01\xe4\x00\xe4\x0f\x10\x0f\x18\x00\xf6\x00\xe2";

    let relations =
        positional_relation_table(payload, 0, payload.len(), 100).expect("positional relat_ptr");

    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(100));
    assert_eq!(relations.rows.len(), 1);
    assert_eq!(relations.rows[0].relation_id, 8);
    assert_eq!(relations.rows[0].used, 0);
    assert_eq!(relations.rows[0].sign, 0);
    assert_eq!(relations.rows[0].dimension_id, 246);
    assert_eq!(relations.rows[0].relation_type, 0);
    assert!(relations.rows[0].operand_vectors.is_some());
}

#[test]
fn relation_table_retains_solver_children_after_an_invalid_row() {
    let payload = b"relat_ptr\0\xf4\x04\xf8\x03\xf7\x6a\xfb\xe2\
            schema\xf1\xf7\x6a\xe2invalid\
            skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2";

    let relations = relation_table(payload, 0, payload.len()).expect("relat_ptr header");

    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(106));
    assert!(relations.rows.is_empty());
    assert_eq!(relations.skamps().len(), 1);
    assert_eq!(relations.skamps()[0].id, 5);
}

#[test]
fn relation_tables_retain_extents_without_their_prototypes() {
    let named = b"relat_ptr\0\xf8\x03\xf7\x64\xfb\xe2";
    let relations = relation_table(named, 0, named.len()).expect("named relat_ptr header");
    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(100));
    assert!(relations.rows.is_empty());

    let positional = b"\xf8\x03\xf7\x64\xfb\xe2";

    let relations = positional_relation_table(positional, 0, positional.len(), 100)
        .expect("positional relat_ptr header");

    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(100));
    assert!(relations.rows.is_empty());
}

#[test]
fn positional_skamp_table_replays_counted_nested_items() {
    let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2\
            \x02\x01\xea\x22\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 2);
    assert_eq!(skamps[0].id, 1);
    assert_eq!(skamps[0].kind, 0);
    assert_eq!(skamps[0].items.len(), 2);
    assert_eq!(skamps[0].items[0].entity_id, 6);
    assert_eq!(skamps[0].items[1].sense, 2);
    assert_eq!(skamps[1].kind, 1);
    assert_eq!(skamps[1].flags, 34);
    assert_eq!(skamps[1].status, 35);
    assert_eq!(skamps[1].items[0].entity_id, 8);
}

#[test]
fn positional_skamp_table_replays_consecutive_single_item_rows() {
    let payload = b"\xf8\x03\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\xe2\
            \x02\x01\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x07\x00\xe2\
            \x03\x02\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 3);
    assert_eq!(
        skamps
            .iter()
            .map(|skamp| (skamp.id, skamp.kind, skamp.items[0].entity_id))
            .collect::<Vec<_>>(),
        [(1, 0, 6), (2, 1, 7), (3, 2, 8)]
    );
}

#[test]
fn positional_skamp_table_accepts_a_following_table_wrapper_boundary() {
    let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\xe2\
            \x02\x01\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x07\x00\xf1\xf7\x60\xe2\x08\x02\
            \xf4\x04\xf7\x64\xf8\x01\xf7\x64\xfb\xe2";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 2);
    assert_eq!(skamps[1].items.len(), 2);
    assert_eq!(
        skamps[1]
            .items
            .iter()
            .map(|item| (item.entity_id, item.sense))
            .collect::<Vec<_>>(),
        [(7, 0), (8, 2)]
    );
}

#[test]
fn positional_skamp_table_accepts_a_following_table_header_boundary() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\
            \xf8\x02\xf7\x64\xfb\xe2\xf7\x65";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 1);
    assert_eq!(skamps[0].items[0].entity_id, 6);
}

#[test]
fn positional_skamp_table_accepts_a_following_wrapper_body_boundary() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\
            \xf4\x04\xf7\x64\xe1\xe1\xe3";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 1);
    assert_eq!(skamps[0].items[0].entity_id, 6);
}

#[test]
fn positional_skamp_table_skips_row_auxiliary_frames() {
    let payload = b"\xf8\x03\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2\
            \x02\x04\x00\x22\xe0\x02aux\0\xf8\x02\x0a\x0b\xf7\x60\
            \xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00\xf3\xf7\x58\xe2\
            \x03\x02\x00\x23\xf7\x60\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x09\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 3);
    assert_eq!(skamps[1].id, 2);
    assert_eq!(skamps[1].kind, 4);
    assert_eq!(skamps[1].items[0].entity_id, 8);
    assert_eq!(skamps[2].id, 3);
    assert_eq!(skamps[2].kind, 2);
    assert_eq!(skamps[2].items[0].entity_id, 9);
}

#[test]
fn positional_skamp_table_rejects_ambiguous_nested_item_arrays() {
    let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\
            \xf3\xf7\x58\xe2\x02\x04\x00\x22\
            \xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x07\x00\
            \xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 1);
    assert_eq!(skamps[0].id, 1);
}

#[test]
fn positional_skamp_table_rejects_multiple_matching_nested_item_arrays() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\
            \xf8\x01\xf7\x62\xfb\xe2\xf7\x63\x07\x00\
            \xf3\xf7\x58\xe2";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert!(skamps.is_empty());
}

#[test]
fn positional_solver_tables_retain_complete_prefix_rows() {
    let skamps = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2";
    let rows = positional_feature_skamps(skamps, 0, skamps.len(), 88);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 1);

    let triples = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2";
    let rows = positional_relation_triples(triples, 0, triples.len(), 100);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relation_id, Some(1));
}

#[test]
fn positional_skamp_items_refuse_before_vec_growth() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::CollectionItems,
        "creo skamp items",
        |ctx| parse_positional_feature_skamps(ctx, payload, 0, payload.len(), 88),
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    policy.limits.max_collection_items = refusal.limit;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_positional_feature_skamps(&ctx, payload, 0, payload.len(), 88),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo skamp items")
    );
}

#[test]
fn positional_skamp_rows_refuse_before_vec_growth() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::CollectionItems,
        "creo skamp rows",
        |ctx| parse_positional_feature_skamps(ctx, payload, 0, payload.len(), 88),
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    policy.limits.max_collection_items = refusal.limit;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_positional_feature_skamps(&ctx, payload, 0, payload.len(), 88),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo skamp rows")
    );
    assert_eq!(
        positional_feature_skamps(payload, 0, payload.len(), 88).len(),
        1
    );
}

#[test]
fn named_skamp_prototype_items_refuse_before_vec_growth() {
    let payload = b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::CollectionItems,
        "creo skamp prototype items",
        |ctx| parse_feature_skamps(ctx, payload, 0, payload.len()),
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    policy.limits.max_collection_items = refusal.limit;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_feature_skamps(&ctx, payload, 0, payload.len()),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo skamp prototype items")
    );
}

#[test]
fn named_skamp_rows_refuse_before_vec_growth() {
    let payload = b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::CollectionItems,
        "creo skamp rows",
        |ctx| parse_feature_skamps(ctx, payload, 0, payload.len()),
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    policy.limits.max_collection_items = refusal.limit;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_feature_skamps(&ctx, payload, 0, payload.len()),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo skamp rows")
    );
}

#[test]
fn named_relation_triples_refuse_before_vec_growth() {
    let payload = b"triples_ptr\0\xf4\x04\xf8\x01\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::CollectionItems,
        "creo relation triples",
        |ctx| parse_feature_relation_triples(ctx, payload, 0, payload.len()),
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    policy.limits.max_collection_items = refusal.limit;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_feature_relation_triples(&ctx, payload, 0, payload.len()),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo relation triples")
    );
}

#[test]
fn positional_relation_triples_refuse_before_vec_growth() {
    let payload = b"\xf8\x01\xf7\x64\xfb\xe2\xf7\x65\x01\xf6\x04";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::CollectionItems,
        "creo relation triples",
        |ctx| parse_positional_relation_triples(ctx, payload, 0, payload.len(), 100),
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    policy.limits.max_collection_items = refusal.limit;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_positional_relation_triples(&ctx, payload, 0, payload.len(), 100),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo relation triples")
    );
}

#[test]
fn solver_header_does_not_adopt_a_later_array() {
    let payload = b"skamp_ptr\0opaque\xf8\x02\xf7\x58\xfb\xe2";

    assert!(
        crate::decode::with_test_decode_ctx(|ctx| named_solver_table_header(
            ctx,
            payload,
            b"skamp_ptr\0",
            0,
            payload.len()
        ))
        .expect("solver search admitted")
        .is_none()
    );
}

#[test]
fn optional_solver_header_propagates_the_search_refusal() {
    let payload = b"skamp_ptr\0opaque\xf8\x02\xf7\x58\xfb\xe2";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let error = crate::test_support::last_refusal_at(
        payload,
        ResourceDimension::WorkUnits,
        "find Creo solver table",
        |ctx| named_solver_table_header(ctx, payload, b"skamp_ptr\0", 0, payload.len()),
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    policy.limits.max_work_units = refusal.limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("root admitted");
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        named_solver_table_header(&ctx, payload, b"skamp_ptr\0", 0, payload.len())
            .expect_err("search must refuse")
    else {
        panic!("resource refusal");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.operation, "find Creo solver table");
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn named_solver_tables_retain_complete_prefix_rows() {
    let skamps = b"skamp_ptr\0\xf3\xf8\x02\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2invalid";
    let rows = feature_skamps(skamps, 0, skamps.len());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 5);

    let triples = b"triples_ptr\0\xf4\x04\xf8\x02\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2\x01\x02\x03";
    let rows = feature_relation_triples(triples, 0, triples.len());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relation_id, Some(7));
}

#[test]
fn named_solver_tables_accept_direct_prototype_item_schema_close() {
    let skamps = b"skamp_ptr\0\xf1\xf8\x02\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\
            \xf3\xf7\x6b\xe2\
            \x07\x02\x03\x23\xf8\x01\xf7\x6c\xfb\xe2\
            \xf7\x6d\x2a\x01\xe2";

    let rows = feature_skamps(skamps, 0, skamps.len());

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, 5);
    assert_eq!(
        rows[0].items,
        vec![FeatureSkampItem {
            entity_id: 42,
            sense: 1
        }]
    );
    assert_eq!(rows[1].id, 7);
    assert_eq!(rows[1].kind, 2);
    assert_eq!(rows[1].flags, 3);
    assert_eq!(rows[1].status, 35);
    assert_eq!(
        rows[1].items,
        vec![FeatureSkampItem {
            entity_id: 42,
            sense: 1
        }]
    );
}

#[test]
fn positional_definition_preserves_its_named_solver_tables() {
    let solver_tables = b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2\
            triples_ptr\0\xf4\x04\xf8\x01\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2";
    let mut payload = b"relat_ptr\0\xf4\x04\xf8\x02\xf7\x6a\xfb\xe2schema\xf1\xf7\x6a\xe2".to_vec();
    payload.extend_from_slice(solver_tables);
    let positional_start = payload.len();
    payload.extend_from_slice(solver_tables);
    payload.extend_from_slice(b"\xf8\x02\xf7\x6a\xfb\xe2");
    let prototype_offset = payload.len() + 3;
    assert!((128..=16_383).contains(&prototype_offset));
    payload.extend_from_slice(&[
        psb::token::ENTITY_REF,
        0x80 + u8::try_from(prototype_offset >> 8).expect("prototype offset high byte"),
        u8::try_from(prototype_offset & 0xff).expect("prototype offset low byte"),
    ]);
    payload.extend_from_slice(b"\xf1\xf7\x6a\xe2");

    let definitions = crate::decode::with_test_decode_ctx(|ctx| {
        definitions_in_ranges(
            ctx,
            &payload,
            &[
                crate::feature::definitions::DefinitionStart {
                    offset: 0,
                    id: std::num::NonZeroU32::new(1),
                    owner_override: None,
                    positional: false,
                },
                crate::feature::definitions::DefinitionStart {
                    offset: positional_start,
                    id: std::num::NonZeroU32::new(2),
                    owner_override: None,
                    positional: true,
                },
            ],
            None,
        )
    })
    .expect("definitions admitted");
    let relations = definitions[1].relations.as_ref().expect("relations");

    assert_eq!(relations.skamps().len(), 1);
    assert_eq!(relations.skamps()[0].id, 5);
    assert_eq!(
        relations
            .skamps
            .as_ref()
            .expect("skamp table")
            .header()
            .expect("skamp header")
            .declared_count,
        1
    );
    assert_eq!(relations.triples().len(), 1);
    assert_eq!(relations.triples()[0].relation_id, Some(7));
    assert_eq!(
        relations
            .triples
            .as_ref()
            .expect("triples table")
            .header()
            .expect("triples header")
            .declared_count,
        1
    );
}

#[test]
fn positional_triples_replay_nullable_relation_joins() {
    let payload = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2\x02\xf6\x05";

    let triples = positional_relation_triples(payload, 0, payload.len(), 100);

    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].relation_id, Some(1));
    assert_eq!(triples[0].equation_id, None);
    assert_eq!(triples[0].skamp_id, Some(4));
    assert_eq!(triples[1].relation_id, Some(2));
    assert_eq!(triples[1].skamp_id, Some(5));
}
