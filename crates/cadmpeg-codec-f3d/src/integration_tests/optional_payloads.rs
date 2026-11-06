// SPDX-License-Identifier: Apache-2.0
//! Optional ZIP assets do not own BREP admission or native source replay.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_configuration;
use crate::F3dCodec;

#[test]
fn damaged_first_optional_frame_keeps_detection_and_required_brep_admission() {
    use std::io::{Read as _, Write as _};
    let optional = "FusionAssetName[Active]/Previews/thumbnail.png";
    let original = f3d_with_configuration(&synthetic_geometry_smbh(), optional, b"preview");
    let baseline = super::decode(original.clone());
    let mut source = zip::ZipArchive::new(Cursor::new(original)).unwrap();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let names = std::iter::once(optional.to_owned())
        .chain(
            source
                .file_names()
                .map(|name| name.expect("entry name"))
                .filter(|name| *name != optional)
                .map(std::borrow::Cow::into_owned),
        )
        .collect::<Vec<_>>();
    for name in names {
        let mut payload = Vec::new();
        source
            .by_name(&name)
            .unwrap()
            .read_to_end(&mut payload)
            .unwrap();
        writer.start_file(name, options).unwrap();
        writer.write_all(&payload).unwrap();
    }
    let mut changed = writer.finish().unwrap().into_inner();
    changed[0] ^= 1;
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&F3dCodec, &changed),
        cadmpeg_ir::Confidence::Medium
    );
    let decoded = super::decode(changed);
    assert_eq!(decoded.ir().model, baseline.ir().model);
    super::assert_valid(&decoded);
    assert!(!decoded.report().losses.is_empty());
    assert!(<F3dCodec as CodecBackend>::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir(),
        None,
    )
    .unwrap()
    .is_empty());

    let mut unrelated = zip::ZipWriter::new(Cursor::new(Vec::new()));
    unrelated.start_file("notes.txt", options).unwrap();
    unrelated
        .write_all(b"FusionDocType Breps.BlobParts .smbh")
        .unwrap();
    let mut unrelated = unrelated.finish().unwrap().into_inner();
    unrelated[0] ^= 1;
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&F3dCodec, &unrelated),
        cadmpeg_ir::Confidence::No
    );
}

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
    let payload = usize::try_from(entry.data_start.expect("readable frame")).unwrap();
    drop(ctx);
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
fn unreadable_assets_keep_geometry_and_native_write_omits_unrepresented_payloads() {
    for name in [
        "FusionAssetName[Active]/Previews/thumbnail.png",
        "FusionAssetName[Active]/Images.BlobParts/image.png",
        "FusionAssetName[Active]/OGS.BlobFolder/cache.dat",
        "FusionAssetName[Active]/ProteinAssets.BlobParts/asset.protein",
        "FusionAssetName[Active]/ParaMeshGeometry.BlobParts/asset.paramesh",
        "Properties.dat",
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
                decoded.ir(),
                None,
            )
            .unwrap()
            .is_empty());
            assert!(decoded.report().losses.iter().any(|loss| {
                loss.code
                    == cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                        .note(String::new())
                        .code
                    && loss.severity == cadmpeg_ir::report::Severity::Error
                    && loss.provenance.is_some()
            }));
            assert!(decoded
                .source_fidelity()
                .retained_records()
                .values()
                .any(|record| record.data() == Some(changed.as_slice())));
            let mut replayed = Vec::new();
            let plan = F3dCodec
                .plan(
                    cadmpeg_ir::codec::write::EncodeInput::new(
                        decoded.ir(),
                        Some(decoded.source_fidelity()),
                    ),
                    cadmpeg_ir::codec::write::target::TargetRequest::Inherit,
                )
                .unwrap();
            assert!(matches!(
                plan.report().write_path(),
                cadmpeg_ir::report::export::WritePath::Synthesized { .. }
            ));
            plan.write_to(&mut replayed).unwrap();
            assert_ne!(replayed, changed);
            let output = super::decode(replayed);
            assert_eq!(
                output.ir().model.bodies.len(),
                decoded.ir().model.bodies.len()
            );
            assert_eq!(
                output.ir().model.points[0].position(),
                decoded.ir().model.points[0].position()
            );
        }
    }
}

#[test]
fn damaged_brep_payload_is_given_up_as_one_part() {
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
        let decoded = F3dCodec
            .decode(&mut Cursor::new(changed), &DecodeOptions::default())
            .unwrap();
        assert!(decoded.ir().model.bodies.is_empty());
        assert_eq!(decoded.ir().native_unknowns("f3d").unwrap().len(), 1);
        let losses: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code
                    == cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                        .note("")
                        .code
            })
            .collect();
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains(name));
    }
}

#[test]
fn optional_asset_part_limit_preserves_the_model() {
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let options = DecodeOptions {
        policy,
        ..DecodeOptions::default()
    };
    let decoded = F3dCodec.decode(&mut Cursor::new(bytes), &options).unwrap();
    assert!(!decoded.ir().model.bodies.is_empty());
    assert_eq!(decoded.ir().native_unknowns("f3d").unwrap().len(), 1);
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code
                == cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                    .note("")
                    .code
        })
        .collect();
    let [loss] = losses.as_slice() else {
        panic!("expected one unreadable ZIP entry");
    };
    assert!(loss.message.contains("thumbnail.png"));
    assert!(loss.message.contains("zip.entry"));
}

#[test]
fn unreadable_protein_entry_has_one_loss_for_the_entry() {
    let name = "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein";
    let mut bytes = crate::test_support::zip_test::f3d_with_smbh_and_protein_guids(
        &synthetic_geometry_smbh(),
        &["11111111-2222-3333-4444-555555555555"],
    );
    let baseline = super::decode(bytes.clone());
    assert_eq!(baseline.ir().model.appearances.len(), 1);
    assert!(baseline.ir().model.appearances[0].base_color.is_some());
    damage_entry(&mut bytes, name, Defect::Crc);
    let damaged = super::decode(bytes);
    assert_eq!(damaged.ir().model.appearances.len(), 1);
    assert!(damaged.ir().model.appearances[0].base_color.is_none());
    assert_eq!(
        damaged.ir().model.appearances[0].visual_guid,
        baseline.ir().model.appearances[0].visual_guid
    );
    assert_eq!(damaged.ir().native_unknowns("f3d").unwrap().len(), 1);
    let new_losses: Vec<_> = damaged
        .report()
        .losses
        .iter()
        .filter(|loss| {
            !baseline
                .report()
                .losses
                .iter()
                .any(|old| old.code == loss.code)
        })
        .collect();
    let [loss] = new_losses.as_slice() else {
        panic!("expected one new loss for the unreadable entry");
    };
    assert_eq!(
        loss.code,
        cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
            .note("")
            .code
    );
    assert!(loss.message.contains(name));
}

#[test]
fn textured_mesh_assets_have_no_native_back_links() {
    let input = crate::test_support::mesh_fixture::archive_with_textures(
        &crate::test_support::paramesh::triangle(),
        Some(&synthetic_geometry_smbh()),
        true,
    );
    let decoded = super::decode(input);
    assert_eq!(decoded.ir().model.assets.len(), 2);
    assert_eq!(decoded.ir().model.tessellations.len(), 1);
    super::assert_valid(&decoded);
    let findings = <F3dCodec as CodecBackend>::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir(),
        None,
    )
    .unwrap();
    assert!(findings.is_empty(), "{findings:#?}");
    cadmpeg_test_support::native_linkage::assert_native_linkage(decoded.ir());
}
