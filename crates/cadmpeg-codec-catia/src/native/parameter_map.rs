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
    let (records, temporary) = ctx.with_scoped_storage("catia_a8_map_workspace", || {
        parameter_map::collect(ctx, bytes, sources, consolidated)
    })?;
    // Parsed arrays and payloads become the native records' retained storage.
    temporary.commit()?;
    for record in records {
        let value = CatiaSampledParameterMap {
            id: ctx.format_retained(
                format_args!("catia:a8:sampled-parameter-map#{:00}", output.len()),
                "catia_native_a8_parameter_map_id",
            )?,
            source_index: u64_from_index(record.source_index),
            source_offset: u64_from_index(record.source_offset),
            byte_offset: u64_from_index(record.byte_offset),
            object_id: record.object_id,
            payload: record.payload.into(),
            data: record.data,
        };
        ctx.push_vec(&mut output, value, "catia_native_a8_parameter_maps")?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
