// SPDX-License-Identifier: Apache-2.0
//! Charged parsing of text into standard scalar values.

use super::{u64_from_index, DecodeContext};
use crate::CodecError;

mod sealed {
    pub trait Scalar {}
    pub trait Source {}
}

/// Text values whose borrowed view requires no scan, copy or allocation.
pub trait TextSource: sealed::Source {
    /// Returns the existing UTF-8 view.
    fn as_text(&self) -> &str;
}

impl sealed::Source for str {}
impl TextSource for str {
    fn as_text(&self) -> &str { self }
}
impl sealed::Source for String {}
impl TextSource for String {
    fn as_text(&self) -> &str { self.as_str() }
}
impl<T: TextSource + ?Sized> sealed::Source for &T {}
impl<T: TextSource + ?Sized> TextSource for &T {
    fn as_text(&self) -> &str { T::as_text(*self) }
}

/// Standard scalar parsers that allocate no input-sized result storage.
pub trait TextScalar: sealed::Scalar + std::str::FromStr {}

macro_rules! text_scalars {
    ($($scalar:ty),+) => {$(
        impl sealed::Scalar for $scalar {}
        impl TextScalar for $scalar {}
    )+};
}
text_scalars!(bool, char, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64);
text_scalars!(std::num::NonZeroU8, std::num::NonZeroU16, std::num::NonZeroU32,
    std::num::NonZeroU64, std::num::NonZeroU128, std::num::NonZeroUsize,
    std::num::NonZeroI8, std::num::NonZeroI16, std::num::NonZeroI32,
    std::num::NonZeroI64, std::num::NonZeroI128, std::num::NonZeroIsize);

impl DecodeContext<'_> {
    /// Admits input bytes before parsing and preserves the standard parse error.
    pub fn parse_text<T: TextScalar>(&self, text: &str, operation: &'static str)
        -> Result<Result<T, T::Err>, CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        Ok(text.parse())
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::CodecError;

    #[test]
    fn charged_parse_keeps_standard_errors_and_charges_input_bytes() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        assert_eq!(ctx.parse_text::<u64>("123", "parse").expect("admission"), Ok(123));
        assert_eq!(ctx.parse_text::<u8>("256", "parse").expect("admission"), "256".parse::<u8>());
        assert_eq!(ctx.parse_text::<char>("é", "parse").expect("admission"), Ok('é'));
        let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "probe").expect_err("probe") else { panic!("refusal") };
        // Two three-byte numeric inputs and one two-byte Unicode scalar input.
        assert_eq!(limit.used, 8);
    }

    #[test]
    fn charged_parse_refusal_precedes_parse_error_and_preserves_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let CodecError::ResourceLimit(limit) = ctx.parse_text::<u64>("invalid", "parse").expect_err("refusal") else { panic!("refusal") };
        assert_eq!(ctx.resource_refusal(), Some(limit));
        let CodecError::ResourceLimit(repeated) = ctx.parse_text::<u64>("1", "again").expect_err("fused") else { panic!("refusal") };
        assert_eq!(limit, repeated);
    }
}
