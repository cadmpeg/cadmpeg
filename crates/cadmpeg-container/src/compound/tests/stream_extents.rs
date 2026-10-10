use super::*;
use cadmpeg_core::decode::ByteRange;

#[test]
fn fragmented_short_stream_rejects_before_arena_storage_and_registration() {
    let mut file = partial_regular_fixture();
    directory_entry(
        sector_mut(&mut file, 0),
        3,
        "Large",
        2,
        NO_STREAM,
        NO_STREAM,
        NO_STREAM,
        2,
        4608,
    );
    let fat = sector_mut(&mut file, 11);
    put_u32(fat, 2 * 4, 4);
    put_u32(fat, 4 * 4, 3);
    put_u32(fat, 3 * 4, 5);
    let setup_arena = DecodeArena::new();
    let (setup, root) =
        DecodeContext::from_root_bytes(&file, &setup_arena, &DecodePolicy::service())
            .expect("setup");
    let snapshot = CompoundSnapshot::new(&setup, root).expect("valid metadata");
    let stream = snapshot
        .stream(&setup, "Store/Large")
        .expect("lookup")
        .expect("stream");
    let message = "CFB stream Store/Large is shorter than declared";
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = u64::try_from(2 * message.len()).expect("two diagnostics");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("open context");
    let first = ctx
        .register_slice(root, ByteRange { start: 0, end: 0 })
        .expect("registration control");
    for _ in 0..2 {
        let error = snapshot.open(&ctx, stream).expect_err("short extent");
        assert!(matches!(error, CodecError::Malformed(detail) if detail == message));
        assert_eq!(ctx.resource_refusal(), None);
    }
    let next = ctx
        .register_slice(root, ByteRange { start: 0, end: 0 })
        .expect("no failed open registration");
    assert_eq!(
        next.location().space.index(),
        first.location().space.index() + 1
    );
    ctx.charge_retained(0, "only escaping diagnostic bytes remain")
        .expect("exact retained budget");
}

fn partial_mini_fixture(logical_size: u64, mini_sector: u32) -> Vec<u8> {
    let mut file = fixture();
    file.truncate(5 * SECTOR_SIZE);
    put_u32(&mut file, 60, 1);
    put_u32(&mut file, 76, 2);
    let directory = sector_mut(&mut file, 0);
    initialize_empty_directory_entries(directory);
    directory_entry(
        directory,
        0,
        "Root Entry",
        5,
        NO_STREAM,
        NO_STREAM,
        1,
        3,
        128,
    );
    directory_entry(
        directory,
        1,
        "Small",
        2,
        NO_STREAM,
        NO_STREAM,
        NO_STREAM,
        mini_sector,
        logical_size,
    );
    let mini_fat = sector_mut(&mut file, 1);
    mini_fat.fill(0xff);
    put_u32(
        mini_fat,
        usize::try_from(mini_sector).expect("mini sector") * 4,
        END_OF_CHAIN,
    );
    let fat = sector_mut(&mut file, 2);
    fat.fill(0xff);
    put_u32(fat, 0, END_OF_CHAIN);
    put_u32(fat, 4, END_OF_CHAIN);
    put_u32(fat, 8, FAT_SECTOR);
    put_u32(fat, 12, END_OF_CHAIN);
    sector_mut(&mut file, 3)[..5].copy_from_slice(b"small");
    file.truncate(4 * SECTOR_SIZE + 5);
    file
}

