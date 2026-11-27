// SPDX-License-Identifier: Apache-2.0
//! Feature schema checks.

use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;

use cadmpeg_ir::features::datums::DatumAxis;
use cadmpeg_ir::features::form::{FormConstruction, FormInputs};
use cadmpeg_ir::features::mesh::{MeshPlaneCut, MeshSeparation, MeshSeparationGroups};
use cadmpeg_ir::features::patterns::curve_driven::CurveDrivenPatternParameters;
use cadmpeg_ir::features::patterns::PatternKind;
use cadmpeg_ir::features::replace_face::ReplaceFaceOperands;
use cadmpeg_ir::features::selections::NonEmptyMembers;
use cadmpeg_ir::features::sheet_metal::{
    SheetMetalContourFlangeConstruction, SheetMetalEdgeFlangeConstruction,
    SheetMetalFlangeSymmetricWidth, SheetMetalFlangeTargetConstruction,
};
use cadmpeg_ir::features::sheet_metal_forms::SheetMetalFlangeTwoSidedWidth;
use cadmpeg_ir::features::FeatureOperation;

fn no_inputs() -> serde_json::Value {
    json::to_value(FormInputs::default()).unwrap()
}

fn form_operation_wire(
    kind: &str,
    cages: serde_json::Value,
    inputs: serde_json::Value,
) -> serde_json::Value {
    let mut form = json!({"cages": {"kind": kind}});
    form["cages"]["value"] = cages;
    form["inputs"] = inputs;
    let mut operation = json!({"kind": "form"});
    operation["value"] = form;
    operation
}

#[test]
fn datum_axis_schema_requires_a_readable_fact_and_nonnull_geometry() {
    let schema = json::to_value(schemars::schema_for!(DatumAxis)).unwrap();
    assert_eq!(schema["allOf"][0]["anyOf"].as_array().unwrap().len(), 5);
    assert_eq!(
        schema["allOf"][0]["anyOf"][2]["properties"]["inputs"]["minItems"],
        1
    );
    assert!(!schema["properties"]["origin"].to_string().contains("null"));
    assert!(!schema["properties"]["direction"]
        .to_string()
        .contains("null"));
}

#[test]
fn form_schema_covers_legacy_arrays_and_retained_construction() {
    fn has_null_type(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(fields) => {
                fields.get("type").is_some_and(|kind| {
                    kind == "null"
                        || kind
                            .as_array()
                            .is_some_and(|types| types.iter().any(|kind| kind == "null"))
                }) || fields.values().any(has_null_type)
            }
            serde_json::Value::Array(values) => values.iter().any(has_null_type),
            _ => false,
        }
    }
    let schema = json::to_value(schemars::schema_for!(FormConstruction)).unwrap();
    let text = schema.to_string();
    assert!(text.contains("FormCagesWire"));
    assert!(text.contains("FormCageObservation"));
    assert!(text.contains("cage_inputs"));
    assert!(!has_null_type(&schema));
    assert!(!schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "cage_inputs"));
    let reference_conditions = &schema["$defs"]["FormNativeReference"]["allOf"];
    let reference_schema = &schema["$defs"]["FormNativeReference"];
    assert!(reference_schema["properties"].get("group").is_some());
    assert!(reference_schema["properties"].get("run").is_none());
    assert!(!reference_schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "group"));
    assert_eq!(reference_schema["additionalProperties"], false);
    assert_eq!(
        reference_conditions[0]["if"]["properties"]["target"]["properties"]["kind"]["const"],
        "document"
    );
    assert_eq!(
        reference_conditions[1]["if"]["properties"]["target"]["properties"]["kind"]["enum"],
        json!(["null", "unreadable", "unvisited"])
    );
    let condition = &schema["allOf"][0];
    assert_eq!(
        condition["then"]["properties"]["cages"]["properties"]["value"]["uniqueItems"],
        true
    );
    assert_eq!(
        condition["then"]["properties"]["cage_inputs"]["properties"]["membership"]["properties"]
            ["kind"]["const"],
        "complete"
    );
    assert_eq!(
        condition["else"]["properties"]["cages"]["properties"]["value"]["properties"]["membership"]
            ["properties"]["kind"]["const"],
        "partial"
    );
}

#[test]
fn resource_association_schema_requires_unique_items() {
    let schema = json::to_value(schemars::schema_for!(FormConstruction)).unwrap();
    assert_eq!(
        schema["$defs"]["FormInputs"]["properties"]["resource_associations"]["uniqueItems"],
        true
    );
    assert_eq!(
        schema["$defs"]["FormInputs"]["properties"]["resource_associations"]["type"],
        "array"
    );
    assert!(schema["$defs"]["FormInputs"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|required| required == "resource_associations"));
}

