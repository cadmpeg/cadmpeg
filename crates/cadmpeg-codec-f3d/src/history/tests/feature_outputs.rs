// SPDX-License-Identifier: Apache-2.0

fn active_body() -> cadmpeg_ir::topology::Body {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .model
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    body
}

fn base_feature() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    use cadmpeg_ir::ids::BodyId;
    Feature {
        id: FeatureId::mint("test:model:feature#feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Feature".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Native("native:scope".into()),
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(vec![
                BodyId::mint("test:model:body#2").unwrap(),
                BodyId::mint("test:model:body#1").unwrap(),
            ])
            .unwrap(),
        ),
        native_ref: Some("native:scope".into()),
    }
}

fn state(
    state_id: i64,
    node_index: i64,
    next_ref: Option<i64>,
    topology: crate::history_records::AsmHistoricalTopology,
) -> crate::test_support::history::State {
    (
        crate::history_records::AsmDeltaState {
            id: format!("f3d:asm-delta-state#{state_id}"),
            parent: "f3d:asm-history#1".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref,
            node_index,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(topology),
        Vec::new(),
    )
}

fn scope_between(
    state_id: i64,
    previous_state_id: i64,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{state_id}"),
        DesignFeatureKind::BaseFeature,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(state_id);
            draft.previous_history_state_id = Some(previous_state_id);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn bodies_topology(bodies: &[i64]) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    AsmHistoricalTopology {
        bodies: bodies.to_vec(),
        body_regions: bodies
            .iter()
            .map(|&owner_ref| AsmHistoricalRelation {
                owner_ref,
                member_refs: Vec::new(),
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    }
}

fn output_binding_inputs() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    use crate::history_records::AsmHistoricalTopology;
    let mut feature = base_feature();
    let scope = scope_between(1, 0);
    feature.native_ref = Some(scope.id.clone());
    let current_topology = bodies_topology(&[1]);
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), current_topology),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );
    (feature, scope, history, active_body())
}

#[test]
fn feature_outputs_name_the_bodies_a_state_deletes_from_its_previous_state() {
    use crate::history_records::AsmHistoricalTopology;
    let history = |states| {
        crate::test_support::history::history("f3d:asm-history#1".into(), 0, None, false, states)
    };
    let body = |slot: i64| {
        let mut body = active_body();
        body.id = cadmpeg_ir::ids::BodyId::mint(format!("f3d:brep:entity#{slot}")).unwrap();
        body
    };
    let bodies = [body(1), body(2), body(3)];
    let chain = || {
        history(vec![
            state(3, 3, Some(2), bodies_topology(&[3])),
            state(2, 2, Some(1), bodies_topology(&[1, 3])),
            state(1, 1, Some(0), bodies_topology(&[1, 2])),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ])
    };
    let expected = [(1, vec![1, 2]), (2, vec![2, 3]), (3, vec![1])];
    let feature_scope = |state_id| {
        let mut feature = base_feature();
        let scope = scope_between(state_id, state_id - 1);
        feature.native_ref = Some(scope.id.clone());
        (feature, scope)
    };
    let check = |feature: &cadmpeg_ir::features::Feature,
                 (state_id, expected): &(i64, Vec<i64>)| {
        let outputs = feature
            .evaluation
            .outputs()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>();
        let expected = expected
            .iter()
            .map(|slot| format!("f3d:brep:entity#{slot}"))
            .collect::<Vec<_>>();
        assert_eq!(outputs, expected, "state {state_id}");
    };
    for case in &expected {
        let (mut feature, scope) = feature_scope(case.0);
        super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            &[scope],
            &mut [chain()],
            &bodies,
        )
        .unwrap();
        check(&feature, case);
    }
    let (mut features, scopes): (Vec<_>, Vec<_>) = expected
        .iter()
        .map(|(state_id, _)| feature_scope(*state_id))
        .unzip();
    super::super::bind_feature_outputs(&mut features, &scopes, &mut [chain()], &bodies).unwrap();
    for (feature, case) in features.iter().zip(&expected) {
        check(feature, case);
    }
}

