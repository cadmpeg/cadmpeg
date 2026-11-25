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
    optioned_plane(offset, after, &json!({}))
}

fn optioned_plane(offset: &Value, after: &Value, options: &Value) -> FeatureDefinition {
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
        "native_options": options,
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
    overridden_from(offset_plane(&literal(10.0), &json!([])), definition)
}

/// The fixture with its extrude defined by `base`, and `large` overriding
/// its definition with `definition`.
fn overridden_from(base: FeatureDefinition, definition: FeatureDefinition) -> CadIr {
    let mut ir = document();
    ir.design.features[0].definition = base;
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
    !d16_messages(ir).is_empty()
}

fn d16_messages(ir: &CadIr) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| finding.severity == Severity::Error && finding.message.contains(D16))
        .map(|finding| finding.message)
        .collect()
}

/// A native option map of one integer and one length option.
fn options(count: i64, depth: &Value) -> Value {
    json!({
        "test:count": {"kind": "integer", "value": {"kind": "literal", "value": count}},
        "test:depth": depth
    })
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
fn the_comparison_of_the_deepest_construction_runs_on_a_small_stack() {
    // A nested construction is four levels of JSON: inputs, input, reference
    // and construction, so no document holds a deeper nesting.
    let deepest = crate::bounds::MAX_DOCUMENT_DEPTH / 4;
    let nested = |offset: f64| -> FeatureDefinition {
        let mut inputs = json!([{
            "role": {"subject": "whole", "relation": {"kind": "offset"}},
            "reference": {"kind": "picks", "value": {
                "at": {"kind": "input"},
                "picks": {"kind": "named", "value": [
                    {"target": {"kind": "principal", "value": {"entity": "xy_plane"}}}
                ]}
            }},
            "value": {"kind": "length", "value": literal(offset)}
        }]);
        for _ in 1..deepest {
            inputs = json!([{
                "role": {"subject": "whole", "relation": {"kind": "parallel"}},
                "reference": {"kind": "built", "value": {"kind": "plane", "inputs": inputs, "after": [], "resolved_in": {}}}
            }]);
        }
        serde_json::from_value(json!({"operation": {"kind": "datum", "value": {
            "element": {"kind": "plane", "value": {
                "inputs": inputs,
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

#[test]
fn the_place_names_fields_tags_and_indices() {
    let messages = d16_messages(&overridden(offset_plane(&literal(12.0), &json!([]))));
    assert_eq!(messages.len(), 1, "{messages:#?}");
    assert!(
        messages[0].contains(
            "at operation.datum.value.element.plane.value.inputs.0.value.length.value than"
        ),
        "{messages:#?}"
    );
}

#[test]
fn a_map_entry_is_placed_under_its_key() {
    let length = |value: f64| json!({"kind": "length", "value": literal(value)});
    let plane = |count: i64, depth: f64| {
        optioned_plane(&literal(10.0), &json!([]), &options(count, &length(depth)))
    };
    assert!(!d16(&overridden_from(plane(3, 1.0), plane(3, 1.0))));
    let messages = d16_messages(&overridden_from(plane(3, 1.0), plane(4, 1.0)));
    assert_eq!(messages.len(), 1, "{messages:#?}");
    assert!(
        messages[0]
            .contains("at operation.datum.value.native_options.test:count.integer.value than"),
        "{messages:#?}"
    );
    let messages = d16_messages(&overridden_from(plane(3, 1.0), plane(3, 2.0)));
    assert_eq!(messages.len(), 1, "{messages:#?}");
    assert!(
        messages[0]
            .contains("at operation.datum.value.native_options.test:depth.length.value than"),
        "{messages:#?}"
    );
}

#[test]
fn a_dimension_under_another_tag_is_not_compared() {
    let base = optioned_plane(
        &literal(10.0),
        &json!([]),
        &options(3, &json!({"kind": "length", "value": literal(1.0)})),
    );
    let real = optioned_plane(
        &literal(10.0),
        &json!([]),
        &options(3, &json!({"kind": "real", "value": literal(2.0)})),
    );
    assert!(!d16(&overridden_from(base, real)));
}
