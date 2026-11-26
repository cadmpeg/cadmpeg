use super::{SheetMetalBendLinesConstruction, SheetMetalLineBend};
use cadmpeg_core::serde_erase::json;
use serde_json::json;

#[test]
fn edge_partial_extent_inputs_keep_each_known_member() {
    use crate::features::FeatureOperation;
    for width in [
        json!({"kind":"symmetric_per_edge","widths":[{"width":3.0},{"width":7.0},{}]}),
        json!({"kind":"two_sides_per_edge","widths":[{"first":4.0},{"second":2.0}]}),
        json!({"kind":"symmetric","width":3.0}),
        json!({"kind":"two_sides","second":2.0}),
    ] {
        let wire = json!({"kind": "sheet_metal_edge_flange", "value": {
            "height":{"kind":"to_object","value":{"target":{"kind":"native","value":"test:target"}}},
            "width":width
        }});
        let value: FeatureOperation =
            json::from_value(wire.clone()).expect("independent extent inputs");
        assert_eq!(json::to_value(value).expect("extent wire"), wire);
    }
    for (field, ty) in [("width", 0), ("first", 1), ("second", 1)] {
        let value = json!({field: null});
        let error = if ty == 0 {
            json::from_value::<super::SheetMetalFlangeSymmetricWidth>(value)
                .expect_err("null width")
                .to_string()
        } else {
            json::from_value::<crate::features::sheet_metal_forms::SheetMetalFlangeTwoSidedWidth>(
                value,
            )
            .expect_err("null distance")
            .to_string()
        };
        assert!(error.contains(field), "{error}");
    }
    for field in ["target", "offset"] {
        assert!(
            json::from_value::<super::SheetMetalFlangeTargetConstruction>(json!({field:null}))
                .expect_err("null height input")
                .to_string()
                .contains(field)
        );
    }
    assert!(
        json::from_value::<crate::features::sheet_metal_forms::SheetMetalFlangeWidth>(
            json!({"kind":"two_sides_per_edge","widths":[]})
        )
        .is_err()
    );
}

#[test]
fn edge_complete_extent_laws_keep_their_existing_wire() {
    use crate::features::sheet_metal_forms::{SheetMetalFlangeHeight, SheetMetalFlangeWidth};
    for wire in [
        json!({"kind":"symmetric","width":3.0}),
        json!({"kind":"two_sides","first":4.0,"second":2.0}),
        json!({"kind":"two_sides_per_edge","widths":[{"first":4.0,"second":2.0}]}),
    ] {
        let value: SheetMetalFlangeWidth =
            json::from_value(wire.clone()).expect("complete width law");
        assert_eq!(json::to_value(value).expect("complete width wire"), wire);
    }
    let wire = json!({"kind":"to_object","value":{"target":{"kind":"native","value":"test:plane"},"offset":-2.0}});
    let value: SheetMetalFlangeHeight =
        json::from_value(wire.clone()).expect("complete height law");
    assert_eq!(json::to_value(value).expect("complete height wire"), wire);
}

#[test]
fn edge_construction_keeps_complete_flat_wire_and_accepts_sparse_inputs() {
    use super::SheetMetalEdgeFlangeConstruction;
    use crate::features::FeatureOperation;
    let wire = json!({"kind": "sheet_metal_edge_flange", "value": {
        "edges": {"kind": "native", "value": "test:edges"},
        "height": {"kind": "distance", "value": 5.0},
        "angle": 1.2,
        "height_datum": "outer_faces",
        "bend_position": "inside",
        "width": {"kind": "full_edge"},
        "bend_radius": 0.5
    }});
    let operation: FeatureOperation = json::from_value(wire.clone()).expect("complete edge wire");
    assert_eq!(json::to_value(operation).expect("edge wire"), wire);
    let sparse = json!({"kind": "sheet_metal_edge_flange", "value": {}});
    let operation: FeatureOperation = json::from_value(sparse.clone()).expect("sparse edge wire");
    assert_eq!(
        operation,
        FeatureOperation::SheetMetalEdgeFlange(SheetMetalEdgeFlangeConstruction::default())
    );
    assert_eq!(json::to_value(operation).expect("sparse edge wire"), sparse);
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
        invalid["value"][key] = serde_json::Value::Null;
        let error = json::from_value::<FeatureOperation>(invalid)
            .expect_err("null construction member")
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    let mut unknown = wire.clone();
    unknown["value"]["extra"] = json!(true);
    assert!(json::from_value::<FeatureOperation>(unknown).is_err());
    let mut zero_radius = wire;
    zero_radius["value"]["bend_radius"] = json!(0.0);
    assert!(json::from_value::<FeatureOperation>(zero_radius).is_ok());
}

