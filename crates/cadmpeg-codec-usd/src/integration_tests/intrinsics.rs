// SPDX-License-Identifier: Apache-2.0
//! Intrinsic primitives become B-rep bodies with their exact volumes and
//! areas (spec §11.5).

use std::f64::consts::PI;

use cadmpeg_measure::test_support::round_trip::measured;
use cadmpeg_measure::value::Quantity;

use crate::test_support::decode;

/// Allowance of a measured value beyond its numeric error, per unit of the
/// expected magnitude.
const RELATIVE_FLOOR: f64 = 1e-9;

fn fixture() -> String {
    std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/intrinsics.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

fn value(quantity: Option<&Quantity<cadmpeg_measure::value::Scalar>>) -> (f64, f64) {
    let measured = quantity
        .and_then(Quantity::measured)
        .expect("a measured value");
    (measured.value().value.get(), measured.numeric_error().get())
}

/// Volume and area in cubic and square millimetres; the layer is in
/// centimetres.
fn expected() -> Vec<(&'static str, Option<f64>, f64)> {
    let cone_slant = (20.0_f64 * 20.0 + 30.0 * 30.0).sqrt();
    let frustum_slant = (10.0_f64 * 10.0 + 30.0 * 30.0).sqrt();
    vec![
        ("Box", Some(27_000.0), 5_400.0),
        ("Ball", Some(4.0 / 3.0 * PI * 8_000.0), 4.0 * PI * 400.0),
        (
            "Can",
            Some(PI * 100.0 * 40.0),
            2.0 * PI * 10.0 * 40.0 + 2.0 * PI * 100.0,
        ),
        (
            "Funnel",
            Some(PI * 400.0 * 30.0 / 3.0),
            PI * 400.0 + PI * 20.0 * cone_slant,
        ),
        (
            "Pill",
            Some(PI * 100.0 * 20.0 + 4.0 / 3.0 * PI * 1_000.0),
            2.0 * PI * 10.0 * 20.0 + 4.0 * PI * 100.0,
        ),
        (
            "Frustum",
            Some(PI * 30.0 / 3.0 * (400.0 + 200.0 + 100.0)),
            PI * 30.0 * frustum_slant + PI * 400.0 + PI * 100.0,
        ),
        ("Sheet", None, 2_000.0),
    ]
}

fn assert_measures(ir: &cadmpeg_ir::CadIr) {
    let report = measured("intrinsics", ir).expect("measured");
    for (name, volume, area) in expected() {
        let body = report
            .bodies()
            .iter()
            .find(|body| body.name().as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no body {name}"));
        let close = |(found, numeric): (f64, f64), expected: f64| {
            (found - expected).abs() <= numeric + RELATIVE_FLOOR * expected
        };
        assert!(close(value(body.area().as_ref()), area), "{name} area");
        if let Some(volume) = volume {
            assert!(
                close(value(body.volume().as_ref()), volume),
                "{name} volume"
            );
        }
    }
}

#[test]
fn intrinsic_primitives_are_exact_bodies() {
    let result = decode(&fixture());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("intrinsic primitives")));
    assert_measures(result.ir());
}

#[test]
fn a_capsule_with_unequal_radii_is_reported() {
    let result = decode("#usda 1.0\ndef Capsule_1 \"C\"\n{\n    double radiusTop = 1\n    double radiusBottom = 2\n}\n");
    assert!(result.ir().model.bodies.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("unequal radii")));
}

/// The carrier kind of each face and edge of `model`, and the number of
/// pcurves of each coedge, by entity identity.
fn carriers(model: &cadmpeg_ir::document::Model) -> Vec<(String, String)> {
    let json = serde_json::to_value(model).expect("a model serializes");
    let kinds = |arena: &str| -> std::collections::BTreeMap<String, String> {
        json[arena]
            .as_array()
            .into_iter()
            .flatten()
            .map(|record| {
                (
                    record["id"].to_string(),
                    record["geometry"]["kind"].to_string(),
                )
            })
            .collect()
    };
    let (surfaces, curves) = (kinds("surfaces"), kinds("curves"));
    let mut out: Vec<(String, String)> = Vec::new();
    let records = |arena: &str| json[arena].as_array().cloned().unwrap_or_default();
    for face in records("faces") {
        let kind = surfaces.get(&face["surface"].to_string()).cloned();
        out.push((face["id"].to_string(), kind.unwrap_or_default()));
    }
    for edge in records("edges") {
        let kind = curves.get(&edge["carrier"]["curve"].to_string()).cloned();
        out.push((edge["id"].to_string(), kind.unwrap_or_default()));
    }
    for coedge in records("coedges") {
        let pcurves = coedge["pcurves"].as_array().map_or(0, Vec::len);
        out.push((coedge["id"].to_string(), pcurves.to_string()));
    }
    out.sort();
    out
}

#[test]
fn analytic_carriers_survive_a_write() {
    let first = decode(&fixture());
    let (text, _) = crate::test_support::encode(first.ir());
    assert!(text.contains("CadBrepAnalyticAPI"), "{text}");
    let second = decode(&text);
    let (before, after) = (&first.ir().model, &second.ir().model);
    assert_eq!(carriers(before), carriers(after));
    let loops = |model: &cadmpeg_ir::document::Model| model.loops.len();
    assert_eq!(loops(before), loops(after));
}
