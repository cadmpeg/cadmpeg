use super::*;
use crate::compound::{parse_directory_records, StructuralRecords};
use cadmpeg_core::decode::View;

#[test]
fn borrowed_free_directory_records_need_only_slot_storage_and_reached_work() {
    for sector_size in [512, 4096] {
        let mut bytes = vec![0_u8; 2 * sector_size];
        initialize_empty_directory_entries(&mut bytes[sector_size..]);
        let first = 0;
        let count = sector_size / directory_layout::LEN;
        let slot_bytes = count * std::mem::size_of::<DirectorySlot>();
        for work in [count, count - 1] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = u64::try_from(slot_bytes).expect("slot bytes");
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = u64::try_from(count).expect("record slots");
            policy.limits.max_work_units = u64::try_from(work).expect("record visits");
            with_context(&bytes, &policy, |ctx| {
                let records = StructuralRecords::new(ctx, &bytes, sector_size, 1, Some((&first, &[])))
                    .expect("fixed full sector borrows without work or storage");
                let result = ctx.with_scoped_storage("borrowed directory slots", || {
                    parse_directory_records(ctx, &records, CompoundVersion::V3)
                });
                if work == count {
                    let (entries, storage) = result.expect("only slots are materialized");
                    assert_eq!(entries.len(), count);
                    assert!(entries.iter().all(|entry| matches!(entry, DirectorySlot::Free)));
                    let error = ctx.reserve_scoped(1, "after borrowed directory slots")
                        .expect_err("exact slot backing stays live");
                    assert!(matches!(error, CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::MaterializedBytes
                            && limit.used == u64::try_from(slot_bytes).expect("live slots")
                            && limit.additional == 1));
                    drop((entries, storage));
                } else {
                    let error = result.expect_err("last free record needs its own visit");
                    let CodecError::ResourceLimit(first) = error else { panic!("record refusal") };
                    assert_eq!(first.operation, "visit CFB directory records");
                    assert_eq!((first.used, first.additional),
                        (u64::try_from(count - 1).expect("prior record visits"), 1));
                    let repeated = match StructuralRecords::<128>::new(ctx, &bytes, sector_size, 1, None) {
                        Ok(_) => panic!("empty records preserve original refusal"), Err(error) => error,
                    };
                    assert!(matches!(repeated, CodecError::ResourceLimit(limit) if limit == first));
                    assert_eq!(ctx.resource_refusal(), Some(first));
                }
            });
        }
    }
}

#[test]
fn structural_extent_gate_precedes_record_grammar_and_slot_allocation() {
    let mut bytes = [0_u8; 4 * SECTOR_SIZE];
    initialize_empty_directory_entries(&mut bytes[SECTOR_SIZE..]);
    bytes[SECTOR_SIZE + directory_layout::OBJECT_TYPE] = 3;
    let first = 0;
    let rest = [2];
    let partial = &bytes[..3 * SECTOR_SIZE + 37];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    with_context(partial, &policy, |ctx| {
        let error = match StructuralRecords::<128>::new(ctx, partial, SECTOR_SIZE, 3, Some((&first, &rest))) {
            Ok(_) => panic!("later structural sector is partial"), Err(error) => error,
        };
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "CFB structural sector is truncated"));
        assert_eq!(ctx.resource_refusal(), None);
    });
    with_context(&bytes, &DecodePolicy::service(), |ctx| {
        let records = StructuralRecords::new(ctx, &bytes, SECTOR_SIZE, 3, Some((&first, &rest)))
            .expect("complete extents");
        let error = parse_directory_records(ctx, &records, CompoundVersion::V3)
            .expect_err("the first actual record type is malformed");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "invalid CFB directory object type"));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn directory_record_ordinals_follow_nonadjacent_chain_order() {
    let mut bytes = [0_u8; 4 * SECTOR_SIZE];
    initialize_empty_directory_entries(&mut bytes[SECTOR_SIZE..]);
    directory_entry(&mut bytes[3 * SECTOR_SIZE..], 3, "Last", 2,
        NO_STREAM, NO_STREAM, NO_STREAM, END_OF_CHAIN, 0);
    directory_entry(&mut bytes[SECTOR_SIZE..2 * SECTOR_SIZE], 0, "Next", 2,
        NO_STREAM, NO_STREAM, NO_STREAM, END_OF_CHAIN, 0);
    let first = 2;
    let rest = [0];
    with_context(&bytes, &DecodePolicy::service(), |ctx| {
        let records = StructuralRecords::new(ctx, &bytes, SECTOR_SIZE, 3, Some((&first, &rest)))
            .expect("complete nonadjacent sectors");
        let directory = parse_directory_records(ctx, &records, CompoundVersion::V3)
            .expect("shared directory grammar");
        assert_eq!(directory.len(), 8);
        assert_eq!(directory[3].live().expect("last first-sector record").name.as_str(), "Last");
        assert_eq!(directory[4].live().expect("first next-sector record").name.as_str(), "Next");
    });
}

