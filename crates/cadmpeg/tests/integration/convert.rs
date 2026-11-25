// SPDX-License-Identifier: Apache-2.0
//! Convert: destination rules, format selection, and write-back fidelity.

#[cfg(any(
    feature = "creo",
    feature = "fcstd",
    feature = "iges",
    feature = "rhino",
    feature = "sldprt",
    feature = "step"
))]
use std::fs;
#[cfg(feature = "rhino")]
use std::io::Cursor;

use assert_cmd::Command;
#[cfg(feature = "rhino")]
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use predicates::prelude::predicate;
use tempfile::tempdir;

use crate::support::fixture;
#[cfg(feature = "creo")]
use crate::support::geometryless_creo;
#[cfg(feature = "sldprt")]
use crate::support::sldprt_cube;

#[cfg(all(feature = "catia", feature = "step"))]
#[test]
fn convert_native_owned_sheet_preserves_shared_edge_in_step() {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::topology::Sense;
    use cadmpeg_test_support::catia::joined_planar_sheet;

    let dir = tempdir().unwrap();
    let input = dir.path().join("joined.CATPart");
    let output = dir.path().join("joined.step");
    fs::write(&input, joined_planar_sheet(0x33, 0x01)).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .arg("convert")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .assert()
        .success();

    let step = cadmpeg_codec_step::StepCodec::default()
        .decode(
            &mut Cursor::new(fs::read(output).unwrap()),
            &DecodeOptions::default(),
        )
        .expect("read the exported STEP sheet");
    let geometry = &step.ir().geometry;
    assert_eq!(geometry.faces.len(), 2);
    assert_eq!(geometry.edges.len(), 7);
    assert!(geometry
        .faces
        .iter()
        .all(|face| face.sense == Sense::Forward));
    let shared: Vec<_> = geometry
        .coedges
        .iter()
        .filter(|side| side.radial_next != side.id)
        .collect();
    let [first, second] = shared.as_slice() else {
        panic!("one edge shared by two coedges");
    };
    assert_eq!(first.edge, second.edge);
    assert_eq!(first.radial_next, second.id);
    assert_eq!(second.radial_next, first.id);
    assert_ne!(first.sense, second.sense);
}

#[cfg(feature = "fcstd")]
#[test]
fn validation_budgets_follow_loaded_native_and_cadir_input_sizes() {
    use std::fmt::Write as _;
    use std::io::{Read, Write};
    let dir = tempdir().unwrap();
    let input = dir.path().join("naming-metadata.FCStd");
    let mut source = zip::ZipArchive::new(std::io::Cursor::new(include_bytes!(
        "../../../../corpus/freecad_fcstd/fixtures/geometry_topology.FCStd"
    )))
    .unwrap();
    let mut writer = zip::ZipWriter::new(fs::File::create(&input).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name().expect("entry name") == "Document.xml" {
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text.contains("<Properties Count=\"17\""));
            let mut extra = String::new();
            for number in 0..1000 {
                writeln!(
                    extra,
                    "<Property name=\"Stress{number}\" type=\"App::PropertyString\"><StringHasher count=\"0\"/></Property>"
                )
                .expect("writing metadata into a String succeeds");
            }
            bytes = text
                .replacen("<Properties Count=\"17\"", "<Properties Count=\"1017\"", 1)
                .replacen("</Properties>", &format!("{extra}</Properties>"), 1)
                .into_bytes();
        }
        writer
            .start_file(entry.name().expect("entry name"), options)
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap();

    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap()])
        .assert()
        .success();
    let output = dir.path().join("naming-metadata.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "cadir",
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", output.to_str().unwrap()])
        .assert()
        .success();
    fs::remove_file(cadmpeg_ir::decode_sidecar_path(&output).unwrap()).unwrap();
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", output.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn convert_stdout_contains_only_json_artifact() {
    let dir = tempdir().unwrap();
    let input = fixture(
        dir.path(),
        "cube.cadir.json",
        &cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted"),
    );
    let output = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", input.to_str().unwrap(), "-f", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("check: OK"));
}

