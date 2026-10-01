// SPDX-License-Identifier: Apache-2.0
//! Bounded input acquisition under the caller's session budget.

use std::io::Read;

use crate::CodecError;
use super::{u64_from_index, DecodeContext, ResourceDimension, ResourceFailure};

impl DecodeContext<'_> {
    /// Reads at most `length` bytes and admits each copied byte before growth.
    /// Input storage is governed by the input-byte dimension.
    pub fn read_input_prefix(&self, reader: &mut dyn Read, length: usize) -> Result<Vec<u8>, CodecError> {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 8192];
        while bytes.len() < length {
            let count = (length - bytes.len()).min(chunk.len());
            self.charge_work(u64_from_index(count), "read input prefix")?;
            let read = reader.read(&mut chunk[..count]).map_err(CodecError::Io)?;
            if read == 0 {
                break;
            }
            self.budget.charge_input(u64_from_index(read), "read input prefix")?;
            self.charge_collection_items(u64_from_index(read), "input byte slots")?;
            self.charge_work(u64_from_index(read), "copy input prefix")?;
            bytes.try_reserve_exact(read).map_err(|_| self.budget.refuse(
                ResourceDimension::InputBytes, ResourceFailure::AllocationFailed,
                self.policy().limits.max_input_bytes, self.budget.input_bytes(),
                u64_from_index(read), "input prefix storage"))?;
            bytes.extend_from_slice(&chunk[..read]);
        }
        Ok(bytes)
    }

    /// Raises a typed input-byte refusal without changing its dimension.
    pub fn refuse_input_limit(&self, additional: u64, operation: &'static str) -> CodecError {
        self.budget.refuse(ResourceDimension::InputBytes, ResourceFailure::BudgetExceeded,
            self.policy().limits.max_input_bytes, self.budget.input_bytes(), additional, operation)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use crate::CodecError;
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

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
        assert_eq!(ctx.read_input_prefix(&mut reader, 2).expect("prefix"), b"ab");
        assert!(matches!(ctx.read_input_prefix(&mut reader, 1),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::InputBytes
                && limit.used == 2 && limit.additional == 1));
    }
}
