// SPDX-License-Identifier: Apache-2.0
//! Fallible collection admission for `FreeCAD` decoding.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
use std::fmt;

pub(crate) fn malformed_charged(
    ctx: &DecodeContext<'_>,
    arguments: fmt::Arguments<'_>,
    operation: &'static str,
) -> CodecError {
    match ctx.format_retained(arguments, operation) {
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
            return Err(CodecError::Malformed(ctx.join_retained(
                &[record, " states a property with a blank key"],
                "",
                operation,
            )?));
        };
        ctx.charge_collection_items(1, operation)?;
        keyed.insert(key, value);
    }
    Ok(keyed)
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
    I::try_from(ctx.copy_retained_text(value, operation)?).map_err(CodecError::malformed)
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
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "test identity copy")
        );
    }

    #[test]
    fn retained_format_preserves_text_and_refuses_before_allocation() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let text = "Shape & Surface";
        let expected = format!("invalid {text}: {}", 12);
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert_eq!(
            ctx.format_retained(
                format_args!("invalid {text}: {}", 12),
                "test formatted diagnostic"
            )
            .expect("format fits"),
            expected
        );
        let mut policy = policy;
        policy.limits.max_retained_bytes = expected.len() as u64 - 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(
            matches!(ctx.format_retained(format_args!("invalid {text}: {}", 12),
            "test formatted diagnostic"), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "test formatted diagnostic")
        );
    }

    #[test]
    fn charged_named_entries_refuse_at_collection_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let entries = std::collections::BTreeMap::from([("role".to_owned(), 1_u8)]);
        assert!(
            matches!(super::named_entries_charged(&ctx, "owner", entries, "test keyed entries"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "test keyed entries")
        );
    }

    #[test]
    fn charged_hash_index_refuses_at_caller_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let mut index = std::collections::HashMap::new();
        assert!(
            matches!(ctx.insert_hash_map(&mut index, "key", 1_u8, "fcstd test index"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "fcstd test index")
        );
    }
}
