// SPDX-License-Identifier: Apache-2.0
//! Typed identity replacement under the caller's decode policy.

use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit, ScopedReservation};
use cadmpeg_core::CodecError;

mod cache;
pub mod native_fields;

use cache::{Key, ReplacementIndex};

/// Rewrite owned fields without projecting or reconstructing a serde value.
pub trait RewriteIdentities: Sized {
    /// Rewrite identity markers in an existing native wire value.
    /// Owners without a declared native wire layout refuse this operation.
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        ctx.charge_work(1, "check native identity layout")?;
        Err(CodecError::malformed(
            "typed owner has no native identity layout",
        ))
    }

    /// Visit borrowed typed references without projecting a value tree.
    /// Scalar owners admit one visit and expose no references.
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError>;

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
    context: &'ctx DecodeContext<'ctx>,
    text_index: Option<ReplacementIndex<'ctx>>,
    targets: BTreeMap<Key<'ctx>, crate::ids::Identity>,
    occupied: BTreeSet<Key<'ctx>>,
    operation: &'static str,
    refused: Option<String>,
    resource_refusal: Option<ResourceLimit>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, F: FnMut(&str) -> Result<String, CodecError>> IdentityMap<'ctx, F> {
    /// Admit scratch before creating the identity cache.
    pub fn new(
        ctx: &'ctx DecodeContext<'_>,
        operation: &'static str,
        map: F,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            map,
            context: ctx,
            text_index: None,
            targets: BTreeMap::new(),
            occupied: BTreeSet::new(),
            storage: ctx.reserve_scoped(0, operation)?,
            operation,
            refused: None,
            resource_refusal: None,
        })
    }

    /// Also rewrite ordinary text that exactly names an owned source identity.
    /// Source keys must expose unique text in byte order.
    pub fn with_text_replacements<K: AsRef<str> + 'ctx>(
        mut self,
        replacements: impl ExactSizeIterator<Item = (&'ctx K, &'ctx String)>,
    ) -> Result<Self, CodecError> {
        self.text_index = Some(ReplacementIndex::build(
            self.context,
            replacements,
            &mut self.storage,
            self.operation,
        )?);
        Ok(self)
    }

    fn text(&mut self, ctx: &DecodeContext<'_>, source: String) -> Result<String, CodecError> {
        let result = (|| {
            if let Some(limit) = self.resource_refusal {
                return Err(CodecError::ResourceLimit(limit));
            }
            ctx.charge_work(1, self.operation)?;
            let Some(index) = &self.text_index else {
                return Ok(source);
            };
            match index.get(ctx, &source, self.operation)? {
                Some(target) => ctx.copy_retained_text(target, self.operation),
                None => Ok(source),
            }
        })();
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            if self.resource_refusal.is_none() {
                self.resource_refusal = Some(*limit);
            }
        }
        result
    }

    fn refuse<T>(
        &mut self,
        ctx: &DecodeContext<'_>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<T, CodecError> {
        let message = ctx.format_retained(message, self.operation)?;
        self.refused = Some(
            self.storage
                .with_storage(|| ctx.copy_retained_text(&message, self.operation))?,
        );
        Err(CodecError::Malformed(message))
    }

    /// Return any mapping refusal that an enclosing field walk intercepted.
    pub fn finish(&self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        if let Some(limit) = self.resource_refusal {
            return Err(CodecError::ResourceLimit(limit));
        }
        ctx.charge_work(0, self.operation)?;
        match &self.refused {
            Some(message) => Err(CodecError::Malformed(
                ctx.copy_retained_text(message, self.operation)?,
            )),
            None => Ok(()),
        }
    }

    /// Replace an identity once and refuse invalid or colliding targets.
    pub fn identity(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: &str,
    ) -> Result<String, CodecError> {
        self.identity_value(ctx, source)
            .map(crate::ids::Identity::into_string)
    }

    fn identity_value(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: &str,
    ) -> Result<crate::ids::Identity, CodecError> {
        let result = self.replace_identity(ctx, source);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            if self.resource_refusal.is_none() {
                self.resource_refusal = Some(*limit);
            }
        }
        result
    }

    fn replace_identity(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: &str,
    ) -> Result<crate::ids::Identity, CodecError> {
        if let Some(limit) = self.resource_refusal {
            return Err(CodecError::ResourceLimit(limit));
        }
        let operation = self.operation;
        ctx.charge_work(1, operation)?;
        let cached = self
            .targets
            .get(&Key::borrowed(self.context, source, operation));
        self.context.charge_work_limit(0, operation)?;
        if let Some(target) = cached {
            return target.try_clone_for_decode(ctx, operation);
        }
        let target = (self.map)(source)?;
        let target = match crate::ids::Identity::admit_text(target, |work| {
            ctx.charge_work(work, operation)
        })? {
            Ok(target) => target,
            Err(target) => {
                return self.refuse(
                    ctx,
                    format_args!("identity {source} rewrites to invalid identity {target:?}"),
                )
            }
        };
        let key = self
            .storage
            .with_storage(|| ctx.copy_retained_text(source, operation))?;
        let cached = self
            .storage
            .with_storage(|| target.try_clone_for_decode(ctx, operation))?;
        let occupied = self
            .storage
            .with_storage(|| ctx.copy_retained_text(target.as_str(), operation))?;
        let destination = Key::owned(self.context, occupied, operation);
        let collision = self.occupied.contains(&destination);
        self.context.charge_work_limit(0, operation)?;
        if collision {
            return self.refuse(
                ctx,
                format_args!("identity {source} collides at rewritten identity {target}"),
            );
        }
        self.storage
            .with_storage(|| ctx.admit_retained_btree_record::<Key<'_>, ()>(0, operation))?;
        self.occupied.insert(destination);
        self.context.charge_work_limit(0, operation)?;
        let source_entry = self.targets.entry(Key::owned(self.context, key, operation));
        self.context.charge_work_limit(0, operation)?;
        match source_entry {
            Entry::Occupied(_) => {
                return Err(CodecError::malformed(
                    "identity rewrite cache contains the source",
                ))
            }
            Entry::Vacant(entry) => {
                self.storage.with_storage(|| {
                    ctx.admit_retained_btree_record::<Key<'_>, crate::ids::Identity>(0, operation)
                })?;
                entry.insert(cached);
            }
        }
        Ok(target)
    }
}

