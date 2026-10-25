// SPDX-License-Identifier: Apache-2.0
//! TopExp indices of the current canonical native topology graph.

use super::{
    indexed_name, source_topology_indices, LocationIdentityError, Locations, ShapePayloadRecord,
    Tables,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(crate) fn for_payload(
    ctx: &DecodeContext<'_>,
    payload: &ShapePayloadRecord,
) -> Result<Vec<(&'static str, usize)>, CodecError> {
    let Some(tables) = Tables::from_payload(payload) else {
        return Ok(Vec::new());
    };
    let indices = (|| {
        let mut locations = Locations::new(ctx, tables)?;
        source_topology_indices(ctx, tables, &mut locations)
    })();
    let indices = match indices {
        Ok(indices) => indices,
        Err(LocationIdentityError::Decode(error)) => return Err(error),
        Err(LocationIdentityError::Bounded) => return Ok(Vec::new()),
    };
    let mut indices = indices
        .into_iter()
        .map(|((kind, _), index)| (indexed_name(kind), index))
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}
