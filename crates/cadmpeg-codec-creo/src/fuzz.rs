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
pub fn surface_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let _probe = crate::surface::rows(ctx, data)?;
    Ok(())
}

/// Exercise Creo PSB scalar decoding.
pub fn scalar(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let cache = ScalarCache::from_section_checked(ctx, data)?;
    let mut offsets = 0usize..data.len();
    while !offsets.is_empty() {
        let Some(offset) = ctx.next_charged(&mut offsets, "creo fuzz scalar traversal")? else {
            break;
        };
        match decode_in_lane(data, offset, &cache) {
            Some((_, next)) if next > offset => offsets.start = next,
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
pub fn psb_tokens(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
    let _probe = crate::psb::tokens(ctx, data)
        .try_fold(0usize, |count, token| token.map(|_| count + 1))?;
    Ok(())
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
    let scan_owned_storage = ctx.with_scoped_storage("creo container scan storage", || {
        crate::container::scan_bytes(ctx, data)
    })?;
    let scan_storage = scan_owned_storage.1;
    let scan = scan_owned_storage.0;
    drop(scan);
    drop(scan_storage);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        crate::decode::with_test_decode_ctx(|ctx| super::datum(ctx, &[]))
            .expect("datum fuzz wrapper");
        crate::decode::with_test_decode_ctx(|ctx| super::curve_prototypes(ctx, &[]))
            .expect("curve fuzz wrapper");
        crate::decode::with_test_decode_ctx(|ctx| super::surface_rows(ctx, &[]))
            .expect("surface row fuzz wrapper");
        crate::decode::with_test_decode_ctx(|ctx| super::scalar(ctx, &[]))
            .expect("scalar fuzz wrapper");
        super::compact_int(&[]);
        crate::decode::with_test_decode_ctx(|ctx| super::psb_tokens(ctx, &[]))
            .expect("PSB token fuzz wrapper");
        super::short_form_float(&[]);
        let _probe = crate::decode::with_test_decode_ctx(|ctx| super::container_scan(ctx, &[]));
    }

    #[test]
    fn wrappers_accept_fixture() {
        let data = crate::test_support::build_prt("1.0", &[]);
        crate::decode::with_test_decode_ctx(|ctx| super::datum(ctx, &data))
            .expect("datum fuzz wrapper");
        crate::decode::with_test_decode_ctx(|ctx| super::curve_prototypes(ctx, &data))
            .expect("curve fuzz wrapper");
        crate::decode::with_test_decode_ctx(|ctx| super::surface_rows(ctx, &data))
            .expect("surface row fuzz wrapper");
        crate::decode::with_test_decode_ctx(|ctx| super::scalar(ctx, &data))
            .expect("scalar fuzz wrapper");
        super::compact_int(&data);
        crate::decode::with_test_decode_ctx(|ctx| super::psb_tokens(ctx, &data))
            .expect("PSB token fuzz wrapper");
        super::short_form_float(&data);
        crate::decode::with_test_decode_ctx(|ctx| super::container_scan(ctx, &data))
            .expect("container fuzz wrapper");
    }

    #[test]
    fn scalar_wrapper_refuses_unadmitted_cache_image() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let data = [0x46, 0x08, 0, 0, 0, 0, 0, 0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&data, &arena, &policy).expect("scalar input admitted");
        let error = super::scalar(&ctx, &data).expect_err("cache image exceeds collection limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo scalar cache unique images")
        );
    }

    #[test]
    fn container_wrapper_propagates_caller_resource_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let data = crate::test_support::build_prt("1.0", &[("VisibGeom", Vec::new())]);
        crate::decode::with_test_decode_ctx(|ctx| super::container_scan(ctx, &data))
            .expect("service profile admits the container");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&data, &arena, &policy).expect("root input is admitted");
        let error = super::container_scan(&ctx, &data)
            .expect_err("the first scanned section needs a collection item");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.limit == refusal.used
                && refusal.additional > 0)
        );
    }
    #[test]
    fn scalar_probe_admits_each_visited_value() {
        let data = [0x00, 0x00];
        crate::test_support::assert_work_boundaries(&["creo fuzz scalar traversal"], |ctx| {
            super::scalar(ctx, &data)
        });
    }

    #[test]
    fn scalar_probe_empty_range_uses_zero_work() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        super::scalar(&ctx, &[]).expect("empty scalar probe does no work");
        assert_eq!(ctx.resource_refusal(), None);
    }

    #[test]
    fn psb_token_probe_preserves_caller_resource_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        use cadmpeg_core::CodecError;

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let CodecError::ResourceLimit(original) = ctx
            .charge_work(1, "caller refusal")
            .expect_err("work limit is zero")
        else {
            panic!("work refusal expected");
        };
        assert!(matches!(
            super::psb_tokens(&ctx, &[]),
            Err(CodecError::ResourceLimit(actual)) if actual == original
        ));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}
