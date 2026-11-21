// SPDX-License-Identifier: Apache-2.0
//! Optional archive recovery and required-member admission boundaries.

use crate::test_support::assembly_test::{
    f3d_with_text_brep_stream, f3d_without_brep, f3z_archive, XREF_ROLE,
};
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
    let preview = b"unrelated preview".to_vec();
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
        assert!(!baseline.ir().geometry.faces.is_empty());
        let mut changed = original;
        damage_local_frame(&mut changed, name);
        let decoded = cadmpeg_test_support::EditableDecodeResult::from(
            crate::test_support::archive_session::decode(
                &mut Cursor::new(&changed),
                &DecodeOptions::default(),
            )
            .unwrap(),
        );
        assert_eq!(decoded.ir().geometry, baseline.ir().geometry);
        assert_eq!(decoded.ir().model, baseline.ir().model);
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .unwrap()
            .is_ok());
        assert!(<F3dCodec as CodecBackend>::validate_native(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            decoded.ir(),
            None,
        )
        .unwrap()
        .is_empty());
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "decode/record.unreadable"));
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
            .any(|loss| loss.code.to_string() == "decode/record.unreadable"));
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
    assert_eq!(decoded.ir().geometry, baseline.ir().geometry);
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
    let mut changed = root;
    damage_local_frame(&mut changed, "Manifest.dat");
    assert!(crate::test_support::archive_session::decode(
        &mut Cursor::new(changed),
        &DecodeOptions::default()
    )
    .is_err());
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
    assert!(!baseline.ir().geometry.faces.is_empty());
    damage_local_frame(&mut changed, "component.f3d");
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(changed),
        &DecodeOptions::default(),
    )
    .unwrap();
    assert!(decoded.ir().geometry.faces.is_empty());
    assert!(decoded.ir().geometry.surfaces.is_empty());
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::XrefMemberUndecoded.note("").code));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "decode/record.unreadable"));
}

#[test]
fn a_merged_member_part_loss_names_the_stub_in_the_document() {
    let preview = "FusionAssetName[Active]/Previews/thumbnail.png";
    let mut component = crate::test_support::zip_test::f3d_with_configuration(
        &synthetic_geometry_smbh(),
        preview,
        b"preview",
    );
    damage_local_frame(&mut component, preview);
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("component.f3d", XREF_ROLE)],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[("root.f3d", &root), ("component.f3d", &component)],
    );
    let decoded = cadmpeg_test_support::EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(&archive),
            &DecodeOptions::default(),
        )
        .unwrap(),
    );
    assert!(!decoded.ir().geometry.faces.is_empty());
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
        .find(|stub| stub.id.as_str().contains("/source:part#"))
        .expect("the member's given-up preview has a stub");
    assert!(stub.id.as_str().starts_with("f3d:xref/"), "{}", stub.id);
    assert!(
        loss.message.contains(&format!(" record={}: ", stub.id)),
        "{}",
        loss.message
    );
    let retained = decoded
        .source_fidelity()
        .retained_record(stub.id.as_str())
        .expect("the part is named in the sidecar");
    assert_eq!(retained.data(), None);
}

