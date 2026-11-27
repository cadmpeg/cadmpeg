// SPDX-License-Identifier: Apache-2.0

use super::{bodies_topology, native_base_feature, state};
use crate::history::feature_outputs::bind_feature_outputs;
use crate::history_records::AsmHistory;
use crate::loss::F3dLossCode;
use std::collections::HashMap;

type Bound = (Vec<cadmpeg_ir::ids::BodyId>, Vec<String>);

/// A scope that stores state 1 and no previous state, its feature, the
/// history that holds states 1 and 0, and the active body of its output.
fn unstored_previous() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    let (feature, mut scope, history, body) = native_base_feature();
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = None;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (feature, scope, history, body)
}

/// A second history that holds state 1 alone and links it to no state.
fn abstaining_tail() -> AsmHistory {
    crate::test_support::history::history(
        "f3d:asm-history#2".into(),
        0,
        None,
        false,
        vec![state(1, 1, None, bodies_topology(&[1]))],
    )
}

/// A second history that holds state 1 alone and links it to `previous`.
fn linked_tail(previous: i64) -> AsmHistory {
    let mut tail = abstaining_tail();
    tail.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(previous);
    tail
}

fn bind(
    feature: &cadmpeg_ir::features::Feature,
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &mut [AsmHistory],
    body: &cadmpeg_ir::topology::Body,
    scope_histories: &HashMap<String, String>,
) -> Bound {
    let mut feature = feature.clone();
    let losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(scope),
        histories,
        std::slice::from_ref(body),
        scope_histories,
        None,
    )
    .unwrap()
    .losses;
    let charged = losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::FeatureOutputStateChainUnbound.kind())
        .map(|loss| loss.message.clone())
        .collect();
    (feature.evaluation.outputs().clone(), charged)
}

fn unlinked(feature: &cadmpeg_ir::features::Feature, scope: &str, reason: &str) -> String {
    format!(
        "{} has state 1 of scope {scope} in the body history, and {reason}.",
        feature.id
    )
}

fn project_scopes(
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    item: u64,
    body_bindings: &[crate::records::bodies::DesignBodyBinding],
) -> Vec<cadmpeg_ir::features::Feature> {
    use crate::design::feature_project::{
        project_parameter_design_with_edge_identities, ProjectInputs,
    };
    use crate::records::entity_header::{DesignFeatureTimeline, DesignTimelineFrame};
    use crate::records::identity::Located;
    use std::num::NonZeroU64;

    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream("f3d:Design/BulkStream.dat", 0),
        DesignTimelineFrame::test_items(
            0,
            vec![Located {
                value: item,
                offset: 0,
            }],
        ),
        "256".to_owned().try_into().unwrap(),
        NonZeroU64::new(1).unwrap(),
        0,
        NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                scopes,
                timelines: std::slice::from_ref(&timeline),
                body_bindings,
                ..Default::default()
            },
        )
    })
    .unwrap()
    .features
}

