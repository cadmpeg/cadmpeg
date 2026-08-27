// SPDX-License-Identifier: Apache-2.0
//! Convert: destination rules, format selection, and write-back fidelity.

use std::fs;
use std::io::Cursor;

use assert_cmd::Command;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::*;
use tempfile::tempdir;

use crate::support::*;

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
    assert!(String::from_utf8_lossy(&output.stderr).contains("check: OK"));
}

#[test]
fn step_artifact_starts_with_step_header() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--step-target",
            "ap242e3",
            "--reject-step-losses",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.starts_with(b"ISO-10303-21"));
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("check:"));
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
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 6);
    assert_eq!(decoded.ir().model.edges.len(), 12);
}

#[test]
fn source_less_ir_exports_to_decodable_rhino() {
    let dir = tempdir().unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.points.push(cadmpeg_ir::topology::Point {
        id: cadmpeg_ir::ids::PointId("cadir:model:point#cli".into()),
        position: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let input = fixture(dir.path(), "point.cadir.json", &ir);
    let output = dir.path().join("point.3dm");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--allow-errors",
        ])
        .assert()
        .success();

    let decoded = cadmpeg_codec_rhino::RhinoCodec
        .decode(
            &mut Cursor::new(fs::read(output).unwrap()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(
        decoded.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    );
}

#[test]
fn rhino_output_version_is_selected_explicitly() {
    let dir = tempdir().unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.points.push(cadmpeg_ir::topology::Point {
        id: cadmpeg_ir::ids::PointId("cadir:model:point#version".into()),
        position: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let input = fixture(dir.path(), "point.cadir.json", &ir);
    let output = dir.path().join("point.3dm");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--rhino-target",
            "60",
            "--allow-errors",
        ])
        .assert()
        .success();
    assert_eq!(&fs::read(output).unwrap()[24..32], b"      60");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "cadir",
            "--rhino-target",
            "60",
            "--allow-errors",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires Rhino output"));
}

#[test]
fn check_blocks_conversion_unless_overridden() {
    let dir = tempdir().unwrap();
    let mut invalid = unit_cube();
    invalid.model.faces[0].surface.0 = "missing".into();
    let input = fixture(dir.path(), "invalid.json", &invalid);
    let output = dir.path().join("blocked.step");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap()])
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
            "--allow-errors",
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
        .args(["convert", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot infer format; pass -f"));
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
fn input_named_tmp_survives_convert_and_temp_names_do_not_collide() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "part.tmp", &unit_cube());
    let original = fs::read(&input).unwrap();
    let cadir = dir.path().join("part.cadir.json");
    let step = dir.path().join("part.step");

    for (format, output) in [("cadir", &cadir), ("step", &step)] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
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
fn cadir_extension_is_inferred_and_decode_output_matches_stdout() {
    let dir = tempdir().unwrap();
    let cube = fixture(dir.path(), "cube.json", &unit_cube());
    let inferred = dir.path().join("part.cadir");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cube.to_str().unwrap(),
            "-o",
            inferred.to_str().unwrap(),
        ])
        .assert()
        .success();
    serde_json::from_slice::<serde_json::Value>(&fs::read(&inferred).unwrap()).unwrap();

    let native = geometryless_creo(dir.path(), "empty.prt");
    let stdout = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", native.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(stdout.status.success());
    assert!(String::from_utf8_lossy(&stdout.stderr)
        .contains("stdout cannot carry its decode-fidelity sidecar"));
    let output = dir.path().join("empty.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "dump",
            native.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(stdout.stdout, fs::read(&output).unwrap());
    let sidecar_path = cadmpeg_ir::decode_sidecar_path(&output);
    let sidecar = cadmpeg_ir::DecodeSidecar::from_json(
        &fs::read_to_string(&sidecar_path).expect("decode writes fidelity sidecar"),
    )
    .unwrap();
    assert!(sidecar.matches(&fs::read(&output).unwrap()));

    let neutral_sidecar = cadmpeg_ir::decode_sidecar_path(&inferred);
    fs::write(&neutral_sidecar, "stale").unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cube.to_str().unwrap(),
            "-o",
            inferred.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
    assert!(!neutral_sidecar.exists());
}

#[test]
fn fidelity_sidecar_replays_native_bytes_and_missing_sidecar_refuses_prewrite() {
    let dir = tempdir().unwrap();
    for format in ["f3d", "sldprt"] {
        let source_ir = if format == "sldprt" {
            sldprt_cube()
        } else {
            unit_cube()
        };
        let cube = fixture(dir.path(), &format!("cube-{format}.cadir.json"), &source_ir);
        let native = dir.path().join(format!("source.{format}"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                cube.to_str().unwrap(),
                "-o",
                native.to_str().unwrap(),
            ])
            .assert()
            .success();

        let persisted = dir.path().join(format!("decoded-{format}.cadir.json"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "dump",
                native.to_str().unwrap(),
                "-o",
                persisted.to_str().unwrap(),
            ])
            .assert()
            .success();
        let sidecar = cadmpeg_ir::decode_sidecar_path(&persisted);
        assert!(sidecar.exists());

        let replay = dir.path().join(format!("replay.{format}"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                persisted.to_str().unwrap(),
                "-o",
                replay.to_str().unwrap(),
            ])
            .assert()
            .success();
        assert_eq!(fs::read(&native).unwrap(), fs::read(&replay).unwrap());

        fs::remove_file(sidecar).unwrap();
        let refused = dir.path().join(format!("refused.{format}"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                persisted.to_str().unwrap(),
                "-o",
                refused.to_str().unwrap(),
                "--reject-lossy",
            ])
            .assert()
            .code(1)
            .stderr(
                predicate::str::contains("Preserved")
                    .or(predicate::str::contains("export planning reported 1 loss")),
            );
        assert!(!refused.exists());
    }
}

#[test]
fn cadir_format_name_and_json_alias_both_work() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    for format in ["cadir", "json"] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["convert", input.to_str().unwrap(), "-f", format])
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
            "convert",
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

