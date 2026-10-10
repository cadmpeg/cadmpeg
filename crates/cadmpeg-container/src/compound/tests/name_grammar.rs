use super::*;

fn counted_name(units: &[u16]) -> [u8; 128] {
    let mut bytes = [0_u8; 128];
    directory_entry(&mut bytes, 0, "A", 2, NO_STREAM, NO_STREAM, NO_STREAM, END_OF_CHAIN, 0);
    bytes[..64].fill(0);
    for (index, unit) in units.iter().enumerate() {
        put_u16(&mut bytes, index * 2, *unit);
    }
    put_u16(&mut bytes, 64, u16::try_from(units.len() * 2).expect("fixed field"));
    bytes
}

#[test]
fn earlier_counted_name_terminator_rejects_before_text_work_or_storage() {
    for units in [&[0x41, 0, 0x42, 0][..], &[0, 0x42, 0][..]] {
        let bytes = counted_name(units);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = u64::try_from(std::mem::size_of::<DirectorySlot>())
            .expect("one parsed slot");
        with_context(&bytes, &policy, |ctx| {
            let error = parse_directory(ctx, &bytes, CompoundVersion::V3).expect_err("earlier null");
            assert!(matches!(error, CodecError::Malformed(message)
                if message == "CFB directory name has an earlier terminator"));
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
    let bytes = counted_name(&[0x41, 0x58, 0x42, 0]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    with_context(&bytes, &policy, |ctx| {
        let error = parse_directory(ctx, &bytes, CompoundVersion::V3).expect_err("real name decoding work");
        let CodecError::ResourceLimit(first) = error else { panic!("text work admission") };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "decode CFB directory name");
        assert_eq!((first.used, first.additional), (1, 6));
        assert_eq!(ctx.resource_refusal(), Some(first));
    });
}

#[test]
fn directory_name_ignores_padding_beyond_the_declared_terminator() {
    let mut bytes = counted_name(&[0x41, 0]);
    bytes[4..64].fill(0xff);
    put_u16(&mut bytes, 8, 0);
    put_u16(&mut bytes, 12, 0xd800);
    with_context(&bytes, &DecodePolicy::service(), |ctx| {
        let entries = parse_directory(ctx, &bytes, CompoundVersion::V3).expect("irrelevant name padding");
        assert_eq!(entries[0].live().expect("live stream").name.as_str(), "A");
    });
}

#[test]
fn directory_name_accepts_the_full_31_unit_content_width() {
    let mut units = [0x41_u16; 32];
    units[31] = 0;
    let bytes = counted_name(&units);
    with_context(&bytes, &DecodePolicy::service(), |ctx| {
        let entries = parse_directory(ctx, &bytes, CompoundVersion::V3).expect("full legal name field");
        assert_eq!(entries[0].live().expect("live stream").name.as_str(), "A".repeat(31));
    });
}

#[test]
fn name_field_rejection_preserves_original_fused_refusal() {
    let bytes = counted_name(&[0x41, 0, 0x42, 0]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    with_context(&bytes, &policy, |ctx| {
        let CodecError::ResourceLimit(first) = ctx.charge_work(1, "seed counted-name refusal")
            .expect_err("zero work") else { panic!("resource refusal") };
        let error = parse_directory(ctx, &bytes, CompoundVersion::V3).expect_err("sticky refusal");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit == first));
        assert_eq!(ctx.resource_refusal(), Some(first));
    });
}

#[test]
fn alternate_minor_version_preserves_full_and_prefix_recovery() {
    for mut file in [fixture(), fixture_v4()] {
        let expected = probe(&file);
        for minor in [0, 0x003e, 0x1234] {
            put_u16(&mut file, 24, minor);
            assert_eq!(probe(&file), expected);
            with_context(&file, &DecodePolicy::service(), |ctx| {
                let snapshot = CompoundSnapshot::new(ctx, cadmpeg_core::decode::View::over_retained(&file))
                    .expect("minor version is advisory");
                let path = if snapshot.major_version() == 3 { "Store/Large" } else { "Wide" };
                let stream = snapshot.stream(ctx, path).expect("lookup").expect("regular stream");
                assert_eq!(snapshot.open(ctx, stream).expect("unchanged payload").window().len(), 4096);
            });
        }
        put_u16(&mut file, 28, 0);
        assert!(matches!(probe(&file), CompoundPrefixProbe::Malformed(_)));
        with_context(&file, &DecodePolicy::service(), |ctx| {
            let error = CompoundSnapshot::new(ctx, cadmpeg_core::decode::View::over_retained(&file))
                .expect_err("mandatory byte order still rejects");
            assert!(matches!(error, CodecError::Malformed(message)
                if message == "invalid CFB header identity or byte order"));
        });
    }
}


#[test]
fn counted_name_width_needs_no_decoded_text_rescan() {
    for name in ["A".repeat(31), format!("{}A", "😀".repeat(15))] {
        let mut units = name.encode_utf16().collect::<Vec<_>>();
        assert_eq!(units.len(), 31);
        units.push(0);
        let bytes = counted_name(&units);
        // One record, two strict 62-byte UTF-16 passes, exact UTF-8 output
        // copying and the character gate including its end probe.
        let character_visits = name.chars().count() + 1;
        let exact_work = 1 + 2 * 62 + name.len() + character_visits;
        for limit in [exact_work, exact_work - 1] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::try_from(limit).expect("name work");
            with_context(&bytes, &policy, |ctx| {
                let result = parse_directory(ctx, &bytes, CompoundVersion::V3);
                if limit == exact_work {
                    let entries = result.expect("fixed field already proves the width");
                    assert_eq!(entries[0].live().expect("live name").name.as_str(), name);
                    assert_eq!(ctx.resource_refusal(), None);
                } else {
                    let error = result.expect_err("the last character-gate end probe needs admission");
                    let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
                    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(refusal.operation, "check CFB directory name characters");
                    assert_eq!((refusal.used, refusal.additional),
                        (u64::try_from(exact_work - 1).expect("prior name work"), 1));
                    assert_eq!(ctx.resource_refusal(), Some(refusal));
                }
            });
        }
    }
}

#[test]
fn counted_name_rejects_overwidth_before_text_decode() {
    let mut bytes = counted_name(&[0x41, 0]);
    put_u16(&mut bytes, 64, 66);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    with_context(&bytes, &policy, |ctx| {
        let error = parse_directory(ctx, &bytes, CompoundVersion::V3)
            .expect_err("declared content cannot exceed the fixed field");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "invalid CFB directory name length or terminator"));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn directory_name_character_gate_stops_at_the_first_forbidden_scalar() {
    let name = format!("/{}", "A".repeat(30));
    let mut units = name.encode_utf16().collect::<Vec<_>>();
    units.push(0);
    let bytes = counted_name(&units);
    let decode_work = 1 + 2 * 62 + name.len();
    for limit in [decode_work + 1, decode_work] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(limit).expect("name work");
        with_context(&bytes, &policy, |ctx| {
            let error = parse_directory(ctx, &bytes, CompoundVersion::V3)
                .expect_err("forbidden path scalar");
            if limit == decode_work + 1 {
                assert!(matches!(error, CodecError::Malformed(message)
                    if message == "CFB directory name contains a forbidden character or exceeds 31 UTF-16 units"));
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let CodecError::ResourceLimit(refusal) = error else { panic!("resource refusal") };
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(refusal.operation, "check CFB directory name characters");
                assert_eq!((refusal.used, refusal.additional),
                    (u64::try_from(decode_work).expect("prior name work"), 1));
                let repeated = parse_directory(ctx, &bytes, CompoundVersion::V3)
                    .expect_err("original refusal survives repeated parsing");
                assert!(matches!(repeated, CodecError::ResourceLimit(limit) if limit == refusal));
                assert_eq!(ctx.resource_refusal(), Some(refusal));
            }
        });
    }
}


#[test]
fn sibling_comparison_admits_only_reached_unit_pairs() {
    let same = "A".repeat(31);
    let later = "B".repeat(31);
    for (left, right, expected, exact_work) in [
        (same.as_str(), later.as_str(), Ordering::Less, 62_u64 + 1),
        (same.as_str(), same.as_str(), Ordering::Equal, 62 + 31 + 1),
        ("😀", "😁", Ordering::Less, 8 + 2),
        ("ᾠ", "ᾨ", Ordering::Equal, 6 + 1 + 1),
    ] {
        for limit in [exact_work, exact_work - 1] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            with_context(&[], &policy, |ctx| {
                let result = cfb_name_cmp(ctx, left, right);
                if limit == exact_work {
                    assert_eq!(result.expect("reached comparison pairs"), expected);
                    assert_eq!(ctx.resource_refusal(), None);
                    let CodecError::ResourceLimit(refusal) = ctx.charge_work(1, "after CFB comparison")
                        .expect_err("exact work exhausted") else { panic!("resource refusal") };
                    assert_eq!((refusal.used, refusal.additional), (exact_work, 1));
                } else {
                    let CodecError::ResourceLimit(refusal) = result.expect_err("last reached pair or end probe")
                        else { panic!("resource refusal") };
                    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(refusal.operation, "compare CFB sibling names");
                    assert_eq!((refusal.used, refusal.additional), (exact_work - 1, 1));
                    let repeated = cfb_name_cmp(ctx, "", "").expect_err("empty comparison preserves fuse");
                    assert!(matches!(repeated, CodecError::ResourceLimit(limit) if limit == refusal));
                    assert_eq!(ctx.resource_refusal(), Some(refusal));
                }
            });
        }
    }
}

