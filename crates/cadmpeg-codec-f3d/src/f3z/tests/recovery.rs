// SPDX-License-Identifier: Apache-2.0
//! Optional archive recovery and required-member admission boundaries.

use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::{F3dCodec, F3dLossCode};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn damage_local_frame(bytes: &mut [u8], name: &str) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let snapshot = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let entry = snapshot.entry(name).unwrap();
    let start = usize::try_from(entry.header_start).unwrap();
    bytes[start] ^= 1;
}

#[test]
fn unreadable_unrelated_members_preserve_geometry_source_image_and_named_loss() {
    let root = f3d_with_smbh(&synthetic_geometry_smbh());
    let preview = vec![0; 2_000_000];
    for name in [
        "preview.bin",
        "FusionAssetName[Active]/Previews/preview.png",
    ] {
        let original = f3z_archive("root.f3d", &[(name, &preview), ("root.f3d", &root)]);
        let baseline = crate::test_support::archive_session::decode(
            &mut Cursor::new(&original),
            &DecodeOptions::default(),
        )
        .unwrap();
        assert!(!baseline.ir().model.faces.is_empty());
        let mut changed = original;
        damage_local_frame(&mut changed, name);
        let decoded = cadmpeg_test_support::EditableDecodeResult::from(
            crate::test_support::archive_session::decode(
                &mut Cursor::new(&changed),
                &DecodeOptions::default(),
            )
            .unwrap(),
        );
        assert_eq!(decoded.ir().model, baseline.ir().model);
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .unwrap()
            .is_ok());
        assert!(<F3dCodec as CodecBackend>::validate_native(
            &cadmpeg_test_support::service_decode_context(),
            decoded.ir(),
            None,
        )
        .unwrap()
        .is_empty());
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::ArchiveEntryUnreadable.note("").code));
        assert_eq!(
            decoded
                .source_fidelity()
                .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
                .unwrap()
                .data(),
            Some(changed.as_slice())
        );
        let summary = F3dCodec
            .inspect(&mut Cursor::new(&changed), &InspectOptions::default())
            .unwrap();
        assert!(summary
            .losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::ArchiveEntryUnreadable.note("").code));
    }
}

#[test]
fn unreadable_unreferenced_document_does_not_hide_a_readable_model_root() {
    let root = f3d_with_smbh(&synthetic_geometry_smbh());
    let original = f3z_archive("root.f3d", &[("unused.f3d", &root), ("root.f3d", &root)]);
    let baseline = crate::test_support::archive_session::decode(
        &mut Cursor::new(&original),
        &DecodeOptions::default(),
    )
    .unwrap();
    let mut changed = original;
    damage_local_frame(&mut changed, "unused.f3d");
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(&changed),
        &DecodeOptions::default(),
    )
    .unwrap();
    assert_eq!(decoded.ir().model, baseline.ir().model);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::XrefMemberUndecoded.note("").code));
}

#[test]
fn unreadable_required_members_still_refuse_the_document() {
    let root = f3d_with_smbh(&synthetic_geometry_smbh());
    let original = f3z_archive("root.f3d", &[("root.f3d", &root), ("other.f3d", &root)]);
    for name in ["root.f3d", "Manifest.json", "DesignDescription.json"] {
        let mut changed = original.clone();
        damage_local_frame(&mut changed, name);
        assert!(
            crate::test_support::archive_session::decode(
                &mut Cursor::new(changed),
                &DecodeOptions::default()
            )
            .is_err(),
            "{name}"
        );
    }
    for name in [
        "Manifest.dat",
        "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh",
    ] {
        let mut changed = root.clone();
        damage_local_frame(&mut changed, name);
        assert!(
            crate::test_support::archive_session::decode(
                &mut Cursor::new(changed),
                &DecodeOptions::default()
            )
            .is_err(),
            "{name}"
        );
    }
}

#[test]
fn unreadable_referenced_document_withholds_the_occurrence_with_named_losses() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("component.f3d", XREF_ROLE)],
    );
    let mut changed = f3z_archive(
        "root.f3d",
        &[("root.f3d", &root), ("component.f3d", &component)],
    );
    let baseline = crate::test_support::archive_session::decode(
        &mut Cursor::new(&changed),
        &DecodeOptions::default(),
    )
    .unwrap();
    assert!(!baseline.ir().model.faces.is_empty());
    damage_local_frame(&mut changed, "component.f3d");
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(changed),
        &DecodeOptions::default(),
    )
    .unwrap();
    assert!(decoded.ir().model.faces.is_empty());
    assert!(decoded.ir().model.surfaces.is_empty());
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::XrefMemberUndecoded.note("").code));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ArchiveEntryUnreadable.note("").code));
}

/// Smallest cumulative decompression ceiling at which `archive` decodes.
fn smallest_decompression_ceiling(archive: &[u8]) -> u64 {
    let decodes = |ceiling: u64| {
        let mut policy = DecodePolicy::desktop();
        policy.limits.max_decompressed_bytes_total = ceiling;
        F3dCodec
            .decode(
                &mut Cursor::new(archive),
                &DecodeOptions {
                    policy,
                    ..DecodeOptions::default()
                },
            )
            .is_ok()
    };
    let (mut low, mut high) = (
        0_u64,
        DecodePolicy::desktop().limits.max_decompressed_bytes_total,
    );
    assert!(decodes(high));
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if decodes(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

#[test]
fn failed_referenced_document_decodes_once_for_every_reference() {
    let mut component = f3d_with_smbh(&synthetic_geometry_smbh());
    damage_local_frame(
        &mut component,
        "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh",
    );
    let parent = |name: &str, targets: &[(&str, &str)]| f3d_without_brep("design", name, targets);
    let archive = |second: &[(&str, &str)]| {
        let root = f3d_without_brep(
            "assembly-design",
            "root.f3d",
            &[("first.f3d", XREF_ROLE), ("second.f3d", XREF_ROLE)],
        );
        let first = parent("first.f3d", &[("component.f3d", XREF_ROLE)]);
        let second = parent("second.f3d", second);
        f3z_archive(
            "root.f3d",
            &[
                ("root.f3d", &root),
                ("first.f3d", &first),
                ("second.f3d", &second),
                ("component.f3d", &component),
            ],
        )
    };
    let unreferenced = archive(&[("missing.f3d", XREF_ROLE)]);
    let repeated = archive(&[("component.f3d", XREF_ROLE)]);
    // The second parent names a missing member or the failed one; the
    // tables have equal length, so any extra cost is a second decode.
    let base = smallest_decompression_ceiling(&unreferenced);
    let ceiling = smallest_decompression_ceiling(&repeated);
    eprintln!("ceilings: unreferenced {base}, repeated {ceiling}");
    assert!(ceiling <= base, "{ceiling} > {base}");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&repeated), &DecodeOptions::default())
        .unwrap();
    let undecoded = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::XrefMemberUndecoded.note("").code)
        .count();
    assert_eq!(undecoded, 2);
}
