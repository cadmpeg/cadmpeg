// SPDX-License-Identifier: Apache-2.0
//! Bounded input acquisition under the caller's session budget.

use std::io::Read;

use super::{u64_from_index, DecodeContext, ResourceDimension, ResourceFailure};
use crate::CodecError;

impl DecodeContext<'_> {
    /// Reads at most `length` bytes and admits each copied byte before growth.
    /// Input storage is governed by the input-byte dimension.
    pub fn read_input_prefix(
        &self,
        reader: &mut dyn Read,
        length: usize,
    ) -> Result<Vec<u8>, CodecError> {
        let mut bytes = Vec::new();
        self.extend_input_prefix(reader, &mut bytes, length)?;
        Ok(bytes)
    }

    /// Extends admitted input storage up to a total bounded length.
    pub fn extend_input_prefix(
        &self,
        reader: &mut dyn Read,
        bytes: &mut Vec<u8>,
        length: usize,
    ) -> Result<(), CodecError> {
        let mut chunk = [0_u8; 8192];
        while bytes.len() < length {
            self.charge_work(1, "input prefix iteration")?;
            let count = (length - bytes.len()).min(chunk.len());
            self.charge_work(u64_from_index(count), "read input prefix")?;
            let read = reader.read(&mut chunk[..count]).map_err(CodecError::Io)?;
            if read == 0 {
                break;
            }
            self.budget
                .charge_input(u64_from_index(read), "read input prefix")?;
            self.charge_collection_items(u64_from_index(read), "input byte slots")?;
            self.charge_work(u64_from_index(read), "copy input prefix")?;
            bytes.try_reserve_exact(read).map_err(|_| {
                self.budget.refuse(
                    ResourceDimension::InputBytes,
                    ResourceFailure::AllocationFailed,
                    self.policy().limits.max_input_bytes,
                    self.budget.input_bytes(),
                    u64_from_index(read),
                    "input prefix storage",
                )
            })?;
            bytes.extend_from_slice(&chunk[..read]);
        }
        Ok(())
    }

    /// Acquires the remaining input without reading or charging the prefix twice.
    pub fn complete_input(
        &self,
        reader: &mut dyn Read,
        bytes: &mut Vec<u8>,
    ) -> Result<(), CodecError> {
        let max = self.policy().limits.max_input_bytes;
        let length = usize::try_from(max).map_or(usize::MAX, std::convert::identity);
        self.extend_input_prefix(reader, bytes, length)?;
        self.charge_work(1, "input end probe")?;
        if reader.read(&mut [0_u8; 1])? != 0 {
            return Err(self.refuse_input_limit(1, "complete input"));
        }
        Ok(())
    }

    /// Raises a typed input-byte refusal without changing its dimension.
    pub fn refuse_input_limit(&self, additional: u64, operation: &'static str) -> CodecError {
        self.budget.refuse(
            ResourceDimension::InputBytes,
            ResourceFailure::BudgetExceeded,
            self.policy().limits.max_input_bytes,
            self.budget.input_bytes(),
            additional,
            operation,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;
    use std::io::Cursor;

    #[test]
    fn input_prefix_iteration_refuses_before_read() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut reader = Cursor::new(b"a");
        assert!(matches!(ctx.read_input_prefix(&mut reader, 1),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits));
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn input_prefix_refusal_keeps_input_dimension() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_input_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(ctx.read_input_prefix(&mut Cursor::new(b"abc"), 3),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::InputBytes
                && limit.used == 0 && limit.additional == 3));
    }

    #[test]
    fn input_prefix_reads_share_one_input_budget() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_input_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut reader = Cursor::new(b"abc");
        assert_eq!(
            ctx.read_input_prefix(&mut reader, 2).expect("prefix"),
            b"ab"
        );
        assert!(matches!(ctx.read_input_prefix(&mut reader, 1),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::InputBytes
                && limit.used == 2 && limit.additional == 1));
    }
}
