use super::*;

#[test]
fn stream_open_admits_only_needed_sector_view_storage() {
    use cadmpeg_core::decode::ResourceDimension;
    for fragmented in [false, true] {
        let mut file = fixture();
        if fragmented {
            let fat = sector_mut(&mut file, 11);
            put_u32(fat, 2 * 4, 4);
            put_u32(fat, 4 * 4, 3);
            put_u32(fat, 3 * 4, 5);
        }
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&file, &arena, &policy).expect("fixture root");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("valid allocation");
        let stream = snapshot
            .stream(&ctx, "Store/Large")
            .expect("lookup admission")
            .expect("large stream");
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        with_context(&[], &policy, |ctx| {
            let opened = snapshot.open(ctx, stream);
            if fragmented {
                assert!(
                    matches!(opened, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
                );
            } else {
                assert_eq!(
                    opened.expect("contiguous stream borrows sectors").window(),
                    &[0x5a; 4096]
                );
            }
        });
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let error = with_context(&[], &policy, |ctx| {
            let error = snapshot
                .open(ctx, stream)
                .expect_err("remaining sector visits exceed zero work");
            let CodecError::ResourceLimit(first) = &error else {
                panic!("remaining sector visits refuse under zero work");
            };
            let repeated = snapshot.open(ctx, stream).expect_err("original tail refusal");
            assert!(matches!(repeated, CodecError::ResourceLimit(limit) if limit == *first));
            assert_eq!(ctx.resource_refusal(), Some(*first));
            error
        });
        let CodecError::ResourceLimit(limit) = error else {
            panic!("remaining sector visits refuse under zero work");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "visit CFB stream sectors");
        // The first tail visit is admitted before its view check. The six
        // later sectors remain unvisited after that refusal.
        assert_eq!((limit.used, limit.additional), (0, 1));
    }
}

#[test]
fn fragmented_stream_open_charges_concat_copy_once() {
    let mut file = fixture();
    let fat = sector_mut(&mut file, 11);
    put_u32(fat, 2 * 4, 4);
    put_u32(fat, 4 * 4, 3);
    put_u32(fat, 3 * 4, 5);

    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(
        &file,
        &arena,
        &DecodePolicy::service(),
    )
    .expect("fixture root");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("valid allocation");
    let stream = snapshot
        .stream(&ctx, "Store/Large")
        .expect("lookup admission")
        .expect("large stream");

    // Two seven-sector rest traversals, two eight-view concat passes,
    // and one 4096-byte copy.
    let work_limit = u64::try_from(2 * (8 - 1) + 2 * 8 + 8 * SECTOR_SIZE)
        .expect("work total fits u64");
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work_limit;
    with_context(&[], &policy, |ctx| {
        assert_eq!(
            snapshot
                .open(ctx, stream)
                .expect("concat_views admits the copy")
                .window(),
            &[0x5a; 8 * SECTOR_SIZE]
        );
    });
}

#[test]
fn snapshot_opens_regular_and_mini_streams_lazily() {
    let file = fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
        .expect("synthetic CFB fits the decode policy");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("synthetic CFB parses");
    assert_eq!(snapshot.major_version(), 3);
    assert_eq!(
        snapshot
            .open(
                &ctx,
                snapshot
                    .stream(&ctx, "small")
                    .expect("lookup admission")
                    .expect("small stream exists"),
            )
            .expect("small stream opens")
            .window(),
        b"small"
    );
    assert_eq!(
        snapshot
            .open(
                &ctx,
                snapshot
                    .stream(&ctx, "Store/Large")
                    .expect("lookup admission")
                    .expect("regular stream exists"),
            )
            .expect("regular stream opens")
            .window(),
        vec![0x5a; 4096]
    );
    assert!(matches!(
        snapshot.entry(&ctx, "STORE").expect("lookup admission"),
        Some(CompoundEntry::Storage(_))
    ));
}

