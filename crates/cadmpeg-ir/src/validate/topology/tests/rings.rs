// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::ids::{CoedgeId, CurveId, EdgeId};
use crate::report::Check;
use crate::validate::validate_neutral;

#[test]
fn non_finite_body_transform_is_invalid() {
    let mut ir = unit_cube();
    let mut transform = crate::transform::Transform::identity();
    transform.rows[2][3] = f64::NAN;
    ir.model.bodies[0].transform = Some(transform);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::Bounds && finding.message.contains("non-finite")
        }));
}

#[test]
fn dangling_reference_is_flagged() {
    let mut ir = unit_cube();
    // Point a coedge's edge at something that does not exist.
    ir.model.coedges[0].edge = EdgeId("does-not-exist".into());
    let report = validate_neutral(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|f| f.check == Check::ReferentialIntegrity));
    assert!(!report.is_ok());
}

#[test]
fn coedge_use_curve_requires_a_resolved_carrier() {
    let mut ir = unit_cube();
    ir.model.coedges[0].use_curve = Some(crate::topology::CoedgeUseCurve {
        curve: CurveId("missing:use-curve#0".into()),
        parameter_range: [0.0, 1.0],
    });
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity && finding.message.contains("coedge use curve")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain
            && finding.message.contains("outside its carrier domain")
    }));
}

#[test]
fn broken_loop_ring_is_flagged() {
    let mut ir = unit_cube();
    // Redirect a coedge's `next` to a valid coedge in a different loop, so the
    // referenced id resolves but the ring no longer closes.
    let foreign = ir.model.coedges[20].id.clone();
    ir.model.coedges[0].next = foreign;
    let report = validate_neutral(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::LoopClosure),
        "expected a loop-closure finding, got: {:?}",
        report.findings
    );
}

#[test]
fn mismatched_partner_edge_is_flagged() {
    let mut ir = unit_cube();
    // Force a coedge's partner to reference a coedge on a different edge by
    // repointing the partner's edge. Find coedge[0]'s partner and change it.
    let partner_id: CoedgeId = ir.model.coedges[0].radial_next.clone();
    let other_edge = ir
        .model
        .coedges
        .iter()
        .find(|c| c.edge != ir.model.coedges[0].edge)
        .unwrap()
        .edge
        .clone();
    for c in &mut ir.model.coedges {
        if c.id == partner_id {
            c.edge = other_edge.clone();
        }
    }
    let report = validate_neutral(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::CoedgePairing),
        "expected a coedge-pairing finding, got: {:?}",
        report.findings
    );
}

#[test]
fn new_topology_references_are_validated() {
    let mut ir = unit_cube();
    ir.model.shells[0]
        .wire_edges
        .push(EdgeId("missing-wire".into()));
    ir.model.shells[0]
        .free_vertices
        .push(crate::ids::VertexId("missing-free".into()));
    ir.model.coedges[0].radial_next = CoedgeId("missing-radial".into());

    let report = validate_neutral(&ir, Vec::new());
    let messages = report
        .findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|message| message.contains("wire edge")));
    assert!(messages
        .iter()
        .any(|message| message.contains("free vertex")));
    assert!(messages
        .iter()
        .any(|message| message.contains("coedge(radial_next)")));
}

#[test]
fn two_member_radial_ring_with_equal_senses_warns() {
    let mut ir = unit_cube();
    let other_id = ir.model.coedges[0].radial_next.clone();
    let sense = ir.model.coedges[0].sense;
    ir.model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == other_id)
        .unwrap()
        .sense = sense;
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::CoedgePairing
                && finding.severity == crate::report::Severity::Warning
        }));
}

#[test]
fn coedge_backed_edge_cannot_be_a_wire_edge() {
    let mut ir = unit_cube();
    ir.model.shells[0]
        .wire_edges
        .push(ir.model.coedges[0].edge.clone());
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::WireTopology));
}

