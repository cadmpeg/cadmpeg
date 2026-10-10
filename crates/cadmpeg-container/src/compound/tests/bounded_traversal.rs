use super::*;
use crate::compound::{probe_directory_availability, DirectoryKind, PrefixDirectoryAvailability};

fn id_node_bytes() -> usize {
    11 * (std::mem::size_of::<u32>() + std::mem::size_of::<()>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<u32>()
            .max(std::mem::align_of::<()>())
            .max(std::mem::align_of::<usize>())
}

fn empty_state(fat: Vec<u32>, sector_count: usize) -> CompoundState {
    let mut directory = [0_u8; 128];
    directory_entry(
        &mut directory, 0, "Root Entry", 5, NO_STREAM, NO_STREAM, NO_STREAM,
        END_OF_CHAIN, 0,
    );
    let directory = with_context(&directory, &DecodePolicy::service(), |ctx| {
        parse_directory(ctx, &directory, CompoundVersion::V3).expect("root directory")
    });
    CompoundState {
        version: CompoundVersion::V3,
        sector_count,
        fat,
        mini_fat: Vec::new(),
        directory,
        directory_chain: None,
        mini_fat_chain: None,
        root_mini_chain: None,
        fat_sectors: std::collections::BTreeSet::new(),
        difat_sectors: std::collections::BTreeSet::new(),
        range_lock_sector: None,
    }
}

fn many_fat_ids(first_available: bool) -> Vec<u8> {
    let mut file = fixture();
    put_u32(&mut file, 44, 109);
    for index in 0..109 {
        let id = if first_available && index == 0 {
            11
        } else {
            1_000 + u32::try_from(index).expect("header index fits u32")
        };
        put_u32(&mut file, 76 + index * 4, id);
    }
    file
}

#[test]
fn fat_ownership_admits_only_physical_sector_entries() {
    let mut fat = vec![FREE_SECTOR; SECTOR_SIZE / std::mem::size_of::<u32>()];
    fat[0] = END_OF_CHAIN;
    let mut state = empty_state(fat, 1);
    state.range_lock_sector = Some(0);
    let insertion_work = 3 * id_node_bytes();
    let physical_visits = 1;
    let membership_work = std::mem::size_of::<u32>();
    let exact_work = insertion_work + physical_visits + membership_work;
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from(exact_work).expect("owner work fits u64");
    with_context(&[], &policy, |ctx| {
        state.validate_sector_ownership(ctx, &[]).expect("unused FAT padding is not visited");
        assert_eq!(ctx.resource_refusal(), None);
    });

    policy.limits.max_work_units -= 1;
    with_context(&[], &policy, |ctx| {
        let error = state.validate_sector_ownership(ctx, &[]).expect_err("last comparison refuses");
        let CodecError::ResourceLimit(limit) = error else { panic!("resource refusal") };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "check CFB FAT ownership");
        assert_eq!(limit.used, u64::try_from(insertion_work + physical_visits).expect("used work"));
        assert_eq!(limit.additional, u64::try_from(membership_work).expect("comparison work"));
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });

    // A second physical allocation still needs an owner; padding is not an excuse.
    state.sector_count = 2;
    state.fat[1] = END_OF_CHAIN;
    let invalid_work = insertion_work + 2 + 2 * membership_work;
    policy.limits.max_work_units = u64::try_from(invalid_work).expect("invalid owner work");
    with_context(&[], &policy, |ctx| {
        let error = state.validate_sector_ownership(ctx, &[]).expect_err("second sector is unowned");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "unowned CFB sector is not marked free"));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn empty_hierarchy_build_skips_the_fixed_root_record() {
    let state = empty_state(Vec::new(), 0);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    with_context(&[], &policy, |ctx| {
        assert!(build_entries(ctx, &state, 1).expect("empty hierarchy uses no work or storage").is_empty());
        assert_eq!(ctx.resource_refusal(), None);
        let CodecError::ResourceLimit(first) = ctx.charge_work(1, "seed root reachability refusal")
            .expect_err("zero work fuses the context") else { panic!("resource refusal") };
        let error = build_entries(ctx, &state, 1).expect_err("empty tail preserves saved refusal");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit == first));
        assert_eq!(ctx.resource_refusal(), Some(first));
    });
}

