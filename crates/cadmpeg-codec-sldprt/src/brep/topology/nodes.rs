// SPDX-License-Identifier: Apache-2.0
//! Topology tables read from the node sequence of a partition or plain stream
//! (`parasolid.md` §2).

use super::{
    merge_record_frame, parse_sense, Bridge, Coedge, EdgeReferences, EdgeUse, Loop, Point,
    RecordFrame, Tables, VertexUse,
};
use cadmpeg_parasolid::records::deltas::node::DeltasNode;
use cadmpeg_parasolid::records::node_type::{EDGE, FACE, FIN, LOOP, POINT, VERTEX};
use cadmpeg_parasolid::records::NodeFields;
use cadmpeg_parasolid::records::{DeltasRecords, PrimaryRecords};
use std::collections::{HashMap, HashSet};

/// The node sequence of `payload`, a complete transmit stream, when it
/// reaches its terminator.
pub(in crate::brep) fn sequence(payload: &[u8]) -> Option<PrimaryRecords<'_>> {
    let records = PrimaryRecords::read(payload).ok()??;
    records.is_terminated().then_some(records)
}

/// The topology tables of a node sequence over `payload` whose body starts at
/// `body_offset`, when every topology node carries its base fields and no two
/// topology nodes share an index. Record offsets are body-relative, like the
/// scanned tables.
pub(in crate::brep) fn tables(
    records: &PrimaryRecords<'_>,
    payload: &[u8],
    body_offset: usize,
) -> Option<Tables> {
    let mut t = Tables::default();
    let mut indices = HashSet::new();
    for node in records.nodes() {
        if !matches!(node.kind(), FACE | LOOP | EDGE | FIN | VERTEX | POINT) {
            continue;
        }
        indices.insert(node.index()).then_some(())?;
        let range = node.range();
        let offset = range.start.checked_sub(body_offset)?;
        let frame = || RecordFrame {
            partition: true,
            bytes: payload.get(range.clone()).map(<[u8]>::to_vec),
        };
        match node.kind() {
            FACE => {
                let record = bridge(&node, offset)?;
                t.bridge_frames.insert(record.attr, frame());
                t.bridges.insert(record.attr, record);
            }
            LOOP => {
                let record = loop_record(&node, offset)?;
                t.loop_frames.insert(record.attr, frame());
                t.loops.insert(record.attr, record);
            }
            EDGE => {
                let record = edge_use(&node, offset)?;
                t.edge_uses.insert(record.attr, record);
            }
            FIN => {
                let record = coedge(&node, offset)?;
                t.coedge_frames.insert(record.attr, frame());
                t.coedge_owners
                    .insert(record.attr, HashSet::from([record.refs[1]]));
                t.coedges.insert(record.attr, record);
            }
            VERTEX => {
                let record = vertex_use(&node, offset)?;
                t.vertex_frames.insert(record.attr, frame());
                t.vertex_classes.insert(record.attr, true);
                let supported = record.tolerance_m.is_none_or(|value| {
                    value > 0.0 && (value * super::super::LEN_TO_MM).is_finite()
                });
                if supported {
                    t.vertex_uses.insert(record.attr, record);
                }
            }
            _ => {
                let record = point(&node, offset, body_offset)?;
                t.point_frames.insert(record.attr, frame());
                t.points.insert(record.attr, record);
            }
        }
    }
    Some(t)
}

/// The node instances of a Deltas stream over `payload` with one instance
/// per identity: the first version of an identity whose saved versions all
/// carry equal data bytes. An identity with differing versions has none.
pub(in crate::brep) fn consistent_deltas_nodes<'r>(
    records: &'r DeltasRecords<'_>,
    payload: &[u8],
) -> Vec<DeltasNode<'r>> {
    let nodes: Vec<_> = records.nodes().collect();
    let mut versions = HashMap::<(u16, u32), Option<(usize, &[u8])>>::new();
    for (position, node) in nodes.iter().enumerate() {
        let data = payload.get(node.data_range()).unwrap_or_default();
        versions
            .entry((node.kind(), node.index()))
            .and_modify(|slot| {
                if slot.is_some_and(|(_, first)| first != data) {
                    *slot = None;
                }
            })
            .or_insert(Some((position, data)));
    }
    let keep: HashSet<usize> = versions
        .into_values()
        .flatten()
        .map(|(position, _)| position)
        .collect();
    nodes
        .into_iter()
        .enumerate()
        .filter_map(|(position, node)| keep.contains(&position).then_some(node))
        .collect()
}

