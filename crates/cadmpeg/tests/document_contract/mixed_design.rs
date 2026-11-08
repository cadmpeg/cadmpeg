// SPDX-License-Identifier: Apache-2.0
//! Partial `FreeCAD` design projection exports only document-held content.

use std::fs;
use std::io::{Cursor, Read, Write};

use assert_cmd::Command;
use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::document::ArenaClass;
use cadmpeg_ir::features::{Dimension, FeatureOperation, ParameterValue};
use cadmpeg_ir::report::export::{ExportReport, WritePath};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::CadIr;
use predicates::prelude::predicate;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

use super::{decode, workspace, write_decode};

#[derive(Clone, Copy, Debug)]
enum MixedKind {
    Unreadable,
    Vendor,
}

fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut member = archive.by_name(name).unwrap();
    let mut output = Vec::new();
    member.read_to_end(&mut output).unwrap();
    output
}

fn mixed(kind: MixedKind) -> Vec<u8> {
    let bytes =
        fs::read(workspace().join("corpus/freecad_fcstd/fixtures/design_history.FCStd")).unwrap();
    let mut source = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut member = source.by_index(index).unwrap();
        let name = member.name().unwrap().into_owned();
        let mut data = Vec::new();
        member.read_to_end(&mut data).unwrap();
        if name == "Document.xml" {
            let mut xml = String::from_utf8(data).unwrap();
            match kind {
                MixedKind::Vendor => {
                    assert!(xml.contains("type=\"PartDesign::Pad\""));
                    xml = xml.replacen(
                        "type=\"PartDesign::Pad\"",
                        "type=\"Vendor::PartDesign::PadLike\"",
                        1,
                    );
                }
                MixedKind::Unreadable => {
                    let object = xml
                        .find("<Object name=\"Pad\"")
                        .expect("Pad property object");
                    let property = object
                        + xml[object..]
                            .find("name=\"Length\"")
                            .expect("Pad.Length property");
                    let body = property + xml[property..].find('>').unwrap() + 1;
                    let end = body + xml[body..].find("</Property>").unwrap();
                    xml.replace_range(body..end, "<Float value=\"not-a-number\"/>");
                }
            }
            data = xml.into_bytes();
        }
        output
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        output.write_all(&data).unwrap();
    }
    output.finish().unwrap().into_inner()
}

