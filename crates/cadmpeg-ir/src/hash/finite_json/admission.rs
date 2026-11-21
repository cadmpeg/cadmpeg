// SPDX-License-Identifier: Apache-2.0
//! Typed traversal admission for canonical JSON serialization.

use cadmpeg_core::decode::{DecodeContext, DepthGuard, ResourceLimit};
use cadmpeg_core::CodecError;

pub(in crate::hash) trait Admission {
    type Error;
    type Depth<'scope>
    where
        Self: 'scope;

    fn enter(&self) -> Result<Self::Depth<'_>, Self::Error>;
    fn resource(error: Self::Error) -> Option<ResourceLimit>;
}

/// The depth admission of one canonical JSON walk.
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
}

impl Admission for CanonicalAdmission<'_, '_> {
    type Error = CodecError;
    type Depth<'scope>
        = Option<DepthGuard<'scope>>
    where
        Self: 'scope;

    fn enter(&self) -> Result<Option<DepthGuard<'_>>, CodecError> {
        match self.context {
            Some((ctx, operation)) => ctx.enter_nested(operation).map(Some),
            None => Ok(None),
        }
    }
    fn resource(error: CodecError) -> Option<ResourceLimit> {
        match error {
            CodecError::ResourceLimit(limit) => Some(limit),
            _ => None,
        }
    }
}
