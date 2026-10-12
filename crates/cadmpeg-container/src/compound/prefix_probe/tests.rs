use super::{probe_directory_availability, PrefixDirectoryAvailability, PrefixProbe};
use crate::compound::tests::{
    directory_entry, fixture_v4, initialize_empty_directory_entries, probe, sector_mut_with_size,
};
use crate::compound::{
    read_detection_prefix, CompoundPrefixProbe, DIFAT_SECTOR, END_OF_CHAIN, FAT_SECTOR,
    FREE_SECTOR, NO_STREAM,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;
use cadmpeg_test_support::bytes::{put_u16, put_u32};

fn with_context<T>(
    bytes: &[u8],
    policy: &DecodePolicy,
    use_context: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy).expect("context");
    use_context(&ctx)
}

const V4_SECTOR_SIZE: usize = 4096;
const PREFIX_STEP: usize = 64 * 1024;
const DIRECTORY_ID: u32 = 95;

fn late_directory(late_fat: bool) -> Vec<u8> {
    let mut file = fixture_v4();
    file.resize(V4_SECTOR_SIZE * 97, 0);
    let directory = sector_mut_with_size(
        &mut file,
        V4_SECTOR_SIZE,
        usize::try_from(DIRECTORY_ID).expect("directory id"),
    );
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
    let fat_id = if late_fat { 94 } else { 2 };
    put_u32(&mut file, 48, DIRECTORY_ID);
    put_u32(&mut file, 76, fat_id);
    let fat = sector_mut_with_size(
        &mut file,
        V4_SECTOR_SIZE,
        usize::try_from(fat_id).expect("FAT id"),
    );
    fat.fill(0xff);
    put_u32(
        fat,
        usize::try_from(DIRECTORY_ID).expect("directory id") * 4,
        END_OF_CHAIN,
    );
    put_u32(
        fat,
        usize::try_from(fat_id).expect("FAT id") * 4,
        FAT_SECTOR,
    );
    file
}

fn probe_work() -> usize {
    let id_node_bytes = 11 * std::mem::size_of::<u32>()
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<usize>());
    // One FAT visit, 1024 words, a FAT role and its end visit, the
    // empty DIFAT end visit, and one directory visit with three node passes.
    1 + V4_SECTOR_SIZE / 4 + 2 + 1 + 1 + 3 * id_node_bytes
}

#[test]
fn resumable_detection_work_equals_final_pass_and_extension_visits() {
    for late_fat in [false, true] {
        let file = late_directory(late_fat);
        let final_probe_work = probe_work();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cadmpeg_core::decode::u64_from_index(final_probe_work);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("root");
        let PrefixDirectoryAvailability::Ready {
            directory_chain,
            storage,
            ..
        } = probe_directory_availability(&ctx, root).expect("one exact final pass")
        else {
            panic!("final prefix is ready");
        };
        assert_eq!(directory_chain, [DIRECTORY_ID]);
        drop((directory_chain, storage));
        assert!(
            matches!(ctx.charge_work(1, "exact final pass"), Err(CodecError::ResourceLimit(limit))
            if limit.used == cadmpeg_core::decode::u64_from_index(final_probe_work))
        );

        // Input uses 48 full 8192-byte chunks, one half chunk, and an EOF
        // read. Each successful growth moves the preceding exact capacity.
        let full_chunks = file.len() / 8192;
        let read_work = file.len().next_multiple_of(8192) + 8192;
        let copy_work = file.len();
        let input_visits = full_chunks + 2;
        let relocation_work = 8192 * full_chunks * (full_chunks + 1) / 2;
        let prefix_visits = file.len().div_ceil(PREFIX_STEP);
        let exact_work = read_work
            + copy_work
            + input_visits
            + relocation_work
            + final_probe_work
            + prefix_visits;
        for limit in [exact_work, exact_work - 1] {
            policy.limits.max_work_units = cadmpeg_core::decode::u64_from_index(limit);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            let mut source = std::io::Cursor::new(&file);
            let result = read_detection_prefix(&ctx, &mut source, PREFIX_STEP);
            if limit == exact_work {
                assert_eq!(result.expect("one pass plus extension visits"), file);
                assert!(
                    matches!(ctx.charge_work(1, "exact detection pass"), Err(CodecError::ResourceLimit(refusal))
                    if refusal.used == cadmpeg_core::decode::u64_from_index(exact_work))
                );
            } else {
                assert!(matches!(result, Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::WorkUnits
                        && refusal.operation == "CFB probe directory visits"
                        && refusal.used + refusal.additional == cadmpeg_core::decode::u64_from_index(exact_work)));
            }
        }
    }
}

