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

const XML: &[u8] = br#"<document><model><tool id="t1"/><object id="s" displayName="Synthetic" strategy="setup"/><object id="o" parent="s" strategy="face" toolId="t1" valid="true"/></model></document>"#;
const MEMBER: &str = "Toolpath/Iron.Document/synthetic.irondoc";

#[test]
fn cam_tool_overall_length_comes_from_overall_length_not_body_length() {
    let xml = br#"<document><model><tool id="t1" type="flat end mill" unit="millimeters"><body diameter="6" flute-length="15" body-length="25" overall-length="50"/></tool><object id="s" displayName="Setup11" strategy="setup"><Parameters/></object></model></document>"#;
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, xml);
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    let process = &result.ir().process;
    let cutter = cadmpeg_core::serde_erase::json::to_value(&process.tools[0].cutter).unwrap();
    assert_eq!(cutter["overall_length"]["value"], 50.0);
    assert_eq!(cutter["flute_length"]["value"], 15.0);
    assert_eq!(process.setups[0].name.as_deref(), Some("Setup11"));
}

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
    let cam = &manufacturing(result.ir());
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
    assert_eq!(op.family, crate::cam::records::ManufacturingStrategy::Face);
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
fn cam_object_structure_emits_assumption() {
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, XML);
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    let manufacturing = &manufacturing(result.ir());
    assert_eq!(manufacturing.setups.len(), 1);
    assert_eq!(manufacturing.operations.len(), 1);
    assert_eq!(
        manufacturing.operations[0].setup,
        manufacturing.setups[0].id
    );
    assert_eq!(
        manufacturing.operations[0].family,
        crate::cam::records::ManufacturingStrategy::Face
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::CamStructureAssumed.kind()));
}

#[test]
fn cam_repeated_tool_reference_selects_last_tool_and_emits_assumption() {
    let xml = br#"<document><model><tool id="t1"><nc number="1"/></tool><tool id="t1"><nc number="2"/></tool><object id="s" strategy="setup"/><object parent="s" strategy="face" toolId="t1"/></model></document>"#;
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, xml);
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    let manufacturing = &manufacturing(result.ir());
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
    assert_eq!(manufacturing(direct.ir()).setups.len(), 1);
    assert_eq!(manufacturing(direct.ir()).operations.len(), 1);
    let archive = f3z_archive("synthetic.f3d", &[("synthetic.f3d", &part)]);
    let result = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let manufacturing = &manufacturing(result.ir());
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
    let manufacturing = &manufacturing(result.ir());
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
    assert_eq!(manufacturing(result.ir()).operations.len(), 1);
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
        assert!(manufacturing(result.ir()).is_empty());
    }
}

#[test]
fn cam_native_archive_replay_preserves_source_bytes() {
    use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, XML);
    let result = F3dCodec
        .decode(&mut Cursor::new(&part), &DecodeOptions::default())
        .unwrap();
    let (ir, _, fidelity) = result.into_parts();
    let mut written = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, Some(&fidelity)))
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
    assert_eq!(manufacturing(result.ir()).setups.len(), 1);
    assert_eq!(manufacturing(result.ir()).tools.len(), 1);
    assert_eq!(manufacturing(result.ir()).operations.len(), 1);
}

fn manufacturing(ir: &cadmpeg_ir::CadIr) -> crate::cam::records::Manufacturing {
    crate::cam::project::retained(ir).unwrap()
}

#[test]
fn cam_face_strategy_wire_round_trip() {
    use crate::cam::records::ManufacturingStrategy;
    let encoded = serde_json::to_string(&ManufacturingStrategy::Face).unwrap();
    assert_eq!(encoded, "\"face\"");
    assert_eq!(
        serde_json::from_str::<ManufacturingStrategy>(&encoded).unwrap(),
        ManufacturingStrategy::Face
    );
}

#[test]
fn cam_manufacturing_wire_keeps_exact_parameter_text() {
    let xml = br#"<document><tool id="t1" type="flat end mill"><Parameters><Parameter name="tool_diameter" value="6.000" expression="6mm"/></Parameters></tool></document>"#;
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, xml);
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    let encoded = serde_json::to_string(ir).unwrap();
    let decoded: cadmpeg_ir::CadIr = serde_json::from_str(&encoded).unwrap();
    assert_eq!(&decoded, ir);
    assert_eq!(ir.ir_version(), "11");
    assert!(encoded.contains("6.000"));
    assert_eq!(
        manufacturing(&decoded).tools[0].parameters[0].value,
        "6.000"
    );
    assert_eq!(ir.process.tools.len(), 1);
    assert!(!cadmpeg_ir::diff::diff(&cadmpeg_ir::CadIr::empty(), ir).is_empty());
}

