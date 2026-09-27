// SPDX-License-Identifier: Apache-2.0
//! Admission and fallible reservation for IGES decode collections.

use cadmpeg_core::decode::{refuse_local_limit, u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

pub(crate) fn reserve_vec<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let requested = u64_from_index(count);
    ctx.charge_collection_items(requested, operation)?;
    reserve_admitted_vec(count, operation)
}

pub(crate) fn reserve_admitted_vec<T>(
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| refuse_local_limit(operation, u64_from_index(count), u64_from_index(count)))?;
    Ok(values)
}

pub(crate) fn reserve_vec_growth<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let requested = u64_from_index(additional);
    ctx.charge_collection_items(requested, operation)?;
    values
        .try_reserve(additional)
        .map_err(|_| refuse_local_limit(operation, requested, requested))
}

pub(crate) fn reserve_optional_vec_growth<T>(
    ctx: Option<&DecodeContext<'_>>,
    values: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    match ctx {
        Some(ctx) => reserve_vec_growth(ctx, values, additional, operation),
        None => values.try_reserve(additional).map_err(|_| {
            refuse_local_limit(operation, u64_from_index(additional), u64_from_index(additional))
        }),
    }
}

pub(crate) fn collect_optional_vec<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = Option<T>>,
    operation: &'static str,
) -> Result<Option<Vec<T>>, CodecError> {
    let mut collected = Vec::new();
    for value in values {
        let Some(value) = value else {
            return Ok(None);
        };
        ctx.charge_collection_items(1, operation)?;
        collected
            .try_reserve(1)
            .map_err(|_| refuse_local_limit(operation, 1, 1))?;
        collected.push(value);
    }
    Ok(Some(collected))
}

#[cfg(test)]
mod tests {
    use super::collect_optional_vec;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn optional_collection_keeps_an_earlier_missing_value() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = collect_optional_vec(&ctx, [None, Some(7_u8)], "iges optional test").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn optional_collection_refuses_when_the_second_value_needs_a_slot() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = collect_optional_vec(&ctx, [Some(3_u8), Some(7_u8)], "iges optional test");
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.used == 1
                    && limit.additional == 1
        ));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let result =
            collect_optional_vec(&ctx, [Some(3_u8), Some(7_u8)], "iges optional test").unwrap();
        assert_eq!(result, Some(vec![3, 7]));
    }
}
