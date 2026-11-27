// SPDX-License-Identifier: Apache-2.0
//! The change between two stored states of one blob, the placement of the
//! state after each timeline item, and the evaluations and losses of the
//! items of a scope.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::design::configuration::{DesignConfiguration, Overrides};
use cadmpeg_ir::design::evaluation::{
    CurrentState, Evaluation, EvaluationStatus, StateBase, StateDelta,
};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::names::NameEntry;
use cadmpeg_ir::design::refs::{EntityRef, Evaluated, Member, Position, RecordRef};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::{ConfigurationId, FeatureId};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::{BodyId, PointId, ProductDefinitionId, SurfaceId, UnknownId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::products::{ProductDefinition, Storage};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind, Point};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::SourceFidelity;

use crate::decode::geometry_admission::Note;
use crate::design::components::HistoryPairOwner;
use crate::history::stored_states::{Blob, Read, StateRecords, Want};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignScopeTailFlags;
use crate::test_support::zip_test::f3d_with_smbh;

use super::{
    attach_bound, bind, held, history_owners, inherited_bodies, mark_current, marked, omitted,
    placed, plan, poses_assumed, project, resolve, role, step, stopped, tails_assumed, unresolved,
    unstated, wants, Chain, Consumed, Entry, History, Kind, Marks, Plan, Planned, Reader, Role,
    Rule, Scope, Tail, Tails,
};

fn point(name: &str, x: f64) -> Point {
    Point::new(
        PointId::mint(format!("f3d:brep:entity#{name}")).expect("identity grammar"),
        FinitePoint3::new(Point3::new(x, 0.0, 0.0)).expect("a finite point"),
        None,
    )
}

fn points(points: Vec<Point>) -> Topology {
    Topology {
        points,
        ..Topology::default()
    }
}

fn body(name: &str, label: Option<&str>) -> Body {
    Body {
        id: BodyId::mint(format!("f3d:brep:entity#{name}")).expect("identity grammar"),
        kind: BodyKind::Solid,
        transform: None,
        name: label.map(str::to_owned),
        color: None,
        visible: None,
        unstated: None,
        native_ref: None,
    }
}

#[test]
fn a_step_adds_the_changed_and_new_records_and_removes_the_absent_ones() {
    let before = points(vec![point("1", 0.0), point("2", 1.0), point("4", 4.0)]);
    let after = points(vec![point("1", 0.0), point("2", 2.0), point("3", 3.0)]);
    let head = points(vec![point("3", 3.0)]);
    let current = points(vec![point("3", 5.0)]);
    let mut delta = StateDelta::default();
    step(&before, &after, &head, &current, &mut delta).expect("the step");
    assert_eq!(delta.records.points, [point("2", 2.0), point("3", 5.0)]);
    assert_eq!(delta.removed, [RecordRef::Point(point("4", 0.0).id)]);
}

#[test]
fn a_step_between_equal_states_is_empty() {
    let state = points(vec![point("1", 0.0), point("2", 1.0)]);
    let mut delta = StateDelta::default();
    step(&state, &state, &state, &Topology::default(), &mut delta).expect("the step");
    assert_eq!(delta, StateDelta::default());
}

/// The Design stream of the timeline items of [`design`].
const STREAM: &str = "f3d:timeline";

fn feature_id(name: &str) -> FeatureId {
    FeatureId::mint(format!("test:design:feature#{name}")).expect("identity grammar")
}

/// A root scope and its document.
struct Design {
    ir: CadIr,
    scope: Scope,
}

/// The operation of the fixture feature `name`: a joint, a body move, a
/// rigid group, or a new component where its name starts with that word,
/// else an extrude.
fn operation(name: &str) -> FeatureOperation {
    let family = |family| FeatureOperation::Unresolved { family };
    match name {
        _ if name.starts_with("joint") => family(OperationFamily::Joint),
        _ if name.starts_with("move") => family(OperationFamily::TransformBodies),
        _ if name.starts_with("group") => family(OperationFamily::RigidGroup),
        _ if name.starts_with("component") => FeatureOperation::Native {
            kind: CodecName::new(&cadmpeg_ir::codec_id!("f3d"), "NewComponent")
                .expect("a codec name"),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        },
        _ => family(OperationFamily::Extrude),
    }
}

/// A root scope whose timeline holds the features `names`, of the
/// operations that [`operation`] gives them, with scope records 10 onward,
/// whose current state is after the feature `at` and holds `bodies`.
fn design(names: &[&str], at: &str, bodies: Vec<Body>) -> Design {
    let mut ir = CadIr::empty();
    let mut members = Vec::new();
    let mut items = BTreeMap::new();
    for (record, name) in (10..).zip(names) {
        let id = feature_id(name);
        ir.design.features.push(Feature {
            id: id.clone(),
            name: None,
            suppressed: None,
            definition: FeatureDefinition {
                operation: operation(name),
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: None,
        });
        members.push(Member::Feature(id.clone()));
        items.insert(id, (STREAM.to_owned(), record));
    }
    let definition =
        ProductDefinitionId::mint("test:design:definition#root").expect("identity grammar");
    let at = Position::After(Member::Feature(feature_id(at)));
    ir.design.scopes.push(DesignScope {
        definition: definition.clone(),
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at: at.clone(),
            bodies: bodies.iter().map(|body| body.id.clone()).collect(),
            poses: BTreeMap::new(),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
        unproduced: BTreeSet::new(),
    });
    let held = bodies.iter().map(|body| body.id.clone()).collect();
    ir.geometry.bodies = bodies;
    Design {
        ir,
        scope: Scope {
            definition,
            at,
            bodies: held,
            members,
            items,
        },
    }
}

/// The tail of the item `record` that stores `state` and the no-result flag
/// `s0` of a rolled-back item.
fn rolled_back(record: u64, state: i64) -> Tails<'static> {
    let flags = DesignScopeTailFlags {
        suppression: [false; 2],
        unevaluated: false,
        wide_state: None,
        state_offset: 0,
        no_result: true,
        rolled_back: true,
        no_result_offset: 0,
        envelope_offset: None,
    };
    Tails::from([(
        (STREAM, record),
        Tail {
            state: Some(state),
            previous: None,
            flags: Some(flags),
            history: None,
        },
    )])
}

/// The losses of the states of `design` in an archive with no model blob,
/// after the evaluations go to its document.
fn project_without_blobs(design: &mut Design, tails: &Tails<'_>) -> Vec<LossNote> {
    let bytes = f3d_with_smbh(&[]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("the archive opens");
    let scan = crate::container::scan(&ctx, root).expect("the archive scans");
    let fidelity = SourceFidelity::new(Sha256Digest::digest(b""));
    let scope_histories = std::collections::HashMap::new();
    let history_owners = std::collections::HashMap::new();
    let sources = super::super::StoredStates {
        scan: &scan,
        scope_histories: &scope_histories,
        history_owners: &history_owners,
        fidelity: &fidelity,
    };
    let mut losses = Vec::new();
    let carried = project(
        &ctx,
        &mut design.ir,
        &sources,
        &design.scope,
        tails,
        &mut losses,
    )
    .expect("the states project");
    assert!(carried.is_empty());
    losses
}

/// The tails of the items `records` that store no state id.
fn stateless_tails(records: &[u64]) -> Tails<'static> {
    records
        .iter()
        .map(|record| {
            (
                (STREAM, *record),
                Tail {
                    state: None,
                    previous: None,
                    flags: None,
                    history: None,
                },
            )
        })
        .collect()
}

