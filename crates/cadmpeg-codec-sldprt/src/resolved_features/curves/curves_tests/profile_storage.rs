//! Closed profile publication storage and original refusal boundaries.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::SketchEntityUse;

fn retained_profile_bytes() -> u64 {
    // Each initially empty output vector reserves four slots. The two
    // identities are copied once into the surviving two-use profile.
    let identities = super::closed_profile_limit_entities();
    let bytes = 4 * std::mem::size_of::<Vec<SketchEntityUse>>()
        + 4 * std::mem::size_of::<SketchEntityUse>()
        + identities[0].id().as_str().len()
        + identities[1].id().as_str().len();
    u64::try_from(bytes).expect("fixture storage fits u64")
}

fn profile_scratch_bytes() -> u64 {
    // Closed-curve indexes: four curve/frontier slots, one four-bucket
    // incidence table, two four-slot incidence groups, and exact usage arrays.
    let incidence = 4 * std::mem::size_of::<(&str, Vec<usize>)>()
        + std::mem::align_of::<(&str, Vec<usize>)>().max(16) - 1 + 4 + 16;
    let indexes = 4 * std::mem::size_of::<usize>()
        + incidence
        + 2 * 4 * std::mem::size_of::<usize>()
        + 2 * std::mem::size_of::<bool>()
        + 2 * std::mem::size_of::<usize>()
        + 4 * std::mem::size_of::<usize>();
    let output_shell = 4 * std::mem::size_of::<Vec<SketchEntityUse>>();
    u64::try_from(indexes).unwrap() + retained_profile_bytes()
        - u64::try_from(output_shell).unwrap()
}

#[test]
fn closed_profile_publication_retains_only_the_intact_profile() {
    let entities = super::closed_profile_limit_entities();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let retained = retained_profile_bytes();
    policy.limits.max_retained_bytes = retained;
    policy.limits.max_materialized_bytes = profile_scratch_bytes();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    let profiles = super::super::closed_marker_profiles(&ctx, &entities).unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 2);
    assert_eq!(profiles[0][0].entity, *entities[0].id());
    assert_eq!(profiles[0][1].entity, *entities[1].id());
    assert!(!profiles[0][0].reversed);
    assert!(!profiles[0][1].reversed);
    assert!(ctx.resource_refusal().is_none());

    // The full scratch allowance is available while the returned profile
    // remains live. Its vector slots and identity bytes stay retained.
    let scratch = ctx
        .reserve_scoped(policy.limits.max_materialized_bytes, "reuse closed profile scratch")
        .unwrap();
    drop(scratch);
    let error = ctx
        .copy_retained_text("x", "probe retained closed profile")
        .unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("retained boundary must refuse")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.used, retained);
    assert_eq!(limit.additional, 1);
    assert_eq!(profiles[0][0].entity, *entities[0].id());
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
}

#[test]
fn closed_profile_publication_preserves_original_sticky_retained_refusal() {
    let entities = super::closed_profile_limit_entities();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let retained = retained_profile_bytes();
    let shell = u64::try_from(4 * std::mem::size_of::<Vec<SketchEntityUse>>()).unwrap();
    policy.limits.max_retained_bytes = retained - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    let error = super::super::closed_marker_profiles(&ctx, &entities).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("profile storage promotion must refuse")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "collect SLDPRT closed curve profile");
    assert_eq!(limit.limit, retained - 1);
    assert_eq!(limit.used, shell);
    assert_eq!(limit.additional, retained - shell);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let replay = ctx.charge_work(0, "probe refused profile session").unwrap_err();
    let CodecError::ResourceLimit(replay) = replay else {
        panic!("refused session must replay its original refusal")
    };
    assert_eq!(replay, limit);
}

#[test]
fn closed_profile_publication_preserves_original_sticky_scratch_refusal() {
    let entities = super::closed_profile_limit_entities();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let scratch = profile_scratch_bytes();
    let copied_identity = u64::try_from(entities[1].id().as_str().len()).unwrap();
    policy.limits.max_materialized_bytes = scratch - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    let error = super::super::closed_marker_profiles(&ctx, &entities).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("second profile identity must refuse")
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "copy SLDPRT closed curve identity");
    assert_eq!(limit.limit, scratch - 1);
    assert_eq!(limit.used, scratch - copied_identity);
    assert_eq!(limit.additional, copied_identity);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let replay = ctx.charge_work(0, "probe refused scratch session").unwrap_err();
    let CodecError::ResourceLimit(replay) = replay else {
        panic!("refused session must replay its original refusal")
    };
    assert_eq!(replay, limit);
}
