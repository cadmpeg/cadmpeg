// SPDX-License-Identifier: Apache-2.0

use crate::brep::graph::test_support::with_test_context;

#[test]
fn shared_edge_coedge_parity_orients_connected_faces() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Coedge, Face, Loop, Sense};

    let face = |id: &str, lp: &str| Face {
        id: FaceId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: SurfaceId::mint(format!("test:model:entity#surface-{id}"))
            .expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![LoopId::mint(format!(
            "test:model:entity#{lp}"
        ))
        .expect("identity grammar")]),
        name: None,
        color: None,
        tolerance: None,
    };
    let lp = |id: &str, face: &str, coedge: &str| Loop {
        id: LoopId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
        face: FaceId::mint(format!("test:model:entity#{face}")).expect("identity grammar"),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![CoedgeId::mint(format!("test:model:entity#{coedge}"))
                    .expect("identity grammar")],
                Vec::new(),
            )
            .expect("fixture ring admission")
            .expect("valid loop ring"),
        ),
    };
    let coedge = |id: &str, lp: &str, radial: &str, sense| Coedge {
        id: CoedgeId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
        owner_loop: LoopId::mint(format!("test:model:entity#{lp}")).expect("identity grammar"),
        edge: EdgeId::mint("test:model:entity#edge").expect("identity grammar"),
        radial_next: CoedgeId::mint(format!("test:model:entity#{radial}"))
            .expect("identity grammar"),
        sense,
        use_curve: None,
        pcurves: Vec::new(),
    };
    let mut brep = crate::brep::graph::Brep {
        faces: vec![face("face-a", "loop-a"), face("face-b", "loop-b")],
        loops: vec![
            lp("loop-a", "face-a", "coedge-a"),
            lp("loop-b", "face-b", "coedge-b"),
        ],
        coedges: vec![
            coedge("coedge-a", "loop-a", "coedge-b", Sense::Forward),
            coedge("coedge-b", "loop-b", "coedge-a", Sense::Forward),
        ],
        ..Default::default()
    };

    with_test_context(|ctx| crate::brep::graph::orientation::solve_face_orientation(ctx, &mut brep))
        .expect("orient first face pair");
    assert_eq!(brep.faces[0].sense, Sense::Forward);
    assert_eq!(brep.faces[1].sense, Sense::Reversed);

    brep.faces[1].sense = Sense::Reversed;
    brep.coedges[1].sense = Sense::Reversed;
    with_test_context(|ctx| crate::brep::graph::orientation::solve_face_orientation(ctx, &mut brep))
        .expect("orient reversed face pair");
    assert_eq!(brep.faces[0].sense, Sense::Forward);
    assert_eq!(brep.faces[1].sense, Sense::Forward);
}

