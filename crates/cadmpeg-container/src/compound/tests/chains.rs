use super::*;
use cadmpeg_core::decode::ResourceDimension;

fn chain_node_bytes() -> usize {
    11 * std::mem::size_of::<u32>()
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<usize>())
}

fn no_chain_admission_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 0;
    policy
}

fn assert_chain_diagnostic(
    fat: &[u32],
    sector_count: usize,
    start: u32,
    length: Option<ChainLength>,
    materialized: usize,
    retained: usize,
    expected: &str,
) {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        u64::try_from(materialized).expect("chain peak fits u64");
    policy.limits.max_retained_bytes =
        u64::try_from(retained).expect("chain output slots fit u64");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let error = chain(&ctx, fat, sector_count, start, length, ChainRole::Directory)
        .expect_err("structural chain failure");
    assert!(matches!(error, CodecError::Malformed(message) if message == expected));
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn declared_eoc_chain_diagnostic_is_free() {
    assert_chain_diagnostic(
        &[],
        1,
        END_OF_CHAIN,
        Some(ChainLength::Declared(std::num::NonZeroUsize::MIN)),
        0,
        0,
        "CFB directory chain length does not match its declaration",
    );
}

#[test]
fn unbounded_eoc_chain_diagnostic_is_free() {
    assert_chain_diagnostic(
        &[],
        0,
        END_OF_CHAIN,
        Some(ChainLength::Unbounded),
        0,
        0,
        "empty CFB directory",
    );
}

#[test]
fn out_of_range_chain_diagnostic_is_free() {
    assert_chain_diagnostic(
        &[],
        0,
        0,
        Some(ChainLength::Unbounded),
        0,
        0,
        "CFB directory chain is cyclic, overlong, or out of range",
    );
}

#[test]
fn repeated_sector_chain_diagnostic_is_free() {
    assert_chain_diagnostic(
        &[0, FREE_SECTOR],
        2,
        0,
        Some(ChainLength::Unbounded),
        chain_node_bytes(),
        0,
        "CFB directory chain is cyclic, overlong, or out of range",
    );
}

#[test]
fn reserved_sector_chain_diagnostic_is_free() {
    assert_chain_diagnostic(
        &[FREE_SECTOR],
        1,
        0,
        Some(ChainLength::Unbounded),
        chain_node_bytes(),
        0,
        "CFB directory chain enters a reserved sector role",
    );
}

#[test]
fn short_declared_chain_diagnostic_is_free() {
    // The output candidate and one visited B-tree node are the live peak.
    assert_chain_diagnostic(
        &[END_OF_CHAIN, FREE_SECTOR],
        2,
        0,
        Some(ChainLength::Declared(
            std::num::NonZeroUsize::new(2).expect("nonzero chain length"),
        )),
        chain_node_bytes() + std::mem::size_of::<u32>(),
        0,
        "CFB directory chain length does not match its declaration",
    );
}

#[test]
fn impossible_declared_length_is_rejected_before_admission() {
    let arena = DecodeArena::new();
    let policy = no_chain_admission_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let error = chain(
        &ctx,
        &[END_OF_CHAIN],
        1,
        0,
        Some(ChainLength::Declared(
            std::num::NonZeroUsize::new(2).expect("nonzero chain length"),
        )),
        ChainRole::Directory,
    )
    .expect_err("declared chain cannot fit the available sectors");
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "CFB directory chain length exceeds available sectors"));
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn initial_out_of_range_sector_is_rejected_before_admission() {
    let arena = DecodeArena::new();
    let policy = no_chain_admission_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let error = chain(
        &ctx,
        &[END_OF_CHAIN],
        1,
        1,
        Some(ChainLength::Declared(
            std::num::NonZeroUsize::new(1).expect("nonzero chain length"),
        )),
        ChainRole::Directory,
    )
    .expect_err("initial sector is outside the address domain");
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "CFB directory chain is cyclic, overlong, or out of range"));
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn declared_eoc_is_rejected_before_collection_admission() {
    let arena = DecodeArena::new();
    let policy = no_chain_admission_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let error = chain(
        &ctx,
        &[],
        0,
        END_OF_CHAIN,
        Some(ChainLength::Declared(
            std::num::NonZeroUsize::new(1).expect("nonzero chain length"),
        )),
        ChainRole::Directory,
    )
    .expect_err("an EOC start cannot satisfy a nonempty declaration");
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "CFB directory chain length does not match its declaration"));
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn failed_candidates_release_storage_before_a_valid_chain() {
    let slot_bytes = std::mem::size_of::<u32>();
    let materialized_peak = chain_node_bytes() + slot_bytes;
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        u64::try_from(materialized_peak).expect("chain peak fits u64");
    policy.limits.max_retained_bytes = u64::try_from(slot_bytes).expect("slot fits u64");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let short_fat = [END_OF_CHAIN, FREE_SECTOR];
    for _ in 0..2 {
        let error = chain(
            &ctx,
            &short_fat,
            2,
            0,
            Some(ChainLength::Declared(
                std::num::NonZeroUsize::new(2).expect("nonzero chain length"),
            )),
            ChainRole::Directory,
        )
        .expect_err("the chain ends before its declared length");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "CFB directory chain length does not match its declaration"));
        assert!(ctx.resource_refusal().is_none());
    }

    let valid_fat = [1, END_OF_CHAIN];
    let accepted = chain(
        &ctx,
        &valid_fat,
        2,
        0,
        Some(ChainLength::Declared(
            std::num::NonZeroUsize::new(2).expect("nonzero chain length"),
        )),
        ChainRole::Directory,
    )
    .expect("valid chain admission")
    .expect("nonempty chain");
    assert_eq!(accepted.first, 0);
    assert_eq!(accepted.rest.as_slice(), &[1]);

    let error = chain(
        &ctx,
        &valid_fat,
        2,
        0,
        Some(ChainLength::Declared(
            std::num::NonZeroUsize::new(2).expect("nonzero chain length"),
        )),
        ChainRole::Directory,
    )
    .expect_err("the retained output cap is now full");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("retained output cap refuses the next chain");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        (limit.used, limit.additional),
        (
            u64::try_from(slot_bytes).expect("slot fits u64"),
            u64::try_from(slot_bytes).expect("slot fits u64"),
        )
    );
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn nested_storage_receives_only_the_accepted_chain_output() {
    let slot_bytes = std::mem::size_of::<u32>();
    let node_bytes = chain_node_bytes();
    let materialized_peak = node_bytes + slot_bytes;
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        u64::try_from(materialized_peak).expect("chain peak fits u64");
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut caller_storage =
        ctx.reserve_scoped(0, "enclose CFB chain output").expect("caller scope");
    let accepted = caller_storage
        .with_storage(|| {
            chain(
                &ctx,
                &[1, END_OF_CHAIN],
                2,
                0,
                Some(ChainLength::Declared(
                    std::num::NonZeroUsize::new(2).expect("nonzero chain length"),
                )),
                ChainRole::Directory,
            )
        })
        .expect("nested chain admission")
        .expect("nonempty chain");
    assert_eq!(accepted.first, 0);
    assert_eq!(accepted.rest.as_slice(), &[1]);

    let _node_probe = ctx
        .reserve_scoped(
            u64::try_from(node_bytes).expect("node bound fits u64"),
            "probe CFB chain output ownership",
        )
        .expect("the caller scope holds only the returned u32 slot");
    let error = ctx
        .reserve_scoped(1, "probe CFB chain output boundary")
        .expect_err("the output slot and one scratch node fill the materialized cap");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("materialized boundary refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(
        (limit.used, limit.additional),
        (
            u64::try_from(materialized_peak).expect("chain peak fits u64"),
            1,
        )
    );
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

