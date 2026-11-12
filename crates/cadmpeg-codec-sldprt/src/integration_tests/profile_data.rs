// SPDX-License-Identifier: Apache-2.0
//! Feature-reference curve-data diagnostics and validation.

use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::CadIr;
use serde_json::{json, Value};

const OWNER: &str = "synthetic:test:feature#profile";
const EXTRUDE: &str = "synthetic:test:feature#extrude";
const SKETCH: &str = "synthetic:test:sketch#profile";

fn profile_model(geometry: &Value, construction: bool) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.features = serde_json::from_value(json!([
        {"id": OWNER, "ordinal": 0, "suppressed": false,
         "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": SKETCH}}},
        {"id": EXTRUDE, "ordinal": 1, "suppressed": false,
         "definition": {"definition": "extrude", "profile": {"kind": "feature", "value": OWNER},
          "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"},
          "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 3.0}}},
          "op": "join", "solid": true}}
    ])).expect("synthetic feature definitions");
    ir.model.sketches.push(
        serde_json::from_value(json!({
            "id": SKETCH, "placement": {"kind": "unresolved"}
        }))
        .expect("synthetic sketch"),
    );
    ir.model.sketch_entities.push(
        serde_json::from_value(json!({
            "id": "synthetic:test:entity#curve", "sketch": SKETCH,
            "construction": construction, "geometry": geometry
        }))
        .expect("synthetic geometry"),
    );
    crate::native::SldprtNative::default()
        .store(
            &cadmpeg_test_support::service_decode_context(),
            ir.native.namespace_mut("sldprt"),
        )
        .expect("synthetic native namespace");
    ir
}

fn assert_curve_diagnostic(ir: &CadIr, missing: bool) {
    let mut report = DecodeBody::new(DecodeTransfer::full(true));
    crate::decode::append_design_losses(
        &cadmpeg_test_support::service_decode_context(),
        ir,
        &mut report,
    )
    .expect("design diagnostics");
    let losses = report
        .losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::SldprtLossCode::FeatureProfileCurvesUnavailable.kind()
        })
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), usize::from(missing));
    if missing {
        assert_eq!(
            losses[0].message,
            "1 extrusion feature-reference profile(s) lack decoded non-construction curves."
        );
    }
    assert_eq!(
        report.losses.iter().any(|loss| {
            loss.code == crate::loss::SldprtLossCode::FeatureTypedOperandIncomplete.kind()
        }),
        missing
    );
    let findings = crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        ir,
    )
    .expect("native validation");
    let findings = findings
        .iter()
        .filter(|finding| finding.entity.as_deref() == Some(EXTRUDE))
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), usize::from(missing));
    if missing {
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(
            findings[0].message,
            "SolidWorks extrusion feature-reference profile lacks decoded non-construction curves"
        );
    }
}

#[test]
fn feature_profile_curve_data_distinguishes_points_native_and_construction() {
    let circle = json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 2.0});
    assert_curve_diagnostic(&profile_model(&circle, false), false);
    assert_curve_diagnostic(&profile_model(&circle, true), true);
    assert_curve_diagnostic(
        &profile_model(
            &json!({"kind": "point", "position": {"u": 0.0, "v": 0.0}}),
            false,
        ),
        true,
    );
    assert_curve_diagnostic(
        &profile_model(
            &json!({"kind": "native", "native_kind": "synthetic:curve"}),
            false,
        ),
        true,
    );
    assert_curve_diagnostic(
        &profile_model(
            &json!({"kind": "external_reference", "object": "synthetic:curve"}),
            false,
        ),
        true,
    );
}

#[test]
fn feature_profile_curve_data_accepts_lines_arcs_circles_and_splines() {
    for geometry in [
        json!({"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 2.0, "v": 0.0}}),
        json!({"kind": "arc", "center": {"u": 0.0, "v": 0.0}, "radius": 2.0, "start_angle": 0.0, "end_angle": 1.0}),
        json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 2.0}),
        json!({"kind": "nurbs", "curve": {"degree": 1, "knots": [0.0, 0.0, 1.0, 1.0],
            "poles": {"form": "polynomial", "points": [{"u": 0.0, "v": 0.0}, {"u": 2.0, "v": 0.0}]}, "periodic": false}}),
    ] {
        assert_curve_diagnostic(&profile_model(&geometry, false), false);
    }
}

#[test]
fn feature_profile_curve_data_requires_the_referenced_owner_and_sketch() {
    let geometry = json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 2.0});
    let mut ir = profile_model(&geometry, false);
    ir.model.sketches.clear();
    assert_curve_diagnostic(&ir, true);
    let mut ir = profile_model(&geometry, false);
    ir.model.sketch_entities[0].sketch =
        cadmpeg_ir::sketches::SketchId::mint("synthetic:test:sketch#other").expect("identity");
    assert_curve_diagnostic(&ir, true);
    let mut ir = profile_model(&geometry, false);
    ir.model.features.remove(0);
    assert_curve_diagnostic(&ir, true);
    let mut ir = profile_model(&geometry, false);
    ir.model.sketch_entities.clear();
    assert_curve_diagnostic(&ir, true);
}

#[test]
fn native_profile_diagnostics_require_sldprt_namespace() {
    let mut ir = profile_model(
        &json!({"kind": "point", "position": {"u": 0.0, "v": 0.0}}),
        false,
    );
    ir.native = cadmpeg_ir::native::Native::default();
    assert!(crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
    )
    .expect("native validation")
    .is_empty());
}
