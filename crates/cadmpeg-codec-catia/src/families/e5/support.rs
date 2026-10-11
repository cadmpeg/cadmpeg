// SPDX-License-Identifier: Apache-2.0
//! Complete E5 boundary and equivalent-parameter support programs.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeLength, NonZeroReal};
use serde::{Deserialize, Serialize};

const MAP_ARITHMETIC_ULPS: f64 = 32.0;

/// A support's ordered reference facet and its useful parameter interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SupportWire")]
pub(crate) struct E5CurveSupport {
    /// Boundary or two-facet support program.
    pub(crate) kind: E5CurveSupportKind,
    /// Useful limits in the reference facet's parameterization.
    pub(crate) range: [FiniteReal; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupportWire {
    kind: E5CurveSupportKind,
    range: [FiniteReal; 2],
}

impl TryFrom<SupportWire> for E5CurveSupport {
    type Error = &'static str;

    fn try_from(wire: SupportWire) -> Result<Self, Self::Error> {
        let support = Self {
            kind: wire.kind,
            range: wire.range,
        };
        support
            .valid()
            .then_some(support)
            .ok_or("invalid E5 equivalent-parameter support")
    }
}

/// The closed boundary and intersection productions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum E5CurveSupportKind {
    /// One reference facet with the zero-tolerance mode-zero suffix.
    Boundary { reference_curve: u32 },
    /// Reference facet followed by the co-facet and its correspondence program.
    Intersection {
        curves: [u32; 2],
        correspondence: E5EquivalentParameters,
    },
}

/// Independent accuracy, domains and fit fields surrounding a map program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct E5EquivalentParameters {
    /// Geometric accuracy of corresponding points, in millimetres.
    pub(crate) map_accuracy: NonNegativeLength,
    /// Useful co-facet limits.
    pub(crate) co_domain: [FiniteReal; 2],
    /// Affine equation or paired samples.
    pub(crate) program: E5ParameterProgram,
    /// Active reference limits, independently retained from the fixed header.
    pub(crate) active_reference_limits: [FiniteReal; 2],
    /// Fit tolerance, independently retained from mapping accuracy.
    pub(crate) fit_tolerance: NonNegativeLength,
}

/// Map from a co-facet parameter to the reference facet parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum E5ParameterProgram {
    /// `reference = scale * co + offset`.
    Affine {
        scale: NonZeroReal,
        offset: FiniteReal,
    },
    /// Finite corresponding sites. No interpolation rule is assigned.
    Sampled {
        reference_parameters: Vec<FiniteReal>,
        co_parameters: Vec<FiniteReal>,
    },
}

impl E5CurveSupport {
    /// Facets in source order; the reference facet comes first.
    pub(crate) fn pcurves(&self) -> &[u32] {
        match &self.kind {
            E5CurveSupportKind::Boundary { reference_curve } => {
                std::slice::from_ref(reference_curve)
            }
            E5CurveSupportKind::Intersection { curves, .. } => curves,
        }
    }

    /// Whether this support has a second facet.
    pub(crate) fn is_intersection(&self) -> bool {
        matches!(self.kind, E5CurveSupportKind::Intersection { .. })
    }

    fn valid(&self) -> bool {
        if self.range[0] >= self.range[1] {
            return false;
        }
        let E5CurveSupportKind::Intersection {
            correspondence: map,
            ..
        } = &self.kind
        else {
            return true;
        };
        if map.co_domain[0] >= map.co_domain[1]
            || !map
                .active_reference_limits
                .iter()
                .zip(self.range)
                .all(|(a, b)| a.get().to_bits() == b.get().to_bits())
            || map.map_accuracy.get().to_bits() != map.fit_tolerance.get().to_bits()
        {
            return false;
        }
        match &map.program {
            E5ParameterProgram::Affine { scale, offset } => {
                let reference = if scale.get() > 0.0 {
                    self.range
                } else {
                    [self.range[1], self.range[0]]
                };
                map.co_domain.iter().zip(reference).all(|(co, reference)| {
                    let term = scale.get() * co.get();
                    let mapped = term + offset.get();
                    let tolerance = MAP_ARITHMETIC_ULPS
                        * ulp(term).max(ulp(offset.get())).max(ulp(reference.get()));
                    term.is_finite()
                        && mapped.is_finite()
                        && tolerance.is_finite()
                        && (mapped - reference.get()).abs() <= tolerance
                })
            }
            E5ParameterProgram::Sampled {
                reference_parameters,
                co_parameters,
            } => {
                if !(2..=127).contains(&reference_parameters.len())
                    || reference_parameters.len() != co_parameters.len()
                    || !monotone(reference_parameters)
                    || !monotone(co_parameters)
                    || !within(reference_parameters, self.range)
                    || !within(co_parameters, map.co_domain)
                {
                    return false;
                }
                let Some((&first, &last)) = co_parameters.first().zip(co_parameters.last()) else {
                    return false;
                };
                let endpoints = if first < last {
                    [first, last]
                } else {
                    [last, first]
                };
                endpoints
                    .iter()
                    .zip(map.co_domain)
                    .all(|(a, b)| a.get().to_bits() == b.get().to_bits())
            }
        }
    }
}

