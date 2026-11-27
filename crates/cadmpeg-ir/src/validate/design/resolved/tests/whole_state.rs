// SPDX-License-Identifier: Apache-2.0
//! The topology rules of each stored state, read on the region its change
//! reaches, against the rules read on the whole base and the whole state:
//! each state copied from its resolved base, the rules run on both copies,
//! and the state's findings that the base lacks reported against it.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use super::super::{locate, records, Arena, Located, Resolver};
use super::cost::history;
use crate::design::evaluation::{EvaluationStatus, StateBase, StateContent, StateDelta};
use crate::design::refs::RecordRef;
use crate::document::{CadIr, Topology};
use crate::features::geometry::FinitePoint3;
use crate::ids::{PointId, VertexId};
use crate::index::ModelIndex;
use crate::math::Point3;
use crate::report::check::{Check, Finding};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use crate::schema::EntitySchema;
use crate::validate::design::fixture::{design_findings, document, id};
use crate::validate::design::records::{DesignRecords, PositionKey};
use crate::validate::nested_finding;
use crate::validate::topology::nested;

/// `records` as a `Topology` of their own, in id order.
fn topology(records: &BTreeMap<&str, Located<'_>>) -> Topology {
    let mut topology = Topology::default();
    for located in records.values() {
        located.copy_into(&mut topology);
    }
    topology.finalize();
    topology
}

/// `state` with `delta` applied: each removed record the state holds in its
/// arena goes, then each record of the delta replaces the record of its id.
fn apply<'a>(
    mut state: BTreeMap<&'a str, Located<'a>>,
    delta: &'a StateDelta,
) -> BTreeMap<&'a str, Located<'a>> {
    for removed in &delta.removed {
        let (arena, id) = Arena::of_record(removed);
        if state.get(id).is_some_and(|located| located.arena == arena) {
            state.remove(id);
        }
    }
    for (arena, id, index) in records(&delta.records) {
        state.insert(
            id,
            Located {
                arena,
                topology: &delta.records,
                index,
            },
        );
    }
    state
}

/// The resolved state of the evaluation `at`: its base resolved, with its
/// delta applied. `None` where its chain does not resolve.
fn resolve<'a>(
    ir: &'a CadIr,
    resolver: &Resolver<'a, '_, '_>,
    at: usize,
    depth: usize,
) -> Result<Option<BTreeMap<&'a str, Located<'a>>>, CodecError> {
    let Some(content) = ir.design.evaluations[at].content.as_ref() else {
        return Ok(None);
    };
    let Some(base) = resolve_base(ir, resolver, at, content, depth)? else {
        return Ok(None);
    };
    Ok(Some(apply(base, &content.delta)))
}

fn resolve_base<'a>(
    ir: &'a CadIr,
    resolver: &Resolver<'a, '_, '_>,
    at: usize,
    content: &'a StateContent,
    depth: usize,
) -> Result<Option<BTreeMap<&'a str, Located<'a>>>, CodecError> {
    if depth > ir.design.evaluations.len() {
        return Ok(None);
    }
    match &content.base {
        StateBase::Empty => Ok(Some(BTreeMap::new())),
        StateBase::Current(position) => {
            let scope = ir.design.evaluations[at].scope.as_str();
            let current = ir
                .design
                .scopes
                .iter()
                .filter(|candidate| candidate.definition.as_str() == scope)
                .flat_map(|candidate| &candidate.current)
                .find(|current| PositionKey::of(&current.at) == PositionKey::of(position));
            match current {
                Some(current) => resolver
                    .geometry_extent(&current.bodies)
                    .map(|extent| Some(extent.records)),
                None => Ok(None),
            }
        }
        StateBase::Evaluation(base) => {
            let first = ir
                .design
                .evaluations
                .iter()
                .position(|evaluation| evaluation.id == *base);
            match first {
                Some(first) => resolve(ir, resolver, first, depth + 1),
                None => Ok(None),
            }
        }
    }
}

