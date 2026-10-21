// SPDX-License-Identifier: Apache-2.0
//! Partial FreeCAD design projection exports only document-held content.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Read, Write};

use assert_cmd::Command;
use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::document::{ArenaClass, ArenaName, ContentScope};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::CadIr;
use predicates::prelude::predicate;
use serde_json::json;
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

fn clear_design(ir: &CadIr, scope: ContentScope) -> CadIr {
    let mut value = serde_json::to_value(ir).unwrap();
    delete_native_design(&mut value);
    for arena in ArenaName::ALL {
        if arena.class() == ArenaClass::Design {
            value["model"][arena.as_str()] = json!([]);
        }
    }
    let mut result = CadIr::from_json(&value.to_string()).unwrap();
    result.set_scope(scope).unwrap();
    result
}

// An editor deletes native design content as well as its neutral projection.
// Neutral topology/geometry stays untouched; synthesized BRep uses those arenas.
fn delete_native_design(document: &mut serde_json::Value) {
    let namespace = document["native"]["fcstd"].as_object_mut().unwrap();
    let mut removed = namespace
        .get("objects")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|record| {
            record["type_name"].as_str().is_some_and(|kind| {
                kind.contains("PartDesign::")
                    || kind.starts_with("Sketcher::")
                    || (kind.starts_with("Part::") && kind != "Part::Feature")
                    || kind.starts_with("Spreadsheet::")
                    || ["App::Origin", "App::Line", "App::Plane", "App::Point"]
                        .iter()
                        .any(|prefix| kind.starts_with(prefix))
            })
        })
        .filter_map(|record| record["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    assert!(
        !removed.is_empty(),
        "the editor must identify source design objects"
    );
    // Source-layout and census records no longer describe the edited native state.
    for arena in [
        "document",
        "document_facts",
        "physical_ledger",
        "logical_ledger",
        "byte_coverage",
        "archive_spans",
        "design_census",
        "carrier_census",
    ] {
        namespace.remove(arena);
    }
    loop {
        let before = removed.len();
        for (arena, records) in &mut *namespace {
            if arena == "entries" {
                continue;
            }
            if let Some(records) = records.as_array_mut() {
                records.retain(|record| {
                    if has_removed_reference(record, &removed) {
                        if let Some(id) = record["id"].as_str() {
                            removed.insert(id.to_owned());
                        }
                        false
                    } else {
                        true
                    }
                });
            }
        }
        if before == removed.len() {
            break;
        }
    }
    if let Some(entries) = namespace
        .get_mut("entries")
        .and_then(serde_json::Value::as_array_mut)
    {
        for entry in entries {
            if let Some(references) = entry
                .get_mut("referenced_by")
                .and_then(serde_json::Value::as_array_mut)
            {
                references.retain(|reference| !has_removed_reference(reference, &removed));
            }
        }
    }
}

fn has_removed_reference(value: &serde_json::Value, removed: &BTreeSet<String>) -> bool {
    match value {
        serde_json::Value::String(value) => removed.contains(value),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| has_removed_reference(value, removed)),
        serde_json::Value::Object(fields) => fields
            .values()
            .any(|value| has_removed_reference(value, removed)),
        _ => false,
    }
}

fn author_one_box(ir: &CadIr) -> CadIr {
    let mut value = serde_json::to_value(ir).unwrap();
    value["model"]["features"] = json!([{
        "id": "cadir:design:feature#OnlyBox", "ordinal": 0, "name": "OnlyBox", "suppressed": false, "needs_recompute": false,
        "source_tag": "Part::Box", "definition": {"definition": "primitive", "placement": null, "op": "new_body", "solid": {"kind": "box", "length": {"kind":"literal","value":2.0}, "width": {"kind":"literal","value":3.0}, "height": {"kind":"literal","value":4.0}}}
    }]);
    CadIr::from_json(&value.to_string()).unwrap()
}

