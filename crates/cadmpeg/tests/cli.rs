// SPDX-License-Identifier: Apache-2.0
//! CLI integration tests.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Cursor, Write};

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

fn minimal_fcstd(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let file = fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file(
        "Document.xml",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(
        b"<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\"><Object/></Document>",
    )
    .unwrap();
    zip.finish().unwrap();
    path
}

#[test]
fn fcstd_inspect_and_container_decode_work_automatically_and_forced() {
    let dir = tempdir().unwrap();
    let input = minimal_fcstd(dir.path(), "document.FCStd");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("format: fcstd (detected high)")
                .and(predicate::str::contains("SchemaVersion=4")),
        );

    for forced in [false, true] {
        let mut command = Command::cargo_bin("cadmpeg").unwrap();
        command.args(["dump", input.to_str().unwrap(), "--container-only"]);
        if forced {
            command.args(["--input-format", "fcstd"]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["source"]["format"], "fcstd");
        assert_eq!(value["source"]["attributes"]["schema_version"], "4");
    }
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

fn rhino_header(version: &str) -> Vec<u8> {
    let mut bytes = b"3D Geometry File Format ".to_vec();
    let mut version_field = [b' '; 8];
    let start = version_field.len() - version.len();
    version_field[start..].copy_from_slice(version.as_bytes());
    bytes.extend(version_field);
    assert_eq!(bytes.len(), 32);
    bytes
}

fn rhino_long_chunk(version: u64, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    if version >= 50 {
        bytes.extend((body.len() as i64).to_le_bytes());
    } else {
        bytes.extend((body.len() as i32).to_le_bytes());
    }
    bytes.extend(body);
    bytes
}

fn rhino_short_chunk(version: u64, typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    if version >= 50 {
        bytes.extend(value.to_le_bytes());
    } else {
        bytes.extend((value as i32).to_le_bytes());
    }
    bytes
}

fn rhino_crc_chunk(version: u64, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(body).to_le_bytes());
    rhino_long_chunk(version, typecode, &payload)
}

fn rhino_table(version: u64, typecode: u32) -> Vec<u8> {
    let end = rhino_short_chunk(version, 0xffff_ffff, 0);
    rhino_long_chunk(version, typecode, &end)
}

fn rhino_object_record(version: u64, class_uuid: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let object_type = rhino_short_chunk(version, 0x8200_0071, 1);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = rhino_long_chunk(version, 0x0002_fffb, &uuid_body);
    let class_data = rhino_crc_chunk(version, 0x0002_fffc, payload);
    let class_end = rhino_short_chunk(version, 0x8002_7fff, 0);
    let class = rhino_long_chunk(
        version,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    let object_end = rhino_short_chunk(version, 0x8200_007f, 0);
    rhino_crc_chunk(
        version,
        0x2000_8070,
        &[object_type, class, object_end].concat(),
    )
}

fn synthetic_rhino_point(dir: &std::path::Path, name: &str, point: [f64; 3]) -> std::path::PathBuf {
    let version = 50;
    let mut payload = vec![0x10];
    for coordinate in point {
        payload.extend(coordinate.to_le_bytes());
    }
    let point_class = [
        0x1d, 0x1a, 0x10, 0xc3, 0x57, 0xf1, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let object = rhino_object_record(version, point_class, &payload);
    let end = rhino_short_chunk(version, 0xffff_ffff, 0);
    let object_table = rhino_long_chunk(version, 0x1000_0013, &[object, end].concat());

    let mut units = 100_i32.to_le_bytes().to_vec();
    units.extend(2_i32.to_le_bytes());
    units.extend(0.01_f64.to_le_bytes());
    units.extend(0.1_f64.to_le_bytes());
    units.extend(0.001_f64.to_le_bytes());
    let units = rhino_crc_chunk(version, 0x2000_8031, &units);
    let settings_table = rhino_long_chunk(
        version,
        0x1000_0015,
        &[units, rhino_short_chunk(version, 0xffff_ffff, 0)].concat(),
    );

    let mut bytes = rhino_header("50");
    bytes.extend(rhino_long_chunk(version, 1, b"cadmpeg CLI geometry"));
    bytes.extend(rhino_table(version, 0x1000_0014));
    bytes.extend(settings_table);
    bytes.extend(object_table);
    let eof_offset = bytes.len();
    bytes.extend(rhino_long_chunk(version, 0x0000_7fff, &[0; 8]));
    let eof = rhino_long_chunk(version, 0x0000_7fff, &(bytes.len() as u64).to_le_bytes());
    bytes[eof_offset..].copy_from_slice(&eof);

    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn minimal_rhino_archive(
    dir: &std::path::Path,
    name: &str,
    version_text: &str,
) -> std::path::PathBuf {
    minimal_rhino_archive_with_comment(dir, name, version_text, b"cadmpeg test")
}

fn minimal_rhino_archive_with_comment(
    dir: &std::path::Path,
    name: &str,
    version_text: &str,
    comment: &[u8],
) -> std::path::PathBuf {
    let version = version_text.parse::<u64>().unwrap();
    let mut bytes = rhino_header(version_text);
    bytes.extend(rhino_long_chunk(version, 0x0000_0001, comment));
    bytes.extend(rhino_table(version, 0x1000_0014));
    bytes.extend(rhino_table(version, 0x1000_0015));
    bytes.extend(rhino_table(version, 0x1000_0013));

    let eof_offset = bytes.len();
    let width = if version >= 50 { 8 } else { 4 };
    bytes.extend(rhino_long_chunk(version, 0x0000_7fff, &vec![0; width]));
    let file_size = bytes.len();
    let eof_body = if version >= 50 {
        (file_size as u64).to_le_bytes().to_vec()
    } else {
        (file_size as u32).to_le_bytes().to_vec()
    };
    let eof = rhino_long_chunk(version, 0x0000_7fff, &eof_body);
    bytes[eof_offset..].copy_from_slice(&eof);

    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
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

/// A cube carrying source metadata with the given attributes.
fn cube_with_source(attributes: &[(&str, &str)]) -> cadmpeg_ir::CadIr {
    let mut ir = unit_cube();
    ir.source = Some(cadmpeg_ir::SourceMeta {
        format: "synthetic".into(),
        attributes: attributes
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    });
    ir
}

#[test]
fn diff_reports_a_source_attribute_change_and_exits_one() {
    let dir = tempdir().unwrap();
    let a = fixture(
        dir.path(),
        "a.json",
        &cube_with_source(&[("program_version", "1.0"), ("object_count", "3")]),
    );
    let b = fixture(
        dir.path(),
        "b.json",
        &cube_with_source(&[("program_version", "1.1"), ("object_count", "3")]),
    );

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "source program_version: 1.0 → 1.1",
        ))
        .stderr(predicate::str::is_empty());
}

/// A machine-local digest cannot agree across platforms, so a difference in one
/// alone is reported without making the documents different.
#[test]
fn diff_reports_a_machine_local_digest_difference_without_a_verdict() {
    let dir = tempdir().unwrap();
    let a = fixture(
        dir.path(),
        "a.json",
        &cube_with_source(&[("document_local_sha256", &"0".repeat(64))]),
    );
    let b = fixture(
        dir.path(),
        "b.json",
        &cube_with_source(&[("document_local_sha256", &"1".repeat(64))]),
    );

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("machine-local digests")
                .and(predicate::str::contains("document_local_sha256"))
                .and(predicate::str::contains("identical")),
        );

    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", "--json", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success();
    let value: serde_json::Value =
        serde_json::from_slice(&output.get_output().stdout).expect("diff --json emits JSON");
    assert_eq!(value["different"], serde_json::json!(false));
    assert_eq!(
        value["diff"]["source"]["local_digests"][0]["key"],
        serde_json::json!("document_local_sha256")
    );
    assert!(value["diff"]["source"]["attributes"]
        .as_array()
        .expect("attributes is an array")
        .is_empty());
}

/// A document with no source metadata against one that has some must not panic.
#[test]
fn diff_handles_absent_source_metadata() {
    let dir = tempdir().unwrap();
    let mut bare = unit_cube();
    bare.source = None;
    let a = fixture(dir.path(), "a.json", &bare);
    let b = fixture(
        dir.path(),
        "b.json",
        &cube_with_source(&[("object_count", "3")]),
    );

    for (left, right) in [(&a, &b), (&b, &a)] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["diff", left.to_str().unwrap(), right.to_str().unwrap()])
            .assert()
            .code(1)
            .stdout(predicate::str::contains("source format:"))
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn diff_summarizes_the_source_fidelity_sidecar_for_native_inputs() {
    let dir = tempdir().unwrap();
    let a = minimal_rhino_archive(dir.path(), "a.3dm", "50");
    let b = minimal_rhino_archive(dir.path(), "b.3dm", "50");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("source fidelity: identical"));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", "--json", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"source_fidelity\"")
                .and(predicate::str::contains("\"present\": \"both\"")),
        );
}

