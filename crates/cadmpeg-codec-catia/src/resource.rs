// SPDX-License-Identifier: Apache-2.0
//! Charged fallible growth for CATIA decode collections.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};
use std::fmt::Write;
use std::hash::Hash;

use cadmpeg_core::decode::{
    DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit, ScopedReservation,
};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::decode::{Coverage, CoverageKey};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::AnnotationBuilder;

use crate::loss::CatiaLossCode;

pub(crate) struct HexBytes<'a>(pub(crate) &'a [u8]);

impl std::fmt::Display for HexBytes<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

pub(crate) fn allocation_failed(
    used: usize,
    capacity: usize,
    additional: usize,
    operation: &'static str,
) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::Codec(operation),
        reason: ResourceFailure::AllocationFailed,
        limit: capacity as u64,
        used: used as u64,
        additional: additional as u64,
        operation,
    })
}

pub(crate) fn push<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    values.push(value);
    Ok(())
}

pub(crate) fn push_back<T>(
    ctx: &DecodeContext<'_>,
    values: &mut VecDeque<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    values.push_back(value);
    Ok(())
}

pub(crate) fn reserve_vec<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(additional as u64, operation)?;
    values
        .try_reserve(additional)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), additional, operation))
}

pub(crate) fn reserve_heap<T: Ord>(
    ctx: &DecodeContext<'_>,
    values: &mut BinaryHeap<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(additional as u64, operation)?;
    values
        .try_reserve(additional)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), additional, operation))
}

/// Reserve storage for items already charged as one aggregate admission.
pub(crate) fn reserve_admitted_vec<T>(
    values: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    values
        .try_reserve(additional)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), additional, operation))
}

pub(crate) fn copy_admitted_slice<T: Clone>(
    values: &[T],
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut copy = Vec::new();
    reserve_admitted_vec(&mut copy, values.len(), operation)?;
    copy.extend_from_slice(values);
    Ok(copy)
}

pub(crate) fn copy_admitted_rows<T: Clone>(
    values: &[T],
    row_len: usize,
    operation: &'static str,
) -> Result<Vec<Vec<T>>, CodecError> {
    let mut rows = Vec::new();
    let row_count = values.len().div_ceil(row_len);
    reserve_admitted_vec(&mut rows, row_count, operation)?;
    for chunk in values.chunks(row_len) {
        rows.push(copy_admitted_slice(chunk, operation)?);
    }
    Ok(rows)
}

pub(crate) fn reserve_admitted_map<K: Eq + Hash, V>(
    values: &mut HashMap<K, V>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    values
        .try_reserve(additional)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), additional, operation))
}

pub(crate) fn reserve_admitted_set<T: Eq + Hash>(
    values: &mut HashSet<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    values
        .try_reserve(additional)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), additional, operation))
}

pub(crate) fn copy_slice<T: Clone>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut copy = Vec::new();
    reserve_vec(ctx, &mut copy, values.len(), operation)?;
    copy.extend_from_slice(values);
    Ok(copy)
}

pub(crate) fn collect_vec<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut collected = Vec::new();
    for value in values {
        push(ctx, &mut collected, value, operation)?;
    }
    Ok(collected)
}

pub(crate) fn try_collect_vec<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = Result<T, CodecError>>,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut collected = Vec::new();
    for value in values {
        push(ctx, &mut collected, value?, operation)?;
    }
    Ok(collected)
}

pub(crate) fn collect_options<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = Option<T>>,
    operation: &'static str,
) -> Result<Option<Vec<T>>, CodecError> {
    let mut collected = Vec::new();
    for value in values {
        let Some(value) = value else { return Ok(None) };
        push(ctx, &mut collected, value, operation)?;
    }
    Ok(Some(collected))
}

pub(crate) fn collect_fallible_options<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = Result<Option<T>, cadmpeg_core::decode::ResourceLimit>>,
    operation: &'static str,
) -> Result<Option<Vec<T>>, CodecError> {
    let mut collected = Vec::new();
    for value in values {
        let Some(value) = value? else { return Ok(None) };
        push(ctx, &mut collected, value, operation)?;
    }
    Ok(Some(collected))
}

