// SPDX-License-Identifier: Apache-2.0
//! Rhino native metadata cannot block synthesis or override neutral edits.

use std::fs;

use assert_cmd::Command;
use cadmpeg_codec_rhino::RhinoCodec;
use cadmpeg_ir::codec::write::{ArenaDisposition, NativeDisposition, NativeDispositions};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::{decode_sidecar_path, CadIr, DecodeSidecar};
use predicates::prelude::predicate;
use tempfile::tempdir;

use super::{decode, workspace, write_decode};

const TARGET: &str = "rhino:archive-80";
const EPS_POINT: f64 = 1e-6;
const FIXTURES: [&str; 2] = ["extrusion_hole.3dm", "curves_mixed.3dm"];

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        workspace()
            .join("crates/cadmpeg-codec-rhino/tests/golden/fixtures")
            .join(name),
    )
    .unwrap()
}

fn counts(ir: &CadIr) -> [usize; 5] {
    [
        ir.model.bodies.len(),
        ir.model.faces.len(),
        ir.model.edges.len(),
        ir.model.curves.len(),
        ir.model.points.len(),
    ]
}

fn assert_source_settings_populated(ir: &CadIr) {
    assert!(
        ir.native
            .namespace("rhino")
            .and_then(|namespace| namespace.arenas().get("document_settings"))
            .is_some_and(|records| !records.is_empty()),
        "the test must exercise held native settings that the writer cannot reemit"
    );
}

#[test]
fn every_decodable_rhino_golden_with_native_records_can_be_synthesized() {
    let directory = workspace().join("crates/cadmpeg-codec-rhino/tests/golden/fixtures");
    let mut fixtures = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "3dm"))
        .collect::<Vec<_>>();
    fixtures.sort();
    let mut checked = 0;
    for path in fixtures {
        if path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .starts_with("reject_")
        {
            continue;
        }
        let result = decode(&RhinoCodec, &fs::read(&path).unwrap());
        let (written, report) = write_decode(
            &RhinoCodec,
            &RhinoCodec,
            result.ir(),
            Some(result.source_fidelity()),
            Some(TARGET),
        );
        assert!(
            matches!(report.write_path(), WritePath::Synthesized { .. }),
            "{}: {:?}",
            path.display(),
            report.write_path()
        );
        assert_eq!(
            written.ir().scope,
            written.report().scope(),
            "{}",
            path.display()
        );
        eprintln!(
            "{}: coverage={:?}; native={:?}; losses={:?}",
            path.file_name().unwrap().to_string_lossy(),
            report.coverage,
            report.native_dispositions,
            report
                .losses
                .iter()
                .map(|loss| loss.code.to_string())
                .collect::<Vec<_>>()
        );
        assert_native_omissions_accounted(result.ir(), &report.native_dispositions, &report.losses);
        checked += 1;
    }
    assert!(
        checked >= 26,
        "the 26 decodable Rhino golden fixtures must all be exercised"
    );
}

#[test]
fn cli_neutral_body_name_and_color_override_retained_rhino_attributes() {
    for name in FIXTURES {
        let dir = tempdir().unwrap();
        let input = dir.path().join(name);
        let cadir = dir.path().join("edited.cadir.json");
        let output = dir.path().join("written.3dm");
        let report = dir.path().join("report.json");
        fs::write(&input, fixture(name)).unwrap();
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "dump",
                input.to_str().unwrap(),
                "-o",
                cadir.to_str().unwrap(),
            ])
            .assert()
            .success();
        let mut ir = CadIr::from_json(&fs::read_to_string(&cadir).unwrap()).unwrap();
        if name == "extrusion_hole.3dm" {
            assert_eq!(counts(&ir)[..3], [1, 2, 4]);
        } else {
            assert_eq!(
                [
                    ir.model.bodies.len(),
                    ir.model.curves.len(),
                    ir.model.points.len()
                ],
                [2, 7, 3]
            );
        }
        assert_source_settings_populated(&ir);
        let expected_counts = counts(&ir);
        let color = Color::from_rgba8(32, 96, 160, 255);
        ir.model.bodies[0].name = Some("authored body".into());
        ir.model.bodies[0].color = Some(color);
        fs::write(&cadir, ir.to_canonical_json().unwrap()).unwrap();
        fs::remove_file(decode_sidecar_path(&cadir).unwrap()).unwrap();
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                cadir.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--to",
                TARGET,
                "--report",
                report.to_str().unwrap(),
            ])
            .assert()
            .success();
        let written = decode(&RhinoCodec, &fs::read(output).unwrap());
        assert_eq!(counts(written.ir()), expected_counts, "{name}");
        let body = written
            .ir()
            .model
            .bodies
            .iter()
            .find(|body| body.name.as_deref() == Some("authored body"))
            .expect("the authored name survives synthesis");
        assert_eq!(body.color, Some(color));
        let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        let dispositions: NativeDispositions =
            serde_json::from_value(report["export"]["native_dispositions"].clone()).unwrap();
        let losses: Vec<LossNote> =
            serde_json::from_value(report["export"]["losses"].clone()).unwrap();
        assert_settings_written(&ir, &dispositions);
        assert_native_omissions_accounted(&ir, &dispositions, &losses);
    }
}

