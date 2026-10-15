// SPDX-License-Identifier: Apache-2.0
//! Typed identity replacement under the caller's decode policy.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::CodecError;

mod cache;
pub mod native_fields;

use cache::ReplacementIndex;

/// Rewrite owned fields without projecting or reconstructing a serde value.
pub trait RewriteIdentities: Sized {
    /// False only when identity and text replacement cannot change this value.
    const HAS_IDENTITY_FIELDS: bool = true;

    /// Rewrite identity markers in an existing native wire value.
    /// Owners without a declared native wire layout refuse this operation.
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
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

/// One injective identity mapping and its cache.
#[derive(Debug)]
pub struct IdentityMap<'ctx, F> {
    map: F,
    text_index: Option<ReplacementIndex<'ctx>>,
    targets: HashMap<String, crate::ids::Identity>,
    occupied: HashSet<String>,
    refused: Option<String>,
    resource_refusal: Option<ResourceLimit>,
}

impl<'ctx, F: FnMut(&str) -> Result<String, CodecError>> IdentityMap<'ctx, F> {
    /// Create the identity cache.
    pub fn new(
        _ctx: &'ctx DecodeContext<'_>,
        _operation: &'static str,
        map: F,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            map,
            text_index: None,
            targets: HashMap::new(),
            occupied: HashSet::new(),
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
        self.text_index = Some(ReplacementIndex::build(replacements)?);
        Ok(self)
    }

    fn text(&mut self, source: String) -> Result<String, CodecError> {
        if let Some(limit) = self.resource_refusal {
            return Err(CodecError::ResourceLimit(limit));
        }
        let Some(index) = &self.text_index else {
            return Ok(source);
        };
        Ok(index.get(&source).map_or(source, str::to_owned))
    }

    fn refuse<T>(&mut self, message: std::fmt::Arguments<'_>) -> Result<T, CodecError> {
        let message = std::fmt::format(message);
        self.refused = Some(message.clone());
        Err(CodecError::Malformed(message))
    }

    /// Return any mapping refusal that an enclosing field walk intercepted.
    pub fn finish(&self, _ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        if let Some(limit) = self.resource_refusal {
            return Err(CodecError::ResourceLimit(limit));
        }

        match &self.refused {
            Some(message) => Err(CodecError::Malformed(message.to_owned())),
            None => Ok(()),
        }
    }

    /// Replace an identity once and refuse invalid or colliding targets.
    pub fn identity(
        &mut self,
        _ctx: &DecodeContext<'_>,
        source: &str,
    ) -> Result<String, CodecError> {
        self.identity_value(source)
            .map(crate::ids::Identity::into_string)
    }

    fn identity_value(&mut self, source: &str) -> Result<crate::ids::Identity, CodecError> {
        let result = self.replace_identity(source);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            if self.resource_refusal.is_none() {
                self.resource_refusal = Some(*limit);
            }
        }
        result
    }

    fn replace_identity(&mut self, source: &str) -> Result<crate::ids::Identity, CodecError> {
        if let Some(limit) = self.resource_refusal {
            return Err(CodecError::ResourceLimit(limit));
        }
        if let Some(target) = self.targets.get(source) {
            return Ok(target.clone());
        }
        let target = (self.map)(source)?;
        let target = match crate::ids::Identity::admit_text(target, |_| {
            Ok::<(), std::convert::Infallible>(())
        }) {
            Ok(Ok(target)) => target,
            Ok(Err(target)) => {
                return self.refuse(format_args!(
                    "identity {source} rewrites to invalid identity {target:?}"
                ))
            }
            Err(never) => match never {},
        };
        if !self.occupied.insert(target.as_str().to_owned()) {
            return self.refuse(format_args!(
                "identity {source} collides at rewritten identity {target}"
            ));
        }
        self.targets.insert(source.to_owned(), target.clone());
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

        let serde_json::Value::String(source) = value else {
            return Err(CodecError::malformed("native identity must be text"));
        };
        *source = map.identity(ctx, source)?;
        Ok(())
    }

    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(self.as_str())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        map.identity_value(self.as_str())
    }
}

