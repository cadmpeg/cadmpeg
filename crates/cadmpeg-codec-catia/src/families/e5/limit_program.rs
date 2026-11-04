// SPDX-License-Identifier: Apache-2.0
//! Retain limit programs independently of their geometric admission.

use cadmpeg_core::decode::View;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CurveLimits {
    pub(crate) lower_arc: u16,
    pub(crate) upper_arc: u16,
    pub(crate) lower_local_bits: u64,
    pub(crate) upper_local_bits: u64,
    pub(crate) coefficient_bits: u64,
    pub(crate) shift_bits: u64,
    pub(crate) extrapolation_control: u8,
}

impl CurveLimits {
    pub(crate) const LEN: usize = crate::layout::e5_nurbs_pcurve_limits::LEN;

    pub(crate) fn read(view: &mut View<'_>) -> Option<Self> {
        if view.remaining() != Self::LEN {
            return None;
        }
        Some(Self {
            lower_arc: view.u16_le()?,
            upper_arc: view.u16_le()?,
            lower_local_bits: view.u64_le()?,
            upper_local_bits: view.u64_le()?,
            coefficient_bits: view.u64_le()?,
            shift_bits: view.u64_le()?,
            extrapolation_control: view.u8()?,
        })
    }

    pub(crate) fn is_disabled_identity(&self) -> bool {
        f64::from_bits(self.coefficient_bits) == 1.0
            && f64::from_bits(self.shift_bits) == 0.0
            && self.extrapolation_control == 0
    }

    pub(crate) fn arcs(&self) -> [u16; 2] {
        [self.lower_arc, self.upper_arc]
    }

    pub(crate) fn locals(&self) -> [f64; 2] {
        [
            f64::from_bits(self.lower_local_bits),
            f64::from_bits(self.upper_local_bits),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct SuffixControl(pub(crate) u8);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SurfaceLimits {
    pub(crate) patches: [u16; 4],
    pub(crate) local_bits: [u64; 4],
    pub(crate) affine_bits: [u64; 4],
    pub(crate) extrapolation_flags: [u16; 3],
    pub(crate) continuation_bits: [u64; 8],
    pub(crate) suffix_controls: [SuffixControl; 6],
}

impl SurfaceLimits {
    pub(crate) const LEN: usize = crate::layout::e5_nurbs_surface_limits::LEN;

    pub(crate) fn read(view: &mut View<'_>) -> Option<Self> {
        if view.remaining() != Self::LEN {
            return None;
        }
        let patches = read_u16s(view)?;
        let local_bits = read_u64s(view)?;
        let affine_bits = read_u64s(view)?;
        let extrapolation_flags = read_u16s(view)?;
        let continuation_bits = read_u64s(view)?;
        let mut suffix_controls = [SuffixControl(0); 6];
        for value in &mut suffix_controls {
            *value = SuffixControl(view.u8()?);
        }
        Some(Self {
            patches,
            local_bits,
            affine_bits,
            extrapolation_flags,
            continuation_bits,
            suffix_controls,
        })
    }

    pub(crate) fn is_disabled_identity(&self) -> bool {
        self.affine_bits.map(f64::from_bits) == [1.0, 0.0, 1.0, 0.0]
            && self.extrapolation_flags == [0; 3]
            && self
                .continuation_bits
                .iter()
                .all(|word| f64::from_bits(*word) == 0.0)
            && self.suffix_controls.iter().all(|control| control.0 == 0)
    }

    pub(crate) fn locals(&self) -> [f64; 4] {
        self.local_bits.map(f64::from_bits)
    }
}

fn read_u16s<const N: usize>(view: &mut View<'_>) -> Option<[u16; N]> {
    let mut values = [0; N];
    for value in &mut values {
        *value = view.u16_le()?;
    }
    Some(values)
}

fn read_u64s<const N: usize>(view: &mut View<'_>) -> Option<[u64; N]> {
    let mut values = [0; N];
    for value in &mut values {
        *value = view.u64_le()?;
    }
    Some(values)
}

#[cfg(test)]
mod tests;