#[test]
fn translated_library_edits_with_rebound_history_override_retained_rhino_geometry() {
    for name in FIXTURES {
        let dir = tempdir().unwrap();
        let result = decode(&RhinoCodec, &fixture(name));
        let mut ir = result.ir().clone();
        assert_source_settings_populated(&ir);
        let before = ir.model.points[0].position().get();
        let original_counts = counts(&ir);
        cadmpeg_test_support::contract::translate_model(
            &mut ir,
            cadmpeg_ir::math::Vector3::new(10.0, 0.0, 0.0),
        )
        .expect("translate valid geometry");
        let cadir = dir.path().join("translated.cadir.json");
        let output = dir.path().join("translated.3dm");
        let text = ir.to_canonical_json().unwrap();
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
                "-o",
                output.to_str().unwrap(),
                "--to",
                TARGET,
            ])
            .assert()
            .success()
            .stderr(predicate::str::contains("decode that produced this CADIR"));
        let written = decode(&RhinoCodec, &fs::read(output).unwrap());
        assert_eq!(counts(written.ir()), original_counts, "{name}");
        assert!(
            written.ir().model.points.iter().any(|point| {
                let position = point.position().get();
                (position.x - before.x - 10.0).abs() <= EPS_POINT
                    && (position.y - before.y).abs() <= EPS_POINT
                    && (position.z - before.z).abs() <= EPS_POINT
            }),
            "{name}: the translated point must be in the written model"
        );
        let (_, report) = write_decode(
            &RhinoCodec,
            &RhinoCodec,
            &ir,
            Some(result.source_fidelity()),
            Some(TARGET),
        );
        assert_settings_written(&ir, &report.native_dispositions);
        assert_native_omissions_accounted(&ir, &report.native_dispositions, &report.losses);
    }
}

#[test]
fn unedited_v8_roundtrip_agrees_on_every_arena_declared_written() {
    for name in [FIXTURES[0], FIXTURES[1], "brep.3dm", "generated_point.3dm"] {
        let result = decode(&RhinoCodec, &fixture(name));
        let (written, report) = write_decode(
            &RhinoCodec,
            &RhinoCodec,
            result.ir(),
            Some(result.source_fidelity()),
            Some(TARGET),
        );
        let claims = cadmpeg_test_support::contract::written_arenas(&report);
        let required: &[&str] = match name {
            "brep.3dm" => &["faces", "edges"],
            "generated_point.3dm" => &["bodies", "points"],
            _ => &[],
        };
        for arena in required {
            let arena = cadmpeg_ir::ArenaName::parse(arena).expect("registered arena");
            assert!(
                claims.contains(&arena),
                "{name} must make a substantive Written claim for {arena:?}"
            );
            let model = serde_json::to_value(&result.ir().model).unwrap();
            assert!(
                !model[arena.as_str()].as_array().unwrap().is_empty(),
                "{name}: the required Written arena must be populated"
            );
        }
        cadmpeg_test_support::contract::assert_written_arenas_agree(
            &report,
            result.ir(),
            written.ir(),
        );
    }
}