#[test]
fn feature_output_binding_preserves_base_feature_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = output_binding_inputs();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history],
        std::slice::from_ref(&body),
    )
    .unwrap();
    assert_eq!(feature.evaluation.outputs().len(), 1);
    assert_eq!(feature.evaluation.outputs()[0], body.id);
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, native }
        }) if bodies.as_slice() == [body.id] && native == "native:scope"));
}

fn native_base_feature() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    let (mut feature, scope, history, body) = output_binding_inputs();
    feature
        .evaluation
        .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native {
                kind: "Base Feature".into(),
                parameters: Default::default(),
            },
        ));
    feature.evaluation.set_outputs(Default::default());
    (feature, scope, history, body)
}

#[test]
fn stored_body_scope_uses_unique_complete_history_outputs() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = native_base_feature();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        std::slice::from_ref(&body),
    )
    .unwrap();
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, native }
        }) if bodies.as_slice() == [body.id] && native == &scope.id));
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()
    );
}

#[test]
fn stored_body_scope_refuses_missing_bodies_and_ambiguous_states() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    for refusal in ["missing body", "duplicate state", "ambiguous body"] {
        let (mut feature, scope, history, body) = native_base_feature();
        let mut histories = vec![history];
        let bodies = if refusal == "duplicate state" {
            let (_, _, duplicate, _) = native_base_feature();
            histories.push(duplicate);
            vec![body]
        } else if refusal == "ambiguous body" {
            let mut other = body.clone();
            other.id = cadmpeg_ir::ids::BodyId::mint("test:model:body#1").unwrap();
            vec![body, other]
        } else {
            Vec::new()
        };
        super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut histories,
            &bodies,
        )
        .unwrap();
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            F3dLossCode::FeatureStoredBodiesUnbound.kind()
        );
    }
}

#[test]
fn stored_body_scope_keeps_proven_empty_history_as_no_op() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, _, _) = native_base_feature();
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), bodies_topology(&[])),
            state(0, 0, None, bodies_topology(&[])),
        ],
    );
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
    )
    .unwrap();
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, .. }
        }) if bodies.is_empty()));
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::FeatureStoredBodiesEmpty.kind());
}

#[test]
fn stored_body_scope_does_not_type_derived_or_box_from_outputs() {
    use crate::loss::F3dLossCode;
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    for (kind, code) in [
        (
            DesignFeatureKind::try_from("Derived Base Feature".to_owned()).unwrap(),
            F3dLossCode::FeatureDerivedBaseUnresolved,
        ),
        (
            DesignFeatureKind::BoxPrimitive,
            F3dLossCode::FeatureBoxPrimitiveUnresolved,
        ),
    ] {
        let (mut feature, _, _, body) = native_base_feature();
        feature.evaluation.set_outputs(
            cadmpeg_ir::features::DistinctMembers::try_from(vec![body.id.clone()]).unwrap(),
        );
        let scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#9",
            kind,
            1,
        );
        feature.native_ref = Some(scope.id.clone());
        let active = [body];
        let index = super::super::stored_bodies::ActiveBodies::new(&active);
        assert!(!super::super::stored_bodies::bind_history_outputs(
            &mut feature,
            &scope,
            &[1],
            None,
            &index,
        ));
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, code.kind());
    }
}

#[test]
fn stored_body_scope_reports_recipe_absence_from_snapshot_kind() {
    use crate::loss::F3dLossCode;
    use crate::records::feature::scope::DesignScopePayloadMut;
    let (mut feature, mut scope, history, body) = native_base_feature();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        &[body],
    )
    .unwrap();
    let losses = super::super::stored_bodies::losses(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()
    );
    if let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
        *slot = Some(
            serde_json::from_value(serde_json::json!({
                "body_entity_suffixes":[81], "body_entity_suffix_offsets":[22],
                "body_reference_records":[81], "body_reference_record_offsets":[22],
                "parameter_body_record":91, "parameter_body_record_offset":100,
                "auxiliary_record":92, "auxiliary_record_offset":120,
                "envelope_guid":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
                "envelope_guid_offset":140, "tag_body_based_on_faces":true,
                "tag_body_based_on_faces_offset":90
            }))
            .unwrap(),
        );
    } else {
        panic!("Base Feature payload");
    }
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()
    );
}

