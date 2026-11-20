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
fn cam_bracketed_workspace_reads_flat_objects_and_attribute_tools() {
    let xml = br#"<document xmlns="urn:synthetic:document"><model>
      <tool-table xmlns="urn:synthetic:tools"><tool id="cutter" type="flat end mill" unit="millimeters">
        <nc number="7"/><body diameter="8.000" number-of-flutes="4" flute-length="18.00" body-length="32.00"/>
        <holder diameter="14.00"/>
      </tool></tool-table>
      <docObjects>
        <object id="root" strategy="container" displayName="Document"/>
        <object id="cut" parent="group" strategy="face" toolId="cutter" displayName="Facing" valid="true">
          <toolpathToolView/>
          <Parameters><Parameter name="stepover" value="3.25" expression="3,25mm"/><Parameter name="tool_feedCutting" value="850.00"/></Parameters>
        </object>
        <object id="group" parent="job" strategy="folder"/>
        <object id="job" parent="root" strategy="setup" displayName="Fixture">
          <Parameters><Parameter name="job_type" value="milling"/><Parameter name="job_stockMode" value="default"/><Parameter name="job_stockOffsetTop" value="2.500" expression="2,5mm"/><Parameter name="wcs_orientation_mode" value="modelOrientation"/><Parameter name="wcs_origin_mode" value="stockPoint"/><Parameter name="wcs_origin_boxPoint" value="top center"/></Parameters>
        </object>
      </docObjects>
    </model></document>"#;
    let member = "Toolpath[synthetic]/Iron.Document/definition.irondoc";
    let part = with_cam(
        f3d_without_brep("part-design", "synthetic.f3d", &[]),
        member,
        xml,
    );
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    let cam = &result.ir().manufacturing;
    assert_eq!(
        (cam.setups.len(), cam.operations.len(), cam.tools.len()),
        (1, 1, 1)
    );
    let setup = &cam.setups[0];
    assert_eq!(setup.name.as_deref(), Some("Fixture"));
    assert_eq!(setup.stock.kind.as_deref(), Some("default"));
    assert_eq!(setup.stock.offsets[0].value, "2.500");
    assert!(setup.stock.offsets[0].native_xml.contains("2,5mm"));
    assert_eq!(setup.wcs.orientation[0].value, "modelOrientation");
    assert_eq!(setup.wcs.origin_mode.as_deref(), Some("stockPoint"));
    assert_eq!(setup.wcs.origin[0].value, "top center");
    let op = &cam.operations[0];
    assert_eq!(op.setup, setup.id);
    assert_eq!(op.order, 0);
    assert_eq!(op.name.as_deref(), Some("Facing"));
    assert_eq!(op.strategy, "face");
    assert_eq!(
        op.family,
        cadmpeg_ir::manufacturing::ManufacturingStrategy::Face
    );
    assert_eq!(op.tool.as_ref(), Some(&cam.tools[0].id));
    assert!(op.computed_path_present);
    assert_eq!(op.computed_path_valid, Some(true));
    assert_eq!(op.parameters[0].value, "3.25");
    assert!(op.parameters[0].native_xml.contains("3,25mm"));
    assert_eq!(op.parameters[1].value, "850.00");
    assert_eq!(cam.tools[0].number.as_deref(), Some("7"));
    assert_eq!(cam.tools[0].tool_type.as_deref(), Some("flat end mill"));
    let diameter = cam.tools[0]
        .parameters
        .iter()
        .find(|p| p.name == "body.diameter")
        .unwrap();
    assert_eq!(diameter.value, "8.000");
    assert!(diameter.native_xml.contains("diameter=\"8.000\""));
    assert!(cam.tools[0]
        .holder
        .iter()
        .any(|p| p.name == "holder.diameter" && p.value == "14.00"));
    for source in [&setup.source, &op.source, &cam.tools[0].source] {
        assert_eq!(source.member, member);
        assert_eq!(
            &xml[usize::try_from(source.start).unwrap()..usize::try_from(source.end).unwrap()],
            source.native_xml.as_bytes()
        );
    }
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::CamSetupAssumed.kind()));
}

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