#[test]
fn a_built_item_with_no_state_id_has_no_evaluation_and_a_loss_names_it() {
    let mut design = design(&["first", "second"], "second", Vec::new());
    let losses = project_without_blobs(&mut design, &stateless_tails(&[10, 11]));
    assert!(design.ir.design.evaluations.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        loss.message.contains(feature_id("first").as_str())
            && loss.message.contains("they store no history state"),
        "{}",
        loss.message
    );
}

#[test]
fn a_new_component_with_no_scope_tail_stores_no_history_state() {
    let mut design = design(&["component", "second"], "second", Vec::new());
    let losses = project_without_blobs(&mut design, &stateless_tails(&[11]));
    assert!(design.ir.design.evaluations.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        loss.message.contains(feature_id("component").as_str())
            && loss.message.contains("they store no history state")
            && !loss.message.contains("scope tail"),
        "{}",
        loss.message
    );
}

#[test]
fn an_item_with_no_scope_tail_has_no_evaluation_and_a_loss_names_it() {
    let mut design = design(&["first", "second"], "second", Vec::new());
    let losses = project_without_blobs(&mut design, &stateless_tails(&[11]));
    assert!(design.ir.design.evaluations.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        loss.message.contains(feature_id("first").as_str())
            && loss.message.contains("its scope tail is not read"),
        "{}",
        loss.message
    );
}

#[test]
fn a_rolled_back_item_whose_state_is_not_stored_has_no_evaluation_and_no_loss() {
    let mut design = design(&["first", "second"], "first", Vec::new());
    let losses = project_without_blobs(&mut design, &rolled_back(11, 7));
    assert!(design.ir.design.evaluations.is_empty());
    assert!(losses.is_empty(), "{losses:#?}");
}

#[test]
fn a_scope_with_configurations_and_no_active_one_states_no_evaluation() {
    let mut design = design(&["first", "second"], "first", Vec::new());
    let definition = design.scope.definition.clone();
    design.ir.design.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("test:design:configuration#large").expect("identity grammar"),
        scope: definition.clone(),
        name: "Large".to_owned(),
        parent: None,
        comment: None,
        overrides: Overrides::default(),
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    });
    let losses = project_without_blobs(&mut design, &rolled_back(11, 7));
    assert!(design.ir.design.evaluations.is_empty());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].code, F3dLossCode::HistoryStatesUnread.kind());
    assert!(
        losses[0].message.contains(definition.as_str()),
        "{}",
        losses[0].message
    );
}

/// One stored history of a fixture: its blob name and stored state ids.
struct Stored {
    name: &'static str,
    states: &'static [i64],
}

fn histories(stored: &[Stored]) -> Vec<History<'static>> {
    stored
        .iter()
        .map(|stored| History {
            blob: Blob {
                bytes: &[],
                stream: stored.name,
                width: RefWidth::Eight,
                namespace: None,
            },
            name: stored.name,
            space: "f3d:brep:".to_owned(),
            states: stored
                .states
                .iter()
                .map(|state| (*state, format!("f3d:asm:state#{state}")))
                .collect(),
        })
        .collect()
}

/// A bound item of history `history` whose state is `state` and whose state
/// before it is `previous`.
fn bound(history: usize, state: i64, previous: Option<i64>) -> Kind {
    Kind::Bound {
        history,
        state,
        previous,
    }
}

/// The heads of `histories`.
fn heads(histories: &[History<'_>]) -> Vec<Option<i64>> {
    histories
        .iter()
        .map(|history| history.states.keys().next_back().copied())
        .collect()
}

/// The entries of the members of `design`, of kinds `kinds`, after the walk
/// binds the states it places among the histories `stored` and marks the
/// current position; and each state so placed.
fn walk<'d>(
    design: &'d Design,
    kinds: Vec<Kind>,
    stored: &[Stored],
) -> (Vec<Entry<'d>>, Vec<(usize, String)>) {
    let mut entries = design
        .scope
        .members
        .iter()
        .zip(kinds)
        .enumerate()
        .map(|(position, (member, kind))| {
            let Member::Feature(feature) = member else {
                panic!("a feature member");
            };
            let (stream, record) = &design.scope.items[feature];
            let role = role(
                &design
                    .ir
                    .feature_definition_in(feature, None)
                    .expect("a definition")
                    .operation,
            );
            Entry {
                feature,
                stream,
                record: *record,
                position,
                stateless: role == Role::Inert,
                moves: role.moves(&kind),
                current: false,
                kind,
            }
        })
        .collect::<Vec<_>>();
    let histories = histories(stored);
    let heads = heads(&histories);
    let inferred = resolve(&mut entries, &heads, &histories);
    mark_current(&mut entries, &heads, &[]);
    (entries, inferred)
}

