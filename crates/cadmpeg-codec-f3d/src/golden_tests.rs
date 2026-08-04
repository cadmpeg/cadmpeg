// SPDX-License-Identifier: Apache-2.0
//! Golden-snapshot harness for decode, inspect, and encode branches.
//!
//! `tests/golden/fixtures/*.f3d` are frozen inputs, and every snapshot here is
//! produced from the committed bytes. Regenerate the artifacts with
//! `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-f3d golden` and review the
//! diff.
//!
//! `UPDATE_GOLDEN=1` deliberately cannot write a fixture. A snapshot separates
//! a codec change from an input change only while the input holds still: once
//! the same command rewrites both sides, a drifting artifact no longer says
//! whether the decoder moved or the bytes under it did, and the tree stops
//! being evidence. Regenerating an input is a separate decision, so it needs
//! the separate `UPDATE_GOLDEN_FIXTURES=1`, and a builder that no longer
//! reproduces its committed input fails
//! [`golden_fixtures_match_builders`] rather than being papered over.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_codec_core::CodecError;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions, DecodeResult, EncodeInput, Encoder};
use cadmpeg_ir::examples;

use super::{
    f3d_with_configuration, f3d_with_smbh, f3d_with_smbh_and_protein, synthetic_comp_spl_sur_smbh,
    synthetic_cyl_spl_sur_smbh, synthetic_exact_spl_sur_smbh, synthetic_free_vertex_body_smbh,
    synthetic_full_rolling_ball_smbh, synthetic_g2_blend_spl_sur_smbh,
    synthetic_geometry_bf4_nurbs_smbh, synthetic_geometry_bf4_smbh, synthetic_geometry_smbh,
    synthetic_geometry_with_attribute_smbh, synthetic_geometry_with_body_color_smbh,
    synthetic_geometry_with_compound_curve_smbh, synthetic_geometry_with_degenerate_curve_smbh,
    synthetic_geometry_with_exact_curve_smbh, synthetic_geometry_with_face_color_smbh,
    synthetic_geometry_with_helix_curve_smbh, synthetic_geometry_with_history_smbh,
    synthetic_geometry_with_law_curve_smbh, synthetic_geometry_with_mesh_surface_smbh,
    synthetic_geometry_with_pcurve_smbh, synthetic_geometry_with_projection_smbh,
    synthetic_geometry_with_rational_pcurve_smbh, synthetic_geometry_with_ref_pcurve_smbh,
    synthetic_geometry_with_silhouette_smbh, synthetic_geometry_with_sketch_link_smbh,
    synthetic_geometry_with_spring_smbh, synthetic_geometry_with_subset_curve_smbh,
    synthetic_geometry_with_surface_intersection_smbh, synthetic_geometry_with_surface_offset_smbh,
    synthetic_geometry_with_transform_smbh, synthetic_helix_surface_smbh,
    synthetic_law_spl_sur_smbh, synthetic_loft_spl_sur_smbh,
    synthetic_minimal_deformable_surface_smbh, synthetic_mixed_face_wire_body_smbh,
    synthetic_net_spl_sur_smbh, synthetic_off_spl_sur_smbh, synthetic_profile_first_sweep_smbh,
    synthetic_rot_spl_sur_smbh, synthetic_ruled_spl_sur_smbh, synthetic_scaled_compound_loft_smbh,
    synthetic_skin_spl_sur_smbh, synthetic_smbh, synthetic_sub_spl_sur_smbh,
    synthetic_sum_spl_sur_smbh, synthetic_t_spl_sur_smbh, synthetic_taper_spl_sur_smbh,
    synthetic_variable_blend_smbh, synthetic_vertex_blend_smbh, synthetic_wire_body_smbh, F3dCodec,
    InspectOptions, SketchLinkForm, TestEncode,
};

