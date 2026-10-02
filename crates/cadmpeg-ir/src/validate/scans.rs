// SPDX-License-Identifier: Apache-2.0
//! Fallible input scans with work admission before each callback.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) fn all<T>(ctx: &DecodeContext<'_>, values: impl IntoIterator<Item = T>, mut predicate: impl FnMut(T) -> Result<bool, CodecError>) -> Result<bool, CodecError> {
    for value in values {
        ctx.charge_work(1, "validation predicate scan")?;
        if !predicate(value)? { return Ok(false); }
    }
    Ok(true)
}

pub(super) fn find_map<T, R>(ctx: &DecodeContext<'_>, values: impl IntoIterator<Item = T>, mut project: impl FnMut(T) -> Result<Option<R>, CodecError>) -> Result<Option<R>, CodecError> {
    for value in values {
        ctx.charge_work(1, "validation witness scan")?;
        if let Some(result) = project(value)? { return Ok(Some(result)); }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn validation_scans_refuse_before_first_and_later_callbacks() {
        for cap in [0, 1] {
            for witness in [false, true] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let visits = std::cell::Cell::new(0);
                let result = if witness {
                    super::find_map(&ctx, [1, 2], |_| { visits.set(visits.get() + 1); Ok(None::<u8>) }).map(|_| ())
                } else {
                    super::all(&ctx, [1, 2], |_| { visits.set(visits.get() + 1); Ok(true) }).map(|_| ())
                };
                let Err(CodecError::ResourceLimit(limit)) = result else { panic!("scan must refuse"); };
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(visits.get(), cap);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
            }
        }
    }

    #[test]
    fn validation_scans_preserve_short_circuit_and_callback_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 3;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(!super::all(&ctx, [1, 2], |_| Ok(false)).unwrap());
        assert_eq!(super::find_map(&ctx, [1, 2], |value| Ok(Some(value))).unwrap(), Some(1));
        let Err(CodecError::ResourceLimit(limit)) = super::all(&ctx, [1], |_| {
            ctx.charge_collection_items(1, "scan callback refusal")?;
            Ok(true)
        }) else { panic!("callback must refuse"); };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, "scan callback refusal");
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}