#[test]
fn garbage_reports_supported_formats() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("garbage.bin");
    fs::write(&input, b"not a CAD file").unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP",
        ));
}

#[test]
fn rhino_inspect_detects_archive_and_reports_tables_in_text_and_json() {
    let dir = tempdir().unwrap();
    let input = minimal_rhino_archive(dir.path(), "empty.3dm", "50");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("format: rhino (detected high)")
                .and(predicate::str::contains("container: 3dm-chunks"))
                .and(predicate::str::contains("entries: 3"))
                .and(predicate::str::contains("table-0x10000014"))
                .and(predicate::str::contains("table-0x10000015"))
                .and(predicate::str::contains("table-0x10000013"))
                .and(predicate::str::contains("archive version 50")),
        );

    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 6);
    assert_eq!(value["command"], "inspect");
    assert_eq!(value["confidence"], "high");
    assert_eq!(value["summary"]["format"], "rhino");
    assert_eq!(value["summary"]["container_kind"], "3dm-chunks");
    assert_eq!(value["summary"]["entries"].as_array().unwrap().len(), 3);
    assert_eq!(value["summary"]["notes"][0], "archive version 50");
}

#[test]
fn rhino_forced_input_format_and_3dm_alias_bypass_detection() {
    let dir = tempdir().unwrap();
    let input = minimal_rhino_archive(dir.path(), "extensionless", "50");

    for input_format in ["rhino", "3dm"] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "inspect",
                input.to_str().unwrap(),
                "--input-format",
                input_format,
            ])
            .assert()
            .success()
            .stdout(
                predicate::str::contains("format: rhino (forced)")
                    .and(predicate::str::contains("container: 3dm-chunks")),
            );

        let output = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "dump",
                input.to_str().unwrap(),
                "--input-format",
                input_format,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ir_version"], cadmpeg_ir::IR_VERSION);
        assert_eq!(value["source"]["format"], "rhino");
    }
}