/// Covering fixture set as `(golden name, full .f3d bytes)`.
// Fifty-three entries, one per line block, appended in the order a reader adds a
// new fixture. A `vec![]` literal would hold the same content behind one more
// level of indentation and gain nothing.
#[allow(clippy::vec_init_then_push)]
fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    let mut f: Vec<(&'static str, Vec<u8>)> = Vec::new();

    f.push(("topology_base", f3d_with_smbh(&synthetic_geometry_smbh())));
    f.push(("wire_body", f3d_with_smbh(&synthetic_wire_body_smbh())));
    f.push((
        "free_vertex_body",
        f3d_with_smbh(&synthetic_free_vertex_body_smbh()),
    ));
    f.push((
        "mixed_face_wire_body",
        f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh()),
    ));
    f.push((
        "attributes",
        f3d_with_smbh(&synthetic_geometry_with_attribute_smbh()),
    ));
    f.push((
        "body_color",
        f3d_with_smbh(&synthetic_geometry_with_body_color_smbh()),
    ));
    f.push((
        "face_color",
        f3d_with_smbh(&synthetic_geometry_with_face_color_smbh()),
    ));
    f.push((
        "body_transform",
        f3d_with_smbh(&synthetic_geometry_with_transform_smbh()),
    ));
    f.push((
        "history",
        f3d_with_smbh(&synthetic_geometry_with_history_smbh()),
    ));
    f.push((
        "design_appearances_protein",
        f3d_with_smbh_and_protein(&synthetic_geometry_smbh()),
    ));
    f.push(("container_metadata_only", f3d_with_smbh(&synthetic_smbh())));
    f.push((
        "mesh_surface",
        f3d_with_smbh(&synthetic_geometry_with_mesh_surface_smbh()),
    ));
    f.push((
        "sketch_link",
        f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh(
            SketchLinkForm::Tagged("113 0 1 0 2 3"),
        )),
    ));
    f.push((
        "pcurve_inline",
        f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh()),
    ));
    f.push((
        "pcurve_ref",
        f3d_with_smbh(&synthetic_geometry_with_ref_pcurve_smbh()),
    ));
    f.push((
        "pcurve_rational",
        f3d_with_smbh(&synthetic_geometry_with_rational_pcurve_smbh()),
    ));
    f.push((
        "exact_curve",
        f3d_with_smbh(&synthetic_geometry_with_exact_curve_smbh()),
    ));
    f.push((
        "subset_curve",
        f3d_with_smbh(&synthetic_geometry_with_subset_curve_smbh()),
    ));
    f.push((
        "compound_curve",
        f3d_with_smbh(&synthetic_geometry_with_compound_curve_smbh()),
    ));
    f.push((
        "helix_curve",
        f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh()),
    ));
    f.push((
        "law_curve",
        f3d_with_smbh(&synthetic_geometry_with_law_curve_smbh()),
    ));
    f.push((
        "silhouette_curve",
        f3d_with_smbh(&synthetic_geometry_with_silhouette_smbh(
            "para_silh_int_cur",
            None,
        )),
    ));
    f.push((
        "surface_offset_curve",
        f3d_with_smbh(&synthetic_geometry_with_surface_offset_smbh()),
    ));
    f.push((
        "spring_curve",
        f3d_with_smbh(&synthetic_geometry_with_spring_smbh()),
    ));
    f.push((
        "projection_curve",
        f3d_with_smbh(&synthetic_geometry_with_projection_smbh()),
    ));
    f.push((
        "surface_intersection_curve",
        f3d_with_smbh(&synthetic_geometry_with_surface_intersection_smbh()),
    ));
    f.push((
        "degenerate_curve",
        f3d_with_smbh(&synthetic_geometry_with_degenerate_curve_smbh()),
    ));
    f.push((
        "cyl_spline_surface",
        f3d_with_smbh(&synthetic_cyl_spl_sur_smbh()),
    ));
    f.push((
        "exact_surface",
        f3d_with_smbh(&synthetic_exact_spl_sur_smbh("exact_spl_sur")),
    ));
    f.push((
        "ruled_surface",
        f3d_with_smbh(&synthetic_ruled_spl_sur_smbh("rule_sur", true)),
    ));
    f.push((
        "revolution_surface",
        f3d_with_smbh(&synthetic_rot_spl_sur_smbh("rot_spl_sur")),
    ));
    f.push((
        "offset_surface",
        f3d_with_smbh(&synthetic_off_spl_sur_smbh("off_spl_sur")),
    ));
    f.push((
        "taper_surface",
        f3d_with_smbh(&synthetic_taper_spl_sur_smbh("taper_spl_sur")),
    ));
    f.push((
        "sum_surface",
        f3d_with_smbh(&synthetic_sum_spl_sur_smbh("sum_spl_sur", true)),
    ));
    f.push((
        "sub_surface",
        f3d_with_smbh(&synthetic_sub_spl_sur_smbh("sub_spl_sur")),
    ));
    f.push((
        "loft_surface",
        f3d_with_smbh(&synthetic_loft_spl_sur_smbh("loft_spl_sur")),
    ));
    f.push((
        "compound_loft_surface",
        f3d_with_smbh(&synthetic_comp_spl_sur_smbh()),
    ));
    f.push((
        "scaled_compound_loft_surface",
        f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(true)),
    ));
    f.push((
        "law_surface",
        f3d_with_smbh(&synthetic_law_spl_sur_smbh("law_spl_sur", false, 0)),
    ));
    f.push((
        "skin_surface",
        f3d_with_smbh(&synthetic_skin_spl_sur_smbh(0, false)),
    ));
    f.push(("net_surface", f3d_with_smbh(&synthetic_net_spl_sur_smbh())));
    f.push((
        "sweep_surface",
        f3d_with_smbh(&synthetic_profile_first_sweep_smbh()),
    ));
    f.push((
        "g2_blend_surface",
        f3d_with_smbh(&synthetic_g2_blend_spl_sur_smbh("g2_blend_spl_sur", true)),
    ));
    f.push((
        "variable_blend_surface",
        f3d_with_smbh(&synthetic_variable_blend_smbh("var_blend_spl_sur")),
    ));
    f.push((
        "rolling_ball_blend_surface",
        f3d_with_smbh(&synthetic_full_rolling_ball_smbh("rb_blend_spl_sur")),
    ));
    f.push((
        "vertex_blend_surface",
        f3d_with_smbh(&synthetic_vertex_blend_smbh("VBL_SURF")),
    ));
    f.push((
        "helix_surface",
        f3d_with_smbh(&synthetic_helix_surface_smbh(true)),
    ));
    f.push((
        "tspline_surface",
        f3d_with_smbh(&synthetic_t_spl_sur_smbh()),
    ));
    f.push((
        "deformable_surface",
        f3d_with_smbh(&synthetic_minimal_deformable_surface_smbh()),
    ));
    f.push(("binaryfile4", f3d_with_smbh(&synthetic_geometry_bf4_smbh())));
    f.push((
        "binaryfile4_nurbs",
        f3d_with_smbh(&synthetic_geometry_bf4_nurbs_smbh()),
    ));
    f.push((
        "design_configuration",
        f3d_with_configuration(
            &synthetic_geometry_smbh(),
            "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg",
            br#"{"configurations":{"wide":{"parameters":{"width":"25 mm"},"suppressed":["slot"]}},"active":"wide"}"#,
        ),
    ));
    f.push(("generated_unit_cube", encoder_generated_unit_cube()));
    f
}

