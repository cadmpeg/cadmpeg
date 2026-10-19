// SPDX-License-Identifier: Apache-2.0
//! Typed traversal admission for canonical JSON serialization.

use std::convert::Infallible;

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

pub(super) struct StandardAdmission;

impl Admission for StandardAdmission {
    type Error = Infallible;
    type Depth<'scope> = ();

    fn enter(&self) -> Result<(), Infallible> {
        Ok(())
    }
    fn resource(error: Infallible) -> Option<ResourceLimit> {
        match error {}
    }
}

impl Admission for &DecodeContext<'_> {
    type Error = CodecError;
    type Depth<'scope>
        = DepthGuard<'scope>
    where
        Self: 'scope;

    fn enter(&self) -> Result<DepthGuard<'_>, CodecError> {
        self.enter_nested("walk document digest")
    }
    fn resource(error: CodecError) -> Option<ResourceLimit> {
        match error {
            CodecError::ResourceLimit(limit) => Some(limit),
            _ => None,
        }
    }
}
