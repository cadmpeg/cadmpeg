use super::*;
use cadmpeg_core::decode::ResourceDimension;
use std::mem::{align_of, size_of};

// Core materialized admission starts at 16 MiB before its input-sized term.
// This ceiling gives each fixture an exact, input-independent probe boundary.
const SNAPSHOT_MATERIALIZED_LIMIT: u64 = 16 * 1024 * 1024;

fn node_bytes<K, V>() -> usize {
    11 * (size_of::<K>() + size_of::<V>())
        + 16 * size_of::<usize>()
        + 2 * align_of::<K>()
            .max(align_of::<V>())
            .max(align_of::<usize>())
}

fn extended_mini_fixture() -> Vec<u8> {
    let mut file = fixture();
    file.resize(14 * SECTOR_SIZE, 0);
    directory_entry(
        sector_mut(&mut file, 0),
        0,
        "Root Entry",
        5,
        NO_STREAM,
        NO_STREAM,
        1,
        1,
        1024,
    );
    directory_entry(
        sector_mut(&mut file, 0),
        1,
        "Small",
        2,
        NO_STREAM,
        2,
        NO_STREAM,
        8,
        5,
    );
    put_u32(sector_mut(&mut file, 11), 4, 12);
    put_u32(sector_mut(&mut file, 11), 12 * 4, END_OF_CHAIN);
    put_u32(sector_mut(&mut file, 10), 0, FREE_SECTOR);
    put_u32(sector_mut(&mut file, 10), 8 * 4, END_OF_CHAIN);
    sector_mut(&mut file, 12)[..5].copy_from_slice(b"small");
    file
}

#[test]
fn snapshot_construction_and_drop_use_no_retained_metadata() {
    let file = fixture();
    let mut invalid = file.clone();
    invalid.resize(14 * SECTOR_SIZE, 0);
    put_u32(sector_mut(&mut invalid, 11), 12 * 4, END_OF_CHAIN);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SNAPSHOT_MATERIALIZED_LIMIT;
    policy.limits.max_retained_bytes = 0;
    let mut bytes = file.clone();
    bytes.extend_from_slice(&invalid);
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
    let invalid_root = root
        .child(file.len(), bytes.len())
        .expect("invalid fixture root");
    let root = root.child(0, file.len()).expect("valid fixture root");
    for _ in 0..2 {
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("scoped valid metadata");
        assert_eq!(snapshot.entries().len(), 3);
        drop(snapshot);
        let error =
            CompoundSnapshot::new(&ctx, invalid_root).expect_err("unowned allocated sector");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "unowned CFB sector is not marked free"));
        let entire_allowance = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released constructor storage",
            )
            .expect("success and failure release all metadata");
        drop(entire_allowance);
    }
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn snapshot_keeps_only_source_derived_navigation_storage() {
    let regular = fixture_v4();
    let mini = extended_mini_fixture();
    let index_nodes = node_bytes::<Vec<Vec<u16>>, usize>()
        + node_bytes::<crate::compound::CompoundStreamId, usize>();
    // Core amortized vectors start with four slots for these element widths.
    // Wide has four UTF-16 units, one component, one path, and no chain rest.
    let regular_bytes = 4 * size_of::<CompoundEntry>()
        + 4
        + 4 * size_of::<Vec<u16>>()
        + 4 * size_of::<u16>()
        + index_nodes;
    // Small, Store, Store/Large have 21 path bytes and four components whose
    // five units each grow to eight slots. Each path holds four components.
    // Large keeps seven FAT ids; the two-sector root keeps one FAT id.
    let mini_bytes = 4 * size_of::<CompoundEntry>()
        + 21
        + 3 * 4 * size_of::<Vec<u16>>()
        + 4 * 8 * size_of::<u16>()
        + 8 * size_of::<u32>()
        + index_nodes;
    for (file, live_bytes) in [(&regular, regular_bytes), (&mini, mini_bytes)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = SNAPSHOT_MATERIALIZED_LIMIT;
        policy.limits.max_retained_bytes = 0;
        let (ctx, root) = DecodeContext::from_root_bytes(file, &arena, &policy).expect("root");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("compact snapshot");
        let remainder = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes
                    - cadmpeg_core::decode::u64_from_index(live_bytes),
                "exact navigation remainder",
            )
            .expect("construction scratch has no surviving charge");
        let CodecError::ResourceLimit(first) = ctx
            .reserve_scoped(1, "one byte beyond live navigation")
            .expect_err("the complete allowance is already live")
        else {
            panic!("typed refusal")
        };
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.used, policy.limits.max_materialized_bytes);
        assert_eq!(first.additional, 1);
        drop(remainder);
        drop(snapshot);
        let CodecError::ResourceLimit(repeated) = ctx
            .reserve_scoped(0, "after snapshot drop")
            .expect_err("drop preserves original refusal")
        else {
            panic!("typed refusal")
        };
        assert_eq!(first, repeated);
    }
}

#[test]
fn opened_regular_and_multi_sector_mini_views_survive_snapshot_drop() {
    for (file, path, expected) in [
        (fixture_v4(), "Wide", vec![0x6d; 4096]),
        (extended_mini_fixture(), "Small", b"small".to_vec()),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = SNAPSHOT_MATERIALIZED_LIMIT;
        policy.limits.max_retained_bytes = 0;
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("root");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("snapshot");
        let stream = snapshot
            .stream(&ctx, path)
            .expect("lookup")
            .expect("stream");
        let opened = snapshot.open(&ctx, stream).expect("borrowed payload");
        drop(snapshot);
        assert_eq!(opened.window(), expected);
        let entire_allowance = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "metadata after opened view",
            )
            .expect("borrowed view retains no snapshot storage");
        drop(entire_allowance);
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn fragmented_opened_view_owns_payload_after_snapshot_drop() {
    let mut file = fixture();
    let fat = sector_mut(&mut file, 11);
    put_u32(fat, 2 * 4, 4);
    put_u32(fat, 4 * 4, 3);
    put_u32(fat, 3 * 4, 5);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SNAPSHOT_MATERIALIZED_LIMIT;
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("root");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("snapshot");
    let stream = snapshot
        .stream(&ctx, "Store/Large")
        .expect("lookup")
        .expect("stream");
    let opened = snapshot.open(&ctx, stream).expect("fragmented payload");
    drop(snapshot);
    assert_eq!(opened.window(), &[0x5a; 4096]);
    let entire_allowance = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "fragmented payload is retained",
        )
        .expect("snapshot and view-list storage released");
    drop(entire_allowance);
}

#[test]
fn short_stream_diagnostic_remains_retained_after_snapshot_drop() {
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
    let message = "CFB stream Store/Large is shorter than declared";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SNAPSHOT_MATERIALIZED_LIMIT;
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(message.len());
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("root");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("snapshot");
    let stream = snapshot
        .stream(&ctx, "Store/Large")
        .expect("lookup")
        .expect("stream");
    let error = snapshot
        .open(&ctx, stream)
        .expect_err("missing logical payload");
    drop(snapshot);
    assert!(matches!(error, CodecError::Malformed(ref detail) if detail == message));
    let entire_allowance = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "diagnostic survives metadata",
        )
        .expect("diagnostic is not scoped metadata");
    drop(entire_allowance);
    let CodecError::ResourceLimit(limit) = ctx
        .charge_retained(1, "retained diagnostic boundary")
        .expect_err("exact diagnostic allowance is used")
    else {
        panic!("typed refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        limit.used,
        cadmpeg_core::decode::u64_from_index(message.len())
    );
    assert_eq!(limit.additional, 1);
}