#[test]
fn fragmented_directory_sectors_preserve_full_and_prefix_identity() {
    let mut file = fixture();
    file.resize(file.len() + SECTOR_SIZE, 0);
    initialize_empty_directory_entries(sector_mut(&mut file, 12));
    put_u32(sector_mut(&mut file, 11), 0, 12);
    put_u32(sector_mut(&mut file, 11), 12 * 4, END_OF_CHAIN);
    for prefix in [0, 17] {
        let mut bytes = vec![0_u8; prefix];
        bytes.extend_from_slice(&file);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("source root");
        let root = root.child(prefix, bytes.len()).expect("CFB child");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("fragmented directory");
        assert_eq!(snapshot.entries().iter().map(CompoundEntry::path).collect::<Vec<_>>(),
            vec!["Small", "Store", "Store/Large"]);
        let small = snapshot.stream(&ctx, "Small").expect("lookup").expect("small");
        let large = snapshot.stream(&ctx, "Store/Large").expect("lookup").expect("large");
        assert_eq!(small.id().directory_id(), 1);
        assert_eq!(large.id().directory_id(), 3);
        assert_eq!(snapshot.open(&ctx, small).expect("mini payload").window(), b"small");
        assert_eq!(snapshot.open(&ctx, large).expect("regular payload").window(), &[0x5a; 4096]);
        let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root).expect("same prefix grammar");
        assert_eq!(probe, CompoundPrefixProbe::DirectoryEvidence(vec!["Small".into(), "Store".into(), "Store/Large".into()]));
        drop(storage);
    }
}

#[test]
fn fragmented_mini_fat_words_preserve_chain_order_and_payload() {
    let mut file = fixture();
    file.resize(file.len() + SECTOR_SIZE, 0xff);
    put_u32(&mut file, 64, 2);
    put_u32(sector_mut(&mut file, 11), 10 * 4, 12);
    put_u32(sector_mut(&mut file, 11), 12 * 4, END_OF_CHAIN);
    let state = with_context(&file, &DecodePolicy::service(), |ctx| parse_state(ctx, &file))
        .expect("two nonadjacent mini-FAT sectors");
    assert_eq!(state.mini_fat.len(), 2 * SECTOR_SIZE / 4);
    assert_eq!(state.mini_fat[0], END_OF_CHAIN);
    assert!(state.mini_fat[1..].iter().all(|word| *word == FREE_SECTOR));
    with_context(&file, &DecodePolicy::service(), |ctx| {
        let snapshot = CompoundSnapshot::new(ctx, View::over_retained(&file)).expect("all extra mini slots stay free");
        let stream = snapshot.stream(ctx, "Small").expect("lookup").expect("stream");
        assert_eq!(stream.id().directory_id(), 1);
        assert_eq!(snapshot.open(ctx, stream).expect("unchanged payload").window(), b"small");
    });
}
