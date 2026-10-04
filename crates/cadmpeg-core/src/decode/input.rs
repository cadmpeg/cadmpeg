// SPDX-License-Identifier: Apache-2.0
//! Bounded input acquisition under the caller's session budget.

use std::io::Read;

use super::{u64_from_index, DecodeContext, ResourceDimension, ResourceFailure};
use crate::CodecError;

const INPUT_CHUNK: usize = 8192;

impl DecodeContext<'_> {
    /// Reads at most `length` bytes and admits each copied byte before growth.
    /// Input storage is governed by the input-byte dimension.
    pub fn read_input_prefix<R: Read + ?Sized>(
        &self,
        reader: &mut R,
        length: usize,
    ) -> Result<Vec<u8>, CodecError> {
        let mut bytes = Vec::new();
        self.extend_input_prefix(reader, &mut bytes, length)?;
        Ok(bytes)
    }

    /// Extends admitted input storage up to a total bounded length.
    pub fn extend_input_prefix<R: Read + ?Sized>(
        &self,
        reader: &mut R,
        bytes: &mut Vec<u8>,
        length: usize,
    ) -> Result<(), CodecError> {
        let mut chunk = [0_u8; INPUT_CHUNK];
        while bytes.len() < length {
            self.charge_work(1, "input prefix iteration")?;
            let count = (length - bytes.len()).min(chunk.len());
            let window = &mut chunk[..count];
            self.charge_work(u64_from_index(window.len()), "read input prefix")?;
            let read = reader.read(window).map_err(CodecError::Io)?;
            if read > count {
                return Err(CodecError::Io(std::io::ErrorKind::InvalidData.into()));
            }
            if read == 0 {
                break;
            }
            self.budget
                .charge_input(u64_from_index(read), "read input prefix")?;
            self.charge_collection_items(u64_from_index(read), "input byte slots")?;
            let copied = &chunk[..read];
            self.reserve_precharged_bytes(
                bytes,
                copied.len(),
                "input prefix storage",
                |storage| {
                    self.budget.refuse(
                        ResourceDimension::InputBytes,
                        ResourceFailure::AllocationFailed,
                        self.policy().limits.max_input_bytes,
                        self.budget.input_bytes(),
                        storage,
                        "input prefix storage",
                    )
                },
            )?;
            self.charge_work(u64_from_index(copied.len()), "copy input prefix")?;
            bytes.extend_from_slice(copied);
        }
        Ok(())
    }

    /// Acquires the remaining input without reading or charging the prefix twice.
    pub fn complete_input<R: Read + ?Sized>(
        &self,
        reader: &mut R,
        bytes: &mut Vec<u8>,
    ) -> Result<(), CodecError> {
        let max = self.policy().limits.max_input_bytes;
        loop {
            self.charge_work(1, "complete input iteration")?;
            let remaining = max
                .checked_sub(u64_from_index(bytes.len()))
                .ok_or_else(|| self.refuse_input_limit(0, "complete input prefix"))?;
            let count = if remaining >= u64_from_index(INPUT_CHUNK) {
                INPUT_CHUNK
            } else {
                usize::try_from(remaining).map_err(|_| {
                    self.refuse_codec_limit(
                        "address input chunk",
                        u64_from_index(INPUT_CHUNK),
                        remaining,
                    )
                })?
            };
            let length = bytes.len().checked_add(count).ok_or_else(|| {
                self.refuse_codec_limit("address complete input", u64_from_index(usize::MAX), max)
            })?;
            self.extend_input_prefix(reader, bytes, length)?;
            if count == 0 || bytes.len() < length {
                break;
            }
        }
        if self.probe_input_end(reader, "input end probe")? {
            return Err(self.refuse_input_limit(1, "complete input"));
        }
        Ok(())
    }

    /// Admits one end probe and validates the reader's reported byte count.
    /// Returns whether input remains without retaining or admitting that byte.
    pub fn probe_input_end<R: Read + ?Sized>(
        &self,
        reader: &mut R,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.charge_work(1, operation)?;
        match reader.read(&mut [0_u8; 1])? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::Io(std::io::ErrorKind::InvalidData.into())),
        }
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

    struct OverreportedCount(usize);

    impl std::io::Read for OverreportedCount {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Ok(self.0)
        }
    }

    #[test]
    fn input_prefix_rejects_overreported_counts_before_charge_or_growth() {
        for reported in [2, usize::MAX] {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
                .expect("empty root");
            let mut bytes = vec![b'p'];
            let capacity = bytes.capacity();
            let error = ctx
                .extend_input_prefix(&mut OverreportedCount(reported), &mut bytes, 2)
                .expect_err("count exceeds the one-byte read window");
            assert!(
                matches!(error, CodecError::Io(error) if error.kind() == std::io::ErrorKind::InvalidData)
            );
            assert_eq!(bytes, b"p");
            assert_eq!(bytes.capacity(), capacity);
            assert_eq!(ctx.budget.input_bytes(), 0);
            assert_eq!(ctx.resource_refusal(), None);
        }
    }

    #[test]
    fn input_completion_rejects_overreported_probe_counts() {
        for reported in [2, usize::MAX] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_input_bytes = 0;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut bytes = Vec::new();
            let error = ctx
                .complete_input(&mut OverreportedCount(reported), &mut bytes)
                .expect_err("count exceeds the one-byte end probe");
            assert!(
                matches!(error, CodecError::Io(error) if error.kind() == std::io::ErrorKind::InvalidData)
            );
            assert!(bytes.is_empty());
            assert_eq!(bytes.capacity(), 0);
            assert_eq!(ctx.budget.input_bytes(), 0);
            assert_eq!(ctx.resource_refusal(), None);
        }
    }

    #[test]
    fn input_growth_refuses_overlap_before_reserving_or_copying() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut reader = Cursor::new(b"ab");
        let mut bytes = ctx.read_input_prefix(&mut reader, 1).expect("first byte");
        let capacity = bytes.capacity();
        let CodecError::ResourceLimit(limit) = ctx
            .extend_input_prefix(&mut reader, &mut bytes, 2)
            .expect_err("old allocation overlaps the new allocation")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.operation, "input prefix storage");
        assert_eq!(limit.additional, 1);
        assert_eq!(bytes, b"a");
        assert_eq!(bytes.capacity(), capacity);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }

    #[test]
    fn input_growth_charges_moves_and_releases_the_overlap() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 1;
        // Two visits, two read bytes, one old-allocation byte and two copied bytes.
        policy.limits.max_work_units = 7;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut reader = Cursor::new(b"ab");
        let mut bytes = ctx.read_input_prefix(&mut reader, 1).expect("first byte");
        ctx.extend_input_prefix(&mut reader, &mut bytes, 2)
            .expect("second byte");
        assert_eq!(bytes, b"ab");
        assert_eq!(ctx.budget.input_bytes(), 2);
        assert!(ctx.reserve_scoped(1, "released overlap").is_ok());
        let CodecError::ResourceLimit(limit) = ctx
            .charge_work(1, "work probe")
            .expect_err("exact work total")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit.used, 7);
    }

    #[test]
    fn input_growth_refuses_move_work_before_reserving() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The first read uses three units; the next visit and read use two.
        policy.limits.max_work_units = 5;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut reader = Cursor::new(b"ab");
        let mut bytes = ctx.read_input_prefix(&mut reader, 1).expect("first byte");
        let capacity = bytes.capacity();
        let CodecError::ResourceLimit(limit) = ctx
            .extend_input_prefix(&mut reader, &mut bytes, 2)
            .expect_err("the old allocation move needs another unit")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "input prefix storage");
        assert_eq!(limit.used, 5);
        assert_eq!(bytes, b"a");
        assert_eq!(bytes.capacity(), capacity);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }

    #[test]
    fn input_prefix_iteration_refuses_before_read() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test operation is admitted");
        let mut reader = Cursor::new(b"a");
        assert!(matches!(ctx.read_input_prefix(&mut reader, 1),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits));
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn input_completion_iteration_refuses_before_read_or_growth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut reader = Cursor::new(b"a");
        let mut bytes = Vec::new();
        let CodecError::ResourceLimit(first) = ctx
            .complete_input(&mut reader, &mut bytes)
            .expect_err("iteration refusal")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "complete input iteration");
        assert_eq!(reader.position(), 0);
        assert!(bytes.is_empty());
        assert_eq!(bytes.capacity(), 0);
        let CodecError::ResourceLimit(repeated) =
            ctx.charge_work(1, "later").expect_err("original refusal")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
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

    #[test]
    fn complete_input_preserves_large_policy_and_prefix() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_input_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let source = vec![b'x'; 9000];
        let mut reader = Cursor::new(&source);
        let mut bytes = ctx.read_input_prefix(&mut reader, 7).expect("prefix");
        ctx.complete_input(&mut reader, &mut bytes)
            .expect("complete input");
        assert_eq!(bytes, source);
        assert_eq!(ctx.budget.input_bytes(), 9000);
    }

    #[test]
    fn complete_input_refusal_keeps_input_dimension() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_input_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut bytes = Vec::new();
        assert!(
            matches!(ctx.complete_input(&mut Cursor::new(b"abc"), &mut bytes),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::InputBytes
                && limit.used == 2 && limit.additional == 1)
        );
        assert_eq!(bytes, b"ab");
    }
}
