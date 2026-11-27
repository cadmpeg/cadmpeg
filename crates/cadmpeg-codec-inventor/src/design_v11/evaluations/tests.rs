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

fn no_sketches() -> crate::sketch::SketchInventory {
    crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        text_members: Vec::new(),
        issues: Vec::new(),
    }
}

/// The messages of the findings of the check that re-projects the design of
/// `ir` from its admitted source operands.
fn validate_operations_findings(ir: &CadIr) -> Vec<String> {
    assert!(ir.model.omitted_arenas_held(), "the check runs");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    super::super::validate_operations(
        &ctx,
        ir,
        &crate::feature::FeatureInventory::default(),
        &no_sketches(),
        &[],
        &[],
    )
    .expect("check")
    .into_iter()
    .map(|finding| finding.message)
    .collect()
}

/// Project `ir` and return the losses and the undecoded records that the
/// stated states reference.
fn project_retaining(
    ir: &mut CadIr,
    states: &BTreeMap<u32, StateRecords>,
) -> (Vec<LossNote>, Vec<(u32, UnknownRecord)>) {
    project_over(ir, states, Vec::new())
}

/// Project `ir` into a document that already holds the undecoded records
/// `document`, and return the losses and the undecoded records that the
/// stated states reference.
fn project_over(
    ir: &mut CadIr,
    states: &BTreeMap<u32, StateRecords>,
    document: Vec<UnknownRecord>,
) -> (Vec<LossNote>, Vec<(u32, UnknownRecord)>) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut fidelity = cadmpeg_ir::SourceFidelity::for_root(root);
    let mut losses = Vec::new();
    super::super::project(
        &ctx,
        ir,
        &crate::feature::FeatureInventory::default(),
        &no_sketches(),
        &[],
        &[],
        &mut losses,
    )
    .expect("projection");
    fidelity
        .attach_native_unknown_records(ir, "inventor", document, &ctx)
        .expect("document records");
    let mut retained = Vec::new();
    super::project(&ctx, ir, states, &fidelity, &mut retained, &mut losses).expect("evaluations");
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

/// The native history-state record that the current state names.
fn current_state_record(ir: &CadIr) -> Option<&str> {
    ir.design.scopes[0].current[0].native_ref.as_deref()
}

/// The ordinals of the native history-state records of `ir`.
fn history_states(ir: &CadIr) -> Vec<String> {
    ir.native
        .namespace("inventor")
        .into_iter()
        .flat_map(cadmpeg_ir::NativeNamespace::arenas)
        .filter(|(name, _)| name.as_str() == HISTORY_STATES)
        .flat_map(|(_, records)| records.iter().map(|record| record.id().to_owned()))
        .collect()
}

/// A Base feature whose state is the current geometry record for record
/// holds the current state: the current state names the native record of
/// the history state, and a CADIR write states the history body in full
/// through the feature and the current state.
#[test]
fn a_base_state_the_current_geometry_holds_is_the_current_state() {
    let mut ir = part(
        vec![extrude("a", 0), extrude("base", 1)],
        vec![history_body("base", 0, false, 7)],
    );
    let states = BTreeMap::from([(0, state(vec![body(7)]))]);
    let losses = project_part(&mut ir, &states);
    assert!(losses.is_empty(), "{losses:?}");
    assert!(ir.design.evaluations.is_empty());
    let native = ids(0).1;
    assert_eq!(current_state_record(&ir), Some(native.as_str()));
    assert_eq!(history_states(&ir), [native]);
    let nest = ir.model.history_bodies[0].id.as_str().to_owned();
    let (omitted, read) = write(&ir);
    assert_stated(&omitted, &nest);
    assert_eq!(read.design.scopes, ir.design.scopes);
    assert_eq!(validate_operations_findings(&ir), Vec::<String>::new());
}