#[test]
fn resumable_prefix_decisions_match_fixed_final_inspection() {
    for outcome in ["ready", "wrong-role", "incomplete"] {
        let mut file = late_directory(true);
        let expected = match outcome {
            "wrong-role" => {
                put_u32(
                    sector_mut_with_size(&mut file, V4_SECTOR_SIZE, 94),
                    94 * 4,
                    END_OF_CHAIN,
                );
                file.truncate(V4_SECTOR_SIZE * 96);
                CompoundPrefixProbe::Malformed(
                    "CFB allocation sector has the wrong role marker".into(),
                )
            }
            "incomplete" => {
                file.truncate(V4_SECTOR_SIZE * 95);
                CompoundPrefixProbe::Incomplete
            }
            _ => CompoundPrefixProbe::DirectoryEvidence(Vec::new()),
        };
        assert_eq!(probe(&file[..PREFIX_STEP]), CompoundPrefixProbe::Incomplete);
        assert_eq!(probe(&file), expected);
        with_context(&[], &DecodePolicy::service(), |ctx| {
            let mut state = PrefixProbe::new();
            assert!(matches!(
                state
                    .advance(ctx, View::over_retained(&file[..PREFIX_STEP]))
                    .expect("initial prefix"),
                PrefixDirectoryAvailability::Incomplete
            ));
            for end in (2 * PREFIX_STEP..file.len()).step_by(PREFIX_STEP) {
                assert!(matches!(
                    state
                        .advance(ctx, View::over_retained(&file[..end]))
                        .expect("extended prefix"),
                    PrefixDirectoryAvailability::Incomplete
                ));
            }
            let decision = state
                .advance(ctx, View::over_retained(&file))
                .expect("final prefix");
            match (&expected, decision) {
                (
                    CompoundPrefixProbe::DirectoryEvidence(_),
                    PrefixDirectoryAvailability::Ready {
                        directory_chain,
                        storage,
                        ..
                    },
                ) => {
                    assert_eq!(directory_chain, [DIRECTORY_ID]);
                    drop((directory_chain, storage));
                }
                (
                    CompoundPrefixProbe::Malformed(message),
                    PrefixDirectoryAvailability::Malformed(detail),
                ) => assert_eq!(message, detail),
                (CompoundPrefixProbe::Incomplete, PrefixDirectoryAvailability::Incomplete) => {}
                _ => panic!("resumed decision differs from fixed inspection"),
            }
        });
        with_context(&[], &DecodePolicy::service(), |ctx| {
            let mut source = std::io::Cursor::new(&file);
            let bytes =
                read_detection_prefix(ctx, &mut source, PREFIX_STEP).expect("detection prefix");
            assert_eq!(bytes, file);
            assert_eq!(probe(&bytes), expected);
        });
    }
}

#[test]
fn resumable_difat_walk_preserves_role_order_and_directory_continuation() {
    let mut file = late_directory(false);
    put_u32(&mut file, 68, 94);
    put_u32(&mut file, 72, 2);
    put_u32(&mut file, 76, FREE_SECTOR);
    let first = sector_mut_with_size(&mut file, V4_SECTOR_SIZE, 94);
    first.fill(0xff);
    put_u32(first, 0, 2);
    put_u32(first, V4_SECTOR_SIZE - 4, 30);
    let second = sector_mut_with_size(&mut file, V4_SECTOR_SIZE, 30);
    second.fill(0xff);
    put_u32(second, V4_SECTOR_SIZE - 4, END_OF_CHAIN);
    let fat = sector_mut_with_size(&mut file, V4_SECTOR_SIZE, 2);
    put_u32(fat, 30 * 4, DIFAT_SECTOR);
    put_u32(fat, 94 * 4, DIFAT_SECTOR);
    // The first directory sector is available before its late continuation.
    put_u32(fat, 0, DIRECTORY_ID);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 40, 2);
    let directory = sector_mut_with_size(&mut file, V4_SECTOR_SIZE, 0);
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
    with_context(&[], &DecodePolicy::service(), |ctx| {
        let mut state = PrefixProbe::new();
        for end in [
            PREFIX_STEP,
            2 * PREFIX_STEP,
            3 * PREFIX_STEP,
            4 * PREFIX_STEP,
            5 * PREFIX_STEP,
            6 * PREFIX_STEP,
        ] {
            assert!(matches!(
                state
                    .advance(ctx, View::over_retained(&file[..end]))
                    .expect("suspended chain"),
                PrefixDirectoryAvailability::Incomplete
            ));
        }
        let PrefixDirectoryAvailability::Ready {
            directory_chain,
            storage,
            ..
        } = state
            .advance(ctx, View::over_retained(&file))
            .expect("complete chains")
        else {
            panic!("chains are ready");
        };
        assert_eq!(directory_chain, [0, DIRECTORY_ID]);
        drop((directory_chain, storage));
    });
    initialize_empty_directory_entries(sector_mut_with_size(
        &mut file,
        V4_SECTOR_SIZE,
        usize::try_from(DIRECTORY_ID).expect("directory id"),
    ));
    assert_eq!(
        probe(&file),
        CompoundPrefixProbe::DirectoryEvidence(Vec::new())
    );
}

