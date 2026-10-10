use super::*;

#[test]
fn malformed_fixed_header_diagnostic_is_free() {
    let mut file = fixture();
    put_u16(&mut file, 30, 8);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
        .expect("fixture root fits the input limit");
    let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
        .expect("fixed header checks and diagnostics use no work");
    assert_eq!(
        probe,
        CompoundPrefixProbe::Malformed("invalid CFB sector layout".into())
    );
    drop(storage);
}

#[test]
fn malformed_directory_display_uses_only_structural_work() {
    let mut file = fixture();
    initialize_empty_directory_entries(sector_mut(&mut file, 0));
    let id_node_bytes = 11 * std::mem::size_of::<u32>()
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<usize>());
    // Source steps: one FAT-id visit, 128 FAT-word visits, three role-check
    // visits (one value and end probe, then the empty DIFAT-set end probe),
    // one directory-chain visit with three first-node passes and four
    // borrowed records. No structural sector copy remains.
    let record_visits = SECTOR_SIZE / 128;
    let work_units = 1
        + SECTOR_SIZE / 4
        + 3
        + 1
        + 3 * id_node_bytes
        + record_visits;
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units =
        u64::try_from(work_units).expect("structural work total fits u64");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
        .expect("fixture root fits the input limit");
    let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
        .expect("fixed CodecError display adds no input-dependent work");
    assert_eq!(
        probe,
        CompoundPrefixProbe::Malformed(
            "malformed container: invalid CFB root directory entry".into()
        )
    );
    drop(storage);

    policy.limits.max_work_units =
        u64::try_from(work_units - 1).expect("one-less structural work fits u64");
    let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
        .expect("fixture root fits the input limit");
    let error = CompoundPrefixProbe::inspect_with_context(&ctx, root)
        .expect_err("one fewer unit refuses the last record before display");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "visit CFB directory records"
            && limit.used == u64::try_from(work_units - 1)
                .expect("pre-final-record work fits u64")
            && limit.additional == 1));
}
