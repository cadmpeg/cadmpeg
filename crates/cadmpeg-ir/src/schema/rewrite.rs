// SPDX-License-Identifier: Apache-2.0
//! Caller-accounted identity rewriting for typed fields.

pub mod typed;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Rewrite each marked identity while retaining ordinary owned text.
pub fn identities<T: typed::RewriteIdentities>(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    value: T,
    map: impl FnMut(&str) -> Result<String, CodecError>,
) -> Result<T, CodecError> {
    let mut identities = typed::IdentityMap::new(ctx, operation, map)?;
    let rewritten = value.rewrite_identities(ctx, &mut identities);
    identities.finish(ctx)?;
    rewritten
}

#[cfg(test)]
mod tests;
