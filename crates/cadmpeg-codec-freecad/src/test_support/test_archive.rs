// SPDX-License-Identifier: Apache-2.0
//! ZIP archive builders and corpus bytes used by `FreeCAD` unit tests.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

pub(crate) const GEOMETRY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/geometry_topology.FCStd"
));

pub(crate) const CORE_DESIGN_PRODUCT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
));

pub(crate) const CORE_OPERATIONS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/core_operations.FCStd"
));

/// Repack `bytes` with `Document.xml`'s `SchemaVersion` set to `version` and
/// every other entry carried across verbatim.
///
/// The one edit is the attribute value, so a decode difference between the
/// original and the result is attributable to the declared version alone.
pub(crate) fn rewrite_schema_version(bytes: &[u8], version: &str) -> Vec<u8> {
    rewrite_entry(bytes, "Document.xml", |data| {
        let text = std::str::from_utf8(data).expect("Document.xml is UTF-8");
        let (start, rest) = text
            .split_once("SchemaVersion=\"")
            .expect("schema declaration");
        let (_, tail) = rest.split_once('"').expect("terminated declaration");
        format!("{start}SchemaVersion=\"{version}\"{tail}").into_bytes()
    })
}

/// Change or add one independent archive member while retaining every peer's bytes.
pub(crate) fn rewrite_entry(
    bytes: &[u8],
    target: &str,
    rewrite: impl Fn(&[u8]) -> Vec<u8>,
) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).expect("read FCStd archive");
    let mut entries = Vec::new();
    let mut found = false;
    for index in 0..source.len() {
        let mut entry = source.by_index(index).expect("archive entry");
        let name = entry.name().expect("entry name").into_owned();
        let mut data = Vec::new();
        std::io::copy(&mut entry, &mut data).expect("read entry");
        if name == target {
            data = rewrite(&data);
            found = true;
        }
        entries.push((name, data));
    }
    if !found {
        entries.push((target.to_owned(), rewrite(&[])));
    }
    archive_entries(
        &entries
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn assert_valid_document(ir: &cadmpeg_ir::CadIr) {
    let errors = cadmpeg_ir::validate_neutral(ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .into_iter()
        .filter(|finding| finding.severity >= cadmpeg_ir::report::Severity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");
}

pub(crate) fn archive(document: &str) -> Vec<u8> {
    archive_entries(&[("Document.xml", document.as_bytes())])
}

pub(crate) fn archive_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut bytes);
    for (name, data) in entries {
        zip.start_file(
            *name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .expect("start entry");
        zip.write_all(data).expect("write entry");
    }
    zip.finish().expect("finish ZIP");
    bytes.into_inner()
}

pub(crate) fn streaming_archive(document: &str) -> Vec<u8> {
    streaming_archive_with_options(document, SimpleFileOptions::default())
}

pub(crate) fn streaming_archive_with_options(
    document: &str,
    options: SimpleFileOptions,
) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new_stream(Vec::new());
    zip.start_file(
        "Document.xml",
        options.compression_method(zip::CompressionMethod::Deflated),
    )
    .expect("start streamed entry");
    zip.write_all(document.as_bytes()).expect("write entry");
    zip.finish().expect("finish ZIP").into_inner()
}

/// Authored optional-member source with one independent broken DEFLATE stream.
pub(crate) fn broken_optional_member_archive() -> Vec<u8> {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    zip_fixture::archive(&[
        RawEntry::stored("Document.xml", b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>"),
        RawEntry::compressed("broken.bin", zip_fixture::DEFLATE, b"\x07", b"not decoded"),
        RawEntry::stored("peer.bin", b"peer source"),
    ])
}

/// Scanning represents actual declaration names and retained document metadata.
pub(crate) fn assert_only_document_file_version_capture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &crate::container::Scan<'_>,
) {
    let document = scan.data["Document.xml"];
    let expected = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    assert_eq!(document.window(), expected);
    assert_eq!(scan.document.file_version.as_str(), "1");
    let extent = ctx.capture_part_extent(document).unwrap();
    let member = scan
        .archive
        .entry("Document.xml")
        .unwrap()
        .stored_range()
        .unwrap();
    assert_eq!(
        member.end - member.start,
        u64::try_from(expected.len()).unwrap()
    );
    let streams = ctx.source_streams().unwrap();
    let source = streams
        .iter()
        .find(|source| source.name() == extent.stream())
        .unwrap();
    assert_eq!(
        source.origin(),
        &cadmpeg_core::decode::SourceOrigin::Alias {
            parent: "file".into(),
            extent: member,
        }
    );
    let captures = ctx.take_record_captures().unwrap();
    let mut expected_ids = scan
        .entry_records
        .iter()
        .map(|entry| entry.id().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    expected_ids.insert(crate::native::native_id("document", "0"));
    assert_eq!(
        captures
            .iter()
            .map(|capture| capture.id().to_owned())
            .collect::<std::collections::BTreeSet<_>>(),
        expected_ids
    );
    assert_eq!(captures.len(), expected_ids.len());
    let capture = captures
        .iter()
        .find(|capture| capture.id() == crate::native::native_id("document", "0"))
        .expect("document producer");
    assert_eq!(capture.stream(), extent.stream());
    assert_eq!(capture.offset(), extent.offset());
    assert_eq!(capture.byte_len(), u64::try_from(expected.len()).unwrap());
    let mut from = 0;
    let reads = [b"Document".as_slice(), b"FileVersion", b"1"].map(|field| {
        let start = from
            + expected[from..]
                .windows(field.len())
                .position(|part| part == field)
                .unwrap();
        from = start + field.len();
        u64::try_from(start).unwrap()..u64::try_from(from).unwrap()
    });
    assert_eq!(
        capture.reads(),
        &std::collections::BTreeMap::from([
            (
                extent.stream().to_owned(),
                reads
                    .iter()
                    .map(|range| extent.offset() + range.start..extent.offset() + range.end)
                    .collect()
            ),
            (
                "file".to_owned(),
                reads
                    .iter()
                    .map(|range| member.start + range.start..member.start + range.end)
                    .collect()
            ),
        ])
    );
    let frames = scan.archive.member_frames(ctx).unwrap();
    for (entry, frame) in scan.archive.entries().iter().zip(frames) {
        if !scan
            .entry_records
            .iter()
            .any(|kept| kept.name() == entry.name)
        {
            assert!(!captures
                .iter()
                .any(|capture| capture.id() == crate::native::native_id("entry", &entry.name)));
            continue;
        }
        let frame = frame.expect("fixture local header");
        let capture = captures
            .iter()
            .find(|capture| capture.id() == crate::native::native_id("entry", &entry.name))
            .expect("actual entry producer");
        let name = scan.archive.entry_name_source(entry).unwrap();
        assert_eq!(capture.stream(), "file");
        assert_eq!(
            capture.offset(),
            u64::try_from(frame.header().start()).unwrap()
        );
        assert_eq!(
            capture.byte_len(),
            u64::try_from(frame.header().window().len()).unwrap()
        );
        assert_eq!(
            capture.reads(),
            &std::collections::BTreeMap::from([(
                "file".to_owned(),
                vec![u64::try_from(name.start()).unwrap()..u64::try_from(name.end()).unwrap()]
            )])
        );
    }
}