/// The plan of the walk of `walk` over the histories `stored`.
fn plan_of(entries: &[Entry<'_>], stored: &[Stored]) -> Plan {
    let names = stored.iter().map(|stored| stored.name).collect::<Vec<_>>();
    plan(entries, &heads(&histories(stored)), &names)
}

/// A read history whose head `head` holds `records`, with the stored
/// `states`.
fn read(head: i64, records: Topology, states: Vec<(Want, Result<StateRecords, String>)>) -> Read {
    Read {
        head,
        head_records: records,
        head_consumed: BTreeMap::new(),
        head_unnamed: BTreeSet::new(),
        states,
    }
}

fn want(state: i64, consumed: &[u64]) -> Want {
    Want {
        state,
        consumed: consumed.iter().copied().collect(),
    }
}

fn records(records: Topology) -> StateRecords {
    StateRecords {
        records,
        unknowns: Vec::new(),
        notes: Vec::new(),
    }
}

/// What the evaluations of a fixture state.
struct Outcome {
    evaluations: Vec<Evaluation>,
    carried: Vec<UnknownRecord>,
    losses: Vec<LossNote>,
}

/// The evaluations of the items of `design`, of kinds `kinds`, from the
/// `reads` of the histories `stored` with the consumed bodies `consumed`.
fn run(
    design: &Design,
    kinds: Vec<Kind>,
    stored: &[Stored],
    reads: &[Result<Read, String>],
    consumed: &[Consumed],
    fidelity: &SourceFidelity,
) -> Outcome {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("a context");
    let histories = histories(stored);
    let (entries, inferred) = walk(design, kinds, stored);
    let heads = heads(&histories);
    let Plan { planned, stop } = plan_of(&entries, stored);
    let definition = &design.scope.definition;
    let mut losses = unstated(definition, &entries)
        .into_iter()
        .chain(unresolved(definition, &entries))
        .chain(stop.as_ref().and_then(|stop| stopped(definition, stop)))
        .collect::<Vec<_>>();
    if !planned.is_empty() {
        losses.extend(placed(definition, &entries, &histories, &inferred));
        losses.extend(tails_assumed(definition, &entries, &histories));
    }
    let wants = wants(&entries, &planned, consumed);
    let current = design
        .ir
        .resolved_current_state(&design.scope.definition, &design.scope.at)
        .expect("the current state");
    let held = vec![design.scope.bodies.clone(); histories.len()];
    let owners = history_owners(&held, &current);
    let chain = Chain {
        entries: &entries,
        planned: &planned,
        wants: &wants,
        histories: &histories,
        heads: &heads,
        reads,
        held: &held,
        owners: &owners,
        consumed,
        current: &current,
    };
    let tails = Tails::new();
    let pairs = std::collections::HashMap::new();
    let mut evaluations = Vec::new();
    let mut carried = Vec::new();
    let mut stops = Vec::new();
    let mut marks = Marks::new();
    let mut reader = Reader {
        ctx: &ctx,
        ir: &design.ir,
        fidelity,
        scope: &design.scope,
        tails: &tails,
        pairs: &pairs,
        losses: &mut losses,
    };
    stops.extend(
        reader
            .emit(&chain, None, &mut marks, &mut evaluations, &mut carried)
            .expect("the states emit"),
    );
    losses.extend(
        stops
            .into_iter()
            .filter_map(|stop| stopped(definition, &stop)),
    );
    losses.extend(marked(definition, &histories, &marks));
    losses.extend(poses_assumed(
        definition,
        &entries,
        &planned[..evaluations.len()],
    ));
    Outcome {
        evaluations,
        carried,
        losses,
    }
}

fn no_fidelity() -> SourceFidelity {
    SourceFidelity::new(Sha256Digest::digest(b""))
}

fn after(name: &str) -> Evaluated {
    Evaluated::State(Position::After(Member::Feature(feature_id(name))))
}

fn content(evaluation: &Evaluation) -> (&StateBase, &StateDelta) {
    let content = evaluation.content.as_ref().expect("the state's content");
    (&content.base, &content.delta)
}

#[test]
fn a_later_feature_that_changes_an_earlier_body_gives_each_earlier_state_its_own_records() {
    let design = design(&["a", "b", "c"], "c", vec![body("1", None)]);
    let head = Topology {
        bodies: vec![body("1", None)],
        points: vec![point("p", 3.0)],
        ..Topology::default()
    };
    let state = |x| Topology {
        bodies: vec![body("1", None)],
        points: vec![point("p", x)],
        ..Topology::default()
    };
    let outcome = run(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
        ],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3],
        }],
        &[Ok(read(
            3,
            head,
            vec![
                (want(2, &[]), Ok(records(state(2.0)))),
                (want(1, &[]), Ok(records(state(1.0)))),
            ],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    assert!(outcome.losses.is_empty(), "{:#?}", outcome.losses);
    let [b, a] = outcome.evaluations.as_slice() else {
        panic!("two evaluations: {:#?}", outcome.evaluations);
    };
    assert_eq!(b.of, after("b"));
    assert_eq!(a.of, after("a"));
    assert_eq!(b.native_ref.as_deref(), Some("f3d:asm:state#2"));
    assert_eq!(a.native_ref.as_deref(), Some("f3d:asm:state#1"));
    let (base, delta) = content(b);
    assert_eq!(*base, StateBase::Current(design.scope.at.clone()));
    assert!(delta.records.bodies.is_empty());
    assert_eq!(delta.records.points, [point("p", 2.0)]);
    let (base, delta) = content(a);
    assert_eq!(*base, StateBase::Evaluation(b.id.clone()));
    assert_eq!(delta.records.points, [point("p", 1.0)]);
    assert!(delta.removed.is_empty());
}

#[test]
fn the_state_before_a_combine_holds_its_tool_body() {
    let design = design(&["a", "combine", "c"], "c", vec![body("1", None)]);
    let head = Topology {
        bodies: vec![body("1", None)],
        ..Topology::default()
    };
    let consumed = Consumed {
        placed: BTreeMap::from([(77, 1)]),
        ..Consumed::default()
    };
    let outcome = run(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
        ],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3],
        }],
        &[Ok(read(
            3,
            head.clone(),
            vec![
                (want(2, &[]), Ok(records(head.clone()))),
                (
                    want(1, &[77]),
                    Ok(records(Topology {
                        bodies: vec![body("1", None), body("tool", None)],
                        ..Topology::default()
                    })),
                ),
            ],
        ))],
        &[consumed],
        &no_fidelity(),
    );
    let [combine, a] = outcome.evaluations.as_slice() else {
        panic!("two evaluations: {:#?}", outcome.evaluations);
    };
    assert_eq!(combine.of, after("combine"));
    assert_eq!(*content(combine).1, StateDelta::default());
    assert_eq!(a.of, after("a"));
    assert_eq!(content(a).1.records.bodies, [body("tool", None)]);
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateOwnerAssumed.kind());
    assert!(
        loss.message.contains("f3d:brep:entity#tool")
            && loss.message.contains(design.scope.definition.as_str())
            && !loss.message.contains("f3d:brep:entity#1"),
        "{}",
        loss.message
    );
}

#[test]
fn only_the_states_before_the_first_consumer_of_a_body_want_it() {
    let design = design(&["a", "b", "c", "d"], "d", Vec::new());
    let stored = [Stored {
        name: "BREP.x",
        states: &[1, 2, 3, 4],
    }];
    let (entries, _) = walk(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
            bound(0, 4, Some(3)),
        ],
        &stored,
    );
    let Plan { planned, stop } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    let consumed = Consumed {
        placed: BTreeMap::from([(5, 1), (6, 2)]),
        unplaced: BTreeSet::from([7]),
        ..Consumed::default()
    };
    assert_eq!(
        wants(&entries, &planned, &[consumed]),
        [
            vec![Some(want(3, &[]))],
            vec![Some(want(2, &[6]))],
            vec![Some(want(1, &[5, 6]))],
        ]
    );
}

#[test]
fn a_body_of_the_head_that_no_item_consumes_charges_a_loss() {
    let definition =
        ProductDefinitionId::mint("test:design:definition#root").expect("identity grammar");
    let consumed = Consumed {
        placed: BTreeMap::from([(5, 1)]),
        unplaced: BTreeSet::from([7, 8]),
        ..Consumed::default()
    };
    let mut head = read(3, Topology::default(), Vec::new());
    head.head_consumed =
        BTreeMap::from([(5, body("consumed", None).id), (7, body("hidden", None).id)]);
    let [loss] = omitted(&definition, "BREP.x", &consumed, &head)
        .try_into()
        .expect("one loss");
    assert_eq!(loss.code, F3dLossCode::HistoryStateBodyOmitted.kind());
    assert!(
        loss.message.contains("f3d:brep:entity#hidden")
            && !loss.message.contains("f3d:brep:entity#consumed"),
        "{}",
        loss.message
    );
    head.head_consumed.remove(&7);
    assert!(omitted(&definition, "BREP.x", &consumed, &head).is_empty());
}

