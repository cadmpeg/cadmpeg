// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn sketch_selections_sort_entities_and_sweep_selections_keep_source_order() {
    let mut model = Model::default();
    let mut queries = Vec::new();
    for native in ["z", "a"] {
        model.sketch_entities.push(
            cadmpeg_core::serde_erase::json::from_value(json!({
                "id":id("entity", &format!("path/{native}")), "sketch":id("sketch", "path"),
                "construction":false, "endpoint_refs":[],
                "geometry":{"kind":"line", "start":{"u":0.0,"v":0.0}, "end":{"u":1.0,"v":0.0}}
            }))
            .unwrap(),
        );
        let compact = format!("%B5$QueryM5Sa$entityTypeBa$EntityTypeS4$EDGESb$historyTypeS8$CREATIONSb$operationIdB2$IdA1Sb$path.wireOpS9$queryTypeB9$QueryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS1${native}");
        queries.push(json!({"queryString":format!("query=qCompressed(1.0,{},id);", cadmpeg_core::serde_erase::json::to_string(&compact).unwrap())}));
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

#[test]
fn whole_sketch_region_queries_match_with_any_whitespace_between_tokens() {
    for (sketch, string) in [
        ("S", "query = qSketchRegion(id + \"S\", false);"),
        ("S", "query=qSketchRegion(id+\"S\",false);"),
        ("S", "query =\n  qSketchRegion( id +\t\"S\" ,false) ;"),
        (
            "Base sketch",
            "query=qSketchRegion(id+\"Base sketch\",false);",
        ),
    ] {
        let query = json!({"btType":"BTMIndividualSketchRegionQuery-140",
            "featureId":sketch, "filterInnerLoops":false, "queryString":string});
        assert!(whole_sketch_region_query(&query), "{string}");
    }
    for (sketch, inner, string) in [
        ("S", false, "query=qSketchRegion(id+\"T\",false);"),
        ("S", false, "query=qSketchRegion(id+\"S\",true);"),
        ("ab", false, "query=qSketchRegion(id+\"a b\",false);"),
        ("S", true, "query=qSketchRegion(id+\"S\",false);"),
    ] {
        let query = json!({"btType":"BTMIndividualSketchRegionQuery-140",
            "featureId":sketch, "filterInnerLoops":inner, "queryString":string});
        assert!(!whole_sketch_region_query(&query), "{string}");
    }
}
