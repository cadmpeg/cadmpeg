// SPDX-License-Identifier: Apache-2.0

use crate::report::{check::Check, Severity};

#[test]
fn radial_ring_crossing_keeps_error_order_and_owner() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let owner = ir.model.coedges[0].id.clone();
    let other = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge != ir.model.coedges[0].edge)
        .unwrap()
        .id
        .clone();
    ir.model.coedges[0].radial_next = other;
    let mut findings = Vec::new();
    super::check_coedge_pairing(&ir, &mut findings).unwrap();
    assert!(findings.len() >= 2);
    for finding in &findings[..2] {
        assert_eq!(finding.check, Check::CoedgePairing);
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.entity.as_deref(), Some(owner.as_str()));
    }
    assert_eq!(findings[0].message, "radial ring crosses edges");
    assert_eq!(findings[1].message, "radial ring does not close");
}

/// Faces with one vertex loop on each named vertex, and shells over the
/// named faces.
fn vertex_loop_model(faces: &[(&str, &[&str])], shells: &[&[&str]]) -> crate::CadIr {
    use crate::ids::{FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId};
    use crate::topology::{Face, FaceLoops, Loop, LoopBoundary, Sense, Shell};
    let mut ir = crate::CadIr::empty();
    let shell_id = |index: usize| ShellId::mint(format!("test:model:shell#{index}")).unwrap();
    for (face, vertices) in faces {
        let face_id = FaceId::mint(format!("test:model:face#{face}")).unwrap();
        let loop_ids: Vec<LoopId> = vertices
            .iter()
            .map(|vertex| LoopId::mint(format!("test:model:loop#{face}-{vertex}")).unwrap())
            .collect();
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: shell_id(0),
            surface: SurfaceId::mint("test:model:surface#plane").unwrap(),
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(loop_ids.clone()),
            name: None,
            color: None,
            tolerance: None,
        });
        for (loop_id, vertex) in loop_ids.into_iter().zip(*vertices) {
            ir.model.loops.push(Loop {
                id: loop_id,
                face: face_id.clone(),
                boundary: LoopBoundary::Vertex {
                    vertex: VertexId::mint(format!("test:model:vertex#{vertex}")).unwrap(),
                    pcurves: Vec::new(),
                },
            });
        }
    }
    for (index, members) in shells.iter().enumerate() {
        let members = members
            .iter()
            .map(|face| FaceId::mint(format!("test:model:face#{face}")).unwrap())
            .collect();
        ir.model.shells.push(
            Shell::new(
                shell_id(index),
                RegionId::mint("test:model:region#0").unwrap(),
                members,
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
        );
    }
    ir
}

fn disconnected_shells(ir: &crate::CadIr) -> Vec<String> {
    let mut findings = Vec::new();
    super::check_shell_connectivity(ir, &mut findings).unwrap();
    findings
        .into_iter()
        .map(|finding| {
            assert_eq!(finding.check, Check::ShellTopology);
            finding.entity.unwrap()
        })
        .collect()
}

#[test]
fn shell_connectivity_joins_only_the_shells_own_faces() {
    // Faces a and b share no vertex; each shares one with c.
    let faces: [(&str, &[&str]); 3] = [("a", &["1"]), ("b", &["2"]), ("c", &["1", "2"])];
    let ir = vertex_loop_model(&faces, &[&["a", "b"], &["a", "b", "c"], &["b", "c"]]);
    assert_eq!(disconnected_shells(&ir), ["test:model:shell#0"]);
}

#[test]
fn shell_connectivity_through_one_vertex_shared_by_many_faces_is_linear() {
    // Every ordered pair of faces on the shared vertex would be 2^32 steps.
    const FACES: usize = 1 << 16;
    let names: Vec<String> = (0..FACES).map(|index| index.to_string()).collect();
    let mut faces: Vec<(&str, &[&str])> = names
        .iter()
        .map(|name| (name.as_str(), &["shared"][..]))
        .collect();
    let members: Vec<&str> = names.iter().map(String::as_str).collect();
    let ir = vertex_loop_model(&faces, &[&members]);
    assert!(disconnected_shells(&ir).is_empty());
    faces.push(("isolated", &["other"]));
    let mut members = members;
    members.push("isolated");
    let ir = vertex_loop_model(&faces, &[&members]);
    assert_eq!(disconnected_shells(&ir), ["test:model:shell#0"]);
}
