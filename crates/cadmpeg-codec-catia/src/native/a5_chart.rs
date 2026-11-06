// SPDX-License-Identifier: Apache-2.0
//! Source-framed A5 parameter chart boundaries.

use std::collections::HashMap;

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;
use serde::{Deserialize, Serialize};

use crate::families::consolidated::a5_chart::{
    a5_chart_boundaries, chart_bounds, chart_side_payload, ChartAxis,
};
use crate::wire::compact_unsigned::CompactUnsigned;
use crate::wire::records::{ConsolidatedRawFrame, ConsolidatedRecord, WidthCodedToken};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ChartBoundaryWire")]
pub(crate) struct CatiaConsolidatedA5Chart {
    pub(super) id: String,
    pub(super) source_index: u64,
    pub(super) source_offset: u64,
    carrier_byte_offset: u64,
    carrier_source_len: u64,
    sides: [ChartSideFrame; 4],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChartBoundaryWire {
    id: String,
    source_index: u64,
    source_offset: u64,
    carrier_byte_offset: u64,
    carrier_source_len: u64,
    sides: [ChartSideFrame; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChartSideFrame {
    source_offset: u64,
    operand: CompactUnsigned,
    axis: ChartAxis,
    fixed: FiniteReal,
    interval: FiniteVector<2>,
    parameter_range: FiniteVector<2>,
    origin: FiniteVector<2>,
    direction: FiniteVector<2>,
    sign_ambiguous: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reverse_interval"
    )]
    reverse_interval: Option<FiniteVector<2>>,
    frame: ConsolidatedRawFrame<u64>,
}

impl TryFrom<ChartBoundaryWire> for CatiaConsolidatedA5Chart {
    type Error = &'static str;

    fn try_from(wire: ChartBoundaryWire) -> Result<Self, Self::Error> {
        let mut next = wire
            .source_offset
            .checked_add(wire.carrier_source_len)
            .ok_or("chart carrier source range overflow")?;
        let mut expected = 0u32;
        let mut parsed = Vec::new();
        for side in &wire.sides {
            if wire.carrier_source_len == 0 || next != side.source_offset {
                return Err("chart sides do not follow the carrier in source order");
            }
            let increment = WidthCodedToken::new(side.frame.width(), side.frame.header_token())
                .ok()
                .and_then(WidthCodedToken::compact_operand)
                .map(CompactUnsigned::value)
                .filter(|value| *value != 0)
                .ok_or("chart header is not a positive compact increment")?;
            expected = expected
                .checked_add(increment)
                .ok_or("chart operand overflow")?;
            let parsed_side = chart_side_payload(side.frame.payload.as_ref(), 0, expected)
                .ok_or("invalid chart side payload")?;
            if parsed_side.operand != side.operand
                || parsed_side.axis != side.axis
                || parsed_side.fixed != side.fixed.get()
                || parsed_side.interval != side.interval.get()
                || parsed_side.reverse_interval != side.reverse_interval.map(FiniteVector::get)
                || parsed_side.line.parameter_range != side.parameter_range.get()
                || parsed_side.line.origin != side.origin.get()
                || parsed_side.line.direction != side.direction.get()
                || parsed_side.line.sign_ambiguous != side.sign_ambiguous
            {
                return Err("chart side does not match its source frame");
            }
            parsed.push(parsed_side);
            let payload_len = u64::try_from(side.frame.payload.len())
                .map_err(|_| "chart payload length overflow")?;
            if payload_len > u64::from(u8::MAX) {
                return Err("chart payload exceeds its B-family frame");
            }
            let frame_len = 4 + u64::from(u8::from(side.frame.width())) + payload_len;
            next = side
                .source_offset
                .checked_add(frame_len)
                .ok_or("chart side source range overflow")?;
            side.frame
                .pos
                .checked_add(frame_len)
                .ok_or("chart side physical range overflow")?;
        }
        let sides: [crate::families::consolidated::a5_chart::ChartSide; 4] =
            parsed.try_into().map_err(|_| "chart requires four sides")?;
        if chart_bounds(&sides).is_none() {
            return Err("chart sides do not define a bounded rectangle");
        }
        Ok(Self {
            id: wire.id,
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            carrier_byte_offset: wire.carrier_byte_offset,
            carrier_source_len: wire.carrier_source_len,
            sides: wire.sides,
        })
    }
}

pub(super) fn derive(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedA5Chart>, CodecError> {
    let by_position = records
        .iter()
        .map(|record| ((record.source_index(), record.source_range().start), record))
        .collect::<HashMap<_, _>>();
    let mut result = Vec::new();
    for chart in a5_chart_boundaries(bytes, records) {
        let carrier = by_position
            .get(&(chart.source_index, chart.source_offset))
            .ok_or_else(|| CodecError::malformed("chart carrier has no source frame"))?;
        let mut sides = Vec::new();
        for side in chart.sides {
            let record = by_position
                .get(&(chart.source_index, side.source_offset))
                .ok_or_else(|| CodecError::malformed("chart side has no source frame"))?;
            let payload = record
                .payload()
                .ok_or_else(|| CodecError::malformed("chart side has no bounded payload"))?;
            sides.push(ChartSideFrame {
                source_offset: u64_from_index(record.source_range().start),
                operand: side.operand,
                axis: side.axis,
                fixed: FiniteReal::new(side.fixed)
                    .ok_or_else(|| CodecError::malformed("non-finite chart side"))?,
                interval: FiniteVector::new(side.interval)
                    .ok_or_else(|| CodecError::malformed("non-finite chart interval"))?,
                parameter_range: FiniteVector::new(side.line.parameter_range)
                    .ok_or_else(|| CodecError::malformed("non-finite chart parameter range"))?,
                origin: FiniteVector::new(side.line.origin)
                    .ok_or_else(|| CodecError::malformed("non-finite chart origin"))?,
                direction: FiniteVector::new(side.line.direction)
                    .ok_or_else(|| CodecError::malformed("non-finite chart direction"))?,
                sign_ambiguous: side.line.sign_ambiguous,
                reverse_interval: side
                    .reverse_interval
                    .map(|range| {
                        FiniteVector::new(range).ok_or_else(|| {
                            CodecError::malformed("non-finite reverse chart interval")
                        })
                    })
                    .transpose()?,
                frame: ConsolidatedRawFrame::from_record(record, bytes[payload].to_vec())?.into(),
            });
        }
        let sides = sides
            .try_into()
            .map_err(|_| CodecError::malformed("chart requires four source sides"))?;
        let wire = ChartBoundaryWire {
            id: format!("catia:consolidated:a5-chart#{:00}", result.len()),
            source_index: u64_from_index(chart.source_index),
            source_offset: u64_from_index(carrier.source_range().start),
            carrier_byte_offset: u64_from_index(chart.carrier_pos),
            carrier_source_len: u64_from_index(carrier.source_range().len()),
            sides,
        };
        result.push(CatiaConsolidatedA5Chart::try_from(wire).map_err(CodecError::malformed)?);
    }
    Ok(result)
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_reverse_interval,
    FiniteVector<2>,
    "reverse_interval"
);

#[cfg(test)]
mod tests;
