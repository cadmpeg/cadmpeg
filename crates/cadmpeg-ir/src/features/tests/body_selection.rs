// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::features::{BodyMember, BodyMembers, BodySelection};
use crate::ids::{BodyId, HistoricalBodyId};

#[test]
fn ordered_members_reject_empty_sets_and_duplicates() {
    let a: BodyId = "test:selection:body#a".try_into().unwrap();
    let b: BodyId = "test:selection:body#b".try_into().unwrap();
    for (bodies, native) in [
        (Vec::new(), Vec::new()),
        (vec![a.clone(), a.clone()], vec!["a", "b"]),
        (vec![a, b], vec!["a", "a"]),
    ] {
        let json_rows = bodies
            .iter()
            .zip(&native)
            .map(|(body, native)| serde_json::json!({"body": body, "native": native}))
            .collect::<Vec<_>>();
        let rows = bodies
            .into_iter()
            .zip(native)
            .map(|(body, native)| {
                BodyMember::new(
                    body,
                    cadmpeg_core::text::NonBlankString::try_from(native.to_owned())
                        .expect("non-blank native fixture"),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            BodyMembers::try_from_rows(rows, &cadmpeg_test_support::service_decode_context())
                .expect("selection storage is admitted")
                .is_err()
        );
        assert!(
            serde_json::from_value::<BodyMembers<BodyId>>(serde_json::json!(json_rows)).is_err()
        );
    }
}

#[test]
fn historical_body_members_refuse_the_deleted_parallel_arrays() {
    let a: HistoricalBodyId = "a".try_into().unwrap();
    let b: HistoricalBodyId = "b".try_into().unwrap();
    for (bodies, native) in [
        (Vec::new(), Vec::new()),
        (vec![a.clone(), a.clone()], vec!["a", "b"]),
        (vec![a.clone(), b.clone()], vec!["a", "a"]),
    ] {
        let rows = bodies
            .into_iter()
            .zip(native)
            .map(|(body, native)| {
                BodyMember::new(
                    body,
                    cadmpeg_core::text::NonBlankString::try_from(native.to_owned())
                        .expect("non-blank native fixture"),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            BodyMembers::try_from_rows(rows, &cadmpeg_test_support::service_decode_context())
                .expect("selection storage is admitted")
                .is_err()
        );
    }
    assert!(cadmpeg_core::text::NonBlankString::try_from(" ").is_err());

    let members = BodyMembers::try_from_rows(
        vec![
            BodyMember::new(
                b.clone(),
                cadmpeg_core::text::NonBlankString::try_from("native-first").expect("non-blank fixture"),
            ),
            BodyMember::new(
                a.clone(),
                cadmpeg_core::text::NonBlankString::try_from("native-second")
                    .expect("non-blank fixture"),
            ),
        ],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection storage is admitted")
    .unwrap();
    assert_eq!(members.bodies().collect::<Vec<_>>(), [&b, &a]);
    assert_eq!(
        members
            .0
            .iter()
            .map(|row| row.native.as_str())
            .collect::<Vec<_>>(),
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

#[test]
fn body_member_constructor_charges_each_uniqueness_slot_once() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let body = crate::ids::BodyId::mint("test:model:body#member").unwrap();
    let native = cadmpeg_core::text::NonBlankString::try_from("native").unwrap();
    let rows =
        BodyMembers::try_from_rows(vec![crate::features::BodyMember::new(body, native)], &ctx)
            .unwrap()
            .unwrap();
    assert_eq!(rows.count(), 1);
    assert!(ctx
        .charge_collection_items(1, "test after body member indexes")
        .is_err());
}
