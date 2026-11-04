// SPDX-License-Identifier: Apache-2.0

use super::{candidate_has_valid_family_framing, Graph, NodeCandidate, NodeKind};
use cadmpeg_core::CodecError;
use cadmpeg_parasolid::records::primary::PrimaryRecord;
use cadmpeg_parasolid::records::{EdgeFrame, PrimaryRecords};
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
        let (frames, stop, limit_term_use) = {
            let Some(primary) = PrimaryRecords::read(stream)? else {
                return Ok(Self::parse(stream));
            };
            let limit_term_use = crate::intersection::limit_term_use(&primary);
            let frames = PrimaryFrames {
                edges: primary.edge_frames(),
                records: primary.records().iter().map(PrimaryRecord::range).collect(),
                terminated: primary.stop_offset().is_none(),
            };
            (frames, primary.stop_offset(), limit_term_use)
        };
        let mut graph = Self::parse_fixed(stream, Some(&frames));
        graph.primary_edge_refusals = frames
            .edges
            .iter()
            .filter(|frame| Self::primary_edge_candidate(stream, frame).is_none())
            .map(|frame| frame.range.start)
            .collect();
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
