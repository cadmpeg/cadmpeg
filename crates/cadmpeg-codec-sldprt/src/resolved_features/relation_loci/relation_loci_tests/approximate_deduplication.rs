//! Coincident canonical profile point selection and refusal.

use super::profile;
use crate::resolved_features::transforms::SketchLocusRole;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
};

#[test]
fn canonical_profile_point_deduplication_preserves_identity_order_and_refusal() {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{
        SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
        SketchLocus,
    };
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let input = ["z", "a"].map(|suffix| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{suffix}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        )
    });
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::canonical_profile_loci(ctx, &sketch, &profile(&input))
    };
    assert_eq!(
        solve(&cadmpeg_test_support::service_decode_context()).unwrap(),
        vec![(
            Point2::new(0.0, 0.0),
            SketchLocus::Entity(input[1].id().clone())
        )]
    );
    crate::test_support::work_refusal_at("deduplicate SLDPRT canonical profile loci", solve);
}

const OPERATION: &str = "collect SLDPRT canonical profile loci";
const QUANTUM: f64 = 1.0e-8;

fn coincident_fixture() -> (SketchId, [SketchEntity; 2]) {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let input = ["z-discarded-identity", "a"].map(|suffix| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{suffix}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            }).unwrap(),
        )
    });
    (sketch, input)
}

fn first_backing<T>() -> u64 {
    let size = std::mem::size_of::<T>();
    let minimum = match size {
        1 => 8,
        2..=1024 => 4,
        _ => 1,
    };
    u64::try_from(minimum * size).unwrap()
}

#[test]
fn canonical_loci_copy_only_surviving_identity() {
    let (sketch, input) = coincident_fixture();
    let entities = profile(&input);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let identity_bytes = u64::try_from(input[1].id().as_str().len()).unwrap();
    let output_bytes = first_backing::<(Point2, SketchLocus)>();
    policy.limits.max_retained_bytes = identity_bytes + output_bytes;
    policy.limits.max_materialized_bytes = first_backing::<(usize, Point2, &SketchEntityId, SketchLocusRole)>();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::canonical_profile_loci(&ctx, &sketch, &entities).unwrap();
    assert_eq!(result, [(Point2::new(0.0, 0.0), SketchLocus::Entity(input[1].id().clone()))]);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn canonical_loci_preserve_original_output_reservation_refusal() {
    let (sketch, input) = coincident_fixture();
    let entities = profile(&input);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let identity_bytes = u64::try_from(input[1].id().as_str().len()).unwrap();
    let output_bytes = first_backing::<(Point2, SketchLocus)>();
    policy.limits.max_retained_bytes = identity_bytes + output_bytes - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = super::super::canonical_profile_loci(&ctx, &sketch, &entities).unwrap_err() else {
        panic!("expected retained output refusal");
    };
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.used, identity_bytes);
    assert_eq!(limit.additional, output_bytes);
    assert_eq!(limit.limit, identity_bytes + output_bytes - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit));
}

#[test]
fn canonical_loci_release_borrowed_workspace_and_scoped_output_each_time() {
    let (sketch, input) = coincident_fixture();
    let entities = profile(&input);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let identity_bytes = u64::try_from(input[1].id().as_str().len()).unwrap();
    let output_bytes = first_backing::<(Point2, SketchLocus)>();
    let index_bytes = first_backing::<(usize, Point2, &SketchEntityId, SketchLocusRole)>();
    policy.limits.max_retained_bytes = 0;
    // The consuming index iterator still owns its backing when the output is built.
    policy.limits.max_materialized_bytes = index_bytes + identity_bytes + output_bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let output_storage;
        let output;
        (output, output_storage) = ctx.with_scoped_storage(OPERATION, || {
            super::super::canonical_profile_loci(&ctx, &sketch, &entities)
        }).unwrap();
        assert_eq!(output, [(Point2::new(0.0, 0.0), SketchLocus::Entity(input[1].id().clone()))]);
        drop(output);
        drop(output_storage);
    }
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn canonical_loci_keep_role_order_for_coincident_endpoints() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#line").unwrap(), sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0), end: Point2::new(0.0, 0.0),
        }).unwrap(),
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let input = [entity];
    assert_eq!(super::super::canonical_profile_loci(&ctx, &sketch, &profile(&input)).unwrap(),
        [(Point2::new(0.0, 0.0), SketchLocus::Start(input[0].id().clone()))]);
}

#[test]
fn canonical_loci_keep_source_order_after_equal_position_and_identity_keys() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let input = [0.2, 0.3].map(|fraction| SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#same").unwrap(), sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(fraction * QUANTUM, 0.0),
        }).unwrap(),
    ));
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(super::super::canonical_profile_loci(&ctx, &sketch, &profile(&input)).unwrap(),
        [(Point2::new(0.2 * QUANTUM, 0.0), SketchLocus::Entity(input[0].id().clone()))]);
}