fn encoder_generated_unit_cube() -> Vec<u8> {
    let ir = examples::unit_cube();
    let mut bytes = Vec::new();
    F3dCodec
        .encode(&ir, &mut bytes)
        .expect("encode neutral unit-cube IR");
    bytes
}

fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn update_requested() -> bool {
    std::env::var_os("UPDATE_GOLDEN").is_some()
}

/// Whether the caller asked to rewrite the frozen `tests/golden/fixtures/*.f3d`
/// inputs from the builders. Separate from [`update_requested`] on purpose; see
/// the module documentation.
fn fixture_update_requested() -> bool {
    std::env::var_os("UPDATE_GOLDEN_FIXTURES").is_some()
}

fn read_fixture(name: &str) -> Result<Vec<u8>, String> {
    let path = golden_root().join("fixtures").join(format!("{name}.f3d"));
    std::fs::read(&path).map_err(|error| {
        format!(
            "fixture `{name}`: cannot read {} ({error}); restore the committed input, or rebuild it from its builder with `UPDATE_GOLDEN_FIXTURES=1 cargo test -p cadmpeg-codec-f3d golden`",
            path.display()
        )
    })
}

fn decode_result(bytes: &[u8]) -> Result<DecodeResult, CodecError> {
    F3dCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
}

fn indent_block(block: &str) -> String {
    let mut lines = block.lines();
    let mut out = String::new();
    if let Some(first) = lines.next() {
        out.push_str(first);
    }
    for line in lines {
        out.push('\n');
        out.push_str("  ");
        out.push_str(line);
    }
    out
}

