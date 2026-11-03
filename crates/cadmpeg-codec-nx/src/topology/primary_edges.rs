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
}

impl PrimaryFrames {
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
        let (frames, stop) = {
            let Some(primary) = PrimaryRecords::read(stream)? else {
                return Ok(Self::parse(stream));
            };
            let frames = PrimaryFrames {
                edges: primary.edge_frames(),
                records: primary.records().iter().map(PrimaryRecord::range).collect(),
            };
            (frames, primary.stop_offset())
        };
        let mut graph = Self::parse_fixed(stream, Some(&frames));
        graph.primary_edge_refusals = frames
            .edges
            .iter()
            .filter(|frame| Self::primary_edge_candidate(stream, frame).is_none())
            .map(|frame| frame.range.start)
            .collect();
        graph.primary_unread_from = stop.filter(|offset| *offset < stream.len());
        Ok(graph)
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
