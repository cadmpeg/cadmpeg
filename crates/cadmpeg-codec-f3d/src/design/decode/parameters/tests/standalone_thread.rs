// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::streams_fixture::design_metastream_with_records;

/// Owners decoded from one Thread scope (record 12) and one standalone scalar
/// owner frame (record 44) that stores `local_ordinal`.
fn decode_thread_owner(
    local_ordinal: u32,
) -> Vec<crate::records::parameters::DesignParameterOwner> {
    let mut bulk = Vec::new();
    crate::test_support::indexed_header(&mut bulk, *b"256", 12);
    let owner_at = u64::try_from(bulk.len()).unwrap();
    let mut owner = parameter_owner_frame();
    owner[4..7].copy_from_slice(b"257");
    owner[35..39].copy_from_slice(&local_ordinal.to_le_bytes());
    bulk.extend_from_slice(&owner);
    let paired_at = u64::try_from(bulk.len()).unwrap();
    crate::test_support::indexed_header(&mut bulk, *b"256", 44);
    let mut meta = design_metastream_with_records(
        &[
            (
                "584D9526-EF22-4F6F-9FE1-5FCF1F3CE575",
                "",
                1,
                "Fusion",
                &[12],
            ),
            (
                "D91D429C-1BB5-4640-B49F-69AF14F463E9",
                "B24D5130-8751-4272-8143-D230D8757873",
                0,
                "Fusion",
                &[44],
            ),
        ],
        &[(12, 0), (44, owner_at)],
    );
    meta.truncate(meta.len() - 20);
    meta.extend_from_slice(&1u32.to_le_bytes());
    meta.extend_from_slice(&44u64.to_le_bytes());
    meta.extend_from_slice(&paired_at.to_le_bytes());
    meta.extend_from_slice(&[0; 16]);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    for (name, bytes) in [
        ("FusionAssetName[Active]/Design1/MetaStream.dat", meta),
        ("FusionAssetName[Active]/Design1/BulkStream.dat", bulk),
    ] {
        zip.start_file(name, stored).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let archive = zip.finish().unwrap().into_inner();
    with_scan(&archive, |scan| {
        super::super::decode_parameter_owners(scan, &[], &[])
    })
    .unwrap()
    .owners
}

#[test]
fn thread_scalar_owner_decodes_without_a_dimension_parameter() {
    let owners = decode_thread_owner(1);
    let [owner] = owners.as_slice() else {
        panic!("one standalone owner")
    };
    assert_eq!(owner.record_index(), 44);
    assert_eq!(owner.scope_record_index(), 12);
    assert_eq!(owner.local_ordinal(), 1);
    assert_eq!(owner.evaluated_value().get(), 6.0);
}

#[test]
fn thread_scalar_owner_past_the_length_ordinal_is_not_admitted() {
    assert!(decode_thread_owner(2).is_empty());
}