/// The topology tables of a Deltas stream over `payload` whose body starts at
/// `body_offset`, read from its node instances. Each saved version of an
/// identity merges as a deltas frame of its data bytes: equal versions keep
/// the record, and differing versions leave the identity ambiguous. An edge
/// use is kept when all its versions are equal. `None` when a topology node
/// lacks a base field.
pub(in crate::brep) fn deltas_tables(
    records: &DeltasRecords<'_>,
    payload: &[u8],
    body_offset: usize,
) -> Option<Tables> {
    let mut t = Tables::default();
    let mut edges = HashMap::<u32, Vec<EdgeUse>>::new();
    for node in records.nodes() {
        if !matches!(node.kind(), FACE | LOOP | EDGE | FIN | VERTEX | POINT) {
            continue;
        }
        let offset = node.range().start.checked_sub(body_offset)?;
        let frame = RecordFrame {
            partition: false,
            bytes: Some(payload.get(node.data_range())?.to_vec()),
        };
        match node.kind() {
            FACE => {
                let record = bridge(&node, offset)?;
                merge_record_frame(
                    &mut t.bridges,
                    &mut t.bridge_frames,
                    record.attr,
                    frame,
                    Some(record),
                );
            }
            LOOP => {
                let record = loop_record(&node, offset)?;
                merge_record_frame(
                    &mut t.loops,
                    &mut t.loop_frames,
                    record.attr,
                    frame,
                    Some(record),
                );
            }
            EDGE => {
                let record = edge_use(&node, offset)?;
                edges.entry(record.attr).or_default().push(record);
            }
            FIN => {
                let record = coedge(&node, offset)?;
                let owners = HashSet::from([record.refs[1]]);
                t.merge_coedge_frame(record.attr, frame, Some(record), owners);
            }
            VERTEX => {
                let record = vertex_use(&node, offset)?;
                let supported = record.tolerance_m.is_none_or(|value| {
                    value > 0.0 && (value * super::super::LEN_TO_MM).is_finite()
                });
                t.merge_vertex_frame(record.attr, frame, supported.then_some(record));
            }
            _ => {
                let record = point(&node, offset, body_offset)?;
                t.merge_point_frame(record.attr, frame, Some(record));
            }
        }
    }
    for (attr, versions) in edges {
        let mut records = versions.into_iter();
        let Some(first) = records.next() else {
            continue;
        };
        let same = |record: &EdgeUse| {
            record.references == first.references
                && record.sequence == first.sequence
                && record.tolerance_m.map(f64::to_bits) == first.tolerance_m.map(f64::to_bits)
        };
        if records.all(|record| same(&record)) {
            t.edge_uses.insert(attr, first);
        }
    }
    Some(t)
}

/// Apply the typed ownership of the stream's document to its node tables: a
/// FACE without a loop at an offset in `excluded_bridge_offsets` is an
/// ownership-only node, and `owner_attrs` are the typed topology owners.
pub(in crate::brep) fn own(
    t: &mut Tables,
    excluded_bridge_offsets: &HashSet<usize>,
    owner_attrs: &HashSet<u32>,
) {
    t.bridges.retain(|_, bridge| {
        bridge.refs[2] > 1 || !excluded_bridge_offsets.contains(&bridge.offset)
    });
    let bridges = &t.bridges;
    t.bridge_frames.retain(|attr, _| bridges.contains_key(attr));
    t.owner_classes.extend(owner_attrs.iter().copied());
}

fn bridge(node: &impl NodeFields, offset: usize) -> Option<Bridge> {
    let attributes = node.pointer("attributes_features")?;
    Some(Bridge {
        attr: node.index(),
        refs: node.pointers(["next", "previous", "loop", "shell", "surface"])?,
        sequence: node.integer("node_id")?,
        sense: parse_sense(node.char("sense")?)?,
        attributes_head: (attributes > 1).then_some(attributes),
        offset,
    })
}

fn loop_record(node: &impl NodeFields, offset: usize) -> Option<Loop> {
    Some(Loop {
        attr: node.index(),
        refs: node.pointers(["attributes_features", "halfedge", "face", "next"])?,
        offset,
    })
}

fn edge_use(node: &impl NodeFields, offset: usize) -> Option<EdgeUse> {
    Some(EdgeUse {
        attr: node.index(),
        references: EdgeReferences::Bare(node.pointers([
            "halfedge",
            "previous",
            "next",
            "curve",
            "next_on_curve",
            "previous_on_curve",
        ])?),
        sequence: node.integer("node_id")?,
        tolerance_m: (!node.is_null("tolerance")?).then_some(node.real("tolerance")?),
        offset,
    })
}

fn coedge(node: &impl NodeFields, offset: usize) -> Option<Coedge> {
    Some(Coedge {
        attr: node.index(),
        refs: node.pointers([
            "attributes_features",
            "loop",
            "forward",
            "backward",
            "vertex",
            "other",
            "edge",
            "curve",
            "next_at_vx",
        ])?,
        sense: parse_sense(node.char("sense")?)?,
        offset,
    })
}

fn vertex_use(node: &impl NodeFields, offset: usize) -> Option<VertexUse> {
    Some(VertexUse {
        attr: node.index(),
        refs: node.pointers([
            "attributes_features",
            "halfedge",
            "previous",
            "next",
            "point",
        ])?,
        sequence: node.integer("node_id")?,
        owner: node.pointer("owner")?,
        tolerance_m: (!node.is_null("tolerance")?).then_some(node.real("tolerance")?),
        offset,
    })
}

fn point(node: &impl NodeFields, offset: usize, body_offset: usize) -> Option<Point> {
    let xyz_m = node.vector("pvec")?;
    xyz_m.iter().all(|value| value.is_finite()).then_some(())?;
    Some(Point {
        attr: node.index(),
        refs: node
            .pointers(["attributes_features", "owner", "next", "previous"])?
            .to_vec(),
        xyz_m,
        xyz_offset: node.field_range("pvec")?.start.checked_sub(body_offset)?,
        offset,
    })
}

#[cfg(test)]
mod tests;
