// SPDX-License-Identifier: Apache-2.0
//! Fallible collection admission for `FreeCAD` decoding.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
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

#[cfg(test)]
mod tests {
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
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(expected.len()) - 1;
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
            matches!(cadmpeg_core::text::named_entries_for_decode(&ctx, "owner", entries).map_err(cadmpeg_core::CodecError::from),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "named entry map nodes")
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
