use super::{SheetMetalBendLinesConstruction, SheetMetalLineBend};
use serde_json::json;

#[test]
fn edge_construction_keeps_complete_flat_wire_and_accepts_sparse_inputs() {
    use super::SheetMetalEdgeFlangeConstruction;
    use crate::features::FeatureOperation;
    let wire = json!({
        "definition": "sheet_metal_edge_flange",
        "edges": {"kind": "native", "value": "test:edges"},
        "height": {"kind": "distance", "value": 5.0},
        "angle": 1.2,
        "height_datum": "outer_faces",
        "bend_position": "inside",
        "width": {"kind": "full_edge"},
        "bend_radius": 0.5
    });
    let operation: FeatureOperation =
        serde_json::from_value(wire.clone()).expect("complete edge wire");
    assert_eq!(serde_json::to_value(operation).expect("edge wire"), wire);
    let sparse = json!({"definition": "sheet_metal_edge_flange"});
    let operation: FeatureOperation =
        serde_json::from_value(sparse.clone()).expect("sparse edge wire");
    assert_eq!(
        operation,
        FeatureOperation::SheetMetalEdgeFlange(SheetMetalEdgeFlangeConstruction::default())
    );
    assert_eq!(
        serde_json::to_value(operation).expect("sparse edge wire"),
        sparse
    );
    for key in [
        "edges",
        "height",
        "angle",
        "height_datum",
        "bend_position",
        "width",
        "bend_radius",
    ] {
        let mut invalid = wire.clone();
        invalid[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<FeatureOperation>(invalid)
            .expect_err("null construction member")
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    let mut unknown = wire.clone();
    unknown["extra"] = json!(true);
    assert!(serde_json::from_value::<FeatureOperation>(unknown).is_err());
    let mut zero_radius = wire;
    zero_radius["bend_radius"] = json!(0.0);
    assert!(serde_json::from_value::<FeatureOperation>(zero_radius).is_ok());
}

#[test]
fn edge_offset_rows_keep_signed_and_missing_members() {
    use super::SheetMetalFlangeOffset;
    use crate::features::SheetMetalFlangeWidth;
    let wire = json!({"kind": "two_offsets_per_edge", "value": {"offsets": [
        {"first": 0.0, "second": -7.5, "first_reference": "test:boundary#first", "second_reference": "test:boundary#second"},
        {"first": -5.0}
    ]}});
    let width: SheetMetalFlangeWidth =
        serde_json::from_value(wire.clone()).expect("signed source offsets");
    assert_eq!(serde_json::to_value(width).expect("offset wire"), wire);
    assert!(serde_json::from_value::<SheetMetalFlangeWidth>(
        json!({"kind":"two_offsets_per_edge","value":{"offsets":[]}})
    )
    .is_err());
    for key in ["first", "second", "first_reference", "second_reference"] {
        let mut invalid = json!({});
        invalid[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<SheetMetalFlangeOffset>(invalid)
            .expect_err("null offset member")
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    assert!(
        serde_json::from_value::<SheetMetalFlangeOffset>(json!({"first_reference":""})).is_err()
    );
    assert!(serde_json::from_value::<SheetMetalFlangeOffset>(json!({"extra": 1})).is_err());
}

#[cfg(feature = "schema")]
#[test]
fn edge_construction_schema_admits_absence_and_excludes_null() {
    use super::SheetMetalEdgeFlangeConstruction;
    let schema = serde_json::to_value(schemars::schema_for!(SheetMetalEdgeFlangeConstruction))
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
}

#[test]
fn edge_construction_rewrite_visits_optional_height_feature() {
    use super::SheetMetalEdgeFlangeConstruction;
    use crate::{
        features::{FeatureId, SheetMetalFlangeHeight, SheetMetalFlangeHeightTarget},
        scalar::Length,
        schema::rewrite::typed::{IdentityMap, RewriteIdentities},
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let construction = SheetMetalEdgeFlangeConstruction {
        height: Some(SheetMetalFlangeHeight::ToObject {
            target: SheetMetalFlangeHeightTarget::Feature(
                FeatureId::mint("test:model:feature#source").expect("source feature"),
            ),
            offset: Length::new(-2.0).expect("signed height offset"),
        }),
        ..Default::default()
    };
    let arena = DecodeArena::new();
    let (context, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("rewrite context");
    let mut map = IdentityMap::new(|source: &str| Ok(source.replace("#source", "#rewritten")));
    let rewritten = construction
        .rewrite_identities(&context, &mut map)
        .expect("rewrite edge construction");
    assert!(
        matches!(rewritten.height, Some(SheetMetalFlangeHeight::ToObject { target: SheetMetalFlangeHeightTarget::Feature(ref id), .. }) if id.as_str()=="test:model:feature#rewritten")
    );
    map.finish().expect("finish rewrite");
}

#[test]
fn bend_construction_omits_absent_optional_inputs() {
    let value = json!({"bends": [{}]});
    let construction: SheetMetalBendLinesConstruction =
        serde_json::from_value(value.clone()).unwrap();
    assert_eq!(construction.bends, vec![SheetMetalLineBend::default()]);
    assert_eq!(construction.stationary, None);
    assert_eq!(construction.inside_radius, None);
    assert_eq!(serde_json::to_value(construction).unwrap(), value);
}

#[test]
fn bend_construction_round_trips_present_inputs() {
    let value = json!({
        "bends": [{
            "line": {"kind": "native", "value": "test:line"},
            "angle": -0.75
        }],
        "stationary": {"kind": "native", "value": "test:stationary"},
        "inside_radius": 2.5
    });
    let construction: SheetMetalBendLinesConstruction =
        serde_json::from_value(value.clone()).unwrap();
    assert_eq!(construction.bends[0].angle.unwrap().get(), -0.75);
    assert_eq!(construction.inside_radius.unwrap().get(), 2.5);
    assert_eq!(serde_json::to_value(construction).unwrap(), value);
}

#[test]
fn bend_optional_inputs_refuse_named_nulls() {
    for key in ["stationary", "inside_radius"] {
        let mut value = json!({"bends": []});
        value[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<SheetMetalBendLinesConstruction>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    for key in ["line", "angle"] {
        let mut value = json!({});
        value[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<SheetMetalLineBend>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn fold_constructions_distinguish_absent_present_and_null_inputs() {
    use super::{SheetMetalRefoldConstruction, SheetMetalUnfoldConstruction};
    let empty: SheetMetalUnfoldConstruction = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty, SheetMetalUnfoldConstruction::default());
    assert_eq!(serde_json::to_value(empty).unwrap(), json!({}));
    let value = json!({
        "stationary": {"kind": "native", "value": "test:stationary"},
        "bends": {"kind": "native", "value": "test:bends"},
        "all_bends": false,
        "neutral_factor": 1.0
    });
    let construction: SheetMetalUnfoldConstruction = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(construction.all_bends, Some(false));
    assert_eq!(construction.neutral_factor.unwrap().get(), 1.0);
    assert_eq!(serde_json::to_value(construction).unwrap(), value);
    for key in ["stationary", "bends", "all_bends", "neutral_factor"] {
        let mut value = json!({});
        value[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<SheetMetalUnfoldConstruction>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    let empty: SheetMetalRefoldConstruction = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty, SheetMetalRefoldConstruction::default());
    assert_eq!(serde_json::to_value(empty).unwrap(), json!({}));
    let value = json!({"unfold": "test:model:feature#unfold"});
    let construction: SheetMetalRefoldConstruction = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(construction).unwrap(), value);
    let error = serde_json::from_value::<SheetMetalRefoldConstruction>(json!({"unfold": null}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unfold"), "{error}");
}