#[test]
fn unequal_name_lengths_skip_the_unit_comparison() {
    let mut policy = DecodePolicy::service();
    // Both complete ASCII length scans execute; no unit pair is compared.
    policy.limits.max_work_units = 3;
    with_context(&[], &policy, |ctx| {
        assert_eq!(cfb_name_cmp(ctx, "A", "BB").expect("length ordering"), Ordering::Less);
        assert_eq!(ctx.resource_refusal(), None);
        let CodecError::ResourceLimit(refusal) = ctx.charge_work(1, "after CFB length ordering")
            .expect_err("exact length work exhausted") else { panic!("resource refusal") };
        assert_eq!((refusal.used, refusal.additional), (3, 1));
    });
}


#[test]
fn path_normalization_does_not_introduce_a_component_separator() {
    for unit in 0..=u16::MAX {
        if unit != u16::from(b'/') {
            assert_ne!(cfb_upper_unit(unit), u16::from(b'/'), "unit {unit:04x}");
        }
    }
}

#[test]
fn path_key_admits_reached_scalars_and_end_probe() {
    for limit in [4, 3] {
        let mut policy = DecodePolicy::service();
        // Three scalars and the end probe; the surrogate pair is fixed scalar
        // encoding. Initial four-slot vector capacities require no relocation.
        policy.limits.max_work_units = limit;
        with_context(&[], &policy, |ctx| {
            let result = path_key(ctx, "A/😀");
            if limit == 4 {
                assert_eq!(result.expect("exact scalar work"), vec![vec![0x41], vec![0xd83d, 0xde00]]);
                assert_eq!(ctx.resource_refusal(), None);
                let CodecError::ResourceLimit(refusal) = ctx.charge_work(1, "after CFB path key")
                    .expect_err("exact work exhausted") else { panic!("resource refusal") };
                assert_eq!((refusal.used, refusal.additional), (4, 1));
            } else {
                let CodecError::ResourceLimit(refusal) = result.expect_err("actual end probe")
                    else { panic!("resource refusal") };
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(refusal.operation, "scan CFB path key");
                assert_eq!((refusal.used, refusal.additional), (3, 1));
                let repeated = path_key(ctx, "").expect_err("empty key preserves refusal");
                assert!(matches!(repeated, CodecError::ResourceLimit(limit) if limit == refusal));
            }
        });
    }
}

#[test]
fn path_key_stops_before_unvisited_scalars_after_item_refusal() {
    let path = format!("A{}", "B".repeat(31));
    for work in [1, 0] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_collection_items = 0;
        with_context(&[], &policy, |ctx| {
            let CodecError::ResourceLimit(refusal) = path_key(ctx, &path)
                .expect_err("first reached item or step refuses") else { panic!("resource refusal") };
            if work == 1 {
                assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
                assert_eq!(refusal.operation, "CFB path key units");
            } else {
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(refusal.operation, "scan CFB path key");
            }
            assert_eq!((refusal.used, refusal.additional), (0, 1));
            assert_eq!(ctx.resource_refusal(), Some(refusal));
        });
    }
}
