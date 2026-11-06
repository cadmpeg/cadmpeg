// SPDX-License-Identifier: Apache-2.0
use serde_json::json;

use super::{
    base_binding_findings, base_ordinal_findings, base_property_loss_findings, snake, value_matches,
};

#[test]
fn snake_folds_layout_spellings() {
    assert_eq!(snake("EquationId"), "equation_id");
    assert_eq!(
        snake("native_parameter.affected_edge_ids"),
        "native_parameter_affected_edge_ids"
    );
    assert_eq!(snake("sw-configuration-name"), "sw_configuration_name");
    assert_eq!(snake("angle"), "angle");
}

#[test]
fn values_match_as_text_with_numbers_by_value() {
    assert!(value_matches(&json!(2.0), &json!("2")));
    assert!(!value_matches(&json!("YES"), &json!(true)));
    assert!(value_matches(&json!(true), &json!("True")));
    assert!(!value_matches(&json!(2.0), &json!("3")));
    assert!(value_matches(&json!("a"), &json!("a")));
}

#[test]
fn a_kept_map_entry_passes() {
    let base = json!({"model": {"features": [
        {"id": "f", "properties": {"k": "v"}, "source_properties": {"s": 1}},
    ]}});
    let head = json!({"model": {"features": [
        {"id": "f", "properties": {"k": "v"}, "source_properties": {"s": 1}},
    ]}});
    assert!(base_property_loss_findings("step", &base, &head).is_empty());
}

#[test]
fn an_entry_kept_in_the_other_map_passes() {
    let base = json!({"model": {"features": [{"id": "f", "properties": {"k": "v"}}]}});
    let head = json!({"model": {"features": [{"id": "f", "source_properties": {"k": "v"}}]}});
    assert!(base_property_loss_findings("step", &base, &head).is_empty());
}

#[test]
fn a_typed_field_holds_a_moved_entry() {
    let base = json!({"model": {"features": [{"id": "f", "properties": {"EquationId": "D1"}}]}});
    let head = json!({"model": {"features": [{"id": "f", "equation_id": "D1"}]}});
    assert!(base_property_loss_findings("step", &base, &head).is_empty());
}

#[test]
fn a_native_ref_record_holds_a_moved_entry() {
    let base = json!({"model": {"features": [
        {"id": "f", "native_ref": "n", "properties": {"unit": "mm"}},
    ]}});
    let head = json!({
        "model": {"features": [{"id": "f", "native_ref": "n"}]},
        "native": {"sldprt": {"rows": [{"id": "n", "unit": "mm"}]}},
    });
    assert!(base_property_loss_findings("step", &base, &head).is_empty());
}

#[test]
fn an_owned_native_record_holds_a_moved_entry() {
    let base = json!({"model": {"features": [{"id": "f", "properties": {"k": "v"}}]}});
    let head = json!({
        "model": {"features": [{"id": "f"}]},
        "native": {"fcstd": {"properties": [{"id": "n", "owner": "f", "name": "k", "value": "v"}]}},
    });
    assert!(base_property_loss_findings("fcstd", &base, &head).is_empty());
}

#[test]
fn freecad_native_property_requires_the_matching_owner_and_value() {
    let base = json!({"model": {"features": [
        {"id": "f", "source_properties": {"Visibility": "true"}},
    ]}});
    let mut head = json!({
        "model": {"features": [{"id": "f", "native_ref": "object"}]},
        "native": {"fcstd": {"properties": [{
            "id": "p", "owner": "object", "name": "Visibility",
            "values": [{"parent": null, "attributes": {"value": "true"}}]
        }]}},
    });
    assert!(base_property_loss_findings("fcstd", &base, &head).is_empty());
    head["native"]["fcstd"]["properties"][0]["owner"] = json!("other");
    assert_eq!(base_property_loss_findings("fcstd", &base, &head).len(), 1);
    head["native"]["fcstd"]["properties"][0]["owner"] = json!("object");
    head["native"]["fcstd"]["properties"][0]["values"][0]["attributes"]["value"] = json!("false");
    assert_eq!(base_property_loss_findings("fcstd", &base, &head).len(), 1);
}

#[test]
fn freecad_cell_properties_must_match_the_parameter_cell() {
    let base = json!({"model": {"parameters": [
        {"id": "parameter#Sheet:cell:B1", "properties": {"address": "B1", "alias": "width"}},
    ]}});
    let mut head = json!({
        "model": {"parameters": [{"id": "parameter#Sheet:cell:B1", "name": "width", "native_ref": "cells"}]},
        "native": {"fcstd": {"properties": [{"id": "cells", "name": "cells", "values": [
            {"tag": "Cell", "attributes": {"address": "B1", "alias": "width"}}
        ]}]}},
    });
    assert!(base_property_loss_findings("fcstd", &base, &head).is_empty());
    head["native"]["fcstd"]["properties"][0]["values"][0]["attributes"]["address"] = json!("B2");
    assert_eq!(base_property_loss_findings("fcstd", &base, &head).len(), 2);
}

