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
    operation: &'static str,
    refused: Option<String>,
}

impl<'ctx, F: FnMut(&str) -> Result<String, CodecError>> IdentityMap<'ctx, F> {
    /// Admit scratch before creating the identity cache.
    pub fn new(ctx: &'ctx DecodeContext<'_>, operation: &'static str, map: F) -> Result<Self, CodecError> {
        Ok(Self {
            map,
            targets: BTreeMap::new(),
            occupied: BTreeSet::new(),
            storage: ctx.reserve_scoped(0, operation)?,
            longest: 0,
            operation,
            refused: None,
        })
    }

    fn refuse<T>(&mut self, ctx: &DecodeContext<'_>, message: std::fmt::Arguments<'_>) -> Result<T, CodecError> {
        let message = ctx.format_retained(message, self.operation)?;
        ctx.charge_work(u64_from_index(message.len()), self.operation)?;
        self.refused = Some(ctx.copy_scoped_text(&message, &mut self.storage, self.operation)?);
        Err(CodecError::Malformed(message))
    }

    /// Return any mapping refusal that an enclosing field walk intercepted.
    pub fn finish(&self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        ctx.charge_work(0, self.operation)?;
        match &self.refused {
            Some(message) => Err(CodecError::Malformed(ctx.copy_retained_text(message, self.operation)?)),
            None => Ok(()),
        }
    }

    /// Replace an identity once and refuse invalid or colliding targets.
    pub fn identity(&mut self, ctx: &DecodeContext<'_>, source: &str) -> Result<String, CodecError> {
        let operation = self.operation;
        self.longest = self.longest.max(source.len());
        let work = u64_from_index(self.longest)
            .checked_mul(u64_from_index(self.targets.len()).checked_add(1).ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, operation)?;
        if let Some(target) = self.targets.get(source) {
            return ctx.copy_retained_text(target, operation);
        }
        let target = (self.map)(source)?;
        self.longest = self.longest.max(target.len());
        ctx.charge_work(u64_from_index(target.len()), operation)?;
        if !crate::ids::is_valid_identity(&target) {
            return self.refuse(ctx, format_args!("identity {source} rewrites to invalid identity {target:?}"));
        }
        let copies = u64_from_index(source.len()).checked_add(u64_from_index(target.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?).ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(copies, operation)?;
        let key = ctx.copy_scoped_text(source, &mut self.storage, operation)?;
        let cached = ctx.copy_scoped_text(&target, &mut self.storage, operation)?;
        let occupied = ctx.copy_scoped_text(&target, &mut self.storage, operation)?;
        ctx.charge_work(u64_from_index(self.longest).checked_mul(u64_from_index(self.occupied.len())).ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?, operation)?;
        if !ctx.insert_scoped_btree_set(&mut self.storage, &mut self.occupied, occupied, operation, operation)? {
            return self.refuse(ctx, format_args!("identity {source} collides at rewritten identity {target}"));
        }
        // The absent source was checked before invoking the mapping callback.
        if !ctx.insert_scoped_btree_map_if_vacant(&mut self.storage, &mut self.targets, key, cached, operation, operation)? {
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

impl<K: RewriteIdentities + Ord + std::hash::Hash, V: RewriteIdentities> RewriteIdentities for BTreeMap<K, V> {
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite map")?;
        let mut rewritten = Self::new();
        let mut longest = 0;
        for (key, value) in self {
            let key = key.rewrite_identities(ctx, map)?;
            let value = value.rewrite_identities(ctx, map)?;
            crate::features::member_work::admit_member_work(ctx, &key, rewritten.len(), &mut longest, "identity rewrite map")?;
            ctx.insert_btree_map(&mut rewritten, key, value, "identity rewrite map")?;
        }
        Ok(rewritten)
    }
}

#[cfg(test)]
mod tests;

impl RewriteIdentities for cadmpeg_core::text::NonBlankString {
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, _map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        ctx.charge_work(1, "rewrite nonblank text node")?;
        Ok(self)
    }
}
rewrite_scalars!(std::num::NonZeroI64, std::num::NonZeroU32);

impl<T: RewriteIdentities> RewriteIdentities for Box<T> {
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite box")?;
        ctx.charge_collection_items(1, "identity rewrite box")?;
        ctx.charge_retained(u64_from_index(std::mem::size_of::<T>()), "identity rewrite box")?;
        Ok(Box::new((*self).rewrite_identities(ctx, map)?))
    }
}

macro_rules! rewrite_tuple {
    ($($type:ident: $field:ident),*) => {
        impl<$($type: RewriteIdentities),*> RewriteIdentities for ($($type,)*) {
            fn rewrite_identities<RewriteMapFn: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, RewriteMapFn>) -> Result<Self, CodecError> {
                let _depth = ctx.enter_nested("identity rewrite tuple")?;
                let ($($field,)*) = self;
                Ok(($($field.rewrite_identities(ctx, map)?,)*))
            }
        }
    };
}
rewrite_tuple!(A: first, B: second);
rewrite_tuple!(A: first, B: second, C: third);