#[test]
fn empty_prefix_evidence_does_not_queue_absent_links() {
    let mut file = fixture();
    let directory = sector_mut(&mut file, 0);
    initialize_empty_directory_entries(directory);
    directory_entry(
        directory, 0, "Root Entry", 5, NO_STREAM, NO_STREAM, NO_STREAM,
        END_OF_CHAIN, 0,
    );
    let fat_visits = 1;
    let fat_words = SECTOR_SIZE / std::mem::size_of::<u32>();
    let fat_role_probes = 2;
    let empty_difat_probe = 1;
    let directory_visit = 1;
    let directory_insertion = 3 * id_node_bytes();
    let directory_records = SECTOR_SIZE / 128;
    let root_name = "Root Entry";
    let utf16_bytes = 2 * root_name.len();
    let name_work = 2 * utf16_bytes + root_name.len() + root_name.len() + 1;
    let tail_records = directory_records - 1;
    let root_validation_probes = tail_records + 1;
    let exact_work = fat_visits + fat_words + fat_role_probes + empty_difat_probe
        + directory_visit + directory_insertion + directory_records
        + name_work + root_validation_probes + tail_records;
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from(exact_work).expect("prefix work fits u64");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("prefix root");
    let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
        .expect("only real table and tail operations consume work");
    assert_eq!(probe, CompoundPrefixProbe::DirectoryEvidence(Vec::new()));
    drop(storage);
    assert_eq!(ctx.resource_refusal(), None);

    policy.limits.max_work_units -= 1;
    let limited_arena = DecodeArena::new();
    let (limited_ctx, root) = DecodeContext::from_root_bytes(&file, &limited_arena, &policy)
        .expect("limited prefix root");
    let error = CompoundPrefixProbe::inspect_with_context(&limited_ctx, root)
        .expect_err("the real tail traversal still requires admission");
    let CodecError::ResourceLimit(limit) = error else { panic!("resource refusal") };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "visit CFB probe reachability");
    assert_eq!(limit.used, u64::try_from(exact_work - 1).expect("prior work"));
    assert_eq!(limit.additional, 1);
    assert_eq!(limited_ctx.resource_refusal(), Some(limit));
}

#[test]
fn unavailable_first_fat_id_stops_before_unvisited_header_ids() {
    let file = many_fat_ids(false);
    // Discovered IDs grow at capacities 4, 8, 16, 32 and 64. Core admits
    // their old backing bytes before relocation. Then one FAT-id visit,
    // two empty role-search end probes and one directory visit execute.
    let relocation_work = (4 + 8 + 16 + 32 + 64) * std::mem::size_of::<u32>();
    let exact_work = u64::try_from(relocation_work + 1 + 1 + 1 + 1).expect("prefix work");
    for limit in [exact_work, exact_work - 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("prefix root");
        let result = probe_directory_availability(&ctx, root);
        if limit == exact_work {
            assert!(matches!(result.expect("only the first FAT id is visited"),
                PrefixDirectoryAvailability::Incomplete));
            assert_eq!(ctx.resource_refusal(), None);
        } else {
            let error = match result { Ok(_) => panic!("directory visit must refuse"), Err(error) => error };
            let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!(refusal.operation, "visit CFB probe directory chain");
            assert_eq!((refusal.used, refusal.additional), (exact_work - 1, 1));
            assert_eq!(ctx.resource_refusal(), Some(refusal));
        }
    }
}

