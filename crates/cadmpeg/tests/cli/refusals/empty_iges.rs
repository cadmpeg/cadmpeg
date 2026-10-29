// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Cursor;

use assert_cmd::Command;
use cadmpeg_ir::codec::write::NativeDisposition;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::{CadIr, NativeRecord};
use serde_json::{json, Value};
use tempfile::tempdir;

use crate::support::fixture;

fn native_document(arena: &str, id: &str, fields: &Value) -> CadIr {
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("iges").arenas_mut().insert(
        arena.into(),
        vec![NativeRecord::new(
            Identity::new(id).unwrap(),
            fields.as_object().unwrap().clone(),
        )
        .unwrap()],
    );
    ir
}

fn empty_warning(report: &Value) -> Vec<&Value> {
    report["export"]["losses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|loss| {
            loss["code"]["namespace"] == "export" && loss["code"]["code"] == "output.empty"
        })
        .collect()
}

#[test]
fn current_native_font_is_written_but_does_not_make_geometry_nonempty() {
    let ir = native_document(
        "line_fonts",
        "iges:presentation:line-font#D1",
        &json!({
            "source_entity":"iges:entity:directory#1", "kind":"visible_blank_pattern",
            "fallback_line_font_number":1, "segment_count":2, "lengths":[2.0,1.0],
            "hexadecimal_pattern":"41"
        }),
    );
    assert!(ir.model.bodies.is_empty());
    assert!(ir.model.points.is_empty());
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "font.cadir.json", &ir);
    let output = dir.path().join("font.igs");
    let report = dir.path().join("font.report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "iges:5.3-fixed-ascii",
            "-o",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    let value: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["status"], "ok");
    let export: cadmpeg_ir::report::export::ExportReport =
        serde_json::from_value(value["export"].clone()).unwrap();
    assert!(matches!(
        export.native_dispositions.record_disposition(
            "iges",
            "line_fonts",
            "iges:presentation:line-font#D1",
        ),
        NativeDisposition::Written | NativeDisposition::Regenerated
    ));
    let warnings = empty_warning(&value);
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["severity"], "warning");
    let decoded = cadmpeg_codec_iges::IgesCodec
        .decode(
            &mut Cursor::new(fs::read(output).unwrap()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let fonts = &decoded.ir().native.namespace("iges").unwrap().arenas()["line_fonts"];
    assert_eq!(fonts.len(), 1);
    let fields = fonts[0].fields().unwrap();
    assert_eq!(fields["kind"], "visible_blank_pattern");
    assert_eq!(fields["lengths"], json!([2.0, 1.0]));
    assert_eq!(fields["hexadecimal_pattern"], "41");
    assert!(decoded.ir().model.points.is_empty());
    assert!(decoded.ir().model.curves.is_empty());
    assert!(decoded.ir().model.surfaces.is_empty());
    assert!(decoded.ir().model.bodies.is_empty());
    for (suffix, policy, code) in [
        ("empty", "--reject-empty", "empty_geometry"),
        ("loss", "--reject-lossy=export", "export_loss_rejected"),
    ] {
        let refused = dir.path().join(format!("refused-{suffix}.igs"));
        let report = dir.path().join(format!("refused-{suffix}.json"));
        Command::cargo_bin("cadmpeg")
            .unwrap()
            .args([
                "convert",
                input.to_str().unwrap(),
                "--to",
                "iges:5.3-fixed-ascii",
                "-o",
                refused.to_str().unwrap(),
                policy,
                "--report",
                report.to_str().unwrap(),
            ])
            .assert()
            .code(1);
        assert!(!refused.exists());
        let value: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(value["refusal"]["code"], code);
        assert_eq!(empty_warning(&value).len(), 1);
    }
}

#[test]
fn current_native_flash_writes_real_geometry_without_a_neutral_projection() {
    let ir = native_document(
        "flashes",
        "iges:native:flash#D1",
        &json!({
            "source_entity":"iges:entity:directory#1", "form":2,
            "reference_point":[5.0,6.0], "dimension_1":10.0, "dimension_2":20.0,
            "rotation":0.5, "reference_entity":null
        }),
    );
    assert!(ir.model.bodies.is_empty());
    assert!(ir.model.points.is_empty());
    assert!(ir.model.curves.is_empty());
    assert!(ir.model.surfaces.is_empty());
    let dir = tempdir().unwrap();
    let input = fixture(dir.path(), "flash.cadir.json", &ir);
    let output = dir.path().join("flash.igs");
    let report = dir.path().join("flash.report.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--to",
            "iges:5.3-fixed-ascii",
            "--reject-empty",
            "-o",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .success();
    let value: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["status"], "ok");
    let export: cadmpeg_ir::report::export::ExportReport =
        serde_json::from_value(value["export"].clone()).unwrap();
    assert!(matches!(
        export
            .native_dispositions
            .record_disposition("iges", "flashes", "iges:native:flash#D1"),
        NativeDisposition::Written | NativeDisposition::Regenerated
    ));
    assert!(empty_warning(&value).is_empty());
    let decoded = cadmpeg_codec_iges::IgesCodec
        .decode(
            &mut Cursor::new(fs::read(output).unwrap()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(
        decoded.ir().model.points[0].position().get(),
        cadmpeg_ir::math::Point3::new(5.0, 6.0, 0.0)
    );
    let flashes = &decoded.ir().native.namespace("iges").unwrap().arenas()["flashes"];
    assert_eq!(flashes.len(), 1);
    let fields = flashes[0].fields().unwrap();
    assert_eq!(fields["form"], 2);
    assert_eq!(fields["reference_point"], json!([5.0, 6.0]));
    assert_eq!(fields["dimension_1"], 10.0);
    assert_eq!(fields["dimension_2"], 20.0);
    assert_eq!(fields["rotation"], 0.5);
}
