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

/// History `HISTORY`, of BREP source `root.smbh`: feature `FIRST` (state 23
/// from 2) inserts slot 1, feature `SECOND` (state 27 from 23) inserts slot
/// 2, and the Combine (state 30 from 27) selects target slot 1 and tool slot
/// 2 in its input state 27 and keeps the bodies `after_combine`. With
/// `later_update`, a later transition (state 31 from 30) updates slot 1.
fn combine_case(
    model_bodies: &[&str],
    after_combine: Vec<i64>,
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
    let tools = BodySelection::historical(
        input_state,
        vec![crate::ids::history_input_body_id(&prefix, 2)],
        TOOL_NATIVE.to_owned(),
    )
    .unwrap();
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
    let state = |state_id: i64, bodies: Vec<i64>, record: i64| {
        let versions = bodies
            .iter()
            .map(|body| AsmEntityVersion {
                entity_ref: *body,
                record_ref: record,
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
    let mut states = vec![
        state(30, after_combine.clone(), 30),
        state(27, vec![1, 2], 27),
        state(23, vec![1], 23),
        state(2, Vec::new(), 2),
    ];
    if later_update {
        states.insert(0, state(31, after_combine, 31));
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
    let (combine, results, losses) = combine_case(&[], vec![1], false);
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
    let (combine, _, losses) = combine_case(&["f3d:brep/root.smbh/brep:entity#1"], vec![1], false);
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
fn a_live_target_that_a_later_transition_updates_stays_historical() {
    let (combine, _, losses) = combine_case(&["f3d:brep/root.smbh/brep:entity#1"], vec![1], true);
    assert!(matches!(
        operands(&combine).target(),
        BodySelection::Historical { .. }
    ));
    assert_eq!(generated(operands(&combine).tools()), [(SECOND, "2")]);
    assert_eq!(producer_unresolved(&losses), 1);
}

#[test]
fn a_deleted_target_names_the_feature_that_inserted_it() {
    let (combine, _, losses) = combine_case(&[], vec![1], true);
    assert_eq!(generated(operands(&combine).target()), [(FIRST, "1")]);
    assert_eq!(generated(operands(&combine).tools()), [(SECOND, "2")]);
    assert_eq!(producer_unresolved(&losses), 0);
}
