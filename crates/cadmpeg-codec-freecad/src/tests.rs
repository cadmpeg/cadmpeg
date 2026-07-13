use std::io::{Cursor, Write};

use cadmpeg_ir::{Codec, Confidence, DecodeOptions};
use zip::write::SimpleFileOptions;

use crate::FcstdCodec;

fn archive(document: &str) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut bytes);
    zip.start_file("Document.xml", SimpleFileOptions::default())
        .expect("start XML");
    zip.write_all(document.as_bytes()).expect("write XML");
    zip.finish().expect("finish ZIP");
    bytes.into_inner()
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
}
