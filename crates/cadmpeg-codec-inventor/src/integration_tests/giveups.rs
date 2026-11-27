// SPDX-License-Identifier: Apache-2.0
//! Give-ups of whole streams and metadata sections, as a decode reports them.

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{record_unreadable_kind, Cause};

use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream, fixture_with_ufrx, primary_envelope_fixture,
    primary_envelope_fixture_with_design, primary_envelope_fixture_with_drawing_tables,
    primary_envelope_fixture_with_kernel, BaseEnvelopeRecords, EnvelopeDeclarations,
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
        assert!(message.contains("rse.stream-unpaired"), "{message}");
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
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    let [charge] = charges
        .iter()
        .filter(|message| message.contains("inventor:drawing:reference-tables#dc"))
        .collect::<Vec<_>>()[..]
    else {
        panic!("one charge reaches the reference tables: {losses:#?}");
    };
    assert!(
        charge.starts_with("part meta-section stream=\"RSeStorage/Mdc:expanded\" offset="),
        "{charge}"
    );
    assert!(
        charge.contains("drawing reference tables do not read"),
        "{charge}"
    );
    assert!(
        charge.ends_with("; dropped=[inventor:drawing:reference-tables#dc]"),
        "{charge}"
    );
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
