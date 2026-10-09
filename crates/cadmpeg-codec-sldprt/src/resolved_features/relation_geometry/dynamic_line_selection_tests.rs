use super::unique_dynamic_line_pair;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId};

const QUANTUM: f64 = 1.0e-8;
const OPERATION: &str = "collect SLDPRT dynamic line candidates";

fn fixture() -> (SketchId, [SketchEntity; 2], Vec<SketchEntity>) {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let line = |id: &str, start: Point2, end: Point2| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
        .with_construction(true)
        .with_endpoint_refs(vec!["first".into(), "second".into()])
    };
    let generated = [
        line("synthetic:test:id#horizontal", Point2::new(-13.0, 3.0), Point2::new(0.0, 3.0)),
        line("synthetic:test:id#vertical", Point2::new(0.0, 0.0), Point2::new(0.0, 13.0)),
    ];
    let existing = vec![line("synthetic:test:id#profile", Point2::new(-16.0, 3.0), Point2::new(-16.0, 7.0))];
    (sketch, generated, existing)
}

#[test]
fn dynamic_line_selection_returns_source_positions_without_retained_copies() {
    let (sketch, generated, mut existing) = fixture();
    let refs = [&generated[0].geometry, &generated[1].geometry];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // Each tagged coordinate has a discriminant and 64-bit payload; a row is 72 bytes.
    // Three unique rows fit core's first amortized allocation of four rows.
    policy.limits.max_materialized_bytes = 288;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        assert_eq!(unique_dynamic_line_pair(&ctx, 16.0, &sketch, &existing, &refs, QUANTUM).unwrap(), Some([2, 1]));
    }
    let selected = unique_dynamic_line_pair(&ctx, 16.0, &sketch, &existing, &refs, QUANTUM).unwrap().unwrap();
    let original = existing[0].id().clone();
    // Reallocation keeps the position of the selected existing entity.
    for _ in 0..64 {
        existing.push(generated[0].clone());
    }
    assert_eq!(existing[selected[0] - refs.len()].id(), &original);
    assert_eq!(generated[selected[1]].endpoint_refs, ["first", "second"]);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn dynamic_line_selection_preserves_original_candidate_storage_refusal() {
    let (sketch, generated, existing) = fixture();
    let refs = [&generated[0].geometry, &generated[1].geometry];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 287;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = unique_dynamic_line_pair(&ctx, 16.0, &sketch, &existing, &refs, QUANTUM).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("expected candidate storage refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 288);
    assert_eq!(limit.limit, 287);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit));
}

#[test]
fn ambiguous_dynamic_line_selection_releases_only_its_scratch() {
    let (sketch, generated, mut existing) = fixture();
    existing.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#ambiguous").unwrap(), sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(16.0, 3.0), end: Point2::new(16.0, 7.0),
        }).unwrap(),
    ));
    let refs = [&generated[0].geometry, &generated[1].geometry];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 288;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        assert_eq!(unique_dynamic_line_pair(&ctx, 16.0, &sketch, &existing, &refs, QUANTUM).unwrap(), None);
    }
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

fn generated_geometry() -> [SketchGeometry; 2] {
    [0.0, 16.0].map(|u| {
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(u, 0.0),
            end: Point2::new(u, 10.0),
        }).unwrap()
    })
}

#[test]
fn generated_line_geometry_needs_no_entity_identity_or_retained_storage() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // Two unique 72-byte rows use the first four-row amortized backing.
    policy.limits.max_materialized_bytes = 288;
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let generated = generated_geometry();
    assert_eq!(unique_dynamic_line_pair(
        &ctx, 16.0, &sketch, &[], &[&generated[0], &generated[1]], QUANTUM,
    ).unwrap(), Some([0, 1]));
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn generated_geometry_preserves_original_sized_visit_refusal() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let generated = generated_geometry();
    let CodecError::ResourceLimit(limit) = unique_dynamic_line_pair(
        &ctx, 16.0, &sketch, &[], &[&generated[0], &generated[1]], QUANTUM,
    ).unwrap_err() else { panic!("expected first generated visit refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "scan SLDPRT dynamic line candidates");
    assert_eq!(limit.used, 0);
    // The slice source admits its exact two-element extent before traversal.
    assert_eq!(limit.additional, u64::try_from(generated.len()).unwrap());
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit));
}