fn decode_snapshot(bytes: &[u8]) -> String {
    match decode_result(bytes) {
        Ok(result) => {
            let ir = result
                .ir
                .to_canonical_json()
                .expect("serialize canonical ir");
            let report = serde_json::to_string_pretty(&result.report).expect("serialize report");
            let fidelity = serde_json::to_string_pretty(&result.source_fidelity)
                .expect("serialize source_fidelity");
            let mut out = String::from("{\n");
            out.push_str("  \"ir\": ");
            out.push_str(&indent_block(&ir));
            out.push_str(",\n  \"report\": ");
            out.push_str(&indent_block(&report));
            out.push_str(",\n  \"source_fidelity\": ");
            out.push_str(&indent_block(&fidelity));
            out.push_str("\n}\n");
            out
        }
        Err(error) => {
            let value = serde_json::json!({ "decode_error": error.to_string() });
            let mut text = serde_json::to_string_pretty(&value).expect("serialize decode error");
            text.push('\n');
            text
        }
    }
}

fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match F3dCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default())
    {
        Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    let mut text = serde_json::to_string_pretty(&value).expect("serialize inspect snapshot");
    text.push('\n');
    text
}

fn replay_outcome(bytes: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let result = decode_result(bytes).ok()?;
    let mut out = Vec::new();
    Some(
        match F3dCodec
            .plan(EncodeInput {
                ir: &result.ir,
                fidelity: Some(&result.source_fidelity),
            })
            .and_then(|plan| plan.write_to(&mut out))
        {
            Ok(_) => Ok(out),
            Err(error) => Err(error.to_string()),
        },
    )
}

fn generate_outcome(bytes: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let result = decode_result(bytes).ok()?;
    let mut out = Vec::new();
    Some(match F3dCodec.encode(&result.ir, &mut out) {
        Ok(_) => Ok(out),
        Err(error) => Err(error.to_string()),
    })
}

fn patch_outcome(bytes: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let result = decode_result(bytes).ok()?;
    if result.ir.model.points.is_empty() {
        return None;
    }
    let mut edited = result.ir.clone();
    edited.model.points[0].position.x += 1.0;
    let mut out = Vec::new();
    Some(
        match F3dCodec.write_preserved_with_source_fidelity(
            &edited,
            &result.source_fidelity,
            &mut out,
        ) {
            Ok(()) => Ok(out),
            Err(error) => Err(error.to_string()),
        },
    )
}

