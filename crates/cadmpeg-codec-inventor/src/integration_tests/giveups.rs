// SPDX-License-Identifier: Apache-2.0
//! Give-ups of whole streams and metadata sections, as a decode reports them.

use cadmpeg_core::decode::{DecodeDepth, InspectOptions};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{record_unreadable_kind, Cause};

use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream, fixture_with_ufrx, paged_instance, primary_envelope_fixture,
    primary_envelope_fixture_with_design, primary_envelope_fixture_with_drawing_tables,
    primary_envelope_fixture_with_kernel, push_lp, BaseEnvelopeRecords, EnvelopeDeclarations,
};
use crate::InventorCodec;
use cadmpeg_test_support::bytes::{put_u16, put_u32};

fn charges(losses: &[LossNote]) -> Vec<&str> {
    losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

/// A design envelope whose design metadata stream is renamed `Mde`, so that
/// its bulk stream `Bdc` and the renamed stream each have no partner.
fn unpaired_envelope() -> Vec<u8> {
    let mut bytes = primary_envelope_fixture_with_design(
        &acis_sphere_kernel_stream(21_800),
        &[([0x71; 16], vec![0_u8; 8])],
    );
    let name = "Mdc"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let found = bytes
        .windows(name.len())
        .enumerate()
        .filter(|(_, window)| *window == name.as_slice())
        .map(|(at, _)| at)
        .collect::<Vec<_>>();
    let [at] = found.as_slice() else {
        panic!("one directory entry names the design metadata stream: {found:?}");
    };
    bytes[at + 4] = b'e';
    bytes
}

/// The primary envelope with a root stream `U+0005Broken` of 64 bytes, the
/// left sibling of `RSeStorage`, whose start sector is outside the mini
/// allocation table, so that its chain breaks.
fn envelope_with_broken_property_set_stream() -> Vec<u8> {
    const DIRECTORY: usize = 512;
    const RSE_STORAGE: usize = 1;
    const ENTRY: usize = 10;
    let mut bytes = primary_envelope_fixture();
    let entry = DIRECTORY + ENTRY * 128;
    assert_eq!(bytes[entry + 66], 0, "directory entry {ENTRY} is unused");
    let name = "\u{5}Broken".encode_utf16().collect::<Vec<_>>();
    for (index, unit) in name.iter().enumerate() {
        put_u16(&mut bytes, entry + index * 2, *unit);
    }
    put_u16(
        &mut bytes,
        entry + 64,
        u16::try_from((name.len() + 1) * 2).expect("the name length fits a u16"),
    );
    bytes[entry + 66] = 2;
    bytes[entry + 67] = 1;
    put_u32(&mut bytes, entry + 116, 0x00ff_fff0);
    put_u32(&mut bytes, entry + 120, 64);
    put_u32(
        &mut bytes,
        DIRECTORY + RSE_STORAGE * 128 + 68,
        u32::try_from(ENTRY).expect("the entry index fits a u32"),
    );
    bytes
}

#[test]
fn a_property_set_stream_that_does_not_open_is_one_unreadable_give_up() {
    let bytes = envelope_with_broken_property_set_stream();
    for depth in [DecodeDepth::Full, DecodeDepth::Container] {
        let mut options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &options)
            .expect("a default decode gives up the stream and continues");
        let losses = &decoded.report().losses;
        let noncanonical = crate::loss::InventorLossCode::ContainerNoncanonical.kind();
        assert!(
            losses.iter().all(|loss| loss.code != noncanonical),
            "{losses:#?}"
        );
        let broken = charges(losses)
            .into_iter()
            .filter(|message| message.contains("Broken"))
            .collect::<Vec<_>>();
        let [charge] = broken.as_slice() else {
            panic!("one property-set give-up at {depth:?}: {losses:#?}");
        };
        assert!(charge.starts_with("part property-set "), "{charge}");

        options.strictness.pinned = true;
        if depth == DecodeDepth::Container {
            InventorCodec
                .decode(&mut std::io::Cursor::new(&bytes), &options)
                .expect("container depth is exempt from pinned floors");
            options.strictness = options.strictness.with_code("record.unreadable");
        }
        match InventorCodec.decode(&mut std::io::Cursor::new(&bytes), &options) {
            Err(DecodeFailure::StrictRejected { rejection }) => {
                assert_eq!(rejection.loss().code, record_unreadable_kind());
                assert!(rejection.loss().message.contains("Broken"));
            }
            other => panic!("strict decode refuses the give-up: {:?}", other.err()),
        }
    }
}