#[test]
fn mini_stream_rejects_a_partial_regular_sector_boundary() {
    let mut file = partial_mini_fixture(5, 0);
    // One 64-byte logical mini sector suffices for the five-byte payload.
    sector_mut(&mut file, 0)[120..128].copy_from_slice(&64_u64.to_le_bytes());
    for prefix in [0, 17] {
        let mut bytes = vec![0_u8; prefix];
        bytes.extend_from_slice(&file);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let root = root.child(prefix, bytes.len()).expect("CFB root");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("consistent mini allocation");
        let stream = snapshot
            .stream(&ctx, "Small")
            .expect("lookup")
            .expect("stream");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        with_context(&[], &policy, |open_ctx| {
            let error = snapshot
                .open(open_ctx, stream)
                .expect_err("a full mini sector must be available");
            assert!(matches!(error, CodecError::Malformed(message)
                if message == "CFB mini sector crosses a regular-sector boundary"));
            assert_eq!(open_ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn partial_mini_stream_rejects_boundary_before_logical_payload_check() {
    let file = partial_mini_fixture(6, 0);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service()).expect("root");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("consistent declared allocation");
    let stream = snapshot
        .stream(&ctx, "Small")
        .expect("lookup")
        .expect("stream");
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes =
        u64::try_from("CFB stream Small is shorter than declared".len()).expect("diagnostic bytes");
    with_context(&[], &policy, |open_ctx| {
        let error = snapshot
            .open(open_ctx, stream)
            .expect_err("the physical mini sector is incomplete");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "CFB mini sector crosses a regular-sector boundary"));
        assert_eq!(open_ctx.resource_refusal(), None);
    });
}

#[test]
fn partial_mini_stream_rejects_a_wholly_absent_mini_sector() {
    let file = partial_mini_fixture(5, 1);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service()).expect("root");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("consistent declared allocation");
    let stream = snapshot
        .stream(&ctx, "Small")
        .expect("lookup")
        .expect("stream");
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_work_units = 0;
    with_context(&[], &policy, |open_ctx| {
        let error = snapshot
            .open(open_ctx, stream)
            .expect_err("the mini sector has no bytes");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "CFB mini sector crosses a regular-sector boundary"));
        assert_eq!(open_ctx.resource_refusal(), None);
    });
}

#[test]
fn stream_extent_stops_at_the_first_partial_mini_sector() {
    let mut file = partial_mini_fixture(130, 0);
    sector_mut(&mut file, 0)[120..128].copy_from_slice(&192_u64.to_le_bytes());
    let mini_fat = sector_mut(&mut file, 1);
    put_u32(mini_fat, 0, 1);
    put_u32(mini_fat, 4, 2);
    put_u32(mini_fat, 8, END_OF_CHAIN);
    for prefix in [0, 17] {
        let mut bytes = vec![0_u8; prefix];
        bytes.extend_from_slice(&file);
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("setup root");
        let root = root.child(prefix, bytes.len()).expect("CFB source");
        let snapshot =
            CompoundSnapshot::new(&setup, root).expect("valid declared mini allocations");
        let stream = snapshot
            .stream(&setup, "Small")
            .expect("lookup")
            .expect("stream");
        for work in [1, 0] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            with_context(&[], &policy, |ctx| {
                let error = snapshot
                    .open(ctx, stream)
                    .expect_err("first mini sector is partial");
                assert!(matches!(error, CodecError::Malformed(message)
                    if message == "CFB mini sector crosses a regular-sector boundary"));
                assert_eq!(ctx.resource_refusal(), None);
                let CodecError::ResourceLimit(refusal) = ctx
                    .charge_work(work + 1, "after absent CFB first sector")
                    .expect_err("the fixed first sector uses no work")
                else {
                    panic!("resource refusal")
                };
                assert_eq!((refusal.used, refusal.additional), (0, work + 1));
            });
        }
    }
}

#[test]
fn accepted_empty_stream_reports_end_of_chain_for_zero_start_in_summary() {
    let mut file = fixture();
    directory_entry(
        sector_mut(&mut file, 0),
        1,
        "Small",
        2,
        NO_STREAM,
        2,
        NO_STREAM,
        0,
        0,
    );
    put_u32(sector_mut(&mut file, 10), 0, FREE_SECTOR);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service()).expect("root");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("accepted empty stream");
    let stream = snapshot
        .stream(&ctx, "Small")
        .expect("lookup")
        .expect("stream");
    assert_eq!(stream.start_sector(), END_OF_CHAIN);
    assert_eq!(stream.logical_size(), 0);
    assert_eq!(stream.allocation(), None);
    assert!(snapshot
        .open(&ctx, stream)
        .expect("empty stream")
        .window()
        .is_empty());
    let summary = snapshot
        .container_entries(&ctx, |_| ContainerRole::Stream)
        .expect("summaries");
    let summary = summary
        .iter()
        .find(|entry| entry.name == "Small")
        .expect("small summary");
    assert_eq!(summary.attributes["start_sector"], END_OF_CHAIN.to_string());
}
