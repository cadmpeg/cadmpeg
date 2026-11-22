// SPDX-License-Identifier: Apache-2.0
//! Feature lengths of the NIST inch specimens: each is the stored metres
//! value of its dimension, in millimetres.

use cadmpeg_ir::CadIr;
use serde_json::Value;

use crate::test_support::nist::{ftc_06, ftc_08};

const MM_PER_INCH: f64 = 25.4;
const LENGTH_TOLERANCE_MM: f64 = 1.0e-9;

fn operation(ir: &CadIr, name: &str) -> Value {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing feature {name}"));
    let definition = serde_json::to_value(feature.evaluation.definition())
        .unwrap_or_else(|error| panic!("serialize {name}: {error}"));
    definition["value"]["value"].clone()
}

fn blind_depths(ir: &CadIr, name: &str) -> Vec<f64> {
    let extent = &operation(ir, name)["extent"];
    ["side", "first", "second"]
        .iter()
        .filter_map(|side| extent.get(side))
        .filter(|side| side["termination"]["kind"] == "blind")
        .map(|side| {
            side["termination"]["length"]
                .as_f64()
                .expect("blind length")
        })
        .collect()
}

fn assert_inches(name: &str, actual: &[f64], inches: &[f64]) {
    assert_eq!(actual.len(), inches.len(), "{name}: {actual:?}");
    for (actual, inches) in actual.iter().zip(inches) {
        let expected = inches * MM_PER_INCH;
        assert!(
            (actual - expected).abs() <= LENGTH_TOLERANCE_MM,
            "{name}: {actual} mm, expected {expected} mm"
        );
    }
}

fn assert_depths(ir: &CadIr, expected: &[(&str, &[f64])]) {
    for (name, inches) in expected {
        assert_inches(name, &blind_depths(ir, name), inches);
    }
}

fn assert_field(ir: &CadIr, field: &[&str], expected: &[(&str, f64)]) {
    for (name, inches) in expected {
        let value = field
            .iter()
            .fold(operation(ir, name), |value, key| value[*key].clone());
        assert_inches(name, &[value.as_f64().expect("length field")], &[*inches]);
    }
}

#[test]
fn ftc_06_extrude_depths_plane_offsets_and_pattern_spacings_are_stored_inches() {
    let ir = ftc_06().ir();
    assert_depths(
        ir,
        &[
            ("Cut-Extrude2", &[0.1]),
            ("Boss-Extrude1", &[6.0, 6.0]),
            ("Boss-Extrude2", &[1.5]),
        ],
    );
    assert_field(
        ir,
        &["distance"],
        &[
            ("Plane2", 9.75),
            ("Plane3", 1.85),
            ("Plane4", 3.0),
            ("Plane5", 4.5),
            ("Plane14", 2.6),
            ("Plane17", -3.25),
        ],
    );
    assert_field(
        ir,
        &["pattern", "spacing"],
        &[
            ("LPattern1", 9.0),
            ("LPattern2", 6.0),
            ("LPattern3", 2.85),
            ("LPattern4", 10.0),
            ("LPattern5", 6.5),
        ],
    );
}

#[test]
fn ftc_08_extrude_depths_and_pattern_spacings_are_stored_inches() {
    let ir = ftc_08().ir();
    assert_depths(
        ir,
        &[
            ("Cut-Extrude1", &[1.765]),
            ("Cut-Extrude3", &[1.765]),
            ("Cut-Extrude4", &[1.765]),
            ("Cut-Extrude5", &[0.05]),
            ("Boss-Extrude1", &[0.4]),
            ("Boss-Extrude2", &[1.5]),
            ("Cut-Extrude7", &[0.1]),
        ],
    );
    assert_field(
        ir,
        &["pattern", "spacing"],
        &[
            ("LPattern1", 11.25),
            ("LPattern2", 1.75),
            ("LPattern3", 5.38),
            ("LPattern4", 2.25),
            ("LPattern5", 3.65),
            ("LPattern6", 3.65),
        ],
    );
}
