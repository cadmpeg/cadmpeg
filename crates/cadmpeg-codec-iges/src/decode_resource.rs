// SPDX-License-Identifier: Apache-2.0
//! Admission and fallible reservation for IGES decode collections.

use cadmpeg_core::decode::{refuse_local_limit, u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};

struct TextLength(usize);

impl Write for TextLength {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

pub(crate) fn format_retained(
    ctx: &DecodeContext<'_>,
    args: fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut length = TextLength(0);
    fmt::write(&mut length, args).map_err(|_| refuse_local_limit(operation, u64::MAX, 1))?;
    let count = u64_from_index(length.0);
    ctx.charge_retained(count, operation)?;
    let mut text = String::new();
    text.try_reserve_exact(length.0)
        .map_err(|_| refuse_local_limit(operation, count, count))?;
    fmt::write(&mut text, args)
        .map_err(|_| CodecError::Malformed("IGES formatted text cannot be rendered".into()))?;
    Ok(text)
}

pub(crate) fn copy_optional_identity<T: TryFrom<String>>(
    ctx: Option<&DecodeContext<'_>>,
    value: &str,
    operation: &'static str,
) -> Result<T, CodecError> {
    let text = match ctx {
        Some(ctx) => format_retained(ctx, format_args!("{value}"), operation)?,
        None => {
            let mut text = String::new();
            text.try_reserve_exact(value.len()).map_err(|_| refuse_local_limit(operation, u64_from_index(value.len()), u64_from_index(value.len())))?;
            text.push_str(value);
            text
        }
    };
    T::try_from(text).map_err(|_| CodecError::Malformed("IGES identity copy is invalid".into()))
}

pub(crate) fn push_formatted_note(
    ctx: &DecodeContext<'_>,
    notes: &mut Vec<String>,
    args: fmt::Arguments<'_>,
    slots_operation: &'static str,
    text_operation: &'static str,
) -> Result<(), CodecError> {
    reserve_vec_growth(ctx, notes, 1, slots_operation)?;
    notes.push(format_retained(ctx, args, text_operation)?);
    Ok(())
}

pub(crate) fn lossy_retained(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut remaining = bytes;
    let mut output_len = 0_usize;
    while let Err(error) = std::str::from_utf8(remaining) {
        output_len = output_len
            .checked_add(error.valid_up_to())
            .and_then(|length| length.checked_add('�'.len_utf8()))
            .ok_or_else(|| refuse_local_limit(operation, u64::MAX, 1))?;
        let invalid_len = error
            .error_len()
            .unwrap_or(remaining.len() - error.valid_up_to());
        remaining = &remaining[error.valid_up_to() + invalid_len..];
    }
    output_len = output_len
        .checked_add(remaining.len())
        .ok_or_else(|| refuse_local_limit(operation, u64::MAX, 1))?;
    let count = u64_from_index(output_len);
    ctx.charge_retained(count, operation)?;
    let mut text = String::new();
    text.try_reserve_exact(output_len)
        .map_err(|_| refuse_local_limit(operation, count, count))?;
    let mut remaining = bytes;
    loop {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                text.push_str(valid);
                break;
            }
            Err(error) => {
                let valid = std::str::from_utf8(&remaining[..error.valid_up_to()])
                    .map_err(|_| CodecError::Malformed("IGES UTF-8 prefix is invalid".into()))?;
                text.push_str(valid);
                text.push('�');
                let invalid_len = error
                    .error_len()
                    .unwrap_or(remaining.len() - error.valid_up_to());
                remaining = &remaining[error.valid_up_to() + invalid_len..];
            }
        }
    }
    Ok(text)
}

pub(crate) fn reserve_vec<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let requested = u64_from_index(count);
    ctx.charge_collection_items(requested, operation)?;
    reserve_admitted_vec(count, operation)
}

pub(crate) fn admit_optional_entities(
    ctx: Option<&DecodeContext<'_>>,
    count: u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_entities(count, operation)?;
    }
    Ok(())
}

pub(crate) fn reserve_optional_vec<T>(
    ctx: Option<&DecodeContext<'_>>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    match ctx {
        Some(ctx) => reserve_vec(ctx, count, operation),
        None => reserve_admitted_vec(count, operation),
    }
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

