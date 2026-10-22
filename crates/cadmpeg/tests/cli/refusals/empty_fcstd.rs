// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::{Cursor, Write};

use assert_cmd::Command;
use cadmpeg_ir::{CadIr, Codec, ContentScope, DecodeOptions};
use serde_json::Value;
use tempfile::tempdir;

use crate::support::fixture;

fn warnings(report: &Value) -> Vec<&Value> {
    report["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|loss| {
            loss["code"]["namespace"] == "export" && loss["code"]["code"] == "output.empty"
        })
        .collect()
}

fn assert_no_geometry(ir: &CadIr) {
    assert!(ir.model.bodies.is_empty());
    assert!(ir.model.points.is_empty());
    assert!(ir.model.curves.is_empty());
    assert!(ir.model.surfaces.is_empty());
    assert!(ir.model.procedural_curves.is_empty());
    assert!(ir.model.procedural_surfaces.is_empty());
    assert!(ir.model.tessellations.is_empty());
    assert!(ir.model.subds.is_empty());
}

#[test]
fn document_note_is_preserved_but_does_not_make_fcstd_geometry_nonempty() {
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='Note' type='App::PropertyString'><String value='metadata stays'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("Document.xml", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(document).unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let decoded = cadmpeg_codec_freecad::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_no_geometry(decoded.ir());
    let unrepresented = decoded.source_fidelity().unrepresented();
    let native_patch_eligible = unrepresented.is_empty();
    let unrepresented_classes = serde_json::to_value(&unrepresented).unwrap();
    let assert_path = |report: &Value, case: &str| {
        let native = case == "native";
        assert_eq!(
            report["export"]["write_path"]["path"],
            if native && native_patch_eligible {
                "patched"
            } else {
                "synthesized"
            }
        );
        assert_eq!(
            report["export"]["write_path"]["fidelity"]["status"],
            if native {
                if native_patch_eligible {
                    "replayed"
                } else {
                    "not_consumed"
                }
            } else {
                "not_provided"
            }
        );
        if native {
            if native_patch_eligible {
                assert!(report["export"]["synthesis_cause"].is_null());
            } else {
                assert_eq!(
                    report["export"]["synthesis_cause"],
                    serde_json::json!({
                        "cause": "source_unrepresented",
                        "classes": unrepresented_classes,
                    })
                );
            }
        }
    };
    let dir = tempdir().unwrap();
    let native = dir.path().join("note.FCStd");
    fs::write(&native, bytes).unwrap();
    let cadir = fixture(dir.path(), "note.cadir.json", decoded.ir());
    for (case, input) in [("native", native), ("cadir", cadir)] {
        let output = dir.path().join(format!("{case}.FCStd"));
        let report = dir.path().join(format!("{case}.json"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "--to",
                "fcstd:schema-4",
                "-o",
                output.to_str().unwrap(),
                "--report",
                report.to_str().unwrap(),
            ])
            .assert()
            .success();
        let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(report["status"], "ok");
        assert_path(&report, case);
        let empty = warnings(&report);
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0]["severity"], "warning");
        let reopened = cadmpeg_codec_freecad::FcstdCodec
            .decode(
                &mut Cursor::new(fs::read(output).unwrap()),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_no_geometry(reopened.ir());
        let notes = reopened
            .ir()
            .model
            .attributes
            .iter()
            .filter(|attribute| attribute.name == "Note")
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 1);
        assert_eq!(
            notes[0].values,
            [cadmpeg_ir::attributes::AttributeValue::String(
                "metadata stays".into()
            )]
        );
        let refused = dir.path().join(format!("{case}-refused.FCStd"));
        let report = dir.path().join(format!("{case}-refused.json"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "--to",
                "fcstd:schema-4",
                "-o",
                refused.to_str().unwrap(),
                "--reject-empty",
                "--report",
                report.to_str().unwrap(),
            ])
            .assert()
            .code(1);
        assert!(!refused.exists());
        let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(report["refusal"]["code"], "empty_geometry");
        assert_path(&report, case);
        assert_eq!(warnings(&report).len(), 1);
    }
}

