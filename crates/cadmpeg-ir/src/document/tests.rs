// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::document::{EntityRewrite, Model, SourceMeta};
use crate::examples::unit_cube;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use crate::math::{Point3, Vector3};
use crate::validate::validate_neutral;
use crate::{diff, CadIr};
use serde::{de::DeserializeOwned, Serialize};

struct SerdeIdentity;

impl EntityRewrite for SerdeIdentity {
    type Error = serde_json::Error;

    fn rewrite<T: Serialize + DeserializeOwned>(&mut self, entity: T) -> Result<T, Self::Error> {
        serde_json::from_value(serde_json::to_value(entity)?)
    }
}

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
        .map(|arena| arena.kind.to_string())
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

/// The structural edge is on the wire once, inside the owning tree node's
/// ordered children, and a tree child states no regeneration parent.
#[test]
fn feature_parent_wire_is_derived_from_its_single_owner() {
    use crate::features::{
        Feature, FeatureDefinition, FeatureId, FeatureOperation, FeatureTreeNodeRole,
    };

    let parent_id = FeatureId::mint("test:model:feature#parent").expect("identity grammar");
    let child_id = FeatureId::mint("test:model:feature#child").expect("identity grammar");
    let parent = Feature::new(
        parent_id.clone(),
        0,
        FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: crate::features::TreeChildren::new(
                vec![child_id.clone()],
                Some(child_id.clone()),
            )
            .unwrap(),
        }),
    );
    let child = Feature::new(
        child_id.clone(),
        1,
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
    );
    let model = Model {
        features: vec![parent, child],
        ..Model::default()
    };

    let value = serde_json::to_value(&model).unwrap();
    assert!(value["features"][1].get("parent").is_none());
    assert!(value["features"][1].get("regeneration_parent").is_none());
    assert_eq!(
        value["features"][0]["definition"]["children"]["children"][0],
        child_id.as_str()
    );
    assert_eq!(model.feature_parent(&child_id), Some(&parent_id));
    assert_eq!(serde_json::from_value::<Model>(value).unwrap(), model);

    let mut regeneration = Model {
        features: vec![
            Feature::new(
                parent_id.clone(),
                0,
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
            Feature::new(
                child_id.clone(),
                1,
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
        ],
        ..Model::default()
    };
    regeneration
        .set_feature_regeneration_parent(child_id, parent_id.clone())
        .unwrap();
    let value = serde_json::to_value(&regeneration).unwrap();
    assert_eq!(
        value["features"][1]["regeneration_parent"],
        parent_id.as_str()
    );
    assert_eq!(
        serde_json::from_value::<Model>(value).unwrap(),
        regeneration
    );
}

/// The deleted `parent` key is refused at the level it was deleted from.
#[test]
fn feature_wire_refuses_the_deleted_parent_key() {
    let wire = serde_json::json!({
        "features": [
            {
                "id": "test:model:feature#parent",
                "ordinal": 0,
                "definition": {
                    "definition": "tree_node",
                    "role": "history",
                    "children": {"children": ["test:model:feature#child"], "active_child": "test:model:feature#child"}
                }
            },
            {
                "id": "test:model:feature#child",
                "ordinal": 1,
                "parent": "test:model:feature#parent",
                "definition": {"definition": "stored_geometry"}
            }
        ]
    });
    let error = serde_json::from_value::<Model>(wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field `parent`"), "{error}");
}

/// A tree child cannot also state a regeneration predecessor.
#[test]
fn feature_parent_wire_rejects_disagreement_with_tree_children() {
    use crate::features::{
        Feature, FeatureDefinition, FeatureId, FeatureOperation, FeatureTreeNodeRole,
    };

    let first_id = FeatureId::mint("test:model:feature#first").expect("identity grammar");
    let second_id = FeatureId::mint("test:model:feature#second").expect("identity grammar");
    let child_id = FeatureId::mint("test:model:feature#child").expect("identity grammar");
    let model = Model {
        features: vec![
            Feature::new(
                first_id,
                0,
                FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    role: FeatureTreeNodeRole::History,
                    children: crate::features::TreeChildren::new(vec![child_id.clone()], None)
                        .unwrap(),
                }),
            ),
            Feature::new(
                second_id.clone(),
                1,
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
            Feature::new(
                child_id,
                2,
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
        ],
        ..Model::default()
    };
    let mut value = serde_json::to_value(model).unwrap();
    value["features"][2]["regeneration_parent"] =
        serde_json::Value::String(second_id.into_string());

    let error = serde_json::from_value::<Model>(value)
        .unwrap_err()
        .to_string();
    assert!(error.contains("states no regeneration parent"), "{error}");
}

#[test]
fn procedural_carrier_ownership_preserves_the_flat_cadir_wire() {
    let mut ir = CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#cache").expect("valid identity");
    let surface_construction =
        ProceduralSurfaceId::mint("test:model:surface-construction#cache").expect("valid identity");
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            crate::geometry::PlaneSurface::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model
        .add_procedural_surface(
            surface.clone(),
            ProceduralSurface::new(
                surface_construction,
                ProceduralSurfaceDefinition::Unknown { record: None },
                None,
            )
            .unwrap(),
        )
        .unwrap();

    let curve = CurveId::mint("test:model:curve#direct").expect("valid identity");
    let curve_construction =
        ProceduralCurveId::mint("test:model:curve-construction#direct").expect("valid identity");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: curve_construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model
        .add_procedural_curve(
            curve.clone(),
            ProceduralCurve::new(curve_construction, ProceduralCurveDefinition::Exact).unwrap(),
        )
        .unwrap();

    let value = serde_json::to_value(&ir).unwrap();
    let model = value["model"].as_object().unwrap();
    assert_eq!(model["surfaces"][0]["geometry"]["kind"], "plane");
    assert!(model["surfaces"][0]["geometry"].get("cache").is_none());
    assert_eq!(model["procedural_surfaces"][0]["surface"], surface.as_str());
    assert_eq!(model["curves"][0]["geometry"]["kind"], "procedural");
    assert!(model["curves"][0]["geometry"].get("cache").is_none());
    assert_eq!(model["procedural_curves"][0]["curve"], curve.as_str());
    assert_eq!(serde_json::from_value::<CadIr>(value).unwrap(), ir);

    let mut rewritten = Model::default();
    rewritten
        .extend_rewritten(ir.model, &mut SerdeIdentity)
        .unwrap();
    assert!(rewritten.surfaces[0].geometry.solved_cache().is_some());
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
    let expected_version = crate::IR_VERSION;
    for (version, found) in [
        (Some(serde_json::Value::String("0".into())), "Some(\"0\")"),
        (None, "None"),
        (Some(serde_json::Value::Number(1.into())), "None"),
    ] {
        let expected = format!("unsupported ir_version {found}; expected {expected_version}");
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
        assert!(error.to_string().contains(&expected), "{error}");
        let direct = serde_json::from_str::<CadIr>(&json).unwrap_err();
        assert!(direct.to_string().contains(&expected), "{direct}");
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
    let ir = CadIr::empty();
    let json = serde_json::to_value(&ir).unwrap();

    assert_eq!(json["ir_version"], crate::IR_VERSION);
    assert!(json.get("byte_ledger").is_none());
}

/// The format identity is one tagged object, so an unclassified source states
/// its format once and a classified one never restates its payload's format.
#[test]
fn an_unclassified_source_states_its_format_inside_the_identity() {
    let stored = "{\"identity\":{\"classification\":\"unclassified\",\"format\":\"rhino\"},\
                  \"attributes\":{\"object_count\":\"3\"}}";
    let source: SourceMeta = serde_json::from_str(stored).unwrap();

    assert_eq!(source.format(), "rhino");
    assert_eq!(source.dialect(), None);

    let rewritten = serde_json::to_string(&source).unwrap();
    assert_eq!(
        rewritten,
        "{\"identity\":{\"classification\":\"unclassified\",\"format\":\"rhino\"},\
         \"attributes\":{\"object_count\":\"3\"}}"
    );
    assert_eq!(
        serde_json::from_str::<SourceMeta>(&rewritten).unwrap(),
        source
    );
}

#[test]
fn a_classified_source_carries_its_format_once() {
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
    let rendered = serde_json::to_value(&source).unwrap();
    assert_eq!(rendered["identity"]["classification"], "classified");
    assert!(rendered["identity"].get("format").is_none());
    assert_eq!(
        rendered["identity"]["dialects"]["primary"]["format"],
        "rhino"
    );
    assert_eq!(
        serde_json::from_value::<SourceMeta>(rendered.clone()).unwrap(),
        source
    );

    let mut restated = rendered;
    restated["identity"]["format"] = serde_json::json!("step");
    let error = serde_json::from_value::<SourceMeta>(restated)
        .expect_err("a classified identity carries no second format");
    assert!(error.to_string().contains("format"), "{error}");
}

#[cfg(feature = "schema")]
#[test]
fn source_metadata_schema_requires_its_identity_and_has_no_singular_dialect() {
    let schema = serde_json::to_value(schemars::schema_for!(SourceMeta)).unwrap();
    let required = schema["required"].as_array().unwrap();

    assert!(
        required.iter().any(|field| field == "identity"),
        "{schema:#}"
    );
    assert!(schema["properties"].get("dialect").is_none(), "{schema:#}");
}

#[test]
fn parent_only_wire_preserves_regeneration_without_tree_membership() {
    use crate::features::{
        Feature, FeatureDefinition, FeatureId, FeatureOperation, FeatureTreeNodeRole,
    };

    let parent_id = FeatureId::mint("test:model:feature#parent").expect("identity grammar");
    let child_id = FeatureId::mint("test:model:feature#child").expect("identity grammar");
    let mut model = Model {
        features: vec![
            Feature::new(
                parent_id.clone(),
                0,
                FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    role: FeatureTreeNodeRole::SolidBodies,
                    children: crate::features::TreeChildren::default(),
                }),
            ),
            Feature::new(
                child_id.clone(),
                1,
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
        ],
        ..Model::default()
    };
    model
        .set_feature_regeneration_parent(child_id.clone(), parent_id.clone())
        .unwrap();
    assert_eq!(model.feature_tree_parent(&child_id), None);
    assert_eq!(model.feature_parent(&child_id), Some(&parent_id));
    let wire = serde_json::to_value(&model).unwrap();
    assert!(wire["features"][0]["definition"].get("children").is_none());
    assert_eq!(
        wire["features"][1]["regeneration_parent"],
        parent_id.as_str()
    );
    assert!(wire["features"][1].get("parent").is_none());
    assert_eq!(
        serde_json::from_value::<Model>(wire.clone()).unwrap(),
        model
    );

    // The structural edge is stated once, by the owning tree node's children.
    let mut owned = wire;
    owned["features"][0]["definition"]["children"]["children"] =
        serde_json::json!([child_id.as_str()]);
    let error = serde_json::from_value::<Model>(owned)
        .unwrap_err()
        .to_string();
    assert!(error.contains("states no regeneration parent"), "{error}");
}
