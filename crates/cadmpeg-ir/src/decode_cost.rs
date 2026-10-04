// SPDX-License-Identifier: Apache-2.0
//! Field costs for retained geometry. Tags cost one byte; fields exclude addresses.

macro_rules! decode_cost_fields {
    ($ctx:ident, $operation:ident, $tag:literal; []) => { Ok($tag) };
    ($ctx:ident, $operation:ident, $tag:literal; [$($field:ident),+]) => {{
        let mut bytes: u64 = $tag;
        $(bytes = bytes.checked_add(cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            $field, $ctx, $operation,
        )?).ok_or_else(|| $ctx.refuse_codec_limit($operation, u64::MAX, u64::MAX))?;)+
        Ok(bytes)
    }};
}

macro_rules! decode_cost_record {
    ([$($bounds:tt)*] $type:ty $(, depth $guard:ty)?; $pattern:pat => [$($field:ident: $field_type:ty),+]) => {
        impl<$($bounds)*> cadmpeg_core::decode::cost::DecodeCost for $type {
            const FIXED_BYTES: Option<u64> = {
                let mut bytes = Some(0_u64);
                $(bytes = match (bytes, <$field_type as cadmpeg_core::decode::cost::DecodeCost>::FIXED_BYTES) {
                    (Some(left), Some(right)) => left.checked_add(right),
                    _ => None,
                };)+
                bytes
            };
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                if let Some(bytes) = Self::FIXED_BYTES {
                    return Ok(bytes);
                }
                $(let _depth: $guard = ctx.enter_nested(operation)?;)?
                let $pattern = self;
                decode_cost_fields!(ctx, operation, 0; [$($field),+])
            }
        }
    };
}

macro_rules! decode_cost_enum {
    ([$($bounds:tt)*] $type:ty $(, depth $guard:ty)?; $($pattern:pat => [$($field:ident),*]),+ $(,)?) => {
        impl<$($bounds)*> cadmpeg_core::decode::cost::DecodeCost for $type {
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                $(let _depth: $guard = ctx.enter_nested(operation)?;)?
                match self {
                    $($pattern => decode_cost_fields!(ctx, operation, 1; [$($field),*]),)+
                }
            }
        }
    };
    ($type:ty) => {
        impl cadmpeg_core::decode::cost::DecodeCost for $type {
            const FIXED_BYTES: Option<u64> = Some(1);
            fn decode_cost(
                &self,
                _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                _operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                Ok(1)
            }
        }
    };
}