#[test]
fn rhino_full_band_empty_archive_decodes_to_current_ir() {
    let dir = tempdir().unwrap();
    for version in ["50", "60", "70", "80"] {
        let input = minimal_rhino_archive(dir.path(), &format!("empty-{version}.3dm"), version);
        for extra in [None, Some("--container-only")] {
            let mut command = Command::cargo_bin("cadmpeg").unwrap();
            command.args(["dump", input.to_str().unwrap()]);
            if let Some(argument) = extra {
                command.arg(argument);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "archive {version}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["ir_version"], cadmpeg_ir::IR_VERSION);
            assert_eq!(value["source"]["format"], "rhino");
            assert_eq!(value["source"]["attributes"]["archive_version"], version);
            assert_eq!(
                value["source"]["attributes"]["container_kind"],
                "3dm-chunks"
            );
            assert_eq!(value["model"]["subds"], serde_json::json!([]));
        }
    }
}

#[test]
fn rhino_point_archive_inspect_decode_and_validate_expose_geometry() {
    let dir = tempdir().unwrap();
    let input = synthetic_rhino_point(dir.path(), "point.3dm", [1.25, -2.5, 3.75]);

    let inspect = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(
        summary["summary"]["entries"][2]["attributes"]["record_count"],
        "1"
    );

    let decoded = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(decoded.status.success());
    let ir: serde_json::Value = serde_json::from_slice(&decoded.stdout).unwrap();
    assert_eq!(ir["model"]["points"][0]["position"]["x"], 1.25);
    assert_eq!(ir["model"]["points"][0]["position"]["y"], -2.5);
    assert_eq!(ir["model"]["points"][0]["position"]["z"], 3.75);
    let body_id = ir["model"]["bodies"][0]["id"].as_str().unwrap();
    assert!(ir["native"]["rhino"]["arenas"]["unknowns"][0]["links"]
        .as_array()
        .unwrap()
        .iter()
        .any(|link| link == body_id));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("check: OK"));
}

