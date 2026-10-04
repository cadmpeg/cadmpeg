// SPDX-License-Identifier: Apache-2.0
//! Charged parsing of text into standard scalar values.

use super::{u64_from_index, DecodeContext};
use crate::CodecError;

mod sealed {
    pub trait Scalar {}
    pub trait Source {}
    pub trait Radix {}
    pub trait Query {}
}

/// Text values whose borrowed view requires no scan, copy or allocation.
pub trait TextSource: sealed::Source {
    /// Returns the existing UTF-8 view.
    fn as_text(&self) -> &str;

    /// Transfer owned text or copy borrowed text through the caller budget.
    /// The caller admits any existing owned storage.
    fn into_retained_text(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<String, super::ResourceLimit>
    where
        Self: Sized,
    {
        ctx.copy_retained_text_limit(self.as_text(), operation)
    }
}

impl sealed::Source for str {}
impl TextSource for str {
    fn as_text(&self) -> &str {
        self
    }
}
impl sealed::Source for String {}
impl TextSource for String {
    fn as_text(&self) -> &str {
        self.as_str()
    }
    fn into_retained_text(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<String, super::ResourceLimit> {
        ctx.charge_work_limit(0, operation)?;
        Ok(self)
    }
}
impl<T: TextSource + ?Sized> sealed::Source for &T {}
impl<T: TextSource + ?Sized> TextSource for &T {
    fn as_text(&self) -> &str {
        T::as_text(*self)
    }
}

/// Borrowed text and byte views with constant-time extent and range access.
pub trait QuerySource: sealed::Query {
    /// The borrowed result domain: UTF-8 text or bytes.
    type View: ?Sized;
    /// Returns the existing bytes without traversal.
    fn query_bytes(&self) -> &[u8];
    /// Returns an existing range, checking bounds and UTF-8 boundaries.
    fn query_range(&self, range: std::ops::Range<usize>) -> Option<&Self::View>;
}

impl sealed::Query for str {}
impl QuerySource for str {
    type View = str;
    fn query_bytes(&self) -> &[u8] { self.as_bytes() }
    fn query_range(&self, range: std::ops::Range<usize>) -> Option<&str> { self.get(range) }
}
impl sealed::Query for String {}
impl QuerySource for String {
    type View = str;
    fn query_bytes(&self) -> &[u8] { self.as_bytes() }
    fn query_range(&self, range: std::ops::Range<usize>) -> Option<&str> { self.get(range) }
}
impl sealed::Query for [u8] {}
impl QuerySource for [u8] {
    type View = [u8];
    fn query_bytes(&self) -> &[u8] { self }
    fn query_range(&self, range: std::ops::Range<usize>) -> Option<&[u8]> { self.get(range) }
}
impl sealed::Query for Vec<u8> {}
impl QuerySource for Vec<u8> {
    type View = [u8];
    fn query_bytes(&self) -> &[u8] { self.as_slice() }
    fn query_range(&self, range: std::ops::Range<usize>) -> Option<&[u8]> { self.get(range) }
}
impl<const N: usize> sealed::Query for [u8; N] {}
impl<const N: usize> QuerySource for [u8; N] {
    type View = [u8];
    fn query_bytes(&self) -> &[u8] { self.as_slice() }
    fn query_range(&self, range: std::ops::Range<usize>) -> Option<&[u8]> { self.get(range) }
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
text_scalars!(
    std::num::NonZeroU8,
    std::num::NonZeroU16,
    std::num::NonZeroU32,
    std::num::NonZeroU64,
    std::num::NonZeroU128,
    std::num::NonZeroUsize,
    std::num::NonZeroI8,
    std::num::NonZeroI16,
    std::num::NonZeroI32,
    std::num::NonZeroI64,
    std::num::NonZeroI128,
    std::num::NonZeroIsize
);

/// Standard integer radix parsers with no child storage or custom callbacks.
pub trait RadixScalar: sealed::Radix + Sized {
    /// Parses one integer with a radix from 2 through 36.
    fn parse_radix(
        ctx: &DecodeContext<'_>,
        text: &str,
        radix: u32,
        operation: &'static str,
    ) -> Result<Result<Self, std::num::ParseIntError>, CodecError>;
}
macro_rules! radix_scalars {
    ($($scalar:ty),+) => {$(
        impl sealed::Radix for $scalar {}
        impl RadixScalar for $scalar {
            fn parse_radix(ctx: &DecodeContext<'_>, text: &str, radix: u32, operation: &'static str) -> Result<Result<Self, std::num::ParseIntError>, CodecError> {
                ctx.charge_work(u64_from_index(text.len()), operation)?;
                if !(2..=36).contains(&radix) { return Err(CodecError::malformed("integer radix is outside 2 through 36")); }
                Ok(<$scalar>::from_str_radix(text, radix))
            }
        }
    )+};
}
radix_scalars!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

impl DecodeContext<'_> {
    /// Admits integer input bytes and preserves the standard numeric error.
    pub fn parse_radix<T: RadixScalar>(
        &self,
        text: &str,
        radix: u32,
        operation: &'static str,
    ) -> Result<Result<T, std::num::ParseIntError>, CodecError> {
        T::parse_radix(self, text, radix, operation)
    }