#[test]
fn folded_freecad_constraint_keeps_its_exact_native_control() {
    let base = json!({"model": {"parameters": [{
        "id": "fcstd:design:parameter#Sketch:constraint:1", "native_ref": "constraints",
        "expression": "Sheet.width", "properties": {
            "is_driving": "1", "source_name": "width", "expression_native_ref": "engine"
        }
    }]}});
    let mut head = json!({
        "model": {"sketch_constraints": [{
            "id": "fcstd:design:sketch-constraint#Sketch:1", "native_ref": "constraints"
        }]},
        "native": {"fcstd": {"properties": [
            {"id": "constraints", "owner": "sketch", "name": "Constraints", "values": [
                {"tag": "Constrain", "attributes": {"Name": "width", "IsDriving": "1"}}
            ]},
            {"id": "engine", "owner": "sketch", "name": "ExpressionEngine", "values": [
                {"tag": "Expression", "attributes": {"path": ".Constraints.width", "expression": "Sheet.width"}}
            ]}
        ]}}
    });
    assert!(base_property_loss_findings("fcstd", &base, &head).is_empty());
    head["native"]["fcstd"]["properties"][1]["values"][0]["attributes"]["path"] =
        json!(".Constraints.height");
    assert_eq!(base_property_loss_findings("fcstd", &base, &head).len(), 1);
}

#[test]
fn freecad_empty_link_xml_requires_the_same_empty_native_list() {
    let base = json!({"model": {"features": [{"id": "f", "source_properties": {
        "Support": "<Property name='Support' type='App::PropertyLinkSubList'><LinkSubList count='0'/></Property>"
    }}]}});
    let mut head = json!({"model": {"features": [{"id": "f", "native_ref": "object"}]},
        "native": {"fcstd": {"properties": [{"id": "p", "owner": "object", "name": "Support", "type_name": "App::PropertyLinkSubList", "links": [],
            "values": [{"tag": "LinkSubList", "parent": null, "attributes": {"count": "0"}}]
        }]}}
    });
    assert!(base_property_loss_findings("fcstd", &base, &head).is_empty());
    head["native"]["fcstd"]["properties"][0]["values"][0]["attributes"]["count"] = json!("1");
    assert_eq!(base_property_loss_findings("fcstd", &base, &head).len(), 1);
}

#[test]
fn a_section_8_deletion_is_not_a_loss() {
    let base = json!({"model": {"features": [
        {"id": "f", "source_properties": {"view_bytes": "AA==", "kept": "1"}},
    ]}});
    let head = json!({"model": {"features": [{"id": "f", "source_properties": {"kept": "1"}}]}});
    assert!(base_property_loss_findings("rhino", &base, &head).is_empty());
    let findings = base_property_loss_findings("step", &base, &head);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("view_bytes"), "{findings:?}");
}

#[test]
fn a_changed_value_is_a_loss() {
    let base = json!({"model": {"features": [{"id": "f", "properties": {"k": "v"}}]}});
    let head = json!({"model": {"features": [{"id": "f", "properties": {"k": "w"}}]}});
    let findings = base_property_loss_findings("step", &base, &head);
    assert_eq!(findings.len(), 1, "{findings:?}");
}

#[test]
fn a_record_absent_at_head_loses_its_entries() {
    let base = json!({"model": {"features": [{"id": "f", "properties": {"k": "v"}}]}});
    let head = json!({"model": {"features": []}});
    let findings = base_property_loss_findings("step", &base, &head);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("absent at head"), "{findings:?}");
}

#[test]
fn an_nx_native_id_valued_entry_is_a_section_8_deletion() {
    let base = json!({
        "model": {"features": [
            {"id": "f", "source_properties": {"body_write.0": "nx:native:body#1"}},
        ]},
        "native": {"nx": {"bodies": [{"id": "nx:native:body#1"}]}},
    });
    let head = json!({"model": {"features": [{"id": "f"}]}});
    assert!(base_property_loss_findings("nx", &base, &head).is_empty());
    assert_eq!(base_property_loss_findings("step", &base, &head).len(), 1);
}

#[test]
fn a_sldprt_display_state_duplicate_is_a_section_8_deletion() {
    let base = json!({"model": {"configurations": [
        {"id": "c", "properties": {"DisplayState": "Shaded", "kept": "1"}},
    ]}});
    let head = json!({"model": {"configurations": [{"id": "c", "properties": {"kept": "1"}}]}});
    assert!(base_property_loss_findings("sldprt", &base, &head).is_empty());
    let findings = base_property_loss_findings("step", &base, &head);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("DisplayState"), "{findings:?}");
}

#[test]
fn an_owned_operation_slot_bound_at_base_must_not_go_literal() {
    let base = json!({"model": {
        "features": [{"id": "f", "definition": {"angle": -0.08}}],
        "parameters": [{"id": "p", "name": "Angle", "owner": "f"}],
    }});
    let head = json!({"model": {
        "features": [{"id": "f", "definition": {"angle": {"kind": "literal", "value": -0.08}}}],
        "parameters": [{"id": "p", "name": "Angle", "owner": "f"}],
    }});
    let findings = base_binding_findings(&base, &head);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("Angle (p)"), "{findings:?}");
}

