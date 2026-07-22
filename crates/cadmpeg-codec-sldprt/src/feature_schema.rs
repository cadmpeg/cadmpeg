//! Per-feature schema declarations shared across history projection, neutral
//! synchronization, and design-loss auditing.
//!
//! A feature type's native enum tokens (for example the filled-surface
//! continuity order, or the trim-surface keep region) were historically spelled
//! out twice: once in the read path that maps native records to typed
//! [`cadmpeg_ir::FeatureDefinition`] values, and once in the write path that
//! rebuilds native records from them. The two `match` blocks had to stay in
//! lockstep by hand; drift between them is a silent round-trip bug.
//!
//! This module holds each such token map as a single static table. The read
//! path parses against it case-insensitively (preserving the historical
//! `to_ascii_lowercase` matching), and the write path formats through it to the
//! canonical native spelling. Adding or renaming a token is now a one-line edit
//! that both directions inherit.

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
fn parse_token<T: Copy>(table: &[(&'static str, T)], raw: &str) -> Option<T> {
    table
        .iter()
        .find(|(token, _)| raw.eq_ignore_ascii_case(token))
        .map(|(_, value)| *value)
}

/// Canonical native spelling for a typed token-table variant. Panics only if a
/// variant is absent from its table, which the tables above make unreachable.
fn format_token<T: PartialEq>(table: &[(&'static str, T)], value: &T) -> &'static str {
    table
        .iter()
        .find(|(_, candidate)| candidate == value)
        .map(|(token, _)| *token)
        .expect("token table covers every variant")
}

/// Parse a filled-surface continuity order from its native token.
pub(crate) fn parse_surface_continuity(raw: &str) -> Option<SurfaceContinuity> {
    parse_token(SURFACE_CONTINUITY_TOKENS, raw)
}

/// Canonical native token for a filled-surface continuity order.
pub(crate) fn surface_continuity_token(value: SurfaceContinuity) -> &'static str {
    format_token(SURFACE_CONTINUITY_TOKENS, &value)
}

/// Parse a trim-surface keep region from its native token.
pub(crate) fn parse_trim_region(raw: &str) -> Option<TrimRegion> {
    parse_token(TRIM_REGION_TOKENS, raw)
}

/// Canonical native token for a trim-surface keep region.
pub(crate) fn trim_region_token(value: TrimRegion) -> &'static str {
    format_token(TRIM_REGION_TOKENS, &value)
}

/// Parse a surface-extension method from its native token.
pub(crate) fn parse_surface_extension(raw: &str) -> Option<SurfaceExtension> {
    parse_token(SURFACE_EXTENSION_TOKENS, raw)
}

/// Canonical native token for a surface-extension method.
pub(crate) fn surface_extension_token(value: SurfaceExtension) -> &'static str {
    format_token(SURFACE_EXTENSION_TOKENS, &value)
}
