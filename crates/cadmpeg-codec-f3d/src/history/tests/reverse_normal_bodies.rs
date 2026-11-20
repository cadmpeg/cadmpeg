// SPDX-License-Identifier: Apache-2.0
//! Reverse Normal input-state selections read in the bound history.

use crate::history::FeatureBodySelectionInputs;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
use cadmpeg_ir::features::{
    BodySelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    FeatureResultTopology,
};
use cadmpeg_ir::report::loss::LossNote;

const ROOT_HISTORY: &str = "f3d:Breps.BlobParts/BREP.root.smbh:asm-history#1";
const COMPONENT_HISTORY: &str = "f3d:Breps.BlobParts/BREP.component.smbh:asm-history#1";
const OTHER_HISTORY: &str = "f3d:Breps.BlobParts/BREP.other.smbh:asm-history#1";
const FIRST: &str = "f3d:test:feature#first";
const SECOND: &str = "f3d:test:feature#second";
const COMPONENT: &str = "f3d:test:feature#component";
const REVERSE: &str = "f3d:test:feature#reverse";
const GROUP: &str = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";

/// History `ROOT_HISTORY`, of BREP source `root.smbh`, holds states 2, 77,
/// 78 and 80. Feature `FIRST` (scope 5, state 77 from 2) inserts slot 1,
/// feature `SECOND` (scope 6, state 78 from 77) inserts slot 2, and the
/// Reverse Normal (scope 8, ordinal 3, state 80 from 78) selects `slots` in
/// its input state 78 and inserts `own_inserts`. History `COMPONENT_HISTORY`,
/// of source `component.smbh`, holds states 89 and 90: feature `COMPONENT`
/// (scope 9, state 90 from 89) inserts slot 3. With `duplicate_pair`, history
/// `OTHER_HISTORY`, of source `other.smbh`, also holds states 78 and 80, and
/// with `bound` the Reverse Normal scope is bound to `ROOT_HISTORY`.
struct ReverseCase<'a> {
    model_bodies: &'a [&'a str],
    slots: &'a [i64],
    first_ordinal: u64,
    own_inserts: &'a [i64],
    duplicate_pair: bool,
    bound: bool,
}

impl Default for ReverseCase<'_> {
    fn default() -> Self {
        Self {
            model_bodies: &[],
            slots: &[1],
            first_ordinal: 0,
            own_inserts: &[],
            duplicate_pair: false,
            bound: false,
        }
    }
}