#[test]
fn cam_empty_section_preserves_existing_wire() {
    let ir = cadmpeg_ir::CadIr::empty();
    let encoded = cadmpeg_core::serde_erase::json::to_value(&ir).unwrap();
    assert!(encoded.get("manufacturing").is_none());
    let decoded: cadmpeg_ir::CadIr =
        cadmpeg_core::serde_erase::json::from_value(encoded.clone()).unwrap();
    assert!(manufacturing(&decoded).is_empty());
    let mut explicit_null = encoded;
    explicit_null["process"] = serde_json::Value::Null;
    assert!(
        cadmpeg_core::serde_erase::json::from_value::<cadmpeg_ir::CadIr>(explicit_null).is_err()
    );
}

#[test]
fn cam_dangling_setup_is_a_reference_finding() {
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, XML);
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    let mut ir = result.into_parts().0;
    let operation = ir.process.operations[0].id.to_string();
    ir.process.setups.clear();
    let report = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.entity.as_deref() == Some(operation.as_str())));
}

#[test]
fn cam_process_uses_setup_order_tool_references_and_canonical_rates() {
    let xml = br#"<document><model><tool id="t1" type="flat end mill"><body diameter="8.000"/></tool><object id="s" strategy="setup"><Parameters><Parameter name="wcs_orientation_mode" value="modelOrientation"/></Parameters></object><object id="op" parent="s" strategy="face" toolId="t1"><Parameters><Parameter name="tool_feedCutting" value="600" expression="600mmpm"/><Parameter name="tool_spindleSpeed" value="60" expression="60rpm"/></Parameters></object></model></document>"#;
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, xml);
    let result = F3dCodec
        .decode(&mut Cursor::new(part), &DecodeOptions::default())
        .unwrap();
    let process = &result.ir().process;
    assert_eq!(process.setups.len(), 1);
    assert_eq!(process.operations.len(), 1);
    assert_eq!(process.tools.len(), 1);
    assert_eq!(
        process.setups[0].steps,
        vec![cadmpeg_ir::process::setup::Step::Operation(
            process.operations[0].id.clone()
        )]
    );
    assert_eq!(
        process.operations[0].tooling,
        Some(cadmpeg_ir::process::operation::Tooling::Tool(
            process.tools[0].id.clone()
        ))
    );
    let technology =
        cadmpeg_core::serde_erase::json::to_value(&process.operations[0].technology).unwrap();
    let parameter = |name: &str| {
        process
            .parameters
            .iter()
            .find(|parameter| parameter.name.as_str() == name)
            .map(|parameter| cadmpeg_core::serde_erase::json::to_value(parameter).unwrap())
            .unwrap()
    };
    let feed = parameter("tool_feedCutting");
    assert_eq!(feed["value"]["value"], 10.0);
    assert_eq!(technology["feeds"][0][1]["value"]["kind"], "parameter");
    assert_eq!(
        technology["feeds"][0][1]["value"]["value"]["value"],
        feed["id"]
    );
    let speed = parameter("tool_spindleSpeed");
    let rate = speed["value"]["value"].as_f64().unwrap();
    assert!((rate - std::f64::consts::TAU).abs() <= f64::EPSILON * std::f64::consts::TAU);
    assert_eq!(
        technology["spindle"]["value"]["value"]["value"],
        speed["id"]
    );
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|finding| !finding.rule.as_str().starts_with("process.")),
        "{:?}",
        report.findings
    );
}

#[test]
fn cam_f3z_keeps_source_operation_order() {
    let xml = br#"<document><object id="s" strategy="setup"/><!-- padding padding padding padding padding padding --><object parent="s" strategy="face" displayName="first"/><!-- padding padding padding padding padding padding padding padding padding padding padding padding --><object parent="s" strategy="face" displayName="second"/></document>"#;
    let part = with_cam(f3d_with_smbh(&synthetic_geometry_smbh()), MEMBER, xml);
    let archive = f3z_archive("synthetic.f3d", &[("synthetic.f3d", &part)]);
    for bytes in [part, archive] {
        let result = F3dCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let process = &result.ir().process;
        let names: Vec<_> = process.setups[0]
            .steps
            .iter()
            .map(|step| {
                let cadmpeg_ir::process::setup::Step::Operation(id) = step else {
                    panic!("expected an operation step")
                };
                process
                    .operations
                    .iter()
                    .find(|operation| &operation.id == id)
                    .unwrap()
                    .name
                    .as_deref()
                    .unwrap()
            })
            .collect();
        assert_eq!(names, ["first", "second"]);
        let retained = manufacturing(result.ir());
        assert_eq!(retained.operations[0].name.as_deref(), Some("first"));
        assert_eq!(retained.operations[1].name.as_deref(), Some("second"));
    }
}
