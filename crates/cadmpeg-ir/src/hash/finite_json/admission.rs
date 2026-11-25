// SPDX-License-Identifier: Apache-2.0
//! Typed traversal admission for canonical JSON serialization.

use cadmpeg_core::decode::DecodeContext;

/// The depth admission of one canonical JSON walk.
#[derive(Clone, Copy)]
pub(crate) struct CanonicalAdmission<'ctx, 'input> {
    context: Option<(&'ctx DecodeContext<'input>, &'static str)>,
}

impl<'ctx, 'input> CanonicalAdmission<'ctx, 'input> {
    /// Admits every depth.
    pub(crate) fn unlimited() -> Self {
        Self { context: None }
    }

    /// Enters the depth limit of `ctx` at each nested value, charged to
    /// `operation`.
    pub(crate) fn limited(ctx: &'ctx DecodeContext<'input>, operation: &'static str) -> Self {
        Self {
            context: Some((ctx, operation)),
        }
    }

    pub(super) fn context(&self) -> Option<(&'ctx DecodeContext<'input>, &'static str)> {
        self.context
    }
}
