// SPDX-License-Identifier: Apache-2.0
use crate::records::feature::combine::{
    DesignCombineBodyNode, DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation,
    DesignCombineTools,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::FeatureId;

const STREAM: &str = "f3d:Design/BulkStream.dat";

#[allow(clippy::unnecessary_wraps)]
fn node(creating_scope_record_index: u32) -> Option<DesignCombineBodyNode> {
    Some(DesignCombineBodyNode {
        record_index: creating_scope_record_index + 1,
        body_entity_suffix: creating_scope_record_index + 2,
        creating_scope_record_index,
    })
}

fn scope(record_index: u32, state: i64) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{record_index}"),
        DesignFeatureKind::BaseFeature,
        record_index,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(state);
            draft.previous_history_state_id = Some(state - 1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn topology(bodies: &[i64]) -> crate::history_records::AsmHistoricalTopology {
    crate::history_records::AsmHistoricalTopology {
        bodies: bodies.to_vec(),
        ..Default::default()
    }
}

fn history(states: &[&[i64]]) -> crate::history_records::AsmHistory {
    let rows = states
        .iter()
        .enumerate()
        .rev()
        .map(|(id, bodies)| {
            let id = i64::try_from(id).unwrap();
            (
                crate::history_records::AsmDeltaState {
                    id: format!("f3d:asm-delta-state#{id}"),
                    parent: "f3d:asm-history#1".into(),
                    byte_offset: 0,
                    state_id: id,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    next_ref: (id > 0).then_some(id - 1),
                    node_index: id,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(topology(bodies)),
                Vec::new(),
            )
        })
        .collect();
    crate::test_support::history::history("f3d:asm-history#1".into(), 0, None, false, rows)
}

fn operation(
    target: Option<DesignCombineBodyNode>,
    tools: &[Option<DesignCombineBodyNode>],
) -> DesignCombineOperation {
    let mut selections =
        tools
            .iter()
            .zip(12u32..)
            .map(|(body_node, record_index)| DesignCombineBodySelection {
                record_index,
                external_identity: None,
                legacy_external_identity: None,
                body_node: *body_node,
            });
    DesignCombineOperation {
        form: DesignCombineForm::OperandLists,
        operation: cadmpeg_ir::features::operation_settings::BooleanKind::Join,
        operation_offset: 21,
        keep_tools: false,
        keep_tools_offset: 29,
        target_record_index: 11,
        target_body_node: target,
        tools: DesignCombineTools {
            first: selections.next().unwrap(),
            additional: selections.collect(),
        },

        destination: Default::default(),
        created_body_nodes: Vec::new(),
        consumed_body_nodes: Vec::new(),
    }
}

fn bind(
    operation: DesignCombineOperation,
    history: &crate::history_records::AsmHistory,
    scopes: &[DesignParameterScope],
) -> (BodySelection, BodySelection) {
    bind_with_losses(operation, history, scopes).0
}

fn bind_with_losses(
    operation: DesignCombineOperation,
    history: &crate::history_records::AsmHistory,
    scopes: &[DesignParameterScope],
) -> (
    (BodySelection, BodySelection),
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let (selections, losses, _) = bind_with_outcome(operation, history, scopes);
    (selections, losses)
}

fn bind_with_outcome(
    operation: DesignCombineOperation,
    history: &crate::history_records::AsmHistory,
    scopes: &[DesignParameterScope],
) -> (
    (BodySelection, BodySelection),
    Vec<cadmpeg_ir::report::loss::LossNote>,
    Option<bool>,
) {
    bind_with_scope_histories(
        operation,
        history,
        scopes,
        &std::collections::HashMap::new(),
    )
}

fn bind_with_scope_histories(
    operation: DesignCombineOperation,
    history: &crate::history_records::AsmHistory,
    scopes: &[DesignParameterScope],
    scope_histories: &std::collections::HashMap<String, String>,
) -> (
    (BodySelection, BodySelection),
    Vec<cadmpeg_ir::report::loss::LossNote>,
    Option<bool>,
) {
    let combine_state = i64::try_from(history.states().len()).unwrap() - 1;
    let combine = scope(400, combine_state);
    let mut target = BodySelection::Native(format!("{STREAM}:design-record#11"));
    let mut tools = BodySelection::Native(format!("{STREAM}:design-record#12"));
    let mut refusals = Vec::new();
    let shared_body = super::super::super::body_selections::bind_operand_list_combine_bodies(
        (&mut target, &mut tools),
        &combine,
        &operation,
        (
            &FeatureId::mint("test:model:feature#combine").unwrap(),
            combine_state - 1,
        ),
        (std::slice::from_ref(history), scopes, scope_histories),
        &mut refusals,
    );
    let losses = refusals
        .iter()
        .map(crate::history::body_selections::CombineOperandRefusal::note)
        .collect();
    ((target, tools), losses, shared_body)
}

fn input_body_slots(selection: &BodySelection) -> Vec<i64> {
    let ids: Vec<String> = match selection {
        BodySelection::Historical { bodies, .. } => {
            bodies.iter().map(ToString::to_string).collect()
        }
        BodySelection::HistoricalSet { members, .. } => {
            members.iter().map(|row| row.body().to_string()).collect()
        }
        other => panic!("expected a historical selection, got {other:?}"),
    };
    ids.iter()
        .map(|id| crate::history::body_selections::historical_body_slot(id).unwrap())
        .collect()
}

fn creators() -> Vec<DesignParameterScope> {
    vec![scope(100, 1), scope(200, 2), scope(300, 3)]
}

fn assert_native(selections: &(BodySelection, BodySelection)) {
    assert!(
        matches!(selections.0, BodySelection::Native(_)),
        "{selections:?}"
    );
    assert!(
        matches!(selections.1, BodySelection::Native(_)),
        "{selections:?}"
    );
}

#[test]
fn slots_come_from_the_creating_scopes_in_tool_order() {
    let history = history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[2]]);
    let (target, tools) = bind(
        operation(node(200), &[node(300), node(100)]),
        &history,
        &creators(),
    );
    assert_eq!(input_body_slots(&target), [2]);
    assert_eq!(input_body_slots(&tools), [3, 1]);
}

#[test]
fn tools_that_name_one_browser_node_select_one_body() {
    let history = history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[2]]);
    let ((target, tools), losses) = bind_with_losses(
        operation(node(200), &[node(300), node(100), node(300)]),
        &history,
        &creators(),
    );
    assert_eq!(input_body_slots(&target), [2]);
    assert_eq!(input_body_slots(&tools), [3, 1]);
    assert_eq!(operand_refusals(&losses), 0);
}