#[test]
fn each_member_part_links_to_its_own_member_definition() {
    let preview = "FusionAssetName[Active]/Previews/thumbnail.png";
    let member = || {
        let mut member = crate::test_support::zip_test::f3d_with_configuration(
            &synthetic_geometry_smbh(),
            preview,
            b"preview",
        );
        damage_local_frame(&mut member, preview);
        member
    };
    let (first, second) = (member(), member());
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", "first-role"), ("second.f3d", "second-role")],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("first.f3d", &first),
            ("second.f3d", &second),
        ],
    );
    let decoded = cadmpeg_test_support::EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(&archive),
            &DecodeOptions::default(),
        )
        .unwrap(),
    );
    let definitions = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .map(|definition| definition.id.as_str())
        .collect::<Vec<_>>();
    // §8.1: the root file's root definition and one File definition per
    // member.
    assert_eq!(definitions.len(), 3, "{definitions:?}");
    let stubs = decoded
        .ir()
        .native_unknowns("f3d")
        .unwrap()
        .into_iter()
        .filter(|stub| stub.id.as_str().contains("/source:part#"))
        .collect::<Vec<_>>();
    assert_eq!(stubs.len(), 2, "{stubs:?}");
    let mut owners = Vec::new();
    for stub in &stubs {
        let [link] = stub.links.as_slice() else {
            panic!("{} has one link: {:?}", stub.id, stub.links);
        };
        let (member, _) = stub.id.as_str().split_once("/source:part#").unwrap();
        assert_eq!(
            link.as_str(),
            format!("{member}/archive:product-definition#0"),
            "{}",
            stub.id
        );
        assert!(definitions.contains(&link.as_str()));
        owners.push(link.as_str());
        let retained = decoded
            .source_fidelity()
            .retained_record(stub.id.as_str())
            .expect("the part is named in the sidecar");
        assert_eq!(
            retained.untyped(),
            &std::collections::BTreeSet::from([cadmpeg_core::decode::ArenaClass::Model])
        );
    }
    owners.sort_unstable();
    owners.dedup();
    assert_eq!(owners.len(), 2, "each member owns its own part");
    let findings = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .findings;
    assert!(
        !findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::report::check::Check::NativeLinks),
        "{findings:#?}"
    );
    assert!(decoded
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_core::decode::ArenaClass::Model));
    assert!(<F3dCodec as CodecBackend>::validate_native(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        decoded.ir(),
        None,
    )
    .unwrap()
    .is_empty());
}

#[test]
fn a_model_root_part_keeps_its_bytes_when_the_root_source_image_is_replaced() {
    let preview = "FusionAssetName[Active]/Previews/thumbnail.png";
    let mut root = crate::test_support::zip_test::f3d_with_configuration(
        &synthetic_geometry_smbh(),
        preview,
        b"preview",
    );
    damage_local_frame(&mut root, preview);
    let archive = f3z_archive("root.f3d", &[("root.f3d", &root)]);
    let decoded = cadmpeg_test_support::EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(&archive),
            &DecodeOptions::default(),
        )
        .unwrap(),
    );
    let stubs = decoded.ir().native_unknowns("f3d").unwrap();
    let stub = stubs
        .iter()
        .find(|stub| stub.id.as_str().starts_with("f3d:source:part#"))
        .expect("the root's given-up preview has a stub");
    let retained = decoded
        .source_fidelity()
        .retained_record(stub.id.as_str())
        .expect("the part is retained");
    assert_eq!(retained.stream(), "root.f3d");
    let start = usize::try_from(retained.offset()).unwrap();
    let end = usize::try_from(retained.end_offset()).unwrap();
    assert_eq!(retained.data(), Some(&root[start..end]));
}

#[test]
fn failed_member_placed_by_two_parents_reports_each_reference() {
    // An invalid text BREP stream passes the archive scan and fails inside
    // member decode.
    let component = f3d_with_text_brep_stream(
        &["FusionAssetName[Active]/Breps.BlobParts/BREP0.sat"],
        b"invalid SAT stream",
    );
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", XREF_ROLE), ("second.f3d", XREF_ROLE)],
    );
    let first = f3d_without_brep("design", "first.f3d", &[("component.f3d", XREF_ROLE)]);
    let second = f3d_without_brep("design", "second.f3d", &[("component.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("first.f3d", &first),
            ("second.f3d", &second),
            ("component.f3d", &component),
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&archive), &DecodeOptions::default())
        .unwrap();
    let failures = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == F3dLossCode::XrefMemberUndecoded.note("").code
                && loss
                    .message
                    .contains("member component.f3d failed to decode")
        })
        .map(|loss| loss.message.split(": member ").nth(1).unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 2, "{failures:?}");
    assert_eq!(failures[0], failures[1]);
}
