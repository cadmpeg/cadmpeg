// SPDX-License-Identifier: Apache-2.0
use crate::test_support::{fixture, review};
use serde_json::{json, Value};

fn sweep_bundle(operation: &str, control: Option<&str>) -> Vec<u8> {
    review::edit(&fixture("plate-fillet"), |path, value| {
        if !path.ends_with("/features.json") {
            return;
        }
        let mut path_sketch = value["features"][0].clone();
        path_sketch["featureId"] = json!("path-sketch");
        path_sketch["name"] = json!("Path sketch");
        path_sketch["constraints"] = json!([]);
        path_sketch["entities"] = json!([
            {"btType":"BTMSketchCurveSegment-155", "entityId":"z-line",
             "startParam":0.0,"endParam":0.02,
             "geometry":{"btType":"BTCurveGeometryLine-117", "dirX":1.0}},
            {"btType":"BTMSketchCurveSegment-155", "entityId":"a-arc",
             "startParam":-std::f64::consts::FRAC_PI_2,"endParam":0.0,
             "geometry":{"btType":"BTCurveGeometryCircle-115", "radius":0.01,
                "xCenter":0.02,"yCenter":0.01,"xDir":1.0,"yDir":0.0}}
        ]);
        let leaves: Vec<_> = ["z-line", "a-arc"].into_iter().map(|entity| {
            format!(
                "B5$QueryM5Sa$entityTypeBa$EntityTypeS4$EDGESb$historyTypeS8$CREATIONSb$operationIdB2$IdA1S{:x}$path-sketch.wireOpS9$queryTypeB9$QueryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS{:x}${entity}",
                "path-sketch.wireOp".len(), entity.len())
        }).collect();
        let compact_queries = if control.is_some() {
            vec![format!(
                "%B5$QueryM2S9$queryTypeB9$QueryTypeS5$UNIONSa$subqueriesA2{}{}",
                leaves[0], leaves[1]
            )]
        } else {
            leaves.iter().map(|leaf| format!("%{leaf}")).collect()
        };
        let queries: Vec<_> = compact_queries.iter().map(|compact| {
            json!({"queryString":format!("query=qCompressed(1.0,{},id);", serde_json::to_string(compact).unwrap())})
        }).collect();
        let mut sweep = json!({"featureId":"swept-body", "featureType":"sweep", "name":"Sweep", "parameters":[
            {"parameterId":"bodyType","value":"SOLID"},
            {"parameterId":"operationType","value":operation},
            {"parameterId":"profiles","queries":[{"btType":"BTMIndividualSketchRegionQuery-140","featureId":"Base sketch"}]},
            {"parameterId":"path","queries":queries}
        ]});
        if let Some(control) = control {
            sweep["parameters"]
                .as_array_mut()
                .unwrap()
                .push(json!({"parameterId":"profileControl","value":control}));
        }
        let mut features = vec![value["features"][0].clone(), path_sketch];
        if operation == "ADD" {
            features.push(value["features"][1].clone());
        }
        features.push(sweep);
        value["rollbackIndex"] = json!(features.len());
        value["features"] = json!(features);
    })
}

#[test]
fn plain_sweep_resolves_profile_path_order_and_dependencies() {
    for (operation, control, expected) in [
        ("NEW", None, "new_body"),
        ("NEW", Some("NONE"), "new_body"),
        ("ADD", Some("NONE"), "join"),
    ] {
        let result = review::decode(&sweep_bundle(operation, control));
        let wire = review::wire(&result);
        let feature = wire["model"]["features"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(feature["definition"]["value"]["kind"], "sweep");
        let sweep = &feature["definition"]["value"]["value"];
        assert_eq!(sweep["shape"]["op"], expected);
        assert_eq!(
            sweep["shape"]["section"]["value"],
            json!({"kind":"sketch","value":"onshape:model:sketch#Base%20sketch"})
        );
        assert_eq!(
            sweep["path"],
            json!({"kind":"sketch_curves","value":{"sketch":"onshape:model:sketch#path-sketch","curves":["onshape:model:entity#path-sketch/z-line","onshape:model:entity#path-sketch/a-arc"]}})
        );
        assert_eq!(
            feature["dependencies"],
            json!([
                "onshape:model:feature#Base%20sketch",
                "onshape:model:feature#path-sketch"
            ])
        );
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "onshape/sweep.not-supported"));
        review::validate(result);
    }
}

#[test]
fn unsupported_sweep_options_retain_native_with_named_loss() {
    for (key, value) in [
        ("profileControl", json!("KEEP_ORIENTATION")),
        ("bodyType", json!("SURFACE")),
        ("operationType", json!("REMOVE")),
        ("hasTwist", json!(true)),
        ("hasScale", json!(true)),
        ("extendToFullPath", json!(true)),
        ("pathOppositeDirection", json!(true)),
        ("profileControl", Value::Null),
    ] {
        let bytes = review::edit(&sweep_bundle("NEW", Some("NONE")), |path, source| {
            if path.ends_with("/features.json") {
                let parameters = source["features"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()["parameters"]
                    .as_array_mut()
                    .unwrap();
                parameters.retain(|p| p["parameterId"] != key);
                parameters.push(json!({"parameterId":key,"value":value}));
            }
        });
        let result = review::decode(&bytes);
        let wire = review::wire(&result);
        let feature = wire["model"]["features"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(feature["definition"]["value"]["kind"], "native", "{key}");
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.to_string() == "onshape/sweep.not-supported"),
            "{key}"
        );
        review::validate(result);
    }
}
