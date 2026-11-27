// SPDX-License-Identifier: Apache-2.0
//! One unreadable part of a document costs that part (spec §3.3).

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::parasolid::{parasolid_with_body, triangle_body};
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::record_unreadable_kind;
use cadmpeg_test_support::bytes::{put_u16, put_u32};
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

const SECTOR: usize = 512;
const PARTITION: &str = "Config-0-Partition";

fn decode(bytes: Vec<u8>) -> EditableDecodeResult {
    EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("the document decodes"),
    )
}

fn unreadable(decoded: &EditableDecodeResult) -> Vec<&LossNote> {
    decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .collect()
}

/// The committed stub whose id the loss names.
fn names_a_stub(decoded: &EditableDecodeResult, loss: &LossNote) -> bool {
    decoded.ir().native.0["sldprt"].arenas()["unknowns"]
        .iter()
        .any(|stub| loss.message.contains(&format!(" record={}: ", stub.id())))
}

fn sector_mut(file: &mut [u8], sector: usize) -> &mut [u8] {
    let start = (sector + 1) * SECTOR;
    &mut file[start..start + SECTOR]
}

/// The shared CFB fixture with a second regular stream, `Config-0-Partition`,
/// in sectors 10 to 17: a ZLB-wrapped Parasolid partition holding one body.
fn compound_with_partition() -> Vec<u8> {
    let mut file = cadmpeg_test_support::compound::compound_fixture();
    let partition = parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &partition).unwrap();
    let member = encoder.finish().unwrap();
    let mut wrapper = crate::layout::zlb_wrapper_header::MAGIC_VALUE.to_vec();
    wrapper.extend(u32::try_from(partition.len()).unwrap().to_le_bytes());
    wrapper.extend(u32::try_from(member.len()).unwrap().to_le_bytes());
    wrapper.extend(&member);
    wrapper.extend([0; 8]);
    assert!(wrapper.len() <= 8 * SECTOR);
    wrapper.resize(8 * SECTOR, 0);
    file.extend(&wrapper);
    let fat = sector_mut(&mut file, 9);
    for sector in 10..17 {
        put_u32(fat, sector * 4, u32::try_from(sector + 1).unwrap());
    }
    put_u32(fat, 17 * 4, 0xffff_fffe);
    let directory = sector_mut(&mut file, 0);
    put_u32(directory, 128 + 72, 2);
    let entry = &mut directory[2 * 128..3 * 128];
    let units = PARTITION.encode_utf16().collect::<Vec<_>>();
    for (index, unit) in units.iter().enumerate() {
        put_u16(entry, index * 2, *unit);
    }
    put_u16(entry, 64, u16::try_from((units.len() + 1) * 2).unwrap());
    entry[66] = 2;
    entry[67] = 0;
    entry[68..80].fill(0xff);
    put_u32(entry, 116, 10);
    entry[120..128].copy_from_slice(&4096_u64.to_le_bytes());
    file
}

#[test]
fn a_cfb_stream_whose_chain_cannot_be_followed_costs_that_stream() {
    let baseline = decode(compound_with_partition());
    assert!(!baseline.ir().geometry.faces.is_empty());
    assert!(unreadable(&baseline).is_empty());

    let mut file = compound_with_partition();
    put_u32(sector_mut(&mut file, 9), 4, 1);
    let decoded = decode(file);
    assert_eq!(decoded.ir().geometry, baseline.ir().geometry);
    let [loss] = unreadable(&decoded)[..] else {
        panic!("one loss names the stream");
    };
    assert!(loss.message.contains("part compound-stream \"Payload\""));
    assert!(loss.message.contains("cyclic"));
    assert!(names_a_stub(&decoded, loss));
    // The chain locates sector 1 before it returns to it.
    assert!(loss.message.contains(" offset=0 length=512 "));
    assert!(loss.message.contains(
        "the stream declares 4096 bytes; its length counts the 512 bytes its sector chain located"
    ));
    let stub = loss
        .message
        .split(" record=")
        .nth(1)
        .and_then(|rest| rest.split(": ").next())
        .expect("the loss names its stub");
    let retained = decoded
        .source_fidelity()
        .retained_record(stub)
        .expect("the stream's located bytes are retained");
    assert_eq!(retained.stream(), "Payload");
    assert_eq!(retained.byte_len(), 512);
    assert_eq!(
        retained.untyped(),
        &std::collections::BTreeSet::from([
            cadmpeg_core::decode::ArenaClass::Model,
            cadmpeg_core::decode::ArenaClass::Design
        ])
    );
}

