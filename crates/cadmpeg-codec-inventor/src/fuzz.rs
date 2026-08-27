// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Desktop salvage ceilings for fuzz wrappers.
///
/// `DecodePolicy::service()` tightens collection and entity limits 8–16× and
/// would silently shrink coverage. Wrappers must not copy that profile.
fn fuzz_policy() -> DecodePolicy {
    DecodePolicy::default()
}

fn with_source(data: &[u8], run: impl FnOnce(&DecodeContext<'_>, cadmpeg_core::decode::View<'_>)) {
    let arena = DecodeArena::new();
    let Ok((ctx, source)) = DecodeContext::from_root_bytes(data, &arena, &fuzz_policy()) else {
        return;
    };
    run(&ctx, source);
}

/// Exercises the schema-governed database, registry, and revision parsers.
pub fn database(data: &[u8]) {
    with_source(data, |ctx, _| {
        let _ = crate::database::parse_database(ctx, data);
        let _ = crate::database::parse_registry(ctx, data);
        let _ = crate::database::parse_revisions(ctx, data);
    });
}

/// Exercises the version-eight metadata envelope and its exact zlib member.
pub fn meta_stream(data: &[u8]) {
    with_source(data, |ctx, source| {
        crate::rse::fuzz_meta_stream(ctx, source);
    });
}

/// Exercises metadata tables and the corresponding bulk-record framing.
pub fn record_tables(data: &[u8]) {
    let split = data.len() / 2;
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    let Ok((ctx, source)) = DecodeContext::from_root_bytes(data, &arena, &policy) else {
        return;
    };
    let Some(metadata_source) = source.child(source.start(), source.start() + split) else {
        return;
    };
    let Ok(tables) = crate::records::parse_meta_tables(&ctx, metadata_source) else {
        return;
    };
    let Some(bulk_source) = source.child(source.start() + split, source.end()) else {
        return;
    };
    let version = data.first().copied().unwrap_or_default();
    let _ = crate::records::frame_bulk_records(&ctx, bulk_source, &tables, version);
}

/// Exercises the bulk envelope and its exact zlib member.
pub fn bulk_stream(data: &[u8]) {
    with_source(data, crate::rse::fuzz_bulk_stream);
}

/// Exercises OLE property-set section and typed-value parsing.
pub fn property_set(data: &[u8]) {
    with_source(data, |ctx, source| {
        let _ = crate::property_set::parse_property_set_stream(ctx, source);
    });
}

/// Exercises the Inventor length-framed Protein package envelope.
pub fn protein_envelope(data: &[u8]) {
    with_source(data, crate::protein::fuzz_parse_stream);
}
