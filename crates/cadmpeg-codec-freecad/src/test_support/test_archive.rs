// SPDX-License-Identifier: Apache-2.0
//! ZIP archive builders and corpus bytes used by `FreeCAD` unit tests.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

pub(crate) const CORE_DESIGN_PRODUCT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
));

pub(crate) fn assert_valid_document(ir: &cadmpeg_ir::CadIr) {
    let errors = cadmpeg_ir::validate_neutral(ir, Vec::new())
        .findings
        .into_iter()
        .filter(|finding| finding.severity >= cadmpeg_ir::Severity::Error)
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