#[test]
fn form_reference_schema_retains_run_bounds_and_exact_guid_lengths() {
    let schema = json::to_value(schemars::schema_for!(FormConstruction)).unwrap();
    let runs = &schema["$defs"]["FormInputs"]["properties"]["reference_runs"];
    assert_eq!(runs["type"], "array");
    assert_eq!(runs["uniqueItems"], true);
    assert_eq!(runs["items"]["minItems"], 1);
    let bounds = runs["anyOf"].as_array().unwrap();
    assert!(bounds.iter().any(|bound| bound["maxItems"] == 0));
    assert!(bounds.iter().any(|bound| bound["minItems"] == 2));
    let inline = &schema["$defs"]["FormNativeReference"]["properties"]["inline_type_guid"];
    let document = schema["$defs"]["FormNativeReferenceTarget"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|branch| branch["properties"].get("target_type_guid"))
        .unwrap();
    for guid in [inline, document] {
        assert_eq!(guid["minLength"], 36);
        assert_eq!(guid["maxLength"], 36);
    }
    for (value, valid) in [
        (json!([]), true),
        (json!([["test:stream:record#1"]]), false),
        (json!([[], ["test:stream:record#1"]]), false),
        (
            json!([["test:stream:record#1"], ["test:stream:record#2"]]),
            true,
        ),
    ] {
        let mut inputs = no_inputs();
        inputs["reference_runs"] = value;
        let form = form_operation_wire("complete", json!([]), inputs);
        assert_eq!(json::from_value::<FeatureOperation>(form).is_ok(), valid);
    }
}

#[test]
fn separation_schema_matches_optional_nonnull_settings() {
    let schema = json::to_value(schemars::schema_for!(MeshSeparation)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    for name in ["method", "groups", "multiple_bodies", "keep_body"] {
        assert!(!schema["required"]
            .as_array()
            .is_some_and(|required| required.contains(&json!(name))));
        assert!(!schema["properties"][name].to_string().contains("\"null\""));
    }
    assert_eq!(schema["properties"]["keep_body"]["type"], "boolean");
    let groups = json::to_value(schemars::schema_for!(MeshSeparationGroups)).unwrap();
    assert!(!groups.to_string().contains("\"null\""));
}

#[test]
fn conversion_schema_exposes_sources_on_the_tagged_operation() {
    fn find_conversion(value: &serde_json::Value) -> Option<&serde_json::Value> {
        if value["properties"]["kind"]["const"] == "mesh_convert"
            || value["properties"]["kind"]["enum"] == json!(["mesh_convert"])
        {
            return Some(&value["properties"]["value"]);
        }
        match value {
            serde_json::Value::Object(fields) => fields.values().find_map(find_conversion),
            serde_json::Value::Array(values) => values.iter().find_map(find_conversion),
            _ => None,
        }
    }
    let schema = json::to_value(schemars::schema_for!(FeatureOperation)).unwrap();
    let conversion = find_conversion(&schema).unwrap();
    let conversion = match conversion.get("$ref").and_then(serde_json::Value::as_str) {
        Some(reference) => schema
            .pointer(reference.strip_prefix('#').unwrap())
            .unwrap(),
        None => conversion,
    };
    assert_eq!(conversion["properties"]["sources"]["type"], "array");
    assert!(conversion["required"]
        .as_array()
        .unwrap()
        .contains(&json!("sources")));
    assert!(conversion["properties"]["method"].is_object());
    assert_eq!(conversion["additionalProperties"], false);
}

#[test]
fn plane_cut_schema_matches_omitted_optional_values_and_control_vocabulary() {
    let schema = json::to_value(schemars::schema_for!(MeshPlaneCut)).unwrap();
    for key in ["origin", "normal", "mode", "fill", "retained_side"] {
        assert!(schema["properties"][key].is_object());
        assert!(!schema["properties"][key].to_string().contains("null"));
        assert!(!schema["required"]
            .as_array()
            .is_some_and(|required| required.contains(&json!(key))));
    }
    assert_eq!(schema["additionalProperties"], false);
    for value in [
        "trim",
        "split_bodies",
        "split_faces",
        "no_fill",
        "minimal",
        "uniform",
        "positive",
        "negative",
    ] {
        assert!(schema.to_string().contains(value));
    }
}

#[test]
fn partial_curve_driven_schema_agrees_with_reader_on_optional_fields_and_count() {
    let schema = json::to_value(schemars::schema_for!(CurveDrivenPatternParameters)).unwrap();
    let properties = schema["properties"].as_object().unwrap();
    assert!(schema.get("required").is_none());
    for name in [
        "path",
        "count",
        "spacing",
        "distance",
        "total_extent",
        "reversed",
        "path_oriented",
        "start_parameter",
    ] {
        let property = &properties[name];
        assert!(
            !property.to_string().contains("\"null\""),
            "{name}: {property}"
        );
        let mut fields = serde_json::Map::new();
        fields.insert(name.into(), serde_json::Value::Null);
        assert!(json::from_value::<CurveDrivenPatternParameters>(fields.into()).is_err());
    }
    assert_eq!(properties["count"]["minimum"], json!(1));
    for (count, accepted) in [(0, false), (1, true)] {
        let value = json!({"kind":"curve_driven_partial", "parameters":{"count":count}});
        assert_eq!(json::from_value::<PatternKind>(value).is_ok(), accepted);
    }
    assert!(json::from_value::<PatternKind>(
        json!({"kind":"curve_driven_partial", "parameters":{}})
    )
    .is_ok());
}

#[test]
fn tangent_chain_schema_is_optional_boolean() {
    let schema = json::to_value(schemars::schema_for!(ReplaceFaceOperands)).unwrap();
    assert_eq!(schema["properties"]["tangent_chain"]["type"], "boolean");
    assert!(!schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("tangent_chain")));
}