pub(crate) fn collect_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    operation: &'static str,
) -> Result<HashSet<T>, CodecError> {
    let mut collected = HashSet::new();
    for value in values {
        insert_set(ctx, &mut collected, value, operation)?;
    }
    Ok(collected)
}

pub(crate) fn collect_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = (K, V)>,
    operation: &'static str,
) -> Result<HashMap<K, V>, CodecError> {
    let mut collected = HashMap::new();
    for (key, value) in values {
        insert_map(ctx, &mut collected, key, value, operation)?;
    }
    Ok(collected)
}

pub(crate) fn collect_string_set<'a>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = &'a str>,
    operation: &'static str,
) -> Result<HashSet<String>, CodecError> {
    let mut collected = HashSet::new();
    for value in values {
        if !collected.contains(value) {
            let owned = copy_retained_str(ctx, value, operation)?;
            insert_set(ctx, &mut collected, owned, operation)?;
        }
    }
    Ok(collected)
}

#[cfg(test)]
mod collection_tests {
    #[test]
    fn report_index_collections_refuse_before_growth() {
        let set = crate::test_support::with_collection_limit(0, |ctx| {
            super::collect_set(ctx, [7u32], "catia_report_set_test")
        });
        assert!(
            matches!(set, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_set_test")
        );
        let map = crate::test_support::with_collection_limit(0, |ctx| {
            super::collect_map(ctx, [(7u32, 9u32)], "catia_report_map_test")
        });
        assert!(
            matches!(map, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_map_test")
        );
        let owned = crate::test_support::with_retained_limit(0, |ctx| {
            super::collect_string_set(ctx, ["entity"], "catia_report_owned_set_test")
        });
        assert!(
            matches!(owned, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_owned_set_test")
        );
        let service = crate::test_support::with_service_context(|ctx| {
            super::collect_string_set(ctx, ["entity"], "catia_report_owned_set_test")
        })
        .expect("service profile admits one report key");
        assert!(service.contains("entity"));
    }

    #[test]
    fn coverage_entry_refuses_collection_and_retained_limits() {
        let key = cadmpeg_ir::report::decode::CoverageKey::new("decoded_entities");
        let collection = crate::test_support::with_collection_limit(0, |ctx| {
            super::record_coverage(
                ctx,
                &mut cadmpeg_ir::report::decode::Coverage::default(),
                key,
                3,
                "catia_coverage_test",
            )
        });
        assert!(
            matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_coverage_test"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
        let retained = crate::test_support::with_retained_limit(0, |ctx| {
            super::record_coverage(
                ctx,
                &mut cadmpeg_ir::report::decode::Coverage::default(),
                key,
                3,
                "catia_coverage_test",
            )
        });
        assert!(
            matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_coverage_test"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
        let coverage = crate::test_support::with_service_context(|ctx| {
            let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
            super::record_coverage(ctx, &mut coverage, key, 3, "catia_coverage_test")
                .expect("service budget admits coverage entry");
            coverage
        });
        assert_eq!(coverage.get("decoded_entities"), Some(&3));
    }

    #[test]
    fn report_loss_refuses_retained_and_collection_limits() {
        let code = crate::loss::CatiaLossCode::HistoryModelingScopeUnresolved;
        let retained = crate::test_support::with_retained_limit(0, |ctx| {
            super::push_loss(
                ctx,
                &mut Vec::new(),
                code,
                format_args!("one unresolved graph"),
                "catia_report_loss_test",
            )
        });
        assert!(
            matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_loss_test"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
        let collection = crate::test_support::with_collection_limit(0, |ctx| {
            super::push_loss(
                ctx,
                &mut Vec::new(),
                code,
                format_args!("one unresolved graph"),
                "catia_report_loss_test",
            )
        });
        assert!(
            matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_report_loss_test"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
        let notes = crate::test_support::with_service_context(|ctx| {
            let mut notes = Vec::new();
            super::push_loss(
                ctx,
                &mut notes,
                code,
                format_args!("one unresolved graph"),
                "catia_report_loss_test",
            )
            .expect("service profile admits report loss");
            notes
        });
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].code, code.kind());
    }
}

pub(crate) fn copy_retained_slice<T: Clone>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let Some(bytes) = values
        .len()
        .checked_mul(std::mem::size_of::<T>().max(1))
        .and_then(|bytes| u64::try_from(bytes).ok())
    else {
        return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, operation)?;
    copy_slice(ctx, values, operation)
}

pub(crate) fn copy_id<T>(
    ctx: &DecodeContext<'_>,
    value: &str,
    construct: impl FnOnce(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    operation: &'static str,
) -> Result<T, CodecError> {
    construct(copy_retained_str(ctx, value, operation)?).map_err(CodecError::malformed)
}

pub(crate) fn copy_retained_str(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let bytes = u64::try_from(value.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_retained(bytes, operation)?;
    let mut text = String::new();
    text.try_reserve(value.len())
        .map_err(|_| allocation_failed(0, text.capacity(), value.len(), operation))?;
    text.push_str(value);
    Ok(text)
}

pub(crate) fn record_coverage(
    ctx: &DecodeContext<'_>,
    coverage: &mut Coverage,
    key: CoverageKey,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !coverage.contains_key(key.as_str()) {
        ctx.charge_collection_items(1, operation)?;
    }
    let name = copy_retained_str(ctx, key.as_str(), operation)?;
    coverage
        .record_owned(key, name, count)
        .map_err(CodecError::malformed)
}

pub(crate) fn source_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let key = format_retained(ctx, key, operation)?;
    let key = NonBlankString::new(key)
        .ok_or_else(|| CodecError::malformed("CATIA source attribute key is blank"))?;
    let value = format_retained(ctx, value, operation)?;
    insert_btree_map(ctx, attributes, key, value, operation)?;
    Ok(())
}

pub(crate) fn string_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    key: &str,
    value: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let key = copy_retained_str(ctx, key, operation)?;
    let value = format_retained(ctx, value, operation)?;
    insert_btree_map(ctx, attributes, key, value, operation)?;
    Ok(())
}

pub(crate) fn push_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    code: CatiaLossCode,
    args: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let message = format_retained(ctx, args, operation)?;
    let note = code.note_charged(ctx, message, operation)?;
    push(ctx, losses, note, operation)
}

pub(crate) fn derived_annotation(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    field: &str,
    operation: &'static str,
) -> Result<(), CodecError> {
    let id = format_retained(ctx, format_args!("{id}"), operation)?;
    let (outer, inner) = annotations.derived_field_admission(&id, field);
    if outer {
        ctx.charge_collection_items(1, operation)?;
    }
    if inner {
        ctx.charge_collection_items(1, operation)?;
    }
    let field = copy_retained_str(ctx, field, operation)?;
    annotations
        .field_exactness_owned(id, field, cadmpeg_ir::Exactness::Derived)
        .map_err(CodecError::malformed)?;
    Ok(())
}

#[cfg(test)]
mod derived_annotation_tests {
    #[test]
    fn derived_field_refuses_retained_and_collection_limits() {
        let retained = crate::test_support::with_retained_limit(0, |ctx| {
            super::derived_annotation(
                ctx,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                "catia:test:vertex#0",
                "point",
                "catia_annotation_field",
            )
        });
        assert!(
            matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_annotation_field")
        );
        let collection = crate::test_support::with_collection_limit(0, |ctx| {
            super::derived_annotation(
                ctx,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                "catia:test:vertex#0",
                "point",
                "catia_annotation_field",
            )
        });
        assert!(
            matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_annotation_field")
        );
        let annotations = crate::test_support::with_service_context(|ctx| {
            let mut builder = cadmpeg_ir::AnnotationBuilder::new();
            super::derived_annotation(
                ctx,
                &mut builder,
                "catia:test:vertex#0",
                "point",
                "catia_annotation_field",
            )
            .expect("service profile admits derived field");
            builder.build()
        });
        let fields = annotations.exactness()["catia:test:vertex#0"].fields();
        assert_eq!(fields.get("point"), Some(&cadmpeg_ir::Exactness::Derived));
    }
}

pub(crate) fn format_retained(
    ctx: &DecodeContext<'_>,
    args: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    let length = formatted_length(ctx, args, operation)?;
    let bytes =
        u64::try_from(length).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_retained(bytes, operation)?;
    let mut text = String::new();
    text.try_reserve(length)
        .map_err(|_| allocation_failed(0, text.capacity(), length, operation))?;
    std::fmt::write(&mut text, args).map_err(CodecError::malformed)?;
    Ok(text)
}

pub(crate) fn format_scoped<'a>(
    ctx: &'a DecodeContext<'_>,
    args: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(String, ScopedReservation<'a>), CodecError> {
    let length = formatted_length(ctx, args, operation)?;
    let bytes =
        u64::try_from(length).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    let reservation = ctx.reserve_scoped(bytes, operation)?;
    let mut text = String::new();
    text.try_reserve(length)
        .map_err(|_| allocation_failed(0, text.capacity(), length, operation))?;
    std::fmt::write(&mut text, args).map_err(CodecError::malformed)?;
    Ok((text, reservation))
}

fn formatted_length(
    ctx: &DecodeContext<'_>,
    args: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<usize, CodecError> {
    struct ByteCount(Option<usize>);
    impl Write for ByteCount {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            self.0 = self.0.and_then(|bytes| bytes.checked_add(value.len()));
            if self.0.is_some() {
                Ok(())
            } else {
                Err(std::fmt::Error)
            }
        }
    }
    let mut count = ByteCount(Some(0));
    if std::fmt::write(&mut count, args).is_err() {
        return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
    }
    let Some(length) = count.0 else {
        return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
    };
    Ok(length)
}

#[cfg(test)]
mod scoped_format_tests {
    #[test]
    fn scoped_format_refuses_before_temporary_string_growth() {
        let refused = crate::test_support::with_materialized_limit(0, |ctx| {
            super::format_scoped(ctx, format_args!("edge {}", 42), "catia_test_scoped_format")
                .map(|(text, _reservation)| text)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_test_scoped_format")
        );
        let text = crate::test_support::with_service_context(|ctx| {
            super::format_scoped(ctx, format_args!("edge {}", 42), "catia_test_scoped_format")
                .map(|(text, _reservation)| text)
        })
        .expect("service budget admits temporary text");
        assert_eq!(text, "edge 42");
    }
}

pub(crate) fn extend_retained_bytes(
    ctx: &DecodeContext<'_>,
    target: &mut Vec<u8>,
    source: &[u8],
    operation: &'static str,
) -> Result<(), CodecError> {
    let count = u64::try_from(source.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_collection_items(count, operation)?;
    ctx.charge_retained(count, operation)?;
    target
        .try_reserve(source.len())
        .map_err(|_| allocation_failed(target.len(), target.capacity(), source.len(), operation))?;
    target.extend_from_slice(source);
    Ok(())
}

pub(crate) fn format_usize_id(
    ctx: &DecodeContext<'_>,
    prefix: &'static str,
    value: usize,
    minimum_digits: usize,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut number = value;
    let mut digits = 1usize;
    while number >= 10 {
        number /= 10;
        digits += 1;
    }
    let length = prefix
        .len()
        .checked_add(digits.max(minimum_digits))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    let bytes =
        u64::try_from(length).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_retained(bytes, operation)?;
    let mut id = String::new();
    id.try_reserve(length)
        .map_err(|_| allocation_failed(0, id.capacity(), length, operation))?;
    id.push_str(prefix);
    write!(&mut id, "{value:0minimum_digits$}").map_err(CodecError::malformed)?;
    Ok(id)
}

pub(crate) fn compose_index_id<T>(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::ids::IdentityNamespace,
    index: usize,
    construct: impl FnOnce(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    operation: &'static str,
) -> Result<T, CodecError> {
    let id = format_retained(
        ctx,
        format_args!(
            "{}:{}:{}#{index}",
            namespace.format(),
            namespace.scope(),
            namespace.kind()
        ),
        operation,
    )?;
    construct(id).map_err(CodecError::malformed)
}

pub(crate) fn compose_u32_id<T>(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::ids::IdentityNamespace,
    value: u32,
    construct: impl FnOnce(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    operation: &'static str,
) -> Result<T, CodecError> {
    let index = usize::try_from(value)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    compose_index_id(ctx, namespace, index, construct, operation)
}

#[cfg(test)]
mod id_format_tests {
    #[test]
    fn b5_numeric_identity_refuses_retained_limit_and_preserves_spelling() {
        use cadmpeg_ir::ids::CurveId;

        let namespace = cadmpeg_ir::identity_namespace!("catia", "b5", "profile");
        let limited = crate::test_support::with_retained_limit(1, |ctx| {
            super::compose_u32_id(ctx, &namespace, 42, CurveId::mint, "catia_b5_profile_id")
        });
        assert!(matches!(limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_b5_profile_id"));
        let id = crate::test_support::with_service_context(|ctx| {
            super::compose_u32_id(ctx, &namespace, 42, CurveId::mint, "catia_b5_profile_id")
        })
        .expect("service budget admits the identity");
        assert_eq!(id.as_str(), "catia:b5:profile#42");
    }

    #[test]
    fn native_owner_id_format_refuses_retained_limit() {
        let limited = crate::test_support::with_retained_limit(39, |ctx| {
            super::format_usize_id(
                ctx,
                "catia:consolidated:owner-packet#",
                7,
                10,
                "catia_native_owner_packet_id",
            )
        });
        assert!(matches!(
            limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        let id = crate::test_support::with_service_context(|ctx| {
            super::format_usize_id(
                ctx,
                "catia:consolidated:owner-packet#",
                7,
                10,
                "catia_native_owner_packet_id",
            )
        })
        .expect("service retained budget");
        assert_eq!(id, "catia:consolidated:owner-packet#0000000007");
    }
}

pub(crate) fn copy_retained_rows<T: Clone>(
    ctx: &DecodeContext<'_>,
    rows: &[Vec<T>],
    row_operation: &'static str,
    item_operation: &'static str,
) -> Result<Vec<Vec<T>>, CodecError> {
    let Some(bytes) = rows
        .len()
        .checked_mul(std::mem::size_of::<Vec<T>>())
        .and_then(|bytes| u64::try_from(bytes).ok())
    else {
        return Err(ctx.refuse_codec_limit(row_operation, u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, row_operation)?;
    let mut copy = Vec::new();
    reserve_vec(ctx, &mut copy, rows.len(), row_operation)?;
    for row in rows {
        copy.push(copy_retained_slice(ctx, row, item_operation)?);
    }
    Ok(copy)
}

pub(crate) fn copy_retained_set<T: Copy + Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: &HashSet<T>,
    operation: &'static str,
) -> Result<HashSet<T>, CodecError> {
    let Some(bytes) = values
        .len()
        .checked_mul(std::mem::size_of::<T>().max(1))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<HashSet<T>>()))
        .and_then(|bytes| u64::try_from(bytes).ok())
    else {
        return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, operation)?;
    let mut copy = HashSet::new();
    reserve_set(ctx, &mut copy, values.len(), operation)?;
    copy.extend(values.iter().copied());
    Ok(copy)
}

pub(crate) fn copy_knot_vector(
    ctx: &DecodeContext<'_>,
    knots: &cadmpeg_ir::geometry::nurbs::KnotVector,
    operation: &'static str,
) -> Result<cadmpeg_ir::geometry::nurbs::KnotVector, CodecError> {
    let count = u64::try_from(knots.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_collection_items(count, operation)?;
    let bytes = count
        .checked_mul(std::mem::size_of::<f64>() as u64)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_retained(bytes, operation)?;
    knots
        .try_clone()
        .map_err(|_| allocation_failed(0, 0, knots.len(), operation))
}

pub(crate) fn copy_nurbs_curve(
    ctx: &DecodeContext<'_>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    operation: &'static str,
) -> Result<cadmpeg_ir::geometry::nurbs::NurbsCurve, CodecError> {
    curve.try_clone_for_decode(ctx, operation)
}

pub(crate) fn copy_pcurve_geometry(
    ctx: &DecodeContext<'_>,
    geometry: &cadmpeg_ir::geometry::pcurve::PcurveGeometry,
    operation: &'static str,
) -> Result<cadmpeg_ir::geometry::pcurve::PcurveGeometry, CodecError> {
    use cadmpeg_ir::geometry::pcurve::{
        OffsetPcurve, PcurveGeometry, PcurveNurbs, PcurveNurbsPoles, PlacedPcurve, PolarNurbsPoles,
        PolarPcurveNurbs, TrimmedPcurve,
    };

    Ok(match geometry {
        PcurveGeometry::Nurbs { nurbs } => {
            let knots = copy_knot_vector(ctx, nurbs.knots(), operation)?;
            let poles = match nurbs.pole_rows() {
                PcurveNurbsPoles::Polynomial { points } => PcurveNurbsPoles::Polynomial {
                    points: copy_retained_slice(ctx, points, operation)?,
                },
                PcurveNurbsPoles::Rational { points } => PcurveNurbsPoles::Rational {
                    points: copy_retained_slice(ctx, points, operation)?,
                },
            };
            PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::from_admitted_rows(
                    nurbs.degree(),
                    knots,
                    poles,
                    nurbs.periodic(),
                )
                .map_err(CodecError::malformed)?,
            }
        }
        PcurveGeometry::PolarNurbs { nurbs } => {
            let knots = copy_knot_vector(ctx, nurbs.knots(), operation)?;
            let poles = match nurbs.pole_rows() {
                PolarNurbsPoles::Polynomial { poles } => PolarNurbsPoles::Polynomial {
                    poles: copy_retained_slice(ctx, poles, operation)?,
                },
                PolarNurbsPoles::Rational { poles } => PolarNurbsPoles::Rational {
                    poles: copy_retained_slice(ctx, poles, operation)?,
                },
            };
            PcurveGeometry::PolarNurbs {
                nurbs: PolarPcurveNurbs::from_admitted_parts(
                    nurbs.degree(),
                    knots,
                    poles,
                    nurbs.periodic(),
                )
                .map_err(CodecError::malformed)?,
            }
        }
        PcurveGeometry::Transformed(placed) => {
            let _depth = ctx.enter_nested(operation)?;
            ctx.charge_retained(size_of::<PcurveGeometry>() as u64, operation)?;
            let basis = Box::new(copy_pcurve_geometry(ctx, placed.basis(), operation)?);
            PcurveGeometry::Transformed(
                PlacedPcurve::try_new(basis, *placed.transform()).map_err(CodecError::malformed)?,
            )
        }
        PcurveGeometry::Trimmed(trimmed) => {
            let _depth = ctx.enter_nested(operation)?;
            ctx.charge_retained(size_of::<PcurveGeometry>() as u64, operation)?;
            let basis = Box::new(copy_pcurve_geometry(ctx, trimmed.basis(), operation)?);
            PcurveGeometry::Trimmed(
                TrimmedPcurve::try_new(
                    trimmed.parameter_range().endpoints(),
                    trimmed.same_sense(),
                    basis,
                )
                .map_err(CodecError::malformed)?,
            )
        }
        PcurveGeometry::Offset(offset) => {
            let _depth = ctx.enter_nested(operation)?;
            ctx.charge_retained(size_of::<PcurveGeometry>() as u64, operation)?;
            let basis = Box::new(copy_pcurve_geometry(ctx, offset.basis(), operation)?);
            PcurveGeometry::Offset(
                OffsetPcurve::from_finite_parts(offset.distance(), basis)
                    .map_err(CodecError::malformed)?,
            )
        }
        other => other.clone(),
    })
}

pub(crate) fn copy_intcurve_support_context(
    ctx: &DecodeContext<'_>,
    context: &cadmpeg_ir::geometry::IntcurveSupportContext,
    operation: &'static str,
) -> Result<cadmpeg_ir::geometry::IntcurveSupportContext, CodecError> {
    use cadmpeg_ir::geometry::{IntcurveSupportContext, IntcurveSupportSide, SupportPcurve};
    use cadmpeg_ir::ids::SurfaceId;

    let [left, right] = context.sides().each_ref().map(|side| {
        Ok::<_, CodecError>(IntcurveSupportSide {
            surface: side
                .surface
                .as_ref()
                .map(|id| copy_id(ctx, id.as_str(), SurfaceId::mint, operation))
                .transpose()?,
            pcurve: side
                .pcurve
                .as_ref()
                .map(|pcurve| {
                    Ok::<_, CodecError>(SupportPcurve::new(
                        copy_pcurve_geometry(ctx, &pcurve.geometry, operation)?,
                        pcurve.parameter_range,
                    ))
                })
                .transpose()?,
        })
    });
    let [first, second, third] = context
        .discontinuities()
        .each_ref()
        .map(|lane| copy_retained_slice(ctx, lane, operation));
    IntcurveSupportContext::from_parts(
        [left?, right?],
        context.parameter_range(),
        [first?, second?, third?],
    )
    .map_err(CodecError::malformed)
}

#[cfg(test)]
mod pcurve_copy_tests {
    use super::copy_pcurve_geometry;
    use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use cadmpeg_ir::math::Point2;

    #[test]
    fn copied_nurbs_pcurve_refuses_knot_and_pole_collection_limit() {
        let geometry = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
                None,
                false,
            )
            .expect("valid pcurve fixture"),
        };
        let refused = crate::test_support::with_collection_limit(5, |ctx| {
            copy_pcurve_geometry(ctx, &geometry, "catia_test_pcurve_copy")
        });
        assert!(matches!(
            refused,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        let copy = crate::test_support::with_service_context(|ctx| {
            copy_pcurve_geometry(ctx, &geometry, "catia_test_pcurve_copy")
        })
        .expect("service budget");
        assert_eq!(copy, geometry);
    }
}

pub(crate) fn copy_nurbs_surface(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    operation: &'static str,
) -> Result<cadmpeg_ir::geometry::nurbs::NurbsSurface, CodecError> {
    surface.try_clone_for_decode(ctx, operation)
}

#[cfg(test)]
mod nurbs_copy_tests {
    use super::{copy_nurbs_curve, copy_nurbs_surface};
    use cadmpeg_ir::geometry::nurbs::{
        NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
    };
    use cadmpeg_ir::math::Point3;

    #[test]
    fn nurbs_curve_copy_refuses_before_knot_and_pole_lanes() {
        use cadmpeg_core::CodecError;

        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("valid line NURBS");
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(
                copy_nurbs_curve(ctx, &curve, "catia_nurbs_curve_copy").expect("service budget"),
                curve
            );
        });
        assert!(matches!(
            crate::test_support::with_collection_limit(0, |ctx| copy_nurbs_curve(ctx, &curve, "catia_nurbs_curve_copy")),
            Err(CodecError::ResourceLimit(error)) if error.operation == "catia_nurbs_curve_copy"
        ));
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits retained limit");
        assert!(matches!(
            copy_nurbs_curve(&ctx, &curve, "catia_nurbs_curve_copy"),
            Err(CodecError::ResourceLimit(error)) if error.operation == "catia_nurbs_curve_copy"
        ));
    }

    #[test]
    fn nurbs_surface_copy_refuses_before_nested_lanes() {
        let surface = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                ],
                None,
            ),
            false,
        )
        .expect("valid surface");
        assert_eq!(
            crate::test_support::with_service_context(|ctx| copy_nurbs_surface(
                ctx,
                &surface,
                "catia_nurbs_surface_copy"
            ))
            .expect("service resource budget"),
            surface
        );
        assert!(matches!(
            crate::test_support::with_collection_limit(0, |ctx| copy_nurbs_surface(ctx, &surface, "catia_nurbs_surface_copy")),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_nurbs_surface_copy"
        ));
    }
}

pub(crate) fn reserve_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<T>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), count, operation))
}

pub(crate) fn reserve_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), count, operation))
}