#[test]
fn rhino_v1_to_v4_decode_metadata_but_legacy_v5_is_header_only() {
    let dir = tempdir().unwrap();
    for version in ["1", "2", "3", "4"] {
        let input = minimal_rhino_archive(dir.path(), &format!("empty-{version}.3dm"), version);
        let output = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["dump", input.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ir_version"], cadmpeg_ir::IR_VERSION);
        assert_eq!(value["source"]["attributes"]["archive_version"], version);
        assert_eq!(value["model"]["subds"], serde_json::json!([]));
    }

    let input = dir.path().join("header-5.3dm");
    fs::write(&input, rhino_header("5")).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("container: 3dm-chunks")
                .and(predicate::str::contains("entries: 0"))
                .and(predicate::str::contains("archive version 5")),
        );
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "Rhino archive version 5 decode is not implemented",
        ));
}

#[test]
fn rhino_cli_rejects_truncated_and_malformed_archives_with_context() {
    let dir = tempdir().unwrap();
    let truncated = dir.path().join("truncated.3dm");
    let mut bytes = rhino_header("50");
    bytes.extend([1, 0, 0]);
    fs::write(&truncated, bytes).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", truncated.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("inspecting").and(predicate::str::contains("truncated")));

    let malformed = minimal_rhino_archive(dir.path(), "malformed.3dm", "50");
    let mut bytes = fs::read(&malformed).unwrap();
    bytes.truncate(bytes.len() - 20);
    fs::write(&malformed, bytes).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", malformed.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("decoding")
                .and(predicate::str::contains("missing end-of-file chunk")),
        );
}

#[test]
fn inspect_garbage_reports_rhino_among_supported_formats() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("garbage.bin");
    fs::write(&input, b"not a CAD file").unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP",
        ));
}

#[test]
fn cadir_override_bypasses_native_detection() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "no-extension", &unit_cube());
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap(), "--input-format", "cadir"])
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
fn exit_codes_distinguish_semantic_and_operational_failures() {
    let dir = tempdir().unwrap();
    let garbage = dir.path().join("garbage");
    fs::write(&garbage, b"garbage").unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", garbage.to_str().unwrap()])
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
        .args(["check", invalid.to_str().unwrap()])
        .assert()
        .code(1);
}

#[test]
fn reject_lossy_refuses_lossy_export_as_a_model_refusal() {
    let dir = tempdir().unwrap();
    let lossy = geometryless_creo(dir.path(), "lossy.prt");

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            lossy.to_str().unwrap(),
            "-f",
            "step",
            "--allow-empty",
            "--reject-lossy",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("refusing to write a lossy"));

    let report = dir.path().join("lossy-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            lossy.to_str().unwrap(),
            "-f",
            "step",
            "--allow-empty",
            "--reject-lossy",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .code(1);
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert!(value["decode_report"].is_object());
    assert!(value["export"].is_null());

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            lossy.to_str().unwrap(),
            "-f",
            "step",
            "--allow-empty",
        ])
        .assert()
        .success();
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
fn convert_rejects_container_only_geometry_unless_allowed() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
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
            "convert",
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
    assert_eq!(value["schema_version"], 6);
    assert_eq!(value["command"], "convert");
    assert_eq!(value["status"], "ok");
    assert!(value["refusal"].is_null());
    assert!(value["decode_report"].is_null());
    assert!(value["check_report"].is_object());
    assert_eq!(value["export"]["format"], "step");
    assert_eq!(value["export"]["census"]["basis"], "target_records");
    assert!(value["export"]["census"]["counts"].is_object());
    assert_eq!(value["export"]["fidelity"]["status"], "not_provided");
    assert!(value["export"]["losses"].is_array());
    assert!(value["export"]["notes"].is_array());

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
    assert_eq!(value["schema_version"], 6);
    assert_eq!(value["status"], "refused");
    assert_eq!(value["refusal"]["stage"], "export");
    assert_eq!(value["refusal"]["code"], "empty_geometry");
    assert!(value["refusal"]["message"]
        .as_str()
        .unwrap()
        .contains("geometry"));
    assert!(value["decode_report"].is_object());
    assert!(value["check_report"].is_object());
    assert!(value["export"].is_null());
}