#[test]
fn current_native_brep_without_a_neutral_model_writes_nonempty_fcstd_geometry() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
    ));
    let decoded = cadmpeg_codec_freecad::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let expected = decoded.ir().model.bodies.len();
    assert!(expected > 0);
    assert!(!decoded.ir().native.namespace("fcstd").unwrap().arenas()["shape_payloads"].is_empty());
    let mut ir = decoded.ir().clone();
    ir.model = Default::default();
    ir.set_scope(ContentScope::container()).unwrap();
    assert_no_geometry(&ir);
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "native-shape.cadir.json", &ir);
    let output = dir.path().join("native-shape.FCStd");
    let report = dir.path().join("native-shape.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "fcstd:schema-4",
            "-o",
            output.to_str().unwrap(),
            "--reject-empty",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert!(warnings(&report).is_empty());
    assert_eq!(report["export"]["write_path"]["path"], "synthesized");
    assert_eq!(
        report["export"]["write_path"]["fidelity"]["status"],
        "not_provided"
    );
    let reopened = cadmpeg_codec_freecad::FcstdCodec
        .decode(
            &mut Cursor::new(fs::read(output).unwrap()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(reopened.ir().model.bodies.len(), expected);
}

#[test]
fn an_emitted_group_without_geometry_is_empty_but_its_late_brep_attachment_is_not() {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };
    for attached in [false, true] {
        let mut ir = if attached {
            cadmpeg_ir::examples::unit_cube().unwrap()
        } else {
            CadIr::empty()
        };
        let expected_bodies = ir.model.bodies.len();
        let outputs = ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let operation: FeatureOperation = serde_json::from_value(serde_json::json!({
            "definition":"tree_node", "role":"solid_bodies",
            "children":{"children":[]}
        }))
        .unwrap();
        ir.model.features.push(Feature {
            id: FeatureId::mint("test:feature:grouping#0").unwrap(),
            ordinal: 0,
            name: Some("Grouping".into()),
            suppressed: Some(false),
            needs_recompute: false,
            dependencies: DistinctMembers::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::new(FeatureDefinition::Operation(operation), outputs),
            native_ref: None,
        });
        let dir = tempdir().unwrap();
        let input = fixture(dir.path(), "group.cadir.json", &ir);
        let output = dir.path().join("group.FCStd");
        let report = dir.path().join("group.json");
        let mut command = Command::cargo_bin("cadmpeg").unwrap();
        command.args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "fcstd:schema-4",
            "-o",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ]);
        if attached {
            command.arg("--reject-empty");
        }
        command.assert().success();
        let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(report["export"]["write_path"]["path"], "synthesized");
        assert_eq!(
            report["export"]["write_path"]["fidelity"]["status"],
            "not_provided"
        );
        assert_eq!(warnings(&report).len(), usize::from(!attached));
        let reopened = cadmpeg_codec_freecad::FcstdCodec
            .decode(
                &mut Cursor::new(fs::read(output).unwrap()),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(reopened.ir().model.bodies.len(), expected_bodies);
        let objects = &reopened.ir().native.namespace("fcstd").unwrap().arenas()["objects"];
        assert_eq!(objects.len(), 1);
        let properties = &reopened.ir().native.namespace("fcstd").unwrap().arenas()["properties"];
        let shape = properties
            .iter()
            .filter(|property| {
                property.fields().unwrap().get("name") == Some(&serde_json::json!("Shape"))
            })
            .count();
        assert_eq!(shape, usize::from(attached));
        assert!(reopened
            .ir()
            .model
            .features
            .iter()
            .any(|feature| feature.name.as_deref() == Some("Grouping")));
        if !attached {
            assert_no_geometry(reopened.ir());
            assert_eq!(warnings(&report)[0]["severity"], "warning");
            let refused = dir.path().join("refused.FCStd");
            Command::cargo_bin("cadmpeg")
                .unwrap()
                .args([
                    "convert",
                    input.to_str().unwrap(),
                    "--to",
                    "fcstd:schema-4",
                    "-o",
                    refused.to_str().unwrap(),
                    "--reject-empty",
                ])
                .assert()
                .code(1);
            assert!(!refused.exists());
        }
    }
}
