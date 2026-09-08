// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::features::{BodyMembers, HistoricalUnorderedBodySelection};
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
fn unordered_members_reject_invalid_collections_without_pairing_their_orders() {
    let a: HistoricalBodyId = "a".try_into().unwrap();
    let b: HistoricalBodyId = "b".try_into().unwrap();
    for (bodies, native) in [
        (Vec::new(), Vec::new()),
        (vec![a.clone()], Vec::new()),
        (vec![a.clone(), a.clone()], vec!["a", "b"]),
        (vec![a.clone(), b.clone()], vec!["a", "a"]),
        (vec![a.clone()], vec![" "]),
    ] {
        let wire = serde_json::json!({"bodies": bodies, "native": native});
        assert!(HistoricalUnorderedBodySelection::try_from_parts(
            bodies,
            native.into_iter().map(str::to_owned).collect(),
        )
        .is_err());
        assert!(serde_json::from_value::<HistoricalUnorderedBodySelection>(wire).is_err());
    }
    let selection = HistoricalUnorderedBodySelection::try_from_parts(
        vec![b.clone(), a.clone()],
        vec!["native-first".into(), "native-second".into()],
    )
    .unwrap();
    assert_eq!(selection.bodies(), [b, a]);
    assert_eq!(selection.native(), ["native-first", "native-second"]);
    assert_eq!(
        serde_json::from_value::<HistoricalUnorderedBodySelection>(
            serde_json::to_value(&selection).unwrap(),
        )
        .unwrap(),
        selection,
    );
}