impl RewriteIdentities for crate::ids::Identity {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("rewrite native identity")?;
        ctx.charge_work(1, "rewrite native identity")?;
        let serde_json::Value::String(source) = value else {
            return Err(CodecError::malformed("native identity must be text"));
        };
        *source = map.identity(ctx, source)?;
        Ok(())
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        ctx.charge_work(1, "walk typed identity reference")?;
        visitor(self.as_str())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        map.identity_value(ctx, self.as_str())
    }
}

macro_rules! rewrite_scalars {
    ($($type:ty),* $(,)?) => {$(
        impl RewriteIdentities for $type {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(ctx: &DecodeContext<'_>, _value: &mut serde_json::Value, _map: &mut IdentityMap<'_, F>) -> Result<(), CodecError> {
        ctx.charge_work(1, "walk native identity scalar")
    }

            fn visit_identity_references(&self, ctx: &DecodeContext<'_>, _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                ctx.charge_work(1, "walk typed reference scalar")
            }
            fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, _map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
                ctx.charge_work(1, "identity rewrite scalar")?;
                Ok(self)
            }
        }
    )*};
}
rewrite_scalars!(
    bool,
    char,
    u8,
    u16,
    u32,
    u64,
    usize,
    i8,
    i16,
    i32,
    i64,
    isize,
    f32,
    f64,
    ()
);

impl RewriteIdentities for String {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        ctx.charge_work(1, "walk native identity scalar")
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        ctx.charge_work(1, "walk typed reference scalar")
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        map.text(ctx, self)
    }
}

impl<T: RewriteIdentities> RewriteIdentities for Option<T> {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk native identity collection")?;
        ctx.charge_work(1, "walk native identity collection")?;
        if !value.is_null() {
            T::rewrite_native_value(ctx, value, map)?;
        }
        Ok(())
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed reference option")?;
        ctx.charge_work(1, "walk typed reference option")?;
        if let Some(value) = self {
            value.visit_identity_references(ctx, visitor)?;
        }
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite option")?;
        ctx.charge_work(1, "identity rewrite option")?;
        self.map(|value| value.rewrite_identities(ctx, map))
            .transpose()
    }
}