pub(crate) fn collect_result_vec<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
    mut value_at: impl FnMut(usize) -> Result<T, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let mut values = reserve_vec(ctx, count, operation)?;
    for index in 0..count {
        values.push(value_at(index)?);
    }
    Ok(values)
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
            refuse_local_limit(
                operation,
                u64_from_index(additional),
                u64_from_index(additional),
            )
        }),
    }
}

pub(crate) fn copy_optional_retained(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<Vec<u8>, CodecError> {
    match ctx {
        Some(ctx) => ctx.copy_retained(bytes, operation),
        None => {
            let mut copy = Vec::new();
            copy.try_reserve_exact(bytes.len()).map_err(|_| {
                refuse_local_limit(
                    operation,
                    u64_from_index(bytes.len()),
                    u64_from_index(bytes.len()),
                )
            })?;
            copy.extend_from_slice(bytes);
            Ok(copy)
        }
    }
}

pub(crate) fn insert_optional_btree_set<T: Ord>(
    ctx: Option<&DecodeContext<'_>>,
    values: &mut BTreeSet<T>,
    value: T,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if !values.contains(&value) {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, operation)?;
        }
    }
    Ok(values.insert(value))
}

pub(crate) fn insert_optional_btree_map<K: Ord, V>(
    ctx: Option<&DecodeContext<'_>>,
    values: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<Option<V>, CodecError> {
    if !values.contains_key(&key) {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, operation)?;
        }
    }
    Ok(values.insert(key, value))
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
    use super::{admit_optional_entities, collect_optional_vec, copy_optional_identity, format_retained, lossy_retained};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn lossy_retained_text_refuses_expanded_utf8_bytes_before_allocation() {
        let bytes = b"a\xffb";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 4;
        let (ctx, _) =
            DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("valid test fixture");
        let result = lossy_retained(&ctx, bytes, "iges lossy text test");
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.used == 0
                    && limit.additional == 5
                    && limit.operation == "iges lossy text test"
        ));

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("valid test fixture");
        assert_eq!(
            lossy_retained(&ctx, bytes, "iges lossy text test").expect("valid test fixture"),
            "a�b"
        );
    }

    #[test]
    fn formatted_retained_text_refuses_before_reservation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 4;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("valid test fixture");
        let result = format_retained(&ctx, format_args!("item{}", 7), "iges formatted test");
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.used == 0
                    && limit.additional == 5
                    && limit.operation == "iges formatted test"
        ));

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("valid test fixture");
        assert_eq!(
            format_retained(&ctx, format_args!("item{}", 7), "iges formatted test")
                .expect("valid test fixture"),
            "item7"
        );
    }

    #[test]
    fn copied_identity_refuses_retained_bytes_before_allocation() {
        let source = "test:model:curve#1";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(source.len() - 1).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let refused = copy_optional_identity::<cadmpeg_ir::ids::CurveId>(Some(&ctx), source, "iges identity copy test");
        assert!(matches!(refused,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "iges identity copy test"
                    && limit.used == 0
                    && limit.additional == u64::try_from(source.len()).unwrap()
        ));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let copied = copy_optional_identity::<cadmpeg_ir::ids::CurveId>(Some(&ctx), source, "iges identity copy test").unwrap();
        assert_eq!(copied.as_str(), source);
    }

    #[test]
    fn optional_collection_keeps_an_earlier_missing_value() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("valid test fixture");
        let result = collect_optional_vec(&ctx, [None, Some(7_u8)], "iges optional test")
            .expect("valid test fixture");
        assert_eq!(result, None);
    }

    #[test]
    fn optional_collection_refuses_when_the_second_value_needs_a_slot() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("valid test fixture");
        let result = collect_optional_vec(&ctx, [Some(3_u8), Some(7_u8)], "iges optional test");
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.used == 1
                    && limit.additional == 1
        ));

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("valid test fixture");
        let result = collect_optional_vec(&ctx, [Some(3_u8), Some(7_u8)], "iges optional test")
            .expect("valid test fixture");
        assert_eq!(result, Some(vec![3, 7]));
    }

    #[test]
    fn geometry_creation_refuses_the_next_entity_at_its_boundary() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        admit_optional_entities(Some(&ctx), 2, "iges geometry test").unwrap();
        let result = admit_optional_entities(Some(&ctx), 1, "iges geometry test");
        assert!(matches!(result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "iges geometry test"
                    && limit.used == 2
                    && limit.additional == 1
        ));
    }
}
