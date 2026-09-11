// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::features::{BodyMembers, BodySelection};
use crate::ids::{BodyId, HistoricalBodyId};

#[test]
fn ordered_members_reject_empty_sets_duplicates_and_mismatched_columns() {
    let a: BodyId = "test:selection:body#a".try_into().unwrap();
    let b: BodyId = "test:selection:body#b".try_into().unwrap();
    for (bodies, native) in [
        (Vec::new(), Vec::new()),
        (vec![a.clone(), a.clone()], vec!["a", "b"]),
        (vec![a.clone(), b], vec!["a", "a"]),
    ] {
        let rows = bodies
            .iter()
            .zip(&native)
            .map(|(body, native)| serde_json::json!({"body": body, "native": native}))
            .collect::<Vec<_>>();
        assert!(BodyMembers::try_from_parts(
            bodies,
            native.into_iter().map(str::to_owned).collect(),
        )
        .is_err());
        assert!(serde_json::from_value::<BodyMembers<BodyId>>(serde_json::json!(rows)).is_err());
    }
    assert!(BodyMembers::try_from_parts(vec![a], Vec::new()).is_err());
}

#[test]
fn historical_body_members_refuse_the_deleted_parallel_arrays() {
    let a: HistoricalBodyId = "a".try_into().unwrap();
    let b: HistoricalBodyId = "b".try_into().unwrap();
    for (bodies, native) in [
        (Vec::new(), Vec::new()),
        (vec![a.clone()], Vec::new()),
        (vec![a.clone(), a.clone()], vec!["a", "b"]),
        (vec![a.clone(), b.clone()], vec!["a", "a"]),
        (vec![a.clone()], vec![" "]),
    ] {
        assert!(BodyMembers::try_from_parts(
            bodies,
            native.into_iter().map(str::to_owned).collect(),
        )
        .is_err());
    }

    let members = BodyMembers::try_from_parts(
        vec![b.clone(), a.clone()],
        vec!["native-first".into(), "native-second".into()],
    )
    .unwrap();
    assert_eq!(members.bodies().collect::<Vec<_>>(), [&b, &a]);
    assert_eq!(
        members.native().collect::<Vec<_>>(),
        ["native-first", "native-second"]
    );
    let selection = BodySelection::HistoricalSet {
        state: "test:selection:feature_input_topology#state"
            .try_into()
            .unwrap(),
        members,
    };
    let wire = serde_json::to_value(&selection).unwrap();
    assert!(wire["value"].get("bodies").is_none());
    assert_eq!(
        serde_json::from_value::<BodySelection>(wire.clone()).unwrap(),
        selection
    );

    let mut restated = wire;
    let value = restated["value"].as_object_mut().unwrap();
    value.insert("bodies".into(), serde_json::json!(["b", "a"]));
    value.insert(
        "native".into(),
        serde_json::json!(["native-first", "native-second"]),
    );
    let error = serde_json::from_value::<BodySelection>(restated)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field `bodies`"), "{error}");
}