impl<T: RewriteIdentities> RewriteIdentities for Vec<T> {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk native identity collection")?;
        ctx.charge_work(1, "walk native identity collection")?;
        if let serde_json::Value::Array(values) = value {
            for value in values {
                T::rewrite_native_value(ctx, value, map)?;
            }
        }
        Ok(())
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed reference sequence")?;
        ctx.charge_work(1, "walk typed reference sequence")?;
        for value in self {
            value.visit_identity_references(ctx, visitor)?;
        }
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite sequence")?;
        ctx.charge_work(1, "identity rewrite sequence")?;
        let mut rewritten = Vec::new();
        for value in self {
            let value = value.rewrite_identities(ctx, map)?;
            ctx.charge_work(
                u64_from_index(std::mem::size_of::<T>()),
                "identity rewrite sequence",
            )?;
            ctx.push_retained_vec(&mut rewritten, value, "identity rewrite sequence")?;
        }
        Ok(rewritten)
    }
}

impl<T: RewriteIdentities, const N: usize> RewriteIdentities for [T; N] {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk native identity collection")?;
        ctx.charge_work(1, "walk native identity collection")?;
        if let serde_json::Value::Array(values) = value {
            for value in values {
                T::rewrite_native_value(ctx, value, map)?;
            }
        }
        Ok(())
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed reference array")?;
        ctx.charge_work(1, "walk typed reference array")?;
        for value in self {
            value.visit_identity_references(ctx, visitor)?;
        }
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite array")?;
        let mut storage = ctx.reserve_scoped(0, "identity rewrite array")?;
        let mut values = Vec::new();
        ctx.reserve_scoped_vec(&mut storage, &mut values, N, "identity rewrite array")?;
        for value in self {
            values.push(value.rewrite_identities(ctx, map)?);
        }
        let result = values
            .try_into()
            .map_err(|_| CodecError::malformed("identity rewrite changed array length"));
        drop(storage);
        result
    }
}

impl<K: RewriteIdentities + Ord, V: RewriteIdentities> RewriteIdentities for BTreeMap<K, V> {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed reference map")?;
        ctx.charge_work(1, "walk typed reference map")?;
        for (key, value) in self {
            key.visit_identity_references(ctx, visitor)?;
            value.visit_identity_references(ctx, visitor)?;
        }
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
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

impl RewriteIdentities for cadmpeg_core::text::NonBlankString {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        ctx.charge_work(1, "walk native identity scalar")
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        ctx.charge_work(1, "walk typed reference scalar")
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let text = map.text(ctx, self.into_string())?;
        ctx.charge_work(u64_from_index(text.len()), "rewrite nonblank text")?;
        Self::new(text).ok_or_else(|| CodecError::malformed("rewritten text must be nonblank"))
    }
}
rewrite_scalars!(std::num::NonZeroI64, std::num::NonZeroU32);

impl<T: RewriteIdentities> RewriteIdentities for Box<T> {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk native identity collection")?;
        ctx.charge_work(1, "walk native identity collection")?;
        T::rewrite_native_value(ctx, value, map)?;
        Ok(())
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed reference box")?;
        ctx.charge_work(1, "walk typed reference box")?;
        self.as_ref().visit_identity_references(ctx, visitor)?;
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite box")?;
        ctx.charge_collection_items(1, "identity rewrite box")?;
        ctx.charge_retained(
            u64_from_index(std::mem::size_of::<T>()),
            "identity rewrite box",
        )?;
        Ok(Box::new((*self).rewrite_identities(ctx, map)?))
    }
}

macro_rules! rewrite_tuple {
    ($($type:ident: $field:ident),*) => {
        impl<$($type: RewriteIdentities),*> RewriteIdentities for ($($type,)*) {
            fn visit_identity_references(&self, ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                let _depth = ctx.enter_nested("walk typed reference tuple")?;
                ctx.charge_work(1, "walk typed reference tuple")?;
                let ($($field,)*) = self;
                $($field.visit_identity_references(ctx, visitor)?;)*
                Ok(())
            }
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