#[test]
fn f3d_export_report_identifies_regenerated_output() {
    let dir = tempdir().unwrap();
    let cube = fixture(dir.path(), "cube.json", &unit_cube());
    let output = dir.path().join("cube.f3d");
    let report = dir.path().join("f3d-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            cube.to_str().unwrap(),
            "-f",
            "f3d",
            "-o",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 6);
    assert_eq!(value["export"]["format"], "f3d");
    assert!(value["export"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note
            .as_str()
            .is_some_and(|note| note.contains("regenerated"))));
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
fn reporting_commands_emit_versioned_json_only_on_stdout() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &unit_cube());
    let validate = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&validate.stdout).unwrap();
    assert_eq!(value["schema_version"], 6);
    assert_eq!(value["command"], "check");

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
    assert_eq!(value["schema_version"], 6);
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
    assert_eq!(value["schema_version"], 6);
    assert_eq!(value["command"], "inspect");
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
fn inspect_report_writes_versioned_summary_to_file() {
    let dir = tempdir().unwrap();
    let input = minimal_rhino_archive(dir.path(), "empty.3dm", "50");
    let report = dir.path().join("inspect-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "inspect",
            input.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("format: rhino (detected high)"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 6);
    assert_eq!(value["command"], "inspect");
    assert_eq!(value["confidence"], "high");
    assert_eq!(value["summary"]["format"], "rhino");
}

#[test]
fn validate_report_writes_versioned_result_to_file() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let report = dir.path().join("validate-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("check: OK"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 6);
    assert_eq!(value["command"], "check");
    assert!(value["decode_report"].is_null());
    assert!(value["check_report"].is_object());
}

#[test]
fn reporting_commands_accept_o_for_the_report_and_force_to_replace_it() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let report = dir.path().join("report.json");

    for (command, path_flag) in [
        (vec!["check"], "-o"),
        (vec!["check"], "--output"),
        (vec!["diff"], "-o"),
    ] {
        fs::write(&report, b"keep").unwrap();
        let mut arguments = command.clone();
        arguments.push(input.to_str().unwrap());
        if command[0] == "diff" {
            arguments.push(input.to_str().unwrap());
        }
        arguments.extend([path_flag, report.to_str().unwrap()]);

        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(&arguments)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("pass --force to overwrite"));
        assert_eq!(fs::read(&report).unwrap(), b"keep");

        arguments.push("--force");
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(&arguments)
            .assert()
            .success();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        assert_eq!(value["command"], command[0]);
    }
}

#[test]
fn validate_agrees_between_its_exit_code_printed_summary_and_report() {
    // findings under `.check_report.findings` in the written report.
    let dir = tempdir().unwrap();
    let mut ir = unit_cube();
    let absent = format!("{}-absent", ir.model.faces[0].surface);
    ir.model.faces[0].surface = absent.into();
    let input = fixture(dir.path(), "broken.cadir.json", &ir);
    let report = dir.path().join("report.json");

    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "-o",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let printed = String::from_utf8(output.stdout).unwrap();
    let summary = printed
        .lines()
        .find(|line| line.starts_with("check: FAILED"))
        .unwrap_or_else(|| panic!("{printed}"));
    let printed_errors: usize = summary
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(" error(s)"))
        .map_or_else(|| panic!("{summary}"), |(count, _)| count.parse().unwrap());

    let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    let findings = value["check_report"]["findings"].as_array().unwrap();
    let reported_errors = findings
        .iter()
        .filter(|finding| finding["severity"] == "error" || finding["severity"] == "blocking")
        .count();
    assert!(printed_errors > 0, "{summary}");
    assert_eq!(printed_errors, reported_errors, "{findings:#?}");
}

