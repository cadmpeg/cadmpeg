// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::ids::{CoedgeId, CurveId, EdgeId};
use crate::report::check::Check;
use crate::validate::validate_neutral;

#[test]
fn dangling_reference_is_flagged() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    // Point a coedge's edge at something that does not exist.
    ir.geometry.coedges[0].edge =
        EdgeId::mint("test:model:entity#does-not-exist").expect("valid identity");
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(report
        .findings
        .iter()
        .any(|f| f.check == Check::ReferentialIntegrity));
    assert!(!report.is_ok());
}

#[test]
fn coedge_use_curve_requires_a_resolved_carrier() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.geometry.coedges[0].use_curve = Some(crate::topology::CoedgeUseCurve {
        curve: CurveId::mint("missing:model:use-curve#0").expect("valid identity"),
        parameter_range: crate::topology::ParameterInterval::new([0.0, 1.0]).unwrap(),
    });
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity && finding.message.contains("coedge use curve")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain
            && finding
                .message
                .contains("coedge use-curve names no curve carrier")
    }));
}

#[test]
fn mismatched_partner_edge_is_flagged() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    // Force a coedge's partner to reference a coedge on a different edge by
    // repointing the partner's edge. Find coedge[0]'s partner and change it.
    let partner_id: CoedgeId = ir.geometry.coedges[0].radial_next.clone();
    let other_edge = ir
        .geometry
        .coedges
        .iter()
        .find(|c| c.edge != ir.geometry.coedges[0].edge)
        .unwrap()
        .edge
        .clone();
    for c in &mut ir.geometry.coedges {
        if c.id == partner_id {
            c.edge = other_edge.clone();
        }
    }
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
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
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.geometry.shells[0]
        .add_wire_edge(EdgeId::mint("test:model:entity#missing-wire").expect("valid identity"));
    ir.geometry.shells[0].add_free_vertex(
        crate::ids::VertexId::mint("test:model:entity#missing-free").expect("valid identity"),
    );
    ir.geometry.coedges[0].radial_next =
        CoedgeId::mint("test:model:entity#missing-radial").expect("valid identity");

    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
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
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let other_id = ir.geometry.coedges[0].radial_next.clone();
    let sense = ir.geometry.coedges[0].sense;
    ir.geometry
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == other_id)
        .unwrap()
        .sense = sense;
    assert!(validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::CoedgePairing
                && finding.severity == crate::report::Severity::Warning
        }));
}

#[test]
fn coedge_backed_edge_cannot_be_a_wire_edge() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.geometry.shells[0].add_wire_edge(ir.geometry.coedges[0].edge.clone());
    assert!(validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .iter()
        .any(|finding| finding.check == Check::WireTopology));
}

#[test]
fn wire_and_free_topology_negative_cases_are_reported() {
    let mut ir = unit_cube().expect("valid unit cube fixture");

    let mut unowned_edge = ir.geometry.edges[0].clone();
    unowned_edge.id = "synthetic:test:edge#unowned"
        .try_into()
        .expect("valid identity");
    ir.geometry.edges.push(unowned_edge);

    let mut duplicate_edge = ir.geometry.edges[1].clone();
    duplicate_edge.id = "synthetic:test:edge#duplicate"
        .try_into()
        .expect("valid identity");
    {
        for member in [duplicate_edge.id.clone(), duplicate_edge.id.clone()] {
            ir.geometry.shells[0].add_wire_edge(member);
        }
    };
    ir.geometry.edges.push(duplicate_edge);

    let mut unowned_vertex = ir.geometry.vertices[0].clone();
    unowned_vertex.id = "synthetic:test:vertex#unowned"
        .try_into()
        .expect("valid identity");
    ir.geometry.vertices.push(unowned_vertex);

    ir.geometry.shells[0]
        .add_free_vertex(ir.geometry.edges[0].ends.as_ref().unwrap().start.clone());
    ir.geometry.bodies[0].kind = crate::topology::BodyKind::Wire;
    ir.finalize();

    let findings = validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .findings;
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
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let vertex = ir.geometry.vertices[0].id.clone();
    ir.geometry.loops[0].boundary = crate::topology::LoopBoundary::Vertex {
        vertex: vertex.clone(),
        pcurves: Vec::new(),
    };
    ir.geometry.shells[0].add_free_vertex(vertex.clone());
    let mut second_shell = ir.geometry.shells[0].clone();
    second_shell.id = "synthetic:test:shell#second"
        .try_into()
        .expect("valid identity");
    second_shell.members = vec![crate::topology::ShellMember::FreeVertex { id: vertex }];
    ir.geometry.regions[0].shells.push(second_shell.id.clone());
    ir.geometry.shells.push(second_shell);

    assert!(validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::WireTopology
                && finding.message == "free vertex must belong to exactly one shell"
        }));
}

#[test]
fn carrierless_edge_range_requires_finite_values_but_not_ordering() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.geometry.edges[0].set_curve(None).unwrap();
    ir.geometry.edges[0].carrier =
        crate::topology::EdgeCarrier::new(ir.geometry.edges[0].curve().cloned(), Some([1.0, 0.0]))
            .unwrap();
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(!report.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain
            && finding.entity.as_deref() == Some(ir.geometry.edges[0].id.as_str())
    }));

    assert!(crate::topology::EdgeCarrier::new(
        ir.geometry.edges[0].curve().cloned(),
        Some([f64::NAN, 0.0])
    )
    .is_err());
}

#[test]
fn vertex_loop_is_valid_and_exclusive_with_coedges() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let face_id = ir.geometry.faces[0].id.clone();
    let vertex_id = ir.geometry.vertices[0].id.clone();
    let loop_id = crate::ids::LoopId::mint("synthetic:cube:vertex-loop#0").expect("valid identity");
    ir.geometry.loops.push(crate::topology::Loop {
        id: loop_id.clone(),
        face: face_id,
        boundary: crate::topology::LoopBoundary::Vertex {
            vertex: vertex_id,
            pcurves: Vec::new(),
        },
        native_ref: None,
    });
    let face_loops = ir.geometry.faces[0]
        .loops
        .iter()
        .cloned()
        .chain(std::iter::once(loop_id.clone()))
        .collect();
    ir.geometry.faces[0].loops = crate::topology::FaceLoops::unspecified(face_loops);
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(report.is_ok(), "{:#?}", report.findings);
}

#[test]
fn spring_support_reference_findings_name_the_construction() {
    use crate::geometry::procedural_curves::ProceduralCurveDefinition;
    use crate::geometry::surface_curves::{SpringLayout, SpringPcurve, SpringSupport};
    use crate::geometry::ProceduralCurve;
    use crate::ids::{ProceduralCurveId, SurfaceId};

    let mut ir = unit_cube().expect("valid unit cube fixture");
    let owner = ProceduralCurveId::mint("test:model:procedural-curve#spring").unwrap();
    let missing = SurfaceId::mint("test:model:surface#missing").unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        owner.clone(),
        ProceduralCurveDefinition::Spring(
            crate::geometry::curve_payloads::SpringCurvePayload::try_new(
                SpringLayout::ContextFirst {
                    supports: [
                        SpringSupport::Surface(missing.clone()),
                        SpringSupport::Ranges([[0.0, 1.0]; 2]),
                    ],
                    first_pcurve: Box::new(SpringPcurve::Range([0.0, 1.0])),
                    second_pcurve: None,
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                    discontinuity_flag: false,
                    cache: None,
                },
                1,
            )
            .unwrap(),
        ),
    ));
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity
            && finding.entity.as_deref() == Some(owner.as_str())
            && finding.message == format!("references missing surface `{missing}`")
    }));
}
