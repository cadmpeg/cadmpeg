// SPDX-License-Identifier: Apache-2.0
//! Fallible input scans with work admission before each callback.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) fn all<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    mut predicate: impl FnMut(T) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    for value in values {
        ctx.charge_work(1, "validation predicate scan")?;
        if !predicate(value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn find_map<T, R>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    mut project: impl FnMut(T) -> Result<Option<R>, CodecError>,
) -> Result<Option<R>, CodecError> {
    for value in values {
        ctx.charge_work(1, "validation witness scan")?;
        if let Some(result) = project(value)? {
            return Ok(Some(result));
        }
    }
    Ok(None)
}

pub(super) fn any<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    mut predicate: impl FnMut(T) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    for value in values {
        ctx.charge_work(1, "validation witness predicate scan")?;
        if predicate(value)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn find<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    mut predicate: impl FnMut(&T) -> Result<bool, CodecError>,
) -> Result<Option<T>, CodecError> {
    for value in values {
        ctx.charge_work(1, "validation matching record scan")?;
        if predicate(&value)? {
            return Ok(Some(value));
        }
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
                    super::find_map(&ctx, [1, 2], |_| {
                        visits.set(visits.get() + 1);
                        Ok(None::<u8>)
                    })
                    .map(|_| ())
                } else {
                    super::all(&ctx, [1, 2], |_| {
                        visits.set(visits.get() + 1);
                        Ok(true)
                    })
                    .map(|_| ())
                };
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("scan must refuse");
                };
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(visits.get(), cap);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
                );
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
        assert_eq!(
            super::find_map(&ctx, [1, 2], |value| Ok(Some(value))).unwrap(),
            Some(1)
        );
        let Err(CodecError::ResourceLimit(limit)) = super::all(&ctx, [1], |_| {
            ctx.charge_collection_items(1, "scan callback refusal")?;
            Ok(true)
        }) else {
            panic!("callback must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, "scan callback refusal");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    #[test]
    fn validation_witness_and_matching_scans_refuse_before_callbacks() {
        for cap in [0, 1] {
            for matching in [false, true] {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let visits = std::cell::Cell::new(0);
                let result = if matching {
                    super::find(&ctx, [1, 2], |_| {
                        visits.set(visits.get() + 1);
                        Ok(false)
                    })
                    .map(|_| ())
                } else {
                    super::any(&ctx, [1, 2], |_| {
                        visits.set(visits.get() + 1);
                        Ok(false)
                    })
                    .map(|_| ())
                };
                let Err(CodecError::ResourceLimit(original)) = result else {
                    panic!("scan must refuse");
                };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(visits.get(), cap);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
                );
            }
        }
    }

    #[test]
    fn validation_witness_and_matching_scans_keep_short_circuit_and_callback_refusal() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 3;
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(super::any(&ctx, [1, 2], |_| Ok(true)).unwrap());
        assert_eq!(
            super::find(&ctx, [1, 2], |value| Ok(*value == 1)).unwrap(),
            Some(1)
        );
        let Err(CodecError::ResourceLimit(original)) = super::find(&ctx, [1], |_| {
            ctx.charge_collection_items(1, "matching callback refusal")?;
            Ok(true)
        }) else {
            panic!("callback refusal must propagate");
        };
        assert_eq!(original.dimension, ResourceDimension::CollectionItems);
        assert_eq!(original.operation, "matching callback refusal");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
        );
    }
}