#[test]
fn reported_rhino_curve_and_mesh_arenas_preserve_their_physical_geometry() {
    for name in [
        "line.3dm",
        "circle.3dm",
        "nurbs_curve.3dm",
        "mesh_v1_0.3dm",
        "mesh_v3_5.3dm",
        "mesh_v3_8.3dm",
    ] {
        let source = decode(&RhinoCodec, &fixture(name));
        let (written, report) = write_decode(
            &RhinoCodec,
            &RhinoCodec,
            source.ir(),
            Some(source.source_fidelity()),
            Some(TARGET),
        );
        assert!(
            !report
                .losses
                .iter()
                .any(|loss| loss.severity >= cadmpeg_ir::report::Severity::Error),
            "{name}: {:?}",
            report.losses
        );
        if name.starts_with("mesh_") {
            assert!(
                !source.ir().model.tessellations.is_empty(),
                "{name} must exercise held mesh vertices and triangles"
            );
            assert_eq!(
                source.ir().model.tessellations.len(),
                written.ir().model.tessellations.len(),
                "{name}"
            );
            for (before, after) in source
                .ir()
                .model
                .tessellations
                .iter()
                .zip(&written.ir().model.tessellations)
            {
                cadmpeg_ir::compare::values_agree(
                    &serde_json::to_value(before.vertices()).unwrap(),
                    &serde_json::to_value(after.vertices()).unwrap(),
                )
                .unwrap_or_else(|error| panic!("{name}: mesh positions differ: {error}"));
                assert_eq!(
                    before.triangles(),
                    after.triangles(),
                    "{name}: triangle indices/order must survive"
                );
                if !before.vertex_normals().is_empty() {
                    cadmpeg_ir::compare::values_agree(
                        &serde_json::to_value(before.vertex_normals()).unwrap(),
                        &serde_json::to_value(after.vertex_normals()).unwrap(),
                    )
                    .unwrap_or_else(|error| panic!("{name}: mesh normals differ: {error}"));
                }
            }
        } else {
            assert!(
                !source.ir().model.curves.is_empty(),
                "{name} must exercise a held solved curve"
            );
            assert_eq!(
                source.ir().model.curves.len(),
                written.ir().model.curves.len(),
                "{name}"
            );
            for (before, after) in source
                .ir()
                .model
                .curves
                .iter()
                .zip(&written.ir().model.curves)
            {
                cadmpeg_ir::compare::values_agree(
                    &serde_json::to_value(&before.geometry).unwrap(),
                    &serde_json::to_value(&after.geometry).unwrap(),
                )
                .unwrap_or_else(|error| panic!("{name}: solved curve differs: {error}"));
            }
        }
    }
}

#[test]
fn unsupported_rhino_subds_and_singular_seams_write_readable_reductions_with_losses() {
    for name in [
        "subd_v50.3dm",
        "subd_v60.3dm",
        "subd_v70.3dm",
        "subd_v80.3dm",
        "brep_singular_seam.3dm",
    ] {
        let source = decode(&RhinoCodec, &fixture(name));
        let (written, report) = write_decode(
            &RhinoCodec,
            &RhinoCodec,
            source.ir(),
            Some(source.source_fidelity()),
            Some(TARGET),
        );
        assert!(
            matches!(report.write_path(), WritePath::Synthesized { .. }),
            "{name}"
        );
        assert!(
            written.ir().model.bodies.is_empty(),
            "{name}: omitted unsupported geometry must not reappear"
        );
        assert!(
            report.losses.iter().any(|loss| loss.code.taxonomy()
                == cadmpeg_ir::report::loss::LossTaxonomy::NoExportableSolids),
            "{name}: empty target output must be reported"
        );
        if name.starts_with("subd_") {
            assert!(
                !source.ir().model.subds.is_empty(),
                "{name} must exercise held subdivision content"
            );
            assert!(written.ir().model.subds.is_empty());
            assert_eq!(
                report
                    .coverage
                    .disposition(cadmpeg_ir::schema::EntityKind::SubdSurface),
                ArenaDisposition::Dropped
            );
            assert!(
                report
                    .losses
                    .iter()
                    .any(|loss| loss.code.local_code() == "writer.subds-omitted"),
                "{name}"
            );
        } else {
            assert!(
                !source.ir().model.faces.is_empty(),
                "the seam fixture must contain a face"
            );
            assert!(written.ir().model.faces.is_empty());
            assert!(
                report
                    .losses
                    .iter()
                    .any(|loss| loss.code.local_code() == "writer.body-omitted"
                        && loss.severity == cadmpeg_ir::report::Severity::Error),
                "a structurally omitted seam body must charge Error"
            );
        }
    }
}

fn assert_settings_written(ir: &CadIr, dispositions: &NativeDispositions) {
    let records = ir
        .native
        .namespace("rhino")
        .unwrap()
        .arenas()
        .get("document_settings")
        .expect("held document settings");
    assert!(!records.is_empty());
    for record in records {
        assert_ne!(
            dispositions.record_disposition("rhino", "document_settings", record.id()),
            NativeDisposition::Dropped,
            "Rule W1 re-emits held typed settings from document state"
        );
    }
}

fn assert_native_omissions_accounted(
    ir: &CadIr,
    dispositions: &NativeDispositions,
    losses: &[LossNote],
) {
    let dropped = dispositions.dropped_counts(ir);
    let notes = losses
        .iter()
        .filter(|loss| loss.code.local_code() == "native.records-omitted")
        .collect::<Vec<_>>();
    assert_eq!(
        notes.len(),
        dropped
            .values()
            .map(std::collections::BTreeMap::len)
            .sum::<usize>(),
        "each actually dropped populated native arena gets one shared loss"
    );
    for (namespace, arenas) in dropped {
        for (arena, count) in arenas {
            let name = format!("`{namespace}.{arena}`");
            let quantity = format!("omits {count} native record(s)");
            assert!(
                notes
                    .iter()
                    .any(|loss| loss.message.contains(&name) && loss.message.contains(&quantity)),
                "missing native omission accounting for {name}: {count}"
            );
        }
    }
}
