// SPDX-License-Identifier: Apache-2.0
//! Typed traversal admission for canonical JSON serialization.

use std::convert::Infallible;

use cadmpeg_core::decode::{DecodeContext, DepthGuard, ResourceLimit};
use cadmpeg_core::encode::EncodeSession;

pub(in crate::hash) trait Admission {
    type Error;
    type Depth<'scope>
    where
        Self: 'scope;

    fn enter(&self) -> Result<Self::Depth<'_>, Self::Error>;
    fn resource(error: Self::Error) -> Option<ResourceLimit>;
    fn record(&self) -> Result<(), Self::Error>;
}

pub(super) struct StandardAdmission;

impl Admission for StandardAdmission {
    type Error = Infallible;
    type Depth<'scope> = ();

    fn enter(&self) -> Result<(), Infallible> {
        Ok(())
    }
    fn record(&self) -> Result<(), Infallible> {
        Ok(())
    }
    fn resource(error: Infallible) -> Option<ResourceLimit> {
        match error {}
    }
}

impl Admission for &DecodeContext<'_> {
    type Error = ResourceLimit;
    type Depth<'scope>
        = DepthGuard<'scope>
    where
        Self: 'scope;

    fn enter(&self) -> Result<DepthGuard<'_>, ResourceLimit> {
        self.enter_nested_limit("walk document digest")
    }
    fn record(&self) -> Result<(), ResourceLimit> {
        Ok(())
    }
    fn resource(error: ResourceLimit) -> Option<ResourceLimit> {
        Some(error)
    }
}

/// Entity admission for the serializer, without adding a decode-depth gate.
pub(super) struct EncodeAdmission<'a>(pub &'a EncodeSession);

impl Admission for EncodeAdmission<'_> {
    type Error = ResourceLimit;
    type Depth<'scope>
        = ()
    where
        Self: 'scope;

    fn enter(&self) -> Result<(), ResourceLimit> {
        Ok(())
    }
    fn record(&self) -> Result<(), ResourceLimit> {
        self.0.charge_entities(1, "serialize CADIR record")
    }
    fn resource(error: ResourceLimit) -> Option<ResourceLimit> {
        Some(error)
    }
}