fn assert_partial_design(ir: &CadIr, kind: MixedKind, written: bool) {
    let mut names = ir
        .model
        .features
        .iter()
        .map(|feature| feature.name.as_deref().unwrap())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "Body",
            "Fillet",
            "Pad",
            "PadSketch",
            "Pocket",
            "PocketSketch"
        ],
        "{kind:?}, written={written}"
    );
    let pad = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .unwrap();
    let FeatureOperation::Native {
        kind: native_kind,
        parameters,
    } = pad.definition().operation()
    else {
        panic!("{kind:?}: Pad must retain its actual native operation");
    };
    assert_eq!(
        native_kind.as_str(),
        match kind {
            MixedKind::Unreadable => "PartDesign::Pad",
            MixedKind::Vendor => "Vendor::PartDesign::PadLike",
        }
    );
    let parameter = |name: &str| {
        parameters.get(name).map(|dimension| {
            let Dimension::Parameter(id) = dimension else {
                panic!("{kind:?}: {name} must use its canonical parameter");
            };
            let parameter = ir
                .model
                .parameters
                .iter()
                .find(|parameter| &parameter.id == id)
                .unwrap();
            assert_eq!(parameter.owner.as_ref(), Some(&pad.id));
            assert_eq!(parameter.name, name);
            let Some(ParameterValue::Length(value)) = &parameter.value else {
                panic!("{kind:?}: {name} must retain its length value");
            };
            value.get()
        })
    };
    assert_eq!(parameter("Length2"), Some(10.0));
    match kind {
        MixedKind::Unreadable => assert_eq!(parameter("Length"), None),
        MixedKind::Vendor => assert_eq!(parameter("Length"), Some(10.0)),
    }
    let native = serde_json::to_value(ir.native.namespace("fcstd").unwrap()).unwrap();
    let object = native["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["name"] == "Pad")
        .unwrap();
    assert_eq!(object["type_name"], native_kind.as_str());
    assert_eq!(pad.native_ref.as_deref(), object["id"].as_str());
    let length = native["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["owner"] == object["id"] && property["name"] == "Length");
    match (kind, written) {
        (MixedKind::Unreadable, true) => assert!(length.is_none()),
        (MixedKind::Unreadable, false) => {
            let length = length.expect("unreadable value must retain its property header");
            assert_eq!(length["type_name"], "App::PropertyLength");
        }
        (MixedKind::Vendor, _) => {
            let length = length.expect("vendor property remains document-held");
            assert_eq!(length["values"][0]["tag"], "Float");
            let value = length["values"][0]["attributes"]["value"].as_str().unwrap();
            assert_eq!(value.parse::<f64>().unwrap(), 10.0);
            if !written {
                assert_eq!(value, "10.0000000000000000");
            }
        }
    }
}

fn assert_partial_export(report: &ExportReport, kind: MixedKind) {
    let omitted = report
        .losses
        .iter()
        .filter(|loss| {
            loss.code.local_code() == "metadata.native-property-not-transferred"
                && loss.message.contains("Length")
                && loss.message.contains("Pad")
        })
        .collect::<Vec<_>>();
    match kind {
        MixedKind::Unreadable => {
            assert_eq!(omitted.len(), 1, "{:?}", report.losses);
        }
        MixedKind::Vendor => assert!(omitted.is_empty()),
    }
}

// An editor deletes native design content as well as its neutral projection.
// Neutral topology/geometry stays untouched; synthesized BRep uses those arenas.

#[test]
fn partial_design_default_exports_and_loss_policy_judges_the_current_decode() {
    for kind in [MixedKind::Unreadable, MixedKind::Vendor] {
        let dir = tempdir().unwrap();
        let input = dir.path().join("mixed.FCStd");
        let output = dir.path().join("written.FCStd");
        let report = dir.path().join("report.json");
        let bytes = mixed(kind);
        fs::write(&input, &bytes).unwrap();
        let decoded = decode(&FcstdCodec, &bytes);
        match kind {
            MixedKind::Unreadable => assert!(decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("Pad"))),
            MixedKind::Vendor => assert!(
                decoded.report().losses.is_empty(),
                "{:?}",
                decoded.report().losses
            ),
        }
        let assertion = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--report",
                report.to_str().unwrap(),
            ])
            .assert()
            .success();
        if matches!(kind, MixedKind::Unreadable) {
            assertion.stderr(predicate::str::contains("loss"));
        }
        let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        let classes = decoded.source_fidelity().unrepresented();
        let path = if classes.is_empty() {
            "patched"
        } else {
            "synthesized"
        };
        assert_eq!(report["export"]["write_path"]["path"], path);
        let written_bytes = fs::read(&output).unwrap();
        let xml = String::from_utf8(entry(&written_bytes, "Document.xml")).unwrap();
        let written = decode(&FcstdCodec, &written_bytes);
        if matches!(kind, MixedKind::Vendor) {
            assert!(written.report().losses.is_empty());
        }
        assert_partial_design(decoded.ir(), kind, false);
        assert_partial_design(written.ir(), kind, true);
        assert!(xml.contains("name=\"Pad\""));
        let export: ExportReport = serde_json::from_value(report["export"].clone()).unwrap();
        assert_partial_export(&export, kind);
        cadmpeg_test_support::contract::assert_written_arenas_agree(
            &export,
            decoded.ir(),
            written.ir(),
        );
        let refusal_path = dir.path().join("decode-refusal.json");
        let assertion = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "-f",
                "cadir",
                "--reject-lossy",
                "--report",
                refusal_path.to_str().unwrap(),
            ])
            .assert();
        if matches!(kind, MixedKind::Unreadable) {
            assertion
                .code(1)
                .stderr(predicate::str::contains("decode reported"));
        } else {
            assertion.success();
        }
        let refusal: serde_json::Value =
            serde_json::from_slice(&fs::read(refusal_path).unwrap()).unwrap();
        if matches!(kind, MixedKind::Unreadable) {
            assert_eq!(refusal["status"], "refused");
            assert_eq!(refusal["refusal"]["code"], "decode_loss_rejected");
        } else {
            assert_eq!(refusal["status"], "ok");
        }
        let assertion = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "-f",
                "cadir",
                "--reject-lossy",
                "--reject-lossy-floor=blocking",
            ])
            .assert();
        if decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.severity >= Severity::Blocking)
        {
            assertion.code(1);
        } else {
            assertion.success();
        }
        #[cfg(feature = "step")]
        {
            let step = dir.path().join("geometry.step");
            Command::cargo_bin("cadmpeg")
                .unwrap()
                .args([
                    "convert",
                    input.to_str().unwrap(),
                    "-o",
                    step.to_str().unwrap(),
                ])
                .assert()
                .success();
            let transferred = decode(
                &cadmpeg_codec_step::StepCodec::default(),
                &fs::read(step).unwrap(),
            );
            assert_eq!(
                transferred.ir().model.bodies.len(),
                decoded.ir().model.bodies.len()
            );
        }
    }
}

