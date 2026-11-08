// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use super::super::{definition_catalog, page_logical, RECORD_MARKER, STREAM_HEADER_LEN};
use crate::test_support::lp_ascii;

#[test]
fn damaged_definition_record_keeps_readable_catalog_sibling() {
    let mut logical = RECORD_MARKER.to_vec();
    lp_ascii(&mut logical, "GenericSchema");
    let flag = logical.len();
    logical.push(0);
    lp_ascii(&mut logical, "Prism-001");
    lp_ascii(&mut logical, "Prism-001");
    logical.extend_from_slice(&2_u32.to_le_bytes());
    for value in ["Plastic/Thermoplastic", "Default", "Generated appearance"] {
        lp_ascii(&mut logical, value);
    }
    logical.extend_from_slice(&0_u32.to_le_bytes());
    logical.extend_from_slice(&1_u32.to_le_bytes());
    lp_ascii(&mut logical, "");
    let valid = page_logical(&logical).unwrap();
    logical[flag] = 2;
    let mut pages = page_logical(&logical).unwrap();
    pages.extend_from_slice(&valid[STREAM_HEADER_LEN..]);

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    zip.start_file("AssetData/DefinitionIteratorProperties.bin", stored)
        .unwrap();
    zip.write_all(&pages).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let mut parts =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    let catalog = definition_catalog(&ctx, root, "synthetic.protein", &mut parts).unwrap();
    assert_eq!(
        catalog.get(&("Prism-001".to_owned(), "GenericSchema".to_owned())),
        Some(&Some("Plastic/Thermoplastic".to_owned()))
    );
    let (_, native, _, losses) = parts.into_parts();
    assert_eq!(
        native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
        1
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    let provenance = losses[0].provenance.as_ref().unwrap();
    assert_eq!(
        provenance.stream_name(),
        Some("synthetic.protein/AssetData/DefinitionIteratorProperties.bin")
    );
    assert!(provenance.byte_len().is_some());
}