fn assert_no_original_design(xml: &str) {
    for name in ["Pad", "Pocket", "Fillet", "PadSketch", "PocketSketch"] {
        assert!(
            !xml.contains(&format!("name=\"{name}\"")),
            "original design object {name} was resurrected"
        );
    }
}

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
        assert!(
            decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("Pad")),
            "{kind:?}: {:?}",
            decoded.report().losses
        );
        Command::cargo_bin("cadmpeg")
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
            .success()
            .stderr(predicate::str::contains("loss"));
        let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(report["export"]["write_path"]["path"], "synthesized");
        let written_bytes = fs::read(&output).unwrap();
        let xml = String::from_utf8(entry(&written_bytes, "Document.xml")).unwrap();
        let written = decode(&FcstdCodec, &written_bytes);
        assert!(!written
            .ir()
            .model
            .features
            .iter()
            .any(|feature| feature.name.as_deref() == Some("Pad")));
        match kind {
            MixedKind::Unreadable => assert!(!xml.contains("name=\"Pad\"")),
            MixedKind::Vendor => {
                assert!(xml.contains("Vendor::PartDesign::PadLike"));
                assert!(written
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.code.local_code() == "feature.object-unprojected"));
            }
        }
        let refusal_path = dir.path().join("decode-refusal.json");
        Command::cargo_bin("cadmpeg")
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
            .assert()
            .code(1)
            .stderr(predicate::str::contains("decode reported"));
        let refusal: serde_json::Value =
            serde_json::from_slice(&fs::read(refusal_path).unwrap()).unwrap();
        assert_eq!(refusal["status"], "refused");
        assert_eq!(refusal["refusal"]["code"], "decode_loss_rejected");
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
fn partial_design_native_guard_and_synthesis_do_not_resurrect_unreadable_objects() {
    for kind in [MixedKind::Unreadable, MixedKind::Vendor] {
        let result = decode(&FcstdCodec, &mixed(kind));
        let expected: cadmpeg_ir::document::ArenaClasses = match kind {
            MixedKind::Unreadable => [ArenaClass::Design].into(),
            MixedKind::Vendor => Default::default(),
        };
        assert_eq!(result.source_fidelity().unrepresented(), expected);
        let input = EncodeInput::new(result.ir(), Some(result.source_fidelity()));
        assert_eq!(input.can_reuse_source(), matches!(kind, MixedKind::Vendor));
        let (written, report) = write_decode(
            &FcstdCodec,
            &FcstdCodec,
            result.ir(),
            Some(result.source_fidelity()),
            None,
        );
        assert!(matches!(report.write_path(), WritePath::Synthesized { .. }));
        assert_eq!(written.ir().model.features.len(), 5);
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

#[test]
fn authored_replacement_design_and_geometry_only_scope_drop_original_history() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, Encoder};
    for kind in [MixedKind::Unreadable, MixedKind::Vendor] {
        let result = decode(&FcstdCodec, &mixed(kind));
        for author_box in [false, true] {
            let scope = if author_box {
                ContentScope::full()
            } else {
                ContentScope::geometry()
            };
            let cleared = clear_design(result.ir(), scope);
            let document = if author_box {
                author_one_box(&cleared)
            } else {
                cleared
            };
            let mut bytes = Vec::new();
            FcstdCodec
                .plan(
                    EncodeInput::new(&document, Some(result.source_fidelity())),
                    TargetRequest::Inherit,
                )
                .unwrap()
                .write_to(&mut bytes)
                .unwrap();
            assert_no_original_design(&String::from_utf8(entry(&bytes, "Document.xml")).unwrap());
            let written = decode(&FcstdCodec, &bytes);
            assert_eq!(written.ir().scope, ContentScope::full());
            if author_box {
                assert_eq!(written.ir().model.features.len(), 1);
                assert_eq!(
                    written.ir().model.features[0].name.as_deref(),
                    Some("OnlyBox")
                );
            } else {
                assert!(written.ir().model.features.is_empty());
                assert_eq!(
                    written.ir().model.bodies.len(),
                    result.ir().model.bodies.len()
                );
            }
        }
    }
}
