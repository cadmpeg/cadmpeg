//! Native owner staging storage tests.

use crate::native::attach_to_owners;
use crate::records::FeatureInputName;
use cadmpeg_core::decode::{
    refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;

#[test]
fn native_owner_staging_releases_outer_shells_and_index() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let (owned, storage) = attach_to_owners(
        &ctx,
        &["first", "second"],
        Vec::<FeatureInputName>::new(),
        |name| name.parent.as_str(),
        |name| name.ordinal,
    ).expect("empty owner slots are scratch");
    assert_eq!(owned.len(), 2);
    assert!(owned.iter().all(Vec::is_empty));
    drop((owned, storage));
    let _probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "after SLDPRT owner staging",
        Some(1),
    );
    let error = ctx.reserve_scoped(1, "after SLDPRT owner staging")
        .err().expect("named scratch boundary refuses");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected scratch refusal");
    };
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn native_owner_staging_retains_output_member_storage() {
    let record = FeatureInputName {
        id: "name".into(),
        parent: "owner".into(),
        ordinal: 0,
        offset: 0,
        object_id: None,
        value: "Object".into(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = attach_to_owners(
        &ctx,
        &["owner"],
        vec![record],
        |name| name.parent.as_str(),
        |name| name.ordinal,
    ).err().expect("output member slots need retained admission");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected retained refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "attach SLDPRT native records to owners");
    assert!(limit.additional > 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
