// SPDX-License-Identifier: Apache-2.0
//! Optional ZIP assets do not own BREP admission or native source replay.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_configuration;
use crate::F3dCodec;

#[derive(Clone, Copy)]
enum Defect {
    Crc,
    Encryption,
    UnsupportedCompression,
    InvalidDeflate,
}

fn damage_entry(bytes: &mut [u8], name: &str, defect: Defect) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let entry = archive.entry(name).unwrap();
    let central = usize::try_from(entry.central_start).unwrap();
    let local = usize::try_from(entry.header_start).unwrap();
    let payload = usize::try_from(entry.data_start).unwrap();
    match defect {
        Defect::Crc => bytes[payload] ^= 1,
        Defect::Encryption => {
            for offset in [central + 8, local + 6] {
                let flags = View::u16_le_at(bytes, offset).unwrap() | 1;
                bytes[offset..offset + 2].copy_from_slice(&flags.to_le_bytes());
            }
        }
        Defect::UnsupportedCompression | Defect::InvalidDeflate => {
            let method = if matches!(defect, Defect::InvalidDeflate) {
                8_u16
            } else {
                12
            };
            for offset in [central + 10, local + 8] {
                bytes[offset..offset + 2].copy_from_slice(&method.to_le_bytes());
            }
            if matches!(defect, Defect::InvalidDeflate) {
                // BFINAL=1, BTYPE=3 is forbidden by the DEFLATE grammar.
                bytes[payload] = 7;
            }
        }
    }
}

#[test]
fn unreadable_assets_preserve_the_complete_model_and_exact_archive() {
    for name in [
        "FusionAssetName[Active]/Previews/thumbnail.png",
        "FusionAssetName[Active]/Images.BlobParts/image.png",
        "FusionAssetName[Active]/OGS.BlobFolder/cache.dat",
    ] {
        let original = f3d_with_configuration(&synthetic_geometry_smbh(), name, b"preview");
        let expected = super::decode(original.clone());
        assert_eq!(expected.ir().model.bodies.len(), 1);
        for defect in [
            Defect::Crc,
            Defect::Encryption,
            Defect::UnsupportedCompression,
            Defect::InvalidDeflate,
        ] {
            let mut changed = original.clone();
            damage_entry(&mut changed, name, defect);
            let decoded = super::decode(changed.clone());
            assert_eq!(decoded.ir().model, expected.ir().model);
            super::assert_valid(&decoded);
            assert!(<F3dCodec as CodecBackend>::validate_native(
                &cadmpeg_test_support::service_decode_context(),
                decoded.ir()
            )
            .unwrap()
            .is_empty());
            assert!(decoded.report().losses.iter().any(|loss| loss.code
                == crate::loss::F3dLossCode::ArchiveEntryUnreadable
                    .note(String::new())
                    .code));
            assert!(decoded
                .source_fidelity()
                .retained_records()
                .values()
                .any(|record| record.data() == Some(changed.as_slice())));
            let mut replayed = Vec::new();
            crate::test_support::plan_inherited_write(
                decoded.ir(),
                decoded.source_fidelity(),
                &mut replayed,
            )
            .unwrap();
            assert_eq!(replayed, changed);
        }
    }
}

#[test]
fn required_brep_payload_defects_remain_fatal() {
    let name = "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh";
    let original = crate::test_support::zip_test::f3d_with_smbh(&synthetic_geometry_smbh());
    for defect in [
        Defect::Crc,
        Defect::Encryption,
        Defect::UnsupportedCompression,
        Defect::InvalidDeflate,
    ] {
        let mut changed = original.clone();
        damage_entry(&mut changed, name, defect);
        assert!(F3dCodec
            .decode(&mut Cursor::new(changed), &DecodeOptions::default())
            .is_err());
    }
}

#[test]
fn optional_asset_resource_limits_are_not_recovered() {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut writer, stored);
    writer
        .start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    writer.write_all(&synthetic_geometry_smbh()).unwrap();
    writer
        .start_file(
            "FusionAssetName[Active]/Previews/thumbnail.png",
            crate::zip_write::file_options(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(b"preview").unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = crate::container::scan(&ctx, root)
        .err()
        .expect("optional expansion must refuse");
    assert!(matches!(error, CodecError::ResourceLimit(_)));
}