#[test]
fn named_property_set_headers_are_read_at_both_depths() {
    for (size, order, expected) in [
        (27_u32, 0xfffe, "section count"),
        (64, 0x0000, "byte order"),
    ] {
        const DIRECTORY: usize = 512;
        const ENTRY: usize = DIRECTORY + 3 * 128;
        const FAT: usize = 1024;
        const MINI_FAT: usize = 1536;
        const MINI_STREAM: usize = 2048;
        const END_CHAIN: u32 = 0xffff_fffe;
        let mut bytes = fixture_with_ufrx(&[]);
        bytes.truncate(2560);
        put_u32(&mut bytes, 60, 2);
        put_u32(&mut bytes, 64, 1);
        put_u32(&mut bytes, DIRECTORY + 116, 3);
        put_u32(&mut bytes, DIRECTORY + 120, 64);
        bytes[FAT + 8..MINI_FAT].fill(0xff);
        put_u32(&mut bytes, FAT + 8, END_CHAIN);
        put_u32(&mut bytes, FAT + 12, END_CHAIN);
        bytes[MINI_FAT..MINI_STREAM].fill(0xff);
        put_u32(&mut bytes, MINI_FAT, END_CHAIN);
        bytes[ENTRY..ENTRY + 64].fill(0);
        for (index, unit) in "\u{5}Header".encode_utf16().enumerate() {
            put_u16(&mut bytes, ENTRY + index * 2, unit);
        }
        put_u16(&mut bytes, ENTRY + 64, 16);
        put_u32(&mut bytes, ENTRY + 116, 0);
        put_u32(&mut bytes, ENTRY + 120, size);
        put_u16(&mut bytes, MINI_STREAM, order);
        for depth in [DecodeDepth::Full, DecodeDepth::Container] {
            let options = DecodeOptions {
                depth,
                ..DecodeOptions::default()
            };
            let result = InventorCodec
                .decode(&mut std::io::Cursor::new(&bytes), &options)
                .expect("a damaged property set leaves the file readable");
            let losses = result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.message.starts_with("part property-set "))
                .collect::<Vec<_>>();
            let [loss] = losses.as_slice() else {
                panic!("one named property-set loss at {depth:?}: {losses:#?}");
            };
            assert_eq!(loss.code, Cause::Unreadable.kind());
            assert!(loss.message.contains(expected), "{loss:?}");
        }
        let losses = inspect_losses(&bytes, &InspectOptions::default());
        assert_part_loss(&losses, "part property-set ", expected, Cause::Unreadable);
    }
}

fn rename_root_stream(bytes: &mut [u8], name: &str) {
    let entry = 512 + 3 * 128;
    bytes[entry..entry + 64].fill(0);
    for (index, unit) in name.encode_utf16().enumerate() {
        put_u16(bytes, entry + index * 2, unit);
    }
    put_u16(
        bytes,
        entry + 64,
        u16::try_from((name.encode_utf16().count() + 1) * 2)
            .expect("fixture name fits a directory entry"),
    );
}

fn inspect_losses(bytes: &[u8], options: &InspectOptions) -> Vec<LossNote> {
    InventorCodec
        .inspect(&mut std::io::Cursor::new(bytes), options)
        .expect("container remains inspectable")
        .losses
}

fn assert_part_loss<'a>(
    losses: &'a [LossNote],
    part: &str,
    detail: &str,
    cause: Cause,
) -> &'a LossNote {
    let matching = losses
        .iter()
        .filter(|loss| loss.message.starts_with(part) && loss.message.contains(detail))
        .collect::<Vec<_>>();
    let [loss] = matching.as_slice() else {
        panic!("one {part} charge for {detail}: {losses:#?}");
    };
    assert_eq!(loss.code, cause.kind(), "{loss:?}");
    loss
}

fn oversized_property_set(physical_tail: usize) -> Vec<u8> {
    const SECTOR: usize = 512;
    const STREAM_LEN: u32 = 2 * 1024 * 1024 + 8;
    const END_CHAIN: u32 = 0xffff_fffe;
    const FAT_SECTOR: u32 = 0xffff_fffd;

    let mut bytes = fixture_with_ufrx(&[]);
    let sectors = usize::try_from(STREAM_LEN)
        .expect("fixture length fits usize")
        .div_ceil(SECTOR);
    let fat_count = (1 + sectors).div_ceil(127);
    let first_data = fat_count + 1;
    let last_data = first_data + sectors - 1;
    bytes.resize((last_data + 1) * SECTOR + physical_tail, 0);
    put_u32(
        &mut bytes,
        44,
        u32::try_from(fat_count).expect("fixture FAT count fits u32"),
    );
    rename_root_stream(&mut bytes, "\u{5}Oversized");
    let entry = SECTOR + 3 * 128;
    put_u32(
        &mut bytes,
        entry + 116,
        u32::try_from(first_data).expect("fixture sector fits u32"),
    );
    put_u32(&mut bytes, entry + 120, STREAM_LEN);
    let stream = (first_data + 1) * SECTOR;
    put_u16(&mut bytes, stream, 0xfffe);
    put_u32(&mut bytes, stream + 24, 1);
    put_u32(&mut bytes, stream + 44, 48);
    put_u32(&mut bytes, stream + 48, 8);
    for index in 0..fat_count {
        let sector = index + 1;
        put_u32(
            &mut bytes,
            76 + index * 4,
            u32::try_from(sector).expect("fixture FAT sector fits u32"),
        );
        let at = (sector + 1) * SECTOR;
        bytes[at..at + SECTOR].fill(0xff);
        for (slot, id) in (index * 128..=last_data).take(128).enumerate() {
            let link = if id == 0 || id == last_data {
                END_CHAIN
            } else if id < first_data {
                FAT_SECTOR
            } else {
                u32::try_from(id + 1).expect("fixture FAT link fits u32")
            };
            put_u32(&mut bytes, at + slot * 4, link);
        }
    }
    bytes
}

