// SPDX-License-Identifier: Apache-2.0

use super::plane_for_face;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure, ResourceLimit,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, Sense};

fn policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

fn face(id: &str, surface: &str) -> Face {
    Face {
        id: FaceId::mint(id).unwrap(),
        shell: ShellId::mint("test:model:shell#1").unwrap(),
        surface: SurfaceId::mint(surface).unwrap(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    }
}

fn surface(id: &str, depth: f64) -> Surface {
    Surface {
        id: SurfaceId::mint(id).unwrap(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, depth),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    }
}

fn refusal(error: &CodecError, operation: &'static str, cap: u64) -> ResourceLimit {
    let CodecError::ResourceLimit(actual) = error else {
        panic!("expected resource refusal: {error:?}");
    };
    assert_eq!(
        *actual,
        ResourceLimit {
            dimension: ResourceDimension::WorkUnits,
            reason: ResourceFailure::BudgetExceeded,
            limit: cap,
            used: cap,
            additional: 1,
            operation,
        }
    );
    *actual
}

#[test]
fn plane_query_first_match_keeps_native_long_suffix_unvisited() {
    let first = face("test:model:face#first", "test:model:surface#first");
    let planes = [
        surface(first.surface.as_str(), 2.0),
        surface(
            &format!("test:model:surface#{}", "unread".repeat(1024)),
            9.0,
        ),
    ];
    let faces = [
        first,
        face(
            &format!("test:model:face#{}", "unread".repeat(1024)),
            "test:model:surface#absent",
        ),
    ];
    let before = (
        serde_json::to_value(&faces).unwrap(),
        serde_json::to_value(&planes).unwrap(),
    );
    // Exactly the first face/surface steps and their equal identity byte pairs.
    let work = 2 + u64::try_from(faces[0].id.as_str().len() + planes[0].id.as_str().len()).unwrap();
    for allowed in [true, false] {
        let cap = work - u64::from(!allowed);
        let arena = DecodeArena::new();
        let policy = policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = plane_for_face(&ctx, &faces[0].id, &faces, &planes);
        if allowed {
            let (index, plane) = result.unwrap().unwrap();
            assert_eq!(index, 0);
            assert_eq!(plane.origin().get(), Point3::new(0.0, 0.0, 2.0));
            assert_eq!(ctx.resource_refusal(), None);
            ctx.finish_session().unwrap();
        } else {
            let original = refusal(
                &result.unwrap_err(),
                "compare F3D plane surface identity",
                cap,
            );

            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        }
        assert_eq!(
            (
                serde_json::to_value(&faces).unwrap(),
                serde_json::to_value(&planes).unwrap()
            ),
            before
        );
    }
}

#[test]
fn plane_query_equal_length_first_mismatch_skips_surface_and_remaining_text() {
    let faces = [face(
        &format!("aaa:model:face#{}", "tail".repeat(1024)),
        "test:model:surface#first",
    )];
    let candidate = FaceId::mint(format!("bbb:model:face#{}", "tail".repeat(1024))).unwrap();
    let planes = [surface(faces[0].surface.as_str(), 2.0)];
    let before = serde_json::to_value(&faces).unwrap();
    // One face step, its first mismatching byte pair, and the face end probe.
    for cap in [3, 2] {
        let arena = DecodeArena::new();
        let policy = policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = plane_for_face(&ctx, &candidate, &faces, &planes);
        if cap == 3 {
            assert_eq!(result.unwrap(), None);
            ctx.finish_session().unwrap();
        } else {
            let original = refusal(&result.unwrap_err(), "visit F3D plane face", cap);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        }
        assert_eq!(serde_json::to_value(&faces).unwrap(), before);
    }
}

#[test]
fn plane_query_unequal_length_identity_has_no_byte_comparison_work() {
    let faces = [face("test:model:face#first", "test:model:surface#first")];
    let candidate = FaceId::mint("test:model:face#other-length").unwrap();
    let planes = [surface(faces[0].surface.as_str(), 2.0)];
    let arena = DecodeArena::new();
    let policy = policy(2);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        plane_for_face(&ctx, &candidate, &faces, &planes).unwrap(),
        None
    );
    ctx.finish_session().unwrap();
}

#[test]
fn plane_query_duplicate_carrier_keeps_first_source_plane() {
    let faces = [face("test:model:face#first", "test:model:surface#same")];
    let planes = [
        surface(faces[0].surface.as_str(), 2.0),
        surface(faces[0].surface.as_str(), 99.0),
    ];
    let before = serde_json::to_value(&planes).unwrap();
    let work = 2 + u64::try_from(faces[0].id.as_str().len() + planes[0].id.as_str().len()).unwrap();
    let arena = DecodeArena::new();
    let policy = policy(work);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (_, plane) = plane_for_face(&ctx, &faces[0].id, &faces, &planes)
        .unwrap()
        .unwrap();
    assert_eq!(plane.origin().get(), Point3::new(0.0, 0.0, 2.0));
    assert_eq!(serde_json::to_value(&planes).unwrap(), before);
    ctx.finish_session().unwrap();
}
