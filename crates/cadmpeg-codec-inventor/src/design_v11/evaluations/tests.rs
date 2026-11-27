// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_core::serde_erase::json;
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

/// A decoded state that holds `bodies`.
fn state(bodies: Vec<Body>) -> StateRecords {
    StateRecords {
        geometry: Topology {
            bodies,
            ..Topology::default()
        },
        ..StateRecords::default()
    }
}

/// State 0 holds body slot 9 in place of the current body.
fn states() -> BTreeMap<u32, StateRecords> {
    BTreeMap::from([(0, state(vec![body(9)]))])
}

fn project_part(ir: &mut CadIr, states: &BTreeMap<u32, StateRecords>) -> Vec<LossNote> {
    project_retaining(ir, states).0
}

/// Project `ir` and return the losses and the undecoded records that the
/// stated states reference.
fn project_retaining(
    ir: &mut CadIr,
    states: &BTreeMap<u32, StateRecords>,
) -> (Vec<LossNote>, Vec<(u32, UnknownRecord)>) {
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
    let mut retained = Vec::new();
    super::project(&ctx, ir, states, &mut retained, &mut losses).expect("evaluations");
    (losses, retained)
}

/// The `arena.omitted` losses of a CADIR write of `ir`, and the document it
/// reads back.
fn write(ir: &CadIr) -> (Vec<String>, CadIr) {
    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None))
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

/// A CADIR write states the history body `nest` in full.
fn assert_stated(omitted: &[String], nest: &str) {
    assert!(
        !omitted.iter().any(|message| message.contains(nest)),
        "{omitted:?}"
    );
}

/// The id of the evaluation of history state `ordinal`, and of the native
/// record of the state.
fn ids(ordinal: u32) -> (String, String) {
    (
        format!("inventor:design:evaluation#{ordinal}"),
        format!("inventor:history:state#{ordinal}"),
    )
}

/// A CADIR write states the history body `nest` in every field except its
/// state ordinal and identifier, which the feature that restates it does
/// not hold.
fn assert_stated_but_state(omitted: &[String], nest: &str) {
    assert!(
        omitted
            .iter()
            .any(|message| message.contains(&format!("{nest} (state)"))),
        "{omitted:?}"
    );
    assert!(
        !omitted.iter().any(|message| message
            .split_once("record(s) omitted: ")
            .is_some_and(|(_, ids)| ids.split("; ").next().unwrap_or("").contains(nest))),
        "{omitted:?}"
    );
}

/// The state entering a Split is the state after the nearest earlier feature
/// with a state, based on the current state. The evaluation names the
/// native record of the history state, and a CADIR write carries the
/// Split's history body in full through the two.
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
    let (id, native) = ids(0);
    assert_eq!(evaluation.id.as_str(), id);
    assert_eq!(evaluation.native_ref.as_deref(), Some(native.as_str()));
    let records = &ir
        .native
        .namespace("inventor")
        .expect("inventor namespace")
        .arenas()
        .iter()
        .find(|(name, _)| name.as_str() == HISTORY_STATES)
        .expect("history states")
        .1;
    let [record] = records.as_slice() else {
        panic!("one history state: {records:?}");
    };
    assert_eq!(record.id(), native);
    let fields = record.fields().expect("fields");
    assert_eq!(
        (&fields["ordinal"], &fields["identifier"]),
        (&cadmpeg_core::json!(0), &cadmpeg_core::json!(12))
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
    assert_stated(&omitted, &nest);
    assert_eq!(read.design.evaluations, ir.design.evaluations);
}

