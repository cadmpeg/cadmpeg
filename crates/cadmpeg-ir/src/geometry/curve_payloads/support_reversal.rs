// SPDX-License-Identifier: Apache-2.0
//! Support contexts of payload constructions on the reversed parameter `-t`.

use super::super::support_reversal::{reverse_cache_first_form, reversed_side};
use super::{
    SilhouetteCurveConstruction, SpringCurvePayload, SurfaceOffsetCurveConstruction,
    ThreeSurfaceIntersectionCurvePayload, TwoSidedOffsetCurveConstruction,
};
use crate::geometry::{CacheContract, SpringLayout};

impl SpringCurvePayload {
    pub(crate) fn reverse_cache_first_about_zero(&mut self) -> Option<()> {
        let SpringLayout::CacheFirst { context, form } = &mut self.layout else {
            return None;
        };
        let reversed = context.reversed_about_zero()?;
        *context = reversed.clone();
        self.context = reversed;
        reverse_cache_first_form(form);
        Some(())
    }
}

impl ThreeSurfaceIntersectionCurvePayload {
    pub(crate) fn reverse_about_zero(&mut self) -> Option<()> {
        let context = self.context.reversed_about_zero()?;
        let third = reversed_side(&self.third)?;
        self.context = context;
        self.third = third;
        Some(())
    }
}

impl TwoSidedOffsetCurveConstruction {
    pub(crate) fn reverse_about_zero(&mut self) -> Option<()> {
        self.context = self.context.reversed_about_zero()?;
        Some(())
    }
}

impl SilhouetteCurveConstruction {
    pub(crate) fn reverse_about_zero(&mut self) -> Option<()> {
        self.context = self.context.reversed_about_zero()?;
        Some(())
    }
}

impl SurfaceOffsetCurveConstruction {
    /// The base endpoints pair with the context endpoints by position, so
    /// they swap with them. They are base-curve parameters and keep their
    /// values.
    pub(crate) fn reverse_about_zero(&mut self) -> Option<()> {
        self.context = self.context.reversed_about_zero()?;
        self.base_endpoints.reverse();
        if let CacheContract::Revision { form } = &mut self.cache {
            reverse_cache_first_form(form);
        }
        Some(())
    }
}

#[cfg(test)]
mod tests;