#[test]
fn edge_offset_rows_keep_signed_and_missing_members() {
    use super::SheetMetalFlangeOffset;
    use crate::features::sheet_metal_forms::SheetMetalFlangeWidth;
    let wire = json!({"kind": "two_offsets_per_edge", "offsets": [
        {"first": 0.0, "second": -7.5, "first_reference": "test:boundary#first", "second_reference": "test:boundary#second"},
        {"first": -5.0}
    ]});
    let width: SheetMetalFlangeWidth =
        json::from_value(wire.clone()).expect("signed source offsets");
    assert_eq!(json::to_value(width).expect("offset wire"), wire);
    assert!(json::from_value::<SheetMetalFlangeWidth>(
        json!({"kind":"two_offsets_per_edge","offsets":[]})
    )
    .is_err());
    for key in ["first", "second", "first_reference", "second_reference"] {
        let mut invalid = json!({});
        invalid[key] = serde_json::Value::Null;
        let error = json::from_value::<SheetMetalFlangeOffset>(invalid)
            .expect_err("null offset member")
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    assert!(json::from_value::<SheetMetalFlangeOffset>(json!({"first_reference":""})).is_err());
    assert!(json::from_value::<SheetMetalFlangeOffset>(json!({"extra": 1})).is_err());
}

#[cfg(feature = "schema")]
#[test]
fn edge_construction_schema_admits_absence_and_excludes_null() {
    use super::SheetMetalEdgeFlangeConstruction;
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
        json::to_value(schemars::schema_for!(super::SheetMetalFlangeSymmetricWidth))
            .expect("width schema"),
        json::to_value(schemars::schema_for!(
            crate::features::sheet_metal_forms::SheetMetalFlangeTwoSidedWidth
        ))
        .expect("side schema"),
        json::to_value(schemars::schema_for!(
            super::SheetMetalFlangeTargetConstruction
        ))
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
fn edge_construction_rewrite_visits_optional_height_feature() {
    use super::SheetMetalEdgeFlangeConstruction;
    use crate::features::sheet_metal_forms::{
        SheetMetalFlangeHeight, SheetMetalFlangeHeightTarget,
    };
    use crate::features::FeatureId;
    use crate::scalar::Length;
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let construction = SheetMetalEdgeFlangeConstruction {
        height: Some(SheetMetalFlangeHeight::to_object(
            SheetMetalFlangeHeightTarget::Feature(
                FeatureId::mint("test:model:feature#source").expect("source feature"),
            ),
            Length::new(-2.0).expect("signed height offset"),
        )),
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
        matches!(rewritten.height, Some(SheetMetalFlangeHeight::ToObject(ref target)) if matches!(&target.target, Some(SheetMetalFlangeHeightTarget::Feature(id)) if id.as_str()=="test:model:feature#rewritten"))
    );
    map.finish().expect("finish rewrite");
    let partial = super::SheetMetalFlangeTargetConstruction {
        target: Some(SheetMetalFlangeHeightTarget::Feature(
            FeatureId::mint("test:model:feature#source").expect("partial target"),
        )),
        ..Default::default()
    };
    let height = SheetMetalFlangeHeight::ToObject(partial);
    let mut map = IdentityMap::new(|source: &str| Ok(source.replace("#source", "#rewritten")));
    let rewritten = height
        .rewrite_identities(&context, &mut map)
        .expect("partial target rewrite");
    assert!(
        matches!(rewritten, SheetMetalFlangeHeight::ToObject(construction) if matches!(construction.target, Some(SheetMetalFlangeHeightTarget::Feature(ref id)) if id.as_str() == "test:model:feature#rewritten") && construction.offset.is_none())
    );
    map.finish().expect("finish partial rewrite");
}

#[test]
fn bend_construction_omits_absent_optional_inputs() {
    let value = json!({"bends": [{}]});
    let construction: SheetMetalBendLinesConstruction = json::from_value(value.clone()).unwrap();
    assert_eq!(construction.bends, vec![SheetMetalLineBend::default()]);
    assert_eq!(construction.stationary, None);
    assert_eq!(construction.inside_radius, None);
    assert_eq!(json::to_value(construction).unwrap(), value);
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
    let construction: SheetMetalBendLinesConstruction = json::from_value(value.clone()).unwrap();
    assert_eq!(construction.bends[0].angle.unwrap().get(), -0.75);
    assert_eq!(construction.inside_radius.unwrap().get(), 2.5);
    assert_eq!(json::to_value(construction).unwrap(), value);
}

#[test]
fn bend_optional_inputs_refuse_named_nulls() {
    for key in ["stationary", "inside_radius"] {
        let mut value = json!({"bends": []});
        value[key] = serde_json::Value::Null;
        let error = json::from_value::<SheetMetalBendLinesConstruction>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    for key in ["line", "angle"] {
        let mut value = json!({});
        value[key] = serde_json::Value::Null;
        let error = json::from_value::<SheetMetalLineBend>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn fold_constructions_distinguish_absent_present_and_null_inputs() {
    use super::{SheetMetalRefoldConstruction, SheetMetalUnfoldConstruction};
    let empty: SheetMetalUnfoldConstruction = json::from_value(json!({})).unwrap();
    assert_eq!(empty, SheetMetalUnfoldConstruction::default());
    assert_eq!(json::to_value(empty).unwrap(), json!({}));
    let value = json!({
        "stationary": {"kind": "native", "value": "test:stationary"},
        "bends": {"kind": "native", "value": "test:bends"},
        "all_bends": false,
        "neutral_factor": 1.0
    });
    let construction: SheetMetalUnfoldConstruction = json::from_value(value.clone()).unwrap();
    assert_eq!(construction.all_bends, Some(false));
    assert_eq!(construction.neutral_factor.unwrap().get(), 1.0);
    assert_eq!(json::to_value(construction).unwrap(), value);
    for key in ["stationary", "bends", "all_bends", "neutral_factor"] {
        let mut value = json!({});
        value[key] = serde_json::Value::Null;
        let error = json::from_value::<SheetMetalUnfoldConstruction>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    let empty: SheetMetalRefoldConstruction = json::from_value(json!({})).unwrap();
    assert_eq!(empty, SheetMetalRefoldConstruction::default());
    assert_eq!(json::to_value(empty).unwrap(), json!({}));
    let value = json!({"unfold": "test:model:feature#unfold"});
    let construction: SheetMetalRefoldConstruction = json::from_value(value.clone()).unwrap();
    assert_eq!(json::to_value(construction).unwrap(), value);
    let error = json::from_value::<SheetMetalRefoldConstruction>(json!({"unfold": null}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unfold"), "{error}");
}

#[test]
fn contour_construction_roundtrips_signed_travel_and_rejects_named_nulls() {
    use super::SheetMetalContourFlangeConstruction;
    let empty: SheetMetalContourFlangeConstruction = json::from_value(json!({})).unwrap();
    assert_eq!(empty, SheetMetalContourFlangeConstruction::default());
    assert_eq!(json::to_value(empty).unwrap(), json!({}));
    let value = json!({
        "profile": {"kind": "native", "value": "test:profile"},
        "direction": {"x": 0.0, "y": -1.0, "z": 0.0},
        "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": -15.0}}},
        "thickness": 1.25,
        "inside_radius": 0.0,
        "side": "left",
        "neutral_factor": 0.44
    });
    let construction: SheetMetalContourFlangeConstruction =
        json::from_value(value.clone()).unwrap();
    assert_eq!(json::to_value(construction).unwrap(), value);
    for key in [
        "profile",
        "direction",
        "extent",
        "thickness",
        "inside_radius",
        "side",
        "neutral_factor",
    ] {
        let mut invalid = value.clone();
        invalid[key] = serde_json::Value::Null;
        let error = json::from_value::<SheetMetalContourFlangeConstruction>(invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
}

#[cfg(feature = "schema")]
#[test]
fn contour_schema_declares_absence_and_rejects_explicit_nulls() {
    use super::SheetMetalContourFlangeConstruction;
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
fn contour_identity_rewrite_traverses_its_profile_reference() {
    use super::SheetMetalContourFlangeConstruction;
    use crate::features::profiles::PlanarProfileRef;
    use crate::features::FeatureId;
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let construction = SheetMetalContourFlangeConstruction {
        profile: Some(PlanarProfileRef::Feature(
            FeatureId::mint("test:model:feature#source").unwrap(),
        )),
        ..Default::default()
    };
    let arena = DecodeArena::new();
    let (context, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut map = IdentityMap::new(|source: &str| Ok(source.replace("#source", "#rewritten")));
    let rewritten = construction.rewrite_identities(&context, &mut map).unwrap();
    assert_eq!(
        rewritten.profile,
        Some(PlanarProfileRef::Feature(
            FeatureId::mint("test:model:feature#rewritten").unwrap()
        ))
    );
    map.finish().unwrap();
}
