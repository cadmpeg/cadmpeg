// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::document::{Model, SourceMeta};
use crate::examples::unit_cube;
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

/// A `SourceMeta` written before dialect layers existed still reads with them
/// absent. Writing it back now states that absence explicitly,
/// which is what moves every document digest over a document that has source
/// metadata.
#[test]
fn pre_migration_source_metadata_reads_back_and_gains_the_dialect_keys() {
    let stored = "{\"format\":\"rhino\",\"attributes\":{\"object_count\":\"3\"}}";
    let source: SourceMeta = serde_json::from_str(stored).unwrap();

    assert_eq!(source.format(), "rhino");
    assert_eq!(source.dialect(), None);

    let rewritten = serde_json::to_string(&source).unwrap();
    assert_eq!(
        rewritten,
        "{\"format\":\"rhino\",\"attributes\":{\"object_count\":\"3\"},\
         \"dialects\":null}"
    );
    assert_eq!(
        serde_json::from_str::<SourceMeta>(&rewritten).unwrap(),
        source
    );
}

#[test]
fn classified_source_metadata_has_one_format_and_rejects_a_foreign_wire_match() {
    let matched = cadmpeg_core::dialect::DialectMatch::admitted(
        cadmpeg_core::dialect::DialectId::pinned("rhino:archive-80"),
    );
    let layers = cadmpeg_core::dialect::DialectLayers::of(matched.clone());
    let source = SourceMeta::classified(
        layers.clone(),
        std::collections::BTreeMap::from([("object_count".into(), "3".into())]),
    );

    assert_eq!(source.format(), "rhino");
    assert_eq!(source.dialect(), Some(&matched));
    assert_eq!(source.dialects(), Some(&layers));
    let rendered = serde_json::to_string(&source).unwrap();
    assert_eq!(
        rendered,
        "{\"format\":\"rhino\",\"attributes\":{\"object_count\":\"3\"},\"dialects\":{\"primary\":{\"format\":\"rhino\",\"dialect\":\"rhino:archive-80\",\"admission\":\"admitted\"},\"extra\":[]}}"
    );
    assert_eq!(
        serde_json::from_str::<SourceMeta>(&rendered).unwrap(),
        source
    );

    let malformed = rendered.replacen("\"format\":\"rhino\"", "\"format\":\"step\"", 1);
    let error = serde_json::from_str::<SourceMeta>(&malformed)
        .expect_err("a source format must match its dialect format");
    assert!(
        error
            .to_string()
            .contains("format \"step\" does not match classified payload format \"rhino\""),
        "{error}"
    );
}

#[test]
fn legacy_singular_source_dialect_migrates_to_current_layers() {
    let stored = "{\"format\":\"rhino\",\"attributes\":{},\"dialect\":{\"format\":\"rhino\",\"dialect\":\"rhino:archive-80\",\"admission\":\"admitted\"}}";
    let source: SourceMeta = serde_json::from_str(stored).unwrap();

    assert_eq!(
        source.dialect().unwrap().dialect().as_str(),
        "rhino:archive-80"
    );
    let rewritten = serde_json::to_string(&source).unwrap();
    let current: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
    assert!(current.get("dialects").is_some(), "{rewritten}");
    assert!(current.get("dialect").is_none(), "{rewritten}");
}

#[test]
fn source_metadata_rejects_current_and_legacy_identity_together() {
    let stored = "{\"format\":\"rhino\",\"attributes\":{},\"dialects\":{\"primary\":{\"format\":\"rhino\",\"dialect\":\"rhino:archive-80\",\"admission\":\"admitted\"},\"extra\":[]},\"dialect\":{\"format\":\"rhino\",\"dialect\":\"rhino:archive-80\",\"admission\":\"admitted\"}}";
    let error = serde_json::from_str::<SourceMeta>(stored).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cannot contain both dialects and legacy dialect fields"),
        "{error}"
    );
}

#[cfg(feature = "schema")]
#[test]
fn current_source_metadata_schema_requires_dialects_and_omits_legacy_dialect() {
    let schema = serde_json::to_value(schemars::schema_for!(SourceMeta)).unwrap();
    let required = schema["required"].as_array().unwrap();

    assert!(
        required.iter().any(|field| field == "dialects"),
        "{schema:#}"
    );
    assert!(schema["properties"].get("dialect").is_none(), "{schema:#}");
}