/// A Split rolled back to the end of part enters the current state, so the
/// current state holds its history body as it holds a Base's.
#[test]
fn a_rolled_back_split_enters_the_current_state() {
    let mut split = extrude("s", 1);
    split.suppressed = Some(true);
    split.source_properties.insert(
        cadmpeg_core::text::NonBlankString::new("end_of_part".to_owned()).expect("key"),
        "below".to_owned(),
    );
    let mut ir = part(
        vec![extrude("a", 0), split],
        vec![history_body("s", 0, true, 7)],
    );
    let states = BTreeMap::from([(0, state(vec![body(7)]))]);
    let losses = project_part(&mut ir, &states);
    assert!(losses.is_empty(), "{losses:?}");
    assert!(ir.design.evaluations.is_empty());
    assert_eq!(current_state_record(&ir), Some(ids(0).1.as_str()));
    let nest = ir.model.history_bodies[0].id.as_str().to_owned();
    let (omitted, _) = write(&ir);
    assert_stated(&omitted, &nest);
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

#[test]
fn a_base_at_the_current_position_cannot_share_different_spline_bytes() {
    use cadmpeg_ir::ids::{FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Face, FaceLoops, Region, Sense, Shell};

    let mut ir = part(
        vec![extrude("a", 0), extrude("base", 1)],
        vec![history_body("base", 0, false, 7)],
    );
    let mut state = spline_state();
    let geometry = &mut state.geometry;
    geometry.bodies = ir.geometry.bodies.clone();
    let region = RegionId::mint("inventor:brep:entity#8").expect("region");
    let shell = ShellId::mint("inventor:brep:entity#9").expect("shell");
    geometry
        .regions
        .push(Region::new(region.clone(), body(7).id, vec![shell.clone()]));
    geometry.shells.push(Shell::new(shell.clone(), region));
    geometry.faces.push(Face {
        id: FaceId::mint("inventor:brep:entity#10").expect("face"),
        shell,
        surface: geometry.surfaces[0].id.clone(),
        sense: Sense::Forward,
        loops: FaceLoops::Unspecified { loops: Vec::new() },
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    ir.geometry = geometry.clone();
    state.unknowns = vec![spline(&[4, 5, 6])];
    let (losses, retained) = project_over(
        &mut ir,
        &BTreeMap::from([(0, state)]),
        vec![spline(&[1, 2, 3])],
    );
    assert_eq!(current_state_record(&ir), None);
    assert!(ir.design.evaluations.is_empty());
    assert!(retained.is_empty());
    assert!(ir
        .model
        .restated()
        .get(ir.model.history_bodies[0].id.as_str())
        .is_none());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert!(loss.message.contains("in history state 0"));
    assert!(loss.message.contains("inventor:brep:spline#20 differs"));
}

#[test]
fn a_state_reports_an_unavailable_byte_image_without_claiming_a_difference() {
    let mut ir = part(
        vec![extrude("a", 0), extrude("b", 1)],
        vec![history_body("a", 0, false, 9)],
    );
    let bytes = spline(&[1, 2, 3]);
    let unavailable = UnknownRecord::unavailable(
        bytes.id().clone(),
        0,
        3,
        "039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81",
        Vec::new(),
    );
    let mut state = spline_state();
    state.unknowns = vec![bytes];
    let (losses, retained) =
        project_over(&mut ir, &BTreeMap::from([(0, state)]), vec![unavailable]);
    assert!(ir.design.evaluations.is_empty());
    assert!(retained.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert!(loss
        .message
        .contains("inventor:brep:spline#20 cannot be compared without both byte images"));
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
        loss.message.ends_with(
            "its undecoded record inventor:brep:spline#20 differs from the document's record of that id."
        ),
        "{}",
        loss.message
    );
}

/// A state whose own undecoded record differs from the document's record
/// of that id cannot read its own record, so the decode charges its
/// history body, names the state and the record, and keeps the document's
/// record.
#[test]
fn a_state_record_that_differs_from_the_document_record_of_its_id_is_charged() {
    let mut ir = part(
        vec![extrude("a", 0), sketch("k", 1), extrude("s", 2)],
        vec![history_body("s", 0, true, 9)],
    );
    let mut state = spline_state();
    state.unknowns = vec![spline(&[4, 5, 6])];
    let (losses, retained) = project_over(
        &mut ir,
        &BTreeMap::from([(0, state)]),
        vec![spline(&[1, 2, 3])],
    );
    assert!(ir.design.evaluations.is_empty());
    assert!(retained.is_empty(), "{retained:?}");
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert!(
        loss.message.contains("in history state 0")
            && loss.message.ends_with(
                "its undecoded record inventor:brep:spline#20 differs from the document's record of that id."
            ),
        "{}",
        loss.message
    );
}

/// A state whose own undecoded record holds the bytes of the document's
/// record of that id shares the document's record.
#[test]
fn a_state_record_equal_to_the_document_record_of_its_id_is_shared() {
    let mut ir = part(
        vec![extrude("a", 0), sketch("k", 1), extrude("s", 2)],
        vec![history_body("s", 0, true, 9)],
    );
    let mut state = spline_state();
    state.unknowns = vec![spline(&[1, 2, 3])];
    let (losses, retained) = project_over(
        &mut ir,
        &BTreeMap::from([(0, state)]),
        vec![spline(&[1, 2, 3])],
    );
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(ir.design.evaluations.len(), 1);
    assert!(retained.is_empty(), "{retained:?}");
    assert!(unclosed(&ir).is_empty(), "{:?}", unclosed(&ir));
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