#[test]
fn diff_report_writes_versioned_result_to_file() {
    let dir = tempdir().unwrap();
    let cube = unit_cube();
    let a = fixture(dir.path(), "a.cadir.json", &cube);
    let b = fixture(dir.path(), "b.cadir.json", &cube);
    let report = dir.path().join("diff-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("identical"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 6);
    assert_eq!(value["command"], "diff");
    assert_eq!(value["different"], false);
    assert!(value["diff"].is_object());
}

#[test]
fn diff_input_format_forces_the_reader_per_input() {
    let dir = tempdir().unwrap();
    let a = fixture(dir.path(), "a.cadir.json", &unit_cube());
    let b = fixture(dir.path(), "b.cadir.json", &unit_cube());

    // Two identical CADIR documents compare equal.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("identical"));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--input-format-a",
            "rhino",
        ])
        .assert()
        .code(2);

    // The second-input flag targets its input independently.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--input-format-b",
            "rhino",
        ])
        .assert()
        .code(2);
}

#[test]
fn report_to_an_unwritable_path_is_an_operational_error() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.cadir.json", &unit_cube());
    let report = dir.path().join("missing-subdir").join("report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "check",
            input.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .code(2);
    assert!(!report.exists());
}

#[test]
fn input_flag_reaches_every_single_input_command() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = fixture(dir.path(), "cube.cadir.json", &ir);
    let path = model.to_str().unwrap();

    // Byte-identical stdout under either input spelling.
    for args in [
        vec!["dump", "--input-format", "cadir"],
        vec!["check"],
        vec!["convert", "-f", "step"],
    ] {
        let positional = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(&args)
            .arg(path)
            .output()
            .unwrap();
        let mut flagged = args.clone();
        flagged.push("--input");
        flagged.push(path);
        let via_flag = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(&flagged)
            .output()
            .unwrap();
        assert_eq!(positional.status.code(), via_flag.status.code(), "{args:?}");
        assert_eq!(positional.stdout, via_flag.stdout, "{args:?}");
    }

    // Both spellings at once are a clap conflict.
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", path, "--input", path])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn json_on_artifact_commands_is_a_teaching_error() {
    let dir = tempdir().unwrap();
    let ir = unit_cube();
    let model = fixture(dir.path(), "cube.cadir.json", &ir);
    let path = model.to_str().unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", path, "--json"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("already JSON").and(predicate::str::contains("cadmpeg query")),
        );

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "--json"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("not an output selector")
                .and(predicate::str::contains("--report")),
        );
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

#[test]
fn wrong_target_flags_refuse_before_reading_input() {
    let dir = tempdir().unwrap();
    // Absent path: wrong-target must fail before open/read.
    let missing = dir.path().join("does-not-exist.cadir.json");
    let path = missing.to_str().unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "-f", "step", "--iges-target", "5.3"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--iges-target requires IGES output",
        ));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "-f", "cadir", "--step-target", "ap214"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--step-target/--reject-step-losses require STEP output",
        ));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "-f", "iges", "--reject-step-losses"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--step-target/--reject-step-losses require STEP output",
        ));

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", path, "-f", "step", "--rhino-target", "80"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--rhino-target requires Rhino output",
        ));
}

#[cfg(unix)]
#[test]
fn closed_stdout_pipe_exits_on_sigpipe_without_panic() {
    use std::io::Read;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command as ProcessCommand, Stdio};

    // Hex of 64 KiB exceeds the typical pipe buffer, so the writer is still
    // printing when the reader closes.
    let dir = tempdir().unwrap();
    let input = dir.path().join("zeros.bin");
    fs::write(&input, vec![0u8; 64 * 1024]).unwrap();

    let mut child = ProcessCommand::new(assert_cmd::cargo::cargo_bin("cadmpeg"))
        .args(["inspect", "hex", input.to_str().unwrap(), "--len", "65536"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0u8; 1];
    stdout.read_exact(&mut first).unwrap();
    drop(stdout);

    let mut stderr = child.stderr.take().unwrap();
    let status = child.wait().unwrap();
    let mut err = String::new();
    stderr.read_to_string(&mut err).unwrap();

    assert!(
        !err.contains("panicked"),
        "closed stdout pipe panicked:\n{err}"
    );
    assert_eq!(
        status.signal(),
        Some(13),
        "expected SIGPIPE, got {status:?}; stderr={err}"
    );
}
