use super::project_features;
use crate::design::components::ComponentInserts;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use cadmpeg_ir::features::selections::{DistinctMembers, InsertedComponents};
use cadmpeg_ir::features::{Feature, FeatureContent, FeatureDefinition, FeatureOperation};
use std::collections::BTreeMap;

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#30",
        DesignFeatureKind::ComponentInsert,
        30,
    );
    if let DesignScopePayloadMut::ComponentInsert(payload) = scope.payload_mut() {
        payload.unresolved_placement = Some(cadmpeg_core::serde_erase::json::from_slice(br#"{"occurrence_identity":19,"matrix":{"offset":54,"value":[[0,-1,0,2.5],[1,0,0,-4],[0,0,1,7],[0,0,0,1]]}}"#).unwrap());
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
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: Some("Component Insert".to_owned()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Component Insert".into(),
                parameters: BTreeMap::default(),
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
    let (definitions, occurrences) = decision.unresolved_occurrences().unwrap();
    assert_eq!(occurrences.len(), 1);
    let occurrence = &occurrences[0];
    assert!(decision.occurrence(&scope.id).is_none());
    // §8.1: the unresolved source is an External definition with no identity.
    assert_eq!(definitions.len(), 1);
    assert_eq!(occurrence.prototype, definitions[0].id);
    assert_eq!(
        definitions[0].storage,
        cadmpeg_ir::products::Storage::External(cadmpeg_ir::products::ExternalDocument::default())
    );
    assert_eq!(occurrence.native_ref.as_deref(), Some(scope.id.as_str()));
    assert_eq!(
        occurrence.transform.unwrap().rows(),
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
    let occurrences = decision.unresolved_occurrences().unwrap().1;
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

#[test]
fn legacy_partial_insert_projection_names_its_membership_assumption() {
    for frame in [256, 257, 261, 267, 385, 389, 399] {
        let mut scope = scope();
        scope
            .try_edit(|draft| {
                draft.frame_length = frame;
                draft.paired_byte_offset = frame;
                draft.feature_ordinal_offset = match frame {
                    256 | 257 | 267 => 180,
                    261 => 184,
                    385 => 308,
                    389 | 399 => 312,
                    _ => unreachable!(),
                };
                draft.kind_offset = draft.feature_ordinal_offset - 32;
                draft.reference_count_offset = match frame {
                    256 | 257 | 267 => 125,
                    261 => 129,
                    385 => 253,
                    389 | 399 => 257,
                    _ => unreachable!(),
                };
                draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                    vec![20],
                    vec![draft.reference_count_offset + 5],
                    "reference_members",
                )
                .unwrap();
            })
            .unwrap();
        if let DesignScopePayloadMut::ComponentInsert(payload) = scope.payload_mut() {
            payload.unresolved_placement = Some(if frame == 385 {
                cadmpeg_core::serde_erase::json::from_slice(br#"{"occurrence_identity":19,"matrix":{"offset":46,"value":[[0.6,-0.8,0,1],[0.8,0.6,0,2],[0,0,1,3],[0,0,0,1]]}}"#).unwrap()
            } else if frame >= 389 {
                cadmpeg_core::serde_erase::json::from_slice(br#"{"occurrence_identity":19,"matrix":{"offset":50,"value":[[0.6,-0.8,0,1],[0.8,0.6,0,2],[0,0,1,3],[0,0,0,1]]}}"#).unwrap()
            } else {
                cadmpeg_core::serde_erase::json::from_slice(br#"{"occurrence_identity":19}"#)
                    .unwrap()
            });
        }
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        let inserts = ComponentInserts::decide(scopes, &timelines, &[]);
        let (_, occurrences) = inserts.unresolved_occurrences().unwrap();
        assert_eq!(occurrences.len(), 1);
        let mut feature = feature(&scope);
        let losses =
            project_features(std::slice::from_mut(&mut feature), scopes, &occurrences).unwrap();
        assert_eq!(
            feature.evaluation.definition(),
            &FeatureDefinition::Operation(FeatureOperation::InsertComponent {
                occurrence: occurrences[0].id.clone()
            })
        );
        assert_eq!(
            losses.iter().map(|loss| &loss.code).collect::<Vec<_>>(),
            [&F3dLossCode::FeatureComponentInsertMembershipAssumed.kind(),]
        );
    }
}
