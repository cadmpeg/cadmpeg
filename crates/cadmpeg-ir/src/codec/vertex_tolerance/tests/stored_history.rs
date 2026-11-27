// SPDX-License-Identifier: Apache-2.0
//! A document whose current state holds many cubes and whose evaluations
//! each restate a few records of one of them: a stored history of one
//! design.

use std::time::Instant;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::super::{widen_measuring, Measured};
use crate::annotations::{AnnotationBuilder, Annotations, StreamHandle};
use crate::codec::loss::DecodeLossCode;
use crate::design::evaluation::{StateBase, StateDelta};
use crate::design::refs::RecordRef;
use crate::document::{CadIr, Topology};
use crate::features::geometry::FinitePoint3;
use crate::geometry::analytic::LineCurve;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
use crate::math::{Point3, Vector3};
use crate::report::loss::LossNote;
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use crate::validate::design::fixture::{document, id};

/// The cube's records with `:cube:` in each identity replaced by
/// `:cube-<copy>:`.
fn cube_copy(cube: &Topology, copy: usize) -> Topology {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let mut map =
        IdentityMap::new(|id: &str| Ok(id.replacen(":cube:", &format!(":cube-{copy}:"), 1)));
    let copied = cube.clone().rewrite_identities(&ctx, &mut map).unwrap();
    map.finish().unwrap();
    ctx.finish_session().unwrap();
    copied
}

/// The design fixture with `bodies` cubes in its current state, and
/// `evaluations` stored states: the last on the current state, each other on
/// the next. Evaluation `k` restates the first point of cube `k % bodies`,
/// moved by (-0.02, -0.02, -0.02): off the ends and the lines of the three
/// edges at that corner.
pub(super) fn history(bodies: usize, evaluations: usize) -> CadIr {
    let mut ir = document();
    let cube = std::mem::take(&mut ir.geometry);
    for copy in 0..bodies {
        let mut copied = cube_copy(&cube, copy);
        let geometry = &mut ir.geometry;
        geometry.bodies.append(&mut copied.bodies);
        geometry.regions.append(&mut copied.regions);
        geometry.shells.append(&mut copied.shells);
        geometry.faces.append(&mut copied.faces);
        geometry.loops.append(&mut copied.loops);
        geometry.coedges.append(&mut copied.coedges);
        geometry.edges.append(&mut copied.edges);
        geometry.vertices.append(&mut copied.vertices);
        geometry.points.append(&mut copied.points);
        geometry.surfaces.append(&mut copied.surfaces);
        geometry.curves.append(&mut copied.curves);
        geometry.pcurves.append(&mut copied.pcurves);
    }
    ir.geometry.finalize();
    ir.design.scopes[0].current[0].bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    let template = ir.design.evaluations[0].clone();
    ir.design.evaluations.clear();
    for k in 0..evaluations {
        let mut evaluation = template.clone();
        evaluation.id = id("evaluation", &format!("step-{k}"));
        let content = evaluation.content.as_mut().unwrap();
        if k + 1 < evaluations {
            content.base = StateBase::Evaluation(id("evaluation", &format!("step-{}", k + 1)));
        }
        let mut point = ir.geometry.points[(k % bodies) * cube.points.len()].clone();
        let at = point.position().get();
        point.set_position(
            FinitePoint3::new(Point3::new(at.x - 0.02, at.y - 0.02, at.z - 0.02)).unwrap(),
        );
        content.delta.records.points.push(point);
        ir.design.evaluations.push(evaluation);
    }
    ir
}

/// The document, annotations, and losses of the pass on `ir`, each vertex
/// of its current geometry annotated as read from offset 11 of a stream,
/// measuring each resolved state as `measured`.
fn widened(mut ir: CadIr, measured: Measured) -> (CadIr, Annotations, Vec<LossNote>) {
    let stream = StreamHandle::new(crate::stream_name!("source"));
    let mut builder = AnnotationBuilder::new();
    for vertex in &ir.geometry.vertices {
        builder.note(vertex.id.as_str(), &stream, 11, None);
    }
    let mut annotations = builder.build();
    let losses = widen_measuring(
        &mut ir,
        &mut annotations,
        &DecodePolicy::default(),
        measured,
    );
    (ir, annotations, losses)
}

