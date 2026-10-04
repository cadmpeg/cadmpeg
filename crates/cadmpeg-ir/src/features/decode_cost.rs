// SPDX-License-Identifier: Apache-2.0
//! Cost helpers for feature-owned decode values.

pub(super) fn checked_feature_decode_cost_sum(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    left: u64,
    right: u64,
    operation: &'static str,
) -> Result<u64, cadmpeg_core::CodecError> {
    left.checked_add(right)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
}

macro_rules! feature_decode_cost_sum {
    ($ctx:ident, $operation:ident; $($value:expr),+ $(,)?) => {{
        let mut total = 0_u64;
        $(total = $crate::features::decode_cost::checked_feature_decode_cost_sum(
            $ctx,
            total,
            cadmpeg_core::decode::cost::DecodeCost::decode_cost(&$value, $ctx, $operation)?,
            $operation,
        )?;)+
        Ok::<u64, cadmpeg_core::CodecError>(total)
    }};
}

macro_rules! impl_feature_decode_cost_copy {
    ($type:ty) => {
        impl cadmpeg_core::decode::cost::DecodeCost for $type {
            const FIXED_BYTES: Option<u64> = Some(cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<Self>(),
            ));

            fn decode_cost(
                &self,
                _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                _operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Self>()))
            }
        }
    };
}

macro_rules! impl_feature_decode_cost_record {
    ($type:ty; map $mapfield:ident; { $($field:ident),+ $(,)? }) => {
        impl cadmpeg_core::decode::cost::DecodeCost for $type {
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                let mut total = feature_decode_cost_sum!(ctx, operation; $( &self.$field ),+)?;
                for entry in ctx.admit_iter(&self.$mapfield, operation)? {
                    total = $crate::features::decode_cost::checked_feature_decode_cost_sum(
                        ctx,
                        total,
                        cadmpeg_core::decode::cost::DecodeCost::decode_cost(&entry, ctx, operation)?,
                        operation,
                    )?;
                }
                Ok(total)
            }
        }
    };
    ($type:ty; { $($field:ident),+ $(,)? }) => {
        impl cadmpeg_core::decode::cost::DecodeCost for $type {
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                feature_decode_cost_sum!(ctx, operation; $( &self.$field ),+)
            }
        }
    };
    ($type:ty; ($field:tt)) => {
        impl cadmpeg_core::decode::cost::DecodeCost for $type {
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                feature_decode_cost_sum!(ctx, operation; &self.$field)
            }
        }
    };
    ($type:ident<$($generic:ident),+>, [$($bound:ident),+]; { $($field:ident),+ $(,)? }) => {
        impl<$($bound: cadmpeg_core::decode::cost::DecodeCost),+>
            cadmpeg_core::decode::cost::DecodeCost for $type<$($generic),+>
        {
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                feature_decode_cost_sum!(ctx, operation; $( &self.$field ),+)
            }
        }
    };
    ($type:ident<$($generic:ident),+>, [$($bound:ident),+]; ($field:tt)) => {
        impl<$($bound: cadmpeg_core::decode::cost::DecodeCost),+>
            cadmpeg_core::decode::cost::DecodeCost for $type<$($generic),+>
        {
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                feature_decode_cost_sum!(ctx, operation; &self.$field)
            }
        }
    };
}

macro_rules! impl_feature_decode_cost_enum {
    ($type:ty; map $mapvariant:ident { $mapvalue:ident; $mapfield:ident }; {
        $($variant:ident $( ( $($tuple:ident),* ) )? $( { $($field:ident),* } )?),* $(,)?
    }) => {
        impl cadmpeg_core::decode::cost::DecodeCost for $type {
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                match self {
                    Self::$mapvariant { $mapvalue, $mapfield } => {
                        let mut total = feature_decode_cost_sum!(ctx, operation; 0_u8, $mapvalue)?;
                        for entry in ctx.admit_iter($mapfield, operation)? {
                            total = $crate::features::decode_cost::checked_feature_decode_cost_sum(
                                ctx,
                                total,
                                cadmpeg_core::decode::cost::DecodeCost::decode_cost(&entry, ctx, operation)?,
                                operation,
                            )?;
                        }
                        Ok(total)
                    },
                    $(
                        Self::$variant $(($($tuple),*))? $({ $($field),* })? => {
                            feature_decode_cost_sum!(ctx, operation; 0_u8 $( $(, $tuple)* )? $( $(, $field)* )?)
                        }
                    ),*
                }
            }
        }
    };
    ($type:ty; {
        $($variant:ident $( ( $($tuple:ident),* ) )? $( { $($field:ident),* } )?),* $(,)?
    }) => {
        impl cadmpeg_core::decode::cost::DecodeCost for $type {
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                match self {
                    $(
                        Self::$variant $(($($tuple),*))? $({ $($field),* })? => {
                            feature_decode_cost_sum!(ctx, operation; 0_u8 $( $(, $tuple)* )? $( $(, $field)* )?)
                        }
                    ),*
                }
            }
        }
    };
    ($type:ident<$($generic:ident),+>, [$($bound:ident),+]; {
        $($variant:ident $( ( $($tuple:ident),* ) )? $( { $($field:ident),* } )?),* $(,)?
    }) => {
        impl<$($bound: cadmpeg_core::decode::cost::DecodeCost),+>
            cadmpeg_core::decode::cost::DecodeCost for $type<$($generic),+>
        {
            fn decode_cost(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                match self {
                    $(
                        Self::$variant $(($($tuple),*))? $({ $($field),* })? => {
                            feature_decode_cost_sum!(ctx, operation; 0_u8 $( $(, $tuple)* )? $( $(, $field)* )?)
                        }
                    ),*
                }
            }
        }
    };
}

macro_rules! impl_feature_decode_cost_empty_enum {
    ($type:ty) => {
        impl cadmpeg_core::decode::cost::DecodeCost for $type {
            fn decode_cost(
                &self,
                _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                _operation: &'static str,
            ) -> Result<u64, cadmpeg_core::CodecError> {
                match *self {}
            }
        }
    };
}