#[test]
fn each_item_steps_the_history_that_stores_its_state() {
    let design = design(&["a", "b", "c", "d"], "d", Vec::new());
    let in_x = |at| points(vec![point("x", at)]);
    let in_y = |at| points(vec![point("y", at)]);
    let outcome = run(
        &design,
        vec![
            bound(0, 1, None),
            bound(1, 2, None),
            bound(0, 3, Some(1)),
            bound(1, 4, Some(2)),
        ],
        &[
            Stored {
                name: "BREP.x",
                states: &[1, 3],
            },
            Stored {
                name: "BREP.y",
                states: &[2, 4],
            },
        ],
        &[
            Ok(read(
                3,
                in_x(3.0),
                vec![(want(1, &[]), Ok(records(in_x(1.0))))],
            )),
            Ok(read(
                4,
                in_y(4.0),
                vec![(want(2, &[]), Ok(records(in_y(2.0))))],
            )),
        ],
        &[Consumed::default(), Consumed::default()],
        &no_fidelity(),
    );
    assert!(outcome.losses.is_empty(), "{:#?}", outcome.losses);
    let [c, b, a] = outcome.evaluations.as_slice() else {
        panic!("three evaluations: {:#?}", outcome.evaluations);
    };
    assert_eq!(
        [&c.of, &b.of, &a.of],
        [&after("c"), &after("b"), &after("a")]
    );
    assert_eq!(content(c).1.records.points, [point("y", 2.0)]);
    assert_eq!(content(b).1.records.points, [point("x", 1.0)]);
    assert!(content(a).1.records.points.is_empty());
    assert_eq!(content(a).1.removed, [RecordRef::Point(point("y", 0.0).id)]);
}

#[test]
fn a_state_that_several_histories_store_lies_in_the_history_its_scope_binds() {
    let histories = histories(&[
        Stored {
            name: "BREP.x",
            states: &[1, 2, 5],
        },
        Stored {
            name: "BREP.y",
            states: &[3, 5],
        },
    ]);
    let tail = |history| Tail {
        state: Some(5),
        previous: Some(3),
        flags: None,
        history,
    };
    assert_eq!(
        bind(&histories, &tail(Some("f3d:Breps/BREP.y:asm-history")), 5),
        bound(1, 5, Some(3))
    );
    assert_eq!(
        bind(&histories, &tail(Some("f3d:Breps/BREP.x:asm-history")), 5),
        bound(0, 5, Some(2))
    );
    assert_eq!(
        bind(&histories, &tail(None), 5),
        Kind::Ambiguous {
            state: 5,
            stored: vec![0, 1],
            previous: Some(3),
        }
    );
    assert_eq!(bind(&histories, &tail(None), 2), bound(0, 2, Some(1)));
    assert!(matches!(
        bind(&histories, &tail(None), 4),
        Kind::Unresolved(_)
    ));
}

#[test]
fn an_item_whose_state_is_not_placed_has_no_evaluation_and_the_walk_goes_on() {
    let design = design(&["c", "a", "u", "b", "d"], "d", Vec::new());
    let stored = [Stored {
        name: "BREP.x",
        states: &[1, 2, 3, 4],
    }];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            Kind::Unresolved("the file stores no state 9".to_owned()),
            bound(0, 3, Some(2)),
            bound(0, 4, Some(3)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan { planned, stop } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned
            .iter()
            .map(|planned| (planned.entry, planned.targets.clone()))
            .collect::<Vec<_>>(),
        [(3, vec![Some(3)]), (1, vec![Some(2)]), (0, vec![Some(1)])]
    );
    let [loss] = unresolved(&design.scope.definition, &entries)
        .try_into()
        .expect("one loss");
    assert_eq!(loss.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        loss.message.contains(&format!(
            "the state after feature {} is not stated: the file stores no state 9",
            feature_id("u")
        )) && !loss.message.contains(feature_id("a").as_str()),
        "{}",
        loss.message
    );
}

#[test]
fn an_unplaced_item_that_steps_a_history_ends_the_states_before_it() {
    let design = design(&["c", "a", "u", "b", "d"], "d", Vec::new());
    let stored = [Stored {
        name: "BREP.x",
        states: &[1, 2, 4, 5],
    }];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            Kind::Unresolved("the file stores no state 3".to_owned()),
            bound(0, 4, Some(3)),
            bound(0, 5, Some(4)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan { planned, stop } = plan_of(&entries, &stored);
    assert_eq!(
        planned,
        [Planned {
            entry: 3,
            targets: vec![Some(4)],
        }]
    );
    let stop = stop.expect("a stop");
    assert_eq!(stop.features, [feature_id("a"), feature_id("c")]);
}

#[test]
fn an_item_whose_state_several_histories_store_lies_in_the_one_the_walk_stands_at_that_state_in() {
    let design = design(&["c", "a", "u", "b", "d"], "d", Vec::new());
    let stored = [
        Stored {
            name: "BREP.x",
            states: &[1, 3, 5],
        },
        Stored {
            name: "BREP.y",
            states: &[2, 3, 4],
        },
    ];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 1, None),
            bound(1, 2, None),
            Kind::Ambiguous {
                state: 3,
                stored: vec![0, 1],
                previous: None,
            },
            bound(1, 4, Some(2)),
            bound(0, 5, Some(3)),
        ],
        &stored,
    );
    assert_eq!(entries[2].kind, bound(0, 3, Some(1)));
    let Plan { planned, stop } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned,
        [
            Planned {
                entry: 3,
                targets: vec![Some(3), Some(4)],
            },
            Planned {
                entry: 2,
                targets: vec![Some(3), Some(2)],
            },
            Planned {
                entry: 1,
                targets: vec![Some(1), Some(2)],
            },
            Planned {
                entry: 0,
                targets: vec![Some(1), None],
            },
        ]
    );
    let histories = histories(&stored);
    let [loss] = placed(&design.scope.definition, &entries, &histories, &inferred)
        .try_into()
        .expect("one loss");
    assert_eq!(loss.code, F3dLossCode::HistoryStateInferred.kind());
    assert!(
        loss.message.contains(&format!(
            "the state after feature {} is taken to be state 3 of BREP.x",
            feature_id("u")
        )) && loss.message.contains("leave BREP.x at that state"),
        "{}",
        loss.message
    );
    assert!(unresolved(&design.scope.definition, &entries).is_empty());
}

