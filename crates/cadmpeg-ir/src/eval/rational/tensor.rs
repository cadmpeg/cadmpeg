// SPDX-License-Identifier: Apache-2.0
//! Selected-source centering from the derivative partition identity.

use super::{decode, Homogeneous, ExactSignedSum, FiniteReal};
use crate::features::FinitePoint3;
use crate::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, WeightedPole3};
use crate::scalar::NonZeroReal;
use cadmpeg_core::decode::ResourceLimit;

/// A borrowed complete active tensor support. No sampled derivative proves
/// a constant: centering uses actual selected source control values.
pub(in crate::eval) struct TensorWindow<'a> {
    surface: &'a NurbsSurface,
    start: [usize; 2],
}

impl<'a> TensorWindow<'a> {
    pub(in crate::eval) const fn new(surface: &'a NurbsSurface, start: [usize; 2]) -> Self {
        Self { surface, start }
    }

    pub(in crate::eval) fn pole(&self, [u, v]: [usize; 2]) -> Option<WeightedPole3<FinitePoint3>> {
        let u = self.start[0].checked_add(u)?;
        let v = self.start[1].checked_add(v)?;
        match self.surface.pole_grid() {
            NurbsPoleGrid::Polynomial { rows } => Some(WeightedPole3 {
                point: *rows.get(u)?.get(v)?, weight: NonZeroReal::ONE,
            }),
            NurbsPoleGrid::Rational { rows } => rows.get(u)?.get(v).copied(),
        }
    }

    /// For a positive u order subtract Q_0j; for a positive v order
    /// subtract Q_i0. For both add Q_00. Each derivative partition sums0.
    /// Expand w*P differences as signed four-factor products before rounding.
    pub(in crate::eval) fn add_partial<const M: usize>(
        &self,
        sums: &mut [ExactSignedSum; M],
        basis: [f64; 2],
        at: [usize; 2],
        pole: WeightedPole3<FinitePoint3>,
        differentiate: [bool; 2],
    ) -> Option<()> {
        const { assert!(M == 3 || M == 4) };
        FiniteReal::array(basis)?;
        let mut add = |pole: WeightedPole3<FinitePoint3>, negative: bool| {
            let u = if negative { -basis[0] } else { basis[0] };
            for (sum, coordinate) in sums.iter_mut().zip([
                pole.point.x, pole.point.y, pole.point.z, 1.0,
            ]) {
                sum.add_factors([u, basis[1], pole.weight.get(), coordinate]);
            }
        };
        add(pole, false);
        if differentiate[0] { add(self.pole([0, at[1]])?, true); }
        if differentiate[1] { add(self.pole([at[0], 0])?, true); }
        if differentiate == [true; 2] { add(self.pole([0, 0])?, false); }
        Some(())
    }
}

impl Homogeneous {
    /// All four homogeneous derivative lanes in one admitted source walk.
    pub(in crate::eval) fn surface_derivative(
        scratch: &decode::Scratch<'_, '_>,
        window: &TensorWindow<'_>,
        axes: [&[f64]; 2],
        differentiate: [bool; 2],
    ) -> Result<Option<Self>, ResourceLimit> {
        const OPERATION: &str = "IR homogeneous derivative pole traversal";
        scratch.admission.work(0, OPERATION)?;
        let [u, v] = axes;
        let Some(count) = u.len().checked_mul(v.len()) else { return Ok(None); };
        let mut sums: [ExactSignedSum; 4] = std::array::from_fn(|_| ExactSignedSum::default());
        for local in 0..count {
            scratch.admission.work(u64::from(count > 2), OPERATION)?;
            let at = [local / v.len(), local % v.len()];
            let Some(pole) = window.pole(at) else { return Ok(None); };
            if window.add_partial(&mut sums, [u[at[0]], v[at[1]]], at, pole, differentiate).is_none() {
                return Ok(None);
            }
        }
        Ok(Some(Self { values: sums.map(ExactSignedSum::finish), constant: [None; 3] }))
    }
}
