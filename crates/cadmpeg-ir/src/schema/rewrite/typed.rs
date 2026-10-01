// SPDX-License-Identifier: Apache-2.0
//! Typed identity replacement under the caller's decode policy.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

/// Rewrite owned fields without projecting or reconstructing a serde value.
pub trait RewriteIdentities: Sized {
    /// Return the rewritten value or the original resource refusal.
    fn rewrite_identities<F>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError>
    where
        F: FnMut(&str) -> Result<String, CodecError>;
}

/// One injective identity mapping with a live reservation for its cache.
#[derive(Debug)]
pub struct IdentityMap<'ctx, F> {
    map: F,
    targets: BTreeMap<String, String>,
    occupied: BTreeSet<String>,
    storage: ScopedReservation<'ctx>,
    longest: usize,
}

impl<'ctx, F: FnMut(&str) -> Result<String, CodecError>> IdentityMap<'ctx, F> {
    /// Admit scratch before creating the identity cache.
    pub fn new(ctx: &'ctx DecodeContext<'_>, map: F) -> Result<Self, CodecError> {
        Ok(Self {
            map,
            targets: BTreeMap::new(),
            occupied: BTreeSet::new(),
            storage: ctx.reserve_scoped(0, "identity rewrite cache")?,
            longest: 0,
        })
    }

    /// Replace an identity once and refuse invalid or colliding targets.
    pub fn identity(&mut self, ctx: &DecodeContext<'_>, source: &str) -> Result<String, CodecError> {
        const OPERATION: &str = "typed identity rewrite";
        self.longest = self.longest.max(source.len());
        let work = u64_from_index(self.longest)
            .checked_mul(u64_from_index(self.targets.len()).checked_add(1).ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, OPERATION)?;
        if let Some(target) = self.targets.get(source) {
            return ctx.copy_retained_text(target, OPERATION);
        }
        let target = (self.map)(source)?;
        self.longest = self.longest.max(target.len());
        ctx.charge_work(u64_from_index(target.len()), OPERATION)?;
        if !crate::ids::is_valid_identity(&target) {
            return Err(CodecError::malformed(format_args!("identity {source} rewrites to invalid identity {target:?}")));
        }
        let copies = u64_from_index(source.len()).checked_add(u64_from_index(target.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?).ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(copies, OPERATION)?;
        let key = ctx.copy_scoped_text(source, &mut self.storage, OPERATION)?;
        let cached = ctx.copy_scoped_text(&target, &mut self.storage, OPERATION)?;
        let occupied = ctx.copy_scoped_text(&target, &mut self.storage, OPERATION)?;
        ctx.charge_work(u64_from_index(self.longest).checked_mul(u64_from_index(self.occupied.len())).ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if !ctx.insert_scoped_btree_set(&mut self.storage, &mut self.occupied, occupied, OPERATION, OPERATION)? {
            return Err(CodecError::malformed(format_args!("identity {source} collides at rewritten identity {target}")));
        }
        // The absent source was checked before invoking the mapping callback.
        if !ctx.insert_scoped_btree_map_if_vacant(&mut self.storage, &mut self.targets, key, cached, OPERATION, OPERATION)? {
            return Err(CodecError::malformed("identity rewrite cache contains the source"));
        }
        Ok(target)
    }
}

impl RewriteIdentities for crate::ids::Identity {
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        let target = map.identity(ctx, self.as_str())?;
        ctx.charge_work(u64_from_index(target.len()), "rewrite identity grammar")?;
        Self::new(target).map_err(CodecError::malformed)
    }
}

macro_rules! rewrite_scalars {
    ($($type:ty),* $(,)?) => {$(
        impl RewriteIdentities for $type {
            fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, _map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
                ctx.charge_work(1, "identity rewrite scalar")?;
                Ok(self)
            }
        }
    )*};
}
rewrite_scalars!(bool, char, u8, u16, u32, u64, usize, i8, i16, i32, i64, isize, f32, f64, ());

impl RewriteIdentities for String {
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, _map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        ctx.charge_work(1, "identity rewrite text node")?;
        Ok(self)
    }
}

impl<T: RewriteIdentities> RewriteIdentities for Option<T> {
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite option")?;
        ctx.charge_work(1, "identity rewrite option")?;
        self.map(|value| value.rewrite_identities(ctx, map)).transpose()
    }
}

impl<T: RewriteIdentities> RewriteIdentities for Vec<T> {
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite sequence")?;
        ctx.try_collect_vec(self.into_iter().map(|value| value.rewrite_identities(ctx, map)), "identity rewrite sequence")
    }
}

impl<T: RewriteIdentities, const N: usize> RewriteIdentities for [T; N] {
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite array")?;
        let (mut values, _storage) = ctx.temporary_vec(N, "identity rewrite array")?;
        for value in self {
            values.push(value.rewrite_identities(ctx, map)?);
        }
        values.try_into().map_err(|_| CodecError::malformed("identity rewrite changed array length"))
    }
}

impl<K: RewriteIdentities + Ord, V: RewriteIdentities> RewriteIdentities for BTreeMap<K, V> {
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite map")?;
        let mut rewritten = Self::new();
        for (key, value) in self {
            let key = key.rewrite_identities(ctx, map)?;
            let value = value.rewrite_identities(ctx, map)?;
            ctx.insert_btree_map(&mut rewritten, key, value, "identity rewrite map")?;
        }
        Ok(rewritten)
    }
}

#[cfg(test)]
mod tests;
