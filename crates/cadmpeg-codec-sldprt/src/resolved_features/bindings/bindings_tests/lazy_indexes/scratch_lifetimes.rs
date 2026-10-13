//! Consecutive owner indexes contribute their maximum live workspace, not their sum.

use super::*;
use std::cell::Cell;

fn peak_materialization(lane: &FeatureInputLane) -> u64 {
    const PROBE: &str = "measure SLDPRT binding workspace peak";
    let peak = Cell::new(0);
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::MaterializedBytes, PROBE, |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let mut lane = lane.clone();
        let mut history = crate::resolved_features::selections::SelectionHistory::new(&ctx, &[])?;
        super::super::super::finalize_lane_bindings(&ctx, &[], &mut history, &mut lane)?;
        let error = ctx.reserve_scoped(u64::MAX, PROBE).unwrap_err();
        if cap == u64::MAX {
            let CodecError::ResourceLimit(limit) = &error else { panic!("workspace probe refusal"); };
            assert_eq!(limit.used, 0);
            peak.set(limit.limit);
        }
        Err::<(), _>(error)
    });
    peak.get()
}

#[test]
fn scalar_owner_indexes_release_the_previous_phase_workspace() {
    let (_, mut combined, _) = inputs("moCirPattern_c");
    combined.names.clear();
    combined.native_payload.clear();
    combined.sketch_entities = (0..32).map(|ordinal| {
        let mut point = SketchInputEntity::new(
            format!("point-{ordinal}"), "lane", ordinal, 0, SketchInputKind::Point,
        );
        point.feature_ref = Some("owner".into());
        point
    }).collect();
    combined.scalars = (0..512).map(|ordinal| FeatureInputScalar {
        id: format!("scalar-{ordinal}"), parent: "lane".into(),
        feature_ref: Some("owner".into()), ordinal, offset: u64::from(ordinal),
        object_id: ordinal, name: "absent".into(),
        value: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        role: FeatureInputScalarRole::Native, operands: Vec::new(),
    }).collect();
    let mut entity_phase = combined.clone();
    entity_phase.scalars.clear();
    let mut scalar_phase = combined.clone();
    scalar_phase.sketch_entities.clear();
    let entity_peak = peak_materialization(&entity_phase);
    let scalar_peak = peak_materialization(&scalar_phase);
    assert!(entity_peak > 0);
    assert!(scalar_peak > entity_peak);
    assert_eq!(peak_materialization(&combined), scalar_peak);
}
