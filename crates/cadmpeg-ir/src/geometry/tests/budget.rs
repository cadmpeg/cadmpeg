// SPDX-License-Identifier: Apache-2.0
//! Budget boundaries shared by geometry-owner tests.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

pub(in crate::geometry) fn with_limit<T>(dimension: ResourceDimension, cap: u64,
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>) -> Result<T, CodecError> {
    with_policy(dimension, cap, DecodePolicy::service(), run)
}

pub(in crate::geometry) fn with_policy<T>(dimension: ResourceDimension, cap: u64,
    mut policy: DecodePolicy,
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    match dimension {
        ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
        ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = cap,
        _ => panic!("geometry test dimension"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let result = run(&ctx);
    match &result {
        Err(CodecError::ResourceLimit(original)) => {
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *original));
        }
        _ => ctx.finish_session().expect("no resource refusal"),
    }
    result
}



#[test]
fn analytic_operations_are_free_and_preserve_sticky_refusal() {
    use crate::geometry::analytic::{LineCurve, PlaneSurface};
    use crate::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
    use crate::math::{Point3, Vector3};
    use crate::scalar::PositiveReal;
    let line = SolvedCurveGeometry::Line(LineCurve::try_new(
        Point3::new(1., 2., 3.), Vector3::new(1., 0., 0.)).unwrap());
    let plane = SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(
        Point3::new(1., 2., 3.), Vector3::new(0., 0., 1.), Vector3::new(1., 0., 0.)).unwrap());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    with_policy(ResourceDimension::WorkUnits, 0, policy, |ctx| {
        assert_eq!(line.try_clone_for_decode(ctx, "free line copy")?, line);
        assert_eq!(plane.try_clone_for_decode(ctx, "free plane copy")?, plane);
        let scale = PositiveReal::new(2.).unwrap();
        assert_eq!(line.clone().scaled_owned(ctx, scale)?.unwrap(),
            SolvedCurveGeometry::Line(LineCurve::try_new(Point3::new(2., 4., 6.), Vector3::new(1., 0., 0.)).unwrap()));
        assert_eq!(plane.clone().scaled_owned(ctx, scale)?.unwrap(),
            SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(Point3::new(2., 4., 6.),
                Vector3::new(0., 0., 1.), Vector3::new(1., 0., 0.)).unwrap()));
        let original = ctx.charge_work_limit(1, "first refusal").expect_err("zero budget");
        for result in [
            line.try_clone_for_decode(ctx, "later line copy").map(|_| ()),
            plane.try_clone_for_decode(ctx, "later plane copy").map(|_| ()),
            line.scaled_owned(ctx, scale).map(|_| ()),
            plane.scaled_owned(ctx, scale).map(|_| ()),
        ] {
            assert!(matches!(result, Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        }
        Err::<(), _>(original.into())
    }).expect_err("original refusal stays sticky");
}
