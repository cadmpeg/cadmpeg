// SPDX-License-Identifier: Apache-2.0
use super::{distinct, DecodeAdmission, MemberHasher, StandardAdmission};
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

struct Colliding {
    value: u8,
    hashes: Rc<Cell<u64>>,
    comparisons: Rc<Cell<u64>>,
}
impl PartialEq for Colliding {
    fn eq(&self, other: &Self) -> bool {
        self.comparisons.set(self.comparisons.get() + 1);
        self.value == other.value
    }
}
impl Eq for Colliding {}
impl DecodeCost for Colliding {
    const FIXED_BYTES: Option<u64> = Some(1);

    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(1)
    }
}
impl Hash for Colliding {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hashes.set(self.hashes.get() + 1);
        state.write(&[0]);
    }
}

#[test]
fn membership_hash_callbacks_admit_once_and_release_the_index() {
    // Pinned total: 3 source visits, 3 hash calls, 3 hash bytes, 3 comparison steps, 6 operand bytes.
    for allowance in 0..=18 {
        let hashes = Rc::new(Cell::new(0));
        let comparisons = Rc::new(Cell::new(0));
        let values: Vec<_> = (0..3)
            .map(|value| Colliding {
                value,
                hashes: hashes.clone(),
                comparisons: comparisons.clone(),
            })
            .collect();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        policy.limits.max_materialized_bytes = 200;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 3;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = distinct(
            &DecodeAdmission::new(&ctx, "collision admission"),
            &values,
            values.len(),
            |_| true,
        );
        if allowance < 18 {
            let CodecError::ResourceLimit(limit) = result.unwrap_err() else {
                panic!("resource refusal required");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "collision admission");
            if allowance == 6 {
                assert_eq!(comparisons.get(), 0);
            }
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        } else {
            assert!(result.unwrap());
            assert_eq!(hashes.get(), 3);
            assert_eq!(comparisons.get(), 3);
            let storage = ctx
                .reserve_scoped_limit(200, "membership index released")
                .unwrap();
            drop(storage);
            let limit = ctx
                .charge_work_limit(1, "exact membership work")
                .unwrap_err();
            assert_eq!(limit.used, 18);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }
}

#[derive(Default)]
struct WrittenBytes(usize);
impl Hasher for WrittenBytes {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0 += bytes.len();
    }
}

#[test]
fn membership_hasher_refuses_before_copying_each_byte_chunk() {
    for allowance in [0, 4] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let admission = DecodeAdmission::new(&ctx, "hash byte chunk");
        let mut written = WrittenBytes::default();
        let mut state: MemberHasher<'_, _, _, Colliding> = MemberHasher {
            state: &mut written,
            admission: &admission,
            _value: std::marker::PhantomData,
        };
        state.write(&[1, 2, 3, 4]);
        if allowance == 4 {
            state.write(&[5]);
        }
        assert_eq!(written.0, usize::try_from(allowance).unwrap());
        let original = ctx
            .charge_work_limit(0, "observe original hash refusal")
            .unwrap_err();
        assert_eq!(original.operation, "hash byte chunk");
        assert_eq!(original.used, allowance);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }
}

#[derive(Debug)]
struct RefusingComparison {
    key: u8,
    cost_calls: Rc<Cell<usize>>,
    equality_calls: Rc<Cell<usize>>,
}

impl PartialEq for RefusingComparison {
    fn eq(&self, other: &Self) -> bool {
        self.equality_calls
            .set(self.equality_calls.get().saturating_add(1));
        self.key == other.key
    }
}
impl Eq for RefusingComparison {}
impl Hash for RefusingComparison {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&[0]);
    }
}
impl DecodeCost for RefusingComparison {
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        self.cost_calls
            .set(self.cost_calls.get().saturating_add(1));
        Err(CodecError::malformed("comparison cost refused"))
    }
}

fn refusing_comparison(key: u8) -> RefusingComparison {
    RefusingComparison {
        key,
        cost_calls: Rc::new(Cell::new(0)),
        equality_calls: Rc::new(Cell::new(0)),
    }
}

#[test]
fn distinct_members_collision_equality_preserves_non_resource_codec_error() {
    use crate::features::{DistinctMembers, FeatureCollectionError};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = refusing_comparison(1);
    let second = refusing_comparison(2);
    let first_cost_calls = first.cost_calls.clone();
    let second_cost_calls = second.cost_calls.clone();
    let first_equality_calls = first.equality_calls.clone();
    let second_equality_calls = second.equality_calls.clone();
    let error = DistinctMembers::try_from(vec![first, second], &ctx).unwrap_err();

    assert!(matches!(
        error,
        FeatureCollectionError::Codec(CodecError::Malformed(message))
            if message == "comparison cost refused"
    ));
    assert_eq!(first_cost_calls.get() + second_cost_calls.get(), 1);
    assert_eq!(first_equality_calls.get() + second_equality_calls.get(), 0);
}

#[test]
fn distinct_members_vec_insert_preserves_non_resource_codec_error() {
    use crate::features::DistinctMembers;

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = refusing_comparison(1);
    let cost_calls = first.cost_calls.clone();
    let equality_calls = first.equality_calls.clone();
    let mut members = DistinctMembers::default();
    members.extend([first]);

    let result = members.insert(&ctx, refusing_comparison(2), "vector collision comparison");
    assert!(matches!(
        result,
        Err(CodecError::Malformed(message)) if message == "comparison cost refused"
    ));
    assert_eq!(members.len(), 1);
    assert_eq!(cost_calls.get(), 1);
    assert_eq!(equality_calls.get(), 0);
}

#[test]
fn feature_collection_error_round_trips_non_resource_codec_error() {
    use crate::features::FeatureCollectionError;

    let wrapped = FeatureCollectionError::from(CodecError::InvalidInput(
        "comparison cost refused".into(),
    ));
    assert!(matches!(
        &wrapped,
        FeatureCollectionError::Codec(CodecError::InvalidInput(message))
            if message == "comparison cost refused"
    ));

    let propagated = CodecError::from(wrapped);
    assert!(matches!(
        propagated,
        CodecError::InvalidInput(message) if message == "comparison cost refused"
    ));
}

#[test]
fn standard_membership_keeps_eq_hash_values_without_decode_cost() {
    use crate::features::{DistinctMembers, SelectionMembers};

    #[derive(Debug, PartialEq, Eq, Hash)]
    struct StandardOnly(u8);

    let mut distinct_members = DistinctMembers::default();
    distinct_members.extend([StandardOnly(1), StandardOnly(2)]);
    assert!(distinct(&StandardAdmission, &[StandardOnly(5), StandardOnly(6)], 2, |_| true)
        .unwrap());
    let selections = <SelectionMembers<StandardOnly> as TryFrom<Vec<StandardOnly>>>::try_from(
        vec![StandardOnly(3), StandardOnly(4)],
    )
    .unwrap();

    assert_eq!(distinct_members.len(), 2);
    assert_eq!(selections.as_slice().len(), 2);
}
