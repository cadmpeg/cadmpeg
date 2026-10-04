// SPDX-License-Identifier: Apache-2.0
//! Typed storage and work admission for model construction.

use std::convert::Infallible;
use std::fmt;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};

/// Admission selected by model construction or reconstruction.
pub trait ModelAdmission {
    /// Refusal produced by the selected storage policy.
    type Error;

    /// Admit traversal work.
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error>;

    /// Compare identity text after admitting the comparison.
    fn equal(&self, left: &str, right: &str, operation: &'static str) -> Result<bool, Self::Error>;

    /// Format a retained rejection message.
    fn text(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, Self::Error>;

    /// Reserve construction slots.
    fn reserve<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), Self::Error>;

    /// Copy the identity stored by a surface carrier.
    fn surface_id(
        &self,
        id: &ProceduralSurfaceId,
        operation: &'static str,
    ) -> Result<ProceduralSurfaceId, Self::Error>;

    /// Copy the identity stored by a curve carrier.
    fn curve_id(
        &self,
        id: &ProceduralCurveId,
        operation: &'static str,
    ) -> Result<ProceduralCurveId, Self::Error>;
}

/// Standard allocation for reconstruction without decode admission.
pub struct StandardAdmission;

impl ModelAdmission for StandardAdmission {
    type Error = Infallible;

    fn work(&self, _count: usize, _operation: &'static str) -> Result<(), Infallible> {
        Ok(())
    }
    fn equal(&self, left: &str, right: &str, _operation: &'static str) -> Result<bool, Infallible> {
        Ok(left == right)
    }
    fn text(
        &self,
        args: fmt::Arguments<'_>,
        _operation: &'static str,
    ) -> Result<String, Infallible> {
        Ok(args.to_string())
    }
    fn reserve<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        _operation: &'static str,
    ) -> Result<(), Infallible> {
        values.reserve(count);
        Ok(())
    }
    fn surface_id(
        &self,
        id: &ProceduralSurfaceId,
        _operation: &'static str,
    ) -> Result<ProceduralSurfaceId, Infallible> {
        Ok(id.clone())
    }
    fn curve_id(
        &self,
        id: &ProceduralCurveId,
        _operation: &'static str,
    ) -> Result<ProceduralCurveId, Infallible> {
        Ok(id.clone())
    }
}

impl ModelAdmission for DecodeContext<'_> {
    type Error = CodecError;

    fn work(&self, count: usize, operation: &'static str) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(count), operation)
    }
    fn equal(&self, left: &str, right: &str, operation: &'static str) -> Result<bool, CodecError> {
        DecodeContext::equal(self, left, right, operation)
    }
    fn text(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        self.format_retained(args, operation)
    }
    fn reserve<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(values, count, operation)
    }
    fn surface_id(
        &self,
        id: &ProceduralSurfaceId,
        operation: &'static str,
    ) -> Result<ProceduralSurfaceId, CodecError> {
        id.try_clone_for_decode(self, operation)
    }
    fn curve_id(
        &self,
        id: &ProceduralCurveId,
        operation: &'static str,
    ) -> Result<ProceduralCurveId, CodecError> {
        id.try_clone_for_decode(self, operation)
    }
}
