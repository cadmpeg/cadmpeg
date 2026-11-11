// SPDX-License-Identifier: Apache-2.0
//! Topology tables read from the node sequence of a partition or plain stream
//! (`parasolid.md` §2).

use super::{
    merge_record_frame, parse_sense, Bridge, EdgeReferences, EdgeUse, Fin, Loop, Point,
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
                t.merge_bridge_frame(record, frame());
            }
            LOOP => {
                let record = loop_record(&node, offset)?;
                t.loop_frames.insert(record.attr, frame());
                t.loops.insert(record.attr, record);
            }
            EDGE => {
                let record = edge_use(&node, offset)?;
                if stored_tolerance_frames(record.tolerance_m) {
                    t.edge_uses.insert(record.attr, record);
                } else {
                    t.reject_edge(record.attr);
                }
            }
            FIN => {
                let record = fin(&node, offset)?;
                let attr = record.attr();
                t.coedge_frames.insert(attr, frame());
                t.coedge_owners
                    .insert(attr, HashSet::from([record.owner()]));
                match record {
                    Fin::Edge(record) => {
                        t.coedges.insert(attr, record);
                    }
                    Fin::Vertex(record) => {
                        t.vertex_loop_fins.insert(attr, record);
                    }
                }
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

/// The node instances of a Deltas stream with one instance per identity: the
/// first version of an identity whose saved versions agree across the site.
pub(in crate::brep) fn consistent_deltas_nodes<'r>(
    records: &'r DeltasRecords<'_>,
    agreement: &DeltasAgreement,
) -> Vec<DeltasNode<'r>> {
    let mut seen = HashSet::new();
    records
        .nodes()
        .filter(|node| {
            agreement.agrees(node.kind(), node.index()) && seen.insert((node.kind(), node.index()))
        })
        .collect()
}

/// Whether an edge tolerance frames as the scanned edge forms do: the null
/// real, zero, or a stored value inside the stored-tolerance bounds.
fn stored_tolerance_frames(tolerance_m: Option<f64>) -> bool {
    tolerance_m.is_none_or(|value| {
        value == 0.0
            || (super::STORED_TOLERANCE_MIN_M..=super::STORED_TOLERANCE_MAX_M).contains(&value)
    })
}

/// The identities whose saved versions disagree across the Deltas streams of
/// one site.
#[derive(Default)]
pub(in crate::brep) struct DeltasAgreement {
    conflicted: HashSet<(u16, u32)>,
}

impl DeltasAgreement {
    /// The agreement over `streams`, each a Deltas stream and its payload.
    pub(in crate::brep) fn new<'a>(
        streams: impl IntoIterator<Item = (&'a DeltasRecords<'a>, &'a [u8])>,
    ) -> Self {
        let mut first = HashMap::<(u16, u32), &[u8]>::new();
        let mut conflicted = HashSet::new();
        for (records, payload) in streams {
            for node in records.nodes() {
                let data = payload.get(node.data_range()).unwrap_or_default();
                let identity = (node.kind(), node.index());
                if *first.entry(identity).or_insert(data) != data {
                    conflicted.insert(identity);
                }
            }
        }
        Self { conflicted }
    }

    /// The node indices of the identities whose versions disagree.
    pub(in crate::brep) fn conflicted_indices(&self) -> HashSet<u32> {
        self.conflicted.iter().map(|(_, index)| *index).collect()
    }

    /// Whether every saved version of the identity carries equal data.
    pub(in crate::brep) fn agrees(&self, kind: u16, index: u32) -> bool {
        !self.conflicted.contains(&(kind, index))
    }
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
    agreement: &DeltasAgreement,
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
                t.merge_bridge_frame(record, frame);
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
                if stored_tolerance_frames(record.tolerance_m) {
                    edges.entry(record.attr).or_default().push(record);
                }
            }
            FIN => {
                let record = fin(&node, offset)?;
                let owners = HashSet::from([record.owner()]);
                t.merge_fin_frame(record.attr(), frame, Some(record), owners);
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
        if !agreement.agrees(EDGE, attr) {
            continue;
        }
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
    t.bridge_frames
        .retain(|attr, frame| bridges.contains_key(attr) || frame.bytes.is_none());
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

fn fin(node: &impl NodeFields, offset: usize) -> Option<Fin> {
    let refs = node.pointers([
        "attributes_features",
        "loop",
        "forward",
        "backward",
        "vertex",
        "other",
        "edge",
        "curve",
        "next_at_vx",
    ])?;
    Fin::read(node.index(), refs, node.char("sense")?, offset)
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
