// SPDX-License-Identifier: Apache-2.0
//! A8 sampled parameter correspondences between source-local curve representations.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use serde::{Deserialize, Serialize};

use crate::wire::records::{ConsolidatedRecord, SourceExtent};
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

/// A parsed map together with its exact bounded A8 frame.
pub(crate) struct ParameterMapRecord {
    pub(crate) source_index: usize,
    pub(crate) source_offset: usize,
    pub(crate) byte_offset: usize,
    pub(crate) object_id: u32,
    pub(crate) payload: Vec<u8>,
    pub(crate) data: SampledParameterMap,
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

pub(crate) fn collect(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    sources: &[Vec<SourceExtent>],
    consolidated: &[ConsolidatedRecord],
) -> Result<Vec<ParameterMapRecord>, CodecError> {
    let mut output = Vec::new();
    for (source_index, source) in sources.iter().enumerate() {
        let mut logical_start = 0usize;
        let mut spanning_end = 0usize;
        for extent in source {
            let range = extent.range();
            let logical_end = logical_start
                .checked_add(range.len())
                .ok_or_else(|| CodecError::malformed("A8 parameter-map source length overflows"))?;
            let data = &bytes[range.clone()];
            let work = u64_from_index(data.len()).checked_mul(4).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_a8_map_frame_scan", u64::MAX - 1, u64::MAX)
            })?;
            ctx.charge_work(work, "catia_a8_map_frame_scan")?;
            let mut position = if spanning_end > logical_start {
                (spanning_end - logical_start).min(data.len())
            } else {
                0
            };
            while position < data.len() {
                let source_offset = logical_start + position;
                if let Some((end, family, class, object_id)) =
                    super::graph::object_frame(data, position)
                {
                    if family == 0xa8 && class == 0x25 && data[position + 1] == 0x03 {
                        let payload = &data[position + 11..end];
                        let (parsed, reservation) = ctx
                            .with_scoped_storage("catia_a8_map_candidate", || {
                                parse(ctx, payload)
                            })?;
                        if let Some(map) = parsed {
                            reservation.commit()?;
                            let record = ParameterMapRecord {
                                source_index,
                                source_offset,
                                byte_offset: range.start + position,
                                object_id,
                                payload: ctx.copy_retained(payload, "catia_a8_map_payload")?,
                                data: map,
                            };
                            ctx.push_vec(&mut output, record, "catia_a8_parameter_map_records")?;
                        }
                    }
                    position = end;
                    continue;
                }
                if let Some(record) =
                    crate::wire::records::parse_consolidated_record(data, position, data.len())
                {
                    if let Some(range) = record.range() {
                        position = range.end;
                        continue;
                    }
                }
                // A consolidated frame may continue in the next physical extent.
                // Only a frame starting at this peer position can suppress it.
                if matches!(data[position], 0xa5..=0xa7 | 0xb2..=0xb4) {
                    ctx.charge_work(
                        u64::from(usize::BITS - consolidated.len().leading_zeros()),
                        "catia_a8_map_spanning_lookup",
                    )?;
                    let index = consolidated.partition_point(|record| {
                        (record.source_index(), record.source_range().start)
                            < (source_index, source_offset)
                    });
                    if let Some(record) = consolidated.get(index).filter(|record| {
                        record.source_index() == source_index
                            && record.source_range().start == source_offset
                            && record.range().is_none()
                    }) {
                        spanning_end = record.source_range().end;
                        position = spanning_end
                            .checked_sub(logical_start)
                            .ok_or_else(|| {
                                CodecError::malformed("spanning frame ends before its source start")
                            })?
                            .min(data.len());
                        continue;
                    }
                }
                position += 1;
            }
            logical_start = logical_end;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
