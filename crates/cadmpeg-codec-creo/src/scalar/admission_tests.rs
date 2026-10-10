// SPDX-License-Identifier: Apache-2.0

use super::{double_xar_tables, ScalarCache};

#[test]
fn scalar_cache_discovery_and_duplicate_hashing_refuse_work() {
    let images = [0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x46, 0x08, 0, 0, 0, 0, 0, 0];
    let cache = crate::test_support::assert_work_boundaries(
        &[
            "creo scalar cache discovery",
            "creo scalar cache unique images",
        ],
        |ctx| ScalarCache::from_section_checked(ctx, &images).map(|cache| cache.entries.len()),
    );
    assert_eq!(cache, 1, "equal images remain deduplicated");
    let error = crate::test_support::last_refusal_at(
        &[0; 16],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo scalar cache discovery",
        |ctx| ScalarCache::from_section_checked(ctx, &[0; 16]).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "creo scalar cache discovery")
    );
}

#[test]
fn duplicate_scalar_image_hashing_refuses_after_the_first_image() {
    let images = [0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x46, 0x08, 0, 0, 0, 0, 0, 0];
    // The last image-set refusal is the duplicate's lookup: it hashes and
    // compares its eight bytes once and adds no table growth.
    let error = crate::test_support::last_refusal_at(
        &images,
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo scalar cache unique images",
        |ctx| ScalarCache::from_section_checked(ctx, &images).map(|_| ()),
    );
    let cadmpeg_core::CodecError::ResourceLimit(resource) = error else {
        panic!("hash work refusal");
    };
    assert_eq!(resource.operation, "creo scalar cache unique images");
    assert_eq!(resource.additional, 8);
}

#[test]
fn double_xar_discovery_and_slot_parsing_refuse_work() {
    let bytes = b"double_xar\0\xf8\x02\x10\xe0";
    let tables = crate::test_support::assert_work_boundaries(
        &["creo double_xar discovery", "creo double_xar slot parsing"],
        |ctx| double_xar_tables(ctx, bytes),
    );
    assert_eq!(tables.len(), 1);
    let error = crate::test_support::last_refusal_at(
        &[0; 16],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo double_xar discovery",
        |ctx| double_xar_tables(ctx, &[0; 16]),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "creo double_xar discovery")
    );
}

fn checked_cache_with_collection_limit(
    limit: u64,
) -> Result<(usize, Option<u8>), cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = [0x46, 0x08, 1, 2, 3, 4, 5, 6];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("the scalar image fits the root limit");
    ScalarCache::from_section_checked(&ctx, &bytes).map(|cache| {
        (
            cache.entries.len(),
            cache.paired_byte_1(&[1, 2, 3, 4, 5, 6]),
        )
    })
}

fn double_xar_with_limits(
    bytes: &[u8],
    items: u64,
    retained: u64,
) -> Result<Vec<super::DoubleXarTable>, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("the dictionary fixture fits the root limit");
    double_xar_tables(&ctx, bytes)
}

#[test]
fn double_xar_slots_refuse_before_counted_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = b"double_xar\0\xf8\x02\x10\xe0";
    assert_eq!(
        double_xar_with_limits(
            bytes,
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                None,
                |cap| double_xar_with_limits(bytes, cap, u64::MAX)
            ),
            u64::MAX
        )
        .expect("table admitted")
        .len(),
        1
    );
    let error = double_xar_with_limits(
        bytes,
        crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo double_xar slots"),
            |cap| double_xar_with_limits(bytes, cap, u64::MAX),
        ),
        u64::MAX,
    )
    .expect_err("second slot needs admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo double_xar slots")
    );
}

#[test]
fn double_xar_table_refuses_before_result_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = b"double_xar\0\xf8\x02\x10\xe0";
    let error = double_xar_with_limits(
        bytes,
        crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo double_xar tables"),
            |cap| double_xar_with_limits(bytes, cap, u64::MAX),
        ),
        u64::MAX,
    )
    .expect_err("table needs admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo double_xar tables")
    );
}

#[test]
fn double_xar_literal_refuses_before_retained_copy() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = b"double_xar\0\xf8\x02\x46\x08\x00\x00\x00\x00\x00\x00\xe0";
    assert_eq!(
        double_xar_with_limits(
            bytes,
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| double_xar_with_limits(bytes, u64::MAX, cap)
            )
        )
        .expect("literal admitted")
        .len(),
        1
    );
    let error = double_xar_with_limits(
        bytes,
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo double_xar literal bytes"),
            |cap| double_xar_with_limits(bytes, u64::MAX, cap),
        ),
    )
    .expect_err("literal bytes need admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo double_xar literal bytes")
    );
}

#[test]
fn scalar_cache_unique_image_refuses_before_hash_growth() {
    let cache = checked_cache_with_collection_limit(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        None,
        checked_cache_with_collection_limit,
    ))
    .expect("service-sized collection budget admits one scalar");
    assert_eq!(cache.0, 1);
    assert_eq!(cache.1, Some(0x08));
    let error = checked_cache_with_collection_limit(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo scalar cache unique images"),
        checked_cache_with_collection_limit,
    ))
    .expect_err("the unique image needs one collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo scalar cache unique images"
    ));
}

#[test]
fn scalar_cache_paired_tail_refuses_before_tree_insert() {
    let error = checked_cache_with_collection_limit(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo scalar cache paired tails"),
        checked_cache_with_collection_limit,
    ))
    .expect_err("the paired tail follows the unique image");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo scalar cache paired tails"
    ));
}

#[test]
fn scalar_cache_entry_refuses_before_vector_growth() {
    let error = checked_cache_with_collection_limit(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo scalar cache entries"),
        checked_cache_with_collection_limit,
    ))
    .expect_err("the scalar entry follows the hash and tree nodes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo scalar cache entries"
    ));
}
