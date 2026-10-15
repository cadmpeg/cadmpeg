// SPDX-License-Identifier: Apache-2.0
//! Parser probes for the `cadmpeg-fuzz` targets.
//!
//! Context-taking probes return `Result<(), CodecError>`. They discard
//! successful parser values and propagate parser and resource errors.
//! Context-independent primitive probes return `()` and discard their results.
//! Every probe must accept arbitrary bytes without panicking.
#![doc(hidden)]

use crate::scalar::{decode, decode_in_lane, ScalarCache};

/// Exercise Creo datum plane decoders.
pub fn datum(
    _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let _probe = crate::datum::planes(data);
    let _probe = crate::datum::named_plane(data);
    Ok(())
}

/// Exercise Creo curve prototype extraction.
pub fn curve_prototypes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let _probe = crate::curve::prototypes(data);
    let _probe = crate::curve::expression_records_with_model_name(ctx, data, None)?;
    Ok(())
}

/// Exercise Creo surface namespace row extraction.
pub fn surface_rows(
    _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let _probe = crate::surface::rows(data);
    Ok(())
}

/// Exercise Creo PSB scalar decoding.
pub fn scalar(
    _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let cache = ScalarCache::from_section_checked(data);
    let mut offset = 0usize;
    while offset < data.len() {
        match decode_in_lane(data, offset, &cache) {
            Some((_, next)) if next > offset => offset = next,
            _ => break,
        }
    }
    let _probe = decode(data, 0);
    Ok(())
}

/// Exercise Creo compact integer decoding.
pub fn compact_int(data: &[u8]) {
    let _probe = crate::psb::compact_int(data, 0);
}

/// Exercise Creo PSB token stream parsing.
pub fn psb_tokens(data: &[u8]) {
    let _probe = crate::psb::tokens(data).count();
}

/// Exercise Creo short-form float decoding.
pub fn short_form_float(data: &[u8]) {
    // `is_short_form_float` reads one byte. Bytes that state no first byte are
    // the input this wrapper passes through: the predicate does not run for
    // them and nothing stands in for the byte they do not state. The decoder
    // below still sees the whole input.
    let _probe = data.first().copied().map(crate::psb::is_short_form_float);
    let _probe = crate::psb::short_form_float(data, 0);
}

/// Exercise Creo container scanning.
pub fn container_scan(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let _probe = crate::container::scan_bytes(ctx, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        crate::decode::with_service_context(|ctx| super::datum(ctx, &[]))
            .expect("datum fuzz wrapper");
        crate::decode::with_service_context(|ctx| super::curve_prototypes(ctx, &[]))
            .expect("curve fuzz wrapper");
        crate::decode::with_service_context(|ctx| super::surface_rows(ctx, &[]))
            .expect("surface row fuzz wrapper");
        crate::decode::with_service_context(|ctx| super::scalar(ctx, &[]))
            .expect("scalar fuzz wrapper");
        super::compact_int(&[]);
        super::psb_tokens(&[]);
        super::short_form_float(&[]);
        let _probe = crate::decode::with_service_context(|ctx| super::container_scan(ctx, &[]));
    }

    #[test]
    fn wrappers_accept_fixture() {
        let data = crate::test_support::build_prt("1.0", &[]);
        crate::decode::with_service_context(|ctx| super::datum(ctx, &data))
            .expect("datum fuzz wrapper");
        crate::decode::with_service_context(|ctx| super::curve_prototypes(ctx, &data))
            .expect("curve fuzz wrapper");
        crate::decode::with_service_context(|ctx| super::surface_rows(ctx, &data))
            .expect("surface row fuzz wrapper");
        crate::decode::with_service_context(|ctx| super::scalar(ctx, &data))
            .expect("scalar fuzz wrapper");
        super::compact_int(&data);
        super::psb_tokens(&data);
        super::short_form_float(&data);
        crate::decode::with_service_context(|ctx| super::container_scan(ctx, &data))
            .expect("container fuzz wrapper");
    }
}