#[cfg(all(feature = "f3d", feature = "sldprt", feature = "step"))]
#[test]
fn curveless_faces_keep_their_source_vertices_as_points() {
    let dir = tempdir().unwrap();
    let cases: [(&str, &[u8], f64); 2] = [
        (
            "topology.f3d",
            include_bytes!("../../../cadmpeg-codec-f3d/tests/golden/fixtures/topology_base.f3d"),
            10.0,
        ),
        (
            "planar.sldprt",
            include_bytes!("../../../cadmpeg-codec-sldprt/tests/golden/fixtures/authored_complete_planar.sldprt"),
            1000.0,
        ),
    ];
    for (name, bytes, extent) in cases {
        let input = dir.path().join(name);
        let output = dir.path().join(format!("{name}.step"));
        let report = dir.path().join(format!("{name}.report.json"));
        let decoded = dir.path().join(format!("{name}.cadir.json"));
        fs::write(&input, bytes).unwrap();
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
            .success();
        let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(report["status"], "ok");
        assert!(report["export"]["losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["code"]["value"]["code"] == "topology.region-vertices-recovered"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "dump",
                output.to_str().unwrap(),
                "-o",
                decoded.to_str().unwrap(),
            ])
            .assert()
            .success();
        let decoded: serde_json::Value =
            serde_json::from_slice(&fs::read(decoded).unwrap()).unwrap();
        let points = decoded["geometry"]["points"].as_array().unwrap();
        assert_eq!(points.len(), 3, "{name}");
        assert!(points.iter().any(|point| point["position"]["x"] == extent));
        assert!(points.iter().any(|point| point["position"]["y"] == extent));
        assert!(decoded["geometry"]["faces"].as_array().unwrap().is_empty());
    }
}

#[cfg(all(feature = "f3d", feature = "step"))]
#[test]
fn pcurves_recover_edges_even_when_the_face_cannot_be_written() {
    let dir = tempdir().unwrap();
    for (name, bytes) in [
        (
            "inline.f3d",
            include_bytes!("../../../cadmpeg-codec-f3d/tests/golden/fixtures/pcurve_inline.f3d")
                .as_slice(),
        ),
        (
            "rational.f3d",
            include_bytes!("../../../cadmpeg-codec-f3d/tests/golden/fixtures/pcurve_rational.f3d")
                .as_slice(),
        ),
    ] {
        let input = dir.path().join(name);
        let output = dir.path().join(format!("{name}.step"));
        let report = dir.path().join(format!("{name}.json"));
        fs::write(&input, bytes).unwrap();
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
            .success();
        let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        let losses = report["export"]["losses"].as_array().unwrap();
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss["code"]["value"]["code"] == "topology.edge-no-3d-curve")
                .count(),
            2
        );
        assert!(losses
            .iter()
            .any(|loss| loss["code"]["value"]["code"] == "topology.region-edges-recovered"));
        let text = fs::read_to_string(output).unwrap();
        assert!(text.contains("EDGE_BASED_WIREFRAME_MODEL("));
        assert!(text.contains("EDGE_CURVE("));
    }
}

