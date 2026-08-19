// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::document::Model;
use crate::examples::unit_cube;
use crate::report::Check;
use crate::validate::validate_neutral;
use crate::{diff, CadIr};

#[test]
fn entity_schema_registry_covers_arenas_and_unit_cube_references_resolve() {
    fn collect_ids(value: &serde_json::Value, ids: &mut std::collections::HashSet<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(serde_json::Value::String(id)) = fields.get("id") {
                    ids.insert(id.clone());
                }
                for value in fields.values() {
                    collect_ids(value, ids);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    collect_ids(value, ids);
                }
            }
            _ => {}
        }
    }

    assert_eq!(
        crate::schema::EntityKind::ALL.len(),
        Model::arena_names().len()
    );
    let ir = unit_cube();
    let mut ids = std::collections::HashSet::new();
    collect_ids(&serde_json::to_value(&ir.model).unwrap(), &mut ids);
    let mut missing = Vec::new();
    ir.model.visit_references(&mut |reference| {
        if !ids.contains(&reference.target) {
            missing.push(reference.target);
        }
    });
    assert!(missing.is_empty(), "unresolved references: {missing:?}");
}

#[test]
fn arena_registry_drives_counts_and_diff_dispatch() {
    let ir = unit_cube();
    let report = validate_neutral(&ir, Vec::new());
    let diff_kinds = diff(&ir, &ir)
        .per_arena
        .into_iter()
        .map(|arena| arena.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        &diff_kinds[..Model::arena_names().len()],
        Model::arena_names()
    );
    for name in Model::arena_names() {
        assert!(
            report.entity_counts.contains_key(*name),
            "entity counts omitted registered arena {name}"
        );
    }
}

#[test]
fn current_json_without_configurations_defaults_to_empty() {
    let ir = unit_cube();
    let mut value = serde_json::to_value(&ir).unwrap();
    value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .remove("configurations");

    let decoded: CadIr = serde_json::from_value(value).unwrap();
    assert!(decoded.model.configurations.is_empty());
}

#[test]
fn current_json_without_parameters_defaults_to_empty() {
    let ir = unit_cube();
    let mut value = serde_json::to_value(&ir).unwrap();
    value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .remove("parameters");

    let decoded: CadIr = serde_json::from_value(value).unwrap();
    assert!(decoded.model.parameters.is_empty());
}

#[test]
fn current_json_without_sketch_arenas_defaults_to_empty() {
    let ir = unit_cube();
    let mut value = serde_json::to_value(&ir).unwrap();
    let model = value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap();
    model.remove("sketches");
    model.remove("sketch_entities");
    model.remove("sketch_constraints");

    let decoded: CadIr = serde_json::from_value(value).unwrap();
    assert!(decoded.model.sketches.is_empty());
    assert!(decoded.model.sketch_entities.is_empty());
    assert!(decoded.model.sketch_constraints.is_empty());
}

#[test]
fn json_round_trips_and_is_deterministic() {
    let ir = unit_cube();
    let json1 = ir.to_canonical_json().unwrap();
    let json2 = ir.to_canonical_json().unwrap();
    assert_eq!(json1, json2, "serialization must be deterministic");

    let parsed = crate::CadIr::from_json(&json1).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the document");
    assert_eq!(parsed.to_canonical_json().unwrap(), json1);
}

#[test]
fn json_round_trip_preserves_ulp_edge_scalars_exactly() {
    // Byte-backed writers compare parsed documents against fresh decodes with
    // exact f64 equality, so JSON parsing must be correctly rounded. The
    // values one to a few ULPs below 1.0 are the ones a fast non-roundtrip
    // float parser misparses by one ULP.
    let mut ir = unit_cube();
    let edge_values: Vec<f64> = (1..40)
        .map(|n| 1.0f64 - f64::from(n) * f64::EPSILON / 2.0)
        .collect();
    for (point, value) in ir.model.points.iter_mut().zip(edge_values.iter().cycle()) {
        point.position.x = *value;
    }
    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    for (before, after) in ir.model.points.iter().zip(&parsed.model.points) {
        assert_eq!(
            before.position.x.to_bits(),
            after.position.x.to_bits(),
            "JSON round-trip changed {} by at least one ULP",
            before.position.x
        );
    }
}

#[test]
fn wrong_document_version_is_flagged() {
    let mut ir = unit_cube();
    ir.set_ir_version_for_test("1");
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Version));
}

#[test]
fn parser_rejects_unsupported_missing_and_non_string_versions() {
    let canonical = serde_json::to_value(unit_cube()).unwrap();
    for version in [
        Some(serde_json::Value::String("0".into())),
        None,
        Some(serde_json::Value::Number(1.into())),
    ] {
        let mut value = canonical.clone();
        let object = value.as_object_mut().unwrap();
        match version {
            Some(version) => {
                object.insert("ir_version".into(), version);
            }
            None => {
                object.remove("ir_version");
            }
        }
        let json = serde_json::to_string(&value).unwrap();
        let error = CadIr::from_json(&json).unwrap_err();
        assert!(!error.is_syntax());
        assert!(error.to_string().contains("unsupported ir_version"));
        assert!(serde_json::from_str::<CadIr>(&json).is_err());
    }
}

#[test]
fn direct_deserialization_accepts_current_version_and_canonical_round_trip() {
    let ir = unit_cube();
    let json = ir.to_canonical_json().unwrap();
    let parsed = serde_json::from_str::<CadIr>(&json).unwrap();
    assert_eq!(parsed, ir);
    assert_eq!(parsed.to_canonical_json().unwrap(), json);
}

#[test]
fn parser_distinguishes_malformed_json_from_version_rejection() {
    let error = CadIr::from_json("{\"ir_version\":\"1\"").unwrap_err();
    assert!(error.is_syntax() || error.is_eof());
    assert!(!error.to_string().contains("unsupported ir_version"));
}

#[test]
fn current_document_excludes_source_byte_accounting() {
    let ir = CadIr::empty(crate::units::Units::default());
    let json = serde_json::to_value(&ir).unwrap();

    assert_eq!(json["ir_version"], crate::IR_VERSION);
    assert!(json.get("byte_ledger").is_none());
}
