// SPDX-License-Identifier: Apache-2.0
//! Preserve the historical cube input independently of current header authoring.

use std::io::{Cursor, Read, Write};

use cadmpeg_ir::examples;

use crate::test_support::native_test::TestEncode;
use crate::F3dCodec;

const MEMBER: &str = "FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh";
// docs/layouts/asm.toml, asmheader_binaryfile8.entity_count.
const ENTITY_COUNT_OFFSET: usize = 31;

fn current_cube() -> Vec<u8> {
    let ir = examples::unit_cube().expect("unit cube fixture is admitted");
    let mut bytes = Vec::new();
    F3dCodec
        .encode(&ir, &mut bytes)
        .expect("encode neutral unit-cube IR");
    bytes
}

pub(super) fn fixture() -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(current_cube())).unwrap();
    let mut historical = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut changed = 0;
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload).unwrap();
        if name == MEMBER {
            // The frozen input predates the corrected first-owned-index word
            // (docs/formats/asm.md §1, header invariants). Its original header
            // stated 7 even though asmheader #0/body #1 first owns region #2.
            // Keep this exact historical source quirk, not a production rule.
            let original = payload.clone();
            let start = ENTITY_COUNT_OFFSET;
            let end = start + std::mem::size_of::<u64>();
            assert_eq!(&payload[start..end], &2u64.to_le_bytes());
            payload[start..end].copy_from_slice(&7u64.to_le_bytes());
            assert_eq!(&payload[..start], &original[..start]);
            assert_eq!(&payload[end..], &original[end..]);
            changed += 1;
        }
        historical.start_file(name, options).unwrap();
        historical.write_all(&payload).unwrap();
    }
    assert_eq!(changed, 1);
    historical.finish().unwrap().into_inner()
}

#[test]
fn current_cube_header_states_the_actual_first_owned_record() {
    let mut archive = zip::ZipArchive::new(Cursor::new(current_cube())).unwrap();
    let mut payload = Vec::new();
    archive
        .by_name(MEMBER)
        .unwrap()
        .read_to_end(&mut payload)
        .unwrap();
    let header = cadmpeg_asm::asm_header::parse(&payload).unwrap();
    assert_eq!(header.metadata.entity_count, Some(2));
}
