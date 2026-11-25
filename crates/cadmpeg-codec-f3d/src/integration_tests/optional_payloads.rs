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
    assert_eq!(decoded.ir().geometry, baseline.ir().geometry);
    assert_eq!(decoded.ir().model, baseline.ir().model);
    super::assert_valid(&decoded);
    assert!(!decoded.report().losses.is_empty());
    assert!(<F3dCodec as CodecBackend>::validate_native(
        &cadmpeg_core::decode::test_support::service_decode_context(),
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
        assert_eq!(expected.ir().geometry.bodies.len(), 1);
        for defect in [
            Defect::Crc,
            Defect::Encryption,
            Defect::UnsupportedCompression,
            Defect::InvalidDeflate,
        ] {
            let mut changed = original.clone();
            damage_entry(&mut changed, name, defect);
            let decoded = super::decode(changed.clone());
            assert_eq!(decoded.ir().geometry, expected.ir().geometry);
            assert_eq!(decoded.ir().model, expected.ir().model);
            super::assert_valid(&decoded);
            assert!(<F3dCodec as CodecBackend>::validate_native(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                decoded.ir(),
                None,
            )
            .unwrap()
            .is_empty());
            let losses: Vec<_> = decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
                .collect();
            assert_eq!(losses.len(), 1);
            assert!(losses[0]
                .message
                .starts_with(&format!("part archive-entry {name:?} stream=\"\" offset=")));
            let stubs = decoded.ir().native_unknowns("f3d").unwrap();
            let stub = stubs
                .iter()
                .find(|stub| losses[0].message.contains(&format!("record={}", stub.id)))
                .expect("the loss names the stub");
            // No product definition stands for this document, so no record
            // contains the entry.
            assert!(cadmpeg_ir::products::stated_definitions(
                &decoded.ir().model.product_definitions
            )
            .next()
            .is_none());
            assert!(stub.links.is_empty());
            let retained = decoded
                .source_fidelity()
                .retained_record(stub.id.as_str())
                .expect("the entry's stored bytes are named");
            let start = usize::try_from(retained.offset()).unwrap();
            let end = usize::try_from(retained.end_offset()).unwrap();
            // The source image holds the bytes once; the part keeps their digest.
            assert_eq!(retained.data(), None);
            assert_eq!(
                retained.sha256(),
                cadmpeg_ir::hash::digest::Sha256Digest::digest(&changed[start..end]).as_str()
            );
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
fn an_unreadable_asset_beside_a_stored_frame_root_links_to_the_source_root_definition() {
    let name = "FusionAssetName[Active]/Previews/thumbnail.png";
    let (bulk, meta) = crate::test_support::component_design::nested_local_component_design(&[]);
    let original = crate::test_support::body_map_test::f3d_with_breps_design_and_assets(
        &[("BREP.module.smbh", &synthetic_geometry_smbh())],
        &bulk,
        &meta,
        &[(name, b"preview")],
    );
    let expected = super::decode(original.clone());
    // §8.1: the root component is the root definition, and the stored-frame
    // occurrence of the other body-owning definition sits in it.
    let model = &expected.ir().model;
    let root = model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .expect("a root definition");
    let source_root = root.id.as_str().to_owned();
    let stored = root
        .occurrences
        .iter()
        .filter_map(|id| {
            model
                .occurrences
                .iter()
                .find(|occurrence| &occurrence.id == id)
        })
        .filter(|occurrence| crate::design::components::is_stored_frame_occurrence(occurrence))
        .map(|occurrence| occurrence.prototype.as_str().to_owned())
        .collect::<Vec<_>>();
    let Ok([stored_root]) = <[String; 1]>::try_from(stored) else {
        panic!("one stored-frame occurrence: {:?}", model.occurrences);
    };
    assert_ne!(source_root, stored_root, "two root definitions");

    let mut changed = original;
    damage_entry(&mut changed, name, Defect::Crc);
    let decoded = super::decode(changed);
    assert_eq!(decoded.ir().model, expected.ir().model);
    let losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
        .collect::<Vec<_>>();
    let [loss] = losses.as_slice() else {
        panic!("one record.unreadable loss: {losses:?}");
    };
    let stubs = decoded.ir().native_unknowns("f3d").unwrap();
    let stub = stubs
        .iter()
        .find(|stub| loss.message.contains(&format!("record={}", stub.id)))
        .expect("the loss names the stub");
    let links = stub
        .links
        .iter()
        .map(cadmpeg_ir::ids::Identity::as_str)
        .collect::<Vec<_>>();
    assert_eq!(links, [source_root.as_str()]);
    super::assert_valid(&decoded);
}

/// A binary B-rep member whose payload does not open is given up alone: the
/// other member's body decodes as it does from the intact archive, a loss
/// names the member, and a strict decode refuses that loss.
#[test]
fn a_brep_payload_that_does_not_open_is_given_up_alone() {
    use std::io::Write as _;
    let kept = "FusionAssetName[Active]/Breps.BlobParts/BREP.kept.smbh";
    let name = "FusionAssetName[Active]/Breps.BlobParts/BREP.lost.smbh";
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut writer, stored);
    for entry in [kept, name] {
        writer.start_file(entry, stored).unwrap();
        writer.write_all(&synthetic_geometry_smbh()).unwrap();
    }
    let original = writer.finish().unwrap().into_inner();
    let intact = super::decode(original.clone());
    assert_eq!(intact.ir().geometry.bodies.len(), 2);
    let mut damaged = [
        Defect::Crc,
        Defect::Encryption,
        Defect::UnsupportedCompression,
        Defect::InvalidDeflate,
    ]
    .map(|defect| {
        let mut changed = original.clone();
        damage_entry(&mut changed, name, defect);
        changed
    })
    .to_vec();
    // An archive cut inside its last member keeps no central directory, and
    // the member's local payload runs past the end of the file.
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&original, &arena, &policy).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let payload = usize::try_from(archive.entry(name).unwrap().data_start.unwrap()).unwrap();
    damaged.push(original[..=payload].to_vec());
    for changed in damaged {
        let decoded = super::decode(changed.clone());
        let [body] = decoded.ir().geometry.bodies.as_slice() else {
            panic!("one body: {:?}", decoded.ir().geometry.bodies);
        };
        assert!(intact.ir().geometry.bodies.contains(body));
        let losses = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
            .collect::<Vec<_>>();
        let [loss] = losses.as_slice() else {
            panic!("one record.unreadable loss: {losses:?}");
        };
        assert!(loss
            .message
            .starts_with(&format!("part archive-entry {name:?} stream=\"\" offset=")));
        let strict = DecodeOptions {
            strictness: cadmpeg_ir::report::strictness::Strictness {
                pinned: true,
                ..Default::default()
            },
            ..DecodeOptions::default()
        };
        assert!(F3dCodec.decode(&mut Cursor::new(changed), &strict).is_err());
    }
}

#[test]
fn optional_asset_past_the_per_expansion_allowance_is_given_up_alone() {
    let bytes = optional_deflated_preview();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let [given_up] = scan.given_up.as_slice() else {
        panic!("one given-up part");
    };
    let loss = given_up.loss();
    assert_eq!(loss.code.to_string(), "decode/record.unreadable");
    assert!(loss.message.starts_with(
        "part archive-entry \"FusionAssetName[Active]/Previews/thumbnail.png\" stream=\"\" offset="
    ));
    assert!(loss.message.contains("scope Part"));
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn a_required_entry_past_the_per_expansion_allowance_is_given_up_alone() {
    let smbh = synthetic_geometry_smbh();
    let bytes = crate::test_support::zip_test::f3d_with_deflated_smbh(&smbh);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = u64::try_from(smbh.len() - 1).unwrap();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).expect("a Part refusal refuses no file");
    let [given_up] = scan.given_up.as_slice() else {
        panic!("one given-up part");
    };
    let loss = given_up.loss();
    assert!(loss.message.starts_with(
        "part archive-entry \"FusionAssetName[Active]/Breps.BlobParts/Body1.smbh\" stream=\"\" offset="
    ));
    assert!(loss.message.contains("scope Part"));
    assert!(scan.breps.is_empty());
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn optional_asset_past_the_session_allowance_refuses_the_session() {
    let bytes = optional_deflated_preview();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_total = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = crate::container::scan(&ctx, root)
        .err()
        .expect("the session allowance refuses");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(cadmpeg_core::decode::ResourceLimit {
            scope: cadmpeg_core::decode::LimitScope::Session,
            ..
        })
    ));
}

fn optional_deflated_preview() -> Vec<u8> {
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
    writer.finish().unwrap().into_inner()
}