/// Sphere poles, a cone apex, and Rhino singular trims are edges with no 3D
/// curve whose pcurves stay at their one vertex. STEP writes every face of
/// these sources without those edges and reports no loss for them.
#[cfg(all(feature = "fcstd", feature = "rhino"))]
#[test]
fn step_conversion_leaves_collapsed_curveless_edges_out_without_a_loss() {
    let dir = tempdir().unwrap();
    let sources: [(&str, &[u8]); 3] = [
        (
            "geometry_topology.FCStd",
            include_bytes!("../../../../corpus/freecad_fcstd/fixtures/geometry_topology.FCStd"),
        ),
        (
            "binary_exact_shape.FCStd",
            include_bytes!("../../../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"),
        ),
        (
            "brep_singular_seam.3dm",
            include_bytes!(
                "../../../cadmpeg-codec-rhino/tests/golden/fixtures/brep_singular_seam.3dm"
            ),
        ),
    ];
    for (name, bytes) in sources {
        let input = dir.path().join(name);
        fs::write(&input, bytes).unwrap();
        let output = dir.path().join(format!("{name}.step"));
        let report = dir.path().join(format!("{name}.json"));
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
            .success();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        let codes: Vec<&str> = report["export"]["losses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|loss| loss["code"]["value"]["code"].as_str().unwrap())
            .collect();
        for code in [
            "topology.edge-no-3d-curve",
            "geometry.curveless-edge-omitted",
        ] {
            assert!(!codes.contains(&code), "{name}: {codes:?}");
        }
        let faces = report["check_report"]["entity_counts"]["faces"]
            .as_u64()
            .unwrap();
        assert!(faces > 0, "{name}");
        let text = fs::read_to_string(&output).unwrap();
        assert_eq!(
            u64::try_from(text.matches("ADVANCED_FACE(").count()).unwrap(),
            faces,
            "{name}"
        );
    }
}

/// The decode report line names the depth the codec delivered. A codec with
/// no geometry-depth cut delivers `full` for STEP's geometry request, with or
/// without `--full-decode`, which excludes `--container-only`.
#[cfg(all(feature = "creo", feature = "step"))]
#[test]
fn convert_reports_the_delivered_depth() {
    let dir = tempdir().unwrap();
    let input = geometryless_creo(dir.path(), "empty.prt");
    for extra in [None, Some("--full-decode")] {
        let mut args = vec!["convert", input.to_str().unwrap(), "-f", "step"];
        args.extend(extra);
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(args)
            .assert()
            .success()
            .stderr(predicate::str::contains(
                "depth=full, geometry_transferred=false",
            ));
    }
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--full-decode",
            "--container-only",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

/// A `FreeCAD` part converted to STEP decodes at geometry depth by default and
/// writes the same bytes as a `--full-decode` conversion.
#[cfg(all(feature = "fcstd", feature = "step"))]
#[test]
fn a_geometry_depth_step_conversion_writes_the_full_decode_bytes() {
    assert_geometry_depth_step_writes_the_full_decode_bytes(
        "design_history.FCStd",
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/design_history.FCStd"),
    );
}

/// An F3D design converted to STEP decodes at geometry depth by default and
/// writes the same bytes as a `--full-decode` conversion.
#[cfg(all(feature = "f3d", feature = "step"))]
#[test]
fn an_f3d_geometry_depth_step_conversion_writes_the_full_decode_bytes() {
    assert_geometry_depth_step_writes_the_full_decode_bytes(
        "design_sketch_constraints.f3d",
        include_bytes!(
            "../../../cadmpeg-codec-f3d/tests/golden/fixtures/design_sketch_constraints.f3d"
        ),
    );
}

/// A loss policy judges the whole source, so every `--reject-lossy` scope
/// decodes at full depth. A geometry-depth decode of this part holds none of
/// its design history, so STEP export would report none of it lost.
#[cfg(all(feature = "fcstd", feature = "step"))]
#[test]
fn reject_lossy_judges_the_design_history_a_geometry_decode_skips() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("design_history.FCStd");
    fs::write(
        &input,
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/design_history.FCStd"),
    )
    .unwrap();
    let report = dir.path().join("report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "--reject-lossy=export",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("the write's strictness refuses"));
    let report: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(report["refusal"]["code"], "strict_export_rejected");
    assert_eq!(report["decode_report"]["transfer"]["kind"], "full");
    assert!(
        report["export"]["losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["message"]
                .as_str()
                .unwrap()
                .contains("parametric/design record(s) were not represented in STEP")),
        "{report:#}"
    );
}

#[cfg(all(feature = "step", any(feature = "fcstd", feature = "f3d")))]
fn assert_geometry_depth_step_writes_the_full_decode_bytes(name: &str, bytes: &[u8]) {
    let dir = tempdir().unwrap();
    let input = dir.path().join(name);
    fs::write(&input, bytes).unwrap();
    let mut written = Vec::new();
    for (extra, depth) in [(None, "geometry"), (Some("--full-decode"), "full")] {
        let output = dir.path().join(format!("{depth}.step"));
        let mut args = vec![
            "convert",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ];
        args.extend(extra);
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(args)
            .assert()
            .success()
            .stderr(predicate::str::contains(format!("depth={depth},")));
        written.push(fs::read(&output).unwrap());
    }
    assert!(
        written[0] == written[1],
        "the geometry-depth STEP differs from the full-decode STEP"
    );
}

/// `--to` names the schema, in both spellings the grammar admits.
///
/// `step:ap242-e3` is the registry id; `ap242e3` is the catalog alias. The
/// alias is usable bare when the output extension names the format and after a
/// format prefix. Every spelling must reach the same `FILE_SCHEMA`.
#[cfg(feature = "step")]
#[test]
fn step_artifact_starts_with_step_header() {
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "cube.json", &{
        let mut cube = unit_cube().expect("unit cube fixture is admitted");
        cadmpeg_ir::examples::add_document_root(&mut cube);
        cube
    });

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
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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

#[cfg(feature = "sldprt")]
#[test]
fn source_less_sldprt_writes_a_synthesized_native_envelope() {
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
    let written = fs::read(&output).unwrap();
    assert_ne!(&written[..4], &[0; 4], "file_id is nonzero");
    assert_eq!(&written[4..8], &[0, 0, 0, 4]);
}

#[cfg(feature = "rhino")]
#[test]
fn source_less_ir_exports_to_decodable_rhino() {
    let dir = tempdir().unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.geometry.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#cli").expect("identity grammar"),
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            1.0, 2.0, 3.0,
        ))
        .expect("a finite position is a point"),
        None,
    ));
    let input = fixture(dir.path(), "point.cadir.json", &ir);
    let output = dir.path().join("point.3dm");
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

    let decoded = cadmpeg_codec_rhino::RhinoCodec
        .decode(
            &mut Cursor::new(fs::read(output).unwrap()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().geometry.points.len(), 1);
    assert_eq!(
        decoded.ir().geometry.points[0].position().get(),
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    );
}

