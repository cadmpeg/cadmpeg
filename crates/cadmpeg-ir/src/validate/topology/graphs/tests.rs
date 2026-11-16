// SPDX-License-Identifier: Apache-2.0

use crate::report::{check::Check, Severity};

#[test]
fn radial_ring_crossing_keeps_error_order_and_owner() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let owner = ir.geometry.coedges[0].id.clone();
    let other = ir
        .geometry
        .coedges
        .iter()
        .find(|coedge| coedge.edge != ir.geometry.coedges[0].edge)
        .unwrap()
        .id
        .clone();
    ir.geometry.coedges[0].radial_next = other;
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

/// Faces with one vertex loop on each named vertex, each in the shell its
/// index names.
fn vertex_loop_model(faces: &[(&str, usize, &[&str])], shell_count: usize) -> crate::CadIr {
    use crate::ids::{FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId};
    use crate::topology::{Face, FaceLoops, Loop, LoopBoundary, Sense, Shell};
    let mut ir = crate::CadIr::empty();
    let shell_id = |index: usize| ShellId::mint(format!("test:model:shell#{index}")).unwrap();
    for (face, shell, vertices) in faces {
        let face_id = FaceId::mint(format!("test:model:face#{face}")).unwrap();
        let loop_ids: Vec<LoopId> = vertices
            .iter()
            .map(|vertex| LoopId::mint(format!("test:model:loop#{face}-{vertex}")).unwrap())
            .collect();
        ir.geometry.faces.push(Face {
            id: face_id.clone(),
            shell: shell_id(*shell),
            surface: SurfaceId::mint("test:model:surface#plane").unwrap(),
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(loop_ids.clone()),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
        for (loop_id, vertex) in loop_ids.into_iter().zip(*vertices) {
            ir.geometry.loops.push(Loop {
                id: loop_id,
                face: face_id.clone(),
                boundary: LoopBoundary::Vertex {
                    vertex: VertexId::mint(format!("test:model:vertex#{vertex}")).unwrap(),
                    pcurves: Vec::new(),
                },
                native_ref: None,
            });
        }
    }
    for index in 0..shell_count {
        ir.geometry.shells.push(Shell::new(
            shell_id(index),
            RegionId::mint("test:model:region#0").unwrap(),
        ));
    }
    ir
}

fn disconnected_shells(ir: &crate::CadIr) -> Vec<String> {
    let mut findings = Vec::new();
    super::check_shell_connectivity(ir, &mut findings);
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
    // Faces a and b share no vertex; c, in another shell, shares one with
    // each. Faces d and e are joined by f in their own shell.
    let faces: [(&str, usize, &[&str]); 6] = [
        ("a", 0, &["1"]),
        ("b", 0, &["2"]),
        ("c", 1, &["1", "2"]),
        ("d", 2, &["3"]),
        ("e", 2, &["4"]),
        ("f", 2, &["3", "4"]),
    ];
    let ir = vertex_loop_model(&faces, 3);
    assert_eq!(disconnected_shells(&ir), ["test:model:shell#0"]);
}

#[test]
fn shell_connectivity_through_one_vertex_shared_by_many_faces_is_linear() {
    // Every ordered pair of faces on the shared vertex would be 2^32 steps.
    const FACES: usize = 1 << 16;
    let names: Vec<String> = (0..FACES).map(|index| index.to_string()).collect();
    let mut faces: Vec<(&str, usize, &[&str])> = names
        .iter()
        .map(|name| (name.as_str(), 0, &["shared"][..]))
        .collect();
    let ir = vertex_loop_model(&faces, 1);
    assert!(disconnected_shells(&ir).is_empty());
    faces.push(("isolated", 0, &["other"]));
    let ir = vertex_loop_model(&faces, 1);
    assert_eq!(disconnected_shells(&ir), ["test:model:shell#0"]);
}
