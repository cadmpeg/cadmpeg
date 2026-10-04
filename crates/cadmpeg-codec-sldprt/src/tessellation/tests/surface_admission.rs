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
    let result = super::super::assign_unique_surface_owners(&ctx, model);
    if let Err(CodecError::ResourceLimit(first)) = &result {
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(sticky)) if sticky == *first));
    }
    result
}

#[test]
fn geometric_nurbs_surface_route_refuses_scoped_limit() {
    let model = nurbs_display_model(false);
    for original_cap in [0, 3 * (3 + 3) * 8 - 1, 3 * (3 + 3) * 8] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = original_cap;
        let mut preceding = None;
        let mut completed = false;
        for _ in 0..128 {
            let mut admitted = model.clone();
            match assign(&mut admitted, &policy) {
                Err(CodecError::ResourceLimit(first)) => {
                    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                    let need = first.used.checked_add(first.additional).unwrap();
                    assert!(need > policy.limits.max_materialized_bytes);
                    preceding = Some(need);
                    policy.limits.max_materialized_bytes = need;
                }
                Ok(assigned) => {
                    assert_eq!(assigned, vec!["synthetic:test:tessellation#admitted-nurbs"]);
                    assert_eq!(
                        admitted.tessellations[0].faces,
                        vec![admitted.faces[0].id.clone()]
                    );
                    if let Some(need) = preceding {
                        policy.limits.max_materialized_bytes = need - 1;
                        assert!(matches!(assign(&mut model.clone(), &policy),
                            Err(CodecError::ResourceLimit(first))
                                if first.dimension == ResourceDimension::MaterializedBytes));
                    }
                    completed = true;
                    break;
                }
                Err(error) => panic!("unexpected scoped admission: {error}"),
            }
        }
        assert!(
            completed,
            "the complete scratch requirement must be admitted"
        );
    }
}

#[test]
fn geometric_nurbs_surface_route_refuses_work_limit() {
    let model = nurbs_display_model(false);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    for _ in 0..4096 {
        let error = assign(&mut model.clone(), &policy).unwrap_err();
        let CodecError::ResourceLimit(limit) = error else {
            panic!("unexpected surface route error");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        if limit.operation == "IR B-spline basis work" {
            policy.limits.max_work_units = limit.used + limit.additional - 1;
            assert!(
                matches!(assign(&mut model.clone(), &policy), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::WorkUnits && refusal.operation == limit.operation)
            );
            let mut admitted = model;
            assert_eq!(
                assign(&mut admitted, &DecodePolicy::service()).unwrap(),
                vec!["synthetic:test:tessellation#admitted-nurbs"]
            );
            return;
        }
        let next = limit.used + limit.additional;
        assert!(next > policy.limits.max_work_units);
        policy.limits.max_work_units = next;
    }
    panic!("surface projection work admission was not reached");
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
