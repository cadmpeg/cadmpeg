// SPDX-License-Identifier: Apache-2.0

//! End-to-end command behavior and output-contract tests.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Cursor;

use assert_cmd::Command;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::*;
use tempfile::tempdir;

fn fixture(dir: &std::path::Path, name: &str, ir: &cadmpeg_ir::CadIr) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, ir.to_canonical_json().unwrap()).unwrap();
    path
}

fn geometryless_creo(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(
        &path,
        b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n#UGC_TOC\n#END_OF_TOC_HEADER\n#\n#VisibGeom\n\0",
    )
    .unwrap();
    path
}

fn sldprt_cube() -> cadmpeg_ir::CadIr {
    let mut ir = unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    ir
}

#[test]
fn convert_stdout_contains_only_json_artifact() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", input.to_str().unwrap(), "-f", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("validation: OK"));
}

#[test]
fn step_artifact_starts_with_step_header() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", input.to_str().unwrap(), "-f", "step"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.starts_with(b"ISO-10303-21"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("validation:"));
}

#[test]
fn source_less_ir_exports_to_decodable_sldprt() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &sldprt_cube());
    let output = dir.path().join("cube.sldprt");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "sldprt",
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();

    let bytes = fs::read(output).unwrap();
    let decoded = cadmpeg_codec_sldprt::SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 6);
    assert_eq!(decoded.ir.model.edges.len(), 12);
}

#[test]
fn validation_blocks_conversion_unless_overridden() {
    let dir = tempdir().unwrap();
    let mut invalid = unit_cube();
    invalid.model.faces[0].surface.0 = "missing".into();
    let input = fixture(dir.path(), "invalid.json", &invalid);
    let output = dir.path().join("blocked.step");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["validate", input.to_str().unwrap()])
        .assert()
        .code(1);
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("refusing to export"));
    assert!(!output.exists());
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "-o",
            output.to_str().unwrap(),
            "--allow-invalid",
        ])
        .assert()
        .success();
    assert!(output.exists());
}

#[test]
fn output_cannot_replace_input_and_success_is_atomic() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    let original = fs::read(&input).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "json",
            "-o",
            input.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("refusing to overwrite input"));
    assert_eq!(fs::read(&input).unwrap(), original);

    let output = dir.path().join("inferred.step");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(output.exists());
    assert!(!output.with_extension("tmp").exists());
}

#[test]
fn format_is_required_when_stdout_has_no_extension() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["export", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot infer format; pass -f"));
}

#[test]
fn diff_reports_modified_entities_and_uses_diff_exit_codes() {
    let dir = tempdir().unwrap();
    let left = unit_cube();
    let mut right = left.clone();
    right.model.points[0].position.x += 0.5;
    right.model.edges[0].tolerance = Some(0.01);
    right.model.coedges[0].sense = match right.model.coedges[0].sense {
        cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
        cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
    };
    let a = fixture(dir.path(), "a.json", &left);
    let b = fixture(dir.path(), "b.json", &right);

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), a.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("identical"));
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("points: +0 -0 ~1")
                .and(predicate::str::contains("coedges: +0 -0 ~1"))
                .and(predicate::str::contains("edges: +0 -0 ~1")),
        )
        .stderr(predicate::str::is_empty());
}

#[test]
fn garbage_reports_supported_formats() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("garbage.bin");
    fs::write(&input, b"not a CAD file").unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["validate", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "supported: f3d, sldprt, CATPart, NX/Creo prt, Rhino 3DM",
        ));
}

#[test]
fn cadir_override_bypasses_native_detection() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "no-extension", &unit_cube());
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "validate",
            input.to_str().unwrap(),
            "--input-format",
            "cadir",
        ])
        .assert()
        .success();
}

#[test]
fn existing_output_requires_force() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let output = dir.path().join("cube.step");
    fs::write(&output, b"keep").unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("pass --force to overwrite"));
    assert_eq!(fs::read(&output).unwrap(), b"keep");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "-o",
            output.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
    assert!(fs::read(&output).unwrap().starts_with(b"ISO-10303-21"));
}

