//! Connected arc phase expiry and duplicate-key storage boundaries.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId};

const EPS_ARC_MATCH: f64 = 1.0e-9;

fn first_hash_bytes<T>() -> usize {
    4 * std::mem::size_of::<T>() + std::mem::align_of::<T>().max(16) - 1 + 4 + 16
}

fn point_phase_peak(duplicate: bool) -> u64 {
    // A one-byte key and the first String/Point2 hash table survive all phases.
    // The first sketch's ordered node holds an exact one-point group; its
    // second point grows to four tuples while the old tuple remains live.
    let points = 1 + first_hash_bytes::<(String, Point2)>();
    let node = 11 * (std::mem::size_of::<&SketchId>()
        + std::mem::size_of::<Vec<(Option<&str>, Point2)>>())
        + 16 * std::mem::size_of::<usize>() + 2 * std::mem::align_of::<usize>();
    let tuple = std::mem::size_of::<(Option<&str>, Point2)>();
    u64::try_from(points + node + if duplicate { 5 * tuple } else { tuple }).unwrap()
}

fn duplicate_points_with_oriented_arc() -> Vec<SketchEntity> {
    let sketch = SketchId::mint("synthetic:test:id#duplicate-point-sketch").unwrap();
    let point = |id: &str, native: &str, position| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(), sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        ).with_native_ref(Some(native.into()))
    };
    vec![
        point("synthetic:test:id#first-p", "p", Point2::new(-1.0, 0.0)),
        point("synthetic:test:id#last-p", "p", Point2::new(1.0, 0.0)),
        point("synthetic:test:id#q", "q", Point2::new(0.0, 1.0)),
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#oriented-arc").unwrap(), sketch,
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(0.0, 0.0), radius: Length::new(1.0).unwrap(),
                start_angle: Angle::new(0.0).unwrap(),
                end_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            }).unwrap(),
        ).with_endpoint_refs(vec!["p".into(), "q".into()]),
    ]
}

fn accepts_expired_phases(mut entities: Vec<SketchEntity>, duplicate: bool) {
    let expected = entities.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let peak = point_phase_peak(duplicate);
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..2 {
        super::super::resolve_connected_marker_arcs(&ctx, &mut entities, EPS_ARC_MATCH).unwrap();
        assert_eq!(entities, expected);
        assert!(ctx.resource_refusal().is_none());
        let scratch = ctx.reserve_scoped(peak, "reuse connected arc scratch").unwrap();
        drop(scratch);
    }
}

fn refuses_original_point_group(mut entities: Vec<SketchEntity>, duplicate: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let peak = point_phase_peak(duplicate);
    let tuple = u64::try_from(std::mem::size_of::<(Option<&str>, Point2)>()).unwrap();
    policy.limits.max_materialized_bytes = peak - 1;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::resolve_connected_marker_arcs(&ctx, &mut entities, EPS_ARC_MATCH).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("point group must refuse") };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect SLDPRT connected arc point records");
    assert_eq!(limit.limit, peak - 1);
    assert_eq!(limit.used, peak - tuple);
    assert_eq!(limit.additional, tuple);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let replay = ctx.charge_work(0, "probe refused connected arc session").unwrap_err();
    let CodecError::ResourceLimit(replay) = replay else { panic!("sticky refusal") };
    assert_eq!(replay, limit);
}

#[test]
fn connected_arc_phase_expiry_preserves_missing_point_recovery_at_exact_peak() {
    accepts_expired_phases(super::connected_arc_limit_entities(), false);
}

#[test]
fn connected_arc_phase_expiry_preserves_original_missing_point_refusal() {
    refuses_original_point_group(super::connected_arc_limit_entities(), false);
}

#[test]
fn connected_arc_duplicate_point_key_keeps_last_position_without_a_discarded_copy() {
    // The geometry starts at the last p. Using the first p would reverse the
    // final endpoint list. Both point occurrences must keep their identities.
    accepts_expired_phases(duplicate_points_with_oriented_arc(), true);
}

#[test]
fn connected_arc_duplicate_point_group_preserves_original_overlap_refusal() {
    refuses_original_point_group(duplicate_points_with_oriented_arc(), true);
}
