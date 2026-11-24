// SPDX-License-Identifier: Apache-2.0

use crate::design::evaluation::{EvaluationStatus, StateBase};
use crate::document::{CadIr, Topology};
use crate::features::geometry::FinitePoint3;
use crate::geometry::analytic::CircleCurve;
use crate::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use crate::math::{Point3, Vector3};
use crate::report::Severity;
use crate::topology::{Edge, EdgeEnds, FaceLoops, Loop, Shell};
use crate::validate::design::fixture::{
    assert_finding, assert_no_design_finding, design_findings, document, id, set_base,
};

/// The records of the first evaluation's delta, which the baseline stores
/// against the current state after the fillet.
fn delta(ir: &mut CadIr) -> &mut Topology {
    &mut ir.design.evaluations[0]
        .content
        .as_mut()
        .unwrap()
        .delta
        .records
}

fn curve_of(ir: &CadIr, edge: &Edge) -> Curve {
    ir.geometry
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve())
        .unwrap()
        .clone()
}

fn loop_of(ir: &CadIr, face: usize) -> Loop {
    let face = &ir.geometry.faces[face].id;
    ir.geometry
        .loops
        .iter()
        .find(|loop_| &loop_.face == face)
        .unwrap()
        .clone()
}

fn messages(ir: &CadIr, fragment: &str) -> Vec<(Option<String>, String)> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| finding.message.contains(fragment))
        .map(|finding| (finding.entity, finding.message))
        .collect()
}

/// The first evaluation's delta restates the cube's corner at the origin,
/// moved by `distance` along -z.
fn move_origin_corner(ir: &mut CadIr, distance: f64) {
    let mut corner = ir
        .geometry
        .points
        .iter()
        .find(|point| point.position().get() == Point3::new(0.0, 0.0, 0.0))
        .unwrap()
        .clone();
    corner.set_position(FinitePoint3::new(Point3::new(0.0, 0.0, -distance)).unwrap());
    delta(ir).points = vec![corner];
}

#[test]
fn a_current_state_extent_holds_the_records_its_derived_child_lists_reach() {
    let mut ir = document();
    let face = ir.geometry.faces[0].clone();
    let region = ir.geometry.regions[0].clone();
    let loop_ = loop_of(&ir, 0);
    let records = delta(&mut ir);
    records.faces.push(face);
    records.regions.push(region);
    records.loops.push(loop_);
    assert_no_design_finding(&ir);
}

#[test]
fn an_edge_end_belongs_to_the_resolved_state_closure() {
    let mut ir = document();
    set_base(&mut ir, 0, StateBase::Empty);
    let edge = ir.geometry.edges[0].clone();
    let ends = edge.ends.clone().unwrap();
    let curve = curve_of(&ir, &edge);
    let records = delta(&mut ir);
    records.edges.push(edge.clone());
    records.curves.push(curve.clone());
    assert_finding(
        &ir,
        &format!(
            "references {}, which the state does not hold",
            ends.start.as_str()
        ),
    );

    let mut ghost = edge.clone();
    ghost.ends = Some(EdgeEnds::new(id("vertex", "ghost"), ends.end.clone()));
    delta(&mut ir).edges = vec![ghost];
    assert_finding(
        &ir,
        "references test:design:vertex#ghost, which the state does not hold",
    );

    let mut ir = document();
    set_base(&mut ir, 0, StateBase::Empty);
    let vertices: Vec<_> = ir
        .geometry
        .vertices
        .iter()
        .filter(|vertex| vertex.id == ends.start || vertex.id == ends.end)
        .cloned()
        .collect();
    let points: Vec<_> = ir
        .geometry
        .points
        .iter()
        .filter(|point| vertices.iter().any(|vertex| vertex.point == point.id))
        .cloned()
        .collect();
    let records = delta(&mut ir);
    records.edges.push(edge);
    records.curves.push(curve);
    records.vertices = vertices;
    records.points = points;
    assert!(messages(&ir, "which the state does not hold").is_empty());
}

#[test]
fn a_wire_edge_of_a_state_belongs_to_exactly_one_shell() {
    let mut ir = document();
    set_base(&mut ir, 0, StateBase::Empty);
    let edge = ir.geometry.edges[0].clone();
    let edge_id = edge.id.as_str().to_owned();
    let curve = curve_of(&ir, &edge);
    let records = delta(&mut ir);
    records.edges.push(edge);
    records.curves.push(curve);
    let evaluation = ir.design.evaluations[0].id.as_str().to_owned();
    assert_eq!(
        messages(&ir, "wire edge must belong to exactly one shell"),
        vec![(
            Some(evaluation),
            format!("resolved state: record {edge_id}: wire edge must belong to exactly one shell")
        )]
    );
}