#[test]
fn stream_open_uses_absolute_coordinates_for_nonzero_root_views() {
    for prefix_len in [16, 1024] {
        for empty in [false, true] {
            let mut file = fixture();
            if empty {
                directory_entry(
                    sector_mut(&mut file, 0),
                    1,
                    "Small",
                    2,
                    NO_STREAM,
                    2,
                    NO_STREAM,
                    END_OF_CHAIN,
                    0,
                );
                put_u32(sector_mut(&mut file, 10), 0, FREE_SECTOR);
            }
            let mut prefixed = vec![0_u8; prefix_len];
            prefixed.extend_from_slice(&file);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&prefixed, &arena, &DecodePolicy::service())
                    .expect("prefixed fixture fits policy");
            let cfb = root
                .child(prefix_len, prefixed.len())
                .expect("CFB child view");
            let snapshot =
                CompoundSnapshot::new(&ctx, cfb).expect("CFB parses within child view");
            let small = snapshot
                .open(
                    &ctx,
                    snapshot
                        .stream(&ctx, "Small")
                        .expect("lookup admission")
                        .expect("small stream"),
                )
                .expect("mini or empty stream opens within child view");
            assert_eq!(small.window(), if empty { &b""[..] } else { &b"small"[..] });
            assert_eq!(
                small.start(),
                if empty {
                    0
                } else {
                    prefix_len + 2 * SECTOR_SIZE
                }
            );
            assert_eq!(
                snapshot
                    .regular_sector_view(2)
                    .expect("regular sector view")
                    .start(),
                prefix_len + 3 * SECTOR_SIZE
            );
            let large = snapshot
                .open(
                    &ctx,
                    snapshot
                        .stream(&ctx, "Store/Large")
                        .expect("lookup admission")
                        .expect("regular stream"),
                )
                .expect("regular stream opens within child view");
            assert_eq!(large.window(), &[0x5a; 4096]);
        }
    }
}

#[test]
fn snapshot_opens_a_stream_from_a_partial_final_sector() {
    let file = partial_regular_fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
        .expect("synthetic CFB fits the decode policy");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("synthetic CFB parses");
    let stream = snapshot
        .open(
            &ctx,
            snapshot
                .stream(&ctx, "Store/Large")
                .expect("lookup admission")
                .expect("regular stream exists"),
        )
        .expect("regular stream opens through the partial sector");
    assert_eq!(stream.window().len(), 4110);
    assert!(stream.window().iter().all(|byte| *byte == 0x5a));

    let mut too_large = partial_regular_fixture();
    sector_mut(&mut too_large, 0)[3 * 128 + 120..4 * 128]
        .copy_from_slice(&4608_u64.to_le_bytes());
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&too_large, &arena, &policy)
        .expect("synthetic CFB fits the decode policy");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("metadata still parses");
    let stream = snapshot
        .stream(&ctx, "Store/Large")
        .expect("lookup admission")
        .expect("regular stream exists");
    assert!(matches!(
        snapshot.open(&ctx, stream),
        Err(CodecError::Malformed(detail))
            if detail == "CFB stream Store/Large is shorter than declared"
    ));

    let mut limited_policy = DecodePolicy::service();
    // Available extent is checked before any view list or arena copy.
    let diagnostic_bytes = u64::try_from(
        "CFB stream Store/Large is shorter than declared".len(),
    )
    .expect("diagnostic length fits u64");
    limited_policy.limits.max_retained_bytes = diagnostic_bytes;
    limited_policy.limits.max_materialized_bytes = 0;
    let error = with_context(&[], &limited_policy, |ctx| {
        snapshot.open(ctx, stream).expect_err("actual extent is too short")
    });
    assert!(matches!(error, CodecError::Malformed(detail)
        if detail == "CFB stream Store/Large is shorter than declared"));

    limited_policy.limits.max_retained_bytes -= 1;
    let refusal = with_context(&[], &limited_policy, |ctx| {
        snapshot.open(ctx, stream).expect_err("the escaping diagnostic requires storage")
    });
    assert!(matches!(
        refusal,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "CFB stream extent error"
                && limit.used == 0
                && limit.additional == diagnostic_bytes
    ));
}

