// SPDX-License-Identifier: Apache-2.0
//! Fallible collection admission for FreeCAD decoding.

use cadmpeg_core::decode::{DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;
use std::fmt::{self, Write};

pub(crate) fn retained_format(
    ctx: &DecodeContext<'_>,
    arguments: fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    struct Count(usize);
    impl Write for Count {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
            Ok(())
        }
    }
    let mut count = Count(0);
    fmt::write(&mut count, arguments)
        .map_err(|_| retained_allocation_failed(ctx, u64::MAX, operation))?;
    ctx.charge_retained(count.0 as u64, operation)?;
    let mut output = String::new();
    output.try_reserve_exact(count.0)
        .map_err(|_| retained_allocation_failed(ctx, count.0 as u64, operation))?;
    output.write_fmt(arguments)
        .map_err(|_| CodecError::Malformed("FreeCAD diagnostic formatting failed".into()))?;
    Ok(output)
}

pub(crate) fn malformed_charged(
    ctx: &DecodeContext<'_>,
    arguments: fmt::Arguments<'_>,
    operation: &'static str,
) -> CodecError {
    match retained_format(ctx, arguments, operation) {
        Ok(message) => CodecError::Malformed(message),
        Err(refusal) => refusal,
    }
}

pub(crate) fn malformed_optional(
    ctx: Option<&DecodeContext<'_>>,
    arguments: fmt::Arguments<'_>,
    operation: &'static str,
) -> CodecError {
    match ctx {
        Some(ctx) => malformed_charged(ctx, arguments, operation),
        None => CodecError::malformed(arguments),
    }
}

pub(crate) fn named_entries_charged<V>(
    ctx: &DecodeContext<'_>,
    record: &str,
    entries: BTreeMap<String, V>,
    operation: &'static str,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, V>, CodecError> {
    use cadmpeg_core::text::NonBlankString;
    let mut keyed = BTreeMap::new();
    for (name, value) in entries {
        let Some(key) = NonBlankString::new(name) else {
            return Err(CodecError::Malformed(retained_join(
                ctx, &[record, " states a property with a blank key"], "", operation,
            )?));
        };
        ctx.charge_collection_items(1, operation)?;
        keyed.insert(key, value);
    }
    Ok(keyed)
}

pub(crate) fn insert_hash_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    items: &mut HashMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<Option<V>, CodecError> {
    if !items.contains_key(&key) {
        ctx.charge_collection_items(1, operation)?;
        items.try_reserve(1).map_err(|_| collection_allocation_failed(ctx, 1, operation))?;
    }
    Ok(items.insert(key, value))
}

pub(crate) fn collection_vec<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    reserved_vec(ctx, count, operation)
}

pub(crate) fn reserved_vec<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut items = Vec::new();
    items.try_reserve_exact(count).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::AllocationFailed,
            limit: ctx.policy().limits.max_collection_items,
            used: 0,
            additional: count as u64,
            operation,
        })
    })?;
    Ok(items)
}

pub(crate) fn optional_collection_vec<T>(
    ctx: &DecodeContext<'_>,
    present: bool,
    count: usize,
    operation: &'static str,
) -> Result<Option<Vec<T>>, CodecError> {
    if present {
        Ok(Some(collection_vec(ctx, count, operation)?))
    } else {
        Ok(None)
    }
}

pub(crate) fn decode_reserved_vec<T>(
    ctx: Option<&DecodeContext<'_>>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    match ctx {
        Some(ctx) => reserved_vec(ctx, count, operation),
        None => Ok(Vec::new()),
    }
}

pub(crate) fn reserve_charged_vec_items<T>(
    ctx: Option<&DecodeContext<'_>>,
    items: &mut Vec<T>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        items.try_reserve(count).map_err(|_| collection_allocation_failed(ctx, count as u64, operation))?;
    }
    Ok(())
}

pub(crate) fn reserve_vec_items<T>(
    ctx: &DecodeContext<'_>,
    items: &mut Vec<T>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    items.try_reserve(count).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::AllocationFailed,
            limit: ctx.policy().limits.max_collection_items,
            used: 0,
            additional: count as u64,
            operation,
        })
    })
}

pub(crate) fn retained_string(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    String::from_utf8(ctx.copy_retained(value.as_bytes(), operation)?)
        .map_err(|_| CodecError::Malformed("retained text lost UTF-8 encoding".into()))
}

pub(crate) fn copied_identity<I>(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<I, CodecError>
where
    I: TryFrom<String>,
    I::Error: std::fmt::Display,
{
    I::try_from(retained_string(ctx, value, operation)?).map_err(CodecError::malformed)
}

pub(crate) fn retained_strings(
    ctx: &DecodeContext<'_>,
    values: &[String],
    operation: &'static str,
) -> Result<Vec<String>, CodecError> {
    let mut copies = collection_vec(ctx, values.len(), operation)?;
    for value in values {
        copies.push(retained_string(ctx, value, operation)?);
    }
    Ok(copies)
}

pub(crate) fn copied_items<T: Copy>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut copies = collection_vec(ctx, values.len(), operation)?;
    copies.extend_from_slice(values);
    Ok(copies)
}

pub(crate) fn retained_suffix(
    ctx: &DecodeContext<'_>,
    value: &str,
    suffix: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut output = retained_string(ctx, value, operation)?;
    append_retained(ctx, &mut output, suffix, operation)?;
    Ok(output)
}