pub(crate) fn insert_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<T>,
    value: T,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if values.contains(&value) {
        return Ok(false);
    }
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    Ok(values.insert(value))
}

pub(crate) fn insert_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<Option<V>, CodecError> {
    if !values.contains_key(&key) {
        ctx.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    }
    Ok(values.insert(key, value))
}

pub(crate) fn admit_map_entry<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    key: &K,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !values.contains_key(key) {
        ctx.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    }
    Ok(())
}

pub(crate) fn admit_btree_entry<K: Ord, V>(
    ctx: &DecodeContext<'_>,
    values: &BTreeMap<K, V>,
    key: &K,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !values.contains_key(key) {
        ctx.charge_collection_items(1, operation)?;
    }
    Ok(())
}

pub(crate) fn insert_btree_map<K: Ord, V>(
    ctx: &DecodeContext<'_>,
    values: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<Option<V>, CodecError> {
    admit_btree_entry(ctx, values, &key, operation)?;
    Ok(values.insert(key, value))
}

pub(crate) fn insert_btree_set<T: Ord>(
    ctx: &DecodeContext<'_>,
    values: &mut BTreeSet<T>,
    value: T,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if values.contains(&value) {
        return Ok(false);
    }
    ctx.charge_collection_items(1, operation)?;
    Ok(values.insert(value))
}

fn temporary_bytes<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<u64, CodecError> {
    let item_bytes = std::mem::size_of::<T>().max(1);
    let Some(bytes) = item_bytes
        .checked_add(32)
        .and_then(|size| size.checked_mul(count))
        .and_then(|size| u64::try_from(size).ok())
    else {
        return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
    };
    Ok(bytes)
}

pub(crate) fn temporary_vec<'a, T>(
    ctx: &'a DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(Vec<T>, ScopedReservation<'a>), CodecError> {
    let bytes = count
        .checked_mul(std::mem::size_of::<T>().max(1))
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    let reservation = ctx.reserve_scoped(bytes, operation)?;
    let mut values = Vec::new();
    reserve_vec(ctx, &mut values, count, operation)?;
    Ok((values, reservation))
}

