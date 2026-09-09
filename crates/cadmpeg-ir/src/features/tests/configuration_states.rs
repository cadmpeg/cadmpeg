use crate::features::{
    ConfigurationEvaluation, ConfigurationFeatureState, DistinctMembers, FeatureDefinition,
    FeatureId, FinitePoint3,
};
use crate::ids::BodyId;
use crate::math::Point3;

#[test]
fn configuration_output_members_are_distinct_and_empty_active_states_remain_valid() {
    let body = BodyId::mint("test:model:body#one").unwrap();
    assert!(DistinctMembers::try_from(vec![body.clone(), body.clone()]).is_err());
    for evaluation in [
        ConfigurationEvaluation::Suppressed,
        ConfigurationEvaluation::Active {
            outputs: DistinctMembers::default(),
        },
        ConfigurationEvaluation::Active {
            outputs: vec![body.clone()].try_into().unwrap(),
        },
    ] {
        let wire = serde_json::to_value(&evaluation).unwrap();
        assert_eq!(
            serde_json::from_value::<ConfigurationEvaluation>(wire).unwrap(),
            evaluation
        );
    }
    let empty = ConfigurationEvaluation::Active {
        outputs: DistinctMembers::default(),
    };
    assert_eq!(
        serde_json::to_value(empty).unwrap(),
        serde_json::json!({"kind":"active"})
    );
    let error = serde_json::from_value::<ConfigurationEvaluation>(serde_json::json!({
        "kind":"active", "outputs":[body, body]
    }))
    .unwrap_err();
    assert!(error.to_string().contains("outputs"));
}

#[test]
fn configuration_dependencies_reject_duplicates_at_the_wire_boundary() {
    let earlier = FeatureId::mint("test:feature#earlier").unwrap();
    let state = ConfigurationFeatureState {
        evaluation: ConfigurationEvaluation::Active {
            outputs: DistinctMembers::default(),
        },
        dependencies: vec![earlier.clone()].try_into().unwrap(),
        definition: FeatureDefinition::DatumPoint {
            position: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            construction: None,
        },
    };
    let mut wire = serde_json::to_value(&state).unwrap();
    assert_eq!(
        serde_json::from_value::<ConfigurationFeatureState>(wire.clone()).unwrap(),
        state
    );
    wire["dependencies"] = serde_json::json!([earlier, earlier]);
    let error = serde_json::from_value::<ConfigurationFeatureState>(wire.clone()).unwrap_err();
    assert!(error.to_string().contains("dependencies"));
    wire.as_object_mut().unwrap().remove("dependencies");
    let decoded = serde_json::from_value::<ConfigurationFeatureState>(wire.clone()).unwrap();
    assert!(decoded.dependencies.is_empty());
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
}