/// The Rhino archive version is a dialect of `--to`, in every spelling.
///
/// `rhino:archive-60` is the registry id, `3dm:archive-60` reaches the same row
/// through the format alias. `60` is the catalog alias, usable bare because
/// the output path names the format or after either format spelling.
#[cfg(feature = "rhino")]
#[test]
fn rhino_output_version_is_selected_explicitly() {
    let dir = tempdir().unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.geometry.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#version").expect("identity grammar"),
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            1.0, 2.0, 3.0,
        ))
        .expect("a finite position is a point"),
        None,
    ));
    let input = fixture(dir.path(), "point.cadir.json", &ir);
    for (index, spelling) in [
        "rhino:archive-60",
        "3dm:archive-60",
        "rhino:60",
        "3dm:60",
        "60",
        "50",
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
            ])
            .assert()
            .success();
        assert_eq!(
            &fs::read(output).unwrap()[24..32],
            if spelling == "50" {
                b"      50"
            } else {
                b"      60"
            },
            "{spelling}"
        );
    }
}

#[cfg(feature = "step")]
#[test]
fn convert_writes_and_reports_check_errors_unless_rejected() {
    use predicates::prelude::PredicateBooleanExt;
    let dir = tempdir().unwrap();
    let mut invalid = unit_cube().expect("unit cube fixture is admitted");
    invalid.geometry.faces[0].surface =
        cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#missing")
            .expect("valid identity for an absent surface");
    let face = invalid.geometry.faces[0].id.to_string();
    let input = fixture(dir.path(), "invalid.json", &invalid);
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["check", input.to_str().unwrap()])
        .assert()
        .code(1);

    let written = dir.path().join("written.step");
    let written_report = dir.path().join("written-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "-o",
            written.to_str().unwrap(),
            "--report",
            written_report.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(
            predicate::str::contains("check: FAILED")
                .and(predicate::str::contains("output writes")),
        );
    assert!(written.exists());
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&written_report).unwrap()).unwrap();
    assert!(value["refusal"].is_null(), "{value:#}");
    assert!(
        value["check_report"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["severity"] == "error" && finding["entity"] == face.as_str()),
        "{value:#}"
    );

    let refused = dir.path().join("refused.step");
    let refused_report = dir.path().join("refused-report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "-f",
            "step",
            "-o",
            refused.to_str().unwrap(),
            "--report",
            refused_report.to_str().unwrap(),
            "--reject-errors",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "the write's strictness refuses export:record.invalid",
        ));
    assert!(!refused.exists());
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&refused_report).unwrap()).unwrap();
    assert_eq!(value["refusal"]["code"], "strict_export_rejected");
    assert_eq!(value["refusal"]["stage"], "export");
}

