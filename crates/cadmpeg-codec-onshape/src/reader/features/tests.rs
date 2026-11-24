// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn sketch_selections_sort_entities_and_sweep_selections_keep_source_order() {
    let mut model = Model::default();
    let mut queries = Vec::new();
    for native in ["z", "a"] {
        model.sketch_entities.push(
            serde_json::from_value(json!({
                "id":id("entity", &format!("path/{native}")), "sketch":id("sketch", "path"),
                "construction":false, "endpoint_refs":[],
                "geometry":{"kind":"line", "start":{"u":0.0,"v":0.0}, "end":{"u":1.0,"v":0.0}}
            }))
            .unwrap(),
        );
        let compact = format!("%B5$QueryM5Sa$entityTypeBa$EntityTypeS4$EDGESb$historyTypeS8$CREATIONSb$operationIdB2$IdA1Sb$path.wireOpS9$queryTypeB9$QueryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS1${native}");
        queries.push(json!({"queryString":format!("query=qCompressed(1.0,{},id);", serde_json::to_string(&compact).unwrap())}));
    }
    let sorted = sketch_entities(&queries, &model, "EDGE").unwrap();
    assert_eq!(
        sorted["value"]["entities"],
        json!([id("entity", "path/a"), id("entity", "path/z")])
    );
    let source = sketch_entities_in_order(&queries, &model, "EDGE", true).unwrap();
    assert_eq!(
        source["value"]["entities"],
        json!([id("entity", "path/z"), id("entity", "path/a")])
    );
}
