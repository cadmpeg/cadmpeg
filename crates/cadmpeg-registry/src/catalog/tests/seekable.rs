// SPDX-License-Identifier: Apache-2.0
//! Remote ZIP namespace evidence and bounded source acquisition.

use crate::{InputCatalog, ResolvedSource};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

#[cfg(any(feature = "fcstd", feature = "f3d", feature = "step"))]
#[test]
fn zip_members_beyond_the_prefix_resolve_with_or_without_first_frame_damage() {
    let cases = [
        #[cfg(feature = "fcstd")]
        ("Document.xml", "fcstd"),
        #[cfg(feature = "f3d")]
        ("FusionAssetName[Active]/Breps.BlobParts/body.smbh", "f3d"),
        #[cfg(feature = "step")]
        ("ISO-10303.p21", "step"),
    ];
    for (member, format) in cases {
        for damaged in [false, true] {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file("preview.bin", options)
                .expect("ancillary member");
            writer
                .write_all(&vec![b'p'; 2 * 1024 * 1024])
                .expect("large preview");
            writer
                .start_file(member, options)
                .expect("root namespace member");
            writer
                .write_all(b"authored namespace evidence")
                .expect("member contents");
            let mut bytes = writer.finish().expect("archive").into_inner();
            if damaged {
                bytes[0] ^= 1;
            }
            let prefix = &bytes[..crate::DETECTION_PREFIX_LEN];
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 256 * 1024;
            policy.limits.max_collection_items = 32;
            let (ctx, root) =
                DecodeContext::from_root_bytes(prefix, &arena, &policy).expect("bounded prefix");
            let mut source = Cursor::new(&bytes);
            source
                .seek(SeekFrom::Start(cadmpeg_core::decode::u64_from_index(
                    prefix.len(),
                )))
                .expect("prefix position");
            let catalog = InputCatalog::with_builtins();
            let found = catalog
                .resolve_seekable_source(&ctx, root, &mut source, None)
                .expect("directory evidence resolves");
            let ResolvedSource::Native { codec, selection } = found else {
                panic!("native namespace must resolve");
            };
            assert_eq!(codec.id().as_str(), format);
            assert_eq!(
                selection,
                crate::Selection::Detected {
                    confidence: cadmpeg_ir::Confidence::Medium
                }
            );
            assert_eq!(
                source.position(),
                cadmpeg_core::decode::u64_from_index(prefix.len())
            );
            ctx.finish_session()
                .expect("probe preserves the resource policy");
        }
    }
}

#[test]
fn payload_markers_do_not_become_remote_directory_evidence() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "notes.txt",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .expect("member");
    writer
        .write_all(&vec![b'x'; 2 * 1024 * 1024])
        .expect("payload");
    writer
        .write_all(b"Document.xml ISO-10303.p21 FusionDocType body.smbh")
        .expect("payload markers");
    let mut bytes = writer.finish().expect("archive").into_inner();
    bytes[0] ^= 1;
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes[..crate::DETECTION_PREFIX_LEN], &arena, &policy)
            .expect("prefix");
    let catalog = InputCatalog::with_builtins();
    let mut source = Cursor::new(&bytes);
    assert!(matches!(
        catalog
            .resolve_seekable_source(&ctx, root, &mut source, None)
            .expect("probe"),
        ResolvedSource::Unrecognized
    ));
    ctx.finish_session().expect("session");
}

#[test]
fn directory_probe_reads_metadata_without_scanning_or_copying_a_large_payload() {
    struct ObservedSource {
        inner: Cursor<Vec<u8>>,
        read_bytes: usize,
    }
    impl Read for ObservedSource {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            let count = self.inner.read(output)?;
            self.read_bytes += count;
            Ok(count)
        }
    }
    impl Seek for ObservedSource {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(position)
        }
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "preview.bin",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .expect("member");
    writer
        .write_all(&vec![b'x'; 2 * 1024 * 1024])
        .expect("large payload");
    let mut bytes = writer.finish().expect("archive").into_inner();
    bytes[0] ^= 1;
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("session");
    let mut source = ObservedSource {
        inner: Cursor::new(bytes),
        read_bytes: 0,
    };
    let (image, storage) = cadmpeg_container::ArchiveSnapshot::detection_image(&ctx, &mut source)
        .expect("probe")
        .expect("metadata");
    assert!(image.len() < 1024);
    assert!(source.read_bytes < 70_000);
    assert_eq!(source.inner.position(), 0);
    assert!(cadmpeg_container::ArchiveSnapshot::contains_matching_name(
        &ctx,
        View::over_retained(&image),
        |name| name == "preview.bin"
    )
    .expect("namespace"));
    drop((image, storage));
    ctx.finish_session().expect("session");
}