#[test]
fn physical_extent_precedes_the_named_property_set_cap() {
    for (tail, cause, detail) in [
        (8, Cause::Undecoded, "exceeds 2097152 bytes"),
        (4, Cause::Unreadable, "shorter than declared"),
    ] {
        let bytes = oversized_property_set(tail);
        for depth in [DecodeDepth::Full, DecodeDepth::Container] {
            let options = DecodeOptions {
                depth,
                ..DecodeOptions::default()
            };
            let decoded = InventorCodec
                .decode(&mut std::io::Cursor::new(&bytes), &options)
                .expect("a refused property set leaves the document readable");
            assert_part_loss(
                &decoded.report().losses,
                "part property-set ",
                detail,
                cause,
            );
        }
        assert_part_loss(
            &inspect_losses(&bytes, &InspectOptions::default()),
            "part property-set ",
            detail,
            cause,
        );
    }
}

#[test]
fn a_damaged_property_section_keeps_its_readable_sibling() {
    let mut set = vec![0; 112];
    put_u16(&mut set, 0, 0xfffe);
    put_u32(&mut set, 24, 2);
    set[28..44].fill(0x11);
    put_u32(&mut set, 44, 68);
    set[48..64].fill(0x22);
    put_u32(&mut set, 64, 88);
    for (start, size) in [(68, 20), (88, 24)] {
        put_u32(&mut set, start, size);
        put_u32(&mut set, start + 4, 1);
        put_u32(&mut set, start + 8, 2);
        put_u32(&mut set, start + 12, 16);
        put_u32(&mut set, start + 16, 3);
    }
    put_u32(&mut set, 108, 37);
    let mut bytes = fixture_with_ufrx(&set);
    rename_root_stream(&mut bytes, "\u{5}Sections");
    for depth in [DecodeDepth::Full, DecodeDepth::Container] {
        let options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &options)
            .expect("local property damage");
        assert_part_loss(
            &decoded.report().losses,
            "part property-section ",
            "offset=68 length=20",
            Cause::Unreadable,
        );
        let values = decoded
            .ir()
            .native
            .namespace("inventor")
            .expect("the decoded file keeps its Inventor namespace")
            .arena_as::<serde_json::Value>("properties")
            .expect("the surviving property arena");
        let [value] = values.as_slice() else {
            panic!("one property survives: {values:?}")
        };
        assert_eq!(value["scalar_value"], "37");
    }
    assert_part_loss(
        &inspect_losses(&bytes, &InspectOptions::default()),
        "part property-section ",
        "offset=68 length=20",
        Cause::Unreadable,
    );
}

