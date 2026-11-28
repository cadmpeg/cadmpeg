// SPDX-License-Identifier: Apache-2.0

use crate::features::selections::{BodyMember, BodyMembers, BodySelection};
use crate::ids::{BodyId, HistoricalBodyId};
use cadmpeg_core::serde_erase::json;

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
            .map(|(body, native)| cadmpeg_core::json!({"body": body, "native": native}))
            .collect::<Vec<_>>();
        let rows = bodies
            .into_iter()
            .zip(native)
            .map(|(body, native)| {
                BodyMember::new(
                    body,
                    cadmpeg_core::text::NonBlankString::new(native.to_owned())
                        .expect("non-blank native fixture"),
                )
            })
            .collect::<Vec<_>>();
        assert!(BodyMembers::try_from_rows(rows).is_err());
        assert!(json::from_value::<BodyMembers<BodyId>>(cadmpeg_core::json!(json_rows)).is_err());
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
                    cadmpeg_core::text::NonBlankString::new(native.to_owned())
                        .expect("non-blank native fixture"),
                )
            })
            .collect::<Vec<_>>();
        assert!(BodyMembers::try_from_rows(rows).is_err());
    }
    assert!(cadmpeg_core::text::NonBlankString::new(" ").is_none());

    let members = BodyMembers::try_from_rows(vec![
        BodyMember::new(
            b.clone(),
            cadmpeg_core::text::NonBlankString::new("native-first").expect("non-blank fixture"),
        ),
        BodyMember::new(
            a.clone(),
            cadmpeg_core::text::NonBlankString::new("native-second").expect("non-blank fixture"),
        ),
    ])
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
    let wire = json::to_value(&selection).unwrap();
    assert!(wire["value"].get("bodies").is_none());
    assert_eq!(
        json::from_value::<BodySelection>(wire.clone()).unwrap(),
        selection
    );

    let mut restated = wire;
    let value = restated["value"].as_object_mut().unwrap();
    value.insert("bodies".into(), cadmpeg_core::json!(["b", "a"]));
    value.insert(
        "native".into(),
        cadmpeg_core::json!(["native-first", "native-second"]),
    );
    let error = json::from_value::<BodySelection>(restated)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field `bodies`"), "{error}");
}

#[test]
fn generated_body_members_keep_one_native_per_row() {
    use crate::features::selections::GeneratedBodyRef;
    use crate::features::FeatureId;

    let feature = FeatureId::mint("test:selection:feature#producer").unwrap();
    let first = GeneratedBodyRef::new(feature.clone(), "1".into()).unwrap();
    let second = GeneratedBodyRef::new(feature, "2".into()).unwrap();
    let row = |body: &GeneratedBodyRef, native: &str| {
        BodyMember::new(
            body.clone(),
            cadmpeg_core::text::NonBlankString::new(native.to_owned())
                .expect("non-blank native fixture"),
        )
    };
    assert!(BodyMembers::try_from_rows(vec![row(&first, "a"), row(&first, "b")]).is_err());
    assert!(BodyMembers::try_from_rows(vec![row(&first, "a"), row(&second, "a")]).is_err());

    let selection = BodySelection::GeneratedSet {
        members: BodyMembers::try_from_rows(vec![row(&first, "a"), row(&second, "b")]).unwrap(),
    };
    let wire = json::to_value(&selection).unwrap();
    assert_eq!(wire["kind"], "generated_set");
    assert_eq!(wire["value"]["members"][1]["body"]["local_id"], "2");
    assert_eq!(wire["value"]["members"][1]["native"], "b");
    assert_eq!(
        json::from_value::<BodySelection>(wire.clone()).unwrap(),
        selection
    );
    assert!(crate::features::SewBodySelection::try_from(selection).is_ok());

    let mut restated = wire;
    restated["value"]
        .as_object_mut()
        .unwrap()
        .insert("native".into(), cadmpeg_core::json!("a"));
    let error = json::from_value::<BodySelection>(restated)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field `native`"), "{error}");
}

#[test]
fn occurrence_body_selection_names_its_occurrence_and_distinct_bodies() {
    use crate::ids::OccurrenceId;

    let wire = cadmpeg_core::json!({
        "kind": "occurrence",
        "value": {
            "occurrence": "test:selection:occurrence#placed",
            "occurrence_path": [],
            "bodies": ["test:selection:body#a"],
            "native": "native"
        }
    });
    let selection: BodySelection = json::from_value(wire.clone()).unwrap();
    let BodySelection::Occurrence {
        occurrence, bodies, ..
    } = &selection
    else {
        panic!("occurrence body selection");
    };
    assert_eq!(
        occurrence,
        &OccurrenceId::mint("test:selection:occurrence#placed").unwrap()
    );
    assert_eq!(bodies.len(), 1);
    assert_eq!(json::to_value(&selection).unwrap(), wire);

    let mut repeated = wire;
    repeated["value"]["bodies"] =
        cadmpeg_core::json!(["test:selection:body#a", "test:selection:body#a"]);
    assert!(json::from_value::<BodySelection>(repeated).is_err());
}
