// SPDX-License-Identifier: Apache-2.0
//! Typed accessors select identity markers in native wire fields.

use super::{IdentityMap, RewriteIdentities};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde_json::Value;

/// Select a native field with a compile-time accessor to its owning type.
pub fn rewrite_field<
    Owner,
    Field: RewriteIdentities,
    F: FnMut(&str) -> Result<String, CodecError>,
>(
    ctx: &DecodeContext<'_>,
    value: &mut Value,
    name: &str,
    map: &mut IdentityMap<'_, F>,
    _owner_field: fn(&Owner) -> Option<&Field>,
) -> Result<(), CodecError> {
    let Value::Object(fields) = value else {
        return Ok(());
    };

    if let Some(value) = fields.get_mut(name) {
        if !value.is_null() {
            Field::rewrite_native_value(ctx, value, map)?;
        }
    }
    Ok(())
}

/// Rewrite an owner-declared native identity field without touching display text.
pub(crate) fn rewrite_reference<F: FnMut(&str) -> Result<String, CodecError>>(
    value: &mut Value,
    name: &str,
    map: &mut IdentityMap<'_, F>,
) -> Result<(), CodecError> {
    let Value::Object(fields) = value else {
        return Ok(());
    };

    if let Some(value) = fields.get_mut(name) {
        match value {
            Value::String(reference) => *reference = map.identity(reference)?,
            Value::Array(references) => {
                for value in references {
                    if let Value::String(reference) = value {
                        *reference = map.identity(reference)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Native link fields preserve their string wire shape while owning identity semantics.
pub(crate) trait FullFidelityReference: Sized {
    fn rewrite<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError>;
}

impl FullFidelityReference for String {
    fn rewrite<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        map.identity(&self)
    }
}

impl<T: FullFidelityReference> FullFidelityReference for Option<T> {
    fn rewrite<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.map(|value| value.rewrite(map)).transpose()
    }
}

impl<T: FullFidelityReference> FullFidelityReference for Vec<T> {
    fn rewrite<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let mut references = Vec::new();
        for reference in self {
            references.push(reference.rewrite(map)?);
        }
        Ok(references)
    }
}