/// The previous state of a scope that stores none is the state to which its
/// bound history links its state, else the one state to which the histories
/// that hold its state link it. A history that links the state to no state
/// abstains. The feature binds its output through that pair. Histories that
/// link the state to different states, a history that links it to two, and
/// histories that link it to none charge the binding with that reason. When
/// no history holds the state, nothing is charged.
#[test]
fn a_held_state_binds_through_the_previous_state_its_histories_link() {
    let (feature, scope, history, body) = unstored_previous();
    let none = HashMap::new();
    let bound_to = |id: &str| HashMap::from([(scope.id.clone(), id.to_owned())]);
    let bound = (vec![body.id.clone()], Vec::new());
    assert_eq!(
        bind(&feature, &scope, &mut [history.clone()], &body, &none),
        bound
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), linked_tail(0)],
            &body,
            &none
        ),
        bound,
        "histories that agree on the previous state"
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), abstaining_tail()],
            &body,
            &none
        ),
        bound,
        "a history that links the state to no state abstains"
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), linked_tail(7)],
            &body,
            &bound_to(&history.id),
        ),
        bound,
        "the scope's bound history"
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), abstaining_tail()],
            &body,
            &bound_to("f3d:asm-history#2"),
        ),
        (
            Vec::new(),
            vec![format!(
                "{} has no complete unique output transition chain from state 0 to state 1.",
                feature.id
            )]
        ),
        "a bound history that links the state to no state abstains"
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), linked_tail(7)],
            &body,
            &none
        ),
        (
            Vec::new(),
            vec![unlinked(
                &feature,
                &scope.id,
                "the histories that hold it link it to previous states 0 and 7"
            )]
        ),
        "histories that disagree"
    );
    let mut contradicting = history.clone();
    contradicting.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(7);
    assert_eq!(
        bind(&feature, &scope, &mut [contradicting], &body, &none),
        (
            Vec::new(),
            vec![unlinked(
                &feature,
                &scope.id,
                "a history that holds it links it to previous state 7 by its transition and to previous state 0 by its header"
            )]
        ),
        "a history whose transition and header disagree"
    );
    let twice = crate::test_support::history::history(
        "f3d:asm-history#3".into(),
        0,
        None,
        false,
        vec![
            state(1, 2, None, bodies_topology(&[1])),
            state(1, 1, None, bodies_topology(&[1])),
            state(
                0,
                0,
                None,
                crate::history_records::AsmHistoricalTopology::default(),
            ),
        ],
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), twice],
            &body,
            &none
        ),
        (
            Vec::new(),
            vec![unlinked(&feature, &scope.id, "a history holds it twice")]
        ),
        "a history that holds the state twice"
    );
    assert_eq!(
        bind(&feature, &scope, &mut [abstaining_tail()], &body, &none),
        (
            Vec::new(),
            vec![unlinked(
                &feature,
                &scope.id,
                "no history that holds it links it to a previous state"
            )]
        ),
        "histories that link the state to no state"
    );
    assert_eq!(
        bind(&feature, &scope, &mut [], &body, &none),
        (Vec::new(), Vec::new())
    );
}

/// Histories that share the scope's bound history id are the histories the
/// scope reads first, so a history with another id that holds the state
/// twice does not take part.
#[test]
fn histories_that_share_the_bound_history_id_link_the_previous_state() {
    let (feature, scope, history, body) = unstored_previous();
    let mut namesake = abstaining_tail();
    namesake.id.clone_from(&history.id);
    let twice = crate::test_support::history::history(
        "f3d:asm-history#3".into(),
        0,
        None,
        false,
        vec![
            state(1, 2, None, bodies_topology(&[1])),
            state(1, 1, None, bodies_topology(&[1])),
            state(
                0,
                0,
                None,
                crate::history_records::AsmHistoricalTopology::default(),
            ),
        ],
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), namesake, twice],
            &body,
            &HashMap::from([(scope.id.clone(), history.id.clone())]),
        ),
        (
            Vec::new(),
            vec![format!(
                "{} has no complete unique output transition chain from state 0 to state 1.",
                feature.id
            )]
        )
    );
}