/// The state after the last feature is the current state, so it has no
/// evaluation. Where the decoded state differs from the current geometry,
/// the decode charges its history body and a CADIR write names the body as
/// omitted.
#[test]
fn a_current_state_that_differs_from_the_current_geometry_is_charged() {
    let mut ir = part(
        vec![extrude("a", 0), extrude("base", 1)],
        vec![history_body("base", 0, false, 9)],
    );
    let losses = project_part(&mut ir, &states());
    assert!(ir.design.evaluations.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(
        loss.code.local_code(),
        InventorLossCode::FeatureHistoryStateUnplaced.code()
    );
    assert!(
        loss.message
            .ends_with("the state differs from the current geometry."),
        "{}",
        loss.message
    );
    let nest = ir.model.history_bodies[0].id.as_str().to_owned();
    assert!(loss.message.contains(&nest), "{}", loss.message);
    let (omitted, _) = write(&ir);
    assert!(
        omitted.iter().any(|message| message.contains(&nest)),
        "{omitted:?}"
    );
}

/// A Base feature whose state is the current geometry record for record
/// restates its history body through its stored bodies.
#[test]
fn a_base_state_the_current_geometry_holds_is_restated_by_its_feature() {
    let mut ir = part(
        vec![extrude("a", 0), extrude("base", 1)],
        vec![history_body("base", 0, false, 7)],
    );
    let states = BTreeMap::from([(0, state(vec![body(7)]))]);
    let losses = project_part(&mut ir, &states);
    assert!(losses.is_empty(), "{losses:?}");
    assert!(ir.design.evaluations.is_empty());
    let nest = ir.model.history_bodies[0].id.as_str().to_owned();
    let restating = ir.model.restated().get(&nest).expect("restated");
    assert_eq!(
        restating
            .restaters()
            .map(|(by, _)| by)
            .next()
            .expect("restater"),
        &Restater::Record("inventor:design:feature#base".to_owned())
    );
    let (omitted, _) = write(&ir);
    assert_stated_but_state(&omitted, &nest);
}

/// A Base feature followed only by sketches holds the current state, so
/// its state has no evaluation and its history body is restated by the
/// feature.
#[test]
fn a_base_followed_only_by_sketches_holds_the_current_state() {
    let mut ir = part(
        vec![extrude("a", 0), extrude("base", 1), sketch("k", 2)],
        vec![history_body("base", 0, false, 7)],
    );
    let states = BTreeMap::from([(0, state(vec![body(7)]))]);
    let losses = project_part(&mut ir, &states);
    assert!(losses.is_empty(), "{losses:?}");
    assert!(ir.design.evaluations.is_empty());
    let nest = ir.model.history_bodies[0].id.as_str();
    assert!(ir.model.restated().get(nest).is_some());
}

/// A decoded state whose surface names spline record 20.
fn spline_state() -> StateRecords {
    let mut state = state(vec![body(9)]);
    state.geometry.surfaces = vec![json::from_value(cadmpeg_core::json!({
        "id": "inventor:brep:entity#20",
        "geometry": {"kind": "solved", "value": {"kind": "unknown", "record": "inventor:brep:spline#20"}},
    }))
    .expect("surface")];
    state
}

fn spline(bytes: &[u8]) -> UnknownRecord {
    UnknownRecord::retained(
        cadmpeg_ir::ids::UnknownId::mint("inventor:brep:spline#20").expect("id"),
        0,
        bytes.to_vec(),
        Vec::new(),
    )
}

/// The findings of a validation of `ir` that a resolved state references a
/// record that it does not hold.
fn unclosed(ir: &CadIr) -> Vec<String> {
    cadmpeg_ir::validate::validate_neutral(ir, Vec::new())
        .expect("validation")
        .findings
        .into_iter()
        .filter(|finding| finding.message.contains("which the state does not hold"))
        .map(|finding| finding.message)
        .collect()
}

/// A decoded state whose surface names an undecoded record of the state
/// closes once the document holds that record: the decode states the
/// evaluation and retains the record.
#[test]
fn a_state_closes_through_its_undecoded_records() {
    let mut ir = part(
        vec![extrude("a", 0), sketch("k", 1), extrude("s", 2)],
        vec![history_body("s", 0, true, 9)],
    );
    let mut state = spline_state();
    state.unknowns = vec![spline(&[1, 2, 3])];
    let (losses, retained) = project_retaining(&mut ir, &BTreeMap::from([(0, state)]));
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(ir.design.evaluations.len(), 1);
    assert_eq!(retained, vec![(0, spline(&[1, 2, 3]))]);
    assert!(
        !unclosed(&ir).is_empty(),
        "the state is open without the record"
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    cadmpeg_ir::SourceFidelity::for_root(root)
        .attach_native_unknown_records(
            &mut ir,
            "inventor",
            retained.into_iter().map(|(_, record)| record).collect(),
            &ctx,
        )
        .expect("retain");
    assert!(unclosed(&ir).is_empty(), "{:?}", unclosed(&ir));
}

/// Two states that name one undecoded record with other bytes cannot both
/// close through the one record the document holds: the later state is
/// stated, and the earlier one is charged.
#[test]
fn an_undecoded_record_that_a_later_state_holds_with_other_bytes_is_charged() {
    let mut later = spline_state();
    later.geometry.bodies.push(body(11));
    later.unknowns = vec![spline(&[1, 2, 3])];
    let mut earlier = spline_state();
    earlier.unknowns = vec![spline(&[4, 5, 6])];
    let mut second = history_body("b", 1, false, 11);
    second.id = HistoryBodyId::mint("inventor:history-body:nest#h0001").expect("nest");
    let mut ir = part(
        vec![extrude("a", 0), extrude("b", 1), extrude("c", 2)],
        vec![history_body("a", 0, false, 9), second],
    );
    let (losses, retained) =
        project_retaining(&mut ir, &BTreeMap::from([(0, earlier), (1, later)]));
    let [evaluation] = ir.design.evaluations.as_slice() else {
        panic!("one evaluation: {:?}", ir.design.evaluations);
    };
    assert_eq!(evaluation.id.as_str(), ids(1).0);
    assert_eq!(retained, vec![(1, spline(&[1, 2, 3]))]);
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert!(
        loss.message
            .ends_with("the state names a record that it does not hold."),
        "{}",
        loss.message
    );
}

/// A decoded state whose surface names a record that neither the state nor
/// the document holds does not close, so the decode charges its history
/// body in place of an evaluation.
#[test]
fn a_state_that_names_a_record_it_does_not_hold_is_charged() {
    let mut ir = part(
        vec![extrude("a", 0), sketch("k", 1), extrude("s", 2)],
        vec![history_body("s", 0, true, 9)],
    );
    let states = BTreeMap::from([(0, spline_state())]);
    let losses = project_part(&mut ir, &states);
    assert!(ir.design.evaluations.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert!(
        loss.message
            .ends_with("the state names a record that it does not hold."),
        "{}",
        loss.message
    );
}

/// Two states chain back: the earlier is based on the later, and the later
/// on the current state.
#[test]
fn earlier_states_are_based_on_later_ones() {
    let mut states = states();
    states.insert(1, state(vec![body(9), body(11)]));
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
    for nest in [
        "inventor:history-body:nest#h0000",
        "inventor:history-body:nest#h0001",
    ] {
        assert_stated(&omitted, nest);
    }
}

/// Two positions whose states are one history state take two evaluations
/// with distinct ids that name the one native record of the state.
#[test]
fn two_evaluations_of_one_history_state_take_distinct_ids() {
    let mut split = history_body("c", 0, true, 9);
    split.id = HistoryBodyId::mint("inventor:history-body:nest#h0000-split").expect("nest");
    let mut ir = part(
        vec![extrude("a", 0), extrude("b", 1), extrude("c", 2)],
        vec![history_body("a", 0, false, 9), split],
    );
    let losses = project_part(&mut ir, &states());
    assert!(losses.is_empty(), "{losses:?}");
    let (id, native) = ids(0);
    let mut minted = ir
        .design
        .evaluations
        .iter()
        .map(|evaluation| {
            (
                evaluation.id.as_str().to_owned(),
                evaluation.native_ref.clone(),
            )
        })
        .collect::<Vec<_>>();
    minted.sort();
    assert_eq!(
        minted,
        [
            (id.clone(), Some(native.clone())),
            (format!("{id}-1"), Some(native)),
        ]
    );
}