fn first_line_diff(expected: &str, actual: &str) -> (usize, String, String) {
    let mut exp = expected.lines();
    let mut act = actual.lines();
    let mut line = 0usize;
    loop {
        line += 1;
        match (exp.next(), act.next()) {
            (Some(expected_line), Some(actual_line)) if expected_line == actual_line => {}
            (expected_line, actual_line) => {
                let trunc = |value: Option<&str>| match value {
                    Some(text) if text.len() > 200 => format!("{}…", &text[..200]),
                    Some(text) => text.to_string(),
                    None => "<end of file>".to_string(),
                };
                return (line, trunc(expected_line), trunc(actual_line));
            }
        }
    }
}

fn first_byte_diff(expected: &[u8], actual: &[u8]) -> String {
    let offset = expected
        .iter()
        .zip(actual)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    let describe = |bytes: &[u8]| match bytes.get(offset) {
        Some(byte) => format!("0x{byte:02x}"),
        None => "<end of file>".to_string(),
    };
    format!(
        "first byte difference at offset {offset}: golden {}, actual {} (lengths: {} and {})",
        describe(expected),
        describe(actual),
        expected.len(),
        actual.len(),
    )
}

fn compare_text(update: bool, path: &Path, actual: &str, failures: &mut Vec<String>) {
    if update {
        std::fs::create_dir_all(path.parent().expect("golden path has a parent"))
            .expect("create golden dir");
        std::fs::write(path, actual.as_bytes())
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        return;
    }
    match std::fs::read_to_string(path) {
        Ok(expected) => {
            let expected = expected.replace("\r\n", "\n");
            let actual = actual.replace("\r\n", "\n");
            if expected == actual {
                return;
            }
            let (line, expected_line, actual_line) = first_line_diff(&expected, &actual);
            failures.push(format!(
                "{}: diverged at line {line}\n    golden: {expected_line}\n    actual: {actual_line}",
                path.display()
            ));
        }
        Err(error) => failures.push(format!(
            "{}: cannot read golden ({error}); regenerate with `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-f3d golden`",
            path.display()
        )),
    }
}

fn compare_bytes(update: bool, path: &Path, actual: &[u8], failures: &mut Vec<String>) {
    if update {
        std::fs::create_dir_all(path.parent().expect("golden path has a parent"))
            .expect("create golden dir");
        std::fs::write(path, actual)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        return;
    }
    match std::fs::read(path) {
        Ok(expected) if expected == actual => {}
        Ok(expected) => {
            failures.push(format!(
                "{}: {}",
                path.display(),
                first_byte_diff(&expected, actual)
            ));
        }
        Err(error) => failures.push(format!(
            "{}: cannot read golden ({error}); regenerate with `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-f3d golden`",
            path.display()
        )),
    }
}

fn remove_if_exists(path: &Path) {
    if path.exists() {
        std::fs::remove_file(path)
            .unwrap_or_else(|error| panic!("remove {}: {error}", path.display()));
    }
}

fn compare_encode(
    update: bool,
    root: &Path,
    dir: &str,
    name: &str,
    outcome: Option<Result<Vec<u8>, String>>,
    failures: &mut Vec<String>,
) {
    let bin_path = root.join(dir).join(format!("{name}.bin"));
    let err_path = root.join(dir).join(format!("{name}.err.txt"));
    match outcome {
        Some(Ok(encoded)) => {
            if update {
                remove_if_exists(&err_path);
            }
            compare_bytes(update, &bin_path, &encoded, failures);
        }
        Some(Err(message)) => {
            if update {
                remove_if_exists(&bin_path);
            }
            compare_text(update, &err_path, &format!("{message}\n"), failures);
        }
        None if update => {
            remove_if_exists(&bin_path);
            remove_if_exists(&err_path);
        }
        None => {}
    }
}

