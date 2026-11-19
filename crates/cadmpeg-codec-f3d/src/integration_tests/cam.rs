// SPDX-License-Identifier: Apache-2.0
use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::{Cursor, Write};

fn with_cam(bytes: Vec<u8>, name: &str, xml: &[u8]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
    archive
        .start_file(
            name,
            crate::zip_write::file_options(zip::CompressionMethod::Stored),
        )
        .unwrap();
    archive.write_all(xml).unwrap();
    archive.finish().unwrap().into_inner()
}

const XML: &[u8] = br#"<document><model><tool id="t1"/><section type="setup" name="Synthetic"><section strategy="parallel" tool="t1"><toolpath valid="true"/></section></section></model></document>"#;
const MEMBER: &str = "Toolpath/Iron.Document/synthetic.irondoc";

#[test]
fn cam_section_structure_emits_assumption() {
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, XML);
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    let manufacturing = &result.ir().manufacturing;
    assert_eq!(manufacturing.setups.len(), 1);
    assert_eq!(manufacturing.operations.len(), 1);
    assert_eq!(
        manufacturing.operations[0].setup,
        manufacturing.setups[0].id
    );
    assert_eq!(
        manufacturing.operations[0].family,
        cadmpeg_ir::manufacturing::ManufacturingStrategy::Parallel
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::CamStructureAssumed.kind()));
}

#[test]
fn cam_repeated_tool_reference_selects_last_tool_and_emits_assumption() {
    let xml = br#"<document><model><tool id="t1" number="1"/><tool id="t1" number="2"/><section type="setup"><section strategy="parallel" tool="t1"/></section></model></document>"#;
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, xml);
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    let manufacturing = &result.ir().manufacturing;
    assert_eq!(manufacturing.tools.len(), 2);
    assert_eq!(manufacturing.operations.len(), 1);
    assert_ne!(manufacturing.tools[0].id, manufacturing.tools[1].id);
    assert_eq!(manufacturing.tools[0].number.as_deref(), Some("1"));
    assert_eq!(manufacturing.tools[1].number.as_deref(), Some("2"));
    assert_eq!(
        manufacturing.operations[0].tool.as_ref(),
        Some(&manufacturing.tools[1].id)
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::CamToolReferenceLastAssumed.kind()));
}

#[test]
fn cam_archive_decode_and_f3z_root_member() {
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, XML);
    let direct = F3dCodec
        .decode(&mut Cursor::new(&part), &DecodeOptions::default())
        .unwrap();
    assert_eq!(direct.ir().manufacturing.setups.len(), 1);
    assert_eq!(direct.ir().manufacturing.operations.len(), 1);
    let archive = f3z_archive("synthetic.f3d", &[("synthetic.f3d", &part)]);
    let result = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let manufacturing = &result.ir().manufacturing;
    assert_eq!(manufacturing.operations.len(), 1);
    assert_eq!(
        manufacturing.operations[0].setup,
        manufacturing.setups[0].id
    );
    assert_eq!(
        manufacturing.operations[0].tool.as_ref(),
        Some(&manufacturing.tools[0].id)
    );
    assert_eq!(
        manufacturing.setups[0].source.member,
        format!("synthetic.f3d/{MEMBER}")
    );
}

#[test]
fn cam_f3z_child_members_keep_distinct_identities() {
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, XML);
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", XREF_ROLE), ("second.f3d", XREF_ROLE)],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("first.f3d", &part),
            ("second.f3d", &part),
        ],
    );
    let result = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let manufacturing = &result.ir().manufacturing;
    assert_eq!(manufacturing.operations.len(), 2);
    assert_ne!(
        manufacturing.operations[0].id,
        manufacturing.operations[1].id
    );
    for operation in &manufacturing.operations {
        assert!(manufacturing
            .setups
            .iter()
            .any(|setup| setup.id == operation.setup));
        assert!(manufacturing
            .tools
            .iter()
            .any(|tool| Some(&tool.id) == operation.tool.as_ref()));
    }
}

#[test]
fn cam_bad_xml_does_not_fail_file_or_other_document() {
    let part = with_cam(
        with_cam(
            f3d_with_smbh(&synthetic_geometry_smbh()),
            MEMBER,
            b"<document><broken>",
        ),
        "Toolpath/Iron.Document/second.irondoc",
        XML,
    );
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().manufacturing.operations.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::CamDocumentUnreadable.kind()));
}

#[test]
fn cam_obeys_requested_content_scope() {
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, XML);
    for depth in [
        cadmpeg_core::decode::DecodeDepth::Geometry,
        cadmpeg_core::decode::DecodeDepth::Design,
    ] {
        let options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        let result = F3dCodec.decode(&mut Cursor::new(&part), &options).unwrap();
        assert!(result.ir().manufacturing.is_empty());
    }
}

#[test]
fn cam_native_archive_replay_preserves_source_bytes() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, XML);
    let result = F3dCodec
        .decode(&mut Cursor::new(&part), &DecodeOptions::default())
        .unwrap();
    let (ir, _, fidelity) = result.into_parts();
    let mut written = Vec::new();
    F3dCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .unwrap()
        .write_to(&mut written)
        .unwrap();
    assert_eq!(written, part);
}

#[test]
fn cam_workspace_without_brep_keeps_definitions() {
    let part = with_cam(
        f3d_without_brep("part-design", "synthetic.f3d", &[]),
        MEMBER,
        XML,
    );
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().manufacturing.setups.len(), 1);
    assert_eq!(result.ir().manufacturing.tools.len(), 1);
    assert_eq!(result.ir().manufacturing.operations.len(), 1);
}
