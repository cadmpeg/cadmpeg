// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{CadirEncoder, EncodeInput, Encoder};
use cadmpeg_ir::design::refs::RecordRef;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureContent, FeatureDefinition, FeatureOperation,
    SketchFeatureBinding, UnresolvedFamily,
};
use cadmpeg_ir::history_body::{BodyTopology, HistoryStateRef};
use cadmpeg_ir::topology::Body;

use super::*;
use crate::test_support::root_part_ir;

fn feature(key: &str, ordinal: u64, operation: FeatureOperation) -> LegacyFeature {
    LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(format!("inventor:design:feature#{key}")).expect("id"),
        ordinal,
        name: Some(key.to_owned()),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: Some(format!("inventor:pmdc:feature#{key}")),
    }
}

fn extrude(key: &str, ordinal: u64) -> LegacyFeature {
    feature(
        key,
        ordinal,
        FeatureOperation::Unresolved {
            family: UnresolvedFamily::Extrude,
        },
    )
}

fn sketch(key: &str, ordinal: u64) -> LegacyFeature {
    feature(
        key,
        ordinal,
        FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Unresolved,
        },
    )
}

fn body(slot: u32) -> Body {
    json::from_value(cadmpeg_core::json!({
        "id": format!("inventor:brep:entity#{slot}"),
        "kind": "solid",
    }))
    .expect("body")
}

fn history_body(key: &str, ordinal: u32, consumed: bool, slot: u32) -> HistoryBody {
    let feature = FeatureId::mint(format!("inventor:design:feature#{key}")).expect("id");
    let mut nest = body(slot);
    nest.id = cadmpeg_ir::ids::BodyId::mint(format!("inventor:history-body:entity#h0000-{slot}"))
        .expect("nest body");
    HistoryBody {
        id: HistoryBodyId::mint(format!("inventor:history-body:nest#h{ordinal:04}")).expect("nest"),
        produced_by: (!consumed).then(|| feature.clone()),
        consumed_by: consumed.then_some(feature),
        state_of: None,
        state: HistoryStateRef {
            ordinal,
            identifier: 12,
        },
        topology: Box::new(BodyTopology::new(nest)),
    }
}

/// A part whose current geometry holds body slot 7, with `features` in its
/// timeline and `bodies` as its history bodies.
fn part(features: Vec<LegacyFeature>, bodies: Vec<HistoryBody>) -> CadIr {
    let mut ir = root_part_ir();
    ir.model.features = features;
    ir.geometry.bodies.push(body(7));
    ir.model.product_definitions[0].bodies = vec![body(7).id];
    ir.model.history_bodies = bodies;
    ir
}

/// State 0 holds body slot 9 in place of the current body.
fn states() -> BTreeMap<u32, HistoryState> {
    BTreeMap::from([(
        0,
        HistoryState {
            identifier: 12,
            geometry: Topology {
                bodies: vec![body(9)],
                ..Topology::default()
            },
        },
    )])
}

fn project_part(ir: &mut CadIr, states: &BTreeMap<u32, HistoryState>) -> Vec<LossNote> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let sketches = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        text_members: Vec::new(),
        issues: Vec::new(),
    };
    let mut losses = Vec::new();
    super::super::project(
        &ctx,
        ir,
        &crate::feature::FeatureInventory::default(),
        &sketches,
        &[],
        &[],
        &mut losses,
    )
    .expect("projection");
    super::project(&ctx, ir, states, &mut losses).expect("evaluations");
    losses
}

/// The `arena.omitted` losses of a CADIR write of `ir`, and the document it
/// reads back.
fn write(ir: &CadIr) -> (Vec<String>, CadIr) {
    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None), TargetRequest::Unnamed)
        .expect("the CADIR writer does not refuse");
    let omitted = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "arena.omitted")
        .map(|loss| loss.message.clone())
        .collect();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let read = CadIr::from_json(std::str::from_utf8(&bytes).expect("utf-8")).expect("read back");
    (omitted, read)
}