pub(crate) fn append_retained(
    ctx: &DecodeContext<'_>,
    output: &mut String,
    suffix: &str,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_retained(suffix.len() as u64, operation)?;
    output.try_reserve(suffix.len()).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::RetainedBytes,
            reason: ResourceFailure::AllocationFailed,
            limit: ctx.policy().limits.max_retained_bytes,
            used: 0,
            additional: suffix.len() as u64,
            operation,
        })
    })?;
    output.push_str(suffix);
    Ok(())
}

pub(crate) fn retained_join<S: AsRef<str>>(
    ctx: &DecodeContext<'_>,
    parts: &[S],
    separator: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut count = 0_usize;
    for part in parts {
        count = count.checked_add(part.as_ref().len())
            .ok_or_else(|| retained_allocation_failed(ctx, u64::MAX, operation))?;
    }
    let gaps = if parts.is_empty() { 0 } else { parts.len() - 1 };
    count = count.checked_add(separator.len().checked_mul(gaps)
        .ok_or_else(|| retained_allocation_failed(ctx, u64::MAX, operation))?)
        .ok_or_else(|| retained_allocation_failed(ctx, u64::MAX, operation))?;
    ctx.charge_retained(count as u64, operation)?;
    let mut output = String::new();
    output.try_reserve_exact(count)
        .map_err(|_| retained_allocation_failed(ctx, count as u64, operation))?;
    for (index, part) in parts.iter().enumerate() {
        if index != 0 {
            output.push_str(separator);
        }
        output.push_str(part.as_ref());
    }
    Ok(output)
}

pub(crate) fn materialized_bytes<'a>(
    ctx: &'a DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(Vec<u8>, ScopedReservation<'a>), CodecError> {
    let reservation = ctx.reserve_scoped(count as u64, operation)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(count).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::MaterializedBytes,
            reason: ResourceFailure::AllocationFailed,
            limit: ctx.policy().limits.max_materialized_bytes,
            used: 0,
            additional: count as u64,
            operation,
        })
    })?;
    Ok((bytes, reservation))
}

pub(crate) fn materialized_vec<'a, T>(
    ctx: &'a DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(Vec<T>, ScopedReservation<'a>), CodecError> {
    let bytes = count
        .checked_mul(std::mem::size_of::<T>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(|| materialized_allocation_failed(ctx, u64::MAX, operation))?;
    let reservation = ctx.reserve_scoped(bytes, operation)?;
    let mut items = Vec::new();
    items.try_reserve_exact(count)
        .map_err(|_| materialized_allocation_failed(ctx, bytes, operation))?;
    Ok((items, reservation))
}

fn materialized_allocation_failed(
    ctx: &DecodeContext<'_>,
    count: u64,
    operation: &'static str,
) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::MaterializedBytes,
        reason: ResourceFailure::AllocationFailed,
        limit: ctx.policy().limits.max_materialized_bytes,
        used: 0,
        additional: count,
        operation,
    })
}

pub(crate) fn retained_allocation_failed(
    ctx: &DecodeContext<'_>,
    count: u64,
    operation: &'static str,
) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::RetainedBytes,
        reason: ResourceFailure::AllocationFailed,
        limit: ctx.policy().limits.max_retained_bytes,
        used: 0,
        additional: count,
        operation,
    })
}

pub(crate) fn insert_hash_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    items: &mut HashSet<T>,
    value: T,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if items.contains(&value) {
        return Ok(false);
    }
    ctx.charge_collection_items(1, operation)?;
    items.try_reserve(1).map_err(|_| collection_allocation_failed(ctx, 1, operation))?;
    Ok(items.insert(value))
}

pub(crate) fn collection_allocation_failed(
    ctx: &DecodeContext<'_>,
    count: u64,
    operation: &'static str,
) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::CollectionItems,
        reason: ResourceFailure::AllocationFailed,
        limit: ctx.policy().limits.max_collection_items,
        used: 0,
        additional: count,
        operation,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn copied_identity_refuses_before_id_copy() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        let id = "fcstd:model:body#Payload:1";
        policy.limits.max_retained_bytes = id.len() as u64 - 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let result: Result<cadmpeg_ir::ids::BodyId, _> =
            super::copied_identity(&ctx, id, "test identity copy");
        assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "test identity copy"));
    }

    #[test]
    fn retained_format_preserves_text_and_refuses_before_allocation() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let text = "Shape & Surface";
        let expected = format!("invalid {text}: {}", 12);
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert_eq!(super::retained_format(&ctx, format_args!("invalid {text}: {}", 12),
            "test formatted diagnostic").expect("format fits"), expected);
        let mut policy = policy;
        policy.limits.max_retained_bytes = expected.len() as u64 - 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::retained_format(&ctx, format_args!("invalid {text}: {}", 12),
            "test formatted diagnostic"), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "test formatted diagnostic"));
    }

    #[test]
    fn charged_named_entries_refuse_at_collection_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let entries = std::collections::BTreeMap::from([("role".to_owned(), 1_u8)]);
        assert!(matches!(super::named_entries_charged(&ctx, "owner", entries, "test keyed entries"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "test keyed entries"));
    }

    #[test]
    fn charged_hash_index_refuses_at_caller_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let mut index = std::collections::HashMap::new();
        assert!(matches!(super::insert_hash_map(&ctx, &mut index, "key", 1_u8, "fcstd test index"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "fcstd test index"));
    }
}
