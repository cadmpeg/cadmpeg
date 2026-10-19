// SPDX-License-Identifier: Apache-2.0
//! Caller-accounted identity rewriting for typed fields.

pub mod typed;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Rewrite each marked identity while retaining ordinary owned text.
pub fn identities<T: typed::RewriteIdentities>(
    ctx: &DecodeContext<'_>,
    value: T,
    map: impl FnMut(&str) -> Result<String, CodecError>,
) -> Result<T, CodecError> {
    let mut identities = typed::IdentityMap::new(map);
    let rewritten = value.rewrite_identities(ctx, &mut identities);
    identities.finish()?;
    rewritten
}

#[cfg(test)]
mod tests;
