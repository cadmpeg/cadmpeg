// SPDX-License-Identifier: Apache-2.0
//! Genuine automatic classification for the adopted whole source inventory.

use cadmpeg_ir::{ArenaClass, Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn an_opaque_member_has_an_automatic_b1_span_and_no_per_entry_evidence_record() {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='1'><Property name='Included' type='App::PropertyFileIncluded'><FileIncluded file='shared.bin'/></Property></Properties></Object></ObjectData></Document>";
    let payload = b"source bytes shared by application references";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("shared.bin", payload),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let fidelity = decoded.source_fidelity();
    let id = crate::native::native_id("entry", "shared.bin");
    let (stream, offset, byte_len) = member_extent(&bytes, &decoded, "shared.bin");
    assert_eq!(offset, 0);
    assert_eq!(byte_len, u64::try_from(payload.len()).unwrap());
    assert!(fidelity.retained_record(&id).is_none());
    let spans = fidelity.b1_spans()[&stream].spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].range(), 0..u64::try_from(payload.len()).unwrap());
    assert_eq!(
        spans[0].classes(),
        [ArenaClass::Model, ArenaClass::Design].into()
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
    let matching = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code.local_code() == "source.bytes-undecoded"
                && loss.provenance.as_ref().is_some_and(|provenance| {
                    provenance.stream_name() == Some(stream.as_str())
                        && provenance.offset == 0
                        && provenance.byte_len() == Some(u64::try_from(payload.len()).unwrap())
                })
        })
        .count();
    assert_eq!(matching, 1);
    assert_eq!(
        crate::test_support::source_fidelity::entry_bytes(fidelity, &id),
        payload
    );
}

#[test]
fn thumbnail_evidence_names_its_genuine_member_stream_and_preserves_exact_bytes() {
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let thumbnail = b"\x89PNG\r\n\x1a\nsource thumbnail bytes";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("Thumbnail.png", thumbnail),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let fidelity = decoded.source_fidelity();
    let (stream, offset, byte_len) = member_extent(&bytes, &decoded, "Thumbnail.png");
    let id = "fcstd:native:thumbnail#Thumbnail.png";
    let evidence = fidelity.retained_record(id).unwrap();
    assert_eq!(evidence.stream(), stream.as_str());
    assert_eq!(evidence.offset(), offset);
    assert_eq!(evidence.data(), None);
    assert_eq!(byte_len, u64::try_from(thumbnail.len()).unwrap());
    assert_eq!(evidence.byte_len(), byte_len);
    assert_eq!(evidence.sha256(), cadmpeg_ir::hash::sha256_hex(thumbnail));
    let opaque_layout = &fidelity.record_layout()[id];
    assert_eq!(opaque_layout.stream(), stream.as_str());
    assert_eq!(opaque_layout.offset(), offset);
    assert_eq!(opaque_layout.byte_len(), evidence.byte_len());
    assert!(opaque_layout.reads().is_empty());
    assert_eq!(
        crate::test_support::source_fidelity::entry_bytes(
            fidelity,
            &crate::native::native_id("entry", "Thumbnail.png"),
        ),
        thumbnail,
    );
    assert_eq!(
        fidelity
            .retained_records()
            .values()
            .filter(|record| record.data().is_some())
            .count(),
        1,
    );
    assert_eq!(
        fidelity.annotations.provenance[id].stream(),
        stream.as_str()
    );
    assert_eq!(fidelity.annotations.provenance[id].offset, offset);
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
}

#[test]
fn an_empty_given_up_member_keeps_its_real_part_without_a_fabricated_byte_interval() {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>"),
        RawEntry::compressed("broken.bin", 12, b"", b""),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let fidelity = decoded.source_fidelity();
    let failed: Vec<_> = fidelity
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].byte_len(), 0);
    assert_eq!(failed[0].stream(), "file");
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    assert_eq!(unknowns.len(), 1);
    let evidence = fidelity.retained_record(unknowns[0].id.as_str()).unwrap();
    assert_eq!(evidence.stream(), failed[0].stream());
    assert_eq!(evidence.offset(), failed[0].offset());
    assert_eq!(evidence.data(), Some(b"".as_slice()));
    assert!(fidelity
        .b1_spans()
        .values()
        .flat_map(|stream| stream.spans())
        .all(|span| span.range().start < span.range().end));
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
}

fn member_extent(
    bytes: &[u8],
    decoded: &cadmpeg_ir::codec::DecodeResult,
    name: &str,
) -> (String, u64, u64) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let index = archive
        .entries()
        .iter()
        .position(|entry| entry.name == name)
        .unwrap();
    let header = archive.member_frames(&ctx).unwrap()[index]
        .unwrap()
        .header();
    let header = ctx.capture_part_extent(header).unwrap();
    let name_source = archive
        .entry_name_source(&archive.entries()[index])
        .unwrap();
    let name_source = ctx.capture_part_extent(name_source).unwrap();
    let layout =
        &decoded.source_fidelity().record_layout()[&crate::native::native_id("entry", name)];
    assert_eq!(layout.stream(), header.stream());
    assert_eq!(layout.offset(), header.offset());
    assert_eq!(layout.byte_len(), header.byte_len());
    assert_eq!(layout.reads().len(), 1);
    assert_eq!(
        layout.reads()[name_source.stream()],
        [name_source.offset()..name_source.offset() + name_source.byte_len()]
    );
    let member = archive.open(&ctx, name).unwrap();
    let extent = ctx.capture_part_extent(member).unwrap();
    assert_eq!(
        layout.pointers(),
        &crate::source_archive::entry_pointers(&archive.entries()[index], extent.byte_len())
            .unwrap()
    );
    ctx.finish_session().unwrap();
    (
        extent.stream().to_owned(),
        extent.offset(),
        extent.byte_len(),
    )
}
