// SPDX-License-Identifier: Apache-2.0
//! Single archive retention, genuine member origins and lazy source reopening.

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, SourceOrigin};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::{Cursor, Write};

fn source(compression: zip::CompressionMethod) -> Vec<u8> {
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("Document.xml", document.as_slice()),
        ("peer.bin", b"peer source".as_slice()),
    ] {
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default().compression_method(compression),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn stored_and_expanded_members_reopen_from_one_archive_with_exact_origins_and_no_reads() {
    for method in [
        zip::CompressionMethod::Stored,
        zip::CompressionMethod::Deflated,
    ] {
        let bytes = source(method);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let fidelity = decoded.source_fidelity();
        assert_eq!(image(fidelity).unwrap(), bytes);
        for name in ["Document.xml", "peer.bin"] {
            assert!(fidelity
                .retained_record(&crate::native::native_id("entry", name))
                .is_none());
        }
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) =
            DecodeContext::from_root_bytes(image(fidelity).unwrap(), &arena, &policy).unwrap();
        let archive = OriginalArchive::new(&ctx, root, fidelity).unwrap();
        let peer = archive.open(&ctx, "peer.bin").unwrap();
        assert_eq!(peer.window(), b"peer source");
        let peer_capture = ctx.capture_part_extent(peer).unwrap();
        let streams = ctx.source_streams().unwrap();
        let stream = streams
            .iter()
            .find(|stream| stream.name() == peer_capture.stream())
            .unwrap();
        let pointers =
            fidelity.record_layout()[&crate::native::native_id("entry", "peer.bin")].pointers();
        let extent = match (method, stream.origin()) {
            (zip::CompressionMethod::Stored, SourceOrigin::Alias { parent, extent })
            | (zip::CompressionMethod::Deflated, SourceOrigin::Expanded { parent, extent }) => {
                assert_eq!(
                    parent,
                    fidelity
                        .retained_record(crate::fidelity::SOURCE_IMAGE)
                        .unwrap()
                        .stream()
                );
                extent
            }
            other => panic!("wrong actual source origin: {other:?}"),
        };
        assert_eq!(extent.start, pointers["archive.data_start"]);
        assert_eq!(extent.end, pointers["archive.data_end"]);
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn an_origin_mismatch_refuses_that_member_without_dropping_a_valid_peer() {
    let bytes = source(zip::CompressionMethod::Deflated);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut wire = serde_json::to_value(decoded.source_fidelity()).unwrap();
    let id = crate::native::native_id("entry", "Document.xml");
    let pointer = &mut wire["record_layout"][&id]["pointers"]["archive.data_start"];
    *pointer = serde_json::json!(pointer.as_u64().unwrap().checked_add(1).unwrap());
    let changed: SourceFidelity = serde_json::from_value(wire).unwrap();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) =
        DecodeContext::from_root_bytes(image(&changed).unwrap(), &arena, &policy).unwrap();
    let archive = OriginalArchive::new(&ctx, root, &changed).unwrap();
    assert!(archive.open(&ctx, "Document.xml").is_err());
    assert_eq!(
        archive.open(&ctx, "peer.bin").unwrap().window(),
        b"peer source"
    );
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