#[test]
fn resumable_detection_accounts_for_suspended_fat_during_input_growth() {
    let file = late_directory(false);
    let old_peak = 6 * PREFIX_STEP;
    let suspended_fat_bytes = V4_SECTOR_SIZE;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(old_peak);
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("restarting context");
    let mut source = std::io::Cursor::new(&file);
    let mut bytes = ctx
        .read_input_prefix(&mut source, PREFIX_STEP)
        .expect("initial input");
    loop {
        ctx.charge_work(1, "visit CFB detection prefix")
            .expect("extension visit");
        let decision = probe_directory_availability(&ctx, View::over_retained(&bytes))
            .expect("restarting probe");
        let incomplete = matches!(decision, PrefixDirectoryAvailability::Incomplete);
        drop(decision);
        if !incomplete {
            break;
        }
        let end = bytes.len() + PREFIX_STEP;
        ctx.extend_input_prefix(&mut source, &mut bytes, end)
            .expect("old exact overlap fits");
    }
    assert_eq!(bytes, file);

    for extra in [suspended_fat_bytes - 1, suspended_fat_bytes] {
        policy.limits.max_materialized_bytes =
            cadmpeg_core::decode::u64_from_index(old_peak + extra);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("resumable context");
        let mut source = std::io::Cursor::new(&file);
        let result = read_detection_prefix(&ctx, &mut source, PREFIX_STEP);
        if extra == suspended_fat_bytes {
            assert_eq!(
                result.expect("input overlap and live FAT fit exactly"),
                file
            );
        } else {
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "input prefix storage"
                    && limit.used == cadmpeg_core::decode::u64_from_index(suspended_fat_bytes)
                    && limit.additional == cadmpeg_core::decode::u64_from_index(old_peak)));
        }
    }
}

#[test]
fn resumable_difat_role_waits_for_later_fat_coverage_once() {
    const SECTOR_BYTES: usize = 512;
    let mut file = fixture_v4();
    file.truncate(SECTOR_BYTES);
    file.resize(SECTOR_BYTES * 257, 0);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 30, 9);
    put_u32(&mut file, 40, 0);
    put_u32(&mut file, 44, 2);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 68, 254);
    put_u32(&mut file, 72, 1);
    put_u32(&mut file, 76, 11);
    put_u32(&mut file, 80, 255);
    let directory = sector_mut_with_size(&mut file, SECTOR_BYTES, 0);
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
    let first_fat = sector_mut_with_size(&mut file, SECTOR_BYTES, 11);
    first_fat.fill(0xff);
    put_u32(first_fat, 0, END_OF_CHAIN);
    put_u32(first_fat, 11 * 4, FAT_SECTOR);
    let difat = sector_mut_with_size(&mut file, SECTOR_BYTES, 254);
    difat.fill(0xff);
    put_u32(difat, SECTOR_BYTES - 4, END_OF_CHAIN);
    let second_fat = sector_mut_with_size(&mut file, SECTOR_BYTES, 255);
    second_fat.fill(0xff);
    put_u32(second_fat, 126 * 4, DIFAT_SECTOR);
    put_u32(second_fat, 127 * 4, FAT_SECTOR);
    let node_bytes = 11 * std::mem::size_of::<u32>()
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<usize>());
    // One DIFAT visit and 127 entries; two FAT visits and 256 words;
    // relocation of the first 128 words; five role visits including ends;
    // one directory visit; three node passes for each visited sector set.
    let exact_work = 1 + 127 + 2 + 256 + 128 * 4 + 5 + 1 + 6 * node_bytes;
    let arena = DecodeArena::new();
    for resumed in [false, true] {
        for limit in [exact_work, exact_work - 1] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cadmpeg_core::decode::u64_from_index(limit);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            let mut state = PrefixProbe::new();
            if resumed {
                assert!(matches!(
                    state
                        .advance(&ctx, View::over_retained(&file[..2 * PREFIX_STEP]))
                        .expect("role is not covered"),
                    PrefixDirectoryAvailability::Incomplete
                ));
            }
            let result = state.advance(&ctx, View::over_retained(&file));
            if limit == exact_work {
                let PrefixDirectoryAvailability::Ready {
                    directory_chain,
                    storage,
                    ..
                } = result.expect("exact role continuation")
                else {
                    panic!("directory is ready");
                };
                assert_eq!(directory_chain, [0]);
                drop((directory_chain, storage));
                assert!(
                    matches!(ctx.charge_work(1, "exact DIFAT role pass"), Err(CodecError::ResourceLimit(refusal))
                    if refusal.used == cadmpeg_core::decode::u64_from_index(exact_work))
                );
            } else {
                assert!(matches!(result, Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::WorkUnits
                        && refusal.operation == "CFB probe directory visits"
                        && refusal.used + refusal.additional == cadmpeg_core::decode::u64_from_index(exact_work)));
            }
        }
    }
}