#[test]
fn a_delta_lists_children_whose_parent_is_its_lister() {
    let mut ir = document();
    let mut face = ir.geometry.faces[0].clone();
    let stolen = loop_of(&ir, 1).id;
    let mut loops = face.loops.boundary().expect("a complete face").to_vec();
    loops.push(stolen.clone());
    face.loops = FaceLoops::unspecified(loops);
    let face_id = face.id.as_str().to_owned();
    delta(&mut ir).faces.push(face);
    assert_finding(
        &ir,
        &format!(
            "record {face_id}: lists loop `{}`, whose parent is another record",
            stolen.as_str()
        ),
    );

    let mut ir = document();
    let mut face = ir.geometry.faces[0].clone();
    let own = loop_of(&ir, 0).id;
    face.loops = FaceLoops::unspecified(vec![own.clone(), own.clone()]);
    delta(&mut ir).faces.push(face);
    assert_finding(
        &ir,
        &format!(
            "record {face_id}: face lists loop `{}` more than once",
            own.as_str()
        ),
    );

    let mut ir = document();
    let mut moved = loop_of(&ir, 0);
    let other = ir.geometry.faces[1].id.as_str().to_owned();
    moved.face = ir.geometry.faces[1].id.clone();
    let moved_id = moved.id.as_str().to_owned();
    delta(&mut ir).loops.push(moved);
    assert_finding(
        &ir,
        &format!("record {face_id}: lists loop `{moved_id}`, whose parent is another record"),
    );
    assert_finding(
        &ir,
        &format!("record {moved_id}: is not listed by its face `{other}`"),
    );

    let mut ir = document();
    let lister = loop_of(&ir, 0);
    let target = loop_of(&ir, 1);
    let mut coedge = ir
        .geometry
        .coedges
        .iter()
        .find(|coedge| coedge.owner_loop == lister.id)
        .unwrap()
        .clone();
    coedge.owner_loop = target.id.clone();
    let coedge_id = coedge.id.as_str().to_owned();
    delta(&mut ir).coedges.push(coedge);
    assert_finding(
        &ir,
        &format!(
            "record {}: lists coedge `{coedge_id}`, whose parent is another record",
            lister.id.as_str()
        ),
    );
    assert_finding(
        &ir,
        &format!(
            "record {coedge_id}: is not listed by its loop `{}`",
            target.id.as_str()
        ),
    );
}

#[test]
fn a_delta_lists_each_shell_and_shell_member_once_and_by_its_region() {
    let mut ir = document();
    let mut region = ir.geometry.regions[0].clone();
    let shell = region.stated_shells().next().expect("a shell").clone();
    region.retain_stated_shells(|listed| listed != &shell);
    let region_id = region.id.as_str().to_owned();
    delta(&mut ir).regions.push(region);
    assert_finding(
        &ir,
        &format!(
            "record {}: is not listed by its region `{region_id}`",
            shell.as_str()
        ),
    );

    let mut ir = document();
    let mut extra = ir.geometry.regions[0].clone();
    extra.id = id("region", "extra");
    delta(&mut ir).regions.push(extra);
    assert_finding(
        &ir,
        &format!(
            "record test:design:region#extra: lists shell `{}`, whose parent is another record",
            shell.as_str()
        ),
    );

    let mut ir = document();
    let original = ir.geometry.shells[0].clone();
    let edge = ir.geometry.edges[0].id.clone();
    let shell = Shell::with_members(
        original.id.clone(),
        original.region.clone(),
        vec![edge.clone(), edge.clone()],
        Vec::new(),
    );
    delta(&mut ir).shells.push(shell);
    assert_finding(
        &ir,
        &format!(
            "record {}: shell lists member `{}` more than once",
            original.id.as_str(),
            edge.as_str()
        ),
    );
}

#[test]
fn a_breach_of_the_base_is_reported_where_the_base_is_checked() {
    let mut ir = document();
    let moved = loop_of(&ir, 0).id;
    let other = ir.geometry.faces[1].id.clone();
    let at = ir
        .geometry
        .loops
        .iter()
        .position(|loop_| loop_.id == moved)
        .unwrap();
    ir.geometry.loops[at].face = other;
    assert_no_design_finding(&ir);

    let mut ir = document();
    let mut moved = loop_of(&ir, 0);
    moved.face = ir.geometry.faces[1].id.clone();
    delta(&mut ir).loops.push(moved);
    let first = ir.design.evaluations[0].id.clone();
    set_base(&mut ir, 1, StateBase::Evaluation(first.clone()));
    let found = messages(&ir, "is not listed by its face");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0.as_deref(), Some(first.as_str()));
}

