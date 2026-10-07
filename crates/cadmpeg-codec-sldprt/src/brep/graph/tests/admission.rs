// SPDX-License-Identifier: Apache-2.0

const TINY_PARAMETER_DOMAIN: f64 = 1e-12;

#[test]
fn inverse_coordinate_scale_admits_each_point() {
    let points = [
        cadmpeg_ir::math::Point3::new(2.0, 3.0, 4.0),
        cadmpeg_ir::math::Point3::new(5.0, 6.0, 7.0),
    ];
    crate::test_support::work_refusal_at("measure Parasolid inverse coordinate scale", |ctx| {
        super::super::inverse_coordinate_tolerance(ctx, points)
    });
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::super::inverse_coordinate_tolerance(&ctx, []).unwrap(),
        super::super::INVERSE_ABSOLUTE_TOLERANCE_MM
    );
    assert_eq!(
        super::super::inverse_coordinate_tolerance(&ctx, points).unwrap(),
        super::super::looser_tolerance(
            super::super::INVERSE_ABSOLUTE_TOLERANCE_MM,
            7.0 * super::super::INVERSE_RELATIVE_TOLERANCE
        )
    );
}

#[test]
fn incomplete_brep_marker_does_not_consume_collection_slots() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let body = [0x00, 0x1e, 0x00];
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let carriers = super::super::scan_carriers(&ctx, &body).unwrap();
    assert!(carriers.curve_attrs(&ctx).unwrap().is_empty());
    let tables = super::super::topology::scan(&ctx, &body).unwrap();
    assert!(tables.points().is_empty());
    assert!(tables.loops().is_empty());
    ctx.finish_session().unwrap();
    // Graph construction owns one annotation-stream handle even without records.
    policy.limits.max_collection_items = 1;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let decoded =
        super::super::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("incomplete-marker"))
            .unwrap();
    assert!(decoded.points.is_empty());
    assert!(decoded.loops.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn empty_brep_scan_preserves_work_refusal_without_candidate_census() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let body = [0xff; 4096];
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let stream = cadmpeg_ir::stream_name!("no-candidates");
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        super::super::decode_body(&ctx, &body, &stream)
    });
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
        panic!("work refusal");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.operation, "scan SLDPRT analytic carriers");
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert_eq!(allocations, 0);
}

fn one_face_shell() -> super::super::Brep {
    use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Face, FaceLoops, Sense};

    super::super::Brep {
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
    brep: &super::super::Brep,
    shell: &str,
) -> Result<Vec<Vec<usize>>, cadmpeg_core::CodecError> {
    let mut index =
        super::super::ShellFaceIndex::new(ctx, &brep.faces, &brep.loops, &brep.coedges)?;
    Ok(index.take_components(ctx, shell)?.0)
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
            face.id = super::super::id_face(i + 2);
            if i < 2 {
                face.shell =
                    super::super::ShellId::compose(&super::super::shell_namespace(), 2_u16);
            }
            face
        })
        .collect();
    for (i, face) in brep.faces.iter().enumerate() {
        let attr = u16::try_from(i + 2).unwrap();
        let loop_id = super::super::id_loop(attr);
        brep.loops.push(super::super::Loop {
            id: loop_id.clone(),
            face: face.id.clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Vertex {
                vertex: super::super::id_vertex(attr),
                pcurves: Vec::new(),
            },
        });
        brep.coedges.push(super::super::Coedge {
            id: super::super::id_coedge(attr),
            owner_loop: loop_id,
            edge: super::super::id_edge(1),
            radial_next: super::super::id_coedge(attr),
            sense: super::super::Sense::Forward,
            use_curve: None,
            pcurves: Vec::new(),
        });
    }
    let mut index =
        super::super::ShellFaceIndex::new(&ctx, &brep.faces, &brep.loops, &brep.coedges).unwrap();
    assert_eq!(
        index
            .neighbors
            .values()
            .map(std::collections::BTreeSet::len)
            .sum::<usize>(),
        6
    );
    assert_eq!(
        index.take_components(&ctx, "test:model:shell#1").unwrap().0,
        vec![vec![2, 3, 4]]
    );
    assert_eq!(
        index
            .take_components(&ctx, "sldprt:brep:shell#2")
            .unwrap()
            .0,
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

#[test]
fn numerical_followup_inverse_ambiguity_is_independent_of_parameter_units() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    for domain in [1e-200, TINY_PARAMETER_DOMAIN, 1.0, 1e200] {
        assert!(matches!(
            super::super::unique_inverse_parameter(
                &ctx,
                vec![(0.25 * domain, 0.), (0.75 * domain, 0.)],
                0.001,
                [0., domain]
            )
            .unwrap(),
            super::super::InverseResolution::Ambiguous
        ));
        assert!(matches!(
            super::super::unique_inverse_parameter(
                &ctx,
                vec![(0.25 * domain, 0.), (0.25 * domain, 0.)],
                0.001,
                [0., domain]
            )
            .unwrap(),
            super::super::InverseResolution::Unique(_)
        ));
    }
}