/// A derived base group produces the state of its last scope from the
/// previous state of its first scope, read by the same rule. The charge
/// names the first scope and its state.
#[test]
fn a_derived_base_group_reads_the_linked_previous_state_of_its_first_scope() {
    use crate::records::feature::derived_base::DesignDerivedBaseOwner;
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::identity::Located;

    let derived = |record_index, current, previous| {
        let mut scope = DesignParameterScope::empty(
            &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{record_index}"),
            DesignFeatureKind::native("Derived Base Feature"),
            record_index,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(current);
                draft.previous_history_state_id = previous;
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope.derived_base_owner = Some(DesignDerivedBaseOwner {
            record_index: Located {
                value: 50,
                offset: 0,
            },
            repeat_offset: 0,
            result_body: None,
        });
        scope
    };
    let scopes = [derived(70, 1, None), derived(79, 2, Some(1))];
    let [feature]: [cadmpeg_ir::features::Feature; 1] =
        project_scopes(&scopes, 50, &[]).try_into().unwrap();
    assert_eq!(feature.native_ref.as_deref(), Some(scopes[0].id.as_str()));
    let body = super::active_body();
    let mut second = body.clone();
    second.id = "f3d:brep:entity#2".try_into().unwrap();
    let bodies = [body, second];
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(2, 2, Some(1), bodies_topology(&[1, 2])),
            state(1, 1, Some(0), bodies_topology(&[1])),
            state(
                0,
                0,
                None,
                crate::history_records::AsmHistoricalTopology::default(),
            ),
        ],
    );
    let none = HashMap::new();
    let mut bound = feature.clone();
    let losses = bind_feature_outputs(
        std::slice::from_mut(&mut bound),
        &scopes,
        &mut [history.clone()],
        &bodies,
        &none,
        None,
    )
    .unwrap()
    .losses;
    assert_eq!(
        bound.evaluation.outputs().as_slice(),
        bodies.each_ref().map(|body| body.id.clone())
    );
    assert!(losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::FeatureOutputStateChainUnbound.kind()));

    let mut histories = [history, linked_tail(7)];
    let mut unbound = feature.clone();
    let losses = bind_feature_outputs(
        std::slice::from_mut(&mut unbound),
        &scopes,
        &mut histories,
        &bodies,
        &none,
        None,
    )
    .unwrap()
    .losses;
    assert!(unbound.evaluation.outputs().is_empty());
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::FeatureOutputStateChainUnbound.kind())
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>(),
        vec![unlinked(
            &feature,
            &scopes[0].id,
            "the histories that hold it link it to previous states 0 and 7"
        )]
    );
}

#[test]
fn a_stored_copy_paste_result_survives_an_interval_in_another_history() {
    use crate::records::bodies::{DesignBodyBinding, DesignBodyBindingWire};
    use crate::records::feature::body_ops::{
        CopyPasteRecordLocation, DesignCopiedBody, DesignCopyPasteBodiesOperation,
    };
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };
    use crate::records::identity::{Located, ReferenceRun};

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::CopyPasteBodies,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(1);
            draft.reference_members = ReferenceRun::located(vec![
                Located {
                    value: 501,
                    offset: 14,
                },
                Located {
                    value: 502,
                    offset: 25,
                },
            ]);
            draft.layout_fixture_references();
            draft.feature_ordinal_offset = draft.kind_offset + 30;
            draft.frame_length = draft.feature_ordinal_offset + 110;
            draft.paired_byte_offset = draft.frame_length;
        })
        .unwrap();
    let record = |record_index| CopyPasteRecordLocation {
        record_index,
        class_tag: "264".to_owned().try_into().unwrap(),
        byte_offset: 0,
    };
    let DesignScopePayloadMut::CopyPasteBodies(operation) = scope.payload_mut() else {
        panic!("CopyPasteBodies payload");
    };
    *operation = Some(
        DesignCopyPasteBodiesOperation::try_new(
            vec![DesignCopiedBody {
                operand: Located {
                    value: 502,
                    offset: 26,
                },
                source: Located {
                    value: 1,
                    offset: 25,
                },
                copied: Located {
                    value: 4,
                    offset: 40,
                },
            }],
            record(501),
            record(503),
        )
        .unwrap(),
    );
    let (_, _, mut inferred_history, mut inferred) = native_base_feature();
    inferred_history.id = "f3d:BREP.inferred:asm-history#0".into();
    inferred.id = "f3d:brep/inferred/brep:entity#1".try_into().unwrap();
    let mut copy = inferred.clone();
    copy.id = "f3d:brep/copied/brep:entity#1".try_into().unwrap();
    let mut copied_history = abstaining_tail();
    copied_history.id = "f3d:BREP.copied:asm-history#0".into();
    let binding = DesignBodyBinding::try_from(DesignBodyBindingWire {
        id: "f3d:Design/BulkStream.dat:design-body-binding#10".into(),
        stream: "Design/BulkStream.dat".into(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 1,
        asm_body_key_offset: 10,
        entity_suffix: 4,
        entity_suffix_offset: 18,
        blob_name: "BREP.copied".into(),
        blob_name_offset: 30,
        body: Some(copy.id.clone()),
        component_owner: None,
        component_owner_unresolved: false,
        history_only: false,
    })
    .unwrap();
    let scopes = [scope];
    let bindings = [binding];
    let projected = project_scopes(&scopes, 10, &bindings);
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].evaluation.outputs().as_slice(),
        std::slice::from_ref(&copy.id)
    );
    let bodies = [inferred, copy];
    let mut unread_transition = inferred_history.clone();
    unread_transition.states_mut()[0].transition = None;
    for history in [inferred_history, unread_transition] {
        let mut features = projected.clone();
        let mut histories = [history, copied_history.clone()];
        let scope_histories = crate::history::scope_histories::bind_scope_histories(
            &scopes,
            &bindings,
            &[],
            Default::default(),
            &histories,
            &mut Vec::new(),
        );
        assert!(scope_histories.is_empty());
        let result = bind_feature_outputs(
            &mut features,
            &scopes,
            &mut histories,
            &bodies,
            &scope_histories,
            None,
        )
        .unwrap();
        assert_eq!(
            features[0].evaluation.outputs().as_slice(),
            std::slice::from_ref(&bodies[1].id),
        );
        assert!(result.losses.iter().all(|loss| loss.code
            != F3dLossCode::FeatureOutputStateChainUnbound.kind()
            && loss.code != F3dLossCode::FeatureOutputBodyUnbound.kind()));
    }
}