fn reverse_case(case: &ReverseCase<'_>) -> (Feature, Vec<FeatureResultTopology>, Vec<LossNote>) {
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
    let scopes = [
        scope(5, 77, 2),
        scope(6, 78, 77),
        scope(8, 80, 78),
        scope(9, 90, 89),
    ];
    let feature = |id: &str, ordinal: u64, scope: &str, operation| Feature {
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
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: Some(scope.to_owned()),
    };
    let native = || FeatureOperation::Native {
        kind: "Extrude".into(),
        parameters: Default::default(),
    };
    let reverse_id = FeatureId::mint(REVERSE).unwrap();
    let prefix = crate::ids::feature_input_prefix(&reverse_id, 78);
    let source = BodySelection::historical(
        crate::ids::history_input_state_id(&prefix),
        case.slots
            .iter()
            .map(|slot| crate::ids::history_input_body_id(&prefix, *slot))
            .collect(),
        GROUP.to_owned(),
    )
    .unwrap();
    let mut features = vec![
        feature(FIRST, case.first_ordinal, &scopes[0].id, native()),
        feature(SECOND, 1, &scopes[1].id, native()),
        feature(COMPONENT, 2, &scopes[3].id, native()),
        feature(
            REVERSE,
            3,
            &scopes[2].id,
            FeatureOperation::ReverseShape { source },
        ),
    ];
    let state = |state_id: i64, bodies: Vec<i64>| -> crate::test_support::history::State {
        let versions = bodies
            .iter()
            .map(|body| crate::history_records::AsmEntityVersion {
                entity_ref: *body,
                record_ref: state_id,
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
    let reversed = [1, 2]
        .into_iter()
        .chain(case.own_inserts.iter().copied())
        .collect();
    let mut histories = vec![
        crate::test_support::history::history(
            ROOT_HISTORY.into(),
            0,
            None,
            false,
            vec![
                state(80, reversed),
                state(78, vec![1, 2]),
                state(77, vec![1]),
                state(2, Vec::new()),
            ],
        ),
        crate::test_support::history::history(
            COMPONENT_HISTORY.into(),
            0,
            None,
            false,
            vec![state(90, vec![3]), state(89, Vec::new())],
        ),
    ];
    if case.duplicate_pair {
        histories.push(crate::test_support::history::history(
            OTHER_HISTORY.into(),
            0,
            None,
            false,
            vec![state(80, vec![1]), state(78, vec![1])],
        ));
    }
    let mut scope_histories = std::collections::HashMap::new();
    if case.bound {
        scope_histories.insert(scopes[2].id.clone(), ROOT_HISTORY.to_owned());
    }
    let bodies = case
        .model_bodies
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
        &scope_histories,
        None,
        &mut losses,
    );
    (features.remove(3), results, losses)
}

fn reverse_source(feature: &Feature) -> &BodySelection {
    let FeatureDefinition::Operation(FeatureOperation::ReverseShape { source }) =
        feature.evaluation.definition()
    else {
        panic!("reverse definition: {:?}", feature.evaluation.definition());
    };
    source
}

fn resolved_bodies(feature: &Feature) -> Vec<&str> {
    let BodySelection::Resolved { bodies, native } = reverse_source(feature) else {
        panic!("resolved source: {:?}", reverse_source(feature));
    };
    assert_eq!(native, GROUP);
    bodies.iter().map(cadmpeg_ir::ids::BodyId::as_str).collect()
}

fn generated_bodies(feature: &Feature) -> Vec<(&str, &str)> {
    let BodySelection::Generated { bodies, .. } = reverse_source(feature) else {
        panic!("generated source: {:?}", reverse_source(feature));
    };
    bodies
        .iter()
        .map(|body| (body.feature.as_str(), body.local_id.as_str()))
        .collect()
}

fn dependencies(feature: &Feature) -> Vec<&str> {
    feature
        .dependencies
        .as_slice()
        .iter()
        .map(FeatureId::as_str)
        .collect()
}

fn producer_unresolved(losses: &[LossNote]) -> bool {
    losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::FeatureReverseNormalBodyProducerUnresolved.kind()
    })
}

fn assert_kept(case: &ReverseCase<'_>) {
    let (reverse, results, losses) = reverse_case(case);
    assert!(matches!(
        reverse_source(&reverse),
        BodySelection::Historical { .. }
    ));
    assert!(reverse.dependencies.is_empty());
    assert!(results.is_empty());
    assert!(producer_unresolved(&losses));
}

#[test]
fn a_reversed_body_the_source_keeps_selects_that_model_body() {
    let (reverse, results, losses) = reverse_case(&ReverseCase {
        model_bodies: &["f3d:brep/root.smbh/brep:entity#1"],
        ..ReverseCase::default()
    });
    assert_eq!(
        resolved_bodies(&reverse),
        ["f3d:brep/root.smbh/brep:entity#1"]
    );
    assert_eq!(dependencies(&reverse), [FIRST]);
    assert!(results.is_empty());
    assert!(losses.is_empty());
}

#[test]
fn every_reversed_body_the_source_keeps_selects_its_model_body() {
    let (reverse, _, losses) = reverse_case(&ReverseCase {
        model_bodies: &["f3d:brep/root.smbh/brep:entity#2", "f3d:brep:entity#1"],
        slots: &[1, 2],
        ..ReverseCase::default()
    });
    assert_eq!(
        resolved_bodies(&reverse),
        ["f3d:brep:entity#1", "f3d:brep/root.smbh/brep:entity#2"]
    );
    let mut dependencies = dependencies(&reverse);
    dependencies.sort_unstable();
    assert_eq!(dependencies, [FIRST, SECOND]);
    assert!(losses.is_empty());
}

#[test]
fn a_reversed_body_without_a_producer_selects_its_model_body() {
    let (reverse, _, losses) = reverse_case(&ReverseCase {
        model_bodies: &["f3d:brep/root.smbh/brep:entity#5"],
        slots: &[5],
        ..ReverseCase::default()
    });
    assert_eq!(
        resolved_bodies(&reverse),
        ["f3d:brep/root.smbh/brep:entity#5"]
    );
    assert!(reverse.dependencies.is_empty());
    assert!(losses.is_empty());
}

#[test]
fn a_producer_in_another_history_is_no_dependency() {
    let (reverse, _, losses) = reverse_case(&ReverseCase {
        model_bodies: &["f3d:brep/root.smbh/brep:entity#3"],
        slots: &[3],
        ..ReverseCase::default()
    });
    assert_eq!(
        resolved_bodies(&reverse),
        ["f3d:brep/root.smbh/brep:entity#3"]
    );
    assert!(reverse.dependencies.is_empty());
    assert!(losses.is_empty());
}

