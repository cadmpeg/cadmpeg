// SPDX-License-Identifier: Apache-2.0
//! Archive scan and physical-ledger unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;
use zip::write::SimpleFileOptions;

#[test]
fn frames_zip64_streaming_descriptor_and_local_extra() {
    let bytes = streaming_archive_with_options(
        "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>",
        SimpleFileOptions::default().large_file(true),
    );
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("ZIP64 archive fits root policy");
    let scan = crate::container::scan(&ctx, root).expect("ZIP64 streaming ZIP");
    assert!(scan
        .ledger
        .iter()
        .any(|span| span.role == "local-extra" && span.end > span.start));
    let descriptor = scan
        .ledger
        .iter()
        .find(|span| span.role == "data-descriptor")
        .expect("ZIP64 descriptor");
    assert_eq!(descriptor.end - descriptor.start, 24);
}

#[test]
fn frames_streaming_data_descriptor_separately_from_padding() {
    let bytes = streaming_archive("<Document SchemaVersion=\"4\" FileVersion=\"1\"/>");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let scan = crate::container::scan(&ctx, root).expect("streaming ZIP");
    let descriptors = scan
        .ledger
        .iter()
        .filter(|span| span.role == "data-descriptor")
        .collect::<Vec<_>>();
    assert_eq!(descriptors.len(), 1);
    assert!(matches!(descriptors[0].end - descriptors[0].start, 16 | 24));
}

#[test]
pub(crate) fn rejects_unsafe_names() {
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let unsafe_name = archive_entries(&[("../Document.xml", xml), ("Document.xml", xml)]);
    let error = FcstdCodec
        .inspect(
            &mut Cursor::new(unsafe_name),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect_err("unsafe path must fail");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
}

#[test]
fn inspects_and_closes_physical_ledger() {
    let bytes = archive("<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\"><Object/></Document>");
    let archive_len = bytes.len() as u64;
    let summary = FcstdCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("inspect");
    assert_eq!(summary.format, "fcstd");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=4"));
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .expect("decode");
    assert!(result.report().losses.is_empty());
    let ledger = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ArchiveSpan>("physical_ledger")
        .expect("ledger");
    assert_eq!(ledger.first().map(|span| span.start), Some(0));
    assert_eq!(ledger.last().map(|span| span.end), Some(archive_len));
    assert!(ledger.windows(2).all(|pair| pair[0].end == pair[1].start));
    assert!(crate::validate_native(result.ir()).is_empty());
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