#[test]
fn a_fragmented_property_set_above_available_retention_is_undecoded() {
    const DIRECTORY: usize = 512;
    const FAT: usize = 1024;
    const PROPERTY_ENTRY: usize = DIRECTORY + 3 * 128;
    const FIRST_STREAM_BYTE: usize = 1536;
    const STREAM_BYTES: usize = 32 * 1024;
    const END_CHAIN: u32 = 0xffff_fffe;
    let mut property_set = [0_u8; 56];
    put_u16(&mut property_set, 0, 0xfffe);
    put_u32(&mut property_set, 24, 1);
    put_u32(&mut property_set, 44, 48);
    put_u32(&mut property_set, 48, 8);
    let mut bytes = fixture_with_ufrx(&property_set);
    bytes.resize(FIRST_STREAM_BYTE + STREAM_BYTES, 0);
    let name = "\u{5}Budget".encode_utf16().collect::<Vec<_>>();
    bytes[PROPERTY_ENTRY..PROPERTY_ENTRY + 64].fill(0);
    for (index, unit) in name.iter().enumerate() {
        put_u16(&mut bytes, PROPERTY_ENTRY + index * 2, *unit);
    }
    put_u16(
        &mut bytes,
        PROPERTY_ENTRY + 64,
        u16::try_from((name.len() + 1) * 2).expect("fixture name length"),
    );
    put_u32(
        &mut bytes,
        PROPERTY_ENTRY + 120,
        u32::try_from(STREAM_BYTES).expect("fixture stream size"),
    );
    let end = 2 + STREAM_BYTES / 512;
    for sector in 2..end {
        let next = match sector {
            2 => 4,
            3 => 5,
            4 => 3,
            _ if sector + 1 == end => END_CHAIN,
            _ => u32::try_from(sector + 1).expect("fixture FAT link"),
        };
        put_u32(&mut bytes, FAT + sector * 4, next);
    }
    let intact = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("the property-set stream is valid");
    let inventor = intact
        .ir()
        .native
        .namespace("inventor")
        .expect("Inventor namespace");
    let records = inventor
        .arena_as::<crate::native::PropertySetRecord>("property_sets")
        .expect("property-set arena");
    assert_eq!(records.len(), 1);
    for depth in [DecodeDepth::Full, DecodeDepth::Container] {
        let mut options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        options.policy.limits.max_retained_bytes = 16 * 1024;
        let result = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &options)
            .expect("a property-set copy can be skipped without exhausting the session");
        let charges = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.message.starts_with("part property-set "))
            .collect::<Vec<_>>();
        let [loss] = charges.as_slice() else {
            panic!("one property-set charge at {depth:?}: {charges:?}");
        };
        assert_eq!(loss.code, Cause::Undecoded.kind());
        assert!(loss.message.contains("joining CFB stream"));
        assert!(loss.message.contains("Budget"));
    }
    let mut options = cadmpeg_core::decode::InspectOptions::default();
    options.limits.max_retained_bytes = 16 * 1024;
    let summary = InventorCodec
        .inspect(&mut std::io::Cursor::new(&bytes), &options)
        .expect("inspect leaves the session open after a denied copy");
    let charges = summary
        .losses
        .iter()
        .filter(|loss| loss.message.starts_with("part property-set "))
        .collect::<Vec<_>>();
    let [loss] = charges.as_slice() else {
        panic!("one inspect property-set charge: {charges:?}");
    };
    assert_eq!(loss.code, Cause::Undecoded.kind());
    assert!(loss.message.contains("Budget"));
}

#[test]
fn individual_schema_and_instance_expansion_limits_keep_other_zip_members_at_both_depths() {
    use cadmpeg_test_support::zip_fixture;
    use std::io::Write;

    let schema = br#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#;
    let oversized_schema = format!(
        "<Schema><UID val=\"Large\"/><!--{}--></Schema>",
        "x".repeat(2048)
    );
    let mut small_record = Vec::new();
    for value in ["SimpleSchema", "small", "Example", "", &"k".repeat(160)] {
        push_lp(&mut small_record, value);
    }
    let small_instance = paged_instance(&small_record);
    let mut large_record = Vec::new();
    for value in ["SimpleSchema", "large", "Example", "", &"z".repeat(2048)] {
        push_lp(&mut large_record, value);
    }
    let large_instance = paged_instance(&large_record);
    let compress = |bytes: &[u8]| {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).expect("fixture deflate");
        encoder.finish().expect("complete fixture deflate")
    };
    let schema_deflate = compress(oversized_schema.as_bytes());
    let instance_deflate = compress(&large_instance);
    let mut zip = zip_fixture::archive(&[
        zip_fixture::RawEntry::compressed(
            "Schemas/LargeSchema.xml",
            zip_fixture::DEFLATE,
            &schema_deflate,
            oversized_schema.as_bytes(),
        ),
        zip_fixture::RawEntry::stored("Schemas/SimpleSchema.xml", schema),
        zip_fixture::RawEntry::compressed(
            "Large/InstanceProperties.bin",
            zip_fixture::DEFLATE,
            &instance_deflate,
            &large_instance,
        ),
        zip_fixture::RawEntry::stored("Small/InstanceProperties.bin", &small_instance),
    ]);
    let comment = 4092 - zip.len();
    let comment_length = zip.len() - 2;
    zip[comment_length..].copy_from_slice(
        &u16::try_from(comment)
            .expect("fixture comment")
            .to_le_bytes(),
    );
    zip.resize(4092, 0);
    let mut package = 4092_u32.to_le_bytes().to_vec();
    package.extend_from_slice(&zip);
    let mut bytes = fixture_with_ufrx(&package);
    let entry = 512 + 3 * 128;
    bytes[entry..entry + 64].fill(0);
    for (index, unit) in "Protein".encode_utf16().enumerate() {
        bytes[entry + index * 2..entry + index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    bytes[entry + 64..entry + 66].copy_from_slice(&16_u16.to_le_bytes());

    for depth in [DecodeDepth::Full, DecodeDepth::Container] {
        let mut options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        options.policy.limits.max_decompressed_bytes_per_expand = 1024;
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &options)
            .expect("each limited member leaves the package readable");
        let charges = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.message.starts_with("part protein-member "))
            .collect::<Vec<_>>();
        assert_eq!(charges.len(), 2, "{charges:#?}");
        assert!(charges
            .iter()
            .all(|loss| loss.code == Cause::Undecoded.kind()));
        for name in ["Schemas/LargeSchema.xml", "Large/InstanceProperties.bin"] {
            assert_eq!(
                charges
                    .iter()
                    .filter(|loss| loss.message.contains(name))
                    .count(),
                1
            );
        }
        let assets = decoded
            .ir()
            .native
            .namespace("inventor")
            .expect("native namespace")
            .arena_as::<crate::native::protein::ProteinAssetRecord>("protein_assets")
            .expect("assets");
        assert_eq!(assets.len(), 1, "{depth:?}");
        assert_eq!(
            assets[0].entry_name.as_str(),
            "Small/InstanceProperties.bin"
        );
    }
}