#[test]
fn an_item_whose_state_several_histories_stand_at_commits_no_state_and_the_walk_goes_on() {
    let design = design(&["c", "e", "u", "b", "d"], "d", Vec::new());
    let stored = [
        Stored {
            name: "BREP.x",
            states: &[3, 5],
        },
        Stored {
            name: "BREP.y",
            states: &[3, 4],
        },
    ];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 3, None),
            bound(1, 3, None),
            Kind::Ambiguous {
                state: 3,
                stored: vec![0, 1],
                previous: None,
            },
            bound(1, 4, Some(3)),
            bound(0, 5, Some(3)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan { planned, stop } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned,
        [
            Planned {
                entry: 3,
                targets: vec![Some(3), Some(4)],
            },
            Planned {
                entry: 1,
                targets: vec![Some(3), Some(3)],
            },
            Planned {
                entry: 0,
                targets: vec![Some(3), None],
            },
        ]
    );
    let [loss] = unresolved(&design.scope.definition, &entries)
        .try_into()
        .expect("one loss");
    assert!(
        loss.message.contains(feature_id("u").as_str())
            && loss
                .message
                .contains("leave none or several of them at that state")
            && loss.message.contains("taken to commit no history state"),
        "{}",
        loss.message
    );
}

#[test]
fn an_item_whose_state_does_not_meet_the_later_items_ends_the_states_there() {
    let design = design(&["a", "b", "c", "d"], "d", Vec::new());
    let stored = [
        Stored {
            name: "BREP.x",
            states: &[1, 2, 3, 4],
        },
        Stored {
            name: "BREP.y",
            states: &[5],
        },
    ];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 1, None),
            bound(1, 5, None),
            bound(0, 3, Some(2)),
            bound(0, 4, Some(3)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan { planned, stop } = plan_of(&entries, &stored);
    assert_eq!(
        planned,
        [Planned {
            entry: 2,
            targets: vec![Some(3), Some(5)],
        }]
    );
    let stop = stop.expect("a stop");
    assert_eq!(stop.features, [feature_id("b"), feature_id("a")]);
}

#[test]
fn a_stopped_chain_names_every_dropped_feature_in_order() {
    let design = design(&["c", "a", "b", "d"], "d", Vec::new());
    let outcome = run(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
            bound(0, 4, Some(3)),
        ],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3, 4],
        }],
        &[Ok(read(
            4,
            points(vec![point("p", 4.0)]),
            vec![(
                want(3, &[]),
                Err("its record table is incomplete".to_owned()),
            )],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    assert!(outcome.evaluations.is_empty());
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        loss.message.contains(&format!(
            "features {}, {}, {} are not stated",
            feature_id("a"),
            feature_id("b"),
            feature_id("c")
        )),
        "{}",
        loss.message
    );
}

#[test]
fn the_states_before_a_joint_whose_state_is_not_read_take_the_current_poses_with_a_loss() {
    let design = design(&["a", "joint", "c"], "c", Vec::new());
    let outcome = run(
        &design,
        vec![bound(0, 1, None), Kind::NoState, bound(0, 2, Some(1))],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2],
        }],
        &[Ok(read(
            2,
            points(vec![point("p", 2.0)]),
            vec![(want(1, &[]), Ok(records(points(vec![point("p", 1.0)]))))],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    let [a] = outcome.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", outcome.evaluations);
    };
    assert_eq!(a.of, after("a"));
    assert_eq!(
        content(a),
        (
            &StateBase::Current(design.scope.at.clone()),
            &StateDelta {
                records: points(vec![point("p", 1.0)]),
                ..StateDelta::default()
            }
        )
    );
    let [unread, poses] = outcome.losses.as_slice() else {
        panic!("two losses: {:#?}", outcome.losses);
    };
    assert_eq!(unread.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        unread
            .message
            .contains(&format!("features {} are not stated", feature_id("joint"))),
        "{}",
        unread.message
    );
    assert_eq!(poses.code, F3dLossCode::HistoryStatePosesAssumed.kind());
    assert!(
        poses.message.contains(&format!(
            "the states after features {} take the current occurrence poses",
            feature_id("a")
        )) && poses
            .message
            .contains(&format!("features {} after them", feature_id("joint"))),
        "{}",
        poses.message
    );
}

#[test]
fn the_states_after_the_last_item_that_moves_occurrences_take_no_pose_loss() {
    let design = design(&["joint", "a", "c"], "c", Vec::new());
    let outcome = run(
        &design,
        vec![Kind::NoState, bound(0, 1, None), bound(0, 2, Some(1))],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2],
        }],
        &[Ok(read(
            2,
            points(vec![point("p", 2.0)]),
            vec![(want(1, &[]), Ok(records(points(vec![point("p", 1.0)]))))],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    assert_eq!(outcome.evaluations.len(), 1);
    assert!(
        outcome
            .losses
            .iter()
            .all(|loss| loss.code != F3dLossCode::HistoryStatePosesAssumed.kind()),
        "{:#?}",
        outcome.losses
    );
}

#[test]
fn an_item_followed_only_by_stateless_items_holds_the_current_state() {
    let stored = [Stored {
        name: "BREP.x",
        states: &[1, 2],
    }];
    let reads = [Ok(read(
        2,
        points(vec![point("p", 2.0)]),
        vec![(want(1, &[]), Ok(records(points(vec![point("p", 1.0)]))))],
    ))];
    let design = design(&["a", "b", "group"], "group", Vec::new());
    let outcome = run(
        &design,
        vec![bound(0, 1, None), bound(0, 2, Some(1)), Kind::NoState],
        &stored,
        &reads,
        &[Consumed::default()],
        &no_fidelity(),
    );
    assert!(outcome.losses.is_empty(), "{:#?}", outcome.losses);
    let [a] = outcome.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", outcome.evaluations);
    };
    assert_eq!(a.of, after("a"));
    let design = self::design(&["a", "u", "group"], "group", Vec::new());
    let outcome = run(
        &design,
        vec![bound(0, 1, None), Kind::NoState, Kind::NoState],
        &stored,
        &reads,
        &[Consumed::default()],
        &no_fidelity(),
    );
    assert_eq!(outcome.evaluations.len(), 1);
    let [tail] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert_eq!(tail.code, F3dLossCode::HistoryStateTailAssumed.kind());
}

#[test]
fn the_one_item_with_no_scope_tail_between_two_items_whose_states_do_not_meet_takes_the_gap() {
    let design = design(&["a", "holes", "c"], "c", Vec::new());
    let at = |x| points(vec![point("p", x)]);
    let outcome = run(
        &design,
        vec![bound(0, 1, None), Kind::Tailless, bound(0, 3, Some(2))],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3],
        }],
        &[Ok(read(
            3,
            at(3.0),
            vec![
                (want(2, &[]), Ok(records(at(2.0)))),
                (want(1, &[]), Ok(records(at(1.0)))),
            ],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    let [holes, a] = outcome.evaluations.as_slice() else {
        panic!("two evaluations: {:#?}", outcome.evaluations);
    };
    assert_eq!([&holes.of, &a.of], [&after("holes"), &after("a")]);
    assert_eq!(holes.native_ref.as_deref(), Some("f3d:asm:state#2"));
    assert_eq!(content(holes).1.records.points, [point("p", 2.0)]);
    assert_eq!(content(a).1.records.points, [point("p", 1.0)]);
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateInferred.kind());
    assert!(
        loss.message.contains(&format!(
            "the state after feature {} is taken to be state 2 of BREP.x",
            feature_id("holes")
        )),
        "{}",
        loss.message
    );
}

#[test]
fn the_one_item_with_no_scope_tail_after_the_last_item_of_a_history_takes_its_head() {
    let design = design(&["a", "holes"], "holes", Vec::new());
    let (entries, inferred) = walk(
        &design,
        vec![bound(0, 1, None), Kind::Tailless],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2],
        }],
    );
    assert_eq!(entries[1].kind, bound(0, 2, Some(1)));
    assert!(entries[1].current && !entries[0].current);
    let [(1, why)] = inferred.as_slice() else {
        panic!("one placed state: {inferred:#?}");
    };
    assert!(why.contains("its state is the head"), "{why}");
}

