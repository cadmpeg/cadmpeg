// SPDX-License-Identifier: Apache-2.0
//! Fallible linear search and byte comparison under the caller's work budget.

use super::{u64_from_index, DecodeContext};
use crate::CodecError;

impl DecodeContext<'_> {
    /// Returns the first matching position. Each visited slot is admitted before
    /// the predicate runs. The predicate admits its own input-sized child work.
    pub fn position_by<T>(
        &self,
        values: &[T],
        mut predicate: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        for (index, value) in values.iter().enumerate() {
            self.charge_work(1, operation)?;
            if predicate(value)? {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }

    /// Compares equal-length byte slices after admitting the complete scan.
    /// Unequal lengths need no input-sized comparison.
    pub fn equal_bytes(
        &self,
        left: &[u8],
        right: &[u8],
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if left.len() != right.len() {
            return Ok(false);
        }
        self.charge_work(u64_from_index(left.len()), operation)?;
        Ok(left == right)
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    #[test]
    fn scan_search_admits_before_predicate_and_preserves_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut visited = 0;
        let error = ctx.position_by(&[1, 2], |_| {
            visited += 1;
            Ok(false)
        }, "search").unwrap_err();
        assert_eq!(visited, 1);
        let CodecError::ResourceLimit(limit) = error else { panic!("work refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }

    #[test]
    fn scan_search_stops_at_match_and_propagates_child_error() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        assert_eq!(ctx.position_by(&[1, 2, 3], |value| Ok(*value == 2), "search").unwrap(), Some(1));
        assert_eq!(ctx.position_by(&[1], |_| Ok(false), "search").unwrap(), None);
        let error = ctx.position_by(&[1], |_| Err(ctx.refuse_codec_limit("child", 0, 1)), "search").unwrap_err();
        let CodecError::ResourceLimit(limit) = error else { panic!("child refusal"); };
        assert_eq!(limit.operation, "child");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }

    #[test]
    fn scan_byte_equality_admits_before_comparison() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(!ctx.equal_bytes(b"a", b"ab", "compare").unwrap());
        let error = ctx.equal_bytes(b"ab", b"ab", "compare").unwrap_err();
        let CodecError::ResourceLimit(limit) = error else { panic!("work refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }

    #[test]
    fn scan_byte_equality_preserves_exact_results() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        assert!(ctx.equal_bytes(b"ab", b"ab", "compare").unwrap());
        assert!(!ctx.equal_bytes(b"ab", b"ac", "compare").unwrap());
        assert!(ctx.equal_bytes(b"", b"", "compare").unwrap());
    }
}
