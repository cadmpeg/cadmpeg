// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Cursor;

use assert_cmd::Command;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
use cadmpeg_ir::CadIr;
use predicates::prelude::{predicate, PredicateBooleanExt};
use tempfile::tempdir;

use crate::support::fixture;

#[test]
fn omitted_standalone_mesh_reports_empty_but_supported_mesh_writes_geometry() {
    let mut ir = CadIr::empty();
    let vertices = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    ir.model.tessellations.push(
        Tessellation::new(
            TessellationId::mint("test:geometry:tessellation#0").unwrap(),
            TessellationMesh::List {
                vertices: vertices.clone(),
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap(),
    );
    assert!(ir.model.bodies.is_empty());
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "triangle.cadir.json", &ir);
    let output = dir.path().join("empty.step");
    let report = dir.path().join("empty.report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "step:ap203-e1",
            "-o",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(value["status"], "ok");
    let empty = value["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|loss| {
            loss["code"]["namespace"] == "export" && loss["code"]["code"] == "output.empty"
        })
        .collect::<Vec<_>>();
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0]["severity"], "warning");
    let decoded = cadmpeg_codec_step::StepCodec::default()
        .decode(
            &mut Cursor::new(fs::read(&output).unwrap()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.points.is_empty());
    assert!(decoded.ir().model.tessellations.is_empty());
    for (suffix, policy) in [
        ("empty", "--reject-empty"),
        ("loss", "--reject-lossy=export"),
    ] {
        let refused = dir.path().join(format!("refused-{suffix}.step"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "--to",
                "step:ap203-e1",
                "-o",
                refused.to_str().unwrap(),
                policy,
            ])
            .assert()
            .code(1);
        assert!(!refused.exists());
    }
    let supported = dir.path().join("triangle.step");
    let supported_report = dir.path().join("triangle.report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "step:ap242-e3",
            "--reject-empty",
            "-o",
            supported.to_str().unwrap(),
            "--report",
            supported_report.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("refusing to write an empty").not());
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(supported_report).unwrap()).unwrap();
    assert!(!value["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|loss| {
            loss["code"]["namespace"] == "export" && loss["code"]["code"] == "output.empty"
        }));
    let decoded = cadmpeg_codec_step::StepCodec::default()
        .decode(
            &mut Cursor::new(fs::read(supported).unwrap()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().model.tessellations.len(), 1);
    assert_eq!(decoded.ir().model.tessellations[0].vertices(), vertices);
    assert_eq!(decoded.ir().model.tessellations[0].triangles(), [[0, 1, 2]]);
}
