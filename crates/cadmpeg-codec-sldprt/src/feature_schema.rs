//! Per-feature schema declarations shared across history projection, neutral
//! synchronization, and design-loss auditing.
//!
//! Each native enum token has one spelling table. The read path parses a token
//! case-insensitively; the write path formats the canonical spelling. A parse
//! compares against literal tokens only, so its work is bounded by the table.

use cadmpeg_ir::features::{SurfaceContinuity, SurfaceExtension, TrimRegion};

/// Native spellings for [`SurfaceContinuity`], in write-canonical form.
const SURFACE_CONTINUITY_TOKENS: &[(&str, SurfaceContinuity)] = &[
    ("Contact", SurfaceContinuity::Contact),
    ("Tangent", SurfaceContinuity::Tangent),
    ("Curvature", SurfaceContinuity::Curvature),
];

/// Native spellings for the surface-extension method.
const SURFACE_EXTENSION_TOKENS: &[(&str, SurfaceExtension)] = &[
    ("Natural", SurfaceExtension::Natural),
    ("Linear", SurfaceExtension::Linear),
];

/// Parse a native token case-insensitively against a token table, returning the
/// typed variant or `None` for an unrecognized spelling.
fn parse_token<T: Copy>(table: &[(&'static str, T)], raw: &str) -> Option<T> {
    table
        .iter()
        .find(|(token, _)| raw.eq_ignore_ascii_case(token))
        .map(|(_, value)| *value)
}

/// Parse a filled-surface continuity order from its native token.
pub(crate) fn parse_surface_continuity(raw: &str) -> Option<SurfaceContinuity> {
    parse_token(SURFACE_CONTINUITY_TOKENS, raw)
}

/// Canonical native token for a filled-surface continuity order.
pub(crate) fn surface_continuity_token(value: SurfaceContinuity) -> &'static str {
    match value {
        SurfaceContinuity::Contact => "Contact",
        SurfaceContinuity::Tangent => "Tangent",
        SurfaceContinuity::Curvature => "Curvature",
    }
}

/// Parse a trim-surface keep region (`Inside` or `Outside`) from its native token.
pub(crate) fn parse_trim_region(raw: &str) -> Option<TrimRegion> {
    if raw.eq_ignore_ascii_case("Inside") {
        Some(TrimRegion::Inside)
    } else if raw.eq_ignore_ascii_case("Outside") {
        Some(TrimRegion::Outside)
    } else {
        None
    }
}

/// Canonical native token for a trim-surface keep region when representable.
pub(crate) fn trim_region_token(value: &TrimRegion) -> Option<&'static str> {
    match value {
        TrimRegion::Inside => Some("Inside"),
        TrimRegion::Outside => Some("Outside"),
        TrimRegion::Unresolved | TrimRegion::Cells(_) => None,
    }
}

/// Parse a surface-extension method from its native token.
pub(crate) fn parse_surface_extension(raw: &str) -> Option<SurfaceExtension> {
    parse_token(SURFACE_EXTENSION_TOKENS, raw)
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
