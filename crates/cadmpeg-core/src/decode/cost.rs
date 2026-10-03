// SPDX-License-Identifier: Apache-2.0
//! Bytes read by hashing and comparison, including owned children.

use super::{u64_from_index, DecodeContext};
use crate::CodecError;

/// States the bytes read by one hash or comparison of a value.
/// Implementations admit input-sized traversal needed to measure child values.
/// Fixed costs exclude pointer addresses and include the pointed-to value.
pub trait DecodeCost {
    /// Constant byte cost, when independent of the value and its children.
    const FIXED_BYTES: Option<u64> = None;

    /// Measures child bytes with checked arithmetic and charged traversal.
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str)
        -> Result<u64, CodecError>;
}

macro_rules! scalar_cost {
    ($($scalar:ty),+) => {$(
        impl DecodeCost for $scalar {
            const FIXED_BYTES: Option<u64> = Some(std::mem::size_of::<Self>() as u64);
            fn decode_cost(&self, _ctx: &DecodeContext<'_>, _operation: &'static str)
                -> Result<u64, CodecError> {
                Ok(u64_from_index(std::mem::size_of::<Self>()))
            }
        }
    )+};
}
scalar_cost!((), bool, char, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64);

impl DecodeCost for str {
    fn decode_cost(&self, _ctx: &DecodeContext<'_>, _operation: &'static str) -> Result<u64, CodecError> {
        Ok(u64_from_index(self.len()))
    }
}
impl DecodeCost for String {
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
        self.as_str().decode_cost(ctx, operation)
    }
}
impl<T: DecodeCost + ?Sized> DecodeCost for &T {
    const FIXED_BYTES: Option<u64> = T::FIXED_BYTES;
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
        T::decode_cost(self, ctx, operation)
    }
}
impl<T: DecodeCost + ?Sized> DecodeCost for Box<T> {
    const FIXED_BYTES: Option<u64> = T::FIXED_BYTES;
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
        T::decode_cost(self, ctx, operation)
    }
}
impl<T: DecodeCost> DecodeCost for [T] {
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
        if let Some(bytes) = T::FIXED_BYTES {
            return ctx.cost_product(u64_from_index(self.len()), bytes, operation);
        }
        let mut bytes = 0_u64;
        for value in ctx.admit_iter(self, operation)? {
            bytes = ctx.cost_sum(bytes, value.decode_cost(ctx, operation)?, operation)?;
        }
        Ok(bytes)
    }
}
impl<T: DecodeCost> DecodeCost for Vec<T> {
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
        self.as_slice().decode_cost(ctx, operation)
    }
}
impl<T: DecodeCost, const N: usize> DecodeCost for [T; N] {
    const FIXED_BYTES: Option<u64> = match T::FIXED_BYTES {
        Some(bytes) => bytes.checked_mul(N as u64),
        None => None,
    };
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
        self.as_slice().decode_cost(ctx, operation)
    }
}
impl<T: DecodeCost> DecodeCost for Option<T> {
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
        match self {
            Some(value) => ctx.cost_sum(1, value.decode_cost(ctx, operation)?, operation),
            None => Ok(1),
        }
    }
}
macro_rules! tuple_cost {
    ($($name:ident:$field:tt),+) => {
        impl<$($name: DecodeCost),+> DecodeCost for ($($name,)+) {
            fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
                let mut bytes = 0_u64;
                $(bytes = ctx.cost_sum(bytes, self.$field.decode_cost(ctx, operation)?, operation)?;)+
                Ok(bytes)
            }
        }
    };
}
tuple_cost!(A:0);
tuple_cost!(A:0, B:1);
tuple_cost!(A:0, B:1, C:2);
tuple_cost!(A:0, B:1, C:2, D:3);
tuple_cost!(A:0, B:1, C:2, D:3, E:4);
tuple_cost!(A:0, B:1, C:2, D:3, E:4, F:5);

impl DecodeContext<'_> {
    pub(crate) fn cost_sum(&self, left: u64, right: u64, operation: &'static str) -> Result<u64, CodecError> {
        left.checked_add(right).ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }
    pub(crate) fn cost_product(&self, left: u64, right: u64, operation: &'static str) -> Result<u64, CodecError> {
        left.checked_mul(right).ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }
    /// Admits the key bytes read by a known number of comparisons or hashes.
    pub(crate) fn charge_key<T: DecodeCost + ?Sized>(&self, key: &T, comparisons: u64, operation: &'static str) -> Result<(), CodecError> {
        let bytes = key.decode_cost(self, operation)?;
        self.charge_work(self.cost_product(bytes, comparisons, operation)?, operation)
    }
    /// Bounds a B-tree lookup by eleven comparisons per node and log2(n)+1 nodes.
    pub(crate) fn tree_comparisons(&self, length: usize) -> u64 {
        if length == 0 { 0 } else { 11 * (u64::from(length.ilog2()) + 1) }
    }
}
