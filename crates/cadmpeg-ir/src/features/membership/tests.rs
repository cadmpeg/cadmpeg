// SPDX-License-Identifier: Apache-2.0
use super::{distinct, DecodeAdmission, MemberHasher};
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
impl Hash for Colliding {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hashes.set(self.hashes.get() + 1);
        state.write(&[0]);
    }
}

#[test]
fn membership_hash_callbacks_admit_once_and_release_the_index() {
    for allowance in 0..=12 {
        let hashes = Rc::new(Cell::new(0));
        let comparisons = Rc::new(Cell::new(0));
        let values: Vec<_> = (0..3).map(|value| Colliding {
            value, hashes: hashes.clone(), comparisons: comparisons.clone(),
        }).collect();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        policy.limits.max_materialized_bytes = 200;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 3;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = distinct(&DecodeAdmission { ctx: &ctx, operation: "collision admission" }, &values, values.len(), |_| true);
        if allowance < 12 {
            let limit = result.unwrap_err();
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "collision admission");
            if allowance == 6 { assert_eq!(comparisons.get(), 0); }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        } else {
            assert!(result.unwrap());
            assert_eq!(hashes.get(), 3);
            assert_eq!(comparisons.get(), 3);
            let storage = ctx.reserve_scoped_limit(200, "membership index released").unwrap();
            drop(storage);
            let limit = ctx.charge_work_limit(1, "exact membership work").unwrap_err();
            assert_eq!(limit.used, 12);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    }
}

#[derive(Default)]
struct WrittenBytes(usize);
impl Hasher for WrittenBytes {
    fn finish(&self) -> u64 { 0 }
    fn write(&mut self, bytes: &[u8]) { self.0 += bytes.len(); }
}

#[test]
fn membership_hasher_refuses_before_copying_each_byte_chunk() {
    for allowance in [0, 4] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let admission = DecodeAdmission { ctx: &ctx, operation: "hash byte chunk" };
        let mut written = WrittenBytes::default();
        let mut state = MemberHasher { state: &mut written, admission: &admission };
        state.write(&[1, 2, 3, 4]);
        if allowance == 4 { state.write(&[5]); }
        assert_eq!(written.0, usize::try_from(allowance).unwrap());
        let original = ctx.charge_work_limit(0, "observe original hash refusal").unwrap_err();
        assert_eq!(original.operation, "hash byte chunk");
        assert_eq!(original.used, allowance);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
}
