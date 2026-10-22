// SPDX-License-Identifier: Apache-2.0
//! PMARK packet fields and partition-owned main-line admission.

use super::partition_prefix::PartitionPrefixState;
use super::pmark_nodes::PmarkNodes;
use super::pmark_state::PmarkState;
use super::PartitionPrefix;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

/// An enclosing packet and its separately keyed PMARK nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PmarkPacket {
    pub(crate) nodes: PmarkNodes,
    pub(crate) terminal: bool,
    pub(crate) offset: usize,
    pub(crate) end: usize,
}

/// Read a complete packet at the walk cursor. An earlier complete record has
/// already consumed its payload, so a nested packet cannot preempt that owner.
pub(super) fn at(stream: &[u8], offset: usize, prefix: &PartitionPrefix) -> Option<PmarkPacket> {
    let view = View::over_retained(stream).child(offset, stream.len())?;
    let mut nodes = None;
    let parsed = read_fields(view, prefix.state.highest_id(), |node| {
        match &mut nodes {
            None => nodes = Some(PmarkNodes::new(node)),
            Some(nodes) => nodes.push(node),
        }
        Ok::<(), std::convert::Infallible>(())
    });
    let (terminal, end) = match parsed {
        Ok(parsed) => parsed?,
        Err(never) => match never {},
    };
    Some(PmarkPacket {
        nodes: nodes?,
        terminal,
        offset,
        end,
    })
}

pub(crate) fn read_source(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    highest_id: u32,
) -> Result<PmarkPacket, CodecError> {
    let capacity = view.remaining() / (2 + 5 * 2 + 21);
    let mut nodes: Option<Vec<PmarkState>> = None;
    let mut count = 0_usize;
    let parsed = read_fields(view, highest_id, |node| {
        if nodes.is_none() {
            let bytes = capacity
                .checked_mul(size_of::<PmarkState>())
                .ok_or_else(|| CodecError::malformed("NX PMARK node storage overflows"))?;
            ctx.retain_bytes(u64_from_index(bytes), "NX PMARK packet nodes")?;
            nodes = Some(ctx.alloc_filled(capacity, node, "NX PMARK packet nodes")?);
        }
        let slot = nodes
            .as_mut()
            .and_then(|nodes| nodes.get_mut(count))
            .ok_or_else(|| CodecError::malformed("NX PMARK node count exceeds its source bound"))?;
        *slot = node;
        count += 1;
        Ok::<(), CodecError>(())
    })?
    .ok_or_else(|| CodecError::malformed("NX PMARK packet is unreadable"))?;
    let mut nodes = nodes.ok_or_else(|| CodecError::malformed("NX PMARK packet has no nodes"))?;
    nodes.truncate(count);
    Ok(PmarkPacket {
        nodes: PmarkNodes::try_from(nodes).map_err(CodecError::malformed)?,
        terminal: parsed.0,
        offset: view.position(),
        end: parsed.1,
    })
}

fn read_fields<E>(
    mut view: View<'_>,
    highest_id: u32,
    mut retain: impl FnMut(PmarkState) -> Result<(), E>,
) -> Result<Option<(bool, usize)>, E> {
    if !view.unread().starts_with(&[0, 1, 0, 1, 0, 4]) || view.skip(4).is_none() {
        return Ok(None);
    }
    loop {
        let Some((node, end)) = node_at(view) else {
            return Ok(None);
        };
        if node.identifier() > highest_id {
            return Ok(None);
        }
        retain(node)?;
        if view.seek(end).is_none() {
            return Ok(None);
        }
        if !view.unread().starts_with(&[0, 4]) {
            break;
        }
    }
    let terminal = terminal_end(view);
    Ok(Some((
        terminal.is_some(),
        terminal.unwrap_or(view.position()),
    )))
}

fn node_at(mut view: View<'_>) -> Option<(PmarkState, usize)> {
    view.unread().starts_with(&[0, 4]).then_some(())?;
    view.skip(2)?;
    PmarkState::read_view(view)
}

fn terminal_end(view: View<'_>) -> Option<usize> {
    view.unread()
        .starts_with(&[0, 1, 0, 1, 0, 1, 0, 0, 0, 1])
        .then_some(())?;
    view.position().checked_add(10)
}

