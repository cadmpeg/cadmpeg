// SPDX-License-Identifier: Apache-2.0

use super::{
    candidate_has_valid_family_framing, FixedRecordSelection, Graph, NodeCandidate, NodeKind,
};
use cadmpeg_core::CodecError;
use cadmpeg_parasolid::records::primary::PrimaryRecord;
use cadmpeg_parasolid::records::{EdgeFrame, PrimaryRecords};
use cadmpeg_parasolid::xmt::NonNullXmt;
use std::ops::Range;

/// Primary framing of one stream, which outranks the fixed-record scan.
pub(super) struct PrimaryFrames {
    pub(super) edges: Vec<EdgeFrame>,
    records: Vec<Range<usize>>,
    /// Whether a terminator ends the node sequence, so every node of the
    /// stream is in `records`.
    terminated: bool,
}

impl PrimaryFrames {
    /// The node ranges of a terminated sequence.
    pub(super) fn terminated_records(&self) -> Option<&[Range<usize>]> {
        self.terminated.then_some(self.records.as_slice())
    }

    /// Whether primary framing assigns a candidate's bytes to another node.
    pub(super) fn exclude(&self, candidate: &NodeCandidate) -> bool {
        let index = self
            .records
            .partition_point(|record| record.end <= candidate.pos);
        let inside = self
            .records
            .get(index)
            .is_some_and(|record| record.start < candidate.pos);
        inside || self.overlaps_edge(candidate)
    }

    fn overlaps_edge(&self, candidate: &NodeCandidate) -> bool {
        let index = self
            .edges
            .partition_point(|frame| frame.range.end <= candidate.pos);
        self.edges
            .get(index)
            .is_some_and(|frame| frame.range.start < candidate.end)
    }
}

impl Graph {
    /// Use primary record ownership when the header is supported.
    pub(crate) fn parse_source(stream: &[u8]) -> Result<Self, CodecError> {
        let (frames, stop, limit_term_use, selection) = {
            let Some(primary) = PrimaryRecords::read(stream)? else {
                return Ok(Self::parse(stream));
            };
            let limit_term_use = crate::intersection::limit_term_use(&primary);
            let frames = PrimaryFrames {
                edges: primary.edge_frames(),
                records: primary.records().iter().map(PrimaryRecord::range).collect(),
                terminated: primary.stop_offset().is_none(),
            };
            let selection = frames.terminated.then(|| node_selection(stream, &primary));
            (frames, primary.stop_offset(), limit_term_use, selection)
        };
        let mut graph = match selection {
            Some((selection, supported)) => {
                let mut graph = selection.materialize(stream);
                graph.primary_node_refusals = supported
                    .into_iter()
                    .filter(|&(kind, start)| {
                        graph
                            .by_pos
                            .get(&start)
                            .is_none_or(|(selected, _)| *selected != kind)
                    })
                    .collect();
                let refusals = frames
                    .edges
                    .iter()
                    .map(|frame| frame.range.start)
                    .filter(|start| {
                        graph
                            .by_pos
                            .get(start)
                            .is_none_or(|(kind, _)| *kind != NodeKind::Edge)
                    })
                    .collect();
                graph.primary_edge_refusals = refusals;
                graph
            }
            None => {
                let mut graph = Self::parse_fixed(stream, Some(&frames));
                graph.primary_edge_refusals = frames
                    .edges
                    .iter()
                    .filter(|frame| Self::primary_edge_candidate(stream, frame).is_none())
                    .map(|frame| frame.range.start)
                    .collect();
                graph
            }
        };
        graph.primary_unread_from = stop.filter(|offset| *offset < stream.len());
        graph.primary_records = frames.terminated_records().map(<[_]>::to_vec);
        graph.primary_limit_term_use = limit_term_use;
        Ok(graph)
    }

    /// Where the stream's auxiliary records start: the node starts of a
    /// terminated primary sequence, else every tag offset.
    pub(crate) fn record_starts(&self) -> crate::intersection::RecordStarts<'_> {
        self.primary_records
            .as_deref()
            .map_or(crate::intersection::RecordStarts::Scan, |records| {
                crate::intersection::RecordStarts::Nodes {
                    records,
                    limit_term_use: self.primary_limit_term_use,
                }
            })
    }

    pub(crate) fn primary_edge_refusals(&self) -> &[usize] {
        &self.primary_edge_refusals
    }

    pub(crate) fn primary_node_refusals(&self) -> &[(NodeKind, usize)] {
        &self.primary_node_refusals
    }

    pub(crate) fn primary_unread_from(&self) -> Option<usize> {
        self.primary_unread_from
    }

    pub(super) fn primary_edge_candidate(
        stream: &[u8],
        frame: &EdgeFrame,
    ) -> Option<NodeCandidate> {
        let shift = frame
            .fixed_fields_at?
            .checked_sub(frame.range.start.checked_add(4)?)?;
        candidate_has_valid_family_framing(
            stream,
            frame.range.start,
            NodeKind::Edge,
            frame.xmt,
            shift,
            frame.range.end,
            true,
        )?;
        Some(NodeCandidate {
            kind: NodeKind::Edge,
            xmt: frame.xmt,
            pos: frame.range.start,
            shift,
            end: frame.range.end,
        })
    }
}

/// The fixed records of a terminated node sequence: one candidate per
/// supported node with the base field layout, with an identity that two nodes
/// share left out before the family checks. The kind and start of every
/// supported non-EDGE node follow the selection.
fn node_selection(
    stream: &[u8],
    primary: &PrimaryRecords<'_>,
) -> (FixedRecordSelection, Vec<(NodeKind, usize)>) {
    let mut typed = Vec::new();
    let mut ownership = Vec::new();
    let mut supported = Vec::new();
    for node in primary.nodes() {
        let Some(kind) = u8::try_from(node.kind())
            .ok()
            .and_then(|code| NodeKind::try_from(code).ok())
        else {
            continue;
        };
        let range = node.range();
        if kind != NodeKind::Edge {
            supported.push((kind, range.start));
        }
        if !node.has_base_layout() {
            continue;
        }
        let Some(shift) = node
            .fields_start()
            .zip(range.start.checked_add(4))
            .and_then(|(first, fields)| first.checked_sub(fields))
        else {
            continue;
        };
        let Ok(xmt) = NonNullXmt::try_from(node.index()) else {
            continue;
        };
        let candidate = NodeCandidate {
            kind,
            xmt,
            pos: range.start,
            shift,
            end: range.end,
        };
        if matches!(kind, NodeKind::Body | NodeKind::Region) {
            ownership.push(candidate);
        } else {
            typed.push(candidate);
        }
    }
    let valid = |candidates: Vec<NodeCandidate>| {
        Graph::select_unique_candidates(candidates)
            .into_iter()
            .filter(|candidate| {
                candidate_has_valid_family_framing(
                    stream,
                    candidate.pos,
                    candidate.kind,
                    candidate.xmt,
                    candidate.shift,
                    candidate.end,
                    true,
                )
                .is_some()
            })
            .collect()
    };
    let selection = FixedRecordSelection {
        typed: valid(typed),
        ownership: valid(ownership),
    };
    (selection, supported)
}
