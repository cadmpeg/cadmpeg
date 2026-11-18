use crate::design::components::ComponentInserts;
use crate::design::test_support::synthetic_feature_timelines;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::ReferenceRun;

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#30",
        DesignFeatureKind::ComponentInsert,
        30,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 385;
            draft.paired_byte_offset = 385;
            draft.reference_count_offset = 253;
            draft.reference_members =
                ReferenceRun::from_columns(vec![20], vec![258], "reference_members").unwrap();
            draft.kind_offset = 276;
            draft.feature_ordinal_offset = 308;
        })
        .unwrap();
    if let DesignScopePayloadMut::ComponentInsert(payload) = scope.payload_mut() {
        payload.unresolved_placement = Some(serde_json::from_slice(br#"{"occurrence_identity":19,"matrix":{"offset":46,"value":[[0.6,-0.8,0,1],[0.8,0.6,0,2],[0,0,1,3],[0,0,0,1]]}}"#).unwrap());
    }
    scope
}

#[test]
fn legacy_frame_assumption_is_reported_without_feature_projection() {
    let scope = scope();
    let scopes = std::slice::from_ref(&scope);
    let timelines = synthetic_feature_timelines(scopes);
    let inserts = ComponentInserts::decide(scopes, &timelines, &[]);
    let (_, occurrences) = inserts.unresolved_occurrences().unwrap();
    assert_eq!(occurrences.len(), 1);
    assert_eq!(
        occurrences[0].transform.unwrap().rows(),
        [
            [0.6, -0.8, 0.0, 10.0],
            [0.8, 0.6, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    assert_eq!(
        inserts
            .losses()
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [
            F3dLossCode::ComponentInsertLegacyFrameAssumed.kind(),
            F3dLossCode::ComponentInsertContainerAssumed.kind(),
        ]
    );
    assert!(inserts.losses()[0].message.contains(&scope.id));
    assert!(inserts.losses()[0].message.contains("DR-792"));
}

#[test]
fn legacy_frame_assumption_requires_a_projected_occurrence() {
    let mut omitted = scope();
    let listed = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#31",
        DesignFeatureKind::ComponentInsert,
        31,
    );
    let timelines = synthetic_feature_timelines(std::slice::from_ref(&listed));
    let inserts = ComponentInserts::decide(&[omitted.clone(), listed], &timelines, &[]);
    assert!(inserts.unresolved_occurrences().unwrap().1.is_empty());
    assert!(inserts.losses().is_empty());
    if let DesignScopePayloadMut::ComponentInsert(payload) = omitted.payload_mut() {
        payload.unresolved_placement = Some(serde_json::from_slice(br#"{"occurrence_identity":19,"matrix":{"offset":46,"value":[[1,0,0,1e308],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}}"#).unwrap());
    }
    let timelines = synthetic_feature_timelines(std::slice::from_ref(&omitted));
    let inserts = ComponentInserts::decide(&[omitted], &timelines, &[]);
    assert!(inserts.unresolved_occurrences().unwrap().1.is_empty());
    assert_eq!(
        inserts
            .losses()
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [F3dLossCode::ComponentInsertUnplaced.kind()]
    );
}
