use super::*;
use crate::compound::SectorChain;

fn zero_work_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy
}

#[test]
fn one_mini_sector_stream_opens_without_work_or_storage_admission() {
    let file = fixture();
    let setup_arena = DecodeArena::new();
    let setup_policy = DecodePolicy::service();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&file, &setup_arena, &setup_policy).expect("setup root");
    let snapshot = CompoundSnapshot::new(&setup_ctx, root).expect("fixture snapshot");
    let stream = snapshot
        .stream(&setup_ctx, "Small")
        .expect("stream lookup")
        .expect("one mini-sector stream");

    let mut open_policy = zero_work_policy();
    open_policy.limits.max_materialized_bytes = 0;
    open_policy.limits.max_retained_bytes = 0;
    open_policy.limits.max_collection_items = 0;
    let open_arena = DecodeArena::new();
    let (open_ctx, _open_root) =
        DecodeContext::from_root_bytes(&file, &open_arena, &open_policy).expect("open root");
    let opened = snapshot
        .open(&open_ctx, stream)
        .expect("a borrowed one-sector view uses no scan or storage budget");
    assert_eq!(opened.window(), b"small");
    assert_eq!(open_ctx.resource_refusal(), None);
}

#[test]
fn admitted_chain_yields_its_fixed_first_sector_at_zero_work() {
    let chain = SectorChain {
        first: 7,
        rest: Vec::new(),
    };
    let policy = zero_work_policy();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut sectors = Vec::new();
    chain
        .visit(&ctx, |sector| {
            sectors.push(sector);
            Ok(())
        })
        .expect("the fixed first field and empty rest need no work");
    let mut sectors = sectors.iter();
    assert_eq!(sectors.next(), Some(&chain.first));
    assert_eq!(sectors.next(), None);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn admitted_chain_still_refuses_for_a_nonempty_rest() {
    let chain = SectorChain {
        first: 7,
        rest: vec![8],
    };
    let policy = zero_work_policy();
    let (error, refusal) = with_context(&[], &policy, |ctx| {
        let mut reached = Vec::new();
        let error = chain
            .visit(ctx, |sector| {
                reached.push(sector);
                Ok(())
            })
            .expect_err("the variable rest must be admitted before its callback");
        assert_eq!(reached, vec![chain.first]);
        (error, ctx.resource_refusal())
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("rest traversal refusal is typed");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "visit CFB chain sectors");
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert_eq!(refusal, Some(limit));
}

#[test]
fn v4_zero_padding_reaches_fixed_header_count_error_without_work() {
    let mut file = fixture_v4();
    put_u32(&mut file, 44, 0);
    file[512] = 0;
    let policy = zero_work_policy();
    let (error, refusal) = with_context(&file, &policy, |ctx| {
        let error = parse_state(ctx, &file).expect_err("zero FAT count is malformed");
        (error, ctx.resource_refusal())
    });
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "invalid CFB header counts or reserved fields"));
    assert_eq!(refusal, None);
}

#[test]
fn v4_nonzero_padding_returns_its_fixed_error_without_work() {
    let mut file = fixture_v4();
    put_u32(&mut file, 44, 0);
    file[512] = 1;
    let policy = zero_work_policy();
    let (error, refusal) = with_context(&file, &policy, |ctx| {
        let error = parse_state(ctx, &file).expect_err("nonzero padding is malformed");
        (error, ctx.resource_refusal())
    });
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "CFB v4 header padding is not zero"));
    assert_eq!(refusal, None);
}

#[test]
fn v4_zero_padding_prefix_reaches_header_count_error_without_work() {
    let mut file = fixture_v4();
    put_u32(&mut file, 44, 0);
    file[512] = 0;
    let policy = zero_work_policy();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
        .expect("fixture root fits the input limit");
    let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
        .expect("fixed padding and count checks use no work");
    assert_eq!(
        probe,
        CompoundPrefixProbe::Malformed("invalid CFB header counts".into())
    );
    drop(storage);
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(
        super::probe(&file),
        CompoundPrefixProbe::Malformed("invalid CFB header counts".into())
    );
}

#[test]
fn v4_nonzero_padding_prefix_returns_its_fixed_error_without_work() {
    let mut file = fixture_v4();
    put_u32(&mut file, 44, 0);
    file[512] = 1;
    let policy = zero_work_policy();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
        .expect("fixture root fits the input limit");
    let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
        .expect("fixed padding diagnostics use no work");
    assert_eq!(
        probe,
        CompoundPrefixProbe::Malformed("CFB v4 header padding is not zero".into())
    );
    drop(storage);
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(
        super::probe(&file),
        CompoundPrefixProbe::Malformed("CFB v4 header padding is not zero".into())
    );
}

