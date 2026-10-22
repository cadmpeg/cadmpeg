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
    type Error = CodecError;
    type Depth<'scope>
        = DepthGuard<'scope>
    where
        Self: 'scope;

    fn enter(&self) -> Result<DepthGuard<'_>, CodecError> {
        self.enter_nested("walk document digest")
    }
    fn record(&self) -> Result<(), CodecError> {
        Ok(())
    }
    fn resource(error: CodecError) -> Option<ResourceLimit> {
        match error {
            CodecError::ResourceLimit(limit) => Some(limit),
            _ => None,
        }
    }
}

/// Entity admission for the serializer, without adding a decode-depth gate.
pub(super) struct EncodeAdmission<'a, 'arena>(pub &'a DecodeContext<'arena>);

impl Admission for EncodeAdmission<'_, '_> {
    type Error = CodecError;
    type Depth<'scope>
        = ()
    where
        Self: 'scope;

    fn enter(&self) -> Result<(), CodecError> {
        Ok(())
    }
    fn record(&self) -> Result<(), CodecError> {
        self.0.charge_entities(1, "serialize CADIR record")
    }
    fn resource(error: CodecError) -> Option<ResourceLimit> {
        <&DecodeContext<'_> as Admission>::resource(error)
    }
}
