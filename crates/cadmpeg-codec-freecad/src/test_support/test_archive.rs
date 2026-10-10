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
        let name = entry.name().to_owned();
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
        .expect("resource allocation did not fail")
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
