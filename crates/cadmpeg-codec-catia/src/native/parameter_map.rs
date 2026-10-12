// SPDX-License-Identifier: Apache-2.0
//! Exact A8 sampled-map frames and their checked native projection.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use crate::families::b5::parameter_map::{self, SampledParameterMap};
use crate::wire::records::{ConsolidatedRecord, SourceExtent};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ParameterMapWire")]
pub(crate) struct CatiaSampledParameterMap {
    pub(super) id: String,
    pub(super) source_index: u64,
    pub(super) source_offset: u64,
    pub(super) byte_offset: u64,
    object_id: u32,
    payload: NativeBytes<Vec<u8>>,
    data: SampledParameterMap,
}

#[derive(Deserialize)]
struct ParameterMapWire {
    id: String,
    source_index: u64,
    source_offset: u64,
    byte_offset: u64,
    object_id: u32,
    payload: NativeBytes<Vec<u8>>,
    data: SampledParameterMap,
}

impl TryFrom<ParameterMapWire> for CatiaSampledParameterMap {
    type Error = String;

    fn try_from(wire: ParameterMapWire) -> Result<Self, Self::Error> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .map_err(|error| error.to_string())?;
        let derived = parameter_map::parse(&ctx, &wire.payload)
            .map_err(|error| error.to_string())?
            .ok_or("native sampled-map payload is not complete")?;
        if serde_json::to_vec(&wire.data).map_err(|error| error.to_string())?
            != serde_json::to_vec(&derived).map_err(|error| error.to_string())?
        {
            return Err("native sampled-map data differs from its payload".to_owned());
        }
        Ok(Self {
            id: wire.id,
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            byte_offset: wire.byte_offset,
            object_id: wire.object_id,
            payload: wire.payload,
            data: derived,
        })
    }
}

pub(super) fn derive(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    sources: &[Vec<SourceExtent>],
    consolidated: &[ConsolidatedRecord],
) -> Result<Vec<CatiaSampledParameterMap>, CodecError> {
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
                    crate::families::b5::graph::object_frame(data, position)
                {
                    if family == 0xa8 && class == 0x25 && data[position + 1] == 0x03 {
                        let payload = &data[position + 11..end];
                        let (parsed, reservation) = ctx
                            .with_scoped_storage("catia_a8_map_candidate", || {
                                parameter_map::parse(ctx, payload)
                            })?;
                        if let Some(map) = parsed {
                            reservation.commit()?;
                            let record = CatiaSampledParameterMap {
                                id: ctx.format_retained(
                                    format_args!(
                                        "catia:a8:sampled-parameter-map#{:00}",
                                        output.len()
                                    ),
                                    "catia_native_a8_parameter_map_id",
                                )?,
                                source_index: u64_from_index(source_index),
                                source_offset: u64_from_index(source_offset),
                                byte_offset: u64_from_index(range.start + position),
                                object_id,
                                payload: ctx.copy_retained(payload, "catia_a8_map_payload")?.into(),
                                data: map,
                            };
                            ctx.push_vec(&mut output, record, "catia_native_a8_parameter_maps")?;
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