    /// Charge the complete text scan and retain its result with the exact input.
    pub fn validate_nonblank_text<S: TextSource>(
        &self,
        source: S,
        operation: &'static str,
    ) -> Result<crate::text::NonBlankText<S>, super::ResourceLimit> {
        let nonblank = self
            .admit_iter(source.as_text(), operation)?
            .any(|character| !character.is_whitespace());
        Ok(crate::text::NonBlankText { source, nonblank })
    }

    /// Admits input bytes before parsing and preserves the standard parse error.
    pub fn parse_text<T: TextScalar>(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<Result<T, T::Err>, CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        Ok(text.parse())
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::CodecError;

    #[test]
    fn radix_parsing_preserves_values_errors_and_admission() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation succeeds");
        assert_eq!(
            ctx.parse_radix::<i32>("-fF", 16, "radix")
                .expect("test operation succeeds"),
            Ok(-255)
        );
        assert_eq!(
            ctx.parse_radix::<u8>("100", 16, "radix")
                .expect("test operation succeeds"),
            u8::from_str_radix("100", 16)
        );
        assert!(matches!(
            ctx.parse_radix::<u8>("1", 1, "invalid radix"),
            Err(CodecError::Malformed(_))
        ));
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        let CodecError::ResourceLimit(first) = ctx
            .parse_radix::<u8>("invalid", 16, "refusal")
            .expect_err("operation refuses")
        else {
            panic!("resource refusal")
        };
        let CodecError::ResourceLimit(repeated) = ctx
            .parse_radix::<u8>("1", 16, "later")
            .expect_err("operation refuses")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
    }

    #[test]
    fn charged_parse_keeps_standard_errors_and_charges_input_bytes() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        assert_eq!(
            ctx.parse_text::<u64>("123", "parse").expect("admission"),
            Ok(123)
        );
        assert_eq!(
            ctx.parse_text::<u8>("256", "parse").expect("admission"),
            "256".parse::<u8>()
        );
        assert_eq!(
            ctx.parse_text::<char>("é", "parse").expect("admission"),
            Ok('é')
        );
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
            panic!("refusal")
        };
        // Two three-byte numeric inputs and one two-byte Unicode scalar input.
        assert_eq!(limit.used, 8);
    }

    #[test]
    fn charged_parse_refusal_precedes_parse_error_and_preserves_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let CodecError::ResourceLimit(limit) = ctx
            .parse_text::<u64>("invalid", "parse")
            .expect_err("refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(ctx.resource_refusal(), Some(limit));
        let CodecError::ResourceLimit(repeated) =
            ctx.parse_text::<u64>("1", "again").expect_err("fused")
        else {
            panic!("refusal")
        };
        assert_eq!(limit, repeated);
    }
}
