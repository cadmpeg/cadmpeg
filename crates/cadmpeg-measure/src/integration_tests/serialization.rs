// SPDX-License-Identifier: Apache-2.0
//! The serialized report: finite numbers and members in contract order.

use crate::select::Selection;
use crate::test_support::builders::{measure, plate_with_hole};

/// The keys of the top-level object of a JSON text, in text order.
fn keys(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut depth = 0_i32;
    let mut chars = text.chars().peekable();
    let mut string = String::new();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
                string.push(c);
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
                while chars.peek().is_some_and(|next| next.is_whitespace()) {
                    chars.next();
                }
                if depth == 1 && chars.peek() == Some(&':') {
                    keys.push(std::mem::take(&mut string));
                }
                string.clear();
            } else {
                string.push(c);
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            _ => {}
        }
    }
    keys
}

#[test]
fn a_face_report_serializes_members_in_contract_order() {
    let ir = plate_with_hole(true);
    let report = measure(
        &ir,
        &Selection {
            all_faces: true,
            ..Selection::default()
        },
    );
    let text = serde_json::to_string(&report).expect("finite report");
    assert_eq!(
        keys(&text),
        [
            "tolerances",
            "caveats",
            "bodies",
            "faces",
            "edges",
            "surfaces",
            "document"
        ]
    );
    let face = &report.faces()[0];
    assert_eq!(
        keys(&serde_json::to_string(face).unwrap()),
        [
            "face",
            "placement",
            "name",
            "body",
            "shell",
            "sense",
            "carrier",
            "measures",
            "orientation",
            "caveats",
            "status"
        ]
    );
    assert_eq!(
        keys(&serde_json::to_string(face.measures().area()).unwrap()),
        [
            "status",
            "value",
            "unit",
            "method",
            "numeric_error",
            "model_uncertainty"
        ]
    );
    let surfaces = measure(
        &ir,
        &Selection {
            surfaces: vec![ir.model.surfaces[0].id.clone()],
            ..Selection::default()
        },
    );
    assert_eq!(
        keys(&serde_json::to_string(&surfaces.surfaces()[0]).unwrap()),
        [
            "surface",
            "placement",
            "carrier",
            "bodies",
            "faces",
            "status"
        ]
    );
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let face = &value["faces"][0];
    assert_eq!(face["placement"], serde_json::json!({"space": "model"}));
    assert_eq!(face["carrier"]["kind"], "plane");
    assert_eq!(face["carrier"]["parameters"]["origin"]["method"], "source");
    assert_eq!(
        value["caveats"][0]["code"],
        "measure.provenance-not-recorded"
    );
}

/// A quantity that does not apply is absent, not `null`: a face report has
/// no document totals, and an occurrence space that places nothing has no
/// transform.
#[test]
fn quantities_that_do_not_apply_are_absent() {
    let ir = plate_with_hole(true);
    let faces = measure(
        &ir,
        &Selection {
            all_faces: true,
            ..Selection::default()
        },
    );
    let value = serde_json::to_value(&faces).unwrap();
    let document = value["document"].as_object().unwrap();
    for member in ["area", "closed_volume", "bbox"] {
        assert!(!document.contains_key(member), "{member}");
    }
    let bodies = serde_json::to_value(measure(&ir, &Selection::default())).unwrap();
    for member in ["area", "bbox"] {
        assert!(bodies["document"][member].is_object(), "{member}");
    }

    let mut assembly = ir.clone();
    let definition =
        cadmpeg_ir::ids::ProductDefinitionId::mint("test:model:product-definition#empty")
            .expect("identity");
    assembly
        .model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: definition.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: Vec::new(),
            native_ref: None,
        });
    let occurrence =
        cadmpeg_ir::ids::OccurrenceId::mint("test:model:occurrence#empty").expect("identity");
    assembly
        .model
        .occurrences
        .push(cadmpeg_ir::products::Occurrence {
            id: occurrence.clone(),
            prototype: cadmpeg_ir::products::PrototypeReference::Local { definition },
            parent: cadmpeg_ir::products::OccurrenceParent::Root {},
            ordinal: 0,
            transform: Some(cadmpeg_ir::transform::Transform::identity()),
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
    assembly.finalize();
    let unplaced = serde_json::to_value(measure(
        &assembly,
        &Selection {
            occurrence: Some(occurrence),
            ..Selection::default()
        },
    ))
    .unwrap();
    let block = unplaced["occurrence"].as_object().unwrap();
    assert!(!block.contains_key("transform"));
    assert!(!block.contains_key("descendants"));
    assert_eq!(block["unavailable"]["reason"], "placement.occurrence-empty");
}

/// Each mass property is a value object with a `value` member: the tensor
/// entries under `value`, and one value per principal moment and axis.
#[test]
fn mass_properties_are_value_objects() {
    let ir = crate::test_support::builders::cylinder_solid(10.0, 30.0);
    let report = measure(&ir, &Selection::default());
    let typed = report.bodies()[0]
        .second_moments()
        .as_ref()
        .expect("tensor");
    let body = &serde_json::to_value(&report).unwrap()["bodies"][0];
    let tensor = &body["second_moments"];
    assert_eq!(
        keys(&serde_json::to_string(typed).unwrap()),
        [
            "status",
            "value",
            "unit",
            "method",
            "numeric_error",
            "model_uncertainty"
        ]
    );
    for entry in ["xx", "yy", "zz", "xy", "yz", "zx"] {
        assert!(tensor["value"][entry].is_f64(), "{entry}");
    }
    let moments = body["principal_moments"].as_array().unwrap();
    assert_eq!(moments.len(), 3);
    for moment in moments {
        assert!(moment["value"].is_f64());
        assert_eq!(moment["unit"], "mm5");
    }
    let axes = body["principal_axes"].as_array().unwrap();
    assert_eq!(axes.len(), 3);
    assert_eq!(axes[0]["status"], "unavailable");
    assert_eq!(axes[1]["status"], "unavailable");
    assert!(axes[2]["value"]["z"].is_f64());
    assert_eq!(axes[2]["unit"], "1");
}