#[test]
fn wire_and_free_topology_negative_cases_are_reported() {
    let mut ir = unit_cube();

    let mut unowned_edge = ir.model.edges[0].clone();
    unowned_edge.id.0 = "synthetic:test:edge#unowned".into();
    ir.model.edges.push(unowned_edge);

    let mut duplicate_edge = ir.model.edges[1].clone();
    duplicate_edge.id.0 = "synthetic:test:edge#duplicate".into();
    ir.model.shells[0]
        .wire_edges
        .extend([duplicate_edge.id.clone(), duplicate_edge.id.clone()]);
    ir.model.edges.push(duplicate_edge);

    let mut unowned_vertex = ir.model.vertices[0].clone();
    unowned_vertex.id.0 = "synthetic:test:vertex#unowned".into();
    ir.model.vertices.push(unowned_vertex);

    ir.model.shells[0]
        .free_vertices
        .push(ir.model.edges[0].start.clone());
    ir.model.bodies[0].kind = crate::topology::BodyKind::Wire;
    ir.finalize();

    let findings = validate_neutral(&ir, Vec::new()).findings;
    for message in [
        "wire edge must belong to exactly one shell",
        "free vertex must belong to exactly one shell",
        "free vertex is also referenced by an edge",
        "wire body contains faces",
    ] {
        assert!(
            findings.iter().any(|finding| {
                finding.check == Check::WireTopology && finding.message == message
            }),
            "missing `{message}` in {findings:?}"
        );
    }
}

#[test]
fn singular_loop_vertex_cannot_have_multiple_free_shell_owners() {
    let mut ir = unit_cube();
    let vertex = ir.model.vertices[0].id.clone();
    ir.model.loops[0].boundary = crate::topology::LoopBoundary::Vertex {
        vertex: vertex.clone(),
        pcurves: Vec::new(),
    };
    ir.model.shells[0].free_vertices.push(vertex.clone());
    let mut second_shell = ir.model.shells[0].clone();
    second_shell.id.0 = "synthetic:test:shell#second".into();
    second_shell.faces.clear();
    second_shell.wire_edges.clear();
    second_shell.free_vertices = vec![vertex];
    ir.model.regions[0].shells.push(second_shell.id.clone());
    ir.model.shells.push(second_shell);

    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::WireTopology
                && finding.message == "free vertex must belong to exactly one shell"
        }));
}

#[test]
fn empty_shell_is_reported() {
    let mut ir = unit_cube();
    ir.model.shells[0].faces.clear();
    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::WireTopology && finding.message == "shell owns no topology"
    }));
}

#[test]
fn carrierless_edge_range_requires_finite_values_but_not_ordering() {
    let mut ir = unit_cube();
    ir.model.edges[0].curve = None;
    ir.model.edges[0].param_range = Some([1.0, 0.0]);
    let report = validate_neutral(&ir, Vec::new());
    assert!(!report.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain
            && finding.entity.as_deref() == Some(ir.model.edges[0].id.0.as_str())
    }));

    ir.model.edges[0].param_range = Some([f64::NAN, 0.0]);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain
            && finding.entity.as_deref() == Some(ir.model.edges[0].id.0.as_str())
    }));
}

#[test]
fn vertex_loop_is_valid_and_exclusive_with_coedges() {
    let mut ir = unit_cube();
    let face_id = ir.model.faces[0].id.clone();
    let vertex_id = ir.model.vertices[0].id.clone();
    let loop_id = crate::ids::LoopId("synthetic:cube:vertex-loop#0".into());
    ir.model.loops.push(crate::topology::Loop {
        id: loop_id.clone(),
        face: face_id,
        boundary_role: crate::topology::LoopBoundaryRole::Inner,
        boundary: crate::topology::LoopBoundary::Vertex {
            vertex: vertex_id,
            pcurves: Vec::new(),
        },
    });
    ir.model.faces[0].loops.push(loop_id.clone());
    ir.model.finalize();
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:#?}", report.findings);

    ir.model
        .loops
        .iter_mut()
        .find(|loop_| loop_.id == loop_id)
        .unwrap()
        .boundary_role = crate::topology::LoopBoundaryRole::Outer;
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::LoopClosure
            && finding.message == "face has more than one explicit outer loop"
    }));
    ir.model
        .loops
        .iter_mut()
        .find(|loop_| loop_.id == loop_id)
        .unwrap()
        .boundary_role = crate::topology::LoopBoundaryRole::Inner;
}
