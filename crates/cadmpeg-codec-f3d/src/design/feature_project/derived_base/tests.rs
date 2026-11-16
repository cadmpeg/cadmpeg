// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::derived_base::{
    derived_base_scopes_property, DesignDerivedBaseSnapshot, DesignDerivedBaseSource,
};
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::identity::Located;
use cadmpeg_ir::features::{DistinctMembers, FeatureContent, FeatureEvaluation};
use std::collections::BTreeMap;

fn fixture(captured: bool) -> (Feature, DesignParameterScope) {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1000",
        DesignFeatureKind::native("Derived Base Feature"),
        100,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 1000;
            draft.history_state_id = Some(92);
            draft.previous_history_state_id = Some(2);
            draft.reference_count_offset = 1421;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.kind_offset + 160;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.derived_base_source = Some(DesignDerivedBaseSource {
        parameter_body: Located {
            value: 31,
            offset: 1025,
        },
        result_body: Located {
            value: 41,
            offset: 1040,
        },
        owner_record: Located {
            value: 51,
            offset: 1060,
        },
        source_body: Located {
            value: 61,
            offset: 1075,
        },
        result_repeat_offset: 1086,
        snapshot: captured.then(|| DesignDerivedBaseSnapshot {
            history_body: cadmpeg_ir::ids::HistoryBodyId::mint("f3d:history-body:nest#h0000")
                .unwrap(),
            body_slot: 7,
        }),
    });
    let feature = Feature {
        product_definition: None,
        id: crate::ids::neutral_feature_id(&scope),
        ordinal: 4,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::from([(derived_base_scopes_property(), "100".into())]),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: "Derived Base Feature".into(),
                parameters: BTreeMap::new(),
            },
        )),
        native_ref: Some(scope.id.clone()),
    };
    (feature, scope)
}

#[test]
fn captured_result_is_the_base_feature_payload_in_its_committed_state() {
    let (mut feature, scope) = fixture(true);
    project(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &[],
    )
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::BaseFeature {
        bodies:
            BodySelection::Historical {
                state,
                bodies,
                native,
            },
    }) = feature.evaluation.definition()
    else {
        panic!("captured Base Feature")
    };
    assert!(state.as_str().ends_with(":92"));
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].as_str().ends_with(":92:7"));
    assert_eq!(native.as_str(), scope.id);
    assert!(feature.evaluation.outputs().is_empty());
    let losses = crate::history::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()
    );
}

#[test]
fn an_uncaptured_source_pair_keeps_its_native_operation() {
    let (mut feature, scope) = fixture(false);
    project(std::slice::from_mut(&mut feature), &[scope], &[]).unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
}

#[test]
fn source_pair_cadir_round_trip_checks_byte_locations() {
    let (_, scope) = fixture(true);
    let wire = serde_json::to_value(&scope).unwrap();
    let restored: DesignParameterScope = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(restored.derived_base_source, scope.derived_base_source);
    let mut displaced = wire;
    displaced["derived_base_source"]["source_body"]["offset"] = serde_json::json!(1076);
    assert!(serde_json::from_value::<DesignParameterScope>(displaced).is_err());
}
