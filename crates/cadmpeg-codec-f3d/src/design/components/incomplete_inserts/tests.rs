use super::project_features;
use crate::design::components::ComponentInserts;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, InsertedComponents};

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#30",
        DesignFeatureKind::ComponentInsert,
        30,
    );
    if let DesignScopePayloadMut::ComponentInsert(payload) = scope.payload_mut() {
        payload.unresolved_placement = Some(serde_json::from_slice(br#"{"occurrence_identity":19,"matrix":{"offset":54,"value":[[0,-1,0,2.5],[1,0,0,-4],[0,0,1,7],[0,0,0,1]]}}"#).unwrap());
    }
    scope
}

fn feature(scope: &DesignParameterScope) -> Feature {
    Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#insert").unwrap(),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Component Insert".to_owned()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Component Insert".into(),
                parameters: Default::default(),
            }),
        ),
        native_ref: Some(scope.id.clone()),
    }
}

#[test]
fn authored_partial_insert_retains_placement_and_an_unresolved_source() {
    let scope = scope();
    let scopes = std::slice::from_ref(&scope);
    let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
    let decision = ComponentInserts::decide(scopes, &timelines, &[]);
    let occurrences = decision.unresolved_occurrences(0).unwrap();
    assert_eq!(occurrences.len(), 1);
    let occurrence = &occurrences[0];
    assert!(decision.occurrence(&scope.id).is_none());
    assert!(matches!(
        occurrence.prototype,
        cadmpeg_ir::products::PrototypeReference::Unresolved {}
    ));
    assert!(matches!(
        occurrence.parent,
        cadmpeg_ir::products::OccurrenceParent::Root {}
    ));
    assert_eq!(occurrence.native_ref.as_deref(), Some(scope.id.as_str()));
    assert_eq!(
        occurrence.transform.rows(),
        [
            [0.0, -1.0, 0.0, 25.0],
            [1.0, 0.0, 0.0, -40.0],
            [0.0, 0.0, 1.0, 70.0],
            [0.0, 0.0, 0.0, 1.0]
        ]
    );
    assert_eq!(
        decision
            .losses()
            .iter()
            .map(|loss| &loss.code)
            .collect::<Vec<_>>(),
        [&F3dLossCode::ComponentInsertContainerAssumed.kind()]
    );
    let mut feature = feature(&scope);
    let losses =
        project_features(std::slice::from_mut(&mut feature), scopes, &occurrences).unwrap();
    assert_eq!(
        feature.evaluation.definition(),
        &FeatureDefinition::Operation(FeatureOperation::InsertComponent {
            occurrence: occurrence.id.clone()
        })
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureComponentInsertMembershipAssumed.kind()
    );
}

#[test]
fn an_insert_without_a_projected_occurrence_retains_native_membership() {
    let scope = scope();
    let mut feature = feature(&scope);
    let losses = project_features(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &[],
    )
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::InsertComponents {
        components: InsertedComponents::Native { native },
    }) = feature.evaluation.definition()
    else {
        panic!("expected unresolved insertion membership")
    };
    assert_eq!(
        native.as_str(),
        "f3d:Design/BulkStream.dat:design-parameter-scope#30"
    );
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureComponentInsertMembershipAssumed.kind()
    );
    assert!(project_features(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &[]
    )
    .unwrap()
    .is_empty());
}

#[test]
fn a_scope_without_an_admitted_placement_keeps_its_current_definition() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#40",
        DesignFeatureKind::ComponentInsert,
        40,
    );
    let mut feature = feature(&scope);
    let before = feature.clone();
    assert!(
        project_features(std::slice::from_mut(&mut feature), &[scope], &[])
            .unwrap()
            .is_empty()
    );
    assert_eq!(feature, before);
}

#[test]
fn repeated_native_tokens_keep_per_scope_occurrences_with_membership_assumptions() {
    let first = scope();
    let mut second = first.clone();
    second.id = "f3d:Design/BulkStream.dat:design-parameter-scope#31".to_owned();
    second
        .try_edit(|draft| {
            draft.record_index = 31;
            draft.feature_ordinal = std::num::NonZeroU32::new(2).unwrap();
        })
        .unwrap();
    let scopes = [first, second];
    let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
    let decision = ComponentInserts::decide(&scopes, &timelines, &[]);
    let occurrences = decision.unresolved_occurrences(0).unwrap();
    assert_eq!(occurrences.len(), 2);
    assert_ne!(occurrences[0].id, occurrences[1].id);
    for (scope, occurrence) in scopes.iter().zip(occurrences) {
        let mut feature = feature(scope);
        let losses = project_features(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(scope),
            &[occurrence],
        )
        .unwrap();
        assert_eq!(
            losses[0].code,
            F3dLossCode::FeatureComponentInsertMembershipAssumed.kind()
        );
        assert!(losses[0].message.contains("per-scope membership assumed"));
    }
}