#[cfg(feature = "step")]
#[test]
fn output_cannot_replace_input_and_success_is_atomic() {
    let dir = tempdir().unwrap();
    let input = fixture(
        dir.path(),
        "cube.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
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
    let input = fixture(
        dir.path(),
        "cube.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["convert", input.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "cannot infer format from the output path; pass --to FORMAT",
        ));
}

#[cfg(feature = "step")]
#[test]
fn existing_output_requires_force() {
    let dir = tempdir().unwrap();
    let input = fixture(
        dir.path(),
        "cube.cadir.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
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

#[cfg(all(feature = "iges", feature = "step"))]
#[test]
fn existing_output_is_refused_before_decode() {
    use predicates::prelude::PredicateBooleanExt;
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

#[cfg(feature = "step")]
#[test]
fn input_named_tmp_survives_convert_and_temp_names_do_not_collide() {
    let dir = tempdir().unwrap();
    let input = fixture(
        dir.path(),
        "part.tmp",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
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

#[cfg(feature = "creo")]
#[test]
fn cadir_extension_is_inferred_and_decode_output_matches_stdout() {
    let dir = tempdir().unwrap();
    let cube = fixture(
        dir.path(),
        "cube.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
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
    let sidecar_path = cadmpeg_ir::decode_sidecar_path(&output).unwrap();
    let sidecar = cadmpeg_ir::DecodeSidecar::from_json(
        &fs::read_to_string(&sidecar_path).expect("decode writes fidelity sidecar"),
    )
    .unwrap();
    assert!(sidecar.matches(&fs::read(&output).unwrap()));

    let neutral_sidecar = cadmpeg_ir::decode_sidecar_path(&inferred).unwrap();
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

#[cfg(all(feature = "f3d", feature = "sldprt"))]
#[test]
fn a_write_to_another_format_names_the_solidworks_application_element() {
    let keywords: &[u8] = br#"<Keywords Name="Part"><Application Name="Simulation"><Keyword Name="Analysis type">Static</Keyword></Application><Feature id="1" Name="Sketch1" Type="ProfileFeature"/></Keywords>"#;
    let length = u16::try_from(keywords.len()).unwrap();
    let mut stored = vec![1];
    stored.extend(length.to_le_bytes());
    stored.extend((!length).to_le_bytes());
    stored.extend(keywords);
    let name = b"Contents/Keywords"
        .iter()
        .map(|byte| byte.rotate_left(4))
        .collect::<Vec<_>>();
    let mut source = 1_u32.to_le_bytes().to_vec();
    source.extend(4_u32.to_be_bytes());
    source.extend([0x14, 0x00, 0x06, 0x00, 0x08, 0x00]);
    for word in [
        0x42,
        crc32fast::hash(keywords),
        u32::try_from(stored.len()).unwrap(),
        u32::from(length),
        u32::try_from(name.len()).unwrap(),
    ] {
        source.extend(word.to_le_bytes());
    }
    source.extend(name);
    source.extend(stored);

    let dir = tempdir().unwrap();
    let input = dir.path().join("history.sldprt");
    let output = dir.path().join("history.f3d");
    let report = dir.path().join("history.report.json");
    fs::write(&input, source).unwrap();
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
        .success();
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    let named = report["decode_report"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|loss| loss["code"]["value"]["code"] == "history.application-not-transferred")
        .map(|loss| loss["message"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(named.len(), 1, "{report}");
    assert!(
        named[0].contains(r#"Application element "Simulation""#)
            && named[0].contains("a write to another format omits it"),
        "{}",
        named[0]
    );
}

#[cfg(all(feature = "f3d", feature = "sldprt"))]
#[test]
fn fidelity_sidecar_replays_native_bytes_and_missing_sidecar_refuses_prewrite() {
    use predicates::prelude::PredicateBooleanExt;
    let dir = tempdir().unwrap();
    for format in ["f3d", "sldprt"] {
        let native = dir.path().join(format!("source.{format}"));
        if format == "sldprt" {
            fs::write(
                &native,
                include_bytes!("../../../cadmpeg-codec-sldprt/tests/golden/fixtures/authored_complete_planar.sldprt"),
            ).unwrap();
        } else {
            let cube = fixture(
                dir.path(),
                &format!("cube-{format}.cadir.json"),
                &unit_cube().expect("unit cube fixture is admitted"),
            );
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
        }

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
        let sidecar = cadmpeg_ir::decode_sidecar_path(&persisted).unwrap();
        assert!(sidecar.exists());
        // The sidecar annotates no record of the arenas the document omits,
        // so the read document checks as the decode does.
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["check", persisted.to_str().unwrap()])
            .assert()
            .success();

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
        let refused_command = Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                persisted.to_str().unwrap(),
                "-o",
                refused.to_str().unwrap(),
                "--reject-lossy",
            ])
            .assert();
        // Without its retained part, a SolidWorks document writes a lossy
        // native skeleton, which `--reject-lossy` refuses before the write.
        refused_command.failure().stderr(
            predicate::str::contains("Preserved")
                .or(predicate::str::contains("source image is unavailable"))
                .or(predicate::str::contains("read from CADIR"))
                .or(predicate::str::contains(
                    "the write's strictness refuses sldprt/writer.native-skeleton-unlaid-runs",
                )),
        );
        assert!(!refused.exists());
    }
}

#[test]
fn cadir_format_name_and_json_alias_both_work() {
    let dir = tempdir().unwrap();
    let input = fixture(
        dir.path(),
        "cube.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
    for format in ["cadir", "json"] {
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args(["convert", input.to_str().unwrap(), "-f", format])
            .assert()
            .success()
            .stdout(predicate::str::starts_with("{"));
    }
}

#[cfg(feature = "step")]
#[test]
fn explicit_format_warns_when_known_extension_disagrees() {
    let dir = tempdir().unwrap();
    let input = fixture(
        dir.path(),
        "cube.json",
        &unit_cube().expect("unit cube fixture is admitted"),
    );
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

#[cfg(all(feature = "rhino", feature = "sldprt", feature = "step"))]
#[test]
fn convert_refuses_binary_output_to_stdout() {
    use predicates::prelude::PredicateBooleanExt;
    let dir = tempdir().unwrap();
    let ir = unit_cube().expect("unit cube fixture is admitted");
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
    let mut point_ir = cadmpeg_ir::CadIr::empty();
    point_ir
        .geometry
        .points
        .push(cadmpeg_ir::topology::Point::new(
            cadmpeg_ir::ids::PointId::mint("cadir:model:point#guard").expect("identity grammar"),
            cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                1.0, 2.0, 3.0,
            ))
            .expect("a finite position is a point"),
            None,
        ));
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

#[cfg(feature = "step")]
#[test]
fn from_and_to_aliases_match_input_format_and_format() {
    let dir = tempdir().unwrap();
    let ir = unit_cube().expect("unit cube fixture is admitted");
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
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.geometry.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#iges").expect("identity grammar"),
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            1.0, 2.0, 3.0,
        ))
        .expect("a finite position is a point"),
        None,
    ));
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
        ])
        .assert()
        .success();
    assert_ne!(fs::read(&upgraded).unwrap(), original_bytes);
}

#[test]
#[cfg(feature = "iges")]
fn a_same_format_synthesis_prints_why_the_patch_was_declined_once() {
    let dir = tempdir().unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.geometry.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#declined").expect("identity grammar"),
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            1.0, 2.0, 3.0,
        ))
        .expect("a finite position is a point"),
        None,
    ));
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
        ])
        .assert()
        .success();
    let upgraded = dir.path().join("upgraded.igs");
    let report = dir.path().join("upgraded.report.json");
    let stderr = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            original.to_str().unwrap(),
            "-o",
            upgraded.to_str().unwrap(),
            "--to",
            "5.3",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(stderr).unwrap();
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    let reason = report["export"]["synthesis_cause"]["reason"]
        .as_str()
        .unwrap_or_else(|| panic!("a patch-declined reason: {report}"));
    assert_eq!(stderr.matches(reason).count(), 1, "{stderr}");
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
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.geometry.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#bare").expect("identity grammar"),
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            1.0, 2.0, 3.0,
        ))
        .expect("a finite position is a point"),
        None,
    ));
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
            ])
            .assert()
            .success();
        assert_eq!(fs::read(&inherited).unwrap(), original_bytes, "{spelling}");
    }
}

