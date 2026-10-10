// SPDX-License-Identifier: Apache-2.0
//! Hash keys that charge the owning session before hashing or equality.

use super::equal;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use std::borrow::Cow;
use std::hash::{Hash, Hasher};

/// A cache key whose hashing and equality charge its owning session.
#[derive(Debug)]
pub(crate) struct Key<'ctx> {
    text: Cow<'ctx, str>,
    ctx: &'ctx DecodeContext<'ctx>,
    operation: &'static str,
}

impl<'ctx> Key<'ctx> {
    pub(crate) fn owned(
        ctx: &'ctx DecodeContext<'_>,
        text: String,
        operation: &'static str,
    ) -> Self {
        Self {
            text: Cow::Owned(text),
            ctx,
            operation,
        }
    }

    pub(crate) fn borrowed(
        ctx: &'ctx DecodeContext<'_>,
        text: &'ctx str,
        operation: &'static str,
    ) -> Self {
        Self {
            text: Cow::Borrowed(text),
            ctx,
            operation,
        }
    }
}

impl PartialEq for Key<'_> {
    fn eq(&self, other: &Self) -> bool {
        // The owner checks the fused session before exposing any table result.
        equal(self.ctx, &self.text, &other.text, self.operation).unwrap_or(false)
    }
}
impl Eq for Key<'_> {}
impl Hash for Key<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if self.ctx.charge_work_limit(1, self.operation).is_err()
            || self
                .ctx
                .charge_work_limit(u64_from_index(self.text.len()), self.operation)
                .is_err()
        {
            return;
        }
        self.text.hash(state);
    }
}
