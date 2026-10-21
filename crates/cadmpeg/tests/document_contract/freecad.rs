// SPDX-License-Identifier: Apache-2.0
//! FreeCAD document edits survive both sidecar artifact flows.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::codec::write::{Encoder, SynthesisCause};
use cadmpeg_ir::document::ContentScope;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{decode_sidecar_path, CadIr, DecodeSidecar};
use predicates::prelude::predicate;
use serde_json::{json, Value};
use tempfile::tempdir;

use super::{decode, workspace, write_decode};

const EXPRESSION: &str = "37 mm";
const CONSTRAINT_EXPRESSION: &str = "31 mm";
const FEATURE_NAME: &str = "AuthoredPad";
const EDIT_COLOR: [u8; 4] = [48, 96, 144, 255];
const EPS_PARAMETER_VALUE: f64 = 1e-9;

fn fixture(name: &str) -> Vec<u8> {
    fs::read(workspace().join("corpus/freecad_fcstd/fixtures").join(name)).unwrap()
}

fn run_dump(input: &Path, output: &Path) {
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "dump",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();
}

fn read_ir(path: &Path) -> CadIr {
    CadIr::from_json(&fs::read_to_string(path).unwrap()).unwrap()
}

fn read_sidecar(path: &Path) -> DecodeSidecar {
    DecodeSidecar::from_json(&fs::read_to_string(decode_sidecar_path(path).unwrap()).unwrap())
        .unwrap()
}

fn edit_document(ir: &CadIr) -> CadIr {
    use cadmpeg_ir::features::{Expression, ParameterValue};
    use cadmpeg_ir::scalar::Length;

    let mut edited = ir.clone();
    let feature_id = edited
        .model
        .features
        .iter()
        .find(|feature| {
            feature.id.as_str().ends_with("#Pad")
                || feature.id.as_str().ends_with("#DesignedFeature")
        })
        .expect("the fixture holds an extrusion feature")
        .id
        .clone();
    let length = edited
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.id.as_str().ends_with("#Pad:Length")
                || parameter.id.as_str().ends_with("#DesignedFeature:Length")
        })
        .expect("Pad.Length is projected")
        .id
        .clone();
    let feature = edited
        .model
        .features
        .iter()
        .find(|feature| feature.id == feature_id)
        .unwrap();
    let definition = serde_json::to_value(feature.definition()).unwrap();
    let paths = feature
        .definition()
        .dimension_paths()
        .into_iter()
        .filter(|path| {
            definition.pointer(path).is_some_and(|dimension| {
                dimension["kind"] == "parameter"
                    && dimension["value"].as_str() == Some(length.as_str())
            })
        })
        .collect::<Vec<_>>();
    if feature.id.as_str().ends_with("#Pad") {
        assert_eq!(
            paths.len(),
            1,
            "the extrusion consumes its one canonical Length parameter"
        );
        edited
            .set_dimension(&feature_id, &paths[0], Length::new(37.0).unwrap())
            .unwrap();
    } else {
        // PartDesign::Feature retains evaluated geometry and an independent
        // Length property. That property does not assert an extrusion recipe.
        assert!(matches!(
            feature.definition().operation(),
            cadmpeg_ir::features::FeatureOperation::StoredGeometry {}
        ));
        assert!(paths.is_empty());
        let value = ParameterValue::Length(Length::new(37.0).unwrap());
        edited
            .set_parameter_expression(&length, Expression::literal(&value), Some(value))
            .unwrap();
    }
    edited
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id == feature_id)
        .unwrap()
        .name = Some(FEATURE_NAME.into());
    let constraint = edited
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.id.as_str().contains(":constraint:"))
        .expect("the fixture has a dimensional sketch constraint")
        .id
        .clone();
    let value = ParameterValue::Length(Length::new(31.0).unwrap());
    edited
        .set_parameter_expression(&constraint, Expression::literal(&value), Some(value))
        .unwrap();
    assert!(
        edited
            .model
            .features
            .iter()
            .find(|feature| feature.id == feature_id)
            .unwrap()
            .needs_recompute
    );
    let mut value = serde_json::to_value(&edited).unwrap();
    let model = value["model"].as_object_mut().unwrap();
    let body = model["bodies"][0]["id"].clone();
    let appearance_id = "cadir:model:appearance#authored";
    let color = cadmpeg_ir::topology::Color::from_rgba8(
        EDIT_COLOR[0],
        EDIT_COLOR[1],
        EDIT_COLOR[2],
        EDIT_COLOR[3],
    );
    let appearances = model["appearances"].as_array_mut().unwrap();
    if let Some(appearance) = appearances.first_mut() {
        appearance["base_color"] = serde_json::to_value(color).unwrap();
    } else {
        appearances
            .push(json!({"id": appearance_id, "name": "Authored appearance", "base_color": color}));
        model["appearance_bindings"].as_array_mut().unwrap().push(json!({
            "id": "cadir:model:appearance-binding#authored", "target": {"kind": "body", "id": body}, "appearance": appearance_id
        }));
    }
    CadIr::from_json(&value.to_string())
        .expect("the canonical design and display edits form a valid document")
}