pub(crate) fn temporary_set<'a, T: Eq + Hash>(
    ctx: &'a DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(HashSet<T>, ScopedReservation<'a>), CodecError> {
    let reservation =
        ctx.reserve_scoped(temporary_bytes::<T>(ctx, count, operation)?, operation)?;
    let mut values = HashSet::new();
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(0, values.capacity(), count, operation))?;
    Ok((values, reservation))
}

pub(crate) fn temporary_queue<'a, T>(
    ctx: &'a DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(VecDeque<T>, ScopedReservation<'a>), CodecError> {
    let reservation =
        ctx.reserve_scoped(temporary_bytes::<T>(ctx, count, operation)?, operation)?;
    let mut values = VecDeque::new();
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(0, values.capacity(), count, operation))?;
    Ok((values, reservation))
}

#[cfg(test)]
mod tests {
    use super::{admit_map_entry, copy_id};
    use std::collections::HashMap;

    #[test]
    fn copied_surface_identity_refuses_before_string_storage() {
        use cadmpeg_core::CodecError;
        use cadmpeg_ir::ids::SurfaceId;

        let id = SurfaceId::mint("catia:test:surface#copied".to_string())
            .expect("valid fixture identity");
        let copied = crate::test_support::with_service_context(|ctx| {
            copy_id(ctx, id.as_str(), SurfaceId::mint, "catia_surface_id_copy")
        })
        .expect("service budget");
        assert_eq!(copied, id);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits retained limit");
        assert!(matches!(
            copy_id(&ctx, id.as_str(), SurfaceId::mint, "catia_surface_id_copy"),
            Err(CodecError::ResourceLimit(error)) if error.operation == "catia_surface_id_copy"
        ));
    }

    #[test]
    fn zero_entity_source_cache_refuses_before_vacant_map_entry() {
        let mut limited_map = HashMap::<u32, u32>::new();
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            admit_map_entry(
                ctx,
                &mut limited_map,
                &7,
                "catia_zero_wire_source_geometries",
            )
        });
        assert!(matches!(
            limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert!(limited_map.is_empty());

        crate::test_support::with_service_context(|ctx| {
            let mut admitted_map = HashMap::<u32, u32>::new();
            admit_map_entry(
                ctx,
                &mut admitted_map,
                &7,
                "catia_zero_wire_source_geometries",
            )
            .expect("service map entry budget");
            admitted_map.insert(7, 11);
            admit_map_entry(
                ctx,
                &mut admitted_map,
                &7,
                "catia_zero_wire_source_geometries",
            )
            .expect("existing entry needs no allocation");
            assert_eq!(admitted_map.get(&7), Some(&11));
        });
    }
}