fn ulp(value: f64) -> f64 {
    let absolute = value.abs();
    if !absolute.is_finite() {
        return f64::INFINITY;
    }
    let bits = absolute.to_bits();
    if bits == f64::MAX.to_bits() {
        absolute - f64::from_bits(bits - 1)
    } else {
        f64::from_bits(bits + 1) - absolute
    }
}

fn monotone(values: &[FiniteReal]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
        || values.windows(2).all(|pair| pair[0] > pair[1])
}

fn within(values: &[FiniteReal], limits: [FiniteReal; 2]) -> bool {
    let tolerance = MAP_ARITHMETIC_ULPS
        * values
            .iter()
            .chain(&limits)
            .map(|v| ulp(v.get()))
            .fold(0.0_f64, f64::max);
    tolerance.is_finite()
        && values.iter().all(|v| {
            v.get() >= limits[0].get() - tolerance && v.get() <= limits[1].get() + tolerance
        })
}

fn finite(view: &mut View<'_>) -> Option<FiniteReal> {
    FiniteReal::new(view.f64_le()?)
}
fn range(view: &mut View<'_>) -> Option<[FiniteReal; 2]> {
    Some([finite(view)?, finite(view)?])
}

fn sample_lane(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    count: usize,
) -> Result<Option<Vec<FiniteReal>>, CodecError> {
    if view.counted(u64_from_index(count), 8).is_none() {
        return Ok(None);
    }
    ctx.charge_retained(u64_from_index(count) * 8, "catia_e5_map_samples")?;
    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, count, "catia_e5_map_samples")?;
    for _ in 0..count {
        let Some(value) = finite(view) else {
            return Ok(None);
        };
        values.push(value);
    }
    Ok(Some(values))
}

/// Admit the complete payload. Unknown or malformed modes remain native-only.
pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    class: u8,
    payload: &[u8],
) -> Result<Option<E5CurveSupport>, CodecError> {
    let count = match class {
        0xc0 => 1,
        0xc1 => 2,
        _ => return Ok(None),
    };
    if payload.first() != Some(&(0x80 + count)) {
        return Ok(None);
    }
    let mut position = 1;
    let mut curves = [0; 2];
    for curve in curves.iter_mut().take(usize::from(count)) {
        let Some(value) = crate::wire::tokens::object_ref(payload, &mut position, false) else {
            return Ok(None);
        };
        *curve = value;
    }
    let mut view = View::over_retained(payload);
    if view.seek(position).is_none() || view.u8() != Some(0x81) {
        return Ok(None);
    }
    let Some(mode) = view.u8() else {
        return Ok(None);
    };
    if view.u8() != Some(0) {
        return Ok(None);
    }
    let Some(reference_range) = range(&mut view) else {
        return Ok(None);
    };
    if count == 1 {
        if mode != 0
            || view.f64_le().is_none_or(|v| v.to_bits() != 0)
            || view.u32_le() != Some(0)
            || !view.is_empty()
        {
            return Ok(None);
        }
        let support = E5CurveSupport {
            kind: E5CurveSupportKind::Boundary {
                reference_curve: curves[0],
            },
            range: reference_range,
        };
        return Ok(support.valid().then_some(support));
    }
    if !matches!(mode, 3 | 5) {
        return Ok(None);
    }
    let Some((map_accuracy, co_domain)) =
        (|| Some((NonNegativeLength::new(view.f64_le()?)?, range(&mut view)?)))()
    else {
        return Ok(None);
    };
    let program = if mode == 3 {
        let Some((scale, offset)) =
            (|| Some((NonZeroReal::new(view.f64_le()?)?, finite(&mut view)?)))()
        else {
            return Ok(None);
        };
        E5ParameterProgram::Affine { scale, offset }
    } else {
        let Some(count) = view
            .u8()
            .and_then(|v| v.checked_sub(0x80))
            .map(usize::from)
            .filter(|n| *n >= 2)
        else {
            return Ok(None);
        };
        let Some(reference_parameters) = sample_lane(ctx, &mut view, count)? else {
            return Ok(None);
        };
        let Some(co_parameters) = sample_lane(ctx, &mut view, count)? else {
            return Ok(None);
        };
        ctx.charge_work(u64_from_index(count) * 8, "catia_e5_map_validation")?;
        E5ParameterProgram::Sampled {
            reference_parameters,
            co_parameters,
        }
    };
    let Some((active_reference_limits, fit_tolerance)) =
        (|| Some((range(&mut view)?, NonNegativeLength::new(view.f64_le()?)?)))()
    else {
        return Ok(None);
    };
    if view.u32_le() != Some(0) || !view.is_empty() {
        return Ok(None);
    }
    let support = E5CurveSupport {
        kind: E5CurveSupportKind::Intersection {
            curves,
            correspondence: E5EquivalentParameters {
                map_accuracy,
                co_domain,
                program,
                active_reference_limits,
                fit_tolerance,
            },
        },
        range: reference_range,
    };
    Ok(support.valid().then_some(support))
}

#[cfg(test)]
mod tests;
