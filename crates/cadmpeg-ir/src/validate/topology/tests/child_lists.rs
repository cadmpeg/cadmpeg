// SPDX-License-Identifier: Apache-2.0

use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::check::{Check, Finding};
use crate::validate::validate_neutral;

fn findings(ir: &crate::CadIr) -> Vec<Finding> {
    validate_neutral(ir, Vec::new())
        .expect("validate the neutral model")
        .findings
}

fn has(findings: &[Finding], check: Check, entity: &str, message: &str) -> bool {
    findings.iter().any(|finding| {
        finding.check == check
            && finding.entity.as_deref() == Some(entity)
            && finding.message.contains(message)
    })
}

#[test]
fn the_unit_cube_states_reciprocal_child_lists() {
    let ir = unit_cube().unwrap();
    let report = findings(&ir);
    assert!(
        !report
            .iter()
            .any(|finding| finding.message.contains("listed")),
        "{report:?}"
    );
    let shell = &ir.geometry.shells[0];
    assert_eq!(ir.geometry.shell_faces(&shell.id).count(), 6);
    let body = &ir.geometry.bodies[0];
    assert_eq!(ir.geometry.body_regions(&body.id).count(), 1);
    let lists = ir.geometry.child_lists();
    assert_eq!(lists.shell_faces(&shell.id).len(), 6);
    assert_eq!(lists.body_regions(&body.id).len(), 1);
}

#[test]
fn a_stored_child_list_must_name_the_children_whose_parent_is_its_owner() {
    let mut ir = unit_cube().unwrap();
    let region = ir.geometry.regions[0].id.as_str().to_owned();
    let shell = ir.geometry.regions[0].shells.remove(0);
    let report = findings(&ir);
    assert!(has(
        &report,
        Check::ReferentialIntegrity,
        shell.as_str(),
        &format!("is not listed by its region `{region}`")
    ));

    let mut ir = unit_cube().unwrap();
    let stolen = ir.geometry.faces[1].loops.to_vec()[0].clone();
    let face = ir.geometry.faces[0].id.as_str().to_owned();
    let mut loops = ir.geometry.faces[0].loops.to_vec();
    loops.push(stolen.clone());
    ir.geometry.faces[0].loops = crate::topology::FaceLoops::unspecified(loops);
    let report = findings(&ir);
    assert!(has(
        &report,
        Check::ReferentialIntegrity,
        &face,
        &format!(
            "lists loop `{}`, whose parent is another record",
            stolen.as_str()
        )
    ));
}

#[test]
fn an_edge_with_no_vertex_needs_an_unbounded_closed_carrier_and_a_loop_of_its_own() {
    let mut ir = unit_cube().unwrap();
    let edge = ir.geometry.edges[0].id.as_str().to_owned();
    ir.geometry.edges[0].ends = None;
    let report = findings(&ir);
    assert_eq!(
        has(
            &report,
            Check::GeometricConsistency,
            &edge,
            "edge with no vertex must have a carrier curve and no parameter range"
        ),
        ir.geometry.edges[0].param_range().is_some()
    );
    let coedge = ir
        .geometry
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == edge)
        .unwrap();
    assert!(has(
        &report,
        Check::RingOrder,
        coedge.id.as_str(),
        "coedge of an edge with no vertex must be the only coedge of its loop"
    ));

    ir.geometry.edges[0].set_param_range(None);
    let report = findings(&ir);
    assert!(has(
        &report,
        Check::GeometricConsistency,
        &edge,
        "edge with no vertex lies on an open carrier curve"
    ));
}

#[test]
fn a_catmull_clark_cage_states_one_knot_interval() {
    let mut ir = directed_subd_sum().unwrap();
    let subd = ir.geometry.subds[0].id.as_str().to_owned();
    ir.geometry.subds[0]
        .cage
        .edit_edges(|edges| {
            edges[0] = crate::subd::SubdEdge::new(
                [0, 1],
                [0.25, 0.75],
                crate::subd::SubdEdgeTag::Crease,
                Some(1.0),
                [0.125, 0.875],
            )?;
            edges[1] = crate::subd::SubdEdge::new(
                [1, 2],
                [0.0, 0.5],
                crate::subd::SubdEdgeTag::SmoothX,
                Some(2.0),
                [0.25, 0.75],
            )?;
            Ok(())
        })
        .unwrap();
    let message = "Catmull-Clark cage edges state different knot intervals";
    assert!(has(
        &findings(&ir),
        Check::GeometricConsistency,
        &subd,
        message
    ));
    ir.geometry.subds[0].scheme = crate::subd::SubdScheme::TSpline;
    assert!(!has(
        &findings(&ir),
        Check::GeometricConsistency,
        &subd,
        message
    ));
}
