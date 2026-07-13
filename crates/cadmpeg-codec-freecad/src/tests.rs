use std::io::{Cursor, Write};

use cadmpeg_ir::{Codec, Confidence, DecodeOptions};
use zip::write::SimpleFileOptions;

use crate::FcstdCodec;

fn archive(document: &str) -> Vec<u8> {
    archive_entries(&[("Document.xml", document.as_bytes())])
}

fn archive_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
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

#[test]
fn rejects_unsafe_names() {
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let unsafe_name = archive_entries(&[("../Document.xml", xml), ("Document.xml", xml)]);
    let error = FcstdCodec
        .inspect(&mut Cursor::new(unsafe_name))
        .expect_err("unsafe path must fail");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
}

#[test]
fn legacy_layout_is_inspectable_but_explicitly_refused_for_decode() {
    let bytes = archive("<Document SchemaVersion=\"3\" FileVersion=\"1\"/>");
    let summary = FcstdCodec
        .inspect(&mut Cursor::new(&bytes))
        .expect("legacy inspection");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=3"));
    let error = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect_err("legacy decode must fail");
    assert!(error
        .to_string()
        .contains("FCStd SchemaVersion=3 FileVersion=1 persistence layout"));
}

#[test]
fn thumbnail_bytes_are_retained_with_digest() {
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let bytes = archive_entries(&[("Document.xml", xml), ("thumbnails/Thumbnail.png", b"png")]);
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
            },
        )
        .expect("decode");
    let unknowns = result.ir.native_unknowns("fcstd").expect("unknowns");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].data.as_deref(), Some(b"png".as_slice()));
}

#[test]
fn detects_marker_but_not_arbitrary_zip() {
    assert_eq!(
        FcstdCodec.detect(&archive(
            "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>"
        )),
        Confidence::High
    );
    assert_eq!(FcstdCodec.detect(b"PK\x03\x04 unrelated"), Confidence::Low);
    assert_eq!(FcstdCodec.detect(b"not zip"), Confidence::No);
}

#[test]
fn inspects_and_closes_physical_ledger() {
    let bytes = archive("<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\"><Object/></Document>");
    let archive_len = bytes.len() as u64;
    let summary = FcstdCodec
        .inspect(&mut Cursor::new(&bytes))
        .expect("inspect");
    assert_eq!(summary.format, "fcstd");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=4"));
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
            },
        )
        .expect("decode");
    assert!(result.report.losses.is_empty());
    let ledger = result
        .ir
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ArchiveSpan>("physical_ledger")
        .expect("ledger");
    assert_eq!(ledger.first().map(|span| span.start), Some(0));
    assert_eq!(ledger.last().map(|span| span.end), Some(archive_len));
    assert!(ledger.windows(2).all(|pair| pair[0].end == pair[1].start));
    for role in [
        "local-signature",
        "local-fields",
        "local-name",
        "compressed-payload",
        "central-signature",
        "central-fields",
        "central-name",
        "end-record",
    ] {
        assert!(
            ledger.iter().any(|span| span.role == role),
            "missing {role}"
        );
    }
}
