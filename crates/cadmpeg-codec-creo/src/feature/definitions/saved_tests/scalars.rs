// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn saved_dummy_body_refuses_before_retained_copy() {
    assert!(
        matches!(crate::test_support::last_refusal_at(SAVED_DUMMY_LIMIT_INPUT, ResourceDimension::RetainedBytes, "creo saved dummy body", |ctx| {
        parse_saved_dummy_entities(ctx, SAVED_DUMMY_LIMIT_INPUT, 0,
            SAVED_DUMMY_LIMIT_INPUT.len())
    }),
CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved dummy body")
    );
    assert_eq!(
        with_saved_leaf_limits(SAVED_DUMMY_LIMIT_INPUT, u64::MAX, u64::MAX, |ctx| {
            parse_saved_dummy_entities(
                ctx,
                SAVED_DUMMY_LIMIT_INPUT,
                0,
                SAVED_DUMMY_LIMIT_INPUT.len(),
            )
        })
        .expect("dummy admitted")
        .len(),
        1
    );
}

#[test]
fn saved_dummy_entity_refuses_before_append() {
    assert!(
        matches!(crate::test_support::last_refusal_at(SAVED_DUMMY_LIMIT_INPUT, ResourceDimension::CollectionItems, "creo saved dummy entities", |ctx| {
        parse_saved_dummy_entities(ctx, SAVED_DUMMY_LIMIT_INPUT, 0,
            SAVED_DUMMY_LIMIT_INPUT.len())
    }),
CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo saved dummy entities")
    );
    assert_eq!(
        with_saved_leaf_limits(SAVED_DUMMY_LIMIT_INPUT, u64::MAX, u64::MAX, |ctx| {
            parse_saved_dummy_entities(
                ctx,
                SAVED_DUMMY_LIMIT_INPUT,
                0,
                SAVED_DUMMY_LIMIT_INPUT.len(),
            )
        })
        .expect("dummy admitted")
        .len(),
        1
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
        section.extend_from_slice(&[
            0x46,
            0x08,
            u8::try_from(index >> 8).expect("fixture value fits u8"),
            u8::try_from(index).expect("fixture value fits u8"),
            0,
            0,
            0,
            0,
        ]);
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
fn saved_section_assembly_uses_one_entity_output_buffer() {
    let mut payload = b"\xe0\x00p_saved_result\0".to_vec();
    payload.extend_from_slice(SAVED_SPLINE_LIMIT_INPUT);
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::RetainedBytes,
    ] {
        let leaf = |limit| {
            let (collection, retained) = match dimension {
                ResourceDimension::CollectionItems => (limit, u64::MAX),
                _ => (u64::MAX, limit),
            };
            with_saved_leaf_limits(&payload, collection, retained, |ctx| {
                parse_saved_spline_entities(
                    ctx,
                    &payload,
                    0,
                    payload.len(),
                    &scalar::ScalarCache::default(),
                )
            })
        };
        let limit = crate::test_support::allocation_limit_at(dimension, None, leaf);
        let expected = leaf(limit).expect("leaf output admitted");
        let (collection, retained) = match dimension {
            ResourceDimension::CollectionItems => (limit, u64::MAX),
            _ => (u64::MAX, limit),
        };
        let section = with_saved_leaf_limits(&payload, collection, retained, |ctx| {
            parse_saved_section(
                ctx,
                &payload,
                0,
                payload.len(),
                &scalar::ScalarCache::default(),
                None,
                None,
            )
        })
        .expect("assembly has the leaf storage bound")
        .expect("saved section");
        assert_eq!(section.entities, expected);
        assert_eq!(section.entities.len(), 1);
    }
}