#[test]
fn golden_snapshots_are_byte_identical() {
    let update = update_requested();
    let root = golden_root();
    let mut failures = Vec::new();
    for (name, builder_bytes) in fixtures() {
        if fixture_update_requested() {
            let fixture_path = root.join("fixtures").join(format!("{name}.f3d"));
            std::fs::create_dir_all(fixture_path.parent().expect("fixtures dir"))
                .expect("create fixtures dir");
            std::fs::write(&fixture_path, &builder_bytes)
                .unwrap_or_else(|error| panic!("write fixture {name}: {error}"));
        }
        // Always snapshot the committed bytes, never the builder's, so an
        // artifact is pinned to the input a reviewer can read off disk.
        let input = match read_fixture(name) {
            Ok(value) => value,
            Err(message) => {
                failures.push(message);
                continue;
            }
        };

        compare_text(
            update,
            &root.join("decode").join(format!("{name}.json")),
            &decode_snapshot(&input),
            &mut failures,
        );
        compare_text(
            update,
            &root.join("inspect").join(format!("{name}.json")),
            &inspect_snapshot(&input),
            &mut failures,
        );
        compare_encode(
            update,
            &root,
            "replay",
            name,
            replay_outcome(&input),
            &mut failures,
        );
        compare_encode(
            update,
            &root,
            "generate",
            name,
            generate_outcome(&input),
            &mut failures,
        );
        compare_encode(
            update,
            &root,
            "patch",
            name,
            patch_outcome(&input),
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "{} golden artifact(s) drifted; if the change is intended regenerate with `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-f3d golden` and review the diff:\n\n{}",
        failures.len(),
        failures.join("\n\n"),
    );
}

#[test]
fn golden_output_is_deterministic() {
    for (name, bytes) in fixtures() {
        let decode_first = decode_snapshot(&bytes);
        let decode_second = decode_snapshot(&bytes);
        if decode_first != decode_second {
            let (line, first, second) = first_line_diff(&decode_first, &decode_second);
            panic!(
                "fixture `{name}`: nondeterministic decode at line {line}\n    run 1: {first}\n    run 2: {second}"
            );
        }
        let inspect_first = inspect_snapshot(&bytes);
        let inspect_second = inspect_snapshot(&bytes);
        if inspect_first != inspect_second {
            let (line, first, second) = first_line_diff(&inspect_first, &inspect_second);
            panic!(
                "fixture `{name}`: nondeterministic inspect at line {line}\n    run 1: {first}\n    run 2: {second}"
            );
        }
        assert_eq!(
            replay_outcome(&bytes),
            replay_outcome(&bytes),
            "fixture `{name}`: nondeterministic replay"
        );
        assert_eq!(
            generate_outcome(&bytes),
            generate_outcome(&bytes),
            "fixture `{name}`: nondeterministic generate"
        );
        assert_eq!(
            patch_outcome(&bytes),
            patch_outcome(&bytes),
            "fixture `{name}`: nondeterministic patch"
        );
    }
}

/// The tripwire that makes a frozen input auditable: every committed fixture
/// must still be exactly what its builder produces, so a builder edit surfaces
/// here instead of silently invalidating the artifacts pinned against the old
/// bytes.
#[test]
fn golden_fixtures_match_builders() {
    if fixture_update_requested() {
        return;
    }
    let mut failures: Vec<String> = Vec::new();
    for (name, builder_bytes) in fixtures() {
        match read_fixture(name) {
            Ok(bytes) if bytes == builder_bytes => {}
            // Report the first differing offset, not just the lengths: a
            // builder can rewrite a record in place and leave the length alone,
            // and a length-only message reads as "no difference".
            Ok(bytes) => failures.push(format!(
                "fixture `{name}`: {}",
                first_byte_diff(&bytes, &builder_bytes)
            )),
            Err(message) => failures.push(message),
        }
    }
    assert!(
        failures.is_empty(),
        "committed fixtures diverged from their builders; either restore the inputs or, if the builder change is intended, rebuild them with `UPDATE_GOLDEN_FIXTURES=1 cargo test -p cadmpeg-codec-f3d golden` and regenerate every artifact in the same commit:\n\n{}",
        failures.join("\n")
    );
}
