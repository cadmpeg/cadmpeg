// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Brent checkpoints detect a repeated owner with fixed-size state.
pub(super) struct OwnerCycle {
    checkpoint: i64,
    span: usize,
    steps: usize,
    cycle_len: Option<usize>,
}

impl OwnerCycle {
    pub(super) const fn new(first: i64) -> Self {
        Self {
            checkpoint: first,
            span: 1,
            steps: 0,
            cycle_len: None,
        }
    }

    /// Advance after appending the current owner to the admitted path.
    pub(super) fn advance(&mut self, next: Option<i64>) -> Result<bool, CodecError> {
        let Some(next) = next else {
            return Ok(false);
        };
        self.steps += 1;
        if next == self.checkpoint {
            self.cycle_len = Some(self.steps);
            return Ok(true);
        }
        if self.steps == self.span {
            let span = self.span.checked_mul(2).ok_or_else(|| {
                CodecError::Malformed("ASM owner cycle checkpoint span overflows".into())
            })?;
            self.checkpoint = next;
            self.span = span;
            self.steps = 0;
        }
        Ok(false)
    }

    /// Keep the prefix and one cycle before reserving memo slots.
    pub(super) fn trim_path(
        &self,
        ctx: &DecodeContext<'_>,
        path: &mut Vec<i64>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let Some(cycle_len) = self.cycle_len else {
            return Ok(());
        };
        if path.len() == cycle_len {
            return Ok(());
        }
        let prefix = ctx.position_by(
            path.iter().zip(path[cycle_len..].iter()),
            |(first, repeated)| Ok(first == repeated),
            operation,
        )?;
        if let Some(prefix) = prefix {
            path.truncate(prefix + cycle_len);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cyclic_path(prefix: usize, cycle_len: usize) -> (OwnerCycle, Vec<i64>) {
        let unique = prefix + cycle_len;
        let mut owner = 0;
        let mut cycle = OwnerCycle::new(0);
        let mut path = Vec::new();
        for _ in 0..4 * unique {
            path.push(i64::try_from(owner).unwrap());
            owner = if owner + 1 < unique {
                owner + 1
            } else {
                prefix
            };
            if cycle.advance(Some(i64::try_from(owner).unwrap())).unwrap() {
                return (cycle, path);
            }
        }
        panic!("finite owner cycle must be detected");
    }

    #[test]
    fn owner_cycle_rejects_checkpoint_span_overflow() {
        let span = usize::MAX / 2 + 1;
        let mut cycle = OwnerCycle {
            checkpoint: 0,
            span,
            steps: span - 1,
            cycle_len: None,
        };
        assert!(matches!(cycle.advance(Some(1)),
            Err(CodecError::Malformed(message))
                if message == "ASM owner cycle checkpoint span overflows"));
        assert_eq!(cycle.checkpoint, 0);
        assert_eq!(cycle.span, span);
        assert!(cycle.cycle_len.is_none());
    }

    #[test]
    fn owner_cycle_checkpoints_keep_only_the_unique_path() {
        for (prefix, cycle_len) in [(0, 1), (0, 4096), (7, 3), (4096, 1), (4096, 4096)] {
            let (cycle, mut path) = cyclic_path(prefix, cycle_len);
            let ctx = cadmpeg_test_support::service_decode_context();
            cycle
                .trim_path(&ctx, &mut path, "test owner cycle trim")
                .unwrap();
            assert_eq!(
                path,
                (0..prefix + cycle_len)
                    .map(|index| i64::try_from(index).unwrap())
                    .collect::<Vec<_>>()
            );
            ctx.finish_session().unwrap();
        }
    }

    #[test]
    fn owner_cycle_trim_refuses_before_the_next_pair() {
        use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
        let (cycle, path) = cyclic_path(7, 3);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "test owner cycle trim",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                cycle.trim_path(&ctx, &mut path.clone(), "test owner cycle trim")
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("cycle trim refusal");
        };
        assert_eq!(limit.operation, "test owner cycle trim");
        assert_eq!(limit.additional, 1);
    }
}