fn protein_fixture(mut zip: Vec<u8>, has_directory: bool) -> Vec<u8> {
    if has_directory {
        let comment = u16::try_from(4092 - zip.len()).expect("fixture ZIP comment fits u16");
        let comment_length = zip.len() - 2;
        put_u16(&mut zip, comment_length, comment);
        zip.resize(4092, 0);
    }
    assert_eq!(zip.len(), 4092);
    let mut package = 4092_u32.to_le_bytes().to_vec();
    package.extend_from_slice(&zip);
    let mut bytes = fixture_with_ufrx(&package);
    rename_root_stream(&mut bytes, "Protein");
    bytes
}

#[test]
fn a_failed_schema_does_not_hide_independent_instance_damage_or_limits() {
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry, DEFLATE};
    use std::io::Write as _;

    let schema = RawEntry::stored("Schemas/BrokenSchema.xml", b"<Schema><");
    let mut bad = RawEntry::stored("Broken/InstanceProperties.bin", b"bad");
    bad.crc32 ^= 1;
    let large = [0; 2048];
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&large).expect("write fixture instance");
    let compressed = encoder.finish().expect("finish the fixture DEFLATE member");
    let limited =
        RawEntry::compressed("Large/InstanceProperties.bin", DEFLATE, &compressed, &large);
    let bytes = protein_fixture(archive(&[schema, bad, limited]), true);

    let assert_losses = |losses: &[LossNote]| {
        for (member, cause) in [
            ("BrokenSchema.xml", Cause::Unreadable),
            ("Broken/InstanceProperties.bin", Cause::Unreadable),
            ("Large/InstanceProperties.bin", Cause::Undecoded),
        ] {
            assert_part_loss(losses, "part protein-member ", member, cause);
        }
    };
    for depth in [DecodeDepth::Full, DecodeDepth::Container] {
        let mut options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        options.policy.limits.max_decompressed_bytes_per_expand = 1024;
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &options)
            .expect("the remaining parts keep the document readable");
        assert_losses(&decoded.report().losses);
    }
    let mut inspect = InspectOptions::default();
    inspect.limits.max_decompressed_bytes_per_expand = 1024;
    assert_losses(&inspect_losses(&bytes, &inspect));
}

fn healthy_protein_members() -> [cadmpeg_test_support::zip_fixture::RawEntry; 2] {
    use cadmpeg_test_support::zip_fixture::RawEntry;
    let schema = br#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#;
    let mut record = Vec::new();
    for value in ["SimpleSchema", "asset", "Example", "", &"k".repeat(160)] {
        push_lp(&mut record, value);
    }
    [
        RawEntry::stored("Schemas/SimpleSchema.xml", schema),
        RawEntry::stored("Small/InstanceProperties.bin", &paged_instance(&record)),
    ]
}

#[test]
fn an_unsafe_protein_name_leaves_its_readable_siblings() {
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};

    let [schema, instance] = healthy_protein_members();
    let bytes = protein_fixture(
        archive(&[schema, RawEntry::stored("../extra", b"x"), instance]),
        true,
    );
    for depth in [DecodeDepth::Full, DecodeDepth::Container] {
        let options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &options)
            .expect("the remaining parts keep the document readable");
        assert_part_loss(
            &decoded.report().losses,
            "part protein-member ",
            "../extra",
            Cause::Unreadable,
        );
        let assets = decoded
            .ir()
            .native
            .namespace("inventor")
            .expect("the decoded file keeps its Inventor namespace")
            .arena_as::<crate::native::protein::ProteinAssetRecord>("protein_assets")
            .expect("the readable Protein asset arena");
        assert_eq!(assets.len(), 1, "{depth:?}");
        assert_eq!(
            assets[0].entry_name.as_str(),
            "Small/InstanceProperties.bin"
        );
    }
}

