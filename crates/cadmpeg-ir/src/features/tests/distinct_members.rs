// SPDX-License-Identifier: Apache-2.0
use crate::features::DistinctMembers;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn insert_for_decode_refuses_before_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut members = DistinctMembers::default();
    assert!(matches!(members.insert(&ctx, 1_u8, "test member insert"),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems));
    assert_eq!(members.0.capacity(), 0);
    assert!(members.is_empty());
}

#[test]
fn insert_for_decode_service_profile_and_duplicate_without_charge() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut members = DistinctMembers::default();
    assert!(members
        .insert(&ctx, 1_u8, "test member insert")
        .expect("service profile"));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(!members
        .insert(&ctx, 1, "test member duplicate")
        .expect("duplicate needs no slot"));
    assert_eq!(members.as_slice(), &[1]);
}

#[test]
fn reserve_for_decode_refuses_before_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut members = DistinctMembers::<u8>::default();
    assert!(
        matches!(members.reserve_for_decode(&ctx, 2, "test member reserve"),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert_eq!(members.0.capacity(), 0);
    assert!(members.is_empty());
}

#[test]
fn reserve_for_decode_service_profile() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut members = DistinctMembers::<u8>::default();
    members
        .reserve_for_decode(&ctx, 2, "test member reserve")
        .expect("service profile");
    assert!(members.0.capacity() >= 2);
    assert!(members.is_empty());
}

#[test]
fn member_insert_admits_only_the_comparisons_it_performs() {
    use std::cell::Cell;
    use std::rc::Rc;
    #[derive(Clone)]
    struct Counted(u8, Rc<Cell<u64>>);
    impl PartialEq for Counted {
        fn eq(&self, other: &Self) -> bool {
            self.1.set(self.1.get() + 1);
            self.0 == other.0
        }
    }
    impl cadmpeg_core::decode::cost::DecodeCost for Counted {
        fn decode_cost(
            &self,
            ctx: &DecodeContext<'_>,
            operation: &'static str,
        ) -> Result<u64, CodecError> {
            cadmpeg_core::decode::cost::DecodeCost::decode_cost(&self.0, ctx, operation)
        }
    }
    // Each checked member costs one visit and both one-byte key measurements.
    for allowance in 0..=6 {
        let comparisons = Rc::new(Cell::new(0));
        let mut members = DistinctMembers(vec![
            Counted(1, comparisons.clone()),
            Counted(2, comparisons.clone()),
        ]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = members.insert(&ctx, Counted(2, comparisons.clone()), "member comparisons");
        assert_eq!(comparisons.get(), allowance / 3);
        assert_eq!(members.len(), 2);
        if allowance < 6 {
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("work refusal required");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        } else {
            assert!(!result.unwrap());
            ctx.finish_session().unwrap();
        }
    }
    let comparisons = Rc::new(Cell::new(0));
    let mut members = DistinctMembers(vec![
        Counted(1, comparisons.clone()),
        Counted(2, comparisons.clone()),
    ]);
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(members
        .insert(&ctx, Counted(3, comparisons.clone()), "member append")
        .unwrap());
    assert_eq!(comparisons.get(), 2);
    assert_eq!(
        members.iter().map(|value| value.0).collect::<Vec<_>>(),
        [1, 2, 3]
    );
}

#[test]
fn long_parameter_member_comparison_refuses_work_before_equality() {
    use crate::features::ParameterId;

    let member = ParameterId::mint(format!("synthetic:test:parameter#{}", "x".repeat(64)))
        .expect("identity grammar");
    let mut members = DistinctMembers(vec![member.clone()]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let operation = "compare long parameter member";
    let error = members
        .append(&ctx, [member.clone()], operation)
        .expect_err("key comparison must be admitted");

    let CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal required");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, operation);
    assert_eq!(members.as_slice(), &[member]);
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(original)) if original == limit
    ));
}

#[test]
fn member_iterator_reconstruction_uses_the_shared_insertion_algorithm() {
    let mut members: DistinctMembers<_> = [2_u8, 1, 2].into_iter().collect();
    assert_eq!(members.as_slice(), [2, 1]);
    members.extend([1, 3, 2]);
    assert_eq!(members.as_slice(), [2, 1, 3]);
}