#[test]
fn snapshot_rejects_stream_handles_from_another_snapshot() {
    let file = fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
        .expect("synthetic CFB fits the decode policy");
    let first = CompoundSnapshot::new(&ctx, root).expect("first CFB snapshot parses");
    let second = CompoundSnapshot::new(&ctx, root).expect("second CFB snapshot parses");
    let foreign = second
        .stream(&ctx, "Small")
        .expect("lookup admission")
        .expect("foreign stream exists");
    assert!(first.open(&ctx, foreign).is_err());

    let owned = first
        .stream(&ctx, "Small")
        .expect("lookup admission")
        .expect("owned stream exists")
        .clone();
    assert_eq!(
        first
            .open(&ctx, &owned)
            .expect("owned clone opens")
            .window(),
        b"small"
    );
}

#[test]
fn stream_open_propagates_fused_refusal_before_handle_checks() {
    let mut empty_file = fixture();
    directory_entry(
        sector_mut(&mut empty_file, 0),
        1,
        "Small",
        2,
        NO_STREAM,
        2,
        NO_STREAM,
        END_OF_CHAIN,
        0,
    );
    put_u32(sector_mut(&mut empty_file, 10), 0, FREE_SECTOR);
    let foreign_file = fixture();
    let arena = DecodeArena::new();
    let (owned_ctx, owned_root) = DecodeContext::from_root_bytes(
        &empty_file,
        &arena,
        &DecodePolicy::service(),
    )
    .expect("empty-stream fixture root");
    let owned_snapshot =
        CompoundSnapshot::new(&owned_ctx, owned_root).expect("empty-stream snapshot");
    let empty_stream = owned_snapshot
        .stream(&owned_ctx, "Small")
        .expect("empty-stream lookup")
        .expect("empty stream exists")
        .clone();
    let (foreign_ctx, foreign_root) = DecodeContext::from_root_bytes(
        &foreign_file,
        &arena,
        &DecodePolicy::service(),
    )
    .expect("foreign-stream fixture root");
    let foreign_snapshot =
        CompoundSnapshot::new(&foreign_ctx, foreign_root).expect("foreign snapshot");
    let foreign_stream = foreign_snapshot
        .stream(&foreign_ctx, "Small")
        .expect("foreign-stream lookup")
        .expect("foreign stream exists")
        .clone();

    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (fused_ctx, _) =
        DecodeContext::from_root_bytes(&empty_file, &arena, &policy).expect("fused context");
    let CodecError::ResourceLimit(first) = fused_ctx
        .charge_work(1, "fuse CFB open context")
        .expect_err("zero work allowance fuses the context")
    else {
        panic!("resource refusal is typed");
    };
    let CodecError::ResourceLimit(foreign_refusal) = owned_snapshot
        .open(&fused_ctx, &foreign_stream)
        .expect_err("fused refusal precedes foreign handle validation")
    else {
        panic!("resource refusal is preserved");
    };
    assert_eq!(foreign_refusal, first);
    let CodecError::ResourceLimit(empty_refusal) = owned_snapshot
        .open(&fused_ctx, &empty_stream)
        .expect_err("fused refusal precedes the empty-stream fast path")
    else {
        panic!("resource refusal is preserved");
    };
    assert_eq!(empty_refusal, first);
}