/// The state entering a Split is the state after the nearest earlier feature
/// with a state, based on the current state; a CADIR write carries the
/// Split's history body through it.
#[test]
fn the_state_before_a_split_is_an_evaluation_that_carries_its_history_body() {
    let mut ir = part(
        vec![extrude("a", 0), sketch("k", 1), extrude("s", 2)],
        vec![history_body("s", 0, true, 9)],
    );
    let losses = project_part(&mut ir, &states());
    assert!(losses.is_empty(), "{losses:?}");
    let [evaluation] = ir.design.evaluations.as_slice() else {
        panic!("one evaluation: {:?}", ir.design.evaluations);
    };
    let after = |key: &str| {
        Position::After(Member::Feature(
            FeatureId::mint(format!("inventor:design:feature#{key}")).expect("id"),
        ))
    };
    assert_eq!(evaluation.of, Evaluated::State(after("a")));
    assert_eq!(
        evaluation.native_ref.as_deref(),
        Some("inventor:history:0#12")
    );
    let content = evaluation.content.as_ref().expect("content");
    assert_eq!(content.base, StateBase::Current(after("s")));
    assert_eq!(content.delta.records.bodies, vec![body(9)]);
    assert_eq!(content.delta.removed, vec![RecordRef::Body(body(7).id)]);
    assert!(
        super::super::validate_states(&ir, &DecodePolicy::service().limits).is_empty(),
        "the evaluation resolves"
    );
    let nest = ir.model.history_bodies[0].id.as_str().to_owned();
    let (omitted, read) = write(&ir);
    assert!(
        !omitted.iter().any(|message| message.contains(&nest)),
        "{omitted:?}"
    );
    assert_eq!(read.design.evaluations, ir.design.evaluations);
}

/// The state after the last feature is the current state: it has no
/// evaluation, the decode charges its history body, and a CADIR write names
/// the body as omitted.
#[test]
fn a_state_the_current_geometry_holds_is_charged_and_stays_omitted() {
    let mut ir = part(
        vec![extrude("a", 0), extrude("base", 1)],
        vec![history_body("base", 0, false, 9)],
    );
    let losses = project_part(&mut ir, &states());
    assert!(ir.design.evaluations.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(loss.code.local_code(), "feature.history-state-unplaced");
    let nest = ir.model.history_bodies[0].id.as_str().to_owned();
    assert!(loss.message.contains(&nest), "{}", loss.message);
    let (omitted, _) = write(&ir);
    assert!(
        omitted.iter().any(|message| message.contains(&nest)),
        "{omitted:?}"
    );
}

/// Two states chain back: the earlier is based on the later, and the later
/// on the current state.
#[test]
fn earlier_states_are_based_on_later_ones() {
    let mut states = states();
    states.insert(
        1,
        HistoryState {
            identifier: 13,
            geometry: Topology {
                bodies: vec![body(9), body(11)],
                ..Topology::default()
            },
        },
    );
    let mut later = history_body("b", 1, false, 11);
    later.id = HistoryBodyId::mint("inventor:history-body:nest#h0001").expect("nest");
    let mut ir = part(
        vec![extrude("a", 0), extrude("b", 1), extrude("c", 2)],
        vec![history_body("a", 0, false, 9), later],
    );
    let losses = project_part(&mut ir, &states);
    assert!(losses.is_empty(), "{losses:?}");
    let [first, second] = ir.design.evaluations.as_slice() else {
        panic!("two evaluations: {:?}", ir.design.evaluations);
    };
    let first = first.content.as_ref().expect("content");
    assert_eq!(first.base, StateBase::Evaluation(second.id.clone()));
    assert!(first.delta.records.bodies.is_empty());
    assert_eq!(first.delta.removed, vec![RecordRef::Body(body(11).id)]);
    assert!(
        super::super::validate_states(&ir, &DecodePolicy::service().limits).is_empty(),
        "the chain resolves"
    );
    let (omitted, _) = write(&ir);
    assert!(
        !omitted
            .iter()
            .any(|message| message.contains("history_bodies")),
        "{omitted:?}"
    );
}