/// A Base Feature whose stored bodies resolved takes those bodies as its
/// outputs after the history pass, and a Base Feature projected as inserted
/// copies takes the copies, so neither drops anything at an unlinked state.
#[test]
fn a_resolved_base_feature_with_an_unlinked_state_is_not_charged() {
    use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers, InsertedBodies};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = unstored_previous();
    let mut inserted = feature.clone();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: DistinctMembers::try_from(vec![body.id.clone()]).unwrap(),
                    native: scope.id.clone(),
                },
            },
        ));
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), linked_tail(7)],
            &body,
            &HashMap::new()
        ),
        (vec![body.id.clone()], Vec::new())
    );
    inserted
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::InsertBodies {
                bodies: InsertedBodies::Resolved {
                    native: scope.id.clone(),
                },
            },
        ));
    inserted
        .evaluation
        .set_outputs(DistinctMembers::try_from(vec![body.id.clone()]).unwrap());
    assert_eq!(
        bind(
            &inserted,
            &scope,
            &mut [history.clone(), linked_tail(7)],
            &body,
            &HashMap::new()
        ),
        (vec![body.id.clone()], Vec::new())
    );
}

#[test]
fn a_base_feature_with_an_owned_result_does_not_require_an_output_chain() {
    use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers, InsertedBodies};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = unstored_previous();
    let abstaining = abstaining_tail();
    let bound = HashMap::from([(scope.id.clone(), abstaining.id.clone())]);
    let outputs = DistinctMembers::try_from(vec![body.id.clone()]).unwrap();
    for (operation, initial_outputs) in [
        (
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: outputs.clone(),
                    native: scope.id.clone(),
                },
            },
            Default::default(),
        ),
        (
            FeatureOperation::InsertBodies {
                bodies: InsertedBodies::Resolved {
                    native: scope.id.clone(),
                },
            },
            outputs,
        ),
    ] {
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(operation));
        feature.evaluation.set_outputs(initial_outputs);
        assert_eq!(
            bind(
                &feature,
                &scope,
                &mut [history.clone(), abstaining.clone()],
                &body,
                &bound,
            ),
            (vec![body.id.clone()], Vec::new())
        );
    }
}
