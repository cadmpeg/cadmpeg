// SPDX-License-Identifier: Apache-2.0


fn one_face_shell() -> crate::brep::graph::Brep {
    use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Face, FaceLoops, Sense};

    crate::brep::graph::Brep {
        faces: vec![Face {
            id: FaceId::mint("test:model:face#1").expect("face id"),
            shell: ShellId::mint("test:model:shell#1").expect("shell id"),
            surface: SurfaceId::mint("test:model:surface#1").expect("surface id"),
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        }],
        ..Default::default()
    }
}

fn shell_components(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    brep: &crate::brep::graph::Brep,
    shell: &str,
) -> Result<Vec<Vec<usize>>, cadmpeg_core::CodecError> {
    let mut index =
        crate::brep::graph::shells::ShellFaceIndex::new(ctx, &brep.faces, &brep.loops, &brep.coedges)?;
    index.take_components(shell)?.into_retained()
}

#[test]
fn shell_components_use_linear_stars_within_each_shell() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut brep = one_face_shell();
    let template = brep.faces[0].clone();
    brep.faces = (0..5)
        .map(|i| {
            let mut face = template.clone();
            face.id = crate::brep::graph::id_face(i + 2);
            if i < 2 {
                face.shell =
                    cadmpeg_ir::ids::ShellId::compose(&crate::brep::graph::shell_namespace(), 2_u16);
            }
            face
        })
        .collect();
    for (i, face) in brep.faces.iter().enumerate() {
        let attr = u16::try_from(i + 2).unwrap();
        let loop_id = crate::brep::graph::id_loop(attr);
        brep.loops.push(cadmpeg_ir::topology::Loop {
            id: loop_id.clone(),
            face: face.id.clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Vertex {
                vertex: crate::brep::graph::id_vertex(attr),
                pcurves: Vec::new(),
            },
        });
        brep.coedges.push(cadmpeg_ir::topology::Coedge {
            id: crate::brep::graph::id_coedge(attr),
            owner_loop: loop_id,
            edge: crate::brep::graph::id_edge(1),
            radial_next: crate::brep::graph::id_coedge(attr),
            sense: cadmpeg_ir::topology::Sense::Forward,
            use_curve: None,
            pcurves: Vec::new(),
        });
    }
    let mut index =
        crate::brep::graph::shells::ShellFaceIndex::new(&ctx, &brep.faces, &brep.loops, &brep.coedges).unwrap();
    assert_eq!(
        index
            .neighbors()
            .values()
            .map(std::collections::BTreeSet::len)
            .sum::<usize>(),
        6
    );
    assert_eq!(
        index.take_components("test:model:shell#1").unwrap().into_retained().unwrap(),
        vec![vec![2, 3, 4]]
    );
    assert_eq!(
        index
            .take_components("sldprt:brep:shell#2")
            .unwrap()
            .into_retained().unwrap(),
        vec![vec![0, 1]]
    );
}

#[test]
fn shell_components_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(matches!(
        shell_components(&ctx, &one_face_shell(), "test:model:shell#1"),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

#[test]
fn shell_components_refuse_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(matches!(
        shell_components(&ctx, &one_face_shell(), "test:model:shell#1"),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

