// SPDX-License-Identifier: Apache-2.0
//! Combine operands named by model bodies or by the features that generate
//! them.

use crate::history::body_selections::FeatureBodySelectionInputs;
use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::operation_settings::{BooleanKind, CombineDestination};
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{
    CombineOperands, Feature, FeatureDefinition, FeatureId, FeatureOperation,
};
use cadmpeg_ir::report::loss::LossNote;

const HISTORY: &str = "f3d:Breps.BlobParts/BREP.root.smbh:asm-history#1";
const FIRST: &str = "f3d:test:feature#first";
const SECOND: &str = "f3d:test:feature#second";
const COMBINE: &str = "f3d:test:feature#combine";
const TARGET_NATIVE: &str = "f3d:Design/BulkStream.dat:design-record#488";
const TOOL_NATIVE: &str = "f3d:Design/BulkStream.dat:design-record#481";
const SECOND_TOOL_NATIVE: &str = "f3d:Design/BulkStream.dat:design-record#482";
const ONE_TOOL: &[(i64, &str)] = &[(2, TOOL_NATIVE)];

/// History `HISTORY`, of BREP source `root.smbh`: feature `FIRST` (state 23
/// from 2) inserts slot 1, feature `SECOND` (state 27 from 23) inserts the
/// slots of `tools`, and the Combine (state 30 from 27) selects target slot 1
/// and the `tools` slots, each with its native member, in its input state 27
/// and keeps the bodies `after_combine`. One tool slot gives a historical
/// selection with one native expression. With
/// `second_updates_target`, the transition into state 27 also updates slot 1.
/// With `unscoped_update`, a state 25 from 23 that no scope names updates
/// slot 1. With `later_update`, a later transition (state 31 from 30) updates
/// slot 1.
fn combine_case(
    model_bodies: &[&str],
    tools: &[(i64, &str)],
    after_combine: Vec<i64>,
    second_updates_target: bool,
    unscoped_update: bool,
    later_update: bool,
) -> (
    Feature,
    Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
    Vec<LossNote>,
) {
    let scope = |index: u32, state: i64, previous: i64| {
        let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
            &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
            crate::records::feature::scope::DesignFeatureKind::Move,
            index,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(state);
                draft.previous_history_state_id = Some(previous);
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };
    let scopes = [scope(5, 23, 2), scope(6, 27, 23), scope(8, 30, 27)];
    let feature = |id: &str, ordinal: u64, scope: &str, definition| Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: Some(scope.to_owned()),
    };
    let native = || {
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Extrude".into(),
            parameters: Default::default(),
        })
    };
    let combine_id = FeatureId::mint(COMBINE).unwrap();
    let prefix = crate::ids::feature_input_prefix(&combine_id, 27);
    let input_state = crate::ids::history_input_state_id(&prefix);
    let target = BodySelection::historical(
        input_state.clone(),
        vec![crate::ids::history_input_body_id(&prefix, 1)],
        TARGET_NATIVE.to_owned(),
    )
    .unwrap();
    let tool_slots = tools.iter().map(|(slot, _)| *slot).collect::<Vec<_>>();
    let tools = match tools {
        [(slot, native)] => BodySelection::historical(
            input_state,
            vec![crate::ids::history_input_body_id(&prefix, *slot)],
            (*native).to_owned(),
        )
        .unwrap(),
        _ => BodySelection::HistoricalSet {
            state: input_state,
            members: cadmpeg_ir::features::selections::BodyMembers::try_from_rows(
                tools
                    .iter()
                    .map(|(slot, native)| {
                        cadmpeg_ir::features::selections::BodyMember::new(
                            crate::ids::history_input_body_id(&prefix, *slot),
                            cadmpeg_core::text::NonBlankString::new((*native).to_owned()).unwrap(),
                        )
                    })
                    .collect(),
            )
            .unwrap(),
        },
    };
    let mut features = vec![
        feature(FIRST, 0, &scopes[0].id, native()),
        feature(SECOND, 1, &scopes[1].id, native()),
        feature(
            COMBINE,
            2,
            &scopes[2].id,
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands: CombineOperands::new(target, tools).unwrap(),
                op: BooleanKind::Cut,
                keep_tools: false,
                destination: CombineDestination::TargetComponent,
            }),
        ),
    ];
    let state = |state_id: i64, bodies: Vec<i64>, records: Vec<i64>| {
        let versions = bodies
            .iter()
            .zip(&records)
            .map(|(body, record)| AsmEntityVersion {
                entity_ref: *body,
                record_ref: *record,
            })
            .collect();
        (
            AsmDeltaState {
                id: format!("f3d:history:asm-delta-state#{state_id}"),
                parent: "history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology {
                bodies,
                ..AsmHistoricalTopology::default()
            }),
            versions,
        )
    };
    let slot_one_before_27 = if unscoped_update { 25 } else { 23 };
    let slot_one_at_27 = if second_updates_target {
        27
    } else {
        slot_one_before_27
    };
    let mut states = vec![
        state(30, after_combine.clone(), vec![30; after_combine.len()]),
        state(
            27,
            std::iter::once(1)
                .chain(tool_slots.iter().copied())
                .collect(),
            std::iter::once(slot_one_at_27)
                .chain(tool_slots.iter().map(|_| 27))
                .collect(),
        ),
        state(23, vec![1], vec![23]),
        state(2, Vec::new(), Vec::new()),
    ];
    if unscoped_update {
        states.insert(2, state(25, vec![1], vec![25]));
    }
    if later_update {
        let records = vec![31; after_combine.len()];
        states.insert(0, state(31, after_combine, records));
    }
    let histories = [crate::test_support::history::history(
        HISTORY.into(),
        0,
        None,
        false,
        states,
    )];
    let bodies = model_bodies
        .iter()
        .map(|id| {
            let mut body = cadmpeg_ir::examples::unit_cube()
                .unwrap()
                .geometry
                .bodies
                .remove(0);
            body.id = cadmpeg_ir::ids::BodyId::mint(*id).unwrap();
            body
        })
        .collect::<Vec<_>>();
    let inputs = FeatureBodySelectionInputs {
        scopes: &scopes,
        groups: &[],
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &histories,
        bodies: &bodies,
        regions: &[],
        shells: &[],
        faces: &[],
    };
    let mut losses = Vec::new();
    let results = crate::history::bind_generated_body_selections(
        &mut features,
        &inputs,
        &Default::default(),
        None,
        &mut losses,
    );
    (features.remove(2), results, losses)
}