#[test]
fn leading_fat_coverage_stops_at_the_first_unavailable_id() {
    let file = many_fat_ids(true);
    let relocation_work = (4 + 8 + 16 + 32 + 64) * std::mem::size_of::<u32>();
    let visits_before_directory_insert = relocation_work
        + 2 + SECTOR_SIZE / std::mem::size_of::<u32>() + 2 + 1 + 1;
    let insertion_work = 3 * id_node_bytes();
    let exact_work = visits_before_directory_insert + insertion_work;
    for limit in [exact_work, exact_work - 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(limit).expect("prefix work fits u64");
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("prefix root");
        let result = probe_directory_availability(&ctx, root);
        if limit == exact_work {
            let available = result.expect("leading FAT covers the reachable directory");
            let PrefixDirectoryAvailability::Ready { directory_chain, storage, .. } = available
                else { panic!("available leading coverage must remain usable") };
            assert_eq!(directory_chain, vec![0]);
            drop((directory_chain, storage));
            assert_eq!(ctx.resource_refusal(), None);
        } else {
            let error = match result { Ok(_) => panic!("directory insertion must refuse"), Err(error) => error };
            let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!(refusal.operation, "CFB probe directory visits");
            assert_eq!(refusal.used, u64::try_from(visits_before_directory_insert).expect("prior visits"));
            assert_eq!(refusal.additional, u64::try_from(insertion_work).expect("insertion work"));
            assert_eq!(ctx.resource_refusal(), Some(refusal));
        }
    }
}

#[test]
fn directory_parse_admits_only_records_reached_before_malformed_type() {
    let mut directory = [0_u8; SECTOR_SIZE];
    initialize_empty_directory_entries(&mut directory);
    directory[directory_layout::OBJECT_TYPE] = 3;
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    with_context(&directory, &policy, |ctx| {
        let error = parse_directory(ctx, &directory, CompoundVersion::V3)
            .expect_err("the first reached type is malformed");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "invalid CFB directory object type"));
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units = 0;
    with_context(&directory, &policy, |ctx| {
        let error = parse_directory(ctx, &directory, CompoundVersion::V3)
            .expect_err("admission precedes the first type read");
        let CodecError::ResourceLimit(first) = error else { panic!("resource refusal") };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "visit CFB directory records");
        assert_eq!((first.used, first.additional), (0, 1));
        let repeated = parse_directory(ctx, &directory, CompoundVersion::V3)
            .expect_err("the original refusal survives a repeated parse");
        assert!(matches!(repeated, CodecError::ResourceLimit(limit) if limit == first));
        assert_eq!(ctx.resource_refusal(), Some(first));
    });

    directory[directory_layout::OBJECT_TYPE] = 0;
    let records = SECTOR_SIZE / directory_layout::LEN;
    for limit in [records, records - 1] {
        policy.limits.max_work_units = u64::try_from(limit).expect("record work fits u64");
        with_context(&directory, &policy, |ctx| {
            let result = parse_directory(ctx, &directory, CompoundVersion::V3);
            if limit == records {
                let parsed = result.expect("every free record is admitted and checked");
                assert_eq!(parsed.len(), records);
                assert!(parsed.iter().all(|entry| matches!(entry, DirectorySlot::Free)));
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let error = result.expect_err("the final free record still needs admission");
                let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(refusal.operation, "visit CFB directory records");
                assert_eq!((refusal.used, refusal.additional),
                    (u64::try_from(records - 1).expect("prior record work"), 1));
                assert_eq!(ctx.resource_refusal(), Some(refusal));
            }
        });
    }
}