#[test]
fn convert_refuses_binary_output_to_stdout() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = fixture(dir.path(), "cube.cadir.json", &ir);
    let path = model.to_str().unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "--format", "sldprt"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("refusing to write binary sldprt")
                .and(predicate::str::contains("--input-format sldprt"))
                .and(predicate::str::contains("--binary-stdout")),
        );

    // With -o the write succeeds (Rhino is the binary writer that accepts a
    // source-less IR; the guard question is the destination, not the codec).
    let mut point_ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    point_ir.model.points.push(cadmpeg_ir::topology::Point {
        id: cadmpeg_ir::ids::PointId("cadir:model:point#guard".into()),
        position: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let point = fixture(dir.path(), "point.cadir.json", &point_ir);
    let out = dir.path().join("point.3dm");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            point.to_str().unwrap(),
            "--format",
            "rhino",
            "-o",
            out.to_str().unwrap(),
            "--allow-errors",
        ])
        .assert()
        .success();
    let written = fs::read(&out).unwrap();
    assert!(written.starts_with(b"3D Geometry File Format"));

    let streamed = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            point.to_str().unwrap(),
            "--format",
            "rhino",
            "--allow-errors",
            "--binary-stdout",
        ])
        .output()
        .unwrap();
    assert!(streamed.status.success());
    assert!(streamed.stdout.starts_with(b"3D Geometry File Format"));

    // Text formats to stdout stay untouched by the guard.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "--format", "step"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ISO-10303-21;"));
}

#[test]
fn from_and_to_aliases_match_input_format_and_format() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = fixture(dir.path(), "cube.cadir.json", &ir);
    let path = model.to_str().unwrap();

    let long = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            path,
            "--input-format",
            "cadir",
            "--format",
            "step",
        ])
        .output()
        .unwrap();
    let aliased = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "--from", "cadir", "--to", "step"])
        .output()
        .unwrap();
    assert!(long.status.success());
    assert!(aliased.status.success());
    assert_eq!(long.stdout, aliased.stdout);
}

/// `convert in.igs -o out.igs` on a file that is not the writer's newest
/// version keeps the bytes it was handed.
///
/// The command line builds no target for a same-format conversion, so the
/// encoder is asked to inherit, and the resolved dialect is the source's. Under
/// the old CLI-side default the file came back as 5.3 and the replay was
/// silently dropped, which is the round trip a user's own tool could no longer
/// open. The explicit target is still the escape, and it produces different
/// bytes, which is what proves the first conversion preserved rather than
/// happening to agree.
#[test]
#[cfg(feature = "iges")]
fn a_same_format_convert_replays_a_non_default_iges_version() {
    let dir = tempdir().unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.points.push(cadmpeg_ir::topology::Point {
        id: cadmpeg_ir::ids::PointId("cadir:model:point#iges".into()),
        position: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    let input = fixture(dir.path(), "point.cadir.json", &ir);
    let original = dir.path().join("v51.igs");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            original.to_str().unwrap(),
            "--iges-target",
            "5.1",
            "--allow-errors",
        ])
        .assert()
        .success();
    let original_bytes = fs::read(&original).unwrap();

    let inherited = dir.path().join("inherited.igs");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            original.to_str().unwrap(),
            "-o",
            inherited.to_str().unwrap(),
            "--allow-errors",
        ])
        .assert()
        .success();
    assert_eq!(fs::read(&inherited).unwrap(), original_bytes);

    let upgraded = dir.path().join("upgraded.igs");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            original.to_str().unwrap(),
            "-o",
            upgraded.to_str().unwrap(),
            "--iges-target",
            "5.3",
            "--allow-errors",
        ])
        .assert()
        .success();
    assert_ne!(fs::read(&upgraded).unwrap(), original_bytes);
}
