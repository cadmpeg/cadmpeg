// SPDX-License-Identifier: Apache-2.0
//! Rhino native metadata cannot block synthesis or override neutral edits.

use std::fs;

use cadmpeg_codec_rhino::RhinoCodec;
use cadmpeg_ir::codec::write::NativeDispositions;
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::{decode, workspace, write_decode};

const TARGET: &str = "rhino:archive-80";
const FIXTURES: [&str; 2] = ["extrusion_hole.3dm", "curves_mixed.3dm"];

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        workspace()
            .join("crates/cadmpeg-codec-rhino/tests/golden/fixtures")
            .join(name),
    )
    .unwrap()
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