#[test]
fn a_protein_member_give_up_points_into_the_zip_and_links_its_declaration() {
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};

    let [schema, _] = healthy_protein_members();
    let raw = b"bad";
    let mut bad = RawEntry::stored("Bad/InstanceProperties.bin", raw);
    bad.crc32 ^= 1;
    let offset = 30 + schema.name.len() + schema.payload.len() + 30 + bad.name.len();
    let zip = archive(&[schema, bad]);
    assert_eq!(&zip[offset..offset + raw.len()], raw);
    let bytes = protein_fixture(zip, true);
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("the Protein package remains inspectable in the decode");
    let loss = assert_part_loss(
        &decoded.report().losses,
        "part protein-member ",
        "Bad/InstanceProperties.bin",
        Cause::Unreadable,
    );
    assert!(
        loss.message.contains(&format!(
            "stream=\"inventor:Protein:zip\" offset={offset} length=3"
        )),
        "{loss:?}"
    );
    let span = decoded
        .source_fidelity()
        .retained_records()
        .values()
        .find(|span| span.stream() == "inventor:Protein:zip")
        .expect("ZIP-relative evidence");
    assert_eq!(span.offset(), cadmpeg_core::decode::u64_from_index(offset));
    assert_eq!(span.byte_len(), 3);
    assert_eq!(span.sha256(), cadmpeg_ir::hash::sha256_hex(raw));
    let unknowns = decoded
        .ir()
        .native_unknowns("inventor")
        .expect("the member give-up has an Inventor stub");
    let stub = unknowns
        .iter()
        .find(|stub| loss.message.contains(&format!(" record={}: ", stub.id)))
        .expect("the loss names its stub");
    assert_eq!(
        stub.links,
        [cadmpeg_ir::ids::Identity::new("inventor:protein:entry#1")
            .expect("native declaration id")]
    );
}

#[test]
fn a_recovered_protein_directory_is_charged_and_its_local_members_read() {
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
    let [schema, instance] = healthy_protein_members();
    let local_size = 60
        + schema.name.len()
        + schema.payload.len()
        + instance.name.len()
        + instance.payload.len();
    let padding = RawEntry::stored("padding", &vec![0; 4092 - local_size - 37]);
    let mut zip = archive(&[schema, instance, padding]);
    zip.truncate(4092);
    let bytes = protein_fixture(zip, false);
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("the Protein package remains inspectable in the decode");
    assert_part_loss(
        &decoded.report().losses,
        "part protein-directory ",
        "",
        Cause::Unreadable,
    );
    let assets = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("the decoded file keeps its Inventor namespace")
        .arena_as::<crate::native::protein::ProteinAssetRecord>("protein_assets")
        .expect("the recovered Protein assets");
    assert_eq!(assets.len(), 1);
}

#[test]
fn a_foreign_database_grammar_refusal_is_visible_at_each_depth_and_inspect() {
    let bytes =
        crate::test_support::test_fixtures::primary_envelope_fixture_with_unframed_foreign_database(
        );
    for depth in [DecodeDepth::Full, DecodeDepth::Container] {
        let options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &options)
            .expect("the remaining parts keep the document readable");
        assert_part_loss(
            &decoded.report().losses,
            "part rse-database ",
            "schema 12",
            Cause::Undecoded,
        );
    }
    assert_part_loss(
        &inspect_losses(&bytes, &InspectOptions::default()),
        "part rse-database ",
        "schema 12",
        Cause::Undecoded,
    );
}

#[test]
fn a_ufrx_stream_with_an_unallocated_start_is_given_up_once() {
    let mut bytes = fixture_with_ufrx(&[]);
    put_u32(&mut bytes, 512 + 3 * 128 + 116, 128);
    for depth in [DecodeDepth::Full, DecodeDepth::Container] {
        let options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        let result = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &options)
            .expect("an unread reference stream leaves the container readable");
        let all = &result.report().losses;
        let reference_losses: Vec<_> = all
            .iter()
            .filter(|loss| loss.message.contains("UFRxDoc"))
            .collect();
        let [loss] = reference_losses.as_slice() else {
            panic!("one UFRxDoc charge at {depth:?}: {all:?}");
        };
        assert_eq!(loss.code, Cause::Unreadable.kind());
        assert!(loss.message.starts_with("part ufrx-stream "));
    }
}

#[test]
fn an_unpaired_segment_stream_is_given_up_whole() {
    let bytes = unpaired_envelope();
    let decoded = InventorCodec
        .decode(
            &mut std::io::Cursor::new(bytes.clone()),
            &DecodeOptions::default(),
        )
        .expect("a default decode admits the unpaired streams");
    let losses = &decoded.report().losses;
    let charges = charges(losses);
    let unpaired = charges
        .iter()
        .filter(|message| {
            message.starts_with("part rse-bulk stream=\"inventor:RSeStorage/Bdc\" offset=0 ")
                || message
                    .starts_with("part rse-metadata stream=\"inventor:RSeStorage/Mde\" offset=0 ")
        })
        .collect::<Vec<_>>();
    assert_eq!(unpaired.len(), 2, "{losses:#?}");
    for message in &unpaired {
        assert!(message.contains("has no "), "{message}");
        assert!(message.ends_with("; dropped=[]"), "{message}");
        assert!(
            !message.contains("inventor:rse:unpaired-"),
            "the kept unpaired-segment record is not given up: {message}"
        );
    }
    let unknowns = decoded
        .ir()
        .native_unknowns("inventor")
        .expect("native unknowns");
    for message in &unpaired {
        assert!(
            unknowns
                .iter()
                .any(|stub| message.contains(&format!(" record={}: ", stub.id))),
            "the charge names its stub: {message}"
        );
    }

    let mut pinned = DecodeOptions::default();
    pinned.strictness.pinned = true;
    let Err(DecodeFailure::StrictRejected { rejection }) =
        InventorCodec.decode(&mut std::io::Cursor::new(bytes), &pinned)
    else {
        panic!("a pinned decode refuses the unpaired streams");
    };
    assert_eq!(rejection.loss().code, record_unreadable_kind());
    assert!(
        charges.contains(&rejection.loss().message.as_str()),
        "{}",
        rejection.loss().message
    );
}

