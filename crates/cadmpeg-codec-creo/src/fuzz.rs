// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

use crate::scalar::{decode, decode_in_lane, ScalarCache};

/// Exercise Creo datum plane decoders.
pub fn datum(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let _probe = crate::datum::planes(ctx, data)?;
    let _probe = crate::datum::named_plane(ctx, data)?;
    Ok(())
}

/// Exercise Creo curve prototype extraction.
pub fn curve_prototypes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let _probe = crate::curve::prototypes(ctx, data)?;
    let _probe = crate::curve::expression_records_with_model_name(ctx, data, None)?;
    Ok(())
}

/// Exercise Creo surface namespace row extraction.
pub fn surface_rows(data: &[u8]) {
    let _probe = crate::surface::rows(data);
}

/// Exercise Creo PSB scalar decoding.
pub fn scalar(data: &[u8]) {
    let cache = ScalarCache::from_section(data);
    let mut offset = 0usize;
    while offset < data.len() {
        match decode_in_lane(data, offset, &cache) {
            Some((_, next)) if next > offset => offset = next,
            _ => break,
        }
    }
    let _probe = decode(data, 0);
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
pub fn container_scan(data: &[u8]) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let Ok((ctx, _)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    else {
        return;
    };
    match crate::container::scan_bytes(&ctx, data) {
        Ok(scan) => {
            let _probe = scan.framing.sections.len();
        }
        Err(refusal) => {
            let _probe = refusal.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        crate::decode::with_test_decode_ctx(|ctx| super::datum(ctx, &[]))
            .expect("datum fuzz wrapper");
        crate::decode::with_test_decode_ctx(|ctx| super::curve_prototypes(ctx, &[]))
            .expect("curve fuzz wrapper");
        super::surface_rows(&[]);
        super::scalar(&[]);
        super::compact_int(&[]);
        super::psb_tokens(&[]);
        super::short_form_float(&[]);
        super::container_scan(&[]);
    }

    #[test]
    fn wrappers_accept_fixture() {
        let data = crate::test_support::build_prt("1.0", &[]);
        crate::decode::with_test_decode_ctx(|ctx| super::datum(ctx, &data))
            .expect("datum fuzz wrapper");
        crate::decode::with_test_decode_ctx(|ctx| super::curve_prototypes(ctx, &data))
            .expect("curve fuzz wrapper");
        super::surface_rows(&data);
        super::scalar(&data);
        super::compact_int(&data);
        super::psb_tokens(&data);
        super::short_form_float(&data);
        super::container_scan(&data);
    }
}