/// The topology findings of every stored evaluation, each state and its base
/// read whole.
fn whole_state_findings(ir: &CadIr) -> Vec<String> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let ids = ModelIndex::build(ir);
    let index = DesignRecords::new(ir);
    let resolver = Resolver {
        ctx: &ctx,
        index: &index,
        geometry: locate(&ir.geometry),
        children: ir.geometry.child_lists(),
    };
    let mut findings = Vec::new();
    {
        let _depth = ctx.enter_nested("IR neutral validation").unwrap();
        for (at, evaluation) in ir.design.evaluations.iter().enumerate() {
            let Some(content) = evaluation.content.as_ref() else {
                continue;
            };
            let Some(base) = resolve_base(ir, &resolver, at, content, 0).unwrap() else {
                continue;
            };
            let state = apply(base.clone(), &content.delta);
            let rules = |records| {
                nested::check(
                    &ctx,
                    ids.native_view(),
                    topology(records),
                    None,
                    nested::NestRules::All {
                        caches: evaluation.status == EvaluationStatus::Current,
                    },
                )
                .unwrap()
            };
            let inherited: BTreeSet<String> = rules(&base)
                .iter()
                .map(|finding| format!("{finding:?}"))
                .collect();
            for finding in rules(&state) {
                if !inherited.contains(&format!("{finding:?}")) {
                    let finding = nested_finding(
                        finding,
                        Check::Design,
                        evaluation.id.as_str(),
                        "resolved state",
                    );
                    findings.push(format!("{finding:?}"));
                }
            }
        }
    }
    ctx.finish_session().unwrap();
    findings
}

/// The topology findings of every stored evaluation, as design validation
/// reports them.
fn region_findings(ir: &CadIr) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .filter(|finding: &Finding| finding.message.starts_with("resolved state:"))
        .map(|finding| format!("{finding:?}"))
        .collect()
}

fn in_cube<T: EntitySchema + Clone>(records: &[T], cube: usize) -> Vec<T> {
    let scope = format!(":cube-{cube}:");
    records
        .iter()
        .filter(|record| record.identity().contains(&scope))
        .cloned()
        .collect()
}

fn delta(ir: &mut CadIr, step: usize) -> &mut StateDelta {
    &mut ir.design.evaluations[step].content.as_mut().unwrap().delta
}

fn rebase(ir: &mut CadIr, step: usize, base: usize) {
    ir.design.evaluations[step].content.as_mut().unwrap().base =
        StateBase::Evaluation(id("evaluation", &format!("step-{base}")));
}

/// The point at the origin corner of `cube`, at `z`.
fn corner(ir: &CadIr, cube: usize, z: f64) -> crate::topology::Point {
    let mut point = in_cube(&ir.geometry.points, cube)
        .into_iter()
        .find(|point| point.position().get() == Point3::new(0.0, 0.0, 0.0))
        .unwrap();
    point.set_position(FinitePoint3::new(Point3::new(0.0, 0.0, z)).unwrap());
    point
}

/// The vertex of `cube` at its origin corner.
fn corner_vertex(ir: &CadIr, cube: usize) -> crate::topology::Vertex {
    let point = corner(ir, cube, 0.0).id;
    in_cube(&ir.geometry.vertices, cube)
        .into_iter()
        .find(|vertex| vertex.point == point)
        .unwrap()
}

/// A change to the stored history of four cubes whose four evaluations each
/// rest on the next, the last on the current state.
struct Case {
    name: &'static str,
    /// Whether the change is made to breach a topology rule.
    breaches: bool,
    change: fn(&mut CadIr),
}

