// SPDX-License-Identifier: Apache-2.0
//! Top-level and report/document schema checks.

use std::collections::BTreeMap;

use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;

use cadmpeg_ir::annotations::{FieldName, NonEmptyMap};
use cadmpeg_ir::assets::AssetData;
use cadmpeg_ir::container::ContainerSummary;
use cadmpeg_ir::document::{Model, SourceMeta, Topology};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::tree::{FeatureTreeNodeRole, TreeChildren};
use cadmpeg_ir::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation,
};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::{BodyId, HistoricalBodyId, Identity};
use cadmpeg_ir::report::decode::DecodeReport;
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::{
    cadir_json_schema, CadIr, Exactness, SourceObjectAssociation, StreamName, IR_VERSION,
};

#[test]
fn exactness_schemas_preserve_field_and_population_admission() {
    let schema = json::to_value(schemars::schema_for!(NonEmptyMap)).unwrap();
    assert_eq!(schema["minProperties"], 1);
    assert_eq!(schema["additionalProperties"], false);
    assert!(schema["patternProperties"].get("[\\s\\S]").is_some());
    let schema = json::to_value(schemars::schema_for!(FieldName)).unwrap();
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["minLength"], 1);

    // Whitespace and line breaks are legal field text; only absence is refused.
    for field in [" ", "\n", "geometry.radius"] {
        let key = FieldName::try_from(field.to_owned()).unwrap();
        let map = NonEmptyMap::try_from(BTreeMap::from([(key, Exactness::Derived)])).unwrap();
        let wire = json::to_value(&map).unwrap();
        assert_eq!(json::from_value::<NonEmptyMap>(wire).unwrap(), map);
    }
    assert!(FieldName::try_from(String::new()).is_err());
    assert!(NonEmptyMap::try_from(BTreeMap::new()).is_err());
}

#[test]
fn asset_byte_schema_requires_at_least_one_encoded_byte() {
    let schema = json::to_value(schemars::schema_for!(AssetData)).unwrap();
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["minLength"], 4);
    assert_eq!(schema["contentEncoding"], "base64");
}

#[test]
fn current_summary_schema_requires_its_identity_and_losses() {
    let schema =
        json::to_value(schemars::schema_for!(ContainerSummary)).expect("summary schema serializes");
    let required = schema["required"]
        .as_array()
        .expect("summary schema has required fields");
    assert!(
        required.iter().any(|field| field == "identity"),
        "{schema:#}"
    );
    assert!(required.iter().any(|field| field == "losses"), "{schema:#}");
}

#[test]
fn source_metadata_schema_requires_its_identity_and_has_no_singular_dialect() {
    let schema = json::to_value(schemars::schema_for!(SourceMeta)).unwrap();
    let required = schema["required"].as_array().unwrap();

    assert!(
        required.iter().any(|field| field == "identity"),
        "{schema:#}"
    );
    assert!(schema["properties"].get("dialect").is_none(), "{schema:#}");
}

#[test]
fn directed_subd_sum_fixture_matches_schema_shape() {
    let schema = json::to_value(schemars::schema_for!(Topology)).expect("serialize schema");
    let schema_text = schema.to_string();
    assert!(schema_text.contains("procedural_surfaces"));
    assert!(schema_text.contains("sharpness"));
    assert!(schema_text.contains("\"sum\""));
}

#[test]
fn digest_schema_does_not_admit_a_trailing_line_break() {
    let schema = json::to_value(schemars::schema_for!(Sha256Digest)).unwrap();
    assert_eq!(schema["minLength"], 64);
    assert_eq!(schema["maxLength"], 64);
    assert_eq!(schema["pattern"], "^[0-9a-f]{64}$");
    assert!(Sha256Digest::try_from(format!("{}\n", "a".repeat(64))).is_err());
}

#[test]
fn identity_schemas_constrain_strings_and_map_keys() {
    macro_rules! check {
        ($identity:ty) => {{
            let schema = json::to_value(schemars::schema_for!($identity)).unwrap();
            let string_schema = match schema["$ref"].as_str() {
                Some(reference) => schema.pointer(reference.strip_prefix('#').unwrap()).unwrap(),
                None => &schema,
            };
            assert_eq!(string_schema["type"], "string");
            let pattern = string_schema["pattern"].as_str().unwrap();
            let map = json::to_value(schemars::schema_for!(BTreeMap<$identity, u8>)).unwrap();
            assert_eq!(map["additionalProperties"], false);
            assert!(map["patternProperties"].get(pattern).is_some());
        }};
    }
    check!(Identity);
    check!(BodyId);
    check!(HistoricalBodyId);
}