macro_rules! rewrite_scalars {
    ($($type:ty),* $(,)?) => {$(
        impl RewriteIdentities for $type {
            const HAS_IDENTITY_FIELDS: bool = false;
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(_ctx: &DecodeContext<'_>, _value: &mut serde_json::Value, _map: &mut IdentityMap<'_, F>) -> Result<(), CodecError> {
        Ok(())
    }

            fn visit_identity_references(&self, _ctx: &DecodeContext<'_>, _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                Ok(())
            }
            fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, _ctx: &DecodeContext<'_>, _map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
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
        _ctx: &DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        Ok(())
    }

    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        map.text(self)
    }
}

impl<T: RewriteIdentities> RewriteIdentities for Option<T> {
    const HAS_IDENTITY_FIELDS: bool = T::HAS_IDENTITY_FIELDS;
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk native identity collection")?;

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

        self.map(|value| value.rewrite_identities(ctx, map))
            .transpose()
    }
}

impl<T: RewriteIdentities> RewriteIdentities for Vec<T> {
    const HAS_IDENTITY_FIELDS: bool = T::HAS_IDENTITY_FIELDS;
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk native identity collection")?;

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
        if !T::HAS_IDENTITY_FIELDS {
            return Ok(self);
        }
        let _depth = ctx.enter_nested("identity rewrite sequence")?;

        // The consuming, same-type map keeps Vec's allocation through the
        // standard library's in-place collection path, including fallible walks.
        self.into_iter()
            .map(|value| value.rewrite_identities(ctx, map))
            .collect()
    }
}

impl<T: RewriteIdentities, const N: usize> RewriteIdentities for [T; N] {
    const HAS_IDENTITY_FIELDS: bool = T::HAS_IDENTITY_FIELDS;
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk native identity collection")?;

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
        let mut values = Vec::new();
        for value in self {
            values.push(value.rewrite_identities(ctx, map)?);
        }

        values
            .try_into()
            .map_err(|_| CodecError::malformed("identity rewrite changed array length"))
    }
}

impl<K: RewriteIdentities + Ord, V: RewriteIdentities> RewriteIdentities for BTreeMap<K, V> {
    const HAS_IDENTITY_FIELDS: bool = K::HAS_IDENTITY_FIELDS || V::HAS_IDENTITY_FIELDS;
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed reference map")?;

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
            rewritten.insert(key, value);
        }
        Ok(rewritten)
    }
}

#[cfg(test)]
mod tests;

impl RewriteIdentities for cadmpeg_core::text::NonBlankString {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        Ok(())
    }

    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let text = map.text(self.into_string())?;

        Self::new(text).ok_or_else(|| CodecError::malformed("rewritten text must be nonblank"))
    }
}
rewrite_scalars!(std::num::NonZeroI64, std::num::NonZeroU32);

impl<T: RewriteIdentities> RewriteIdentities for Box<T> {
    const HAS_IDENTITY_FIELDS: bool = T::HAS_IDENTITY_FIELDS;
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk native identity collection")?;

        T::rewrite_native_value(ctx, value, map)?;
        Ok(())
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed reference box")?;

        self.as_ref().visit_identity_references(ctx, visitor)?;
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite box")?;

        Ok(Box::new((*self).rewrite_identities(ctx, map)?))
    }
}

macro_rules! rewrite_tuple {
    ($($type:ident: $field:ident),*) => {
        impl<$($type: RewriteIdentities),*> RewriteIdentities for ($($type,)*) {
            const HAS_IDENTITY_FIELDS: bool = false $(|| $type::HAS_IDENTITY_FIELDS)*;
            fn visit_identity_references(&self, ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                let _depth = ctx.enter_nested("walk typed reference tuple")?;
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
