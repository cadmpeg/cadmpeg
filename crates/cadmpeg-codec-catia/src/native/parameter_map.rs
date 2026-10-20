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
        let derived = parameter_map::parse(&wire.payload)
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
    _ctx: &DecodeContext<'_>,
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
            let _work = u64_from_index(data.len()).checked_mul(4).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{} exceeds its local bound",
                    "catia_a8_map_frame_scan"
                ))
            })?;
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
                        if let Some(map) = parameter_map::parse(payload) {
                            let record = CatiaSampledParameterMap {
                                id: format!("catia:a8:sampled-parameter-map#{:00}", output.len()),
                                source_index: u64_from_index(source_index),
                                source_offset: u64_from_index(source_offset),
                                byte_offset: u64_from_index(range.start + position),
                                object_id,
                                payload: (payload).to_vec().into(),
                                data: map,
                            };
                            output.push(record);
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
