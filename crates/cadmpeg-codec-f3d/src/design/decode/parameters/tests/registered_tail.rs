// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use super::super::decode_parameters;
use crate::design::test_support::parameter_record;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::streams_test::design_metastream_with_records;
use crate::test_support::zip_test::with_scan;

#[test]
fn parameter_registered_tail_resolves_full_record_identity_and_refuses_wrong_type() {
    for target in [181u64, 513] {
        for (type_guid, discriminator, admitted, unparsed) in [
            (super::super::PARAMETER_TAIL_TYPE_GUID, 5u64, true, 0),
            (super::super::PARAMETER_TAIL_TYPE_GUID, 9u64, false, 1),
            ("00000000-0000-0000-0000-000000000000", 5u64, false, 0),
        ] {
            let mut frame =
                parameter_record(None, "3 mm", "User Parameter", Some("mm"), "Width", 0.3);
            frame[22..30].copy_from_slice(&discriminator.to_le_bytes());
            let tail = frame.len() - 12;
            frame[tail + 2..tail + 10].copy_from_slice(&target.to_le_bytes());
            let metadata = design_metastream_with_records(
                &[(
                    type_guid,
                    "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
                    3,
                    "CommonData",
                    &[target],
                )],
                &[],
            );
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
            write_synthetic_manifests(&mut zip, stored);
            for (name, payload) in [("BulkStream.dat", frame), ("MetaStream.dat", metadata)] {
                zip.start_file(format!("FusionAssetName[Active]/Design1/{name}"), stored)
                    .unwrap();
                zip.write_all(&payload).unwrap();
            }
            let archive = zip.finish().unwrap().into_inner();
            let decoded = with_scan(&archive, decode_parameters).unwrap();
            assert_eq!(decoded.parameters.len(), usize::from(admitted));
            assert_eq!(
                decoded.unparsed.iter().map(|row| row.count).sum::<usize>(),
                unparsed
            );
            if admitted {
                assert_eq!(decoded.parameters[0].name(), "Width");
                assert_eq!(decoded.parameters[0].expression(), "3 mm");
                assert!(decoded.unparsed.is_empty());
            }
        }
    }
}
