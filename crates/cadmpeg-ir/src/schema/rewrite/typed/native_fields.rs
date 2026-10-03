// SPDX-License-Identifier: Apache-2.0
//! Typed accessors select identity markers in native wire fields.

use super::{IdentityMap, RewriteIdentities};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
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
    let work = u64_from_index(fields.len())
        .checked_add(1)
        .and_then(|count| count.checked_mul(u64_from_index(name.len())))
        .ok_or_else(|| ctx.refuse_codec_limit("find typed native field", u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, "find typed native field")?;
    if let Some(value) = fields.get_mut(name) {
        if !value.is_null() {
            Field::rewrite_native_value(ctx, value, map)?;
        }
    }
    Ok(())
}