fn cases() -> [Case; 8] {
    [
        Case {
            name: "a removed record that others reference",
            breaches: true,
            change: |ir| {
                let coedge = in_cube(&ir.geometry.coedges, 1).remove(0).id;
                delta(ir, 1).removed.push(RecordRef::Coedge(coedge));
            },
        },
        Case {
            name: "a renamed record",
            breaches: false,
            change: |ir| {
                let mut vertex = corner_vertex(ir, 2);
                delta(ir, 1)
                    .removed
                    .push(RecordRef::Vertex(vertex.id.clone()));
                vertex.id = VertexId::mint(format!("{}-renamed", vertex.id.as_str())).unwrap();
                delta(ir, 1).records.vertices.push(vertex);
            },
        },
        Case {
            name: "a record of another arena under the same id",
            breaches: false,
            change: |ir| {
                let curve = in_cube(&ir.geometry.curves, 3).remove(0).id;
                let mut point = corner(ir, 3, 0.0);
                point.id = PointId::mint(curve.as_str()).unwrap();
                delta(ir, 1).removed.push(RecordRef::Curve(curve));
                delta(ir, 1).records.points.push(point);
            },
        },
        Case {
            name: "a moved point that breaches its edges through their vertex",
            breaches: true,
            change: |ir| {
                let point = corner(ir, 1, -0.05);
                delta(ir, 1).records.points = vec![point];
            },
        },
        Case {
            name: "a breach of another cube that the current state already has",
            breaches: true,
            change: |ir| {
                let moved = corner(ir, 0, -0.05);
                let at = ir
                    .geometry
                    .points
                    .iter()
                    .position(|point| point.id == moved.id)
                    .unwrap();
                ir.geometry.points[at] = moved;
                let point = corner(ir, 1, -0.05);
                delta(ir, 1).records.points = vec![point];
            },
        },
        Case {
            name: "two states on one base",
            breaches: true,
            change: |ir| {
                rebase(ir, 0, 2);
                let point = corner(ir, 1, -0.05);
                delta(ir, 0).records.points = vec![point];
                let edge = in_cube(&ir.geometry.edges, 1).remove(0);
                delta(ir, 1).records.edges.push(edge);
            },
        },
        Case {
            name: "a breach and its repair along a chain",
            breaches: true,
            change: |ir| {
                let point = corner(ir, 1, -0.05);
                delta(ir, 2).records.points = vec![point];
                let edge = in_cube(&ir.geometry.edges, 1).remove(0);
                delta(ir, 1).records.edges.push(edge);
                let point = corner(ir, 1, 0.0);
                delta(ir, 0).records.points = vec![point];
            },
        },
        Case {
            name: "a record removed and inserted again further up a chain",
            breaches: true,
            change: |ir| {
                let vertex = corner_vertex(ir, 2);
                delta(ir, 2)
                    .removed
                    .push(RecordRef::Vertex(vertex.id.clone()));
                let point = corner(ir, 2, -0.05);
                let up = delta(ir, 0);
                up.records.vertices.push(vertex);
                up.records.points.push(point);
            },
        },
    ]
}

#[test]
fn the_region_of_a_change_gives_the_findings_of_the_whole_state() {
    for case in cases() {
        let mut ir = history(4, 4);
        (case.change)(&mut ir);
        let whole = whole_state_findings(&ir);
        assert_eq!(region_findings(&ir), whole, "{}", case.name);
        assert!(!case.breaches || !whole.is_empty(), "{}", case.name);
    }
}

/// `topology` with `test:model:` in each identity replaced by `test:<scope>:`.
fn rescoped(topology: Topology, scope: &str) -> Topology {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let mut map =
        IdentityMap::new(|id: &str| Ok(id.replacen("test:model:", &format!("test:{scope}:"), 1)));
    let rescoped = topology.rewrite_identities(&ctx, &mut map).unwrap();
    map.finish().unwrap();
    ctx.finish_session().unwrap();
    rescoped
}

#[test]
fn a_region_that_reaches_the_work_cap_is_read_whole() {
    // Each copy of the carrier chain holds a curve whose evaluation runs out
    // of a work slice of 64 steps. The second state adds a second copy beside
    // the first: its region holds the second copy only, the whole state both.
    use crate::test_support::evaluation_work::{doubling_model, with_evaluation_cap};
    let (chain, _) = doubling_model(8);
    let mut ir = document();
    let first = ir.design.evaluations[0].id.clone();
    ir.design.evaluations[0].content = Some(StateContent {
        base: StateBase::Empty,
        delta: StateDelta {
            records: rescoped(chain.geometry.clone(), "first"),
            ..StateDelta::default()
        },
    });
    let second = &mut ir.design.evaluations[1];
    second.status = EvaluationStatus::Current;
    second.content = Some(StateContent {
        base: StateBase::Evaluation(first),
        delta: StateDelta {
            records: rescoped(chain.geometry, "second"),
            ..StateDelta::default()
        },
    });
    let (region, whole) =
        with_evaluation_cap(64, || (region_findings(&ir), whole_state_findings(&ir)));
    assert_eq!(region, whole);
    let second = ir.design.evaluations[1].id.as_str();
    assert!(
        whole.iter().any(
            |finding| finding.contains(second) && finding.contains("MAX_MODEL_EVALUATION_WORK")
        ),
        "{whole:#?}"
    );
}
