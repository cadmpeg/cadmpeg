// SPDX-License-Identifier: Apache-2.0
//! TopExp indices of the current canonical native topology graph.

use super::{indexed_name, source_topology_indices, ShapePayloadRecord, Tables};
use cadmpeg_core::CodecError;

pub(crate) fn for_payload(
    payload: &ShapePayloadRecord,
) -> Result<Vec<(&'static str, usize)>, CodecError> {
    let Some(tables) = Tables::from_payload(payload) else {
        return Ok(Vec::new());
    };
    let mut indices = source_topology_indices(tables)?
        .into_iter()
        .map(|((kind, _), index)| (indexed_name(kind), index))
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}
