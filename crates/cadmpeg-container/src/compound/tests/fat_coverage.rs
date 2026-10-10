use super::*;
use crate::compound::{probe_directory_availability, PrefixDirectoryAvailability};
use cadmpeg_core::decode::View;

#[test]
fn discovered_fat_ids_admit_only_actual_header_slots() {
    let mut file = fixture();
    put_u32(&mut file, 44, 128);
    put_u32(&mut file, 68, 1_000);
    put_u32(&mut file, 72, 1);
    for index in 0..109 {
        put_u32(
            &mut file,
            76 + index * 4,
            2_000 + u32::try_from(index).expect("fixed index"),
        );
    }
    for limit in [109, 108] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("root");
        let result = probe_directory_availability(&ctx, root);
        if limit == 109 {
            assert!(matches!(
                result.expect("future DIFAT IDs are not allocated"),
                PrefixDirectoryAvailability::Incomplete
            ));
            assert_eq!(ctx.resource_refusal(), None);
            ctx.charge_retained(0, "failed probe releases ID backing")
                .expect("no retained scratch");
        } else {
            let Err(CodecError::ResourceLimit(refusal)) = result else {
                panic!("actual last ID must refuse")
            };
            assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
            assert_eq!(refusal.operation, "probe CFB FAT sectors");
            assert_eq!((refusal.used, refusal.additional), (108, 1));
            assert_eq!(ctx.resource_refusal(), Some(refusal));
            let error = probe_directory_availability(&ctx, root)
                .err()
                .expect("original refusal");
            assert!(matches!(error, CodecError::ResourceLimit(saved) if saved == refusal));
        }
    }
}

fn role_word_after_first_fat() -> Vec<u8> {
    let mut file = fixture();
    file.resize(SECTOR_SIZE * 257, 0);
    put_u32(&mut file, 44, 2);
    put_u32(&mut file, 60, END_OF_CHAIN);
    put_u32(&mut file, 64, 0);
    put_u32(&mut file, 76, 254);
    put_u32(&mut file, 80, 255);
    let directory = sector_mut(&mut file, 0);
    initialize_empty_directory_entries(directory);
    directory_entry(
        directory,
        0,
        "Root Entry",
        5,
        NO_STREAM,
        NO_STREAM,
        NO_STREAM,
        END_OF_CHAIN,
        0,
    );
    let first = sector_mut(&mut file, 254);
    first.fill(0xff);
    put_u32(first, 0, END_OF_CHAIN);
    let second = sector_mut(&mut file, 255);
    second.fill(0xff);
    put_u32(second, 126 * 4, FAT_SECTOR);
    put_u32(second, 127 * 4, FAT_SECTOR);
    file
}

#[test]
fn missing_fat_role_word_is_incomplete_and_detection_reads_more() {
    let file = role_word_after_first_fat();
    let first_prefix = SECTOR_SIZE * 256;
    assert_eq!(
        probe(&file[..first_prefix]),
        CompoundPrefixProbe::Incomplete
    );
    assert_eq!(
        probe(&file),
        CompoundPrefixProbe::DirectoryEvidence(Vec::new())
    );
    with_context(&file, &DecodePolicy::service(), |ctx| {
        let snapshot =
            CompoundSnapshot::new(ctx, View::over_retained(&file)).expect("valid full CFB");
        assert!(snapshot.entries().is_empty());
    });
    with_context(&[], &DecodePolicy::service(), |ctx| {
        let mut source = std::io::Cursor::new(&file);
        let bytes = read_detection_prefix(ctx, &mut source, first_prefix)
            .expect("missing role extends prefix");
        assert_eq!(bytes, file);
        assert_eq!(
            source.position(),
            u64::try_from(file.len()).expect("file length")
        );
    });
}

#[test]
fn present_wrong_fat_role_word_is_malformed() {
    let mut file = role_word_after_first_fat();
    put_u32(sector_mut(&mut file, 255), 126 * 4, END_OF_CHAIN);
    assert_eq!(
        probe(&file),
        CompoundPrefixProbe::Malformed("CFB allocation sector has the wrong role marker".into())
    );
    with_context(&file, &DecodePolicy::service(), |ctx| {
        let error =
            CompoundSnapshot::new(ctx, View::over_retained(&file)).expect_err("known wrong role");
        assert!(matches!(error, CodecError::Malformed(_)));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn discovered_fat_id_backing_observes_actual_growth_and_releases_failed_probe() {
    // The 65th u32 ID grows 64 slots to 128. The new 512-byte backing
    // overlaps the old 256-byte backing until reallocation completes.
    let old_bytes = 64 * std::mem::size_of::<u32>();
    let new_bytes = 128 * std::mem::size_of::<u32>();
    let peak = old_bytes + new_bytes;
    let observed_ids_bytes = 109 * std::mem::size_of::<u32>();
    for absent_difat in [true, false] {
        let mut file = fixture();
        file.truncate(SECTOR_SIZE);
        put_u32(&mut file, 44, 128);
        put_u32(&mut file, 68, if absent_difat { 110 } else { END_OF_CHAIN });
        put_u32(&mut file, 72, u32::from(absent_difat));
        for index in 0..109 {
            put_u32(
                &mut file,
                76 + index * 4,
                u32::try_from(index).expect("fixed header slot"),
            );
        }
        for limit in [observed_ids_bytes, peak - 1, peak] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 109;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(limit);
            let arena = DecodeArena::new();
            let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("root");
            let result = probe_directory_availability(&ctx, root);
            if limit == peak {
                if absent_difat {
                    assert!(matches!(
                        result.expect("discovered header backing fits"),
                        PrefixDirectoryAvailability::Incomplete
                    ));
                } else {
                    assert!(matches!(
                        result.expect("discovered header backing fits"),
                        PrefixDirectoryAvailability::Malformed(
                            "CFB DIFAT does not match its declared FAT count"
                        )
                    ));
                }
                assert_eq!(ctx.resource_refusal(), None);
                let entire_allowance = ctx
                    .reserve_scoped(
                        policy.limits.max_materialized_bytes,
                        "released FAT ID backing",
                    )
                    .expect("probe scratch is fully released");
                drop(entire_allowance);
                ctx.charge_retained(0, "FAT ID probe retains no storage")
                    .expect("zero retained allowance");
            } else {
                let Err(CodecError::ResourceLimit(first)) = result else {
                    panic!("actual vector allocation peak must refuse")
                };
                assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(first.operation, "probe CFB FAT sectors");
                let used = if limit == observed_ids_bytes {
                    old_bytes
                } else {
                    new_bytes
                };
                assert_eq!(
                    (first.used, first.additional),
                    (
                        cadmpeg_core::decode::u64_from_index(used),
                        cadmpeg_core::decode::u64_from_index(old_bytes)
                    )
                );
                assert_eq!(ctx.resource_refusal(), Some(first));
                let error = probe_directory_availability(&ctx, root)
                    .err()
                    .expect("sticky refusal");
                assert!(matches!(error, CodecError::ResourceLimit(saved) if saved == first));
            }
        }
    }
}
