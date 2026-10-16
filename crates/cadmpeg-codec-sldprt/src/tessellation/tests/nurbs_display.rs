// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::geometry::{PlacedSurface, SolvedSurfaceGeometry, SurfaceGeometry};

fn nurbs_display_model(placed: bool) -> Model {
    let mut model = super::model_with_body();
    let surface = super::test_nurbs_surface();
    let corners = super::test_nurbs_corners(&surface);
    let vertices = [(0.15, 0.2), (0.8, 0.2), (0.5, 0.8)]
        .map(|(u, v)| {
            cadmpeg_ir::eval::decode::nurbs_surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &surface,
                u,
                v,
            )
            .unwrap()
            .get()
        })
        .to_vec();
    let geometry = SolvedSurfaceGeometry::Nurbs(surface);
    let geometry = if placed {
        SolvedSurfaceGeometry::Transformed(
            PlacedSurface::try_new(
                Box::new(geometry),
                cadmpeg_ir::transform::Transform::identity(),
            )
            .unwrap(),
        )
    } else {
        geometry
    };
    let face = super::add_face(
        &mut model,
        "admitted-nurbs",
        SurfaceGeometry::Solved(geometry),
        corners,
    );
    super::set_shell_faces(&mut model, vec![face]);
    model.tessellations.push(super::mesh_from(
        "synthetic:test:tessellation#admitted-nurbs",
        vertices,
        vec![[0, 1, 2]],
    ));
    model
}

fn assign(model: &mut Model, policy: &DecodePolicy) -> Result<Vec<String>, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).unwrap();
    let result = super::super::assign_unique_surface_owners(&ctx, model, &mut Vec::new());
    if let Err(CodecError::ResourceLimit(first)) = &result {
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(sticky)) if sticky == *first));
    }
    result
}

#[test]
fn geometric_nurbs_surface_route_assigns_the_face() {
    let mut model = nurbs_display_model(false);
    assert_eq!(
        assign(&mut model, &DecodePolicy::service()).unwrap(),
        vec!["synthetic:test:tessellation#admitted-nurbs"]
    );
    assert_eq!(
        model.tessellations[0].faces,
        vec![model.faces[0].id.clone()]
    );
}

#[test]
fn geometric_placed_surface_route_refuses_nesting_limit() {
    let mut model = nurbs_display_model(true);
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    assert!(
        matches!(assign(&mut model.clone(), &policy), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RecursionDepth)
    );
    assert_eq!(
        assign(&mut model, &DecodePolicy::service()).unwrap(),
        vec!["synthetic:test:tessellation#admitted-nurbs"]
    );
}