#[test]
fn two_items_with_no_scope_tail_in_one_gap_end_the_walk_there() {
    let design = design(&["a", "h", "i", "d"], "d", Vec::new());
    let stored = [Stored {
        name: "BREP.x",
        states: &[1, 2, 3, 4],
    }];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 1, None),
            Kind::Tailless,
            Kind::Tailless,
            bound(0, 4, Some(3)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan { planned, stop } = plan_of(&entries, &stored);
    assert!(planned.is_empty());
    assert_eq!(stop.expect("a stop").features, [feature_id("a")]);
}

#[test]
fn a_joint_origin_a_grounding_and_a_rigid_group_change_no_state() {
    let native = |kind| FeatureOperation::Native {
        kind: CodecName::new(&cadmpeg_ir::codec_id!("f3d"), kind).expect("a codec name"),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    for kind in ["JointOrigin", "Ground", "MotionLink"] {
        assert_eq!(role(&native(kind)), Role::Inert, "{kind}");
    }
    assert_eq!(role(&operation("group")), Role::Inert);
    assert_eq!(role(&native("NewComponent")), Role::Assembly);
    assert_eq!(role(&operation("joint")), Role::Assembly);
    assert_eq!(role(&operation("move")), Role::Move);
    assert!(Role::Move.moves(&Kind::NoState) && !Role::Move.moves(&bound(0, 1, None)));
    assert_eq!(role(&operation("extrude")), Role::Geometry);
}

#[test]
fn the_attach_bound_covers_the_text_of_each_native_record() {
    let id = |name: &str| UnknownId::mint(format!("f3d:asm:record#{name}")).expect("an id");
    let records = vec![
        UnknownRecord::retained(id("plain"), 0, vec![1], Vec::new()),
        UnknownRecord::retained(
            id("linked"),
            0,
            vec![2],
            vec![
                "f3d:asm:record#plain".to_owned(),
                "f3d:brep:a\"b".to_owned(),
            ],
        ),
    ];
    let bound = attach_bound(&records).expect("a bound");
    let text = records
        .iter()
        .map(|record| {
            cadmpeg_core::json!({"id": record.id().as_str(), "links": record.links()})
                .to_string()
                .len()
        })
        .sum::<usize>();
    assert!(
        u64::try_from(text).expect("a length") <= bound,
        "{text} {bound}"
    );
    assert_eq!(attach_bound(&[]), Some(0));
}

#[test]
fn a_state_keeps_the_current_name_of_a_changed_entity_and_drops_that_of_a_removed_one() {
    let mut design = design(&["a", "b", "c"], "c", vec![body("1", None)]);
    let named = EntityRef::Body(body("1", None).id);
    let entry = NameEntry {
        name: None,
        native: None,
    };
    design.ir.design.names = BTreeMap::from([(named.clone(), entry)]).into();
    let outcome = run(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
        ],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3],
        }],
        &[Ok(read(
            3,
            Topology {
                bodies: vec![body("1", None)],
                ..Topology::default()
            },
            vec![
                (
                    want(2, &[]),
                    Ok(records(Topology {
                        bodies: vec![body("1", Some("earlier"))],
                        ..Topology::default()
                    })),
                ),
                (want(1, &[]), Ok(records(Topology::default()))),
            ],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    let [b, a] = outcome.evaluations.as_slice() else {
        panic!("two evaluations: {:#?}", outcome.evaluations);
    };
    assert!(content(b).1.names.is_empty());
    assert_eq!(content(a).1.names, BTreeMap::from([(named.clone(), None)]));
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateNameInherited.kind());
    assert!(
        loss.message.contains(b.id.as_str()) && loss.message.contains(named.id()),
        "{}",
        loss.message
    );
}

/// A design whose one earlier state holds a surface whose record is the
/// native record `spline`, which the state holds with `bytes`.
fn surface_state(bytes: Vec<u8>) -> (Design, Vec<Result<Read, String>>, UnknownRecord) {
    let design = design(&["a", "c"], "c", Vec::new());
    let spline = UnknownId::mint("f3d:brep:spline#5").expect("identity grammar");
    let surface = Surface {
        id: SurfaceId::mint("f3d:brep:entity#s").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(spline.clone()),
        }),
        source_object: None,
        native_ref: None,
    };
    let unknown = UnknownRecord::retained(spline, 0, bytes, Vec::new());
    let unreferenced = UnknownRecord::retained(
        UnknownId::mint("f3d:brep:spline#6").expect("identity grammar"),
        0,
        vec![9],
        Vec::new(),
    );
    let reads = vec![Ok(read(
        2,
        Topology::default(),
        vec![(
            want(1, &[]),
            Ok(StateRecords {
                records: Topology {
                    surfaces: vec![surface],
                    ..Topology::default()
                },
                unknowns: vec![unknown.clone(), unreferenced],
                notes: Vec::new(),
            }),
        )],
    ))];
    (design, reads, unknown)
}

#[test]
fn a_state_carries_the_native_records_its_records_reference() {
    let (design, reads, unknown) = surface_state(vec![1, 2, 3]);
    let outcome = run(
        &design,
        vec![bound(0, 1, None), bound(0, 2, Some(1))],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2],
        }],
        &reads,
        &[Consumed::default()],
        &no_fidelity(),
    );
    assert!(outcome.losses.is_empty(), "{:#?}", outcome.losses);
    assert_eq!(outcome.evaluations.len(), 1);
    assert_eq!(outcome.carried, [unknown]);
}

#[test]
fn a_state_whose_native_record_differs_from_the_documents_ends_the_states() {
    let (design, reads, unknown) = surface_state(vec![1, 2, 3]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("a context");
    let mut fidelity = no_fidelity();
    let document = UnknownRecord::retained(unknown.id().clone(), 0, vec![4], Vec::new());
    fidelity
        .attach_native_unknown_records(&mut CadIr::empty(), "f3d", vec![document], &ctx)
        .expect("the document's record");
    let outcome = run(
        &design,
        vec![bound(0, 1, None), bound(0, 2, Some(1))],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2],
        }],
        &reads,
        &[Consumed::default()],
        &fidelity,
    );
    assert!(outcome.evaluations.is_empty());
    assert!(outcome.carried.is_empty());
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert!(
        loss.message.contains(unknown.id().as_str()),
        "{}",
        loss.message
    );
}

