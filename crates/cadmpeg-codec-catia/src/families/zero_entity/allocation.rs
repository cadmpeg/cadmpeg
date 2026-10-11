// SPDX-License-Identifier: Apache-2.0
//! Complete edge/use/support/vertex identities from backward allocation distances.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use super::records::{self, ZeroEntityRecordIdentity, ZeroEntitySupportRun};

/// One side of a completely closed physical-edge allocation graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AllocationUse {
    pub(crate) use_pos: usize,
    pub(crate) use_record_ordinal: u32,
    pub(crate) support_record_ordinal: u32,
    pub(crate) face_record_ordinal: u32,
    pub(crate) incidence_record_ordinal: u32,
    pub(crate) vertex_record_ordinal: u32,
}

/// A persistent physical edge and its ordered side-one/side-two incidences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AllocationEdge {
    pub(crate) pos: usize,
    pub(crate) record_ordinal: u32,
    pub(crate) header_record_ordinal: u32,
    pub(crate) uses: [AllocationUse; 2],
}

/// A persistent vertex owner at its source byte position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AllocationVertex {
    pub(crate) record_ordinal: u32,
    pub(crate) pos: usize,
}

/// Complete source identity graph, independent of geometric coincidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AllocationTopology {
    pub(crate) edges: Vec<AllocationEdge>,
    pub(crate) vertices: Vec<AllocationVertex>,
}

#[derive(Clone, Copy)]
struct Member {
    support: u32,
    face: u32,
}

fn record(records: &[ZeroEntityRecordIdentity], ordinal: u32) -> Option<&ZeroEntityRecordIdentity> {
    records
        .get(usize::try_from(ordinal.checked_sub(1)?).ok()?)
        .filter(|r| r.record_ordinal == ordinal)
}