#[test]
fn directory_reachability_stops_at_the_first_unreachable_live_record() {
    let mut state = empty_state(Vec::new(), 0);
    let mut unreachable = state.directory[0].live().expect("root entry").clone();
    unreachable.kind = DirectoryKind::Stream;
    state.directory.push(DirectorySlot::Live(unreachable));
    state.directory.push(DirectorySlot::Free);
    state.directory.push(DirectorySlot::Free);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    with_context(&[], &policy, |ctx| {
        let error = build_entries(ctx, &state, 1).expect_err("the first tail record is unreachable");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "CFB directory contains an unreachable live entry"));
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units = 0;
    with_context(&[], &policy, |ctx| {
        let error = build_entries(ctx, &state, 1).expect_err("reachability checks admit before visiting");
        let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "check CFB directory reachability");
        assert_eq!((refusal.used, refusal.additional), (0, 1));
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
    state.directory[1] = DirectorySlot::Free;
    let tail_records = state.directory.len() - 1;
    for limit in [tail_records, tail_records - 1] {
        policy.limits.max_work_units = u64::try_from(limit).expect("tail work fits u64");
        with_context(&[], &policy, |ctx| {
            let result = build_entries(ctx, &state, 1);
            if limit == tail_records {
                assert!(result.expect("all free tail records are reached").is_empty());
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let error = result.expect_err("the last free tail record needs admission");
                let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
                assert_eq!(refusal.operation, "check CFB directory reachability");
                assert_eq!((refusal.used, refusal.additional),
                    (u64::try_from(tail_records - 1).expect("prior tail work"), 1));
                assert_eq!(ctx.resource_refusal(), Some(refusal));
            }
        });
    }
}

#[test]
fn prefix_reachability_stops_before_unvisited_free_records() {
    let mut file = fixture();
    let directory = sector_mut(&mut file, 0);
    initialize_empty_directory_entries(directory);
    directory_entry(directory, 0, "Root Entry", 5, NO_STREAM, NO_STREAM,
        NO_STREAM, END_OF_CHAIN, 0);
    directory_entry(directory, 1, "A", 2, NO_STREAM, NO_STREAM,
        NO_STREAM, END_OF_CHAIN, 0);
    // One FAT id, every FAT word, two FAT-role probes and one empty
    // DIFAT probe, one directory visit, three set-node passes precede parsing of borrowed sectors. UTF-16 decode visits its bytes twice;
    // output-byte copying and forbidden-character visits follow. Root
    // validation visits all tail records and its end probe. The first
    // reachability visit then rejects before the two later free records.
    let directory_records = SECTOR_SIZE / directory_layout::LEN;
    let root_name_bytes = "Root Entry".len();
    let stream_name_bytes = "A".len();
    let name_work = |bytes: usize| 2 * (2 * bytes) + 2 * bytes + 1;
    let prior_work = 1 + SECTOR_SIZE / 4 + 2 + 1 + 1
        + 3 * id_node_bytes() + directory_records
        + name_work(root_name_bytes) + name_work(stream_name_bytes)
        + directory_records;
    let exact_work = prior_work + 1;
    for limit in [exact_work, exact_work - 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(limit).expect("prefix work fits u64");
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy).expect("prefix root");
        let result = CompoundPrefixProbe::inspect_with_context(&ctx, root);
        if limit == exact_work {
            let (probe, storage) = result.expect("only the reached tail slot needs admission");
            assert_eq!(probe, CompoundPrefixProbe::Malformed(
                "CFB directory contains an unreachable live entry".into()));
            drop(storage);
            assert_eq!(ctx.resource_refusal(), None);
        } else {
            let error = result.expect_err("admission precedes the unreachable-slot check");
            let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!(refusal.operation, "visit CFB probe reachability");
            assert_eq!((refusal.used, refusal.additional),
                (u64::try_from(prior_work).expect("prior prefix work"), 1));
            assert_eq!(ctx.resource_refusal(), Some(refusal));
        }
    }
}