fn assert_target_only(selections: &(BodySelection, BodySelection), slot: i64) {
    assert_eq!(input_body_slots(&selections.0), [slot], "{selections:?}");
    assert!(
        matches!(selections.1, BodySelection::Native(_)),
        "{selections:?}"
    );
}

fn operand_refusals(losses: &[cadmpeg_ir::report::loss::LossNote]) -> usize {
    losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::F3dLossCode::FeatureCombineOperandBodyUnbound.kind()
        })
        .count()
}

#[test]
fn the_target_and_the_tools_bind_independently() {
    let history = history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[2]]);
    for (target, tools, target_slot) in [
        (node(200), vec![None, node(100)], 2),
        (node(200), vec![node(300), None], 2),
        (node(200), vec![node(300), node(999)], 2),
    ] {
        let (selections, losses, shared_body) =
            bind_with_outcome(operation(target, &tools), &history, &creators());
        assert_target_only(&selections, target_slot);
        assert_eq!(operand_refusals(&losses), 1);
        assert_ne!(shared_body, Some(true));
    }
    for target in [None, node(999)] {
        let ((target, tools), losses) = bind_with_losses(
            operation(target, &[node(300), node(100)]),
            &history,
            &creators(),
        );
        assert!(matches!(target, BodySelection::Native(_)), "{target:?}");
        assert_eq!(input_body_slots(&tools), [3, 1]);
        assert_eq!(operand_refusals(&losses), 1);
    }
}

#[test]
fn a_tool_that_names_the_target_body_leaves_both_selections_native() {
    let history = history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[2]]);
    for tools in [vec![node(200), node(100)], vec![node(300), node(200)]] {
        let (selections, losses, shared_body) =
            bind_with_outcome(operation(node(200), &tools), &history, &creators());
        assert_native(&selections);
        assert_eq!(operand_refusals(&losses), 1);
        assert_eq!(shared_body, Some(true));
    }
}

#[test]
fn a_tool_whose_creator_reads_another_history_leaves_only_the_tools_native() {
    let history = history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[2]]);
    let other = scope(250, 2);
    let mut scopes = creators();
    scopes.push(other.clone());
    let scope_histories =
        std::collections::HashMap::from([(other.id.clone(), "f3d:asm-history#2".to_owned())]);
    let (selections, losses, shared_body) = bind_with_scope_histories(
        operation(node(200), &[node(250)]),
        &history,
        &scopes,
        &scope_histories,
    );
    assert_target_only(&selections, 2);
    assert_eq!(operand_refusals(&losses), 1);
    assert!(
        losses[0].message.contains("reads another history"),
        "{losses:?}"
    );
    assert_ne!(shared_body, Some(true));
    let (selections, _, shared_body) =
        bind_with_outcome(operation(node(200), &[node(250)]), &history, &scopes);
    assert_native(&selections);
    assert_eq!(shared_body, Some(true));
}

#[test]
fn a_creator_must_insert_one_body_that_reaches_the_input_state() {
    let two_inserted = history(&[&[], &[1], &[1, 2, 3], &[1, 2, 3, 4], &[1]]);
    assert_target_only(
        &bind(
            operation(node(100), &[node(200)]),
            &two_inserted,
            &creators(),
        ),
        1,
    );

    let removed_before_input = history(&[&[], &[1], &[1, 2], &[1], &[1]]);
    assert_target_only(
        &bind(
            operation(node(100), &[node(200)]),
            &removed_before_input,
            &creators(),
        ),
        1,
    );

    let mut no_previous_state = creators();
    no_previous_state[1]
        .try_edit(|draft| {
            draft.previous_history_state_id = None;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (target, tools) = bind(
        operation(node(100), &[node(200)]),
        &history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[1]]),
        &no_previous_state,
    );
    assert_eq!(input_body_slots(&target), [1]);
    assert_eq!(input_body_slots(&tools), [2]);

    let scope_in_other_stream = {
        let mut scopes = creators();
        scopes[1] = DesignParameterScope::empty(
            "f3d:Other/BulkStream.dat:design-parameter-scope#200",
            DesignFeatureKind::BaseFeature,
            200,
        );
        scopes[1]
            .try_edit(|draft| {
                draft.history_state_id = Some(2);
                draft.previous_history_state_id = Some(1);
                draft.layout_fixture_tail();
            })
            .unwrap();
        scopes
    };
    let ordinary = history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[2]]);
    assert_target_only(
        &bind(
            operation(node(100), &[node(200)]),
            &ordinary,
            &scope_in_other_stream,
        ),
        1,
    );
}