fn assert_edits(ir: &CadIr, fixture_name: &str) {
    assert!(ir
        .model
        .features
        .iter()
        .any(|feature| feature.name.as_deref() == Some(FEATURE_NAME)));
    let length = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.expression.literal_text().as_deref() == Some(EXPRESSION))
        .expect("the edited canonical length expression survives");
    assert!(
        matches!(&length.value, Some(cadmpeg_ir::features::ParameterValue::Length(value)) if (value.get()-37.0).abs() <= EPS_PARAMETER_VALUE)
    );
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(FEATURE_NAME))
        .unwrap();
    let definition = serde_json::to_value(feature.definition()).unwrap();
    let bindings = feature
        .definition()
        .dimension_paths()
        .into_iter()
        .filter(|path| {
            definition.pointer(path).is_some_and(|dimension| {
                dimension["kind"] == "parameter"
                    && dimension["value"].as_str() == Some(length.id.as_str())
            })
        })
        .count();
    if fixture_name == "core_design_product.FCStd" {
        assert!(matches!(
            feature.definition().operation(),
            cadmpeg_ir::features::FeatureOperation::StoredGeometry {}
        ));
        assert_eq!(bindings, 0, "a stored result invents no extrusion recipe");
        assert_eq!(length.owner.as_ref(), Some(&feature.id));
    } else {
        assert!(matches!(
            feature.definition().operation(),
            cadmpeg_ir::features::FeatureOperation::Extrude { .. }
        ));
        assert_eq!(
            bindings, 1,
            "the extrusion keeps its canonical parameter binding"
        );
    }
    assert!(
        feature.needs_recompute,
        "FreeCAD must retain the edited feature's Touched state"
    );
    let dimensional = ir
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.expression.literal_text().as_deref() == Some(CONSTRAINT_EXPRESSION)
        })
        .expect("the edited constraint expression survives");
    assert!(
        matches!(&dimensional.value,Some(cadmpeg_ir::features::ParameterValue::Length(value)) if (value.get()-31.0).abs()<=EPS_PARAMETER_VALUE),
        "the edited constraint's numeric value must also survive"
    );
    let color = cadmpeg_ir::topology::Color::from_rgba8(
        EDIT_COLOR[0],
        EDIT_COLOR[1],
        EDIT_COLOR[2],
        EDIT_COLOR[3],
    );
    assert!(ir.model.appearance_bindings.iter().any(|binding| {
        ir.model.appearances.iter().any(|appearance|appearance.id==binding.appearance && appearance.base_color==Some(color))
            && matches!(&binding.target,cadmpeg_ir::appearance::AppearanceTarget::Body(body) if ir.model.bodies.iter().any(|candidate|candidate.id==*body))
    }),"the edited color must remain bound to a written body");
}

fn counts(ir: &CadIr) -> [usize; 9] {
    [
        ir.model.features.len(),
        ir.model.sketches.len(),
        ir.model.sketch_entities.len(),
        ir.model.sketch_constraints.len(),
        ir.model.parameters.len(),
        ir.model.bodies.len(),
        ir.model.spreadsheets.len(),
        ir.model.product_definitions.len(),
        ir.model.occurrences.len(),
    ]
}