#[test]
fn a_later_producer_of_a_model_body_is_no_dependency() {
    let (reverse, _, losses) = reverse_case(&ReverseCase {
        model_bodies: &["f3d:brep/root.smbh/brep:entity#1"],
        first_ordinal: 4,
        ..ReverseCase::default()
    });
    assert_eq!(
        resolved_bodies(&reverse),
        ["f3d:brep/root.smbh/brep:entity#1"]
    );
    assert!(reverse.dependencies.is_empty());
    assert!(losses.is_empty());
}

#[test]
fn a_reversed_body_the_source_omits_is_generated_by_the_feature_that_inserts_it() {
    let (reverse, results, losses) = reverse_case(&ReverseCase::default());
    assert_eq!(generated_bodies(&reverse), [(FIRST, "1")]);
    assert_eq!(dependencies(&reverse), [FIRST]);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].output_of.as_str(), FIRST);
    assert!(losses.is_empty());
}

#[test]
fn a_model_body_of_another_source_leaves_the_slot_absent() {
    let (reverse, _, losses) = reverse_case(&ReverseCase {
        model_bodies: &["f3d:brep/component.smbh/brep:entity#1"],
        ..ReverseCase::default()
    });
    assert_eq!(generated_bodies(&reverse), [(FIRST, "1")]);
    assert_eq!(dependencies(&reverse), [FIRST]);
    assert!(losses.is_empty());
}

#[test]
fn a_saved_free_edge_body_leaves_the_slot_absent() {
    let (reverse, _, losses) = reverse_case(&ReverseCase {
        model_bodies: &["f3d:brep/root.smbh/brep:saved-edge-body#1"],
        ..ReverseCase::default()
    });
    assert_eq!(generated_bodies(&reverse), [(FIRST, "1")]);
    assert!(losses.is_empty());
}

#[test]
fn a_reversed_slot_of_several_model_bodies_stays_in_its_input_state() {
    assert_kept(&ReverseCase {
        model_bodies: &["f3d:brep:entity#1", "f3d:brep/root.smbh/brep:entity#1"],
        ..ReverseCase::default()
    });
}

#[test]
fn reversed_slots_the_source_partly_keeps_stay_in_their_input_state() {
    assert_kept(&ReverseCase {
        model_bodies: &["f3d:brep/root.smbh/brep:entity#1"],
        slots: &[1, 2],
        ..ReverseCase::default()
    });
}

#[test]
fn an_absent_reversed_slot_that_no_feature_inserts_stays_in_its_input_state() {
    assert_kept(&ReverseCase {
        slots: &[5],
        ..ReverseCase::default()
    });
}

#[test]
fn an_absent_reversed_slot_of_a_later_producer_stays_in_its_input_state() {
    assert_kept(&ReverseCase {
        first_ordinal: 4,
        ..ReverseCase::default()
    });
}

#[test]
fn an_absent_reversed_slot_that_the_reverse_normal_inserts_stays_in_its_input_state() {
    assert_kept(&ReverseCase {
        slots: &[9],
        own_inserts: &[9],
        ..ReverseCase::default()
    });
}

#[test]
fn an_absent_reversed_slot_of_another_history_stays_in_its_input_state() {
    assert_kept(&ReverseCase {
        slots: &[3],
        ..ReverseCase::default()
    });
}

#[test]
fn a_state_pair_of_several_histories_reads_the_bound_history() {
    let (reverse, _, losses) = reverse_case(&ReverseCase {
        model_bodies: &[
            "f3d:brep/other.smbh/brep:entity#1",
            "f3d:brep/root.smbh/brep:entity#1",
        ],
        duplicate_pair: true,
        bound: true,
        ..ReverseCase::default()
    });
    assert_eq!(
        resolved_bodies(&reverse),
        ["f3d:brep/root.smbh/brep:entity#1"]
    );
    assert_eq!(dependencies(&reverse), [FIRST]);
    assert!(losses.is_empty());
}

#[test]
fn an_unbound_state_pair_of_several_histories_stays_in_its_input_state() {
    assert_kept(&ReverseCase {
        model_bodies: &[
            "f3d:brep/other.smbh/brep:entity#1",
            "f3d:brep/root.smbh/brep:entity#1",
        ],
        duplicate_pair: true,
        ..ReverseCase::default()
    });
}
