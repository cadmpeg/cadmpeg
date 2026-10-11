//! Revolution indexes initialize at their consuming profile lookup.

use super::axes_tests::{
    revolution_binding_consumer_targets, single_revolution_history, unresolved_revolution_target,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, PartialRevolveConstruction, PlanarProfileRef,
    RevolveConstruction,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement, SketchProfiles};

const MODEL_INDEX: &str = "index SLDPRT revolution model features";
const SKETCH_INDEX: &str = "index SLDPRT revolution sketches";
const OWNER_INDEX: &str = "index SLDPRT revolution sketch owners";

fn sketch() -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:id#profile").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
        native_ref: None,
    }
}

fn select_feature_profile(targets: &mut [cadmpeg_ir::features::Feature; 2]) {
    let profile_id = targets[1].id.clone();
    targets[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction:
                RevolveConstruction::Unresolved(PartialRevolveConstruction::Axis { profile, .. }),
            ..
        }) = definition
        else {
            panic!("unresolved revolution fixture");
        };
        *profile = PlanarProfileRef::Feature(profile_id);
    });
}

#[test]
fn revolution_missing_sketch_owner_needs_no_history_or_sketch_index_storage() {
    let histories = single_revolution_history();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::bind_profile_revolution_axes(
        &ctx,
        &mut [unresolved_revolution_target()],
        &histories,
        &[],
        &[sketch()],
        &[],
    )
    .unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn revolution_profile_route_skips_unread_indexes() {
    for (feature_profile, operation) in [
        (false, MODEL_INDEX),
        (false, SKETCH_INDEX),
        (true, OWNER_INDEX),
    ] {
        let histories = single_revolution_history();
        let mut targets = revolution_binding_consumer_targets(&histories[0].features[0].id);
        if feature_profile {
            select_feature_profile(&mut targets);
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        super::bind_profile_revolution_axes(&ctx, &mut targets, &histories, &[], &[sketch()], &[])
            .unwrap();
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, operation).unwrap_err() else {
            panic!("unused index left its probe armed");
        };
        assert_eq!(limit.operation, operation);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        drop(probe);
    }
}

#[test]
fn revolution_each_index_reader_reaches_its_own_work_boundary() {
    for (feature_profile, operation) in [
        (true, "look up SLDPRT revolution model feature"),
        (false, "look up SLDPRT revolution sketch"),
        (false, "look up SLDPRT revolution sketch owner"),
    ] {
        let mut histories = single_revolution_history();
        histories[0].features[0].input_class = Some("moProfileFeature_c".into());
        let sketches = [sketch()];
        crate::test_support::work_refusal_at(operation, |ctx| {
            let mut targets = revolution_binding_consumer_targets(&histories[0].features[0].id);
            if feature_profile {
                select_feature_profile(&mut targets);
            }
            super::bind_profile_revolution_axes(ctx, &mut targets, &histories, &[], &sketches, &[])
        });
    }
}

#[test]
fn revolution_index_entries_admit_collection_slots() {
    for (feature_profile, operation) in [
        (true, "insert SLDPRT revolution model feature"),
        (false, "insert SLDPRT revolution native feature"),
        (false, "insert SLDPRT revolution sketch"),
        (false, OWNER_INDEX),
    ] {
        let mut histories = single_revolution_history();
        histories[0].features[0].input_class = Some("moProfileFeature_c".into());
        let sketches = [sketch()];
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::CollectionItems, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let mut targets = revolution_binding_consumer_targets(&histories[0].features[0].id);
            if feature_profile { select_feature_profile(&mut targets); }
            super::bind_profile_revolution_axes(&ctx, &mut targets, &histories, &[], &sketches, &[])
        });
    }
}
