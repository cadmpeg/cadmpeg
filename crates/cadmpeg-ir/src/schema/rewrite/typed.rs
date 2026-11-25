// SPDX-License-Identifier: Apache-2.0
//! Typed identity replacement under the caller's decode policy.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::CodecError;

mod cache;
pub mod native_fields;

pub use cache::TextReplacements;

thread_local! {
    static VISITED_ID_TYPE: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
}

/// Restores the previous visited id type when a visit ends, also by unwinding.
struct VisitedIdType(Option<&'static str>);

impl Drop for VisitedIdType {
    fn drop(&mut self) {
        VISITED_ID_TYPE.with(|cell| cell.set(self.0));
    }
}

/// Runs `visit` with `id_type` as the type of the reference it hands to its
/// visitor ([`visited_id_type`]).
pub(crate) fn visiting_id_type<R>(id_type: &'static str, visit: impl FnOnce() -> R) -> R {
    let _restore = VisitedIdType(VISITED_ID_TYPE.with(|cell| cell.replace(Some(id_type))));
    visit()
}

/// The [`std::any::type_name`] of the id type whose reference a visitor of
/// [`RewriteIdentities::visit_identity_references`] is being handed, where a
/// typed id hands it.
pub(crate) fn visited_id_type() -> Option<&'static str> {
    VISITED_ID_TYPE.with(std::cell::Cell::get)
}

/// Rewrite owned fields without projecting or reconstructing a serde value.
pub trait RewriteIdentities: Sized {
    /// False only when identity and text replacement cannot change this value.
    const HAS_IDENTITY_FIELDS: bool = true;

    /// Rewrite identity markers in an existing native wire value.
    /// Owners without a declared native wire layout refuse this operation.
    fn rewrite_native_value(
        _ctx: &DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut IdentityMap<'_>,
    ) -> Result<(), CodecError> {
        Err(CodecError::malformed(
            "typed owner has no native identity layout",
        ))
    }

    /// Visit borrowed typed references without projecting a value tree.
    /// Scalar owners expose no references.
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError>;

    /// Visit borrowed state-local member identities: the historical bodies,
    /// faces, edges, and vertices that `visit_identity_references` leaves out.
    /// Owners without such members expose none.
    fn visit_local_members(
        &self,
        _ctx: &DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        Ok(())
    }

    /// Return the rewritten value or the original resource refusal.
    fn rewrite_identities(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError>;
}

/// The caller's identity mapping. A trait object, so that all callers share
/// one instance of the typed rewrite walk.
type MapFn<'ctx> = dyn FnMut(&str) -> Result<String, CodecError> + 'ctx;

/// One injective identity mapping and its cache.
pub struct IdentityMap<'ctx> {
    map: Box<MapFn<'ctx>>,
    text_index: Option<&'ctx TextReplacements<'ctx>>,
    targets: HashMap<String, crate::ids::Identity>,
    occupied: HashSet<String>,
    refused: Option<String>,
    resource_refusal: Option<ResourceLimit>,
}

impl<'ctx> IdentityMap<'ctx> {
    /// Create the identity cache.
    pub fn new(map: impl FnMut(&str) -> Result<String, CodecError> + 'ctx) -> Self {
        Self {
            map: Box::new(map),
            text_index: None,
            targets: HashMap::new(),
            occupied: HashSet::new(),
            refused: None,
            resource_refusal: None,
        }
    }