fn assert_history_parameters(ir: &CadIr) {
    use cadmpeg_ir::features::ParameterValue;
    use std::collections::BTreeSet;

    let original = BTreeSet::from([
        "Fillet:Radius",
        "Pad:Length",
        "Pad:Length2",
        "PadSketch:constraint:3",
        "PadSketch:constraint:4",
        "Pocket:Length",
        "Pocket:Length2",
        "PocketSketch:constraint:1",
    ]);
    let suffix = |parameter: &cadmpeg_ir::features::DesignParameter| {
        parameter
            .id
            .as_str()
            .strip_prefix("fcstd:design:parameter#")
            .unwrap()
            .to_owned()
    };
    let retained = ir
        .model
        .parameters
        .iter()
        .map(suffix)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        retained.len(),
        ir.model.parameters.len(),
        "parameter identities are unique"
    );
    assert_eq!(
        retained
            .iter()
            .filter(|id| original.contains(id.as_str()))
            .count(),
        8
    );
    let mut additional = BTreeSet::new();
    // These eight source properties were absent from the old neutral projection.
    // They are separate controls, not copies of Length or sketch dimensions.
    for owner in ["Pad", "Pocket"] {
        for name in ["Offset", "Offset2", "TaperAngle", "TaperAngle2"] {
            let id = format!("{owner}:{name}");
            additional.insert(id.clone());
            let parameter = ir
                .model
                .parameters
                .iter()
                .find(|p| suffix(p) == id)
                .unwrap();
            assert_eq!(parameter.name, name);
            assert_eq!(
                parameter.owner.as_ref().unwrap().as_str(),
                format!("fcstd:design:feature#{owner}")
            );
            assert_eq!(
                parameter.native_ref.as_deref(),
                Some(format!("fcstd:native:property#{id}").as_str())
            );
            if name.starts_with("TaperAngle") {
                assert!(
                    matches!(&parameter.value, Some(ParameterValue::Angle(value)) if value.get() == 0.0)
                );
                assert_eq!(
                    parameter.expression.literal_text().as_deref(),
                    Some("0 rad")
                );
            } else {
                assert!(
                    matches!(&parameter.value, Some(ParameterValue::Length(value)) if value.get() == 0.0)
                );
                assert_eq!(parameter.expression.literal_text().as_deref(), Some("0 mm"));
            }
        }
    }
    assert_eq!(
        retained,
        original
            .into_iter()
            .map(str::to_owned)
            .chain(additional)
            .collect()
    );
}

fn assert_full_decode(ir: &CadIr, sidecar: &DecodeSidecar) {
    assert_eq!(ir.scope, ContentScope::full());
    assert_eq!(sidecar.report.scope(), ir.scope);
    assert!(sidecar.fidelity.unrepresented().is_empty());
}

#[test]
fn cadir_edits_with_deleted_sidecar_synthesize_the_edited_document() {
    for name in ["design_history.FCStd", "core_design_product.FCStd"] {
        let dir = tempdir().unwrap();
        let input = dir.path().join(name);
        fs::write(&input, fixture(name)).unwrap();
        let cadir = dir.path().join("a.cadir.json");
        let output = dir.path().join("b.FCStd");
        let redump = dir.path().join("c.cadir.json");
        let report_path = dir.path().join("convert.json");
        run_dump(&input, &cadir);
        let before = read_ir(&cadir);
        let sidecar = read_sidecar(&cadir);
        assert_full_decode(&before, &sidecar);
        assert!(sidecar.report.losses.is_empty(), "{name}");
        if name == "design_history.FCStd" {
            assert_history_parameters(&before);
            assert_eq!(counts(&before)[..6], [6, 2, 5, 5, 16, 8]);
        }
        let edited = edit_document(&before);
        fs::write(&cadir, edited.to_canonical_json().unwrap()).unwrap();
        let arguments = [
            "convert",
            cadir.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--report",
            report_path.to_str().unwrap(),
        ];
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(arguments)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("does not match"));
        assert!(!output.exists());
        fs::remove_file(decode_sidecar_path(&cadir).unwrap()).unwrap();
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(arguments)
            .assert()
            .success();
        let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(report["export"]["write_path"]["path"], "synthesized");
        assert!(!report["export"]["losses"].as_array().unwrap().is_empty());
        run_dump(&output, &redump);
        let written = read_ir(&redump);
        assert_full_decode(&written, &read_sidecar(&redump));
        assert_edits(&written, name);
        assert_eq!(counts(&written), counts(&edited), "{name}");
        let library_report = FcstdCodec
            .plan(
                cadmpeg_ir::codec::write::EncodeInput::new(&edited, None),
                cadmpeg_ir::codec::write::target::TargetRequest::Inherit,
            )
            .unwrap();
        cadmpeg_test_support::contract::assert_written_arenas_agree(
            library_report.report(),
            &edited,
            &written,
        );
        assert!(
            written.model.features.iter().any(|feature| {
                feature.name.as_deref() == Some(FEATURE_NAME) && feature.needs_recompute
            }) || report["export"]["losses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|loss| {
                    loss["code"]["code"] == "evaluation.stale"
                        && matches!(
                            loss["severity"].as_str(),
                            Some("warning" | "error" | "blocking")
                        )
                }),
            "{name}: a stale evaluation is written explicitly or reported at Warning severity: {report}"
        );
        #[cfg(feature = "step")]
        assert_step_geometry_agrees(&cadir, &output, dir.path());
    }
}