#[test]
fn the_walk_of_a_history_starts_at_the_state_after_its_last_item() {
    let design = design(&["a", "b", "c", "d", "e"], "e", Vec::new());
    let at = |x| points(vec![point("p", x)]);
    let outcome = run(
        &design,
        vec![
            bound(0, 3, Some(2)),
            bound(0, 4, Some(3)),
            bound(0, 5, Some(4)),
            bound(0, 6, Some(5)),
            Kind::NoState,
        ],
        &[Stored {
            name: "BREP.x",
            states: &[2, 3, 4, 5, 6, 7],
        }],
        &[Ok(read(
            7,
            at(7.0),
            vec![
                (want(6, &[]), Ok(records(at(6.0)))),
                (want(5, &[]), Ok(records(at(5.0)))),
                (want(4, &[]), Ok(records(at(4.0)))),
                (want(3, &[]), Ok(records(at(3.0)))),
            ],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateTailAssumed.kind());
    assert!(
        loss.message.contains("the states 7 of BREP.x")
            && loss
                .message
                .contains(&format!("after state 6 of feature {}", feature_id("d"))),
        "{}",
        loss.message
    );
    let [d, c, b, a] = outcome.evaluations.as_slice() else {
        panic!("four evaluations: {:#?}", outcome.evaluations);
    };
    assert_eq!(
        [&d.of, &c.of, &b.of, &a.of],
        [&after("d"), &after("c"), &after("b"), &after("a")]
    );
    assert_eq!(d.native_ref.as_deref(), Some("f3d:asm:state#6"));
    let (base, delta) = content(d);
    assert_eq!(*base, StateBase::Current(design.scope.at.clone()));
    assert_eq!(delta.records.points, [point("p", 6.0)]);
    assert_eq!(content(c).1.records.points, [point("p", 5.0)]);
    assert_eq!(content(a).1.records.points, [point("p", 3.0)]);
}

#[test]
fn the_walk_of_each_history_starts_at_the_state_after_its_last_item() {
    let design = design(&["a", "b", "c", "d"], "d", Vec::new());
    let in_x = |at| points(vec![point("x", at)]);
    let in_y = |at| points(vec![point("y", at)]);
    let outcome = run(
        &design,
        vec![
            bound(0, 3, Some(2)),
            bound(1, 2, None),
            bound(0, 4, Some(3)),
            bound(1, 3, Some(2)),
        ],
        &[
            Stored {
                name: "BREP.x",
                states: &[2, 3, 4, 5],
            },
            Stored {
                name: "BREP.y",
                states: &[2, 3, 4],
            },
        ],
        &[
            Ok(read(
                5,
                in_x(5.0),
                vec![
                    (want(4, &[]), Ok(records(in_x(4.0)))),
                    (want(3, &[]), Ok(records(in_x(3.0)))),
                ],
            )),
            Ok(read(
                4,
                in_y(4.0),
                vec![(want(2, &[]), Ok(records(in_y(2.0))))],
            )),
        ],
        &[Consumed::default(), Consumed::default()],
        &no_fidelity(),
    );
    let [tail_x, tail_y] = outcome.losses.as_slice() else {
        panic!("two losses: {:#?}", outcome.losses);
    };
    assert!(
        tail_x.code == F3dLossCode::HistoryStateTailAssumed.kind()
            && tail_y.code == F3dLossCode::HistoryStateTailAssumed.kind(),
        "{tail_x:#?} {tail_y:#?}"
    );
    assert!(
        tail_x.message.contains("the states 5 of BREP.x")
            && tail_y.message.contains("the states 4 of BREP.y"),
        "{tail_x:#?} {tail_y:#?}"
    );
    let [c, b, a] = outcome.evaluations.as_slice() else {
        panic!("three evaluations: {:#?}", outcome.evaluations);
    };
    assert_eq!(
        [&c.of, &b.of, &a.of],
        [&after("c"), &after("b"), &after("a")]
    );
    assert_eq!(
        content(c).1.records.points,
        [point("x", 4.0), point("y", 2.0)]
    );
    assert_eq!(content(b).1.records.points, [point("x", 3.0)]);
    assert!(content(a).1.records.points.is_empty());
    assert_eq!(content(a).1.removed, [RecordRef::Point(point("y", 0.0).id)]);
}

/// The owner of a history-only pair placed with the current bodies
/// `peers`, in the naming space of `definition`.
fn pair(peers: &[&str], definition: Option<&ProductDefinitionId>) -> HistoryPairOwner {
    HistoryPairOwner {
        peers: peers.iter().map(|name| body(name, None).id).collect(),
        definition: definition.cloned(),
    }
}

#[test]
fn a_consumed_body_takes_the_owner_of_its_peers_and_one_with_no_known_owner_is_omitted() {
    let mut design = design(
        &["a", "combine", "c"],
        "c",
        vec![body("1", None), body("2", None)],
    );
    let part = ProductDefinitionId::mint("test:design:definition#part").expect("identity grammar");
    let mut definition = ProductDefinition::new(part.clone(), Storage::Embedded);
    definition.bodies = vec![body("2", None).id];
    design.ir.model.product_definitions.push(definition);
    let current = design
        .ir
        .resolved_current_state(&design.scope.definition, &design.scope.at)
        .expect("the current state");
    let [history] = history_owners(&[design.scope.bodies.clone()], &current)
        .try_into()
        .expect("one history");
    assert_eq!(history, None);
    let mut consumed = Consumed {
        placed: BTreeMap::from([(77, 1), (78, 1)]),
        ..Consumed::default()
    };
    consumed.own(
        &BTreeMap::from([(77, pair(&["2"], None)), (78, pair(&["1", "2"], None))]),
        &BTreeSet::from([&part]),
        history.as_ref(),
        &current,
    );
    assert_eq!(consumed.placed, BTreeMap::from([(77, 1)]));
    assert_eq!(
        consumed.owners,
        BTreeMap::from([(77, (part.clone(), Rule::Cluster))])
    );
    assert_eq!(consumed.unowned, BTreeSet::from([78]));
    let head = Topology {
        bodies: vec![body("1", None), body("2", None)],
        ..Topology::default()
    };
    let mut stored = read(
        3,
        head.clone(),
        vec![
            (want(2, &[]), Ok(records(head.clone()))),
            (
                want(1, &[77]),
                Ok(records(Topology {
                    bodies: vec![body("1", None), body("2", None), body("tool", None)],
                    ..Topology::default()
                })),
            ),
        ],
    );
    stored.head_consumed =
        BTreeMap::from([(77, body("tool", None).id), (78, body("other", None).id)]);
    let [loss] = omitted(&design.scope.definition, "BREP.x", &consumed, &stored)
        .try_into()
        .expect("one loss");
    assert_eq!(loss.code, F3dLossCode::HistoryStateBodyOmitted.kind());
    assert!(
        loss.message.contains("f3d:brep:entity#other")
            && !loss.message.contains("f3d:brep:entity#tool"),
        "{}",
        loss.message
    );
    let outcome = run(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
        ],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3],
        }],
        &[Ok(stored)],
        &[consumed],
        &no_fidelity(),
    );
    let [combine, a] = outcome.evaluations.as_slice() else {
        panic!("two evaluations: {:#?}", outcome.evaluations);
    };
    assert_eq!(combine.of, after("combine"));
    assert_eq!(a.of, after("a"));
    let delta = content(a).1;
    assert_eq!(delta.records.bodies, [body("tool", None)]);
    assert_eq!(
        delta.owners,
        BTreeMap::from([(body("tool", None).id, Some(part.clone()))])
    );
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert_eq!(loss.code, F3dLossCode::ComponentBodyOwnerFallback.kind());
    assert!(
        loss.message.contains("f3d:brep:entity#tool") && loss.message.contains(part.as_str()),
        "{}",
        loss.message
    );
}

