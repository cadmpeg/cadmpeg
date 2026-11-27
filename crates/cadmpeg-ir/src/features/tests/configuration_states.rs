use crate::features::geometry::FinitePoint3;
use crate::features::parameters::{ConfigurationEvaluation, ConfigurationFeatureState};
use crate::features::selections::DistinctMembers;
use crate::features::{FeatureDefinition, FeatureId, FeatureOperation};
use crate::ids::BodyId;
use crate::math::Point3;
use cadmpeg_core::serde_erase::json;

#[test]
fn configuration_output_members_are_distinct_and_empty_active_states_remain_valid() {
    let body = BodyId::mint("test:model:body#one").unwrap();
    assert!(DistinctMembers::try_from(vec![body.clone(), body.clone()],).is_err());
    for evaluation in [
        ConfigurationEvaluation::Suppressed {},
        ConfigurationEvaluation::Active {
            outputs: DistinctMembers::default(),
        },
        ConfigurationEvaluation::Active {
            outputs: crate::features::selections::DistinctMembers::try_from(vec![body.clone()])
                .unwrap(),
        },
    ] {
        let wire = json::to_value(&evaluation).unwrap();
        assert_eq!(
            json::from_value::<ConfigurationEvaluation>(wire).unwrap(),
            evaluation
        );
    }
    let empty = ConfigurationEvaluation::Active {
        outputs: DistinctMembers::default(),
    };
    assert_eq!(
        json::to_value(empty).unwrap(),
        cadmpeg_core::json!({"kind":"active", "outputs":[]})
    );
    let error = json::from_value::<ConfigurationEvaluation>(cadmpeg_core::json!({
        "kind":"active", "outputs":[body, body]
    }))
    .unwrap_err();
    assert!(error.to_string().contains("outputs"));
}

#[test]
fn configuration_dependencies_reject_duplicates_at_the_wire_boundary() {
    let earlier = FeatureId::mint("test:test:feature#earlier").unwrap();
    let state = ConfigurationFeatureState {
        evaluation: ConfigurationEvaluation::Active {
            outputs: DistinctMembers::default(),
        },
        dependencies: crate::features::selections::DistinctMembers::try_from(vec![earlier.clone()])
            .unwrap(),
        definition: FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            position: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            construction: None,
        }),
    };
    let mut wire = json::to_value(&state).unwrap();
    assert_eq!(
        json::from_value::<ConfigurationFeatureState>(wire.clone()).unwrap(),
        state
    );
    wire["dependencies"] = cadmpeg_core::json!([earlier, earlier]);
    let error = json::from_value::<ConfigurationFeatureState>(wire.clone()).unwrap_err();
    assert!(error.to_string().contains("dependencies"));
    wire.as_object_mut().unwrap().remove("dependencies");
    let error = json::from_value::<ConfigurationFeatureState>(wire).unwrap_err();
    assert!(error.to_string().contains("missing field `dependencies`"));
}