#[test]
fn a_metadata_stream_keeps_its_readable_declaration_past_the_body_expand_limit() {
    let bytes = primary_envelope_fixture();
    let mut options = cadmpeg_core::decode::InspectOptions::default();
    options.limits.max_decompressed_bytes_per_expand = 1;
    let summary = InventorCodec
        .inspect(&mut std::io::Cursor::new(&bytes), &options)
        .expect("inspect gives up each limited stream");
    let entry = summary
        .entries
        .iter()
        .find(|entry| entry.name.starts_with("RSeStorage/M"))
        .expect("metadata stream");
    assert_eq!(
        entry.attributes.get("meta_marker").map(String::as_str),
        Some("RSe Meta Stream Version 8")
    );
    assert_eq!(
        entry
            .attributes
            .get("meta_stream_version")
            .map(String::as_str),
        Some("8")
    );
    assert!(summary
        .losses
        .iter()
        .any(|loss| loss.code == Cause::Undecoded.kind()
            && loss.message.starts_with("part rse-metadata ")));
}

#[test]
fn an_unknown_reference_table_width_is_undecoded() {
    assert_reference_table_give_up(76, Cause::Undecoded);
}

#[test]
fn a_reference_table_shorter_than_its_count_is_unreadable() {
    assert_reference_table_give_up(31, Cause::Unreadable);
}

fn assert_reference_table_give_up(length: usize, cause: Cause) {
    let tables = BaseEnvelopeRecords {
        design_version: 18,
        brep: Vec::new(),
        pairs: vec![0x41; length],
        pair_count: Some(1),
        revision_count: None,
        revisions: Vec::new(),
        references: Vec::new(),
    };
    let bytes = primary_envelope_fixture_with_drawing_tables(
        &acis_sphere_kernel_stream(21_800),
        &[([0x71; 16], vec![0_u8; 8])],
        &tables,
    );
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("a default decode admits the unread tables");
    let losses = &decoded.report().losses;
    let charges = losses
        .iter()
        .filter(|loss| loss.code == cause.kind())
        .filter(|loss| {
            loss.message
                .starts_with("part meta-section stream=\"RSeStorage/Mdc:expanded\"")
        })
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    let [charge] = charges.as_slice() else {
        panic!("one charge reaches the reference section: {losses:#?}");
    };
    assert!(charge.contains("reference table of 1 entries"), "{charge}");
    assert!(charge.ends_with("; dropped=[]"), "{charge}");
    let unknowns = decoded
        .ir()
        .native_unknowns("inventor")
        .expect("native unknowns");
    let stub = unknowns
        .iter()
        .find(|stub| charge.contains(&format!(" record={}: ", stub.id)))
        .expect("the charge names its stub");
    assert_eq!(
        stub.links,
        [cadmpeg_ir::ids::Identity::new("inventor:rse:meta-section#dc-7").expect("section id")]
    );
}

#[test]
fn drawing_reference_sections_keep_their_readable_siblings() {
    for (revision_bytes, expected_issues) in [(20, &[7][..]), (19, &[7, 8][..])] {
        let tables = BaseEnvelopeRecords {
            design_version: 18,
            brep: Vec::new(),
            pairs: vec![0x41; 31],
            pair_count: Some(1),
            revision_count: Some(1),
            revisions: vec![0x42; revision_bytes],
            references: vec![0x43; 8],
        };
        let bytes = primary_envelope_fixture_with_drawing_tables(
            &acis_sphere_kernel_stream(21_800),
            &[([0x71; 16], vec![0_u8; 8])],
            &tables,
        );
        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
            .expect("damaged reference tables leave their sibling readable");
        let losses = charges(&decoded.report().losses);
        let sections = decoded
            .ir()
            .native_unknowns("inventor")
            .expect("section stubs");
        let section_losses = sections
            .iter()
            .filter(|stub| {
                losses
                    .iter()
                    .any(|loss| loss.contains(&format!(" record={}: ", stub.id)))
            })
            .flat_map(|stub| stub.links.iter())
            .filter_map(|link| link.as_str().strip_prefix("inventor:rse:meta-section#dc-"))
            .map(|number| number.parse::<u8>().expect("section number"))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(section_losses, expected_issues.iter().copied().collect());
        let namespace = decoded
            .ir()
            .native
            .namespace("inventor")
            .expect("native arena");
        let tables = namespace
            .arena_as::<crate::drawing::references::Tables>("drawing_reference_tables")
            .expect("native drawing tables");
        let drawing = tables
            .iter()
            .find(|table| table.segment_token.as_str() == "dc")
            .expect("table");
        assert_eq!(drawing.counts, [1, 1, 1]);
        assert!(drawing.payloads[0].is_none());
        assert_eq!(
            drawing.payloads[1].as_ref().map(|bytes| bytes.as_slice()),
            (revision_bytes == 20).then_some(&[0x42; 20][..])
        );
        assert_eq!(
            drawing.payloads[2].as_ref().map(|bytes| bytes.as_slice()),
            Some(&[0x43; 8][..])
        );
        assert_eq!(drawing.revisions.len(), usize::from(revision_bytes == 20));
        assert_eq!(drawing.external.len(), 1);
    }
}

