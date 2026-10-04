// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    DirectionAboveEpsilon, FinitePoint2, NonzeroPoint2, OrthonormalFrame3, UnitVector2, UnitVector3,
};

rewrite_scalar!(DirectionAboveEpsilon);
rewrite_scalar!(FinitePoint2);
rewrite_scalar!(NonzeroPoint2);
rewrite_scalar!(OrthonormalFrame3);
rewrite_scalar!(UnitVector2);
rewrite_scalar!(UnitVector3);

impl<const N: usize> crate::schema::rewrite::typed::RewriteIdentities for super::FiniteVector<N> {
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.charge_work(1, "walk typed reference scalar")
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        ctx.charge_work(1, "rewrite finite vector")?;
        Ok(self)
    }
}
impl<const N: usize> crate::schema::rewrite::typed::RewriteIdentities for super::NonzeroVector<N> {
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.charge_work(1, "walk typed reference scalar")
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        ctx.charge_work(1, "rewrite nonzero vector")?;
        Ok(self)
    }
}