#[test]
fn a_breach_is_reported_once_along_a_chain_by_the_delta_that_makes_it() {
    let mut ir = document();
    let face = ir.geometry.faces[0].clone();
    let mut moved = loop_of(&ir, 0);
    moved.face = ir.geometry.faces[1].id.clone();
    let moved_id = moved.id.as_str().to_owned();
    delta(&mut ir).loops.push(moved.clone());
    let first = ir.design.evaluations[0].id.clone();
    let second = ir.design.evaluations[1].id.clone();
    set_base(&mut ir, 1, StateBase::Evaluation(first.clone()));
    ir.design.evaluations[1]
        .content
        .as_mut()
        .unwrap()
        .delta
        .records
        .faces
        .push(face.clone());
    let lists = format!(
        "resolved state: record {}: lists loop `{moved_id}`, whose parent is another record",
        face.id.as_str()
    );
    let found = messages(&ir, &lists);
    assert_eq!(
        found,
        vec![(Some(first.as_str().to_owned()), lists.clone())]
    );

    delta(&mut ir).loops.clear();
    ir.design.evaluations[1]
        .content
        .as_mut()
        .unwrap()
        .delta
        .records
        .loops
        .push(moved);
    let found = messages(&ir, &lists);
    assert_eq!(found, vec![(Some(second.as_str().to_owned()), lists)]);
}

#[test]
fn an_edge_with_no_vertex_in_a_delta_lies_alone_on_a_closed_carrier() {
    let mut ir = document();
    let mut edge = ir.geometry.edges[0].clone();
    edge.ends = None;
    let edge_id = edge.id.as_str().to_owned();
    let coedge = ir
        .geometry
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge.id)
        .unwrap()
        .id
        .as_str()
        .to_owned();
    delta(&mut ir).edges.push(edge.clone());
    assert_finding(
        &ir,
        &format!(
            "record {edge_id}: edge with no vertex must have a carrier curve and no parameter range"
        ),
    );
    assert_finding(
        &ir,
        &format!(
            "record {coedge}: coedge of an edge with no vertex must be the only coedge of its loop"
        ),
    );

    edge.set_param_range(None);
    let curve = curve_of(&ir, &edge);
    delta(&mut ir).edges = vec![edge];
    assert_finding(
        &ir,
        &format!("record {edge_id}: edge with no vertex lies on an open carrier curve"),
    );
    assert!(messages(&ir, "must have a carrier curve").is_empty());

    let mut circle = curve;
    circle.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    ));
    delta(&mut ir).curves.push(circle);
    assert!(messages(&ir, "open carrier").is_empty());
    assert_finding(&ir, "must be the only coedge of its loop");
}

#[test]
fn a_radial_ring_of_a_delta_closes_on_one_edge() {
    let mut ir = document();
    let first = ir.geometry.coedges[0].clone();
    let other = ir
        .geometry
        .coedges
        .iter()
        .find(|coedge| coedge.edge != first.edge)
        .unwrap()
        .id
        .clone();
    let mut crossing = first.clone();
    crossing.radial_next = other;
    delta(&mut ir).coedges.push(crossing);
    let evaluation = ir.design.evaluations[0].id.as_str().to_owned();
    let found = messages(&ir, "radial ring crosses edges");
    assert!(
        found.contains(&(
            Some(evaluation),
            format!(
                "resolved state: record {}: radial ring crosses edges",
                first.id.as_str()
            )
        )),
        "{found:?}"
    );
}

#[test]
fn an_edge_curve_of_a_state_meets_its_vertices_within_the_allowance() {
    let mut ir = document();
    move_origin_corner(&mut ir, crate::units::COINCIDENCE_TOLERANCE);
    assert!(messages(&ir, "miss the edge's vertex positions").is_empty());

    let mut ir = document();
    move_origin_corner(&mut ir, 2.0 * crate::units::COINCIDENCE_TOLERANCE);
    let evaluation = ir.design.evaluations[0].id.as_str();
    let found = messages(&ir, "miss the edge's vertex positions");
    assert!(!found.is_empty());
    assert!(found
        .iter()
        .all(|(entity, _)| entity.as_deref() == Some(evaluation)));
    assert!(design_findings(&ir)
        .iter()
        .filter(|finding| finding.message.contains("miss the edge's vertex positions"))
        .all(|finding| finding.severity == Severity::Error));
}

#[test]
fn a_stale_state_keeps_its_breaches_of_the_topology_rules() {
    let mut ir = document();
    move_origin_corner(&mut ir, 2.0 * crate::units::COINCIDENCE_TOLERANCE);
    ir.design.evaluations[0].status = EvaluationStatus::Stale;
    assert_finding(&ir, "miss the edge's vertex positions");
}