pub(super) fn admit(packets: &[PmarkPacket], prefix: PartitionPrefixState) -> bool {
    let current = prefix.current_pmark();

    let mut by_identity = BTreeMap::new();
    let mut root = None;
    for node in packets.iter().flat_map(|packet| packet.nodes.as_slice()) {
        if node.xmt() == current.xmt()
            || node.xmt() == prefix.partition_xmt()
            || by_identity.contains_key(&node.xmt())
        {
            return false;
        }
        by_identity.insert(node.xmt(), node);
        if node.preceding() == 1 {
            if root.is_some() {
                return false;
            }
            root = Some(node);
        }
    }
    if by_identity.is_empty() {
        return current.preceding() == 1;
    }
    let Some(mut node) = root else {
        return false;
    };
    for visited in 1..=by_identity.len() {
        let target = node.first_following();
        if target == current.xmt() {
            return visited == by_identity.len() && current.preceding() == node.xmt();
        }
        let Some(&next) = by_identity.get(&target) else {
            return false;
        };
        if next.preceding() != node.xmt() {
            return false;
        }
        node = next;
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::deltas::census::walk;
    use crate::test_support::test_pmarks;

    fn packets(bytes: &[u8]) -> Vec<super::PmarkPacket> {
        crate::test_support::with_decode_context(|_ctx| walk(bytes))
            .unwrap()
            .pmark_packets
            .clone()
    }

    #[test]
    fn compound_packet_keeps_two_primary_nodes_and_the_terminal() {
        let mut bytes = test_pmarks::prefix(40_001);
        bytes.extend([0, 1, 0, 1]);
        bytes.extend(test_pmarks::node(40_000, 1, 40_001, 1));
        bytes.extend(test_pmarks::node(40_001, 40_000, 49_948, 2));
        bytes.extend([0, 1, 0, 1, 0, 1, 0, 0, 0, 1]);
        let result = packets(&bytes);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].nodes.as_slice().len(), 2);
        assert!(result[0].terminal);
        assert_eq!(result[0].end, bytes.len());
        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();
        let covered = crate::test_support::with_decode_context(|_ctx| census.covered_spans());
        assert_eq!(covered, [(0, bytes.len())]);
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn counted_integer_storage_owns_nested_packet_bytes() {
        let mut bytes = test_pmarks::prefix(40_000);
        let start = bytes.len();
        let mut payload = test_pmarks::packet();
        while !payload.len().is_multiple_of(4) {
            payload.push(0x7e);
        }
        bytes.extend_from_slice(&82_u16.to_be_bytes());
        bytes.extend_from_slice(&u32::try_from(payload.len() / 4).unwrap().to_be_bytes());
        bytes.extend_from_slice(&7_u16.to_be_bytes());
        bytes.extend(payload);
        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();
        assert!(census.pmark_packets.is_empty());
        assert_eq!(census.records.len(), 1);
        assert_eq!(
            (census.records[0].offset, census.records[0].end),
            (start, bytes.len())
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn following_type_four_requires_complete_valid_storage() {
        for suffix in [
            vec![0, 4],
            test_pmarks::node(40_001, 40_000, 49_948, 2)[..20].to_vec(),
        ] {
            let mut bytes = test_pmarks::prefix(40_000);
            bytes.extend(test_pmarks::packet());
            bytes.extend(suffix);
            assert!(packets(&bytes).is_empty());
        }
    }

    #[test]
    fn identifier_is_not_a_null_reference_trailer() {
        let mut bytes = test_pmarks::prefix(40_000);
        bytes.extend([0, 1, 0, 1]);
        bytes.extend(test_pmarks::node(40_000, 1, 49_948, 0x0001_0001));
        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();
        assert_eq!(census.pmark_packets.len(), 1);
        assert!(census.terminal_null_references.is_none());
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn duplicate_ownership_missing_links_and_cycles_abstain_atomically() {
        for nodes in [
            vec![(40_000, 1, 40_001), (40_000, 40_000, 49_948)],
            vec![(40_000, 1, 40_001)],
            vec![(40_000, 1, 40_001), (40_001, 40_000, 40_000)],
            vec![(40_000, 40_001, 40_001), (40_001, 40_000, 49_948)],
            vec![(40_000, 1, 40_001), (40_001, 1, 49_948)],
            vec![(49_947, 1, 40_001), (40_001, 49_947, 49_948)],
        ] {
            let mut bytes = test_pmarks::prefix(40_001);
            for (xmt, preceding, following) in nodes {
                bytes.extend([0, 1, 0, 1]);
                bytes.extend(test_pmarks::node(xmt, preceding, following, 1));
            }
            assert!(packets(&bytes).is_empty());
        }
        assert!(packets(&test_pmarks::prefix(40_001)).is_empty());
    }
}

#[cfg(test)]
mod source_tests;