#[test]
fn range_lock_sector_insertion_uses_core_tree_admission_once() {
    let mut directory = [0_u8; 4096];
    initialize_empty_directory_entries(&mut directory);
    directory_entry(
        &mut directory,
        0,
        "Root Entry",
        5,
        NO_STREAM,
        NO_STREAM,
        NO_STREAM,
        END_OF_CHAIN,
        0,
    );
    let directory = with_context(&directory, &DecodePolicy::service(), |ctx| {
        parse_directory(ctx, &directory, CompoundVersion::V4).expect("valid root directory")
    });
    let state = CompoundState {
        version: CompoundVersion::V4,
        sector_count: 1,
        fat: vec![END_OF_CHAIN],
        mini_fat: Vec::new(),
        directory,
        directory_chain: None,
        mini_fat_chain: None,
        root_mini_chain: None,
        fat_sectors: std::collections::BTreeSet::new(),
        difat_sectors: std::collections::BTreeSet::new(),
        range_lock_sector: Some(0),
    };

    // A first core B-tree node pays three node passes. The final FAT walk
    // pays one u32 visit and one u32 membership comparison.
    let node_bound = 11 * (std::mem::size_of::<u32>() + std::mem::size_of::<()>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<u32>()
            .max(std::mem::align_of::<()>())
            .max(std::mem::align_of::<usize>());
    let insertion_work = 3 * node_bound;
    let fat_visit_work = 1;
    let fat_membership_work = std::mem::size_of::<u32>();
    let exact_work = insertion_work + fat_visit_work + fat_membership_work;

    let zero_policy = zero_work_policy();
    let (error, refusal) = with_context(&[], &zero_policy, |ctx| {
        let error = state
            .validate_sector_ownership(ctx, &[])
            .expect_err("the core tree insertion requires its actual work admission");
        (error, ctx.resource_refusal())
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("tree work refusal is typed");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "validate CFB sector ownership");
    assert_eq!(
        (limit.used, limit.additional),
        (
            0,
            u64::try_from(insertion_work).expect("insertion work fits u64")
        )
    );
    assert_eq!(refusal, Some(limit));

    let mut exact_policy = DecodePolicy::service();
    exact_policy.limits.max_work_units =
        u64::try_from(exact_work).expect("exact work total fits u64");
    with_context(&[], &exact_policy, |ctx| {
        state
            .validate_sector_ownership(ctx, &[])
            .expect("core insertion and the one-entry FAT check fit the derived work total");
        assert_eq!(ctx.resource_refusal(), None);
    });

    let mut one_less_policy = DecodePolicy::service();
    one_less_policy.limits.max_work_units =
        u64::try_from(exact_work - 1).expect("one-less work total fits u64");
    let (error, refusal) = with_context(&[], &one_less_policy, |ctx| {
        let error = state
            .validate_sector_ownership(ctx, &[])
            .expect_err("one less unit cannot finish the single FAT membership check");
        (error, ctx.resource_refusal())
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("FAT membership refusal is typed");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "check CFB FAT ownership");
    assert_eq!(
        (limit.used, limit.additional),
        (
            u64::try_from(insertion_work + fat_visit_work)
                .expect("work before membership check fits u64"),
            u64::try_from(fat_membership_work).expect("u32 comparison work fits u64")
        )
    );
    assert_eq!(
        limit.limit,
        u64::try_from(exact_work - 1).expect("one-less work total fits u64")
    );
    assert_eq!(refusal, Some(limit));
}

#[test]
fn snapshot_constructor_returns_bad_magic_diagnostic_without_work() {
    let mut file = fixture();
    file[0] = 0;
    let policy = zero_work_policy();
    let fresh_arena = DecodeArena::new();
    let (fresh_ctx, fresh_root) =
        DecodeContext::from_root_bytes(&file, &fresh_arena, &policy).expect("fresh root");
    let error = CompoundSnapshot::new(&fresh_ctx, fresh_root)
        .expect_err("fresh invalid magic keeps its structural diagnostic");
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "input is not a CFB file"));
    assert_eq!(fresh_ctx.resource_refusal(), None);
}

#[test]
fn fragmented_stream_open_refuses_one_unit_below_derived_work_peak() {
    let mut file = fixture();
    let fat = sector_mut(&mut file, 11);
    put_u32(fat, 2 * 4, 4);
    put_u32(fat, 4 * 4, 3);
    put_u32(fat, 3 * 4, 5);

    let setup_arena = DecodeArena::new();
    let setup_policy = DecodePolicy::service();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&file, &setup_arena, &setup_policy).expect("setup root");
    let snapshot = CompoundSnapshot::new(&setup_ctx, root).expect("valid fragmented allocation");
    let stream = snapshot
        .stream(&setup_ctx, "Store/Large")
        .expect("stream lookup")
        .expect("large stream");

    let rest_visits = 2 * (8 - 1);
    let concat_view_visits = 2 * 8;
    let copied_bytes = 8 * SECTOR_SIZE;
    let work_limit = u64::try_from(rest_visits + concat_view_visits + copied_bytes)
        .expect("fragmented open work fits u64");
    let work_before_last_copy = rest_visits + concat_view_visits + (8 - 1) * SECTOR_SIZE;

    let mut exact_policy = DecodePolicy::service();
    exact_policy.limits.max_work_units = work_limit;
    with_context(&[], &exact_policy, |ctx| {
        let opened = snapshot
            .open(ctx, stream)
            .expect("the derived work budget admits the complete copy");
        assert_eq!(opened.window(), &[0x5a; 8 * SECTOR_SIZE]);
        assert_eq!(ctx.resource_refusal(), None);
    });

    let mut one_less_policy = DecodePolicy::service();
    one_less_policy.limits.max_work_units = work_limit - 1;
    let (error, refusal) = with_context(&[], &one_less_policy, |ctx| {
        let Err(error) = snapshot.open(ctx, stream) else {
            panic!("one fewer work unit cannot complete the final sector copy")
        };
        (error, ctx.resource_refusal())
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("concat work refusal is typed");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "concat_views");
    assert_eq!(limit.limit, work_limit - 1);
    assert_eq!(
        (limit.used, limit.additional),
        (
            u64::try_from(work_before_last_copy).expect("prior work fits u64"),
            u64::try_from(SECTOR_SIZE).expect("sector work fits u64"),
        )
    );
    assert_eq!(refusal, Some(limit));
}
