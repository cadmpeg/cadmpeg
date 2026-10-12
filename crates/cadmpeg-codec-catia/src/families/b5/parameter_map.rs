// SPDX-License-Identifier: Apache-2.0
//! A8 sampled parameter correspondences between source-local curve representations.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use serde::{Deserialize, Serialize};

use crate::wire::tokens::object_ref;

const ROUNDING_ULPS: f64 = 64.0;

/// Complete two-representation map; sample block order is the correspondence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SampledParameterMap {
    pub(crate) reference: u32,
    pub(crate) co_curve: u32,
    pub(crate) reference_domain: IncreasingParameterInterval,
    pub(crate) co_domain: IncreasingParameterInterval,
    pub(crate) fit_tolerance: NonNegativeReal,
    pub(crate) reference_parameters: Vec<FiniteReal>,
    pub(crate) co_parameters: Vec<FiniteReal>,
    pub(crate) active_reference_range: IncreasingParameterInterval,
}

pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<SampledParameterMap>, CodecError> {
    ctx.charge_work(
        u64_from_index(payload.len()),
        "catia_a8_parameter_map_parse",
    )?;
    let Some((reference, co_curve, leading, count, arrays_start)) = layout(payload) else {
        return Ok(None);
    };
    let Some(reference_domain) = IncreasingParameterInterval::between(leading[0], leading[1])
    else {
        return Ok(None);
    };
    let Some(co_domain) = IncreasingParameterInterval::between(leading[3], leading[4]) else {
        return Ok(None);
    };
    let Some(fit_tolerance) = NonNegativeReal::new(leading[2].get()) else {
        return Ok(None);
    };
    let tail = arrays_start + count * 16;
    let Some(active) = crate::wire::bytes::read_f64_array::<3>(payload, tail) else {
        return Ok(None);
    };
    let Some(active_reference_range) = IncreasingParameterInterval::between(active[0], active[1])
    else {
        return Ok(None);
    };
    if active[2].get().to_bits() != fit_tolerance.get().to_bits()
        || active_reference_range.lower() < reference_domain.lower()
        || active_reference_range.upper() > reference_domain.upper()
    {
        return Ok(None);
    }
    let mut view = View::over_retained(&payload[arrays_start..tail]);
    let mut reference_parameters =
        ctx.collection_vec(count, "catia_a8_map_reference_parameters")?;
    let mut co_parameters = ctx.collection_vec(count, "catia_a8_map_co_parameters")?;
    for parameters in [&mut reference_parameters, &mut co_parameters] {
        for _ in 0..count {
            let Some(value) = view.f64_le().and_then(FiniteReal::new) else {
                return Ok(None);
            };
            parameters.push(value);
        }
    }
    if !reference_parameters.windows(2).all(|p| p[0] < p[1])
        || !(co_parameters.windows(2).all(|p| p[0] < p[1])
            || co_parameters.windows(2).all(|p| p[0] > p[1]))
    {
        return Ok(None);
    }
    let scale = leading
        .iter()
        .chain(&active)
        .chain(&reference_parameters)
        .chain(&co_parameters)
        .fold(1.0_f64, |scale, value| scale.max(value.get().abs()));
    let bound = fit_tolerance
        .get()
        .max(ROUNDING_ULPS * f64::EPSILON * scale);
    for (parameters, domain) in [
        (&reference_parameters, reference_domain),
        (&co_parameters, co_domain),
    ] {
        if parameters
            .iter()
            .any(|p| p.get() < domain.lower() - bound || p.get() > domain.upper() + bound)
        {
            return Ok(None);
        }
    }
    Ok(Some(SampledParameterMap {
        reference,
        co_curve,
        reference_domain,
        co_domain,
        fit_tolerance,
        reference_parameters,
        co_parameters,
        active_reference_range,
    }))
}

fn layout(payload: &[u8]) -> Option<(u32, u32, [FiniteReal; 5], usize, usize)> {
    if payload.first() != Some(&0x82) {
        return None;
    }
    let mut position = 1;
    let reference = object_ref(payload, &mut position, true)?;
    let co_curve = object_ref(payload, &mut position, true)?;
    if payload.get(position..position.checked_add(2)?) != Some(&[0x81, 0x15]) {
        return None;
    }
    position += 2;
    let leading = crate::wire::bytes::read_f64_array::<5>(payload, position)?;
    position += 40;
    let count = usize::try_from(object_ref(payload, &mut position, true)?).ok()?;
    if count < 2 {
        return None;
    }
    let end = position
        .checked_add(count.checked_mul(16)?)?
        .checked_add(25)?;
    (end == payload.len() && payload.last() == Some(&0x01))
        .then_some((reference, co_curve, leading, count, position))
}

#[cfg(test)]
mod tests;