#[test]
fn fragmented_mini_stream_copies_only_logical_payload() {
    let mut file = fixture();
    directory_entry(sector_mut(&mut file, 0), 1, "Small", 2,
        NO_STREAM, 2, NO_STREAM, 0, 66);
    let mini_fat = sector_mut(&mut file, 10);
    put_u32(mini_fat, 0, 2);
    put_u32(mini_fat, 2 * 4, END_OF_CHAIN);
    let payload = sector_mut(&mut file, 1);
    payload[..64].fill(b'A');
    payload[128..192].fill(b'P');
    payload[128..130].copy_from_slice(b"BC");
    for prefix in [0, 17] {
        let mut bytes = vec![0_u8; prefix];
        bytes.extend_from_slice(&file);
        let setup_arena = DecodeArena::new();
        let (setup, root) = DecodeContext::from_root_bytes(&bytes, &setup_arena, &DecodePolicy::service())
            .expect("prefixed fixture root");
        let source = root.child(prefix, bytes.len()).expect("CFB root");
        let snapshot = CompoundSnapshot::new(&setup, source).expect("valid fragmented mini allocation");
        let stream = snapshot.stream(&setup, "Small").expect("lookup").expect("small stream");
        // Two one-sector tail visits, two two-view concat passes and exactly
        // 64+2 copied logical bytes. The second sector's 62 padding bytes
        // remain borrowed input and are absent from the derived output.
        let logical_bytes = 66;
        let views = 2;
        let prior_copy_work = 2 * (views - 1) + 2 * views;
        let exact_work = prior_copy_work + logical_bytes;
        for limit in [exact_work, exact_work - 1] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::try_from(limit).expect("copy work fits u64");
            with_context(&[], &policy, |ctx| {
                let result = snapshot.open(ctx, stream);
                if limit == exact_work {
                    let opened = result.expect("only logical bytes are copied");
                    assert_eq!(opened.window().len(), logical_bytes);
                    assert_eq!(&opened.window()[..64], &[b'A'; 64]);
                    assert_eq!(&opened.window()[64..], b"BC");
                    assert_eq!(opened.start(), 0);
                    assert_ne!(opened.location().space, source.location().space);
                    assert_eq!(ctx.resource_refusal(), None);
                } else {
                    let error = result.expect_err("the final two payload bytes need admission");
                    let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
                    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(refusal.operation, "concat_views");
                    assert_eq!((refusal.used, refusal.additional),
                        (u64::try_from(prior_copy_work + 64).expect("prior copy work"), 2));
                    assert_eq!(ctx.resource_refusal(), Some(refusal));
                }
            });
        }
    }
}

#[test]
fn fragmented_regular_stream_excludes_final_sector_padding_from_copy() {
    let mut file = partial_regular_fixture();
    let fat = sector_mut(&mut file, 11);
    put_u32(fat, 2 * 4, 4);
    put_u32(fat, 4 * 4, 3);
    put_u32(fat, 3 * 4, 5);
    let setup_arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&file, &setup_arena, &DecodePolicy::service())
        .expect("partial fixture root");
    let snapshot = CompoundSnapshot::new(&setup, root).expect("valid fragmented regular allocation");
    let stream = snapshot.stream(&setup, "Store/Large").expect("lookup").expect("large stream");
    let views = 9;
    let logical_bytes = 4110;
    let prior_copy_work = 2 * (views - 1) + 2 * views;
    let exact_work = prior_copy_work + logical_bytes;
    for limit in [exact_work, exact_work - 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(limit).expect("copy work fits u64");
        with_context(&[], &policy, |ctx| {
            let result = snapshot.open(ctx, stream);
            if limit == exact_work {
                let opened = result.expect("the final fourteen logical bytes fit");
                assert_eq!(opened.window(), &[0x5a; 4110]);
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let error = result.expect_err("the final payload slice needs admission");
                let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(refusal.operation, "concat_views");
                assert_eq!((refusal.used, refusal.additional),
                    (u64::try_from(prior_copy_work + 8 * SECTOR_SIZE).expect("prior copy work"), 14));
                assert_eq!(ctx.resource_refusal(), Some(refusal));
            }
        });
    }
}