#[test]
fn measuring_the_region_of_each_stored_state_widens_as_measuring_it_whole() {
    // Six chained evaluations over three cubes, each restating a moved
    // corner; besides, `step-2` removes an edge of cube 1 and restates
    // nothing, and `step-4` also restates the line of cube 2's first edge
    // moved 0.05 along -z.
    let mut ir = history(3, 6);
    let edge = ir
        .geometry
        .edges
        .iter()
        .find(|edge| edge.id.as_str().contains(":cube-1:"))
        .unwrap()
        .id
        .clone();
    let mut line = ir
        .geometry
        .curves
        .iter()
        .find(|curve| {
            curve.id.as_str().contains(":cube-2:") && curve.id.as_str().ends_with("curve#0")
        })
        .unwrap()
        .clone();
    line.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, -0.05), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    ));
    for evaluation in &mut ir.design.evaluations {
        let delta = &mut evaluation.content.as_mut().unwrap().delta;
        if evaluation.id.as_str().ends_with("step-2") {
            delta.records = Topology::default();
            delta.removed.push(RecordRef::Edge(edge.clone()));
        } else if evaluation.id.as_str().ends_with("step-4") {
            delta.records.curves.push(line.clone());
        }
    }
    let whole = widened(ir.clone(), Measured::Whole);
    let region = widened(ir, Measured::Region);
    let widening = DecodeLossCode::VertexToleranceWidened.note("").code;
    assert!(
        whole.2.iter().any(|loss| loss.code == widening),
        "{:?}",
        whole.2
    );
    assert_eq!(region, whole);
}

#[test]
fn a_state_whose_chain_of_bases_does_not_resolve_is_charged_and_the_others_are_measured() {
    // Three chained evaluations over three cubes, each restating a moved
    // corner; `step-0`'s base names no evaluation.
    let mut ir = history(3, 3);
    let missing = id("evaluation", "missing");
    ir.design.evaluations[0].content.as_mut().unwrap().base = StateBase::Evaluation(missing);
    let (ir, _, losses) = widened(ir, Measured::Region);
    let unmeasured = DecodeLossCode::VertexToleranceUnmeasured.note("").code;
    let charged: Vec<&LossNote> = losses
        .iter()
        .filter(|loss| loss.code == unmeasured)
        .collect();
    let [charged] = charged[..] else {
        panic!("one unmeasured loss: {losses:?}");
    };
    let step = |k: usize| &ir.design.evaluations[k];
    assert!(
        charged
            .message
            .ends_with(&format!(": {}", step(0).id.as_str())),
        "{}",
        charged.message
    );
    let vertices = |k: usize| &step(k).content.as_ref().unwrap().delta.records.vertices;
    assert!(vertices(0).is_empty(), "{:?}", vertices(0));
    for k in [1, 2] {
        let [vertex] = &vertices(k)[..] else {
            panic!("one widened vertex in step {k}: {:?}", vertices(k));
        };
        assert!(vertex.tolerance.is_some(), "{vertex:?}");
    }
}

#[test]
fn a_state_on_a_settled_base_widens_nothing_outside_its_delta_region() {
    // `after-extrude`, on the current state, moves corner 0 off its three
    // edges, corner 1 along edge 0 toward corner 0, and corner 2 along edge 1
    // toward corner 1; measured until a measure widens nothing, it widens all
    // three. `a-chained`, based on it, changes no record: its resolved state
    // widens nothing, measured whole or over its delta's region.
    let mut ir = document();
    let points = super::chained_corner_points(&ir);
    let base = &mut ir.design.evaluations[0];
    base.content.as_mut().unwrap().delta.records.points = points;
    let mut chained = base.clone();
    chained.id = id("evaluation", "a-chained");
    let content = chained.content.as_mut().unwrap();
    content.base = StateBase::Evaluation(base.id.clone());
    content.delta = StateDelta::default();
    ir.design.evaluations.insert(0, chained);
    let whole = widened(ir.clone(), Measured::Whole);
    let region = widened(ir, Measured::Region);
    let delta = |at: usize| {
        &whole.0.design.evaluations[at]
            .content
            .as_ref()
            .unwrap()
            .delta
    };
    assert!(delta(0).records.vertices.is_empty(), "{:?}", delta(0));
    assert_eq!(delta(1).records.vertices.len(), 3, "{:?}", delta(1));
    super::assert_close(
        super::vertex_tolerance(&delta(1).records, 2).unwrap(),
        0.011_718_75,
    );
    assert_eq!(region, whole);
}

/// Prints the time of the vertex tolerance pass on stored histories of
/// growing size.
#[test]
#[ignore = "timing probe: cargo test -p cadmpeg-ir --lib -- --ignored --nocapture stored_history_widening_cost"]
fn stored_history_widening_cost() {
    for bodies in [16, 64] {
        for evaluations in [16, 64] {
            let mut times = Vec::new();
            for measured in [Measured::Region, Measured::Whole] {
                let mut ir = history(bodies, evaluations);
                let mut annotations = Annotations::default();
                let started = Instant::now();
                let losses = widen_measuring(
                    &mut ir,
                    &mut annotations,
                    &DecodePolicy::default(),
                    measured,
                );
                times.push(started.elapsed().as_secs_f64() * 1e3);
                assert!(!losses.is_empty());
            }
            let ir = history(bodies, evaluations);
            let started = Instant::now();
            let index = crate::index::ModelIndex::build(&ir);
            crate::index::identities::IdentitySet::contains(&index, "");
            let index = started.elapsed().as_secs_f64() * 1e3;
            eprintln!(
                "bodies {bodies:>3} evaluations {evaluations:>3}: region {:>9.1} ms, whole {:>9.1} \
                 ms, of which the model index {index:>7.1} ms",
                times[0], times[1]
            );
        }
    }
}
