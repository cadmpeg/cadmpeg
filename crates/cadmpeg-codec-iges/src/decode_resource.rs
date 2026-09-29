// SPDX-License-Identifier: Apache-2.0
//! IGES identity conversion and text decoding.

use cadmpeg_core::decode::{refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;

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
    let mut text = ctx.retained_string(output_len, operation)?;
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

#[cfg(test)]
mod tests {
    use super::{
        admit_optional_entities, lossy_retained,
    };
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
        let result = ctx.format_retained(format_args!("item{}", 7), "iges formatted test");
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
            ctx.format_retained(format_args!("item{}", 7), "iges formatted test")
                .expect("valid test fixture"),
            "item7"
        );
    }

    #[test]
    fn optional_collection_keeps_an_earlier_missing_value() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("valid test fixture");
        let result = ctx
            .collect_options([None, Some(7_u8)], "iges optional test")
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
        let result = ctx.collect_options([Some(3_u8), Some(7_u8)], "iges optional test");
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
        let result = ctx
            .collect_options([Some(3_u8), Some(7_u8)], "iges optional test")
            .expect("valid test fixture");
        assert_eq!(result, Some(vec![3, 7]));
    }

    #[test]
    fn geometry_creation_refuses_the_next_entity_at_its_boundary() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test setup");
        admit_optional_entities(Some(&ctx), 2, "iges geometry test").expect("test setup");
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