#[test]
fn vendor_design_passes_every_loss_rejection_mode() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("vendor.FCStd");
    fs::write(&input, mixed(MixedKind::Vendor)).unwrap();
    for mode in ["decode", "export", "any"] {
        for floor in ["info", "warning", "error", "blocking"] {
            let output = dir.path().join(format!("{mode}-{floor}.FCStd"));
            Command::cargo_bin("cadmpeg")
                .unwrap()
                .args([
                    "convert",
                    input.to_str().unwrap(),
                    "-o",
                    output.to_str().unwrap(),
                    &format!("--reject-lossy={mode}"),
                    &format!("--reject-lossy-floor={floor}"),
                ])
                .assert()
                .success();
        }
    }
}

#[test]
fn partial_design_native_guard_and_synthesis_keep_objects_without_unreadable_values() {
    for kind in [MixedKind::Unreadable, MixedKind::Vendor] {
        let result = decode(&FcstdCodec, &mixed(kind));
        let classes = result.source_fidelity().unrepresented();
        if matches!(kind, MixedKind::Unreadable) {
            assert!(classes.contains(&ArenaClass::Design));
        }
        let input = EncodeInput::new(result.ir(), Some(result.source_fidelity()));
        assert_eq!(input.can_reuse_source(), classes.is_empty());
        let (written, report) = write_decode(
            &FcstdCodec,
            &FcstdCodec,
            result.ir(),
            Some(result.source_fidelity()),
            None,
        );
        if classes.is_empty() {
            assert!(matches!(report.write_path(), WritePath::Patched { .. }));
        } else {
            assert!(matches!(report.write_path(), WritePath::Synthesized { .. }));
            assert!(
                matches!(report.synthesis_cause(), Some(cadmpeg_ir::codec::write::SynthesisCause::SourceUnrepresented { classes: actual }) if *actual == classes)
            );
        }
        assert_partial_design(result.ir(), kind, false);
        assert_partial_design(written.ir(), kind, true);
        assert_partial_export(&report, kind);
        cadmpeg_test_support::contract::assert_written_arenas_agree(
            &report,
            result.ir(),
            written.ir(),
        );
        assert_eq!(
            written.ir().model.bodies.len(),
            result.ir().model.bodies.len()
        );
        if matches!(kind, MixedKind::Vendor) {
            let namespace = written.ir().native.namespace("fcstd").unwrap();
            assert!(namespace.arenas().values().flatten().any(|record| {
                serde_json::to_string(record)
                    .unwrap()
                    .contains("Vendor::PartDesign::PadLike")
            }));
        }
    }
}
