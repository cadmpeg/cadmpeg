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
    if let Some(value) = ctx.find_map(fields.iter_mut(), |(key, value)| {
        Ok(ctx.equal_bytes(key.as_bytes(), name.as_bytes(), "compare typed native field")?.then_some(value))
    }, "find typed native field")? {
        if !value.is_null() {
            Field::rewrite_native_value(ctx, value, map)?;
        }
    }
    Ok(())
}
