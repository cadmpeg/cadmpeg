// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

pub trait Admission {
    fn work(&self, count: usize) -> Result<(), ResourceLimit>;
}

pub struct DecodeAdmission<'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
}

impl Admission for DecodeAdmission<'_, '_> {
    fn work(&self, count: usize) -> Result<(), ResourceLimit> {
        self.ctx.charge_work_limit(u64_from_index(count), "member work")
    }
}

pub struct MemberKey<'a, T, S> {
    value: T,
    admission: &'a S,
}

impl<T: PartialEq, S: Admission> PartialEq for MemberKey<'_, T, S> {
    fn eq(&self, other: &Self) -> bool {
        self.admission.work(1).is_ok() && self.value == other.value
    }
}

impl<T: Eq, S: Admission> Eq for MemberKey<'_, T, S> {}

impl<T: Hash, S: Admission> Hash for MemberKey<'_, T, S> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

fn distinct<T: Eq + Hash, S: Admission>(admission: &S, values: &[T]) -> Result<bool, ResourceLimit> {
    let mut index = HashSet::new();
    for value in values {
        admission.work(1)?;
        // The key's Hash and PartialEq run inside std's insertion, where the
        // call graph cannot follow them; only derived callbacks are bounded.
        if !index.insert(MemberKey { value, admission }) { // finding: unproven_decode_charge
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn decode_distinct(ctx: &DecodeContext<'_>, values: &[String]) -> Result<bool, ResourceLimit> {
    distinct(&DecodeAdmission { ctx }, values)
}
