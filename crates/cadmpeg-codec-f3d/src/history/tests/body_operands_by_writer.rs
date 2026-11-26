// SPDX-License-Identifier: Apache-2.0
//! Input-state body selections of a live slot named by the feature that last
//! writes the slot.

use crate::history::body_selections::FeatureBodySelectionInputs;
use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

const HISTORY: &str = "f3d:Breps.BlobParts/BREP.root.smbh:asm-history#1";
const FIRST: &str = "f3d:test:feature#first";
const SECOND: &str = "f3d:test:feature#second";
const MOVE: &str = "f3d:test:feature#move";
const MOVE_NATIVE: &str = "f3d:Design/BulkStream.dat:design-construction-operand-group#9";
const MODEL_BODY: &str = "f3d:brep/root.smbh/brep:entity#1";

/// History `HISTORY` of BREP source `root.smbh`, whose final model holds slot
/// 1: feature `FIRST` (state 23 from 2) inserts slot 1, feature `SECOND`
/// (state 27 from 23) updates it, and the Move (state 30 from 27) selects
/// slot 1 in its input state 27 and updates it. With `later_update`, a later
/// transition (state 31 from 30) updates slot 1 again.
fn move_case(later_update: bool) -> (Feature, Vec<LossNote>) {
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
    let move_id = FeatureId::mint(MOVE).unwrap();
    let prefix = crate::ids::feature_input_prefix(&move_id, 27);
    let bodies = BodySelection::historical(
        crate::ids::history_input_state_id(&prefix),
        vec![crate::ids::history_input_body_id(&prefix, 1)],
        MOVE_NATIVE.to_owned(),
    )
    .unwrap();
    let mut features = vec![
        feature(FIRST, 0, &scopes[0].id, native()),
        feature(SECOND, 1, &scopes[1].id, native()),
        feature(
            MOVE,
            2,
            &scopes[2].id,
            FeatureDefinition::Operation(FeatureOperation::MoveBody {
                bodies,
                translation: cadmpeg_ir::features::geometry::FiniteVector3::new(
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
                rotation: None,
                copies: 0,
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
        state(30, vec![1], 30),
        state(27, vec![1], 27),
        state(23, vec![1], 23),
        state(2, Vec::new(), 0),
    ];
    if later_update {
        states.insert(0, state(31, vec![1], 31));
    }
    let histories = [crate::test_support::history::history(
        HISTORY.into(),
        0,
        None,
        false,
        states,
    )];
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint(MODEL_BODY).unwrap();
    let bodies = [body];
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
    crate::history::bind_generated_body_selections(
        &mut features,
        &inputs,
        &Default::default(),
        None,
        &mut losses,
    );
    (features.remove(2), losses)
}

fn moved_bodies(feature: &Feature) -> &BodySelection {
    let FeatureDefinition::Operation(FeatureOperation::MoveBody { bodies, .. }) =
        feature.evaluation.definition()
    else {
        panic!("move definition");
    };
    bodies
}

#[test]
fn a_move_of_a_body_that_a_later_transition_updates_names_its_last_writer() {
    let (moved, losses) = move_case(true);
    let BodySelection::Generated { bodies, native } = moved_bodies(&moved) else {
        panic!("generated bodies: {:?}", moved_bodies(&moved));
    };
    assert_eq!(
        bodies
            .iter()
            .map(|body| (body.feature.as_str(), body.local_id.as_str()))
            .collect::<Vec<_>>(),
        [(SECOND, "1")]
    );
    assert_eq!(native.as_str(), MOVE_NATIVE);
    assert!(moved
        .dependencies
        .as_slice()
        .iter()
        .any(|dependency| dependency.as_str() == SECOND));
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_move_of_a_body_that_no_later_transition_changes_names_its_model_body() {
    let (moved, losses) = move_case(false);
    let BodySelection::Resolved { bodies, native } = moved_bodies(&moved) else {
        panic!("model bodies: {:?}", moved_bodies(&moved));
    };
    assert_eq!(
        bodies
            .iter()
            .map(cadmpeg_ir::ids::BodyId::as_str)
            .collect::<Vec<_>>(),
        [MODEL_BODY]
    );
    assert_eq!(native.as_str(), MOVE_NATIVE);
    assert!(moved
        .dependencies
        .as_slice()
        .iter()
        .any(|dependency| dependency.as_str() == FIRST));
    assert!(losses.is_empty(), "{losses:?}");
}
