// SPDX-License-Identifier: Apache-2.0
//! Give-ups of whole streams and metadata sections, as a decode reports them.

use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{record_unreadable_kind, Cause};

use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream, primary_envelope_fixture, primary_envelope_fixture_with_design,
    primary_envelope_fixture_with_drawing_tables, primary_envelope_fixture_with_kernel,
    BaseEnvelopeRecords, EnvelopeDeclarations,
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
    let decoded = InventorCodec
        .decode(
            &mut std::io::Cursor::new(bytes.clone()),
            &DecodeOptions::default(),
        )
        .expect("a default decode gives up the stream and continues");
    let losses = &decoded.report().losses;
    let noncanonical = crate::loss::InventorLossCode::ContainerNoncanonical
        .note("")
        .code;
    assert!(
        losses.iter().all(|loss| loss.code != noncanonical),
        "{losses:#?}"
    );
    let broken = charges(losses)
        .into_iter()
        .filter(|message| message.contains("Broken"))
        .collect::<Vec<_>>();
    assert_eq!(broken.len(), 1, "{losses:#?}");

    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    match InventorCodec.decode(&mut std::io::Cursor::new(bytes), &options) {
        Err(DecodeFailure::StrictRejected { rejection }) => {
            assert_eq!(rejection.loss().code, record_unreadable_kind());
        }
        other => panic!("a pinned decode refuses the give-up: {:?}", other.err()),
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
fn reference_tables_that_do_not_read_give_up_their_section() {
    // Section 7 stores 76 bytes for one 32-byte segment pair.
    let tables = BaseEnvelopeRecords {
        design_version: 18,
        brep: Vec::new(),
        pairs: vec![0x41; 76],
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
        .filter(|loss| loss.code == Cause::Undecoded.kind())
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
