// SPDX-License-Identifier: Apache-2.0
use super::super::{revolution_boundary_pcurve, revolution_face_sense, revolved_brep_surface};
use crate::decode::sweep::profiles::ProfileEntity;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::RevolutionAxis;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};

fn axis() -> RevolutionAxis {
    RevolutionAxis {
        origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite origin"),
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
            .expect("nonzero direction"),
        reference: None,
    }
}

fn transform() -> crate::placement::FeatureSectionTransform {
    crate::placement::FeatureSectionTransform::new(
        7, Some(3), [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0,
    ).expect("orthonormal section frame")
}

fn assert_free_refusal(mut query: impl FnMut(&DecodeContext<'_>) -> Result<Option<()>, CodecError>) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(query(&ctx).expect("fixed rejection"), None);
    let original = ctx.charge_work_limit(1, "seed revolution pcurve refusal").expect_err("zero cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(query(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn unknown_revolution_boundary_is_free_and_preserves_original_refusal() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let axis = axis();
    assert_free_refusal(|ctx| {
        revolution_boundary_pcurve(
            ctx, &surface, [0.0; 3], &axis, &"unsupported boundary",
            &mut crate::lane_refusal::LaneRefusals::new(),
        ).map(|value| value.map(|_| ()))
    });
}

#[test]
fn unsupported_revolved_sketch_is_free_and_preserves_original_refusal() {
    let geometry = SketchGeometry::native(
        cadmpeg_core::text::NonBlankString::try_from("unsupported revolution sketch")
            .expect("nonblank native kind"),
    );
    let axis = axis();
    let transform = transform();
    assert_free_refusal(|ctx| {
        revolved_brep_surface(
            ctx, &transform, &geometry, false, &axis, &"unsupported sketch",
            &mut crate::lane_refusal::LaneRefusals::new(),
        ).map(|value| value.map(|_| ()))
    });
}

#[test]
fn zero_revolution_tangent_is_free_and_preserves_original_refusal() {
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(0.0, 0.0),
    }).expect("finite line endpoints");
    let segment = crate::decode::with_test_decode_ctx(|ctx| ProfileEntity::new(ctx, geometry, false))
        .expect("fixed profile admission").expect("finite profile endpoints");
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let axis = axis();
    let transform = transform();
    assert_free_refusal(|ctx| {
        revolution_face_sense(
            ctx, &transform, &segment, &surface, &axis, 1.0,
            (&"zero profile tangent", &mut crate::lane_refusal::LaneRefusals::new()),
        ).map(|value| value.map(|_| ()))
    });
}