#[test]
fn native_bookkeeping_slots_are_not_dimensions() {
    let base = json!({"model": {
        "features": [{"id": "f", "definition": {"kind": "X"}}],
        "parameters": [{"id": "p", "name": "result", "owner": "f"}],
    }});
    let head = json!({"model": {
        "features": [{"id": "f", "definition": {"kind": "X",
            "entity_id": {"kind": "literal", "value": {"tokens": [{"kind": "text", "value": "7"}]}}}}],
        "parameters": [{"id": "p", "name": "result", "owner": "f"}],
    }});
    assert!(base_binding_findings(&base, &head).is_empty());
}

#[test]
fn a_binding_lost_at_its_own_path_fails() {
    let base = json!({"model": {
        "features": [{"id": "f", "definition": {
            "length": {"kind": "parameter", "value": "p"}}}],
        "parameters": [{"id": "p", "name": "D1"}],
    }});
    let head = json!({"model": {
        "features": [{"id": "f", "definition": {
            "length": {"kind": "literal", "value": 2.0}}}],
        "parameters": [{"id": "p", "name": "D1"}],
    }});
    let findings = base_binding_findings(&base, &head);
    assert_eq!(findings.len(), 1, "{findings:?}");
}

#[test]
fn a_sketch_constraint_naming_its_parameter_fails_when_literal() {
    let base = json!({"model": {
        "sketch_constraints": [{"id": "c", "parameter": "p"}],
        "parameters": [{"id": "p", "name": "D1"}],
    }});
    let head = json!({"model": {
        "sketch_constraints": [{"id": "c", "parameter": "p",
            "distance": {"kind": "literal", "value": 2.0}}],
        "parameters": [{"id": "p", "name": "D1"}],
    }});
    assert_eq!(base_binding_findings(&base, &head).len(), 1);
}

#[test]
fn an_unassociated_literal_passes() {
    let base = json!({"model": {
        "features": [{"id": "f", "definition": {"thickness": 2.0}}],
        "parameters": [{"id": "p", "name": "Other", "owner": "g"}],
    }});
    let head = json!({"model": {
        "features": [{"id": "f", "definition": {"thickness": {"kind": "literal", "value": 2.0}}}],
        "parameters": [{"id": "p", "name": "Other", "owner": "g"}],
    }});
    assert!(base_binding_findings(&base, &head).is_empty());
}

#[test]
fn a_kept_ordinal_order_passes() {
    let base = json!({"model": {"features": [
        {"id": "a", "ordinal": 2}, {"id": "b", "ordinal": 0}, {"id": "c", "ordinal": 1},
    ]}});
    let head = json!({"model": {"features": [
        {"id": "a", "ordinal": 5}, {"id": "b", "ordinal": 3}, {"id": "c", "ordinal": 4},
    ]}});
    assert!(base_ordinal_findings(&base, &head).is_empty());
}

#[test]
fn an_inverted_ordinal_order_fails() {
    let base = json!({"model": {"features": [
        {"id": "a", "ordinal": 0}, {"id": "b", "ordinal": 1},
    ]}});
    let head = json!({"model": {"features": [
        {"id": "a", "ordinal": 1}, {"id": "b", "ordinal": 0},
    ]}});
    let findings = base_ordinal_findings(&base, &head);
    assert_eq!(findings.len(), 1, "{findings:?}");
}

#[test]
fn features_absent_at_either_side_carry_no_order() {
    let base = json!({"model": {"features": [{"id": "a", "ordinal": 0}]}});
    let head = json!({"model": {"features": [{"id": "b", "ordinal": 0}]}});
    assert!(base_ordinal_findings(&base, &head).is_empty());
    let no_ordinals = json!({"model": {"features": [{"id": "a"}, {"id": "b"}]}});
    assert!(base_ordinal_findings(&no_ordinals, &no_ordinals).is_empty());
}

#[test]
fn freecad_support_link_keeps_the_base_object_even_with_subelements() {
    let base =
        json!({"model": {"features": [{"id": "f", "source_properties": {"Support": "target"}}]}});
    let mut head = json!({"model": {"features": [{"id": "f", "native_ref": "owner"}]}, "native": {"fcstd": {"properties": [{"id": "p", "owner": "owner", "name": "Support", "links": [{"document": null, "object": "target", "subelements": ["Vertex1"]}]}]}}});
    assert!(base_property_loss_findings("fcstd", &base, &head).is_empty());
    head["native"]["fcstd"]["properties"][0]["links"][0]["object"] = json!("other");
    assert_eq!(base_property_loss_findings("fcstd", &base, &head).len(), 1);
    head["native"]["fcstd"]["properties"][0]["links"][0]["object"] = json!("target");
    head["native"]["fcstd"]["properties"][0]["links"][0]["document"] = json!("external");
    assert_eq!(base_property_loss_findings("fcstd", &base, &head).len(), 1);
}