#[test]
fn a_consumed_body_with_no_current_peer_takes_its_cluster_definition_else_the_history_owner() {
    let mut design = design(&["a", "combine", "c"], "c", vec![body("1", None)]);
    let part = ProductDefinitionId::mint("test:design:definition#part").expect("identity grammar");
    let other =
        ProductDefinitionId::mint("test:design:definition#other").expect("identity grammar");
    let mut definition = ProductDefinition::new(part.clone(), Storage::Embedded);
    definition.bodies = vec![body("1", None).id];
    design.ir.model.product_definitions.push(definition);
    let current = design
        .ir
        .resolved_current_state(&design.scope.definition, &design.scope.at)
        .expect("the current state");
    let [history] = history_owners(&[design.scope.bodies.clone()], &current)
        .try_into()
        .expect("one history");
    assert_eq!(history.as_ref(), Some(&part));
    let mut consumed = Consumed {
        placed: BTreeMap::from([(76, 1), (77, 1), (78, 1)]),
        ..Consumed::default()
    };
    consumed.own(
        &BTreeMap::from([
            (76, pair(&["gone"], Some(&other))),
            (77, pair(&[], Some(&part))),
        ]),
        &BTreeSet::from([&other]),
        history.as_ref(),
        &current,
    );
    assert_eq!(
        consumed.owners,
        BTreeMap::from([
            (76, (other.clone(), Rule::Cluster)),
            (77, (part.clone(), Rule::Assumed)),
            (78, (part.clone(), Rule::Assumed)),
        ])
    );
    let mut alone = Consumed {
        placed: BTreeMap::from([(79, 1)]),
        ..Consumed::default()
    };
    alone.own(&BTreeMap::new(), &BTreeSet::new(), None, &current);
    assert!(alone.placed.is_empty() && alone.owners.is_empty());
    assert_eq!(alone.unowned, BTreeSet::from([79]));
}

#[test]
fn head_bodies_and_body_map_pairs_that_no_state_joins_charge_losses() {
    let definition =
        ProductDefinitionId::mint("test:design:definition#root").expect("identity grammar");
    let consumed = Consumed {
        placed: BTreeMap::from([(5, 1), (6, 1)]),
        pairs: BTreeMap::from([
            (5, BTreeSet::from(["f3d:design:pair#5".to_owned()])),
            (6, BTreeSet::from(["f3d:design:pair#6".to_owned()])),
        ]),
        ..Consumed::default()
    };
    let mut head = read(3, Topology::default(), Vec::new());
    head.head_consumed = BTreeMap::from([(5, body("consumed", None).id)]);
    head.head_unnamed = BTreeSet::from([body("loose", None).id]);
    let [unnamed, unselected] = omitted(&definition, "BREP.x", &consumed, &head)
        .try_into()
        .expect("two losses");
    assert_eq!(unnamed.code, F3dLossCode::HistoryStateBodyOmitted.kind());
    assert!(
        unnamed.message.contains("f3d:brep:entity#loose"),
        "{}",
        unnamed.message
    );
    assert_eq!(unselected.code, F3dLossCode::HistoryStateBodyOmitted.kind());
    assert!(
        unselected.message.contains("f3d:design:pair#6")
            && !unselected.message.contains("f3d:design:pair#5"),
        "{}",
        unselected.message
    );
}

#[test]
fn an_admission_note_of_a_record_that_a_state_states_charges_a_loss_keyed_by_its_evaluation() {
    let design = design(&["a", "b", "c"], "c", Vec::new());
    let note = |name: &str| Note::SpringPcurve {
        curve: point(name, 0.0).id.as_str().to_owned(),
    };
    let state = StateRecords {
        notes: vec![note("own"), note("held")],
        ..records(points(vec![point("own", 1.0), point("held", 2.0)]))
    };
    let outcome = run(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
        ],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3],
        }],
        &[Ok(read(
            3,
            points(vec![point("held", 2.0)]),
            vec![
                (want(2, &[]), Ok(state)),
                (want(1, &[]), Ok(records(Topology::default()))),
            ],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    let [b, _] = outcome.evaluations.as_slice() else {
        panic!("two evaluations: {:#?}", outcome.evaluations);
    };
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert_eq!(loss.code, F3dLossCode::SpringSupportPcurveUnresolved.kind());
    assert!(
        loss.message
            .contains(&format!("{} {}", b.id, point("own", 0.0).id)),
        "{}",
        loss.message
    );
}

#[test]
fn a_current_body_in_no_history_stays_in_each_earlier_state() {
    let mesh = Body {
        id: BodyId::mint("f3d:mesh:entity#imported").expect("identity grammar"),
        ..body("mesh", None)
    };
    let design = design(&["a", "b"], "b", vec![body("1", None), mesh.clone()]);
    let outcome = run(
        &design,
        vec![bound(0, 1, None), bound(0, 2, Some(1))],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2],
        }],
        &[Ok(read(
            2,
            Topology {
                bodies: vec![body("1", None)],
                ..Topology::default()
            },
            vec![(want(1, &[]), Ok(records(points(vec![point("p", 1.0)]))))],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    let [a] = outcome.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", outcome.evaluations);
    };
    let (_, delta) = content(a);
    assert!(delta.removed.contains(&RecordRef::Body(body("1", None).id)));
    assert!(!delta.removed.contains(&RecordRef::Body(mesh.id.clone())));
    assert!(!delta.records.bodies.iter().any(|body| body.id == mesh.id));
}

#[test]
fn a_current_body_in_no_history_is_inherited_by_every_state_with_a_loss() {
    let definition =
        ProductDefinitionId::mint("test:design:definition#root").expect("identity grammar");
    let bodies = [
        body("kept", None).id,
        BodyId::mint("f3d:mesh:entity#imported").expect("identity grammar"),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let (held, inherited) = held(&bodies, &["f3d:brep:"]);
    assert_eq!(held, vec![BTreeSet::from([body("kept", None).id])]);
    let loss = inherited_bodies(&definition, &inherited).expect("a loss");
    assert_eq!(loss.code, F3dLossCode::HistoryStateBodyInherited.kind());
    assert!(
        loss.message.contains("f3d:mesh:entity#imported")
            && !loss.message.contains("f3d:brep:entity#kept"),
        "{}",
        loss.message
    );
    assert!(inherited_bodies(&definition, &BTreeSet::new()).is_none());
}