#[test]
fn stored_body_scope_without_any_proof_keeps_native_definition() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, scope, _, _) = output_binding_inputs();
    feature.evaluation.set_outputs(Default::default());
    super::super::bind_feature_outputs(std::slice::from_mut(&mut feature), &[scope], &mut [], &[])
        .unwrap();
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
        if kind.as_str() == "Base Feature"));
}

fn history_in_source(source: &str, bodies: &[i64]) -> crate::history_records::AsmHistory {
    let stream = format!("f3d:asset/Breps.BlobParts/BREP.{source}");
    let id = format!("{stream}:asm-history#1");
    let mut states = vec![
        state(1, 1, Some(0), bodies_topology(bodies)),
        state(0, 0, None, bodies_topology(&[])),
    ];
    for (state, _, _) in &mut states {
        state.id = format!("{stream}:asm-delta-state#{}", state.state_id);
        state.parent = id.clone();
    }
    crate::test_support::history::history(id, 0, None, false, states)
}

#[test]
fn stored_body_scope_qualifies_body_slots_by_history_blob() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let mut first = active_body();
    first.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep/first.smbh/brep:entity#1").unwrap();
    let mut second = active_body();
    second.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep/second.smbh/brep:entity#1").unwrap();
    for (source, expected) in [("first.smbh", &first), ("second.smbh", &second)] {
        for bodies in [
            [first.clone(), second.clone()],
            [second.clone(), first.clone()],
        ] {
            let (mut feature, scope, _, _) = native_base_feature();
            super::super::bind_feature_outputs(
                std::slice::from_mut(&mut feature),
                std::slice::from_ref(&scope),
                &mut [history_in_source(source, &[1])],
                &bodies,
            )
            .unwrap();
            assert_eq!(feature.evaluation.outputs(), &[expected.id.clone()]);
            assert!(matches!(feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                    bodies: BodySelection::Resolved { bodies, .. }
                }) if bodies.as_slice() == [expected.id.clone()]));
        }
    }
}

#[test]
fn stored_body_scope_refuses_a_body_from_another_history_blob() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, scope, _, mut body) = native_base_feature();
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep/other.smbh/brep:entity#1").unwrap();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history_in_source("selected.smbh", &[1])],
        &[body],
    )
    .unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureStoredBodiesUnbound.kind()
    );
}

#[test]
fn stored_body_scope_refuses_a_declared_body_count_mismatch() {
    use crate::loss::F3dLossCode;
    use crate::records::feature::scope::DesignScopePayloadMut;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, mut scope, _, _) = native_base_feature();
    if let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
        *slot = Some(
            serde_json::from_value(serde_json::json!({
                "body_entity_suffixes":[81], "body_entity_suffix_offsets":[22],
                "body_reference_records":[81], "body_reference_record_offsets":[22],
                "parameter_body_record":91, "parameter_body_record_offset":100,
                "auxiliary_record":92, "auxiliary_record_offset":120,
                "envelope_guid":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
                "envelope_guid_offset":140, "tag_body_based_on_faces":true,
                "tag_body_based_on_faces_offset":90
            }))
            .unwrap(),
        );
    }
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history_in_source("selected.smbh", &[])],
        &[],
    )
    .unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureStoredBodiesUnbound.kind()
    );
}

#[test]
fn stored_body_scope_binds_explicit_body_members_without_history() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, _, body) = output_binding_inputs();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: cadmpeg_ir::features::DistinctMembers::try_from(vec![body.id.clone()])
                        .unwrap(),
                    native: scope.id.clone(),
                },
            },
        ));
    feature.evaluation.set_outputs(Default::default());
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [],
        std::slice::from_ref(&body),
    )
    .unwrap();
    assert_eq!(feature.evaluation.outputs(), &[body.id]);
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { .. }
        })
    ));
}
