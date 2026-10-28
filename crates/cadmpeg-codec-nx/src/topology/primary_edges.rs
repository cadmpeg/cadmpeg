// SPDX-License-Identifier: Apache-2.0

use super::{candidate_has_valid_family_framing, Graph, NodeCandidate, NodeKind};
use crate::records::{EdgeFrame, PrimaryRecords};
use cadmpeg_core::CodecError;

impl Graph {
    /// Use primary field ownership for EDGE records when the header is supported.
    pub(crate) fn parse_source(stream: &[u8]) -> Result<Self, CodecError> {
        let frames = {
            let Some(primary) = PrimaryRecords::read(stream)? else {
                return Ok(Self::parse(stream));
            };
            primary.edge_frames()
        };
        let mut graph = Self::parse_fixed(stream, Some(&frames));
        graph.primary_edge_refusals = frames
            .iter()
            .filter(|frame| Self::primary_edge_candidate(stream, frame).is_none())
            .map(|frame| frame.range.start)
            .collect();
        Ok(graph)
    }

    pub(crate) fn primary_edge_refusals(&self) -> &[usize] {
        &self.primary_edge_refusals
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
