// SPDX-License-Identifier: Apache-2.0
//! D16: a configuration's definition override restates every dimension at a
//! place of the base definition with the base's value.

use serde_json::{json, Value};

use super::super::fixture::{design_findings, document, feature};
use crate::design::configuration::FeatureStateOverride;
use crate::design::feature::FeatureDefinition;
use crate::document::CadIr;
use crate::report::Severity;

const D16: &str = "a dimension difference is a parameter override";

fn offset_plane(offset: &Value, after: &Value) -> FeatureDefinition {
    serde_json::from_value(json!({"operation": {"kind": "datum", "value": {
        "element": {"kind": "plane", "value": {
            "inputs": [{
                "role": {"subject": "whole", "relation": {"kind": "offset"}},
                "reference": {"kind": "picks", "value": {
                    "at": {"kind": "input"},
                    "picks": {"kind": "named", "value": [
                        {"target": {"kind": "principal", "value": {"entity": "xy_plane"}}}
                    ]}
                }},
                "value": {"kind": "length", "value": offset}
            }],
            "after": after,
            "resolved_in": {}
        }},
        "native_options": {},
        "source_settings": {}
    }}}))
    .unwrap()
}

fn literal(value: f64) -> Value {
    json!({"kind": "literal", "value": value})
}

/// The fixture with its extrude a plane offset 10 from XY, and `large`
/// overriding its definition with `definition`.
fn overridden(definition: FeatureDefinition) -> CadIr {
    let mut ir = document();
    ir.design.features[0].definition = offset_plane(&literal(10.0), &json!([]));
    ir.design.configurations[1].overrides.features.insert(
        feature("extrude"),
        FeatureStateOverride {
            suppressed: None,
            definition: Some(definition),
        },
    );
    ir
}

fn d16(ir: &CadIr) -> bool {
    design_findings(ir)
        .iter()
        .any(|finding| finding.severity == Severity::Error && finding.message.contains(D16))
}

#[test]
fn a_definition_override_restates_the_base_dimensions() {
    assert!(!d16(&overridden(offset_plane(&literal(10.0), &json!([])))));
    assert!(!d16(&overridden(offset_plane(
        &literal(10.0 + 1e-12),
        &json!([])
    ))));
    assert!(d16(&overridden(offset_plane(&literal(12.0), &json!([])))));
    let bound = json!({"kind": "parameter", "value": "test:design:parameter#d"});
    assert!(d16(&overridden(offset_plane(&bound, &json!([])))));
}

#[test]
fn a_dimension_at_a_place_the_base_lacks_is_not_compared() {
    let step = json!([{"kind": "translate", "value": {"axis": "x", "distance": literal(5.0)}}]);
    assert!(!d16(&overridden(offset_plane(&literal(10.0), &step))));
}

#[test]
fn the_comparison_at_the_construction_ceiling_runs_on_a_small_stack() {
    let ceiling =
        usize::try_from(cadmpeg_core::decode::ResourceLimits::desktop().max_construction_depth)
            .unwrap();
    let mut inputs = json!([{
        "role": {"subject": "whole", "relation": {"kind": "offset"}},
        "reference": {"kind": "picks", "value": {
            "at": {"kind": "input"},
            "picks": {"kind": "named", "value": [
                {"target": {"kind": "principal", "value": {"entity": "xy_plane"}}}
            ]}
        }},
        "value": {"kind": "length", "value": literal(10.0)}
    }]);
    for _ in 1..ceiling {
        inputs = json!([{
            "role": {"subject": "whole", "relation": {"kind": "parallel"}},
            "reference": {"kind": "built", "value": {"kind": "plane", "inputs": inputs, "after": [], "resolved_in": {}}}
        }]);
    }
    let nested = |offset: f64| -> FeatureDefinition {
        let mut text = serde_json::to_string(&inputs).unwrap();
        text = text.replace("10.0", &format!("{offset:?}"));
        serde_json::from_value(json!({"operation": {"kind": "datum", "value": {
            "element": {"kind": "plane", "value": {
                "inputs": serde_json::from_str::<Value>(&text).unwrap(),
                "after": [],
                "resolved_in": {}
            }},
            "native_options": {},
            "source_settings": {}
        }}}))
        .unwrap()
    };
    let mut ir = overridden(nested(12.0));
    ir.design.features[0].definition = nested(10.0);
    let found = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || d16(&ir))
        .unwrap()
        .join()
        .unwrap();
    assert!(found);
}
