//! Per-feature schema declarations shared across history projection, neutral
//! synchronization, and design-loss auditing.
//!
//! Native enum tokens are single static tables. The read path parses them
//! case-insensitively; the write path formats the canonical spelling.

use cadmpeg_ir::features::{SurfaceContinuity, SurfaceExtension, TrimRegion};

/// Native spellings for [`SurfaceContinuity`], in write-canonical form. The
/// read path matched these case-insensitively (`contact`/`tangent`/`curvature`).
const SURFACE_CONTINUITY_TOKENS: &[(&str, SurfaceContinuity)] = &[
    ("Contact", SurfaceContinuity::Contact),
    ("Tangent", SurfaceContinuity::Tangent),
    ("Curvature", SurfaceContinuity::Curvature),
];

/// Native spellings for the trim-surface keep region (`inside`/`outside`).
const TRIM_REGION_TOKENS: &[(&str, TrimRegion)] = &[
    ("Inside", TrimRegion::Inside),
    ("Outside", TrimRegion::Outside),
];

/// Native spellings for the surface-extension method (`natural`/`linear`).
const SURFACE_EXTENSION_TOKENS: &[(&str, SurfaceExtension)] = &[
    ("Natural", SurfaceExtension::Natural),
    ("Linear", SurfaceExtension::Linear),
];

/// Parse a native token case-insensitively against a token table, returning the
/// typed variant or `None` for an unrecognized spelling.
fn parse_token<T: Clone>(ctx: &cadmpeg_core::decode::DecodeContext<'_>, table: &[(&'static str, T)], raw: &str) -> Result<Option<T>, cadmpeg_core::CodecError> {
    Ok(ctx.admit_iter(table, "scan SLDPRT feature schema token table")?
        .find(|(token, _)| raw.eq_ignore_ascii_case(token))
        .map(|(_, value)| value.clone()))
}

/// Parse a filled-surface continuity order from its native token.
pub(crate) fn parse_surface_continuity(ctx: &cadmpeg_core::decode::DecodeContext<'_>, raw: &str) -> Result<Option<SurfaceContinuity>, cadmpeg_core::CodecError> {
    parse_token(ctx, SURFACE_CONTINUITY_TOKENS, raw)
}

/// Canonical native token for a filled-surface continuity order.
pub(crate) fn surface_continuity_token(value: SurfaceContinuity) -> &'static str {
    match value {
        SurfaceContinuity::Contact => "Contact",
        SurfaceContinuity::Tangent => "Tangent",
        SurfaceContinuity::Curvature => "Curvature",
    }
}

/// Parse a trim-surface keep region from its native token.
pub(crate) fn parse_trim_region(ctx: &cadmpeg_core::decode::DecodeContext<'_>, raw: &str) -> Result<Option<TrimRegion>, cadmpeg_core::CodecError> {
    parse_token(ctx, TRIM_REGION_TOKENS, raw)
}

/// Canonical native token for a trim-surface keep region when representable.
pub(crate) fn trim_region_token(value: &TrimRegion) -> Option<&'static str> {
    TRIM_REGION_TOKENS
        .iter()
        .find(|(_, candidate)| candidate == value)
        .map(|(token, _)| *token)
}

/// Parse a surface-extension method from its native token.
pub(crate) fn parse_surface_extension(ctx: &cadmpeg_core::decode::DecodeContext<'_>, raw: &str) -> Result<Option<SurfaceExtension>, cadmpeg_core::CodecError> {
    parse_token(ctx, SURFACE_EXTENSION_TOKENS, raw)
}

/// Canonical native token for a surface-extension method, when the native
/// schema has a representation for the neutral value.
pub(crate) fn surface_extension_token(value: SurfaceExtension) -> Option<&'static str> {
    SURFACE_EXTENSION_TOKENS
        .iter()
        .find(|(_, candidate)| *candidate == value)
        .map(|(token, _)| *token)
}

#[cfg(test)]
mod tests {
    use super::surface_extension_token;
    use cadmpeg_ir::features::SurfaceExtension;

    #[test]
    fn surface_extension_token_rejects_unrepresented_method() {
        assert_eq!(
            surface_extension_token(SurfaceExtension::Perpendicular),
            None
        );
    }
}
