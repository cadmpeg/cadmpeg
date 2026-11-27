// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;

#[test]
fn component_insertions_round_trip_native_partial_and_complete_membership() {
    use crate::features::selections::InsertedComponents;
    use crate::features::{FeatureDefinition, FeatureOperation};
    use crate::ids::OccurrenceId;
    let first = OccurrenceId::mint("fixture:model:occurrence#1").unwrap();
    let second = OccurrenceId::mint("fixture:model:occurrence#2").unwrap();
    let cases = [
        (
            InsertedComponents::native("fixture:scope#10".into()).unwrap(),
            cadmpeg_core::json!({"kind":"native","native":"fixture:scope#10"}),
            false,
            0,
        ),
        (
            InsertedComponents::partial(vec![first.clone()], "fixture:scope#10".into()).unwrap(),
            cadmpeg_core::json!({"kind":"partial","native":"fixture:scope#10","occurrences":["fixture:model:occurrence#1"]}),
            false,
            1,
        ),
        (
            InsertedComponents::resolved(vec![second, first], "fixture:scope#10".into()).unwrap(),
            cadmpeg_core::json!({"kind":"resolved","native":"fixture:scope#10","occurrences":["fixture:model:occurrence#2","fixture:model:occurrence#1"]}),
            true,
            2,
        ),
    ];
    for (components, expected, complete, known) in cases {
        assert_eq!(components.is_resolved(), complete);
        assert_eq!(components.occurrences().len(), known);
        assert_eq!(components.native_reference().as_str(), "fixture:scope#10");
        let definition =
            FeatureDefinition::Operation(FeatureOperation::InsertComponents { components });
        let wire = json::to_value(&definition).unwrap();
        assert_eq!(
            wire,
            cadmpeg_core::json!({"kind":"operation","value":{"kind":"insert_components","value":{"components":expected}}})
        );
        assert_eq!(
            json::from_value::<FeatureDefinition>(wire).unwrap(),
            definition
        );
    }
}

#[test]
fn component_insertions_reject_blank_empty_and_repeated_membership() {
    use crate::features::selections::{InsertedComponents, InsertedComponentsError};
    use crate::ids::OccurrenceId;
    let occurrence = OccurrenceId::mint("fixture:model:occurrence#1").unwrap();
    assert_eq!(
        InsertedComponents::native(" \n".into()),
        Err(InsertedComponentsError::BlankNative)
    );
    assert_eq!(
        InsertedComponents::resolved(Vec::new(), "fixture:scope#10".into()),
        Err(InsertedComponentsError::Empty)
    );
    assert_eq!(
        InsertedComponents::partial(Vec::new(), "fixture:scope#10".into()),
        Err(InsertedComponentsError::Empty)
    );
    assert_eq!(
        InsertedComponents::partial(
            vec![occurrence.clone(), occurrence],
            "fixture:scope#10".into()
        ),
        Err(InsertedComponentsError::RepeatedOccurrence)
    );
    for wire in [
        r#"{"kind":"native","native":" "}"#,
        r#"{"kind":"resolved","native":"fixture:scope#10","occurrences":[]}"#,
        r#"{"kind":"partial","native":"fixture:scope#10","occurrences":["fixture:model:occurrence#1","fixture:model:occurrence#1"]}"#,
        r#"{"kind":"native","native":"fixture:scope#10","occurrences":["fixture:model:occurrence#1"]}"#,
    ] {
        assert!(serde_json::from_str::<InsertedComponents>(wire).is_err());
    }
}
