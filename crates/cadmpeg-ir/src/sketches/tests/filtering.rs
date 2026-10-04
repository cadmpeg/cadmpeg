use crate::sketches::{SketchEntityId, SketchEntityUse, SketchProfiles};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

#[test]
fn sketch_profile_filter_propagates_predicate_resource_refusal_without_mutation() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (refusal_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(expected) =
        refusal_ctx.charge_work(1, "filter predicate").unwrap_err()
    else {
        panic!("resource limit expected");
    };
    let usage = SketchEntityUse {
        entity: SketchEntityId::mint("synthetic:test:sketch-entity#filter").unwrap(),
        reversed: false,
    };
    let mut profiles = SketchProfiles::try_from(vec![vec![usage.clone(), usage]]).unwrap();
    let before = profiles.clone();
    let mut calls = 0;
    let error = profiles
        .retain_uses(&ctx, |_| {
            calls += 1;
            if calls == 2 {
                return Err(CodecError::ResourceLimit(expected));
            }
            Ok(false)
        })
        .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit == expected));
    assert_eq!(profiles, before);
    assert_eq!(calls, 2);
}