#[cfg(feature = "step")]
fn assert_step_geometry_agrees(cadir: &Path, native: &Path, directory: &Path) {
    let mut body_counts = Vec::new();
    for (index, input) in [cadir, native].into_iter().enumerate() {
        let step = directory.join(format!("geometry-{index}.step"));
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
        let result = decode(
            &cadmpeg_codec_step::StepCodec::default(),
            &fs::read(step).unwrap(),
        );
        body_counts.push(result.ir().model.bodies.len());
    }
    assert_eq!(body_counts[0], body_counts[1]);
}

#[test]
fn library_editor_rebinds_history_before_synthesizing_its_edits() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    for name in ["design_history.FCStd", "core_design_product.FCStd"] {
        let dir = tempdir().unwrap();
        let result = decode(&FcstdCodec, &fixture(name));
        let edited = edit_document(result.ir());
        let cadir = dir.path().join("editor.cadir.json");
        let text = edited.to_canonical_json().unwrap();
        fs::write(&cadir, &text).unwrap();
        let sidecar = DecodeSidecar::bind_sha256(
            Sha256Digest::digest(text.as_bytes()),
            result.report().clone(),
            result.source_fidelity().clone(),
        );
        fs::write(
            decode_sidecar_path(&cadir).unwrap(),
            sidecar.to_canonical_json().unwrap(),
        )
        .unwrap();
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                cadir.to_str().unwrap(),
                "-f",
                "cadir",
                "--reject-lossy=decode",
            ])
            .assert()
            .success()
            .stderr(predicate::str::contains("decode that produced this CADIR"));
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(&edited, Some(result.source_fidelity())),
                TargetRequest::Inherit,
            )
            .unwrap();
        assert!(matches!(
            plan.report().write_path(),
            WritePath::Synthesized { .. }
        ));
        assert!(matches!(
            plan.report().synthesis_cause(),
            Some(SynthesisCause::PatchDeclined { .. })
        ));
        let (written, report) = write_decode(
            &FcstdCodec,
            &FcstdCodec,
            &edited,
            Some(result.source_fidelity()),
            None,
        );
        assert_edits(written.ir(), name);
        assert_eq!(counts(written.ir()), counts(&edited));
        cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &edited, written.ir());
        let native_output = dir.path().join("rebound.FCStd");
        let output = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                cadir.to_str().unwrap(),
                "-o",
                native_output.to_str().unwrap(),
            ])
            .assert()
            .success();
        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        assert_eq!(
            stderr
                .matches("synthesized because patch was declined:")
                .count(),
            1
        );
        let Some(SynthesisCause::PatchDeclined { reason }) = report.synthesis_cause() else {
            panic!("an edited retained document declines patching");
        };
        assert!(stderr.contains(reason));
        for (floor, severity) in [
            ("info", cadmpeg_ir::report::Severity::Info),
            ("error", cadmpeg_ir::report::Severity::Error),
        ] {
            let output = dir.path().join(format!("{floor}.FCStd"));
            let assertion = Command::cargo_bin("cadmpeg")
                .unwrap()
                .args([
                    "convert",
                    cadir.to_str().unwrap(),
                    "-o",
                    output.to_str().unwrap(),
                    "--reject-lossy",
                    "--reject-lossy-floor",
                    floor,
                ])
                .assert();
            let assertion = if report.losses.iter().any(|loss| loss.severity >= severity) {
                assertion.code(1)
            } else {
                assertion.success()
            };
            let stderr = String::from_utf8_lossy(&assertion.get_output().stderr);
            assert_eq!(
                stderr
                    .matches("synthesized because patch was declined:")
                    .count(),
                1
            );
            assert!(stderr.contains(reason));
        }
    }
}

#[test]
fn unedited_patch_supplies_no_content_beyond_the_synthesized_written_arenas() {
    for name in ["design_history.FCStd", "core_design_product.FCStd"] {
        let result = decode(&FcstdCodec, &fixture(name));
        let (patched, patch_report) = write_decode(
            &FcstdCodec,
            &FcstdCodec,
            result.ir(),
            Some(result.source_fidelity()),
            None,
        );
        let (synthesized, synth_report) =
            write_decode(&FcstdCodec, &FcstdCodec, result.ir(), None, None);
        assert!(matches!(
            patch_report.write_path(),
            WritePath::Patched { .. }
        ));
        assert!(matches!(
            synth_report.write_path(),
            WritePath::Synthesized { .. }
        ));
        cadmpeg_test_support::contract::assert_written_arenas_agree(
            &synth_report,
            patched.ir(),
            synthesized.ir(),
        );
    }
}
