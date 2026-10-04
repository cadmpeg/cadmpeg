// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;
use std::io::{Cursor, Write};
use zip::CompressionMethod;

#[test]
fn body_member_marker_prefix_copy_refuses_work() {
    const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = Vec::new();
    bulk.extend_from_slice(&10_u32.to_le_bytes());
    bulk.extend_from_slice(b"BodiesRoot");
    bulk.extend_from_slice(&0_u16.to_le_bytes());
    bulk.extend_from_slice(&10_u32.to_le_bytes());
    bulk.extend_from_slice(b"BodiesRoot");
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    bulk.push(1);
    bulk.extend_from_slice(&9_u64.to_le_bytes());
    bulk.extend_from_slice(&2_u16.to_le_bytes());
    bulk.push(0);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let members = crate::design::test_support::with_test_decode_context(|ctx| {
            super::super::decode_body_members(ctx, scan)
        })
        .expect("valid one-member BodiesRoot input");
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].entity_suffix, 9);
        assert_eq!(members[0].flags, 2);

        for (skip, additional) in [(0, 8), (1, 20), (2, 8), (3, 4), (4, 8), (5, 16), (6, 20)] {
            let refusal = crate::test_support::resource_refusal_at(
                ResourceDimension::WorkUnits,
                "build F3D body-member marker prefix",
                skip,
                |ctx| super::super::decode_body_members(ctx, scan).map(|_| ()),
            );
            assert!(matches!(
                refusal,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::WorkUnits
                        && limit.operation == "build F3D body-member marker prefix"
                        && limit.additional == additional
            ));
        }
    });
}