fn operands(feature: &Feature) -> &CombineOperands {
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
        feature.evaluation.definition()
    else {
        panic!("combine definition");
    };
    operands
}

fn generated(selection: &BodySelection) -> Vec<(&str, &str)> {
    let BodySelection::Generated { bodies, .. } = selection else {
        panic!("generated selection: {selection:?}");
    };
    bodies
        .iter()
        .map(|body| (body.feature.as_str(), body.local_id.as_str()))
        .collect()
}

fn depends_on(feature: &Feature, producer: &str) -> bool {
    feature
        .dependencies
        .as_slice()
        .iter()
        .any(|dependency| dependency.as_str() == producer)
}

fn producer_unresolved(losses: &[LossNote]) -> usize {
    losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::F3dLossCode::FeatureCombineBodyProducerUnresolved.kind()
        })
        .count()
}

#[test]
fn combine_operands_the_final_model_omits_are_generated_by_their_producers() {
    let (combine, results, losses) = combine_case(&[], ONE_TOOL, vec![1], false, false, false);
    assert_eq!(generated(operands(&combine).target()), [(FIRST, "1")]);
    assert_eq!(generated(operands(&combine).tools()), [(SECOND, "2")]);
    assert!(depends_on(&combine, FIRST) && depends_on(&combine, SECOND));
    for producer in [FIRST, SECOND] {
        assert!(results
            .iter()
            .any(|result| result.output_of.as_str() == producer));
    }
    assert_eq!(producer_unresolved(&losses), 0);
}