/// A CADIR read that gives up a record keeps the record's text: `convert`
/// and `dump` to CADIR write it to the sidecar beside the output, and a
/// read of that output with its sidecar keeps it again (§3.3).
#[test]
fn a_cadir_read_keeps_the_text_of_a_given_up_record_in_the_written_sidecar() {
    let dir = tempdir().unwrap();
    let mut value =
        serde_json::to_value(unit_cube().expect("unit cube fixture is admitted")).unwrap();
    value["geometry"]["points"][0]["position"] = serde_json::json!("not a point");
    let text =
        cadmpeg_ir::hash::finite_json::to_canonical_json_string(&value["geometry"]["points"][0])
            .unwrap();
    let input = dir.path().join("given-up.cadir.json");
    std::fs::write(&input, value.to_string()).unwrap();
    let retained = |output: &std::path::Path| -> Vec<Vec<u8>> {
        let sidecar = cadmpeg_ir::DecodeSidecar::from_json(
            &std::fs::read_to_string(cadmpeg_ir::decode_sidecar_path(output).unwrap())
                .expect("the write keeps a sidecar"),
        )
        .unwrap();
        assert!(sidecar.matches(&std::fs::read(output).unwrap()));
        sidecar
            .fidelity
            .retained_records()
            .values()
            .filter_map(|record| record.data().map(<[u8]>::to_vec))
            .collect()
    };
    let write = |command: &str, from: &std::path::Path, name: &str| {
        let output = dir.path().join(name);
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                command,
                from.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
            ])
            .assert()
            .success();
        output
    };
    let converted = write("convert", &input, "converted.cadir.json");
    assert_eq!(retained(&converted), [text.as_bytes()]);
    let dumped = write("dump", &input, "dumped.cadir.json");
    assert_eq!(retained(&dumped), [text.as_bytes()]);
    let again = write("convert", &converted, "again.cadir.json");
    assert_eq!(retained(&again), [text.as_bytes()]);
}
