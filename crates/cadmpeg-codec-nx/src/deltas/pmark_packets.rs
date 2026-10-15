// SPDX-License-Identifier: Apache-2.0
//! PMARK packet fields and partition-owned main-line admission.

use super::partition_prefix::PartitionPrefixState;
use super::pmark_nodes::PmarkNodes;
use super::pmark_state::PmarkState;
use super::PartitionPrefix;
use cadmpeg_core::decode::{DecodeContext, View};
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
pub(super) fn at(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
    prefix: &PartitionPrefix,
) -> Result<Option<PmarkPacket>, CodecError> {
    let Some(head_end) = offset.checked_add(6) else {
        return Ok(None);
    };
    if stream.get(offset..head_end) != Some(&[0, 1, 0, 1, 0, 4]) {
        return Ok(None);
    }
    let highest_id = prefix.state.highest_id();
    let mut at = offset + 4;
    let Some((first, end)) = node_at(stream, at) else {
        return Ok(None);
    };
    // A syntactically complete candidate can fail its independently parsed
    // partition context. This bound does not discover its byte boundary.
    if first.identifier() > highest_id {
        return Ok(None);
    }
    let mut nodes = PmarkNodes::new(ctx, first)?;
    at = end;
    while View::u16_be_at(stream, at) == Some(4) {
        let Some((node, end)) = node_at(stream, at) else {
            return Ok(None);
        };
        ctx.charge_work(1, "read NX PMARK packet nodes")?;
        if node.identifier() > highest_id {
            return Ok(None);
        }
        nodes.push(ctx, node)?;
        at = end;
    }
    let terminal = terminal_end(stream, at);
    Ok(Some(PmarkPacket {
        nodes,
        terminal: terminal.is_some(),
        offset,
        end: terminal.unwrap_or(at),
    }))
}

fn node_at(stream: &[u8], offset: usize) -> Option<(PmarkState, usize)> {
    (View::u16_be_at(stream, offset) == Some(4)).then_some(())?;
    PmarkState::read(stream, offset.checked_add(2)?)
}

fn terminal_end(stream: &[u8], offset: usize) -> Option<usize> {
    let mut view = View::over_retained(stream);
    view.seek(offset)?;
    for _ in 0..3 {
        (view.u16_be()? == 1).then_some(())?;
    }
    (view.u32_be()? == 1).then_some(view.position())
}

pub(super) fn admit(
    ctx: &DecodeContext<'_>,
    packets: &[PmarkPacket],
    prefix: PartitionPrefixState,
) -> Result<bool, CodecError> {
    let current = prefix.current_pmark();
    let mut index_guard = ctx.reserve_scoped(0, "NX PMARK graph nodes")?;
    let mut by_identity = BTreeMap::new();
    let mut root = None;
    for node in packets.iter().flat_map(|packet| packet.nodes.as_slice()) {
        ctx.charge_work(1, "index NX PMARK graph")?;
        if node.xmt() == current.xmt()
            || node.xmt() == prefix.partition_xmt()
            || by_identity.contains_key(&node.xmt())
        {
            return Ok(false);
        }
        index_guard.with_storage(|| {
            ctx.insert_btree_map(&mut by_identity, node.xmt(), node, "NX PMARK graph nodes")
        })?;
        if node.preceding() == 1 {
            if root.is_some() {
                return Ok(false);
            }
            root = Some(node);
        }
    }
    if by_identity.is_empty() {
        return Ok(current.preceding() == 1);
    }
    let Some(mut node) = root else {
        return Ok(false);
    };
    for visited in 1..=by_identity.len() {
        ctx.charge_work(1, "validate NX PMARK main line")?;
        let target = node.first_following();
        if target == current.xmt() {
            return Ok(visited == by_identity.len() && current.preceding() == node.xmt());
        }
        let Some(&next) = by_identity.get(&target) else {
            return Ok(false);
        };
        if next.preceding() != node.xmt() {
            return Ok(false);
        }
        node = next;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use crate::deltas::census::walk;
    use crate::test_support::test_pmarks;

    fn packets(bytes: &[u8]) -> Vec<super::PmarkPacket> {
        crate::test_support::with_decode_context(|ctx| walk(ctx, bytes))
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
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
        let covered =
            crate::test_support::with_decode_context(|ctx| census.covered_spans(ctx)).unwrap();
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
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
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
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
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
