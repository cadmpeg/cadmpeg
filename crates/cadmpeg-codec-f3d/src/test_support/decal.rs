// SPDX-License-Identifier: Apache-2.0
//! Synthetic decal record fixture.

use crate::records::decal::DesignDecalMappingMode;
use crate::test_support::write_marked_reference;

pub(crate) fn header(bytes: &mut [u8], at: usize, tag: [u8; 3], index: u32) {
    bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[at + 4..at + 7].copy_from_slice(&tag);
    bytes[at + 7..at + 11].copy_from_slice(&index.to_le_bytes());
}

pub(crate) fn fixture() -> (Vec<u8>, usize, crate::metastream::MetaStream) {
    let mut bytes = vec![0; 240];
    let asset_at = 0;
    let name_at = 30;
    let scope_at = 71;
    let end_at = 200;
    header(&mut bytes, asset_at, *b"258", 17);
    write_marked_reference(&mut bytes, asset_at + 19, 50);
    header(&mut bytes, name_at, *b"279", 18);
    let name = "mark.png".encode_utf16().collect::<Vec<_>>();
    bytes[name_at + 21..name_at + 25]
        .copy_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
    for (ordinal, unit) in name.into_iter().enumerate() {
        let at = name_at + 25 + ordinal * 2;
        bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
    }
    header(&mut bytes, scope_at, *b"301", 23);
    write_marked_reference(&mut bytes, scope_at + 21, 17);
    bytes[scope_at + 32] = DesignDecalMappingMode::FitToFaces.code();
    write_marked_reference(&mut bytes, scope_at + 33, 24);
    header(&mut bytes, 150, *b"440", 17);
    header(&mut bytes, end_at, *b"302", 23);
    let metadata =
        crate::test_support::zip_test::indexed_fixture_metastream(&bytes, &[17, 18, 23], &[23]);
    let meta = crate::metastream::parse(&metadata, "Design/MetaStream.dat").unwrap();
    (bytes, scope_at, meta)
}

/// Build an archive with an indexed Decal scope and image asset.
pub(crate) fn archive(brep: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Write};

    let (mut bulk, scope_at, _) = fixture();
    bulk.truncate(scope_at + crate::layout::design_decal_scope_prefix::LEN);
    bulk.extend_from_slice(&2_u32.to_le_bytes());
    crate::test_support::push_reference_u64(&mut bulk, 24);
    crate::test_support::push_reference_u64(&mut bulk, 40);
    bulk.extend_from_slice(&u32::MAX.to_le_bytes());
    crate::test_support::lp_utf16(&mut bulk, "Decal");
    let tail_start = bulk.len();
    bulk.resize(tail_start + 78, 0);
    bulk[tail_start..tail_start + 4].copy_from_slice(&1_u32.to_le_bytes());
    let paired = bulk.len();
    bulk.resize(paired + 19, 0);
    header(&mut bulk, paired, *b"302", 23);
    let group = bulk.len();
    bulk.resize(group + 21, 0);
    header(&mut bulk, group, *b"303", 24);
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    crate::test_support::push_reference_u64(&mut bulk, 40);
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    bulk.extend_from_slice(&0x0000_0004_0000_0000_u64.to_le_bytes());
    bulk.extend_from_slice(&[0; 10]);
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    bulk.extend_from_slice(&1.0_f64.to_le_bytes());
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    crate::test_support::push_reference_u64(&mut bulk, 26);
    bulk.extend_from_slice(&[0, 0]);
    crate::test_support::push_reference_u64(&mut bulk, 25);
    bulk.push(0);
    crate::test_support::push_reference_u64(&mut bulk, 23);
    let paired_group = bulk.len();
    bulk.resize(paired_group + 11, 0);
    header(&mut bulk, paired_group, *b"304", 24);
    let operand = bulk.len();
    bulk.resize(operand + 21, 0);
    header(&mut bulk, operand, *b"305", 40);
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    bulk.extend_from_slice(&985_u64.to_le_bytes());
    bulk.extend_from_slice(&3_u32.to_le_bytes());
    crate::test_support::push_reference_u64(&mut bulk, 43);
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    crate::test_support::lp_utf16(&mut bulk, "11111111-2222-3333-4444-555555555555");
    crate::test_support::lp_utf16(&mut bulk, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    bulk.extend_from_slice(&2_u32.to_le_bytes());
    bulk.extend_from_slice(&[0; 4]);
    let paired_operand = bulk.len();
    bulk.resize(paired_operand + 11, 0);
    header(&mut bulk, paired_operand, *b"306", 40);
    for (id, tag) in [
        (41, *b"307"),
        (42, *b"308"),
        (43, *b"309"),
        (44, *b"310"),
        (50, *b"311"),
    ] {
        let at = bulk.len();
        bulk.resize(at + 11, 0);
        header(&mut bulk, at, tag, id);
        if id == 43 {
            crate::test_support::lp_ascii(&mut bulk, "43");
            bulk.extend_from_slice(&44_u32.to_le_bytes());
            bulk.extend_from_slice(&[0; 12]);
            crate::test_support::lp_ascii(&mut bulk, "body_recipe_data");
        }
    }
    let meta = crate::test_support::zip_test::indexed_fixture_metastream(
        &bulk,
        &[17, 18, 23, 24, 40, 41, 42, 43, 44, 50],
        &[23, 24, 40],
    );
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    for (name, bytes) in [
        (
            "FusionAssetName[Active]/Design1/BulkStream.dat",
            bulk.as_slice(),
        ),
        (
            "FusionAssetName[Active]/Design1/MetaStream.dat",
            meta.as_slice(),
        ),
        (
            "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
            brep,
        ),
        (
            "FusionAssetName[Active]/Design1/Images.BlobParts/mark.png",
            b"PNG".as_slice(),
        ),
    ] {
        zip.start_file(name, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