#[test]
fn an_unread_partition_stream_is_not_reported_as_absent() {
    let mut file = compound_with_partition();
    put_u32(sector_mut(&mut file, 9), 10 * 4, 10);
    let decoded = decode(file);
    assert!(decoded.ir().geometry.faces.is_empty());
    let [loss] = unreadable(&decoded)[..] else {
        panic!("one loss names the partition stream");
    };
    assert!(loss
        .message
        .contains(&format!("part compound-stream \"{PARTITION}\"")));
    assert!(names_a_stub(&decoded, loss));
    let absent = crate::loss::SldprtLossCode::ContainerNoParasolidStream.kind();
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == absent));
}

#[test]
fn an_unterminated_cfb_stream_chain_is_read_and_reported() {
    let mut file = compound_with_partition();
    put_u32(sector_mut(&mut file, 9), 17 * 4, 0xdead_beef);
    let decoded = decode(file.clone());
    assert_eq!(
        decoded.ir().geometry,
        decode(compound_with_partition()).ir().geometry
    );
    assert!(unreadable(&decoded).is_empty());
    let noncanonical = crate::loss::SldprtLossCode::ContainerNoncanonical.kind();
    let [loss] = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == noncanonical)
        .collect::<Vec<_>>()[..]
    else {
        panic!("one loss names the stream");
    };
    assert!(loss.message.starts_with(PARTITION));
    let Err(cadmpeg_ir::DecodeFailure::StrictRejected { rejection }) = SldprtCodec.decode(
        &mut Cursor::new(file),
        &crate::test_support::ir::strict_options(),
    ) else {
        panic!("a pinned decode refuses the compound deviation");
    };
    assert_eq!(rejection.loss().code, noncanonical);
}

#[test]
fn a_native_block_that_fails_its_crc_costs_that_block() {
    let baseline = decode(sldprt_with_body(&triangle_body()));
    let mut bytes = sldprt_with_body(&triangle_body());
    let mut block = make_block(0x43, "Contents/Config-1-Partition", b"payload bytes");
    let crc = crate::layout::block_frame_header::CRC32;
    block[crc] ^= 0xff;
    let offset = bytes.len();
    bytes.extend(&block);
    let decoded = decode(bytes);
    assert!(!baseline.ir().geometry.faces.is_empty());
    assert_eq!(
        decoded.ir().geometry.faces.len(),
        baseline.ir().geometry.faces.len()
    );
    assert_eq!(
        decoded.ir().geometry.bodies.len(),
        baseline.ir().geometry.bodies.len()
    );
    let attributes = &decoded.ir().source.as_ref().expect("source").attributes;
    assert!(!attributes.contains_key("active_parasolid_block"));
    assert_eq!(
        attributes
            .get("sldprt_active_partition_unresolved")
            .map(String::as_str),
        Some("true")
    );
    let unread = crate::loss::SldprtLossCode::ConfigActivePartitionUnread.kind();
    let [note] = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == unread)
        .collect::<Vec<_>>()[..]
    else {
        panic!("one note says why no active partition is chosen");
    };
    assert!(note.message.contains("Contents/Config-1-Partition"));
    let [loss] = unreadable(&decoded)[..] else {
        panic!("one loss names the block");
    };
    assert!(loss
        .message
        .contains("part native-block \"Contents/Config-1-Partition\""));
    assert!(loss.message.contains(&format!("offset={offset} ")));
    assert!(names_a_stub(&decoded, loss));
}

#[test]
fn a_wrapped_member_that_does_not_inflate_is_given_up_as_a_part() {
    let mut file = compound_with_partition();
    let magic = crate::layout::zlb_wrapper_header::MAGIC_VALUE;
    let member = file
        .windows(magic.len())
        .rposition(|window| window == magic)
        .expect("the partition wrapper")
        + crate::layout::zlb_wrapper_header::LEN;
    file[member + 2..member + 34].fill(0xff);
    let decoded = decode(file);
    assert!(decoded.ir().geometry.faces.is_empty());
    let [loss] = unreadable(&decoded)[..] else {
        panic!("one loss names the wrapped member");
    };
    assert!(loss
        .message
        .contains(&format!("part wrapped-member \"{PARTITION}\"")));
    assert!(names_a_stub(&decoded, loss));
    let absent = crate::loss::SldprtLossCode::ContainerNoParasolidStream.kind();
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == absent));
}
