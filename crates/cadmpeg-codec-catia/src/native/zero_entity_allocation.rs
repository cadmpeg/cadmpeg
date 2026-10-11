// SPDX-License-Identifier: Apache-2.0
//! Persistent zero-entity allocation graph references.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use super::{projection::zero_entity_record, CatiaZeroEntityRecord};
use crate::families::zero_entity::allocation::AllocationTopology;

/// One ordered side-use and its complete source incidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatiaZeroEntityAllocationUse {
    /// Exact `0638` use record; array position selects side one or two.
    oriented_use: String,
    /// Independently selected face-local `21xx` occurrence.
    support_record: String,
    /// Face containing the selected support occurrence.
    face_record: String,
    /// Vertex-star `05xx` incidence containing the backward use distance.
    incidence_record: String,
    /// Adjacent persistent `5d06` vertex owner.
    vertex_record: String,
}

/// A physical edge whose complete allocation graph closes in its source stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatiaZeroEntityAllocationEdge {
    pub(crate) id: String,
    /// Exact `5e1a` physical-edge identity.
    edge_record: String,
    /// Exact `2569` side-pair header.
    header_record: String,
    /// Side one followed by side two, with no partial bindings.
    uses: [CatiaZeroEntityAllocationUse; 2],
}

fn record_id(
    ctx: &DecodeContext<'_>,
    records: &[CatiaZeroEntityRecord],
    ordinal: u32,
) -> Result<String, CodecError> {
    let record = zero_entity_record(records, ordinal).ok_or_else(|| {
        CodecError::malformed("allocation target is absent from its source inventory")
    })?;
    ctx.copy_retained_text(&record.id, "catia_native_zero_allocation_target")
}

pub(crate) fn derive(
    ctx: &DecodeContext<'_>,
    topology: Option<AllocationTopology>,
    records: &[CatiaZeroEntityRecord],
) -> Result<Vec<CatiaZeroEntityAllocationEdge>, CodecError> {
    let Some(topology) = topology else {
        return Ok(Vec::new());
    };
    let mut output = Vec::new();
    for edge in topology.edges {
        let [first, second] = edge.uses.map(|side| -> Result<_, CodecError> {
            Ok(CatiaZeroEntityAllocationUse {
                oriented_use: record_id(ctx, records, side.use_record_ordinal)?,
                support_record: record_id(ctx, records, side.support_record_ordinal)?,
                face_record: record_id(ctx, records, side.face_record_ordinal)?,
                incidence_record: record_id(ctx, records, side.incidence_record_ordinal)?,
                vertex_record: record_id(ctx, records, side.vertex_record_ordinal)?,
            })
        });
        let uses = [first?, second?];
        ctx.push_vec(
            &mut output,
            CatiaZeroEntityAllocationEdge {
                id: ctx.format_retained(
                    format_args!("catia:zero-entity:allocation-edge#{}", edge.record_ordinal),
                    "catia_native_zero_allocation_edge_id",
                )?,
                edge_record: record_id(ctx, records, edge.record_ordinal)?,
                header_record: record_id(ctx, records, edge.header_record_ordinal)?,
                uses,
            },
            "catia_native_zero_allocation_edges",
        )?;
    }
    Ok(output)
}

#[cfg(test)]
pub(super) fn validate(
    edges: &[CatiaZeroEntityAllocationEdge],
    records: &[CatiaZeroEntityRecord],
    strides: &[super::CatiaZeroEntityEdgeStride],
    pairs: &[super::CatiaZeroEntityOrientedUsePair],
    incidences: &[super::CatiaZeroEntityVertexIncidence],
    runs: &[super::CatiaZeroEntitySupportRun],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    use std::collections::HashSet;
    let valid = (|| -> Option<()> {
        if edges.is_empty() {
            return Some(());
        }
        if edges.len() != strides.len() || pairs.len() != edges.len() {
            return None;
        }
        let mut selected_edges = HashSet::new();
        let mut selected_headers = HashSet::new();
        let mut selected_uses = HashSet::new();
        let mut selected_supports = HashSet::new();
        for edge in edges {
            let stride = strides.iter().find(|s| {
                zero_entity_record(records, s.record_ordinal)
                    .is_some_and(|r| r.id == edge.edge_record)
            })?;
            let edge_ordinal = stride.record_ordinal;
            let [header_distance, first_vertex_distance, second_vertex_distance, _, _] =
                stride.allocations;
            let header_ordinal = edge_ordinal.checked_sub(header_distance)?;
            let pair = pairs
                .iter()
                .find(|p| p.header_record_ordinal == header_ordinal)?;
            if edge.id != format!("catia:zero-entity:allocation-edge#{edge_ordinal}")
                || zero_entity_record(records, header_ordinal)?.id != edge.header_record
                || !selected_edges.insert(edge_ordinal)
                || !selected_headers.insert(header_ordinal)
            {
                return None;
            }
            let mut vertices = [0, 0];
            for (index, (&distance, side)) in [first_vertex_distance, second_vertex_distance]
                .iter()
                .zip(&edge.uses)
                .enumerate()
            {
                let use_ordinal =
                    header_ordinal.checked_add(u32::try_from(index).ok()?.checked_add(1)?)?;
                let vertex_ordinal = edge_ordinal.checked_sub(distance)?;
                let incidence = incidences
                    .iter()
                    .find(|i| i.record_ordinal.checked_add(1) == Some(vertex_ordinal))?;
                let use_distance = incidence.record_ordinal.checked_sub(use_ordinal)?;
                if zero_entity_record(records, use_ordinal)?.id != side.oriented_use
                    || pair.uses[index].record_ordinal != use_ordinal
                    || zero_entity_record(records, vertex_ordinal)?.id != side.vertex_record
                    || zero_entity_record(records, incidence.record_ordinal)?.id
                        != side.incidence_record
                    || incidence
                        .allocations
                        .iter()
                        .filter(|&&d| d == use_distance)
                        .count()
                        != 1
                    || !selected_uses.insert(use_ordinal)
                {
                    return None;
                }
                vertices[index] = vertex_ordinal;
                let base = header_distance.checked_add(pair.base_columns[index])?;
                let matches = runs
                    .iter()
                    .filter_map(|run| run.face.as_ref())
                    .flat_map(|face| {
                        face.loops.iter().flat_map(move |l| {
                            l.member_ids
                                .iter()
                                .zip(&l.edge_stride_distances)
                                .zip(&l.support_record_ordinals)
                                .filter_map(move |((&a, &q), &support)| {
                                    (l.record_ordinal.checked_sub(q) == Some(edge_ordinal)
                                        && a.checked_sub(q) == Some(base))
                                    .then_some((face.record_ordinal, support))
                                })
                        })
                    })
                    .collect::<Vec<_>>();
                let [(face, support)] = matches.as_slice() else {
                    return None;
                };
                if zero_entity_record(records, *face)?.id != side.face_record
                    || zero_entity_record(records, *support)?.id != side.support_record
                    || !selected_supports.insert(*support)
                {
                    return None;
                }
            }
            if vertices[0] == vertices[1] {
                return None;
            }
        }
        let support_count: usize = runs.iter().map(|r| r.supports.len()).sum();
        let incidence_count: usize = incidences.iter().map(|i| i.allocations.len()).sum();
        if selected_supports.len() != support_count || selected_uses.len() != incidence_count {
            return None;
        }
        Some(())
    })()
    .is_some();
    if valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity allocation graph disagrees with its source records".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests;