#[test]
fn provenance_schemas_use_admitted_stream_names_and_codec_ids() {
    let schema = json::to_value(schemars::schema_for!(StreamName)).unwrap();
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["minLength"], 1);
    let schema = json::to_value(schemars::schema_for!(SourceObjectAssociation)).unwrap();
    assert_eq!(schema["properties"]["format"]["$ref"], "#/$defs/CodecId");
    assert_eq!(schema["$defs"]["CodecId"]["pattern"], "^[a-z0-9]+$");
}

#[test]
fn current_decode_report_schema_requires_its_format_identity() {
    let schema = json::to_value(schemars::schema_for!(DecodeReport))
        .expect("decode report schema serializes");
    let required = schema["required"]
        .as_array()
        .expect("decode report schema has required fields");
    assert!(
        required.iter().any(|field| field == "identity"),
        "{schema:#}"
    );
}

#[test]
fn the_native_export_payload_schema_requires_its_target() {
    let schema = json::to_value(schemars::schema_for!(ExportReport))
        .expect("export report schema serializes");
    let native = schema["$defs"]["ExportIdentity"]["oneOf"]
        .as_array()
        .expect("export identity schema is a tagged union")
        .iter()
        .find(|arm| arm["properties"]["kind"]["const"] == "native")
        .expect("the native arm");
    let required = native["required"]
        .as_array()
        .expect("the native arm has required fields");
    assert!(required.iter().any(|field| field == "target"), "{schema:#}");
}

#[test]
fn schema_constrains_version_and_requires_subd_arena() {
    let schema = json::to_value(cadir_json_schema()).unwrap();
    let version_property = schema.pointer("/properties/ir_version").unwrap();
    let version_schema = match version_property["$ref"].as_str() {
        Some(reference) => schema
            .pointer(reference.strip_prefix('#').unwrap())
            .unwrap(),
        None => version_property,
    };
    assert_eq!(version_schema.get("const"), Some(&json!(IR_VERSION)));
    assert!(schema
        .pointer("/properties/geometry/$ref")
        .and_then(serde_json::Value::as_str)
        .is_some());

    let topology_schema = schema.pointer("/$defs/Topology").unwrap();
    assert!(topology_schema
        .pointer("/required")
        .and_then(serde_json::Value::as_array)
        .unwrap()
        .contains(&json!("subds")));
    assert!(schema.pointer("/properties/byte_ledger").is_none());

    let mut value = json::to_value(unit_cube().expect("valid unit cube fixture")).unwrap();
    value
        .pointer_mut("/geometry")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("subds");
    assert!(json::from_value::<CadIr>(value).is_err());
}

#[test]
fn schema_generation_produces_definitions() {
    let schema = cadir_json_schema();
    let json = json::to_string(&schema).unwrap();
    assert!(json.contains("Body"));
    assert!(json.contains("Coedge"));
    assert!(json.contains("SurfaceGeometry"));
    let defs = schema
        .get("$defs")
        .and_then(serde_json::Value::as_object)
        .expect("schema has a $defs object");
    assert!(!defs.is_empty());
}

#[test]
fn model_wire_and_schema_leave_out_features_and_their_regeneration_parents() {
    let parent = FeatureId::mint("test:model:feature#1-parent").unwrap();
    let child = FeatureId::mint("test:model:feature#2-child").unwrap();
    let mut model = Model::default();
    model.features.push(Feature {
        product_definition: None,
        id: parent.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::SolidBodies,
                children: TreeChildren::default(),
            },
        )),
        native_ref: None,
    });
    let mut child_feature = model.features[0].clone();
    child_feature.id = child.clone();
    child_feature.ordinal = 1;
    child_feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        ));
    model.features.push(child_feature);
    model
        .set_feature_regeneration_parent(&child, &parent)
        .unwrap();
    let wire = json::to_value(&model).unwrap();
    assert!(wire.get("features").is_none());
    let held = model.omitted_arenas_value().unwrap();
    assert_eq!(held["features"][1]["regeneration_parent"], parent.as_str());

    let schema = json::to_value(cadir_json_schema()).unwrap();
    assert!(schema.pointer("/$defs/Model/properties/features").is_none());

    let standalone = json::to_value(schemars::schema_for!(Feature)).unwrap();
    assert!(standalone
        .pointer("/properties/regeneration_parent")
        .is_none());
}
