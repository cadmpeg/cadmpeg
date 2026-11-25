// SPDX-License-Identifier: Apache-2.0
use super as query;
use crate::test_support::projection::{changed, selected};
use serde_json::{json, Value};
use std::fmt::Write as _;

#[test]
fn compressed_reference_expansion_has_a_budget_and_a_depth_limit() {
    // The literal itself stays small while its backward references would make
    // the neutral JSON exponentially large without accounting for each clone.
    let mut literal = "%A29S1$x".to_owned();
    for i in 0..40 {
        write!(literal, "A2R{i:x}R{i:x}").unwrap();
    }
    let mut query = json!({});
    query["queryString"] = json!(format!(
        "query=qCompressed(1.0,{},id);",
        cadmpeg_core::serde_erase::json::to_string(&literal).unwrap()
    ));
    assert!(query::resolved(&query)
        .unwrap_err()
        .contains("reference limit"));
    let mut literal = "%A47S1$x".to_owned();
    for i in 0..70 {
        // Each completed array wraps the preceding cached value, keeping the
        // encoded nesting shallow while the referenced tree grows deeper.
        write!(literal, "A1R{i:x}").unwrap();
    }
    query["queryString"] = json!(format!(
        "query=qCompressed(1.0,{},id);",
        cadmpeg_core::serde_erase::json::to_string(&literal).unwrap()
    ));
    assert!(query::resolved(&query)
        .unwrap_err()
        .contains("nesting limit"));
}

#[test]
fn compressed_queries_are_bounded_and_truncation_never_panics() {
    let mut query = Value::Null;
    changed(|response| query = selected(response).clone());
    let original = query["queryString"].as_str().unwrap().to_owned();
    assert!(query::resolved(&query).is_ok());
    for n in 0..original.trim_end_matches(';').len() {
        query["queryString"] = json!(&original[..n]);
        assert!(query::resolved(&query).is_err());
    }
    for encoded in [
        "%Rffff",
        "%Affff",
        "%M1S1$kR1",
        "%M2S1$kD1R0D2",
        "&100001$eA==",
    ] {
        query["queryString"] = json!(format!(
            "query=qCompressed(1.0,{},id);",
            cadmpeg_core::serde_erase::json::to_string(encoded).unwrap()
        ));
        assert!(query::resolved(&query).is_err(), "{encoded}");
    }
}

#[test]
fn typed_query_unions_preserve_nested_operand_order() {
    let compact = "%B5$QueryM2S9$queryTypeB9$QueryTypeS5$UNIONSa$subqueriesA2M2S9$queryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS1$zM2S9$queryTypeS5$UNIONSa$subqueriesA1M2S9$queryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS1$a";
    let source = json!({"queryString":format!("query=qCompressed(1.0,{},id);",cadmpeg_core::serde_erase::json::to_string(compact).unwrap())});
    let leaves = query::resolved_leaves(&source).unwrap();
    assert_eq!(
        leaves
            .iter()
            .map(|leaf| leaf["sketchEntityId"].clone())
            .collect::<Vec<_>>(),
        vec![json!("z"), json!("a")]
    );
}

#[test]
fn query_unions_require_nonempty_operand_arrays() {
    for compact in [
        "%M2S9$queryTypeS5$UNIONSa$subqueriesA0",
        "%M2S9$queryTypeS5$UNIONSa$subqueriesS1$x",
    ] {
        let source = json!({"queryString":format!("query=qCompressed(1.0,{},id);",cadmpeg_core::serde_erase::json::to_string(compact).unwrap())});
        assert!(query::resolved_leaves(&source).is_err());
    }
}