/// Admit only a complete bijection between loop members, side uses, and incidences.
pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    runs: &[ZeroEntitySupportRun],
) -> Result<Option<AllocationTopology>, CodecError> {
    macro_rules! exact {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let strides = records::zero_entity_edge_strides_in_range(ctx, data, range.clone())?;
    if strides.is_empty() {
        return Ok(None);
    }
    let inventory = records::zero_entity_record_inventory_in_range(ctx, data, range.clone())?;
    let pairs = records::zero_entity_oriented_use_pairs_in_range(ctx, data, range.clone())?;
    let incidences = records::zero_entity_vertex_incidences_in_range(ctx, data, range)?;
    if pairs.len() != strides.len() || incidences.is_empty() {
        return Ok(None);
    }
    let pair_by_header = ctx.collect_hash_map(
        pairs.iter().map(|p| (p.header_record_ordinal, p)),
        "catia_zero_allocation_pair_index",
    )?;
    let mut incidence_by_use = HashMap::new();
    let mut vertices = Vec::new();
    for incidence in &incidences {
        let incidence_ordinal = incidence.record_ordinal;
        let source = exact!(record(&inventory, incidence_ordinal));
        let owner = exact!(record(&inventory, exact!(incidence_ordinal.checked_add(1))));
        if source.pos != incidence.pos || source.end != owner.pos || owner.tag != [0x5d, 0x06] {
            return Ok(None);
        }
        ctx.push_vec(
            &mut vertices,
            AllocationVertex {
                record_ordinal: owner.record_ordinal,
                pos: owner.pos,
            },
            "catia_zero_allocation_vertices",
        )?;
        for &distance in incidence.allocations.as_slice() {
            ctx.charge_work(1, "catia_zero_allocation_incidence_binding")?;
            let use_ordinal = exact!(incidence_ordinal.checked_sub(distance));
            if exact!(record(&inventory, use_ordinal)).tag != [0x06, 0x38]
                || ctx
                    .insert_hash_map(
                        &mut incidence_by_use,
                        use_ordinal,
                        (incidence_ordinal, owner.record_ordinal),
                        "catia_zero_allocation_incidence_index",
                    )?
                    .is_some()
            {
                return Ok(None);
            }
        }
    }
    let mut members = HashMap::new();
    for run in runs {
        let face = exact!(run.face.as_ref());
        let loops = exact!(face.loops.as_ref());
        for loop_record in loops {
            let count = loop_record.members.member_ids().count();
            if count != loop_record.edge_stride_distances.len()
                || count != loop_record.support_record_ordinals.len()
            {
                return Ok(None);
            }
            for ((member_ordinal, &edge_distance), &support) in loop_record
                .members
                .member_ids()
                .zip(&loop_record.edge_stride_distances)
                .zip(&loop_record.support_record_ordinals)
            {
                ctx.charge_work(1, "catia_zero_allocation_member_binding")?;
                let edge = exact!(loop_record.record_ordinal.checked_sub(edge_distance));
                if exact!(record(&inventory, edge)).tag != [0x5e, 0x1a] {
                    return Ok(None);
                }
                let key = exact!(member_ordinal.checked_sub(edge_distance));
                if ctx
                    .insert_hash_map(
                        &mut members,
                        (edge, key),
                        Member {
                            support,
                            face: face.record_ordinal,
                        },
                        "catia_zero_allocation_member_index",
                    )?
                    .is_some()
                {
                    return Ok(None);
                }
            }
        }
    }
    let expected_uses = exact!(strides.len().checked_mul(2));
    if members.len() != expected_uses || incidence_by_use.len() != expected_uses {
        return Ok(None);
    }
    let mut edges = Vec::new();
    let mut selected_supports = HashSet::new();
    let mut selected_headers = HashSet::new();
    for stride in strides {
        ctx.charge_work(1, "catia_zero_allocation_edge_binding")?;
        let edge_ordinal = stride.record_ordinal;
        let source = exact!(record(&inventory, edge_ordinal));
        let [header_distance, first_vertex_distance, second_vertex_distance, _, _] =
            stride.allocations;
        let header_ordinal = exact!(edge_ordinal.checked_sub(header_distance));
        let pair = exact!(pair_by_header.get(&header_ordinal));
        if !ctx.insert_hash_set(
            &mut selected_headers,
            header_ordinal,
            "catia_zero_allocation_selected_headers",
        )? {
            return Ok(None);
        }
        let header = exact!(record(&inventory, header_ordinal));
        if source.pos != stride.pos || header.tag != [0x25, 0x69] || header.pos != pair.header_pos {
            return Ok(None);
        }
        let mut bound = [None, None];
        for (index, (side, owner_distance)) in [1_u32, 2]
            .into_iter()
            .zip([first_vertex_distance, second_vertex_distance])
            .enumerate()
        {
            let use_ordinal = exact!(header_ordinal.checked_add(side));
            let use_record = exact!(record(&inventory, use_ordinal));
            let previous = exact!(record(&inventory, exact!(use_ordinal.checked_sub(1))));
            if pair.uses[index].record_ordinal != use_ordinal
                || pair.uses[index].pos != use_record.pos
                || previous.end != use_record.pos
            {
                return Ok(None);
            }
            let &(incidence_ordinal, vertex_ordinal) = exact!(incidence_by_use.get(&use_ordinal));
            if edge_ordinal.checked_sub(owner_distance) != Some(vertex_ordinal)
                || incidence_ordinal.checked_add(1) != Some(vertex_ordinal)
            {
                return Ok(None);
            }
            let key = exact!(header_distance.checked_add(pair.base_columns()[index]));
            let member = exact!(members.get(&(edge_ordinal, key)));
            if !ctx.insert_hash_set(
                &mut selected_supports,
                member.support,
                "catia_zero_allocation_selected_supports",
            )? {
                return Ok(None);
            }
            bound[index] = Some(AllocationUse {
                use_pos: use_record.pos,
                use_record_ordinal: use_ordinal,
                support_record_ordinal: member.support,
                face_record_ordinal: member.face,
                incidence_record_ordinal: incidence_ordinal,
                vertex_record_ordinal: vertex_ordinal,
            });
        }
        let [Some(first), Some(second)] = bound else {
            return Ok(None);
        };
        if first.vertex_record_ordinal == second.vertex_record_ordinal {
            return Ok(None);
        }
        ctx.push_vec(
            &mut edges,
            AllocationEdge {
                pos: source.pos,
                record_ordinal: edge_ordinal,
                header_record_ordinal: header_ordinal,
                uses: [first, second],
            },
            "catia_zero_allocation_edges",
        )?;
    }
    ctx.charge_work(
        u64_from_index(selected_supports.len()),
        "catia_zero_allocation_complete_roster",
    )?;
    if selected_supports.len() != expected_uses {
        return Ok(None);
    }
    Ok(Some(AllocationTopology { edges, vertices }))
}

#[cfg(test)]
mod tests;