#[test]
fn a_live_target_names_its_model_body_beside_a_generated_tool() {
    let (combine, _, losses) = combine_case(
        &["f3d:brep/root.smbh/brep:entity#1"],
        ONE_TOOL,
        vec![1],
        false,
        false,
        false,
    );
    let BodySelection::Resolved { bodies, native } = operands(&combine).target() else {
        panic!("resolved target: {:?}", operands(&combine).target());
    };
    assert_eq!(
        bodies
            .iter()
            .map(cadmpeg_ir::ids::BodyId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:brep/root.smbh/brep:entity#1"]
    );
    assert_eq!(native, TARGET_NATIVE);
    assert_eq!(generated(operands(&combine).tools()), [(SECOND, "2")]);
    assert!(depends_on(&combine, FIRST) && depends_on(&combine, SECOND));
    assert_eq!(producer_unresolved(&losses), 0);
}

#[test]
fn a_live_target_that_a_later_transition_updates_names_the_feature_that_inserted_it() {
    let (combine, results, losses) = combine_case(
        &["f3d:brep/root.smbh/brep:entity#1"],
        ONE_TOOL,
        vec![1],
        false,
        false,
        true,
    );
    assert_eq!(generated(operands(&combine).target()), [(FIRST, "1")]);
    assert_eq!(generated(operands(&combine).tools()), [(SECOND, "2")]);
    assert!(depends_on(&combine, FIRST) && depends_on(&combine, SECOND));
    assert!(results
        .iter()
        .any(|result| result.output_of.as_str() == FIRST));
    assert_eq!(producer_unresolved(&losses), 0);
}

#[test]
fn a_deleted_target_names_the_feature_that_inserted_it() {
    let (combine, _, losses) = combine_case(&[], ONE_TOOL, vec![1], false, false, true);
    assert_eq!(generated(operands(&combine).target()), [(FIRST, "1")]);
    assert_eq!(generated(operands(&combine).tools()), [(SECOND, "2")]);
    assert_eq!(producer_unresolved(&losses), 0);
}

#[test]
fn a_target_that_an_earlier_feature_updates_names_that_feature() {
    let (combine, results, losses) = combine_case(
        &["f3d:brep/root.smbh/brep:entity#1"],
        ONE_TOOL,
        vec![1],
        true,
        false,
        true,
    );
    assert_eq!(generated(operands(&combine).target()), [(SECOND, "1")]);
    assert_eq!(generated(operands(&combine).tools()), [(SECOND, "2")]);
    assert!(depends_on(&combine, SECOND));
    assert!(results.iter().any(|result| {
        result.output_of.as_str() == SECOND
            && result
                .bodies()
                .iter()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                .eq(["1", "2"])
    }));
    assert_eq!(producer_unresolved(&losses), 0);
}

#[test]
fn a_slot_whose_last_update_has_no_scope_names_its_inserting_feature() {
    let (combine, _, losses) = combine_case(&[], ONE_TOOL, vec![1], false, true, false);
    assert_eq!(generated(operands(&combine).target()), [(FIRST, "1")]);
    assert_eq!(generated(operands(&combine).tools()), [(SECOND, "2")]);
    assert_eq!(producer_unresolved(&losses), 0);
    assert_eq!(
        losses
            .iter()
            .filter(|loss| {
                loss.code == crate::loss::F3dLossCode::FeatureCombineOperandWriterAssumed.kind()
            })
            .count(),
        1
    );
}

#[test]
fn generated_tools_with_distinct_native_members_keep_one_native_per_body() {
    let (combine, _, losses) = combine_case(
        &[],
        &[(2, TOOL_NATIVE), (3, SECOND_TOOL_NATIVE)],
        vec![1],
        false,
        false,
        false,
    );
    assert_eq!(generated(operands(&combine).target()), [(FIRST, "1")]);
    let BodySelection::GeneratedSet { members } = operands(&combine).tools() else {
        panic!("generated tool set: {:?}", operands(&combine).tools());
    };
    let rows = members
        .iter()
        .map(|row| {
            let (body, native) = row.clone().into_parts();
            (
                body.feature.as_str().to_owned(),
                body.local_id.as_str().to_owned(),
                native,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (SECOND.to_owned(), "2".to_owned(), TOOL_NATIVE.to_owned()),
            (
                SECOND.to_owned(),
                "3".to_owned(),
                SECOND_TOOL_NATIVE.to_owned()
            ),
        ]
    );
    assert!(depends_on(&combine, FIRST) && depends_on(&combine, SECOND));
    assert_eq!(producer_unresolved(&losses), 0);
}