#[test]
fn input_named_tmp_survives_export_and_temp_names_do_not_collide() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "part.tmp", &unit_cube());
    let original = fs::read(&input).unwrap();
    let cadir = dir.path().join("part.cadir.json");
    let step = dir.path().join("part.step");

    for (format, output) in [("cadir", &cadir), ("step", &step)] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "export",
                input.to_str().unwrap(),
                "-f",
                format,
                "-o",
                output.to_str().unwrap(),
            ])
            .assert()
            .success();
    }

    assert_eq!(fs::read(&input).unwrap(), original);
    assert!(cadir.exists());
    assert!(step.exists());
    assert_eq!(
        fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
            .count(),
        1
    );
}

#[test]
fn exit_codes_distinguish_semantic_and_operational_failures() {
    let dir = tempdir().unwrap();
    let garbage = dir.path().join("garbage");
    fs::write(&garbage, b"garbage").unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["decode", garbage.to_str().unwrap()])
        .assert()
        .code(2);
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", garbage.to_str().unwrap(), garbage.to_str().unwrap()])
        .assert()
        .code(2);

    let mut invalid = unit_cube();
    invalid.model.faces[0].surface.0 = "missing".into();
    let invalid = fixture(dir.path(), "invalid.json", &invalid);
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["validate", invalid.to_str().unwrap()])
        .assert()
        .code(1);
}

#[test]
fn convert_rejects_empty_native_geometry_unless_allowed() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", input.to_str().unwrap(), "-f", "step"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--allow-empty"));
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--allow-empty",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ISO-10303-21"));
}

#[test]
fn export_rejects_container_only_geometry_unless_allowed() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "export",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--container-only",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--allow-empty"));
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "export",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--container-only",
            "--allow-empty",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ISO-10303-21"));
}

#[test]
fn artifact_reports_cover_success_and_semantic_refusal() {
    let dir = tempdir().unwrap();
    let cube = fixture(dir.path(), "cube.json", &unit_cube());
    let success_report = dir.path().join("success-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cube.to_str().unwrap(),
            "-f",
            "step",
            "--report",
            success_report.to_str().unwrap(),
        ])
        .assert()
        .success();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(success_report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["command"], "convert");
    assert!(value["decode_report"].is_null());
    assert!(value["validation_report"].is_object());
    assert_eq!(value["export"]["format"], "step");

    let empty = geometryless_creo(dir.path(), "empty.prt");
    let refusal_report = dir.path().join("refusal-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            empty.to_str().unwrap(),
            "-f",
            "step",
            "--report",
            refusal_report.to_str().unwrap(),
        ])
        .assert()
        .code(1);
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(refusal_report).unwrap()).unwrap();
    assert_eq!(value["command"], "convert");
    assert!(value["decode_report"].is_object());
    assert!(value["validation_report"].is_object());
    assert!(value["export"].is_null());
}

#[test]
fn diff_rejects_input_format_override() {
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", "a", "b", "--input-format", "cadir"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn cadir_extension_is_inferred_and_decode_output_matches_stdout() {
    let dir = tempdir().unwrap();
    let cube = fixture(dir.path(), "cube.json", &unit_cube());
    let inferred = dir.path().join("part.cadir");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "export",
            cube.to_str().unwrap(),
            "-o",
            inferred.to_str().unwrap(),
        ])
        .assert()
        .success();
    serde_json::from_slice::<serde_json::Value>(&fs::read(inferred).unwrap()).unwrap();

    let native = geometryless_creo(dir.path(), "empty.prt");
    let stdout = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["decode", native.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(stdout.status.success());
    let output = dir.path().join("empty.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "decode",
            native.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(stdout.stdout, fs::read(output).unwrap());
}

#[test]
fn reporting_commands_emit_versioned_json_only_on_stdout() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    let validate = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["validate", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&validate.stdout).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["command"], "validate");

    let diff = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            input.to_str().unwrap(),
            input.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&diff.stdout).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["command"], "diff");

    let native = geometryless_creo(dir.path(), "ambiguous.bin");
    let inspect = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "inspect",
            native.to_str().unwrap(),
            "--input-format",
            "creo",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let value: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["command"], "inspect");
}

#[test]
fn cadir_format_name_and_json_alias_both_work() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    for format in ["cadir", "json"] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["export", input.to_str().unwrap(), "-f", format])
            .assert()
            .success()
            .stdout(predicate::str::starts_with("{"));
    }
}

#[test]
fn explicit_format_warns_when_known_extension_disagrees() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    let output = dir.path().join("cube.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "export",
            input.to_str().unwrap(),
            "-f",
            "step",
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "explicit format step disagrees with output extension format cadir",
        ));
}
