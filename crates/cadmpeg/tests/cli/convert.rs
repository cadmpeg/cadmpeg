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

/// `--to` names the schema, in both spellings the grammar admits.
///
/// `step:ap242-e3` is the registry id; `ap242e3` is the catalog alias. The
/// alias is usable bare when the output extension names the format and after a
/// format prefix. Every spelling must reach the same `FILE_SCHEMA`.
#[test]
fn step_artifact_starts_with_step_header() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());

    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "step:ap242-e3",
            "--reject-lossy=export",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.starts_with(b"ISO-10303-21"));
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("check:"));

    let aliased = dir.path().join("cube.step");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            aliased.to_str().unwrap(),
            "--to",
            "ap242e3",
        ])
        .assert()
        .success();
    assert!(String::from_utf8_lossy(&fs::read(&aliased).unwrap())
        .contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }"));

    let colon_aliased = dir.path().join("cube-colon-alias.step");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-o",
            colon_aliased.to_str().unwrap(),
            "--to",
            "step:ap242e3",
        ])
        .assert()
        .success();
    assert!(String::from_utf8_lossy(&fs::read(&colon_aliased).unwrap())
        .contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }"));
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

/// The Rhino archive version is a dialect of `--to`, in every spelling.
///
/// `rhino:archive-60` is the registry id, `3dm:archive-60` reaches the same row
/// through the format alias. `60` is the catalog alias, usable bare because
/// the output path names the format or after either format spelling.
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
    for (index, spelling) in [
        "rhino:archive-60",
        "3dm:archive-60",
        "rhino:60",
        "3dm:60",
        "60",
    ]
    .into_iter()
    .enumerate()
    {
        let output = dir.path().join(format!("point-{index}.3dm"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--to",
                spelling,
                "--allow-errors",
            ])
            .assert()
            .success();
        assert_eq!(
            &fs::read(output).unwrap()[24..32],
            b"      60",
            "{spelling}"
        );
    }
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
        .stderr(predicate::str::contains(
            "cannot infer format from the output path; pass --to FORMAT",
        ));
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
fn existing_output_is_refused_before_decode() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("malformed.igs");
    let output = dir.path().join("existing.step");
    fs::write(&input, b"not an IGES file").unwrap();
    fs::write(&output, b"keep").unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--input-format",
            "iges",
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("pass --force to overwrite")
                .and(predicate::str::contains("decode failed").not()),
        );
    assert_eq!(fs::read(output).unwrap(), b"keep");
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
            "--to",
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
            "--to",
            "5.3",
            "--allow-errors",
        ])
        .assert()
        .success();
    assert_ne!(fs::read(&upgraded).unwrap(), original_bytes);
}

/// A `--to` that names only a format states no dialect, so a same-format
/// conversion still inherits.
///
/// `--to` is the same flag as `-f`/`--format`, and `-f iges` has always said
/// which kind of file to write, not which dialect of it. Reading a bare format
/// as its catalog default would make `convert old.igs -o new.igs -f iges`
/// silently rewrite the version while the identical command without `-f`
/// preserved it — the defect the identity default exists to close, back
/// through the spelling most users reach for. Naming the dialect is still the
/// escape.
#[test]
#[cfg(feature = "iges")]
fn a_to_that_names_only_the_format_still_inherits() {
    let dir = tempdir().unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.points.push(cadmpeg_ir::topology::Point {
        id: cadmpeg_ir::ids::PointId("cadir:model:point#bare".into()),
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
            "--to",
            "iges:5.1-fixed-ascii",
            "--allow-errors",
        ])
        .assert()
        .success();
    let original_bytes = fs::read(&original).unwrap();

    for spelling in ["iges", "igs"] {
        let inherited = dir.path().join(format!("inherited-{spelling}.igs"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                original.to_str().unwrap(),
                "-o",
                inherited.to_str().unwrap(),
                "--to",
                spelling,
                "--allow-errors",
            ])
            .assert()
            .success();
        assert_eq!(fs::read(&inherited).unwrap(), original_bytes, "{spelling}");
    }
}