#[test]
fn structural_records_reject_a_partial_fixed_first_before_tail_work() {
    let bytes = [0xab_u8; 4 * SECTOR_SIZE];
    let first = 2;
    let rest = [0, 1];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    with_context(&bytes[..3 * SECTOR_SIZE + 37], &policy, |ctx| {
        let error = match crate::compound::StructuralRecords::<128>::new(ctx,
            &bytes[..3 * SECTOR_SIZE + 37], SECTOR_SIZE, 3, Some((&first, &rest))) {
            Ok(_) => panic!("partial fixed first sector"), Err(error) => error,
        };
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "CFB structural sector is truncated"));
        assert_eq!(ctx.resource_refusal(), None);
    });
    // The complete chain validates two reached variable-sector extents.
    // Borrowing the three sectors performs no byte copy or output allocation.
    for work in [2, 1] {
        policy.limits.max_work_units = work;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        with_context(&bytes, &policy, |ctx| {
            let result = crate::compound::StructuralRecords::<128>::new(ctx, &bytes,
                SECTOR_SIZE, 3, Some((&first, &rest)));
            if work == 2 {
                let records = result.unwrap_or_else(|error| panic!("complete chain: {error}"));
                let logical = (0..records.len()).flat_map(|index| records.get(index).expect("record").iter().copied()).collect::<Vec<_>>();
                assert_eq!(logical, vec![0xab; 3 * SECTOR_SIZE]);
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let error = match result { Ok(_) => panic!("last extent visit needs admission"), Err(error) => error };
                let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
                assert_eq!(refusal.operation, "walk CFB structural sectors");
                assert_eq!((refusal.used, refusal.additional), (1, 1));
                assert_eq!(ctx.resource_refusal(), Some(refusal));
            }
        });
    }
}


#[test]
fn allocation_ownership_stops_at_the_first_unowned_marker() {
    for mini in [false, true] {
        let mut state = empty_state(Vec::new(), 0);
        if mini {
            state.mini_fat = vec![END_OF_CHAIN, FREE_SECTOR, FREE_SECTOR];
        } else {
            state.fat = vec![END_OF_CHAIN, FREE_SECTOR, FREE_SECTOR];
            state.sector_count = 3;
        }
        for work in [1, 0] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            with_context(&[], &policy, |ctx| {
                let error = state.validate_sector_ownership(ctx, &[])
                    .expect_err("first allocation marker is unowned");
                if work == 1 {
                    let expected = if mini { "unowned CFB mini sector is not marked free" }
                        else { "unowned CFB sector is not marked free" };
                    assert!(matches!(error, CodecError::Malformed(message) if message == expected));
                    assert_eq!(ctx.resource_refusal(), None);
                    let CodecError::ResourceLimit(refusal) = ctx.charge_work(1, "after first unowned marker")
                        .expect_err("one allocation slot was reached") else { panic!("resource refusal") };
                    assert_eq!((refusal.used, refusal.additional), (1, 1));
                } else {
                    let CodecError::ResourceLimit(refusal) = error else { panic!("first allocation visit") };
                    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(refusal.operation, if mini { "visit CFB mini FAT ownership" }
                        else { "visit CFB FAT ownership" });
                    assert_eq!((refusal.used, refusal.additional), (0, 1));
                    assert_eq!(ctx.resource_refusal(), Some(refusal));
                }
            });
        }
    }
}

#[test]
fn full_fat_loading_rejects_a_partial_first_declared_sector() {
    let mut complete = fixture();
    let first_table = sector_mut(&mut complete, 11).to_vec();
    complete.resize(complete.len() + SECTOR_SIZE, 0xff);
    sector_mut(&mut complete, 12).copy_from_slice(&first_table);
    sector_mut(&mut complete, 11).fill(0xff);
    put_u32(sector_mut(&mut complete, 12), 12 * 4, FAT_SECTOR);
    put_u32(&mut complete, 44, 2);
    put_u32(&mut complete, 76, 12);
    put_u32(&mut complete, 80, 11);
    let partial = &complete[..13 * SECTOR_SIZE + 37];
    with_context(partial, &DecodePolicy::service(), |ctx| {
        let error = parse_state(ctx, partial).expect_err("first FAT sector is partial");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "CFB FAT sector is truncated"));
        assert_eq!(ctx.resource_refusal(), None);
    });
    with_context(&complete, &DecodePolicy::service(), |ctx| {
        let snapshot = CompoundSnapshot::new(ctx, cadmpeg_core::decode::View::over_retained(&complete))
            .expect("complete first FAT sector and trailing free table");
        let stream = snapshot.stream(ctx, "Small").expect("lookup").expect("stream");
        assert_eq!(snapshot.open(ctx, stream).expect("unchanged payload").window(), b"small");
        assert_eq!(ctx.resource_refusal(), None);
    });
}