#[test]
fn edge_construction_schema_admits_absence_and_excludes_null() {
    let schema = json::to_value(schemars::schema_for!(SheetMetalEdgeFlangeConstruction))
        .expect("edge schema");
    assert!(schema.get("required").is_none());
    for key in [
        "edges",
        "height",
        "angle",
        "height_datum",
        "bend_position",
        "width",
        "bend_radius",
    ] {
        assert!(
            !schema["properties"][key].to_string().contains("\"null\""),
            "{key}"
        );
    }
    for schema in [
        json::to_value(schemars::schema_for!(SheetMetalFlangeSymmetricWidth))
            .expect("width schema"),
        json::to_value(schemars::schema_for!(SheetMetalFlangeTwoSidedWidth)).expect("side schema"),
        json::to_value(schemars::schema_for!(SheetMetalFlangeTargetConstruction))
            .expect("target schema"),
    ] {
        assert!(schema.get("required").is_none());
        for field in schema["properties"]
            .as_object()
            .expect("optional properties")
            .values()
        {
            assert!(!field.to_string().contains("\"null\""));
        }
    }
}

#[test]
fn contour_schema_declares_absence_and_rejects_explicit_nulls() {
    let schema =
        json::to_value(schemars::schema_for!(SheetMetalContourFlangeConstruction)).unwrap();
    let properties = schema["properties"].as_object().unwrap();
    assert!(schema.get("required").is_none());
    for name in [
        "profile",
        "direction",
        "extent",
        "thickness",
        "inside_radius",
        "side",
        "neutral_factor",
    ] {
        assert!(
            !properties[name].to_string().contains("\"null\""),
            "{name}: {}",
            properties[name]
        );
        let mut value = json!({});
        value[name] = serde_json::Value::Null;
        assert!(json::from_value::<SheetMetalContourFlangeConstruction>(value).is_err());
    }
}

#[test]
fn corner_schema_requires_bend_and_effect() {
    fn find_corner(value: &serde_json::Value) -> Option<&serde_json::Value> {
        if value["properties"]["kind"]["const"] == "sheet_metal_corner"
            || value["properties"]["kind"]["enum"] == json!(["sheet_metal_corner"])
        {
            return Some(&value["properties"]["value"]);
        }
        match value {
            serde_json::Value::Object(fields) => fields.values().find_map(find_corner),
            serde_json::Value::Array(values) => values.iter().find_map(find_corner),
            _ => None,
        }
    }
    let schema = json::to_value(schemars::schema_for!(FeatureOperation)).unwrap();
    let corner = find_corner(&schema).unwrap();
    let corner = match corner.get("$ref").and_then(serde_json::Value::as_str) {
        Some(reference) => schema
            .pointer(reference.strip_prefix('#').unwrap())
            .unwrap(),
        None => corner,
    };
    let required = corner["required"].as_array().unwrap();
    assert!(required.contains(&json!("bend")));
    assert!(required.contains(&json!("effect")));
}

#[test]
fn nonempty_members_schema_requires_a_member() {
    for (value, valid) in [
        (json!([]), false),
        (json!([7]), true),
        (json!([7, 7]), true),
    ] {
        assert_eq!(
            json::from_value::<NonEmptyMembers<u32>>(value).is_ok(),
            valid
        );
    }
    let schema = json::to_value(schemars::schema_for!(NonEmptyMembers<u32>)).unwrap();
    assert_eq!(schema["type"], "array");
    assert_eq!(schema["minItems"], 1);
}
