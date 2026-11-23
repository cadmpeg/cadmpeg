// SPDX-License-Identifier: Apache-2.0
//! A configuration's materials pick bodies of the scope's definitions, and
//! no two of them select one body.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::super::fixture::{assert_finding, design_findings, document, id};
use crate::design::configuration::MaterialOverride;
use crate::design::names::{NameEntry, NameTable, TopoName};
use crate::design::refs::EntityRef;
use crate::document::CadIr;
use crate::report::Severity;

fn material(pick: &Value) -> MaterialOverride {
    serde_json::from_value(json!({
        "target": {"at": {"kind": "current"}, "picks": {"kind": "named", "value": [pick]}},
        "material": {"kind": "named", "value": "steel"}
    }))
    .unwrap()
}

fn root() -> TopoName {
    TopoName::Root {
        container: id("container", "timeline"),
    }
}

fn body_pick(ir: &CadIr) -> Value {
    json!({
        "target": {"kind": "entity", "value": serde_json::to_value(root()).unwrap()},
        "resolved": [{"kind": "body", "value": ir.geometry.bodies[0].id.as_str()}]
    })
}

/// The fixture with its body named by the timeline.
fn named() -> CadIr {
    let mut ir = document();
    ir.design.names = NameTable::from(BTreeMap::from([(
        EntityRef::Body(ir.geometry.bodies[0].id.clone()),
        NameEntry {
            name: Some(root()),
            native: None,
        },
    )]));
    ir
}

fn overlap(ir: &CadIr) -> bool {
    design_findings(ir).iter().any(|finding| {
        finding.severity == Severity::Error
            && finding.message.contains("no two entries select one body")
    })
}

#[test]
fn two_materials_that_select_one_body_are_an_error() {
    let mut ir = named();
    let pick = body_pick(&ir);
    ir.design.configurations[1].materials = vec![material(&pick)];
    assert!(!overlap(&ir), "{:#?}", design_findings(&ir));
    ir.design.configurations[1].materials.push(material(&pick));
    assert!(overlap(&ir), "{:#?}", design_findings(&ir));
}

#[test]
fn names_select_bodies_only_in_the_active_configuration() {
    let name =
        json!({"target": {"kind": "entity", "value": serde_json::to_value(root()).unwrap()}});
    let mut ir = named();
    ir.design.configurations[0].materials = vec![material(&name), material(&name)];
    assert!(overlap(&ir), "{:#?}", design_findings(&ir));
    let mut ir = named();
    ir.design.configurations[1].materials = vec![material(&name), material(&name)];
    assert!(!overlap(&ir), "{:#?}", design_findings(&ir));
}

#[test]
fn a_material_target_picks_bodies() {
    let mut ir = named();
    let mut face = json!({"target": {"kind": "entity", "value": {"kind": "generated", "value": {
        "kind": "face", "feature": "test:design:feature#extrude"
    }}}});
    face["resolved"] = json!([{"kind": "face", "value": ir.geometry.faces[0].id.as_str()}]);
    ir.design.configurations[1].materials = vec![material(&face)];
    let errors: Vec<_> = design_findings(&ir)
        .into_iter()
        .filter(|finding| {
            finding.severity == Severity::Error
                && (finding
                    .message
                    .contains("picks something other than a body")
                    || finding.message.contains("which is no body"))
        })
        .collect();
    assert_eq!(errors.len(), 1, "{errors:#?}");

    let mut ir = named();
    let mut pick = body_pick(&ir);
    pick["resolved"] = json!([{"kind": "face", "value": ir.geometry.faces[0].id.as_str()}]);
    ir.design.configurations[1].materials = vec![material(&pick)];
    assert_finding(&ir, "which is no body");

    let mut ir = named();
    let datum = json!({"target": {"kind": "principal", "value": {"entity": "xy_plane"}}});
    ir.design.configurations[1].materials = vec![material(&datum)];
    assert_finding(&ir, "picks something other than a body");
}

#[test]
fn a_material_target_picks_in_a_definition_the_scope_spans() {
    let mut ir = named();
    let pick = json!({
        "target": {"kind": "entity", "value": serde_json::to_value(root()).unwrap()},
        "context": {"kind": "definition", "value": {"definition": "test:design:definition#elsewhere"}}
    });
    ir.design.configurations[1].materials = vec![material(&pick)];
    assert_finding(&ir, "which is no definition the scope spans");
}