#[test]
fn a_short_non_drawing_reference_section_keeps_the_segment_records() {
    let tables = BaseEnvelopeRecords {
        design_version: 18,
        brep: Vec::new(),
        pairs: vec![0x41; 31],
        pair_count: Some(1),
        revision_count: None,
        revisions: Vec::new(),
        references: Vec::new(),
    };
    let bytes = crate::test_support::test_fixtures::base_envelope_fixture(
        &acis_sphere_kernel_stream(21_800),
        &[([0x71; 16], vec![0_u8; 8])],
        &tables,
    );
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("the metadata segment remains framed");
    let charges = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.message
                .starts_with("part meta-section stream=\"RSeStorage/Mdc:expanded\"")
        })
        .collect::<Vec<_>>();
    let [loss] = charges.as_slice() else {
        panic!("one reference section charge: {charges:#?}");
    };
    assert_eq!(loss.code, Cause::Unreadable.kind());
    let records = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("native namespace")
        .arena_as::<crate::native::RseRecordRecord>("rse_records")
        .expect("RSe record arena");
    assert!(records
        .iter()
        .any(|record| record.token == "dc" && record.ordinal == 0));
}

/// The sphere kernel stream with a second face, record 6, after the sphere
/// face in the shell. The surface reference of record 6 names no record.
fn kernel_with_a_face_without_surface() -> Vec<u8> {
    let mut kernel = acis_sphere_kernel_stream(21_800);
    let face = [0x0d, 4, b'f', b'a', b'c', b'e'];
    let found = kernel
        .windows(face.len())
        .enumerate()
        .filter(|(_, window)| *window == face.as_slice())
        .map(|(at, _)| at)
        .collect::<Vec<_>>();
    let [at] = found.as_slice() else {
        panic!("one face record: {found:?}");
    };
    // The identifier, the attribute reference, and the long precede the
    // references; the second reference is the next face.
    let next_face = at + face.len() + 5 + 5 + 5 + 1;
    kernel[next_face..next_face + 4].copy_from_slice(&6_i32.to_le_bytes());
    kernel.extend_from_slice(&face);
    for (tag, value) in [(0x0c, -1_i32), (0x04, -1)] {
        kernel.push(tag);
        kernel.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-1_i32, -1, -1, 3, -1, 9] {
        kernel.push(0x0c);
        kernel.extend_from_slice(&value.to_le_bytes());
    }
    kernel.extend_from_slice(&[0x0b, 0x0b, 0x11]);
    kernel
}

#[test]
fn a_kernel_face_the_shared_decode_leaves_out_is_given_up() {
    let bytes = primary_envelope_fixture_with_kernel(
        EnvelopeDeclarations::default(),
        &kernel_with_a_face_without_surface(),
    );
    let decoded = InventorCodec
        .decode(
            &mut std::io::Cursor::new(bytes.clone()),
            &DecodeOptions::default(),
        )
        .expect("a default decode admits the face without surface");
    let losses = &decoded.report().losses;
    assert!(!decoded.ir().geometry.faces.is_empty(), "{losses:#?}");
    let charges = charges(losses);
    let [charge] = charges
        .iter()
        .filter(|message| message.starts_with("part face stream=\"RSeStorage/B"))
        .collect::<Vec<_>>()[..]
    else {
        panic!("one charge gives up the face: {losses:#?}");
    };
    assert!(charge.contains("the face record"), "{charge}");
    assert!(charge.contains("brep:entity#6"), "{charge}");

    let mut pinned = DecodeOptions::default();
    pinned.strictness.pinned = true;
    let Err(DecodeFailure::StrictRejected { rejection }) =
        InventorCodec.decode(&mut std::io::Cursor::new(bytes), &pinned)
    else {
        panic!("a pinned decode refuses the face without surface");
    };
    assert_eq!(rejection.loss().code, record_unreadable_kind());
}