    /// Also rewrite ordinary text that exactly names an owned source identity.
    /// Source keys must expose unique text in byte order.
    #[must_use]
    pub fn with_text_replacements(mut self, replacements: &'ctx TextReplacements<'ctx>) -> Self {
        self.text_index = Some(replacements);
        self
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
    pub fn finish(&self) -> Result<(), CodecError> {
        if let Some(limit) = self.resource_refusal {
            return Err(CodecError::ResourceLimit(limit));
        }

        match &self.refused {
            Some(message) => Err(CodecError::Malformed(message.to_owned())),
            None => Ok(()),
        }
    }

    /// Replace an identity once and refuse invalid or colliding targets.
    pub fn identity(&mut self, source: &str) -> Result<String, CodecError> {
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
        let target = match crate::ids::Identity::admit_text(target) {
            Ok(target) => target,
            Err(target) => {
                return self.refuse(format_args!(
                    "identity {source} rewrites to invalid identity {target:?}"
                ))
            }
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
    fn rewrite_native_value(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("rewrite native identity")?;

        let serde_json::Value::String(source) = value else {
            return Err(CodecError::malformed("native identity must be text"));
        };
        *source = map.identity(source)?;
        Ok(())
    }

    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(self.as_str())
    }
    fn rewrite_identities(
        self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        map.identity_value(self.as_str())
    }
}

macro_rules! rewrite_scalars {
    ($($type:ty),* $(,)?) => {$(
        impl RewriteIdentities for $type {
            const HAS_IDENTITY_FIELDS: bool = false;
    fn rewrite_native_value(_ctx: &DecodeContext<'_>, _value: &mut serde_json::Value, _map: &mut IdentityMap<'_>) -> Result<(), CodecError> {
        Ok(())
    }

            fn visit_identity_references(&self, _ctx: &DecodeContext<'_>, _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                Ok(())
            }
            fn rewrite_identities(self, _ctx: &DecodeContext<'_>, _map: &mut IdentityMap<'_>) -> Result<Self, CodecError> {
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
    fn rewrite_native_value(
        _ctx: &DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut IdentityMap<'_>,
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
    fn rewrite_identities(
        self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        map.text(self)
    }
}

impl<T: RewriteIdentities> RewriteIdentities for Option<T> {
    const HAS_IDENTITY_FIELDS: bool = T::HAS_IDENTITY_FIELDS;
    fn rewrite_native_value(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_>,
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
    fn visit_local_members(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed local members option")?;

        if let Some(value) = self {
            value.visit_local_members(ctx, visitor)?;
        }
        Ok(())
    }
    fn rewrite_identities(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("identity rewrite option")?;

        self.map(|value| value.rewrite_identities(ctx, map))
            .transpose()
    }
}

impl<T: RewriteIdentities> RewriteIdentities for Vec<T> {
    const HAS_IDENTITY_FIELDS: bool = T::HAS_IDENTITY_FIELDS;
    fn rewrite_native_value(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_>,
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
    fn visit_local_members(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed local members sequence")?;

        for value in self {
            value.visit_local_members(ctx, visitor)?;
        }
        Ok(())
    }
    fn rewrite_identities(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
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
    fn rewrite_native_value(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_>,
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
    fn visit_local_members(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed local members array")?;

        for value in self {
            value.visit_local_members(ctx, visitor)?;
        }
        Ok(())
    }
    fn rewrite_identities(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
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
    fn visit_local_members(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed local members map")?;

        for (key, value) in self {
            key.visit_local_members(ctx, visitor)?;
            value.visit_local_members(ctx, visitor)?;
        }
        Ok(())
    }
    fn rewrite_identities(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
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
    fn rewrite_native_value(
        _ctx: &DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut IdentityMap<'_>,
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
    fn rewrite_identities(
        self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        let text = map.text(self.into_string())?;

        Self::new(text).ok_or_else(|| CodecError::malformed("rewritten text must be nonblank"))
    }
}
rewrite_scalars!(std::num::NonZeroI64, std::num::NonZeroU32);

impl<T: RewriteIdentities> RewriteIdentities for Box<T> {
    const HAS_IDENTITY_FIELDS: bool = T::HAS_IDENTITY_FIELDS;
    fn rewrite_native_value(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_>,
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
    fn visit_local_members(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed local members box")?;

        self.as_ref().visit_local_members(ctx, visitor)?;
        Ok(())
    }
    fn rewrite_identities(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
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
            fn visit_local_members(&self, ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                let _depth = ctx.enter_nested("walk typed local members tuple")?;
                let ($($field,)*) = self;
                $($field.visit_local_members(ctx, visitor)?;)*
                Ok(())
            }
            fn rewrite_identities(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_>) -> Result<Self, CodecError> {
                let _depth = ctx.enter_nested("identity rewrite tuple")?;
                let ($($field,)*) = self;
                Ok(($($field.rewrite_identities(ctx, map)?,)*))
            }
        }
    };
}
rewrite_tuple!(A: first, B: second);
rewrite_tuple!(A: first, B: second, C: third);
