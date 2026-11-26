// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::json;

use super::DesignScope;
use crate::design::read;
use crate::ids::BodyId;
use cadmpeg_core::decode::ResourceLimits;

fn scope() -> DesignScope {
    cadmpeg_core::serde_erase::json::from_value(json!({
        "definition": "test:design:definition#part",
        "roots": [], "active_arrangements": [], "occurrences": {}, "current": [],
        "unproduced": []
    }))
    .unwrap()
}

#[test]
fn unproduced_writes_the_empty_array_and_requires_the_key() {
    let scope = scope();
    assert!(scope.unproduced.is_empty());
    let mut json = cadmpeg_core::serde_erase::json::to_value(&scope).unwrap();
    assert_eq!(json["unproduced"], json!([]));
    let back: DesignScope = read::from_str(&json.to_string(), &ResourceLimits::desktop()).unwrap();
    assert_eq!(back, scope);
    json.as_object_mut().unwrap().remove("unproduced");
    assert!(read::from_str::<DesignScope>(&json.to_string(), &ResourceLimits::desktop()).is_err());
}

#[test]
fn unproduced_round_trips_as_a_sorted_distinct_set() {
    let mut scope = scope();
    scope.unproduced = BTreeSet::from([
        BodyId::mint("test:design:body#b").unwrap(),
        BodyId::mint("test:design:body#a").unwrap(),
    ]);
    let json = cadmpeg_core::serde_erase::json::to_value(&scope).unwrap();
    assert_eq!(
        json["unproduced"],
        json!(["test:design:body#a", "test:design:body#b"])
    );
    let back: DesignScope = read::from_str(&json.to_string(), &ResourceLimits::desktop()).unwrap();
    assert_eq!(back, scope);
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(back).unwrap(),
        json
    );
}

#[test]
fn unproduced_refuses_null_duplicates_invalid_ids_and_unknown_fields() {
    for value in [
        json!(null),
        json!(["test:design:body#a", "test:design:body#a"]),
        json!(["invalid"]),
    ] {
        let mut json = cadmpeg_core::serde_erase::json::to_value(scope()).unwrap();
        json["unproduced"] = value;
        assert!(
            read::from_str::<DesignScope>(&json.to_string(), &ResourceLimits::desktop()).is_err()
        );
    }
    let mut json = cadmpeg_core::serde_erase::json::to_value(scope()).unwrap();
    json["extra"] = json!([]);
    assert!(read::from_str::<DesignScope>(&json.to_string(), &ResourceLimits::desktop()).is_err());
}

#[cfg(feature = "schema")]
#[test]
fn unproduced_schema_is_a_required_unique_array_of_body_ids() {
    let schema =
        cadmpeg_core::serde_erase::json::to_value(schemars::schema_for!(DesignScope)).unwrap();
    let property = &schema["properties"]["unproduced"];
    assert_eq!(property["type"], "array");
    assert_eq!(property["uniqueItems"], true);
    assert!(property["items"]["$ref"]
        .as_str()
        .unwrap()
        .ends_with("/BodyId"));
    assert!(schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("unproduced")));
    assert_eq!(schema["additionalProperties"], false);
}

#[test]
fn unproduced_participates_in_the_document_content_digest_and_census() {
    let mut document = crate::CadIr::empty();
    document.design.scopes.push(scope());
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let empty = crate::hash::canonical_json_sha256(&ctx, &document).unwrap();
    document.design.scopes[0]
        .unproduced
        .insert(BodyId::mint("test:design:body#a").unwrap());
    assert_ne!(
        crate::hash::canonical_json_sha256(&ctx, &document).unwrap(),
        empty
    );
    let back = crate::CadIr::from_json(&document.to_canonical_json().unwrap()).unwrap();
    assert_eq!(back.design.scopes, document.design.scopes);
    assert_eq!(back.census()["design.scopes"], 1);
    assert_eq!(
        crate::hash::canonical_json_sha256(&ctx, &back).unwrap(),
        crate::hash::canonical_json_sha256(&ctx, &document).unwrap()
    );
}
