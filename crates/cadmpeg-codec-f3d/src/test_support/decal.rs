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

