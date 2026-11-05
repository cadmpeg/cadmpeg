// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use crate::design::test_support::{parameter_owner_frame, parameter_record};
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::streams_test::design_metastream_with_records;

#[test]
fn decoded_parameter_records_have_their_primary_type_guids() {
    const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    const OWNER: &str = "D91D429C-1BB5-4640-B49F-69AF14F463E9";
    const PARAMETER: &str = "7A6A3D31-BE74-4E19-9115-20642944C2E4";
    const COMPANION: &str = "00000000-0000-0000-0000-000000000001";

    let mut bulk = parameter_owner_frame();
    bulk[4..7].copy_from_slice(b"256");
    let parameter_offset = bulk.len();
    let mut parameter = parameter_record(Some(44), "6 cm", "Distance", Some("cm"), "distance", 6.0);
    parameter[4..7].copy_from_slice(b"257");
    parameter[7..11].copy_from_slice(&45u32.to_le_bytes());
    bulk.extend_from_slice(&parameter);
    let companion_offset = bulk.len();
    let mut companion = vec![0; 58];
    companion[0..4].copy_from_slice(&3u32.to_le_bytes());
    companion[4..7].copy_from_slice(b"258");
    companion[7..11].copy_from_slice(&46u32.to_le_bytes());
    companion[31] = 1;
    companion[32..36].copy_from_slice(&44u32.to_le_bytes());
    companion[42..50].copy_from_slice(&1u64.to_le_bytes());
    bulk.extend_from_slice(&companion);
    let metadata = design_metastream_with_records(
        &[
            (OWNER, "", 1, "Fusion", &[44]),
            (PARAMETER, "", 1, "Fusion", &[45]),
            (COMPANION, "", 1, "Fusion", &[46]),
        ],
        &[
            (44, 0),
            (45, u64::try_from(parameter_offset).unwrap()),
            (46, u64::try_from(companion_offset).unwrap()),
        ],
    );
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(STREAM, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let mut output =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    let mut parameters =
        super::super::decode_parameters(&ctx, &scan, &mut output, &mut Default::default()).unwrap();
    let mut owners =
        super::super::decode_parameter_owners(&ctx, &scan, &mut parameters, &mut output).unwrap();
    let companions = super::super::decode_parameter_companions(
        &ctx,
        &scan,
        &mut owners,
        &mut parameters,
        &mut output,
    )
    .unwrap();
    assert_eq!(
        (parameters.len(), owners.len(), companions.len()),
        (1, 1, 1)
    );
    let meta = crate::design::decode::meta::metadata_for_bulk_stream(&scan, STREAM)
        .unwrap()
        .unwrap();
    let (frames, faults) =
        crate::design::decode::meta::partition_primary_frames(&bulk, &meta).unwrap();
    assert!(faults.is_empty());
    let types: Vec<_> = frames
        .iter()
        .map(|frame| (frame.entity_id, frame.design_type.type_guid.as_str()))
        .collect();
    assert_eq!(
        types,
        [
            (u64::from(owners[0].record_index()), OWNER),
            (u64::from(parameters[0].record_index), PARAMETER),
            (u64::from(companions[0].record_index()), COMPANION)
        ]
    );
    assert!(output.into_parts().3.is_empty());
}
